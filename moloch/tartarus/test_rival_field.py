"""Synthetic Contract-C checks only; no current leaderboard or private positions."""
import copy
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('rival_field', Path(__file__).with_name('rival_field.py'))
R = importlib.util.module_from_spec(spec)
spec.loader.exec_module(R)


def observation(count=1676, rows=128):
    return {'schema_version': 1, 'evidence_class': 'synthetic',
            'as_of_ms': 1000, 'published_at_ms': 900, 'valid_until_ms': 2000,
            'source_sha256': 'a'*64, 'source_url': 'https://example.test/public-leaderboard',
            'participant_count': count, 'our_participant_id': 'ours',
            'observed_top': [{'participant_id': str(i), 'wealth_micros': 1000000-i*100}
                             for i in range(rows)],
            'unobserved_field_statement': 'Synthetic unknown rivals; no private portfolio inference.'}


def case(kind='flat', **updates):
    result = {'id': kind, 'kind': kind, 'assumption_label': 'Synthetic declared scenario stress',
              'decision_ms': 1500, 'projection_at_ms': 900+R.DAY_MS,
              'unknown_envelope_micros': {'D': 900000, 'R': 900000}}
    result.update(updates)
    return result


class RivalFieldTests(unittest.TestCase):
    def build(self, c=None, o=None, ids=None):
        return R.build_models(o or observation(), ids or ['D', 'R'], [c or case()])[0]

    def test_field_larger_than_128_and_top128_not_complete(self):
        m = self.build()
        self.assertEqual(m['opponent_count'], 1675)
        self.assertEqual(m['observed_opponent_count'], 128)
        self.assertEqual(m['unobserved_opponent_count'], 1547)
        self.assertFalse(m['complete_field'])
        self.assertFalse(m['execution_authorized'])
        self.assertEqual(m['thresholds_micros'], [1000000, 1000000])
        self.assertEqual(m['top3_micros'], [None, None])

    def test_unknown_envelope_changes_first_place_range(self):
        low = self.build()
        high = self.build(case(unknown_envelope_micros={'D': 1200000, 'R': 1200000}))
        a = R.first_place_summary(low, [1100000, 1100000], [500000, 500000])
        b = R.first_place_summary(high, [1100000, 1100000], [500000, 500000])
        self.assertEqual(a['strict_first_probability_ppm_bounds'], [1000000, 1000000])
        self.assertEqual(b['strict_first_probability_ppm_bounds'], [0, 0])
        self.assertIsNone(a['first_prize_probability_ppm'])

    def test_unknown_field_missing_is_unavailable_not_zero(self):
        c = case(); del c['unknown_envelope_micros']
        with self.assertRaisesRegex(R.Unavailable, 'UNKNOWN_ENVELOPE'):
            self.build(c)

    def test_joint_factor_alignment_matters_without_independent_shuffle(self):
        c = case('factor_correlated', scenario_factors={'national': {'D': 200000, 'R': -200000}},
                 factor_loadings_ppm={'national': 1000000})
        aligned = self.build(c)
        self.assertEqual(aligned['scenario_ids'], ['D', 'R'])
        self.assertEqual(aligned['thresholds_micros'], [1200000, 900000])
        a = R.first_place_summary(aligned, [1300000, 950000], [500000, 500000])
        c['scenario_factors']['national'] = {'D': -200000, 'R': 200000}
        b = R.first_place_summary(self.build(c), [1300000, 950000], [500000, 500000])
        self.assertEqual(a['strict_first_probability_ppm_bounds'], [1000000, 1000000])
        self.assertEqual(b['strict_first_probability_ppm_bounds'], [500000, 500000])

    def test_participant_loadings_are_explicit_and_can_change_top_order(self):
        c = case('factor_correlated', scenario_factors={'f': {'D': 1000, 'R': -1000}},
                 factor_loadings_ppm={'f': 0}, participant_loadings_ppm={'1': {'f': 2000000}})
        m = self.build(c)
        self.assertEqual(m['thresholds_micros'], [1001900, 1000000])
        self.assertEqual(m['observed_top3_bounds_micros'][0]['upper'][0], 1001900)

    def test_growing_exact_integer_ceiling_and_horizon(self):
        c = case('growing', growth_micros_per_day=7, projection_at_ms=1501)
        self.assertEqual(self.build(c)['thresholds_micros'], [1000001, 1000001])
        c['projection_at_ms'] = 900+R.DAY_MS
        self.assertEqual(self.build(c)['thresholds_micros'], [1000007, 1000007])

    def test_adverse_unknown_overtaking_is_distinct_not_private_holdings(self):
        m = self.build(case('adverse_overtaking', overtaking_margin_micros={'D': 500000, 'R': 1}))
        self.assertEqual(m['thresholds_micros'], [1500000, 1000001])
        self.assertEqual(m['unobserved_opponent_count'], 1547)

    def test_rounding_interval_retains_uncertain_ties(self):
        o = observation(3, 1)
        o['observed_top'][0] = {'participant_id': 'other', 'wealth_low_micros': 100, 'wealth_high_micros': 110}
        m = self.build(case(unknown_envelope_micros={'D': 50, 'R': 50}), o)
        summary = R.first_place_summary(m, [105, 105], [500000, 500000])
        self.assertEqual(summary['strict_first_probability_ppm_bounds'], [0, 1000000])
        self.assertIsNone(summary['expected_prize_micros'])

    def test_ties_only_use_supplied_reviewed_rule_no_split_multiplicity_guess(self):
        m = self.build(case(tie_rule='lose_ties', prizes_micros=[100, 50, 10]))
        summary = R.first_place_summary(m, [1000000, 1000001], [500000, 500000])
        self.assertEqual(summary['first_prize_probability_ppm'], 500000)
        m['tie_rule'] = 'split_occupied_prizes'
        self.assertIsNone(R.first_place_summary(m, [1000000, 1000001], [500000, 500000])['first_prize_probability_ppm'])
        self.assertIsNone(m.get('opponent_tie_counts'))

    def test_unknown_top3_requires_explicit_order_statistics(self):
        c = case(unknown_envelope_micros={'D': 1200000, 'R': 1100000},
                 unknown_top3_micros={'D': [1200000, 1150000, 900000], 'R': [1100000, 1050000, 900000]})
        m = self.build(c)
        self.assertEqual(m['top3_micros'][0]['upper'], [1200000, 1150000, 1000000])
        c['unknown_top3_micros']['D'][0] = 1199999
        with self.assertRaisesRegex(R.Unavailable, 'UNKNOWN_TOP3'):
            self.build(c)

    def test_missing_publication_is_none_and_growth_unavailable(self):
        o = observation();o['published_at_ms'] = None
        self.assertIsNone(self.build(o=o)['published_at_ms'])
        with self.assertRaisesRegex(R.Unavailable, 'GROWTH_PUBLICATION_CLOCK_UNAVAILABLE'):
            self.build(case('growing', growth_micros_per_day=1), o)

    def test_stale_observation_requires_declared_research_and_never_renews_clock(self):
        c = case(decision_ms=3000)
        with self.assertRaisesRegex(R.Unavailable, 'STALE_OBSERVATION'):
            self.build(c)
        c['allow_stale_observation'] = True
        m = self.build(c)
        self.assertTrue(m['source_stale']);self.assertEqual(m['as_of_ms'], 1000)
        self.assertEqual(m['valid_until_ms'], 2000);self.assertFalse(m['execution_authorized'])

    def test_future_sources_duplicate_self_missing_scenario_and_float_money_reject(self):
        for change, code in [(lambda o:o.update(as_of_ms=1600), 'SOURCE_NOT_AVAILABLE'),
                             (lambda o:o['observed_top'].append(copy.deepcopy(o['observed_top'][0])), 'DUPLICATE_OR_SELF'),
                             (lambda o:o['observed_top'][0].update(participant_id='ours'), 'DUPLICATE_OR_SELF'),
                             (lambda o:o['observed_top'][0].update(wealth_micros=1.2), 'OBSERVED_WEALTH'),
                             (lambda o:o.update(participant_count=True), 'PARTICIPANT_COUNT')]:
            o = observation();change(o)
            with self.assertRaisesRegex(R.Unavailable, code):self.build(o=o)
        with self.assertRaisesRegex(R.Unavailable, 'UNKNOWN_ENVELOPE'):
            self.build(case(unknown_envelope_micros={'D': 2}))

    def test_complete_observed_list_still_no_certificate_and_invalid_prizes_reject(self):
        m = self.build(case(unknown_envelope_micros=None), observation(4, 3))
        self.assertEqual(m['unobserved_opponent_count'], 0)
        self.assertFalse(m['complete_field']);self.assertEqual(m['top3_micros'][0]['upper'], [1000000, 999900, 999800])
        with self.assertRaisesRegex(R.Unavailable, 'PRIZE_ORDER'):
            self.build(case(prizes_micros=[1, 2]))
        with self.assertRaisesRegex(R.Unavailable, 'WEIGHT_MASS'):
            R.first_place_summary(m, [1, 2], [1, 2])


if __name__ == '__main__':
    unittest.main()
