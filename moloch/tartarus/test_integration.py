"""One synthetic cross-module campaign; no account, public forecast or order."""
import copy
import importlib.util
import unittest

from moloch.tartarus.account_scenarios import compile_account, AccountScenarioError
from moloch.tartarus.rival_field import build_models
from moloch.tartarus.target_frontier import solve_frontier
from test_target_frontier import fixture, U, SHA


@unittest.skipUnless(importlib.util.find_spec("scipy"), "Private research solver unavailable")
class CampaignIntegration(unittest.TestCase):
    def test_full_account_to_incomplete_field_to_negative_mean_campaign(self):
        compiled, actions, probabilities, _, constraints = fixture()
        common = {k: compiled[k] for k in ("schema_version", "evidence_class", "as_of_ms",
                  "valid_until_ms", "source_sha256", "unmodeled_state_statement")}
        holdings = [{"contract_id": f"held{i}", "exchange_id": f"heldex{i}", "side": "YES",
                     "quantity": 1, "lots": [{"quantity": 1, "unit_cost_micros": 400_000}],
                     "source_sha256": SHA} for i in range(23)]
        contracts = copy.deepcopy(compiled["contracts"]) + [
            {"contract_id": h["contract_id"], "exchange_id": h["exchange_id"],
             "semantic_source_sha256": SHA} for h in holdings]
        account = {**common, "cash_micros": compiled["cash_micros"],
                   "reserved_cash_micros": 0, "terminal_debt_micros": 0,
                   "holdings": holdings, "pending_obligations": []}
        scenarios = {**common, "contracts": contracts, "scenarios": [
            {"id": sid, "kind": "ordinary", "assumption_label": "Synthetic joint state, not independence",
             "payouts": {**payout, **{h["contract_id"]: ("resolved_yes" if index == 0 else "resolved_no")
                                       for h in holdings}}}
            for index, (sid, payout) in enumerate(zip(compiled["scenario_ids"], compiled["scenario_payouts"]))]}
        actual = compile_account(account, scenarios, constraints["decision_ms"])
        self.assertEqual(len(actual["holdings"]), 23)
        self.assertEqual(actual["baseline_wealth_micros"], [94_760 * U, 94_737 * U])
        observation = {**common, "source_url": "https://example.test/synthetic-field",
                       "published_at_ms": 9, "participant_count": 1676, "our_participant_id": "ours",
                       "observed_top": [{"participant_id": str(i), "wealth_micros": 1_000_000 * U - i}
                                        for i in range(128)],
                       "unobserved_field_statement": "Unknown field has an explicitly assumed envelope"}
        rivals = build_models(observation, actual["scenario_ids"], [{
            "id": "fixed", "kind": "flat", "assumption_label": "Synthetic fixed terminal target",
            "decision_ms": constraints["decision_ms"], "projection_at_ms": constraints["decision_ms"],
            "unknown_envelope_micros": {sid: 900_000 * U for sid in actual["scenario_ids"]}}])
        self.assertFalse(rivals[0]["complete_field"])
        result = solve_frontier(actual, actions, probabilities, rivals, constraints)
        attaining = [c for c in result["candidates"] if c["targets"]["six|fixed"]["strict_hit_ppm"] == 60_000]
        self.assertTrue(attaining)
        self.assertTrue(any(c["monetary_expectations"]["six"]["gain_numerator"] < 0 for c in attaining))
        self.assertTrue(all(c["execution_authorized"] is False and c["positive_alpha_qualified"] is False
                            for c in attaining))
        account["pending_obligations"] = [{"kind": "unreconciled_order"}]
        with self.assertRaises(AccountScenarioError):
            compile_account(account, scenarios, constraints["decision_ms"])


if __name__ == "__main__":
    unittest.main()
