import copy
import importlib.util
from pathlib import Path
import unittest

HERE = Path(__file__).resolve().parent
S = importlib.util.spec_from_file_location('guard_study', HERE/'guard-study.py')
G = importlib.util.module_from_spec(S); S.loader.exec_module(G)
S = importlib.util.spec_from_file_location('original_fixtures', HERE/'test_analyze.py')
F = importlib.util.module_from_spec(S); S.loader.exec_module(F)


def data():
    s, d = F.fixture()
    study = G.A.analyze(s, [F.book(2000)], d, 'a'*64)
    study['sourceCodeSha256'] = G.ANALYZER_SHA
    study['source'] = {'report': {'sha256': 'a'*64}}
    return s, study, G.recover_labels(s, study, 'a'*64)


class Tests(unittest.TestCase):
    def test_native_yes_and_no_imbalance_opposite(self):
        yes = G.guard('YES', 400000, 400000, 405000, 1, 99)
        no = G.guard('NO', 595000, 400000, 405000, 1, 99)
        self.assertEqual(yes['nativeSideEdgeUnits'], 50)
        self.assertFalse(yes['passesFixedOriginalMinimumEdge'])
        self.assertTrue(no['passesFixedOriginalMinimumEdge'])

    def test_exact_existing_threshold_boundary_and_native_truncation(self):
        self.assertTrue(G.guard('YES', 400000, 400000, 405000, 1, 4)['passesFixedOriginalMinimumEdge'])
        self.assertFalse(G.guard('YES', 400001, 400000, 405000, 1, 4)['passesFixedOriginalMinimumEdge'])
        self.assertEqual(G.guard('NO', 595000, 400000, 405000, 1, 2)['nativeSideEdgeUnits'], 3334)

    def test_frozen_quote_limit_not_improved_actual_fill_price(self):
        self.assertFalse(G.guard('YES', 405000, 400000, 410000, 1, 1)['passesFixedOriginalMinimumEdge'])
        self.assertTrue(G.guard('YES', 400000, 400000, 410000, 1, 1)['passesFixedOriginalMinimumEdge'])

    def test_submission_clock_does_not_use_later_prefill_book(self):
        _, _, fills = data(); start = G.A.clock(F.stamp(200))
        before = F.book(100); after = F.book(900, bids=[dict(price='0.200000', quantity=3)], rev=2)
        G.context_books(fills, [before, after], {'1': start})
        self.assertEqual(fills[0]['contexts']['submission']['bid'], 450000)
        self.assertEqual(fills[0]['contexts']['prefill']['bid'], 200000)

    def test_late_arrival_and_future_source_not_precredited(self):
        _, _, fills = data()
        G.context_books(fills, [F.book(100, received=1100)], {'1': G.A.clock(F.stamp(200))})
        self.assertEqual(fills[0]['contexts'], {})
        _, _, fills = data()
        G.context_books(fills, [F.book(500, received=100)], {'1': G.A.clock(F.stamp(200))})
        self.assertEqual(fills[0]['contexts'], {})

    def test_gap_clears_only_contexts_after_actual_boundary(self):
        _, _, fills = data()
        G.context_books(fills, [F.book(100), F.book(500, rev=2, resync=True)], {'1': G.A.clock(F.stamp(200))})
        self.assertIn('submission', fills[0]['contexts'])
        self.assertNotIn('prefill', fills[0]['contexts'])

    def test_stale_and_invalid_snapshot_unavailable(self):
        _, _, fills = data(); bad = F.book(100); bad['event']['payload']['markets'][0]['batch']['books'][0]['invalid'] = True
        G.context_books(fills, [bad], {'1': G.A.clock(F.stamp(200))})
        self.assertEqual(fills[0]['contexts'], {})
        _, _, fills = data()
        G.context_books(fills, [F.book(-11000)], {'1': G.A.clock(F.stamp(200))})
        self.assertEqual(fills[0]['contexts'], {})

    def test_missing_context_is_separate_and_original_outcomes_preserved(self):
        _, study, fills = data(); r = G.summarize(fills, study)
        self.assertEqual(r['submission']['horizonsMs']['1000']['unavailable']['fills'], 1)
        self.assertEqual(r['prefill']['horizonsMs']['1000']['unavailable']['displayedMarkMinusEntryAndEntryFees'], '0.130000')
        study['aggregate']['horizonsMs']['1000']['fullDepthShares'] = 2
        with self.assertRaises(ValueError): G.summarize(fills, study)

    def test_report_source_and_actual_fill_matching_not_just_cohort_title(self):
        s, study, _ = data(); s['fills'][0]['quantity'] = 2
        s['orders'][0]['filled'] = 2
        with self.assertRaises(ValueError): G.recover_labels(s, study, 'a'*64)
        s, study, _ = data()
        with self.assertRaises(ValueError): G.recover_labels(s, study, 'b'*64)


if __name__ == '__main__': unittest.main()
