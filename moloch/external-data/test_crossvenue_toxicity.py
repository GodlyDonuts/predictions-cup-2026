import copy
from decimal import Decimal
import gzip
import hashlib
from pathlib import Path
import tempfile
import unittest
from crossvenue_toxicity import feature_state, report, HORIZONS
from crossvenue_economics import canonical
from crossvenue_observer import Study, stamp

BASE = 1792144000000


def records(n=1, step=40000):
    out = []
    for i in range(n):
        at = BASE+i*step
        j, e = hashlib.sha256(f'j{i}'.encode()).hexdigest(), hashlib.sha256(f'e{i}'.encode()).hexdigest()
        sid = hashlib.sha256(f'1:101:{j}:{e}:{at}'.encode()).hexdigest()
        features = {'bid': '0.490000', 'ask': '0.510000', 'bidQuantity': '10', 'askQuantity': '10'}
        out.append({'kind': 'decision_features', 'sampleId': sid, 'marketId': '1', 'exchangeId': '101',
                    'decisionMs': at, 'decisionAt': stamp(at),
                    'sig': {'sourceAt': stamp(at-1), 'receivedAt': stamp(at), 'journalHash': j, 'features': features},
                    'external': {'receivedMs': at, 'availableMs': at, 'bodySha256': e, 'mappingKind': 'direct_party_reference'},
                    'externalLogitChanges': {'1000': .003, '5000': .006, '30000': .02}})
        for h in HORIZONS:
            out.append({'kind': 'horizon_label', 'sampleId': sid, 'horizonMs': h,
                        'sourceAt': stamp(at+h), 'receivedAt': stamp(at+h), 'observedAt': stamp(at+h),
                        'sigJournalHash': hashlib.sha256(f'l{i}{h}'.encode()).hexdigest(),
                        'features': {'bid': '0.530000', 'ask': '0.550000', 'bidQuantity': '10', 'askQuantity': '10'},
                        'counterfactual': True, 'actualFill': False, 'feesAssumed': '0',
                        'yesOneShareTakerRoundTrip': '0.020000', 'noOneShareTakerRoundTrip': '-0.060000'})
    return out


class ToxicityTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def write(self, rows, name='original.gz'):
        path = self.root/name
        with gzip.open(path, 'wt', encoding='utf-8') as f:
            for row in rows:
                f.write(canonical(row)+'\n')
        return path

    def run_report(self, rows, **kwargs):
        return report([self.write(rows)], BASE+10000000, **kwargs)

    def test_fixed_side_rule_uses_only_original_features_not_future(self):
        row = records()[0]
        before = feature_state(row, 'scoped_v2')
        self.assertEqual(before[0], {'YES': 'FAVORABLE', 'NO': 'TOXIC'})
        row['externalLogitChanges'] = {'1000': -.003, '5000': -.006, '30000': -.02}
        self.assertEqual(feature_state(row, 'scoped_v2')[0], {'YES': 'TOXIC', 'NO': 'FAVORABLE'})
        row['externalLogitChanges']['5000'] = .006
        self.assertEqual(feature_state(row, 'scoped_v2')[0], {'YES': 'QUIET', 'NO': 'QUIET'})

    def test_exact_passive_markouts_differ_from_taker_crossing_and_do_not_claim_fills(self):
        r = self.run_report(records(), fee=Decimal('.001'), buffer=Decimal('.005'))
        h = r['horizons']['1000']
        self.assertEqual(Decimal(h['YES']['allMatchedControl']['netExecutableMarkoutSum']), Decimal('.034'))
        self.assertEqual(Decimal(h['NO']['allMatchedControl']['netExecutableMarkoutSum']), Decimal('-.046'))
        self.assertEqual(Decimal(h['NO']['avoidedAdverseCounterfactualMagnitude']), Decimal('.046'))
        self.assertEqual(h['NO']['defensiveRetained']['count'], 0)
        self.assertFalse(r['makerFillQualificationAvailable'])
        self.assertFalse(r['profitEstablished'])
        self.assertFalse(r['modelApproval'])

    def test_toxic_quotation_can_have_positive_markout_and_reports_missed_upside(self):
        rows = records()
        rows[0]['externalLogitChanges'] = {'1000': -.003, '5000': -.006, '30000': -.02}
        h = self.run_report(rows)['horizons']['1000']['YES']
        self.assertEqual(Decimal(h['missedPositiveCounterfactualMagnitude']), Decimal('.035'))
        self.assertEqual(Decimal(h['avoidedAdverseCounterfactualMagnitude']), 0)

    def test_missing_history_is_unavailable_not_quiet_and_missing_labels_by_state(self):
        rows = records(2)
        rows[0]['externalLogitChanges']['30000'] = None
        rows = [row for row in rows if not (row['kind'] == 'horizon_label' and row['sampleId'] == rows[5]['sampleId'])]
        h = self.run_report(rows)['horizons']['1000']['data']
        self.assertEqual(h['missingInnovation'], 1)
        self.assertEqual(h['missingLabel'], 1)
        self.assertEqual(h['missingLabel_TOXIC'], 1)
        self.assertEqual(h['missingLabel_FAVORABLE'], 1)

    def test_freshness_and_future_availability_are_distinct(self):
        rows = records()
        rows[0]['sig']['sourceAt'] = stamp(BASE-10001)
        self.assertEqual(self.run_report(rows)['horizons']['1000']['data']['staleSource'], 1)
        rows = records()
        rows[0]['external']['availableMs'] = BASE+1
        with self.assertRaisesRegex(ValueError, 'FEATURE_LOOKAHEAD'):
            self.run_report(rows)

    def test_original_receipt_age_cannot_be_renewed_by_late_availability(self):
        row = records()[0]
        row['external']['receivedMs'] = BASE-100000
        self.assertEqual(feature_state(row, 'scoped_v2'), (None, 'staleSource'))
        row['external']['receivedMs'] = BASE-20000
        self.assertEqual(feature_state(row, 'scoped_v2')[0]['YES'], 'FAVORABLE')
        row['external']['receivedMs'] = BASE-20001
        self.assertEqual(feature_state(row, 'scoped_v2'), (None, 'staleSource'))
        for value in (-1, None):
            row['external']['receivedMs'] = value
            with self.assertRaisesRegex(ValueError, 'FEATURE_LOOKAHEAD'): feature_state(row, 'scoped_v2')
        del row['external']['receivedMs']
        with self.assertRaisesRegex(ValueError, 'FEATURE_LOOKAHEAD'): feature_state(row, 'scoped_v2')

    def test_optional_original_source_timestamp_is_not_clamped_or_ignored(self):
        row = records()[0]
        row['external']['sourceTs'] = BASE+1
        with self.assertRaisesRegex(ValueError, 'FEATURE_LOOKAHEAD'): feature_state(row, 'scoped_v2')
        row['external']['sourceTs'] = BASE-20001
        self.assertEqual(feature_state(row, 'scoped_v2'), (None, 'staleSource'))
        row['external']['sourceTs'] = BASE-20000
        self.assertEqual(feature_state(row, 'scoped_v2')[0]['YES'], 'FAVORABLE')

    def test_original_ws_server_skew_is_bounded_and_causal_at_first_availability(self):
        row = records()[0]
        row['external'].update(receivedMs=BASE-100, availableMs=BASE, sourceTsMs=BASE-86)
        original = copy.deepcopy(row)
        self.assertEqual(feature_state(row, 'scoped_v2')[0]['YES'], 'FAVORABLE')
        self.assertEqual(row, original)
        row['external']['sourceTsMs'] = BASE+1
        with self.assertRaisesRegex(ValueError, 'FEATURE_LOOKAHEAD'): feature_state(row, 'scoped_v2')
        row['external'].update(receivedMs=BASE-2000, sourceTsMs=BASE-999)
        with self.assertRaisesRegex(ValueError, 'FEATURE_LOOKAHEAD'): feature_state(row, 'scoped_v2')
        row['external']['sourceTsMs'] = BASE-1000
        self.assertEqual(feature_state(row, 'scoped_v2')[0]['YES'], 'FAVORABLE')
        row['external']['sourceTs'] = BASE-1001
        with self.assertRaisesRegex(ValueError, 'SOURCE_CLOCK_CONFLICT'): feature_state(row, 'scoped_v2')

    def test_label_cannot_leak_early_late_or_fabricated_execution(self):
        for case in ('early', 'late', 'fill', 'value'):
            rows = records()
            if case == 'early': rows[1]['sourceAt'] = stamp(BASE+999)
            if case == 'late': rows[1]['receivedAt'] = stamp(BASE+4001); rows[1]['observedAt'] = stamp(BASE+4001)
            if case == 'fill': rows[1]['actualFill'] = True
            if case == 'value': rows[1]['yesOneShareTakerRoundTrip'] = '.9'
            with self.assertRaises(ValueError, msg=case): self.run_report(rows)

    def test_market_time_nonoverlap_not_side_or_exchange_independence(self):
        h = self.run_report(records(10, step=1000))['horizons']['1000']
        self.assertEqual(h['data']['matchedOpportunities'], 10)
        self.assertEqual(h['data']['nonoverlappingMarketTimeClusters'], 3)
        self.assertEqual(h['YES']['allMatchedControl']['count'], h['NO']['allMatchedControl']['count'])
        self.assertEqual(h['allMatchedBeforeClustering']['YES']['allMatchedControl']['count'], 10)

    def test_even_100_clusters_never_qualifies_without_original_fills(self):
        r = self.run_report(records(101))
        for h in r['horizons'].values():
            self.assertTrue(h['researchCoverageSufficient'])
            self.assertTrue(h['abstain'])
            self.assertEqual(h['reason'], 'MAKER_FILL_QUALIFICATION_UNAVAILABLE')

    def test_explicit_legacy_original_ids_and_producer_hash(self):
        rows = records()
        d = rows[0]
        sid = hashlib.sha256(f"{d['sig']['journalHash']}:{d['external']['bodySha256']}:{d['decisionMs']}".encode()).hexdigest()
        for row in rows: row['sampleId'] = sid
        producer = self.root/'producer.py'
        producer.write_bytes(b'original source')
        sha = hashlib.sha256(producer.read_bytes()).hexdigest()
        r = self.run_report(rows, producer_source=producer, producer_sha256=sha, sample_id_format='legacy_unscoped_v1')
        self.assertTrue(r['producerSourceVerified'])
        with self.assertRaisesRegex(ValueError, 'SAMPLE_BINDING'): self.run_report(rows)
        with self.assertRaisesRegex(ValueError, 'PRODUCER_SOURCE_HASH'):
            self.run_report(rows, producer_source=producer, producer_sha256='0'*64, sample_id_format='legacy_unscoped_v1')

    def test_conflict_truncation_duplicate_and_cost_bounds(self):
        rows = records()
        r = self.run_report(rows+[copy.deepcopy(rows[0])])
        self.assertEqual(r['input']['identicalDuplicates'], 1)
        changed = copy.deepcopy(rows[0]); changed['externalLogitChanges']['1000'] = 1
        with self.assertRaisesRegex(ValueError, 'CONFLICTING_DUPLICATE'): self.run_report(rows+[changed])
        with self.assertRaisesRegex(ValueError, 'COSTS'): self.run_report(rows, fee=Decimal('-1'))
        with self.assertRaisesRegex(ValueError, 'LABEL_SLACK'): self.run_report(rows, slack=3001)

    def test_cutoff_and_digest_freeze_original_opportunities(self):
        p = self.write(records())
        early = report([p], BASE+1000)
        late = report([p], BASE+30000)
        self.assertNotEqual(early['cutoffPolicySha256'], late['cutoffPolicySha256'])
        self.assertEqual(early['sources'][0]['sha256'], hashlib.sha256(p.read_bytes()).hexdigest())
        source = Path(__file__).with_name('crossvenue_toxicity.py')
        self.assertEqual(early['sourceCodeSha256'], hashlib.sha256(source.read_bytes()).hexdigest())
        self.assertEqual(late['horizons']['30000']['data']['matchedOpportunities'], 1)
        self.assertEqual(early['horizons']['30000']['data']['missingLabel'], 1)

    def test_exact_predeclared_threshold_boundary_and_invalid_numeric_inputs(self):
        row = records()[0]
        row['externalLogitChanges'] = {'1000': .002, '5000': .005, '30000': .01}
        self.assertEqual(feature_state(row, 'scoped_v2')[0]['YES'], 'FAVORABLE')
        row['externalLogitChanges']['1000'] = .001999
        self.assertEqual(feature_state(row, 'scoped_v2')[0]['YES'], 'QUIET')
        for value in (True, '0.1', float('nan')):
            row['externalLogitChanges']['1000'] = value
            with self.assertRaises(ValueError): feature_state(row, 'scoped_v2')

    def test_insufficient_future_capacity_is_not_assumed_executable(self):
        rows = records()
        rows[1]['features']['bidQuantity'] = '0.999999'
        h = self.run_report(rows)['horizons']['1000']
        self.assertEqual(h['data']['missingOneShareLiquidationCapacity'], 1)
        self.assertEqual(h['YES']['allMatchedControl']['count'], 0)

    def test_real_observer_cold_history_is_not_imputed(self):
        out = []; study = Study(out.append, horizons=(1000,))
        study.external_update({'markets': [{'ticker': 'RACE', 'sigMarketIds': ['1'], 'mappingKind': 'direct_party_reference',
                                           'receivedAt': stamp(BASE), 'bodySha256': 'a'*64,
                                           'normalized': {'yesBids': [{'price': '.500000', 'quantity': '10'}],
                                                          'yesAsks': [{'price': '.520000', 'quantity': '10'}]}}]}, BASE)
        def row(at, seq):
            return {'hash': str(seq)*64, 'sequence': seq, 'event': {'type': 'book', 'book': {
                'marketId': '1', 'exchangeId': '101', 'sequence': str(seq), 'sourceAt': stamp(at), 'receivedAt': stamp(at),
                'bids': [{'price': '.490000', 'quantity': 10}], 'asks': [{'price': '.510000', 'quantity': 10}]}}}
        study.accept(row(BASE, 1), BASE); study.accept(row(BASE+1000, 2), BASE+1000)
        r = self.run_report(out)
        self.assertGreater(r['horizons']['1000']['data']['missingInnovation'], 0)


if __name__ == '__main__': unittest.main()
