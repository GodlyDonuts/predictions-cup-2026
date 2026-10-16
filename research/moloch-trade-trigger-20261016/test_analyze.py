"""Synthetic original-shaped inputs only. Never reads a collecting cohort."""
import copy
from datetime import datetime, timezone
from decimal import Decimal
import gzip
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('trade_analysis', HERE/'analyze.py')
a = importlib.util.module_from_spec(spec); spec.loader.exec_module(a)
EXTERNAL = HERE.parents[1]/'moloch'/'external-data'
sys.path.insert(0, str(EXTERNAL))
from crossvenue_observer import Study, TRADE_SAMPLING_POLICY
from test_crossvenue_observer import row, websocket, public_flow


def stamp(at):
    return datetime.fromtimestamp(at/1000, timezone.utc).isoformat(timespec='milliseconds')


def rebind(r):
    t, e, s, d = r['trigger'], r['external'], r['sig'], r['decisionMs']
    base = a.sha(f"{r['marketId']}:{r['exchangeId']}:{s['journalHash']}:{e['bodySha256']}:{d}".encode())
    r['sampleId'] = a.sha(a.compact({'policy': a.POLICY_SHA, 'trigger': t, 'bookSampleId': base}).encode())
    return r


def evidence_sha(rows):
    return a.sha(json.dumps([{k: r[k] for k in a.FLOW_FIELDS} for r in rows], separators=(',', ':'), ensure_ascii=False).encode())


def sample(at=100000, identity='trade-1', market='1', ticker='RACE-26-D', complete=True):
    """Create through actual sampler, then explicit causal historical anchors."""
    out = []; s = Study(out.append, sampling_mode='trade')
    s.accept(row(at-50), at-50)
    e = websocket(at-10); e['websocket'] = {'coverage': {'connections': 1}}
    e['markets'][0]['tradeFlow'] = public_flow(at-10)
    s.external_update(e, at)
    r = copy.deepcopy(out[0]); r['marketId'] = market
    r['external']['ticker'] = ticker
    f = r['externalTradeFlow']; f['sourceSnapshot']['ticker'] = ticker
    for w in f['sourceSnapshot']['windows'].values():
        for v in w['evidence']:
            v['tradeId'] = identity; v['ticker'] = ticker
        w['evidenceSha256'] = evidence_sha(w['evidence'])
    f['sourceSnapshotSha256'] = a.sha(a.compact(f['sourceSnapshot']).encode())
    r['trigger']['sourceSnapshotSha256'] = f['sourceSnapshotSha256']
    r['trigger']['record'] = copy.deepcopy(f['sourceSnapshot']['windows']['1000']['evidence'][0])
    if complete:
        for lag in (1000, 5000, 30000):
            r['externalLagAnchors'][str(lag)] = {**{k: r['external'].get(k) for k in ('bodySha256', 'receivedMs', 'availableMs', 'source', 'ticker', 'mappingKind', 'sid', 'sequence', 'wsEpoch', 'streamBoundary', 'sourceTsMs')},
                                               'midpoint': r['external']['features']['midpoint']}
            anchor = r['externalLagAnchors'][str(lag)]
            anchor.update(receivedMs=at-lag, availableMs=at-lag, sourceTsMs=at-lag)
            r['externalLogitChanges'][str(lag)] = 0.
    return rebind(r)


def make_book(bid='.43', ask='.45', qty='10'):
    b, c, q = Decimal(bid), Decimal(ask), Decimal(qty)
    return {'bid': str(b), 'ask': str(c), 'bidQuantity': str(q), 'askQuantity': str(q),
            'midpoint': float((b+c)/2), 'spread': float(c-b), 'imbalance': 0., 'microprice': float((b+c)/2)}


