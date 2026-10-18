import unittest
from .native_candidate_search import minimal_over_budget, scenario_keys


class NativeCapacitySearchTests(unittest.TestCase):
    def fixture(self):
        model = {"scenarios": [{"kind": "ordinary", "payouts": {
            "held": "resolved_yes", "a": "resolved_yes" if a else "resolved_no",
            "b": "resolved_yes" if b else "resolved_no"}}
            for a in (0, 1) for b in (0, 1)]}
        return model, [{"thresholds": ["10.000000"] * 4}]

    def test_exact_monotone_cut_keeps_held_scope_and_all_states(self):
        model, fields = self.fixture()
        self.assertEqual(len(scenario_keys(model, fields, {"held"})), 1)
        self.assertEqual(len(scenario_keys(model, fields, {"held", "a"})), 2)
        bad = minimal_over_budget({"held"}, ["a", "b"], {"a": "a", "b": "b"}, model, fields, 2)
        self.assertEqual(bad, ["a", "b"])
        self.assertEqual(len(model["scenarios"]), 4)

    def test_field_thresholds_are_part_of_lossless_capacity(self):
        model, fields = self.fixture()
        fields[0]["thresholds"] = ["10.000000", "11.000000", "12.000000", "13.000000"]
        self.assertEqual(len(scenario_keys(model, fields, {"held"})), 4)
        with self.assertRaisesRegex(ValueError, "EXCLUSION_PRECONDITION"):
            minimal_over_budget({"held"}, ["a", "b"], {"a": "a", "b": "b"}, model, fields, 2)
