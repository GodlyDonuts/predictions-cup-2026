"""Synthetic-only tests; no original live book, fill or model probabilities."""
import copy
import importlib.util
import unittest
from unittest.mock import patch

from moloch.tartarus.target_frontier import evaluate_candidate, solve_frontier, prepare

U = 1_000_000
SHA = "a" * 64
HAS_SCIPY = importlib.util.find_spec("scipy") is not None


def fixture(cash=94_737 * U, depth=1_000_000, probability=60_000):
    account = {"schema_version": 1, "evidence_class": "synthetic", "as_of_ms": 10,
               "valid_until_ms": 1000, "source_sha256": SHA,
               "unmodeled_state_statement": "Synthetic two-state exercise, not election calibration.",
               "execution_authorized": False, "calibrated_probability_claimed": False,
               "scenario_ids": ["wins", "loses"], "baseline_wealth_micros": [cash, cash],
               "cash_micros": cash, "reserved_cash_micros": 0, "available_cash_micros": cash,
               "terminal_debt_micros": 0, "holdings": [],
               "contracts": [{"contract_id": "c", "exchange_id": "x", "semantic_source_sha256": SHA}],
               "scenario_payouts": [{"c": "resolved_yes"}, {"c": "resolved_no"}]}
    actions = [{"id": "buy", "contract_id": "c", "exchange_id": "x", "side": "YES",
                "levels": [{"quantity": depth, "unit_cost_micros": 80_000}],
                "fee_per_share_micros": 0, "source_sha256": SHA,
                "available_at_ms": 10, "valid_until_ms": 900}]
    models = [{"id": "six", "weights_ppm": [probability, U - probability],
               "assumption_label": "Synthetic, not a forecast."}]
    rivals = [{"id": "fixed", "thresholds_micros": [1_000_000 * U - 1] * 2,
               "complete_field": False, "assumption_label": "Synthetic fixed target; no leaderboard query."}]
    constraints = {"decision_ms": 20, "maximum_gross_cost_micros": cash,
                   "maximum_total_quantity": 1_000_000, "solver_time_limit_seconds": 3,
                   "target_pairs": [{"probability_model_id": "six", "rival_model_id": "fixed"}]}
    return account, actions, models, rivals, constraints


