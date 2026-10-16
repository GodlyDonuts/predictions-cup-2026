import copy
from decimal import Decimal
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
import analyze as a
from test_crossvenue_economics import records, BASE


class AblationTests(unittest.TestCase):
    def prepare(self, original):
        d = {r['sampleId']: r for r in original if r['kind'] == 'decision_features'}
        l = {(r['sampleId'], r['horizonMs']): r for r in original if r['kind'] == 'horizon_label'}
        rows, _ = a.e.examples(d, l, 1000, BASE+100000000, 3000, Decimal(0), Decimal('.005'), 'direct_party_reference')
        return d, rows

    def test_full_fixed_model_and_purged_cohort_match_existing_evaluator_exactly(self):
        d, rows = self.prepare(records(120))
        original = a.e.walk_forward(rows)
        r = a.compare(rows, d)
        self.assertEqual(r['fullRidge']['mse'], original['modelMseNetReturn'])
        self.assertEqual(r['fullRidge']['selectedNetCrossingReturnSumExact'], original['selectedNetCrossingReturnSumExact'])
        self.assertEqual(r['outOfSampleSampleIdsSha256'], original['outOfSampleSampleIdsSha256'])
        self.assertEqual(r['sigLocalRidge']['outOfSampleClusters'], original['outOfSampleClusters'])
        for f in r['folds']:
            if f['trained']: self.assertLessEqual(f['latestTrainingLabelAvailableMs'], f['testStartMs'])

    def test_unchanged_sig_bbo_predicts_mechanical_crossing_loss_without_external_information(self):
        original = records(120)
        for r in original:
            if r['kind'] == 'horizon_label':
                r['features'].update(bid='0.490000', ask='0.510000')
                r['yesOneShareTakerRoundTrip'] = '-0.020000'
                r['noOneShareTakerRoundTrip'] = '-0.020000'
        d, rows = self.prepare(original)
        r = a.compare(rows, d)
        self.assertEqual(r['unchangedSigBbo']['mse'], 0)
        self.assertEqual(r['unchangedSigBbo']['selectedClusters'], 0)
        self.assertEqual(r['unchangedSigBbo']['selectedNetCrossingReturnSumExact'], '0')

    def test_sig_only_fit_cannot_use_external_columns_and_keeps_same_cohort(self):
        d, rows = self.prepare(records(120))
        before = a.compare(rows, d)
        modified = copy.deepcopy(rows)
        for i, r in enumerate(modified):
            for j in (0, 1, 2, 3, 6): r['x'][j] = i*2-j
        after = a.compare(modified, d)
        self.assertEqual(before['sigLocalRidge'], after['sigLocalRidge'])
        self.assertEqual(before['unchangedSigBbo'], after['unchangedSigBbo'])

    def test_innovation_missing_zero_nonzero_and_magnitudes_remain_distinct(self):
        d = {r['sampleId']: r for r in records(3) if r['kind'] == 'decision_features'}
        rows = list(d.values())
        rows[0]['externalLogitChanges']['1000'] = None
        rows[1]['externalLogitChanges']['1000'] = 0
        rows[2]['externalLogitChanges']['1000'] = -.1
        r = a.innovations(d)['lags']['1000']
        self.assertEqual((r['missing'], r['zero'], r['nonzero']), (1, 1, 1))
        self.assertEqual(Decimal(r['sumAbsolute']), Decimal('.1'))

    def test_insufficient_cohort_remains_missing_not_positive_selected_return(self):
        d, rows = self.prepare(records(2))
        result = a.compare(rows, d)
        self.assertEqual(result['unchangedSigBbo']['outOfSampleClusters'], 0)
        self.assertIsNone(result['unchangedSigBbo']['mse'])
        self.assertEqual(result['fullRidge']['selectedClusters'], 0)

    def test_source_and_producer_hashes_are_pinned_without_model_approval(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp); p=root/'records.ndjson'; producer=root/'producer.py'
            p.write_text(''.join(a.e.canonical(r)+'\n' for r in records(40)))
            producer.write_text('original fixture producer')
            sha=hashlib.sha256(producer.read_bytes()).hexdigest()
            result=a.analyze([p], BASE+100000000, producer, sha)
            self.assertEqual(result['producerSha256'], sha)
            self.assertEqual(result['sources'][0]['sha256'], hashlib.sha256(p.read_bytes()).hexdigest())
            self.assertFalse(result['modelApproval']);self.assertFalse(result['profitEstablished'])
            self.assertFalse(result['executionAuthority'])
            self.assertFalse(result['policy']['fitOrThresholdTuning'])
            with self.assertRaisesRegex(ValueError,'PRODUCER_SOURCE_HASH'):a.analyze([p],BASE+100000000,producer,'0'*64)


if __name__ == '__main__': unittest.main()