def label(r, h=1000, quantity='10'):
    at = r['decisionMs']+h
    f = make_book(qty=quantity); old = r['sig']['features']
    return {'kind': 'horizon_label', 'sampleId': r['sampleId'], 'horizonMs': h,
            'sourceAt': stamp(at), 'receivedAt': stamp(at), 'observedAt': stamp(at+1),
            'sigJournalHash': 'e'*64, 'features': f,
            'logitMidpointChange': a.logit(f['midpoint'])-a.logit(old['midpoint']),
            'yesOneShareTakerRoundTrip': str(Decimal(f['bid'])-Decimal(old['ask'])) if Decimal(quantity) >= 1 else None,
            'noOneShareTakerRoundTrip': str(Decimal(old['bid'])-Decimal(f['ask'])) if Decimal(quantity) >= 1 else None,
            'counterfactual': True, 'feesAssumed': '0', 'actualFill': False,
            'samplingPolicySha256': a.POLICY_SHA, 'samplingMode': 'first_observed_public_trade_v1'}


class Input:
    def __init__(self, cohorts, cutoff=10000000):
        self.temp = tempfile.TemporaryDirectory(); self.path = Path(self.temp.name)
        producer = self.path/'producer.py'; producer.write_bytes((EXTERNAL/'crossvenue_observer.py').read_bytes())
        spec = lambda p: {'path': p.name, 'bytes': p.stat().st_size, 'sha256': a.sha(p.read_bytes())}
        self.manifest = {'schemaVersion': 1, 'sealed': True, 'asOfUtc': stamp(cutoff), 'analysisProtocolSha256': a.PROTOCOL_SHA, 'cohorts': []}
        for i, rows in enumerate(cohorts):
            p = self.path/f'fixture-{i}.jsonl.gz'
            p.write_bytes(gzip.compress((''.join(a.compact(r)+'\n' for r in rows)).encode(), mtime=0))
            self.manifest['cohorts'].append({'cohortId': f'cohort-{i}', 'producer': spec(producer),
                                           'samplingPolicySha256': a.POLICY_SHA, 'configSha256': 'a'*64, 'mappingSha256': 'b'*64,
                                           'files': [spec(p)]})
        self.file = self.path/'manifest.json'; self.save()

    def save(self):
        self.file.write_text(a.compact(self.manifest), encoding='utf-8')
        self.sha = a.sha(self.file.read_bytes())

    def load(self):
        return a.load(self.file, self.sha)

    def close(self):
        self.temp.cleanup()