class ExactAccounting(unittest.TestCase):
    def test_paper_negative_and_mixed_mean_are_not_alpha(self):
        f = fixture()
        c = evaluate_candidate(*f, {"buy": 983982})
        self.assertEqual(c["gross_cost_micros"], 78_718_560_000)
        self.assertEqual(c["whole_account_wealth_after_micros"], [1_000_000_440_000, 16_018_440_000])
        self.assertEqual(c["monetary_expectations"]["six"]["gain_numerator"], -19_679_640_000 * U)
        self.assertEqual(c["targets"]["six|fixed"]["strict_hit_ppm"], 60_000)
        self.assertFalse(c["positive_alpha_qualified"])
        self.assertFalse(c["execution_authorized"])
        f[2].append({"id": "twelve", "weights_ppm": [120_000, 880_000], "assumption_label": "Different synthetic assumption."})
        f[4]["target_pairs"].append({"probability_model_id": "twelve", "rival_model_id": "fixed"})
        c = evaluate_candidate(*f, {"buy": 983982})
        self.assertEqual(c["classification"], "mixed_monetary_expectation")
        self.assertEqual(c["monetary_expectations"]["twelve"]["gain_numerator"], 39_359_280_000 * U)

    def test_full_23_holding_baseline_refund_and_fee(self):
        f = fixture(cash=100 * U)
        a = f[0]; a["terminal_debt_micros"] = 3 * U; a["reserved_cash_micros"] = 5 * U; a["available_cash_micros"] = 95 * U
        for i in range(23):
            cid = "held" + str(i); a["contracts"].append({"contract_id": cid, "exchange_id": "x" + str(i), "semantic_source_sha256": SHA})
            a["holdings"].append({"contract_id": cid, "exchange_id": "x" + str(i), "side": "NO", "quantity": 2, "lots": [{"quantity": 1, "unit_cost_micros": 200_000}, {"quantity": 1, "unit_cost_micros": 300_000}], "source_sha256": SHA})
            a["scenario_payouts"][0][cid] = "refund_principal"; a["scenario_payouts"][1][cid] = "resolved_yes"
        a["scenario_payouts"][0]["c"] = "refund_principal"
        a["baseline_wealth_micros"] = [108_500_000, 97_000_000]
        f[1][0]["fee_per_share_micros"] = 1000
        c = evaluate_candidate(*f, {"buy": 1})
        self.assertEqual(c["gross_cost_micros"], 81_000)
        self.assertEqual(c["whole_account_wealth_after_micros"], [108_499_000, 96_919_000])
        self.assertEqual(c["cash_after_micros"], 99_919_000)
        del a["holdings"][0]
        with self.assertRaisesRegex(ValueError, "BASELINE_MISMATCH"):
            evaluate_candidate(*f, {})

    def test_actual_cash_not_marks_sale_ack_or_available_override(self):
        f = fixture(cash=3613_785000)
        f[0]["marked_equity_micros"] = 94_737 * U
        f[0]["acknowledged_sale_proceeds_micros"] = 500_000 * U
        with self.assertRaisesRegex(ValueError, "FUNDING"):
            evaluate_candidate(*f, {"buy": 983982})
        f[0]["available_cash_micros"] = 94_737 * U
        with self.assertRaisesRegex(ValueError, "AVAILABLE_CASH"):
            evaluate_candidate(*f, {})

    def test_held_binary_unit_cost_above_one_is_corrupt_even_if_baseline_matches(self):
        f = fixture(cash=100 * U); a = f[0]
        a["holdings"] = [{"contract_id": "c", "exchange_id": "x", "side": "YES", "quantity": 1,
                           "lots": [{"quantity": 1, "unit_cost_micros": U + 1}], "source_sha256": SHA}]
        a["scenario_payouts"][0]["c"] = "refund_principal"
        a["baseline_wealth_micros"] = [101 * U + 1, 100 * U]
        with self.assertRaisesRegex(ValueError, "LOT_COST"):
            evaluate_candidate(*f, {})

    def test_exact_prefix_cost_depth_quantity_clock_identity(self):
        f = fixture(cash=10 * U)
        f[1][0]["levels"] = [{"quantity": 3, "unit_cost_micros": 100_000}, {"quantity": 5, "unit_cost_micros": 200_000}]
        c = evaluate_candidate(*f, {"buy": 5})
        self.assertEqual(c["gross_cost_micros"], 700_000)
        self.assertEqual([x["quantity"] for x in c["depth_prefixes"]], [3, 2])
        for quantity in [9, 1.0, True, -1]:
            with self.assertRaises(ValueError):
                evaluate_candidate(*f, {"buy": quantity})
        f[1][0]["valid_until_ms"] = 20
        with self.assertRaisesRegex(ValueError, "EXPIRED"):
            evaluate_candidate(*f, {})

    def test_original_joint_scenario_alignment_and_ties(self):
        f = fixture(cash=U)
        f[3][0]["thresholds_micros"] = [1_920_000, 2_000_000]
        c = evaluate_candidate(*f, {"buy": 1})
        t = c["targets"]["six|fixed"]
        self.assertEqual(t["strict_hit_ppm"], 0)
        self.assertEqual(t["tie_ppm"], 60_000)
        self.assertIsNone(t["expected_prize"])
        f[4]["target_pairs"] = []
        with self.assertRaisesRegex(ValueError, "EXPLICIT_TARGET_PAIRS"):
            evaluate_candidate(*f, {})

    def test_bool_float_weights_missing_or_conflicting_depth_reject(self):
        f = fixture()
        for value in [True, 60000.0, -1]:
            g = copy.deepcopy(f); g[2][0]["weights_ppm"][0] = value
            with self.assertRaises(ValueError):
                evaluate_candidate(*g, {})
        f[1].append(copy.deepcopy(f[1][0])); f[1][1]["id"] = "duplicate_depth"
        with self.assertRaisesRegex(ValueError, "DUPLICATE_ACTION_OR_DEPTH"):
            evaluate_candidate(*f, {})


