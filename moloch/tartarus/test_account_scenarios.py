"""Synthetic Contract A invariants; no live account, source, or probability claim."""
from copy import deepcopy
import unittest
import account_scenarios as A

H = "a" * 64


def evidence(label="synthetic"):
    return {"schema_version": 1, "evidence_class": label, "as_of_ms": 1000,
            "valid_until_ms": 3000, "source_sha256": H,
            "unmodeled_state_statement": "Synthetic declared states only; exceptions may be omitted."}


def holding(i=0, side="YES", lots=None):
    lots = [{"quantity": 1, "unit_cost_micros": 400_000}] if lots is None else lots
    return {"contract_id": f"c{i}", "exchange_id": f"e{i}", "side": side,
            "quantity": sum(l["quantity"] for l in lots), "lots": lots, "source_sha256": H}


def fixture(n=23):
    account = {**evidence(), "cash_micros": 2_000_000, "reserved_cash_micros": 1_000_000,
               "terminal_debt_micros": 500_000, "holdings": [holding(i) for i in range(n)],
               "pending_obligations": []}
    scenarios = {**evidence(), "contracts": [
        {"contract_id": f"c{i}", "exchange_id": f"e{i}", "semantic_source_sha256": H}
        for i in range(n)], "scenarios": [
            {"id": "joint_yes", "kind": "ordinary", "payouts": {f"c{i}": "resolved_yes" for i in range(n)},
             "assumption_label": "Joint all-YES declared outcome; no independence assumption."},
            {"id": "joint_no", "kind": "ordinary", "payouts": {f"c{i}": "resolved_no" for i in range(n)},
             "assumption_label": "Joint all-NO declared outcome."},
            {"id": "refund", "kind": "exceptional", "payouts": {f"c{i}": "refund_principal" for i in range(n)},
             "assumption_label": "Declared original-principal refund stress, not universal rules."}]}
    return account, scenarios