class ReaderTests(unittest.TestCase):
    def inp(self, cohorts, cutoff=10000000):
        x = Input(cohorts, cutoff); self.addCleanup(x.close); return x

    def test_real_sampler_shape_and_frozen_protocol_hash(self):
        r = sample()
        self.assertEqual(a.decision(r, 100001)[0], ('RACE-26-D', 'trade-1'))
        self.assertEqual(len(a.features(r)), 10)
        self.assertEqual(a.sha((HERE/'analysis-protocol.json').read_bytes().replace(b'\r\n', b'\n')), a.PROTOCOL_SHA)
        self.assertEqual(a.sha(a.compact(TRADE_SAMPLING_POLICY).encode()), a.POLICY_SHA)

    def test_untouched_actual_sampler_bytes_validate_without_id_rewriting(self):
        out = []; s = Study(out.append, sampling_mode='trade')
        s.accept(row(39950), 39950)
        e = websocket(40000); e['markets'][0]['tradeFlow'] = public_flow(40000)
        e['websocket'] = {'coverage': {'connections': 1}}
        s.external_update(e, 40010)
        original = a.compact(out[0])
        a.decision(json.loads(original), 50000)
        self.assertEqual(a.compact(out[0]), original)
        self.assertEqual(a.features(out[0])[:3], [None]*3)

    def test_trade_id_formula_is_not_legacy_book_only_and_trigger_tamper_denied(self):
        r = sample(); r['sampleId'] = a.sha(f"1:101:{r['sig']['journalHash']}:{r['external']['bodySha256']}:100000".encode())
        with self.assertRaisesRegex(ValueError, 'SAMPLE_BINDING'): a.decision(r, 100001)
        r = sample(); r['trigger']['sourceSnapshotSha256'] = 'f'*64; rebind(r)
        with self.assertRaisesRegex(ValueError, 'FLOW_HASH'): a.decision(r, 100001)

    def test_economic_conflict_quarantines_whole_event_across_scopes(self):
        r, other = sample(), sample(market='2')
        for w in other['externalTradeFlow']['sourceSnapshot']['windows'].values():
            w['evidence'][0]['quantity'] = '2.000000'; w['observedQuantity'] = '2.000000'
            w['knownSignedQuantity'] = w['signedQuantity'] = '2.000000'
            w['evidenceSha256'] = evidence_sha(w['evidence'])
        f = other['externalTradeFlow']; f['sourceSnapshotSha256'] = a.sha(a.compact(f['sourceSnapshot']).encode())
        other['trigger']['sourceSnapshotSha256'] = f['sourceSnapshotSha256']
        other['trigger']['record'] = copy.deepcopy(f['sourceSnapshot']['windows']['1000']['evidence'][0]); rebind(other)
        loaded = self.inp([[r], [other]]).load()
        self.assertEqual(loaded[2], []); self.assertEqual(loaded[4]['economicConflicts'], 1)

    def test_alias_no_alternate_scope_or_cohort_rescue(self):
        first = sample(market='1', complete=False); second = sample(market='2')
        _, cutoff, reps, labels, counts, provenance = self.inp([[first], [second, label(second)]]).load()
        rows, c = a.examples(reps, labels, 1000, cutoff)
        self.assertEqual(rows, []); self.assertEqual(c['missingTenFeatures'], 1)
        self.assertEqual(counts['duplicateAliases'], 1); self.assertEqual(len(provenance[0]['occurrences']), 2)

    def test_transport_difference_dedups_without_economic_conflict(self):
        first, second = sample(), sample(at=100010, market='2')
        # Same immutable execution clock, genuine later transport/source export.
        for w in second['externalTradeFlow']['sourceSnapshot']['windows'].values():
            w['evidence'][0]['sourceTsMs'] = first['trigger']['record']['sourceTsMs']
            w['evidenceSha256'] = evidence_sha(w['evidence'])
        f = second['externalTradeFlow']; f['sourceSnapshotSha256'] = a.sha(a.compact(f['sourceSnapshot']).encode())
        second['trigger']['sourceSnapshotSha256'] = f['sourceSnapshotSha256']
        second['trigger']['record'] = copy.deepcopy(f['sourceSnapshot']['windows']['1000']['evidence'][0]); rebind(second)
        loaded = self.inp([[first], [second]]).load()
        self.assertEqual(len(loaded[2]), 1); self.assertEqual(loaded[4]['economicConflicts'], 0)

    def test_clustering_happens_before_missing_feature_or_label_filter(self):
        first = sample(100000, 'first', complete=False)
        later = sample(101000, 'later')
        _, cutoff, reps, labels, _, _ = self.inp([[first, later, label(later)]]).load()
        rows, c = a.examples(reps, labels, 1000, cutoff)
        self.assertEqual(rows, []); self.assertEqual(c['overlappingTickerWindow'], 1)

    def test_original_anchor_availability_and_source_identity_not_renewed(self):
        r = sample(); r['externalLagAnchors']['1000']['availableMs'] = r['decisionMs']+1
        with self.assertRaisesRegex(ValueError, 'ANCHOR_CLOCK'): a.features(r)
        r = sample(); r['externalLagAnchors']['30000']['streamBoundary'] = 123
        with self.assertRaisesRegex(ValueError, 'ANCHOR_SOURCE'): a.features(r)
        r = sample(); r['externalLagAnchors']['1000']['receivedMs'] -= 20001
        with self.assertRaisesRegex(ValueError, 'ANCHOR_CLOCK'): a.features(r)

    def test_exact_yes_no_returns_costs_and_depth_missing_not_zero(self):
        r = sample(); l = label(r)
        y = a.label(r, l, 1000, 200000)
        self.assertEqual(y['yes'], Decimal('.005')); self.assertEqual(y['no'], Decimal('-.055'))
        self.assertIsNone(a.label(r, label(r, quantity='.5'), 1000, 200000))
        l['yesOneShareTakerRoundTrip'] = '0.5'
        with self.assertRaisesRegex(ValueError, 'LABEL_RETURN_CONFLICT'): a.label(r, l, 1000, 200000)

    def test_label_clock_availability_future_and_cutoff_are_strict(self):
        r = sample(); l = label(r); l['sourceAt'] = stamp(r['decisionMs']+999)
        with self.assertRaisesRegex(ValueError, 'LABEL_CLOCK'): a.label(r, l, 1000, 200000)
        l = label(r)
        with self.assertRaisesRegex(ValueError, 'LABEL_CLOCK'): a.label(r, l, 1000, 101000)

    def test_trigger_and_external_clocks_cannot_be_renewed(self):
        r = sample(); r['trigger']['record'].update(receivedMs=r['decisionMs']-2100, sourceTsMs=r['decisionMs']-2100)
        with self.assertRaisesRegex(ValueError, 'TRIGGER_STALE'): a.decision(r, 200000)
        r = sample(); r['external']['receivedMs'] = r['decisionMs']-20001
        with self.assertRaisesRegex(ValueError, 'EXTERNAL_CLOCK_SCOPE'): a.decision(r, 200000)
        r = sample(); r['trigger']['observerFirstAvailableMs'] = r['decisionMs']+1
        with self.assertRaisesRegex(ValueError, 'TRIGGER_CLOCK'): a.decision(r, 200000)

    def test_label_null_depth_and_fee_changes_are_not_authority(self):
        r = sample(); l = label(r, quantity='0.5'); l['yesOneShareTakerRoundTrip'] = '0'
        with self.assertRaisesRegex(ValueError, 'LABEL_RETURN_CONFLICT'): a.label(r, l, 1000, 200000)
        l = label(r); l['feesAssumed'] = '0.001'
        with self.assertRaisesRegex(ValueError, 'LABEL_AUTHORITY'): a.label(r, l, 1000, 200000)

    def test_original_file_manifest_and_producer_are_verified_before_decode(self):
        x = self.inp([[sample()]])
        with self.assertRaisesRegex(ValueError, 'MANIFEST_HASH'): a.load(x.file, '0'*64)
        (x.path/'producer.py').write_bytes(b'not original')
        with self.assertRaisesRegex(ValueError, 'FILE_SIZE'): x.load()

    def test_input_hash_and_duplicate_json_keys_fail_closed(self):
        x = self.inp([[sample()]])
        p = x.path/'fixture-0.jsonl.gz'; p.write_bytes(b'x'*p.stat().st_size)
        with self.assertRaisesRegex(ValueError, 'FILE_HASH'): x.load()
        with self.assertRaisesRegex(ValueError, 'DUPLICATE_JSON_KEY'): a.decode('{"a":1,"a":2}')
        with self.assertRaisesRegex(ValueError, 'NONFINITE_JSON'): a.decode('{"a":NaN}')

    def test_manifest_is_sealed_and_row_bounds_cannot_be_silently_truncated(self):
        x = self.inp([[sample(), label(sample())]])
        with patch.object(a, 'MAX_ROWS', 1):
            with self.assertRaisesRegex(ValueError, 'INPUT_BOUNDS'): x.load()
        x.manifest['sealed'] = False; x.save()
        with self.assertRaisesRegex(ValueError, 'MANIFEST_POLICY'): x.load()

    def test_invalid_label_cannot_hide_behind_missing_features(self):
        r = sample(complete=False); l = label(r); l['yesOneShareTakerRoundTrip'] = '0.9'
        x = self.inp([[r, l]])
        with self.assertRaisesRegex(ValueError, 'LABEL_RETURN_CONFLICT'): x.load()

    def test_original_sample_id_collision_with_changed_evidence_is_rejected(self):
        r = sample(); other = copy.deepcopy(r); other['externalLagAnchors']['1000'] = None
        other['externalLogitChanges']['1000'] = None
        x = self.inp([[r, other]])
        with self.assertRaisesRegex(ValueError, 'CONFLICTING_SAMPLE_ID'): x.load()

    def test_folds_training_scaling_and_shared_model_cohort(self):
        rows = []
        for i in range(120):
            rows.append({'id': str(i), 'ticker': str(i), 'market': '1', 'exchange': '101', 'at': i*10000,
                         'end': i*10000+4000, 'available': i*10000+1001, 'x': [float(i%3)]*10,
                         'yes': Decimal('.01'), 'no': Decimal('-.03'), 'bbo': Decimal('-.025')})
        result = a.compare(rows, [300000, 600000, 900000], 1200000)
        self.assertEqual({v['outOfSampleClusters'] for v in result['models'].values()}, {90})
        self.assertEqual(len({v['outOfSampleIdsSha256'] for v in result['models'].values()}), 1)
        self.assertTrue(all(v['abstain'] for v in result['models'].values()))
        self.assertEqual(result['models']['unchanged_sig_bbo']['selectedClusters'], 0)
        trained = a.fit(rows[:30], (4, 5), 'yes')
        changed = copy.deepcopy(rows); changed[60]['x'] = [1e10]*10
        self.assertEqual(trained, a.fit(changed[:30], (4, 5), 'yes'))

    def test_purge_strict_boundary_and_actual_label_availability(self):
        rows = [{'id': str(i), 'ticker': str(i), 'market': '1', 'at': 100+i, 'end': 500,
                 'available': 499, 'x': [0.]*10, 'yes': Decimal('0'), 'no': Decimal('0'), 'bbo': Decimal('0')} for i in range(20)]
        result = a.compare(rows, [500, 600, 700], 800)
        self.assertEqual(result['folds'][0]['trainingClusters'], 0)
        rows[0]['available'] = 601
        result = a.compare(rows, [500, 600, 700], 800)
        self.assertEqual(result['folds'][1]['trainingClusters'], 19)

    def test_numerical_failure_abstains_all_models_without_fallback(self):
        rows = [{'id': str(i), 'ticker': str(i), 'market': '1', 'at': i*10000, 'end': i*10000+1000,
                 'available': i*10000+1000, 'x': [0.]*10, 'yes': Decimal('0'), 'no': Decimal('0'), 'bbo': Decimal('0')} for i in range(100)]
        with patch.object(a, 'fit', side_effect=ValueError('TRADE_ANALYSIS_SINGULAR_MODEL')):
            result = a.compare(rows, [300000, 600000, 900000], 1000000)
        self.assertEqual({v['outOfSampleClusters'] for v in result['models'].values()}, {0})
        self.assertTrue(all(f['abstentionReason'] == 'NUMERICAL_FIT_FAILURE' for f in result['folds']))

    def test_selection_exact_tie_yes_zero_not_selected_and_minimum_not_approval(self):
        rows = [{'id': str(i), 'ticker': str(i), 'market': '1', 'at': i*4000, 'end': i*4000+4000,
                 'outcomes': [Decimal('0.010001'), Decimal('0.010001')], 'predictions': [.02, .02], 'intercept': [0., 0.]} for i in range(100)]
        m = a.metrics(rows)
        self.assertEqual(m['selectedNetCrossingSumExact'], '1.000100')
        self.assertTrue(m['coverageThresholdsPass']); self.assertFalse(m['abstain'])
        self.assertEqual(m['status'], 'UNQUALIFIED_COUNTERFACTUAL_RESEARCH')
        self.assertTrue(all(r['side'] == 'YES' for r in m['selectedWindows']))
        rows[0]['predictions'] = [0., -1.]
        self.assertEqual(a.metrics(rows)['selectedClusters'], 99)

    def test_end_to_end_small_manifest_outputs_abstention_and_no_authority(self):
        rows = []
        for i in range(8):
            r = sample(100000+i*10000, str(i), ticker=f'RACE-{i}')
            rows.extend([r, label(r)])
        x = self.inp([rows]); result = a.analyze(x.file, x.sha)
        self.assertEqual(result['calendarBoundariesMs'], [120000, 140000, 160000])
        self.assertFalse(result['rawSourceWitnessesIndependentlyVerified'])
        self.assertFalse(result['modelApproval']); self.assertFalse(result['executionAuthority'])
        self.assertTrue(all(v['abstain'] for h in result['horizons'].values() for v in h['models'].values()))
        self.assertEqual(result['horizons']['1000']['coverage']['commonCompleteClusters'], 8)
        self.assertEqual(result['horizons']['3000']['coverage']['missingLabel'], 8)


if __name__ == '__main__':
    unittest.main()