@unittest.skipUnless(HAS_SCIPY, "Optional private SciPy environment unavailable")
class FiniteMILP(unittest.TestCase):
    def test_already_winning_zero_weight_states_force_hit_one_for_mean_and_min_cost(self):
        f = fixture(cash=2_000_000 * U, probability=0)
        out = solve_frontier(*f)
        self.assertTrue(all(r.get("exact_integer_recheck") == "passed" for r in out["solver_runs"]), out["solver_runs"])
        for r in out["solver_runs"]:
            self.assertEqual(r["solver_status"], 0)
        self.assertEqual(out["baseline"]["targets"]["six|fixed"]["strict_hit_ppm"], U)
        self.assertTrue(any(r["goal"] == "mean" for r in out["solver_runs"]))
        self.assertTrue(any(r["goal"] == "minimum_cost_target" for r in out["solver_runs"]))

    def test_negative_mean_rank_survives_and_minimum_target_quantity(self):
        out = solve_frontier(*fixture())
        reached = [c for c in out["candidates"] if c["targets"]["six|fixed"]["strict_hit_ppm"]]
        self.assertTrue(reached, out["solver_runs"])
        self.assertTrue(any(c["quantities"]["buy"] == 983982 for c in reached), out["solver_runs"])
        self.assertTrue(any(c["classification"] == "negative_monetary_expectation" for c in out["frontier"]))
        self.assertEqual(out["baseline"]["gross_cost_micros"], 0)

    def test_positive_mean_is_not_only_candidate_and_declared_sweeps(self):
        f = fixture(probability=120_000)
        f[4]["mean_gain_thresholds_micros"] = [0, 1000 * U]
        f[4]["minimum_wealth_thresholds_micros"] = [20_000 * U]
        out = solve_frontier(*f)
        self.assertGreaterEqual(len(out["frontier"]), 2, out["solver_runs"])
        self.assertTrue(any(c["quantities"]["buy"] == 1_000_000 for c in out["candidates"]), out["solver_runs"])
        self.assertTrue(any(r["minimum_mean_gain_micros"] == 1000 * U for r in out["solver_runs"]))
        self.assertTrue(any(r["minimum_wealth_micros"] == 20_000 * U for r in out["solver_runs"]))

    def test_insufficient_depth_or_cash_has_exact_capacity_rejection_and_fallback(self):
        for f in [fixture(depth=500_000), fixture(cash=3613_785000)]:
            out = solve_frontier(*f)
            self.assertIn("unattainable", out["target_outcomes"]["six|fixed"]["status"])
            self.assertEqual(out["target_outcomes"]["six|fixed"]["analytical_capacity_hit_upper_ppm"], 0)
            self.assertTrue(out["candidates"])
            self.assertTrue(out["frontier"])

    def test_numeric_noninteger_and_infeasible_incumbents_never_escape(self):
        import numpy as np
        from types import SimpleNamespace
        f = fixture()
        for vector in [[983982.2, 1, 0], [1_000_001, 1, 0]]:
            fake = SimpleNamespace(status=1, message="Synthetic solver limit", x=np.array(vector))
            with patch("scipy.optimize.milp", return_value=fake):
                out = solve_frontier(*f)
            self.assertEqual(len(out["candidates"]), 1)
            self.assertTrue(all(r["exact_integer_recheck"] == "rejected" for r in out["solver_runs"]))
            self.assertTrue(all(r["inconclusive"] for r in out["solver_runs"]))

    def test_solver_limit_feasible_is_retained_without_optimality_or_unreachable_claim(self):
        import numpy as np
        from types import SimpleNamespace
        f = fixture(); f[4]["maximum_solver_calls"] = 1
        fake = SimpleNamespace(status=1, message="Synthetic solver time limit", x=np.array([983982., 1., 0.]))
        with patch("scipy.optimize.milp", return_value=fake):
            out = solve_frontier(*f)
        self.assertEqual(out["solver_runs"][0]["exact_integer_recheck"], "passed")
        self.assertFalse(out["solver_runs"][0]["numerical_optimum_reported"])
        self.assertFalse(out["frontier_exhaustive"])
        self.assertGreater(out["sweeps_not_run"], 0)


if __name__ == "__main__":
    unittest.main()