class AccountScenariosTests(unittest.TestCase):
    def reject(self, account, scenarios, code, at=2000):
        with self.assertRaises(A.AccountScenarioError) as ctx:
            A.compile_account(account, scenarios, at)
        self.assertEqual(ctx.exception.code, code)

    def test_whole_account_23_positions(self):
        a, s = fixture()
        original = deepcopy((a, s))
        out = A.compile_account(a, s, 2000)
        self.assertEqual(len(out["holdings"]), 23)
        self.assertEqual(out["scenario_ids"], ["joint_yes", "joint_no", "refund"])
        self.assertEqual(out["baseline_wealth_micros"], [24_500_000, 1_500_000, 10_700_000])
        self.assertEqual(out["available_cash_micros"], 1_000_000)
        self.assertFalse(out["execution_authorized"])
        self.assertFalse(out["calibrated_probability_claimed"])
        self.assertFalse(out["scenario_matrix_exhaustive"])
        self.assertEqual((a, s), original)
        out["holdings"][0]["lots"][0]["quantity"] = 999
        self.assertEqual((a, s), original)

    def test_unmodeled_holding_and_missing_payout_reject(self):
        a, s = fixture()
        s["contracts"].pop()
        for row in s["scenarios"]:
            row["payouts"].pop("c22")
        self.reject(a, s, "HELD_CONTRACT_NOT_IN_EXACT_REGISTRY")
        a, s = fixture()
        s["scenarios"][1]["payouts"].pop("c22")
        self.reject(a, s, "SCENARIO_REGISTRY_COVERAGE")

    def test_wrong_exchange_duplicate_holdings_and_registry_alias_reject(self):
        a, s = fixture(2)
        a["holdings"][0]["exchange_id"] = "e1"
        self.reject(a, s, "HELD_CONTRACT_NOT_IN_EXACT_REGISTRY")
        a, s = fixture(2)
        a["holdings"].append(deepcopy(a["holdings"][0]))
        self.reject(a, s, "DUPLICATE_OR_CONFLICTING_HOLDING")
        a, s = fixture(2)
        s["contracts"][1]["exchange_id"] = "e0"
        self.reject(a, s, "CONTRACT_IDENTITY_CONFLICT")

    def test_fifo_refund_after_partial_sale_uses_only_remaining_principal(self):
        h = holding(lots=[{"quantity": 3, "unit_cost_micros": 900_000},
                          {"quantity": 5, "unit_cost_micros": 200_000}])
        original = deepcopy(h)
        sale = A.fifo_sell(h, 4)
        self.assertEqual(sale["removed_basis_micros"], 2_900_000)
        self.assertEqual(sale["remaining_lots"], [{"quantity": 4, "unit_cost_micros": 200_000}])
        self.assertEqual(sale["remaining_basis_micros"], 800_000)
        self.assertEqual(h, original)
        a, s = fixture(1)
        a["holdings"] = [{**h, "quantity": sale["remaining_quantity"], "lots": sale["remaining_lots"]}]
        out = A.compile_account(a, s, 2000)
        self.assertEqual(out["baseline_wealth_micros"][2], 2_300_000)
        self.assertEqual(out["cash_micros"], 2_000_000, "a hypothetical sale never supplies cash")

    def test_fifo_zero_full_and_excess_sale_are_explicit(self):
        h = holding()
        zero = A.fifo_sell(h, 0)
        self.assertEqual(zero["remaining_lots"], h["lots"])
        self.assertEqual(zero["removed_basis_micros"], 0)
        full = A.fifo_sell(h, 1)
        self.assertEqual(full["remaining_lots"], [])
        self.assertEqual(full["remaining_basis_micros"], 0)
        with self.assertRaisesRegex(A.AccountScenarioError, "FIFO_SELL_EXCEEDS_HOLDING"):
            A.fifo_sell(h, 2)

    def test_cash_and_debt_counted_once_reservations_are_not_terminal_expense(self):
        a, s = fixture(1)
        out = A.compile_account(a, s, 2000)
        a["reserved_cash_micros"] = 0
        other = A.compile_account(a, s, 2000)
        self.assertEqual(out["baseline_wealth_micros"], other["baseline_wealth_micros"])
        self.assertEqual(out["baseline_wealth_micros"][1], 1_500_000)
        self.assertEqual(other["available_cash_micros"] - out["available_cash_micros"], 1_000_000)
        a["terminal_debt_micros"] = 3_000_000
        self.assertEqual(A.compile_account(a, s, 2000)["baseline_wealth_micros"][1], -1_000_000)

    def test_pending_unknown_and_missing_balances_are_unavailable_not_zero(self):
        a, s = fixture(1)
        a["pending_obligations"] = [{"kind": "unconfirmed_order", "quantity": None}]
        self.reject(a, s, "PENDING_OBLIGATIONS_UNAVAILABLE")
        a["pending_obligations"] = None
        self.reject(a, s, "PENDING_OBLIGATION_SHAPE_OR_BOUND")
        a, s = fixture(1)
        a["terminal_debt_micros"] = None
        self.reject(a, s, "TERMINAL_DEBT_AMOUNT")
        a["terminal_debt_micros"] = 0
        a.pop("reserved_cash_micros")
        self.reject(a, s, "ACCOUNT_SHAPE")

    def test_both_sides_are_explicit_not_netted_or_dropped(self):
        a, s = fixture(1)
        a["holdings"] = [holding(side="YES", lots=[{"quantity": 3, "unit_cost_micros": 600_000}]),
                         holding(side="NO", lots=[{"quantity": 2, "unit_cost_micros": 500_000}])]
        self.assertEqual(A.compile_account(a, s, 2000)["baseline_wealth_micros"],
                         [4_500_000, 3_500_000, 4_300_000])

    def test_joint_race_dependence_and_supplied_order_are_preserved(self):
        a, s = fixture(2)
        a.update(cash_micros=0, reserved_cash_micros=0, terminal_debt_micros=0)
        correlated = A.compile_account(a, s, 2000)
        self.assertEqual(correlated["baseline_wealth_micros"][:2], [2_000_000, 0])
        s["scenarios"][0]["payouts"]["c1"] = "resolved_no"
        s["scenarios"][1]["payouts"]["c1"] = "resolved_yes"
        s["scenarios"] = list(reversed(s["scenarios"]))
        out = A.compile_account(a, s, 2000)
        self.assertEqual(out["scenario_ids"], ["refund", "joint_no", "joint_yes"])
        self.assertEqual(out["baseline_wealth_micros"], [800_000, 1_000_000, 1_000_000])
        self.assertNotEqual(out["source_sha256"], correlated["source_sha256"])

    def test_future_expired_equal_boundary_and_clock_types_reject(self):
        for label, at, code in [("future", 999, "ACCOUNT_SOURCE_FUTURE"),
                                ("expired", 3000, "ACCOUNT_SOURCE_EXPIRED"),
                                ("bool", True, "DECISION_CLOCK"),
                                ("float", 2000.0, "DECISION_CLOCK")]:
            with self.subTest(label=label):
                a, s = fixture(1)
                self.reject(a, s, code, at)
        a, s = fixture(1)
        s["as_of_ms"] = 2001
        self.reject(a, s, "SCENARIOS_SOURCE_FUTURE")

    def test_integer_amounts_no_float_bool_or_unbounded_product(self):
        for value in [True, 1.0, -1, None]:
            a, s = fixture(1)
            a["cash_micros"] = value
            self.reject(a, s, "CASH_AMOUNT")
        a, s = fixture(1)
        a["holdings"][0]["lots"][0]["quantity"] = True
        self.reject(a, s, "LOT_QUANTITY")
        a, s = fixture(1)
        a["holdings"][0]["quantity"] = 2
        a["holdings"][0]["lots"] = [{"quantity": 2, "unit_cost_micros": A.MAX_INTEGER}]
        self.reject(a, s, "HOLDING_BASIS_RANGE")

    def test_source_semantics_shape_and_explicit_unavailable_statements_required(self):
        a, s = fixture(1)
        s["contracts"][0]["semantic_source_sha256"] = None
        self.reject(a, s, "CONTRACT_SEMANTIC_SOURCE_SHA256")
        a, s = fixture(1)
        a["unmodeled_state_statement"] = ""
        self.reject(a, s, "ACCOUNT_UNMODELED_STATEMENT")
        a, s = fixture(1)
        a["cash_credit_from_unfilled_order"] = 5_000_000
        self.reject(a, s, "ACCOUNT_SHAPE")

    def test_unknown_resolution_missing_quantity_and_duplicate_scenario_fail(self):
        a, s = fixture(1)
        s["scenarios"][0]["payouts"]["c0"] = "pending"
        self.reject(a, s, "UNKNOWN_SCENARIO_PAYOUT")
        a, s = fixture(1)
        a["holdings"][0]["quantity"] = 2
        self.reject(a, s, "HOLDING_FIFO_QUANTITY_MISMATCH")
        a, s = fixture(1)
        s["scenarios"][1]["id"] = "joint_yes"
        self.reject(a, s, "DUPLICATE_SCENARIO_ID")

    def test_registry_contracts_can_be_flat_but_each_declared_payout_is_exact(self):
        a, s = fixture(2)
        a["holdings"] = []
        out = A.compile_account(a, s, 2000)
        self.assertEqual(out["baseline_wealth_micros"], [1_500_000] * 3)
        s["scenarios"][0]["payouts"]["foreign"] = "resolved_yes"
        self.reject(a, s, "SCENARIO_REGISTRY_COVERAGE")

    def test_dimension_cell_bounds_and_mixed_evidence_are_explicit(self):
        a, s = fixture(1)
        s["evidence_class"] = "sealed_original_but_semantics_assumed"
        out = A.compile_account(a, s, 2000)
        self.assertEqual(out["evidence_class"], "mixed")
        self.assertEqual(out["provenance"]["account_evidence_class"], "synthetic")
        self.assertFalse(out["provenance"]["source_body_membership_independently_verified"])
        a, s = fixture(A.MAX_CONTRACTS + 1)
        self.reject(a, s, "CONTRACT_BOUND")
        a, s = fixture(A.MAX_CONTRACTS)
        s["scenarios"] = [s["scenarios"][0]] * 513
        self.reject(a, s, "SCENARIO_CELL_BOUND")


if __name__ == "__main__":
    unittest.main()
