"""Synthetic sealed reports and declared source evidence; no account API calls."""
from copy import deepcopy
import hashlib
import json
import unittest
import account_scenarios as A
import native_snapshot as N

H = "a" * 64
P = "b" * 64

def seal(value):
    body = json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()
    return (json.dumps({**value, "hash": hashlib.sha256(body).hexdigest()}, sort_keys=True,
                       separators=(",", ":"), allow_nan=False) + "\n").encode()


def common():
    return {"schema_version": 1, "evidence_class": "synthetic", "as_of_ms": 1001,
            "valid_until_ms": 3000, "source_sha256": H,
            "unmodeled_state_statement": "Synthetic source assertion, not independent receipt or semantic verification."}


def fixture(n=23, active=False):
    positions = {str(i): {"YES": [{"quantity": 2, "price": "0.400000", "executedAt": "1970-01-01T00:00:00.500Z"}], "NO": []}
                 for i in range(1, n + 1)}
    positions["zero"] = {"YES": [], "NO": [{"quantity": 0, "price": "0.900000"}]}
    report = {"at": "1970-01-01T00:00:01.000Z", "cash": "10.000000", "reservedCash": "2.000000",
              "ownedCost": f"{n * 8 // 10}.{n * 8 % 10}00000", "positions": positions,
              "activeOrders": 1 if active else 0, "unresolvedOrders": 1 if active else 0}
    d = {"manifestHash": H, "sequence": 7, "previous": P, "report": report,
         "orders": [{"id": 1, "retired": not active, "quantity": 2, "filled": 0}],
         "fills": [], "contestOrders": 0, "sourceCursors": {"fixture": 7}}
    raw = seal(d)
    head = json.loads(seal({"manifestHash": H, "sequence": 7, "previous": P}))
    args = {"expected_report_sha256": hashlib.sha256(raw).hexdigest(), "expected_manifest_sha256": H,
            "expected_head": head, "source_observation_ms": 1001, "decision_ms": 2000, "valid_until_ms": 3000}
    registry = {**common(), "bank_manifest_hash": H, "contracts": [
        {"contract_id": f"official-{i}", "exchange_id": str(i), "market_id": f"market-{i}", "semantic_source_sha256": P}
        for i in range(1, n + 1)]}
    liability = {**common(), "report_sha256": args["expected_report_sha256"], "head": head,
                 "terminal_debt_micros": 500_000, "pending_obligations": [], "physical_open_orders": [],
                 "unknown_acceptance": False, "unresolved_operations": []}
    return d, raw, args, registry, liability


def changed(d, args, **fields):
    out = deepcopy(d)
    out["report"].update(fields)
    raw = seal(out)
    return out, raw, {**args, "expected_report_sha256": hashlib.sha256(raw).hexdigest()}


class NativeSnapshotTests(unittest.TestCase):
    def test_complete_23_holdings_to_contract_a_with_exact_source_boundaries(self):
        d, raw, args, registry, liabilities = fixture()
        result = N.adapt_native_snapshot(raw, registry=registry, liabilities=liabilities, **args)
        self.assertEqual(result["status"], "available")
        a = result["account_input"]
        self.assertEqual(a["evidence_class"], "synthetic")
        self.assertEqual(len(a["holdings"]), 23)
        self.assertEqual(a["holdings"][0]["contract_id"], "official-1")
        self.assertEqual(a["holdings"][0]["lots"], [{"quantity": 2, "unit_cost_micros": 400_000}])
        self.assertEqual(a["cash_micros"], 10_000_000)
        self.assertEqual(a["reserved_cash_micros"], 2_000_000)
        self.assertEqual(a["terminal_debt_micros"], 500_000)
        self.assertEqual(result["snapshot"]["at_original"], d["report"]["at"])
        self.assertEqual(result["original_lot_clocks"][0]["executed_at_original"], ["1970-01-01T00:00:00.500Z"])
        scenarios = {**common(), "contracts": result["contracts"], "scenarios": [
            {"id": "refund", "kind": "exceptional", "payouts": {c["contract_id"]: "refund_principal" for c in result["contracts"]}, "assumption_label": "Declared principal-refund stress."}]}
        compiled = A.compile_account(a, scenarios, 2000)
        self.assertEqual(compiled["baseline_wealth_micros"], [27_900_000])
        self.assertFalse(result["execution_authorized"])
        self.assertFalse(result["calibrated_probability_claimed"])
        self.assertFalse(result["official_receipt_membership_independently_verified"])

    def test_missing_registry_and_debt_keep_only_dated_static_bounds(self):
        d, raw, args, _, _ = fixture()
        result = N.adapt_native_snapshot(raw, **args)
        self.assertEqual(result["status"], "unavailable")
        self.assertIsNone(result["account_input"])
        self.assertIsNone(result["terminal_debt_micros"])
        self.assertEqual(result["unavailable_reasons"], ["REGISTRY_UNAVAILABLE", "LIABILITY_EVIDENCE_UNAVAILABLE"])
        static = result["static_held_bounds"]
        self.assertEqual(static["held_sides"], 23)
        self.assertEqual(static["absolute_shares"], 46)
        self.assertEqual(static["remaining_fifo_principal_micros"], 18_400_000)
        self.assertEqual(static["cash_plus_all_sides_pay_one_ceiling_micros"], 56_000_000)
        self.assertEqual(static["explicit_zero_lots_skipped"], 1)

    def test_native_draft_not_silently_classified_as_physical_order_or_zero_liability(self):
        _, raw, args, registry, liabilities = fixture(active=True)
        result = N.adapt_native_snapshot(raw, registry=registry, liabilities=liabilities, **args)
        self.assertIn("NATIVE_OPEN_ORDER_CLASSIFICATION_UNAVAILABLE", result["unavailable_reasons"])
        self.assertIsNone(result["account_input"])
        self.assertEqual(result["static_held_bounds"]["native_active_orders"], 1)
        self.assertEqual(liabilities["physical_open_orders"], [])

    def test_raw_pin_seal_expected_head_and_manifest_are_independent_guards(self):
        d, raw, args, _, _ = fixture(1)
        with self.assertRaisesRegex(N.NativeSnapshotError, "REPORT_RAW_SHA256"):
            N.adapt_native_snapshot(raw + b" ", **args)
        corrupt = raw.replace(b'"10.000000"', b'"11.000000"')
        with self.assertRaisesRegex(N.NativeSnapshotError, "REPORT_RAW_BODY_SEAL"):
            N.adapt_native_snapshot(corrupt, **{**args, "expected_report_sha256": hashlib.sha256(corrupt).hexdigest()})
        bad = json.loads(seal({"manifestHash": H, "sequence": 8, "previous": P}))
        with self.assertRaisesRegex(N.NativeSnapshotError, "REPORT_EXPECTED_BOUNDARY"):
            N.adapt_native_snapshot(raw, **{**args, "expected_head": bad})
        with self.assertRaisesRegex(N.NativeSnapshotError, "EXPECTED_HEAD_MANIFEST"):
            N.adapt_native_snapshot(raw, **{**args, "expected_manifest_sha256": "c" * 64})

    def test_refund_uses_remaining_fifo_not_reconstructed_original_pre_sale_basis(self):
        d, _, args, registry, liabilities = fixture(1)
        positions = {"1": {"YES": [{"quantity": 4, "price": "0.200000", "executedAt": None}], "NO": []}}
        _, raw, args = changed(d, args, positions=positions, ownedCost="0.800000")
        liabilities["report_sha256"] = args["expected_report_sha256"]
        out = N.adapt_native_snapshot(raw, registry=registry, liabilities=liabilities, **args)
        h = out["account_input"]["holdings"][0]
        self.assertEqual(A.fifo_sell(h, 1)["remaining_basis_micros"], 600_000)
        self.assertEqual(out["static_held_bounds"]["remaining_fifo_principal_micros"], 800_000)

    def test_unmapped_contract_unknown_debt_and_open_official_evidence_are_unavailable(self):
        _, raw, args, registry, liabilities = fixture(2)
        registry["contracts"].pop()
        out = N.adapt_native_snapshot(raw, registry=registry, liabilities=liabilities, **args)
        self.assertIn("EXACT_HELD_CONTRACT_REGISTRY_UNAVAILABLE", out["unavailable_reasons"])
        _, raw, args, registry, liabilities = fixture(1)
        liabilities["terminal_debt_micros"] = None
        out = N.adapt_native_snapshot(raw, registry=registry, liabilities=liabilities, **args)
        self.assertIn("LIABILITY_EVIDENCE_FIELDS_UNAVAILABLE", out["unavailable_reasons"])
        self.assertIsNone(out["terminal_debt_micros"])
        liabilities["terminal_debt_micros"] = 0
        for field, value, code in [("physical_open_orders", [{"source_sha256": H}], "PHYSICAL_OPEN_ORDERS_UNAVAILABLE"),
                                    ("unknown_acceptance", True, "UNKNOWN_ACCEPTANCE_UNAVAILABLE")]:
            changed_liabilities = {**liabilities, field: value}
            out = N.adapt_native_snapshot(raw, registry=registry, liabilities=changed_liabilities, **args)
            self.assertIn(code, out["unavailable_reasons"])

    def test_original_observation_and_execution_clock_future_expiry_missing_do_not_renew(self):
        d, raw, args, registry, liabilities = fixture(1)
        for field, value, reason in [("source_observation_ms", None, "SOURCE_OBSERVATION_UNAVAILABLE"),
                                     ("source_observation_ms", 2001, "SOURCE_OBSERVATION_FUTURE"),
                                     ("source_observation_ms", 999, "SOURCE_OBSERVATION_PRECEDES_SNAPSHOT"),
                                     ("valid_until_ms", 2000, "SNAPSHOT_EXPIRED")]:
            out = N.adapt_native_snapshot(raw, registry=registry, liabilities=liabilities, **{**args, field: value})
            self.assertIn(reason, out["unavailable_reasons"])
            self.assertIsNone(out["account_input"])
        d["report"]["positions"]["1"]["YES"][0]["executedAt"] = "1970-01-01T00:00:01.000000001Z"
        raw = seal(d)
        with self.assertRaisesRegex(N.NativeSnapshotError, "NATIVE_LOT_EXECUTION_FUTURE"):
            N.adapt_native_snapshot(raw, **{**args, "expected_report_sha256": hashlib.sha256(raw).hexdigest()})

    def test_scope_mapping_is_not_contract_identity_and_old_liability_clock_is_unavailable(self):
        _, raw, args, registry, liabilities = fixture(1)
        registry["contracts"][0]["contract_id"] = None
        registry["contracts"][0]["semantic_source_sha256"] = None
        out = N.adapt_native_snapshot(raw, registry=registry, liabilities=liabilities, **args)
        self.assertIn("EXACT_REGISTRY_IDENTITY_UNAVAILABLE", out["unavailable_reasons"])
        self.assertIsNone(out["account_input"])
        _, raw, args, registry, liabilities = fixture(1)
        liabilities["as_of_ms"] = 999
        out = N.adapt_native_snapshot(raw, registry=registry, liabilities=liabilities, **args)
        self.assertIn("LIABILITY_OBSERVATION_PRECEDES_SNAPSHOT", out["unavailable_reasons"])
        self.assertIsNone(out["account_input"])

    def test_source_membership_not_fabricated_and_derived_source_binds_liability_inputs(self):
        _, raw, args, registry, liabilities = fixture(1)
        before = deepcopy((registry, liabilities))
        one = N.adapt_native_snapshot(raw, registry=registry, liabilities=liabilities, **args)
        liabilities["terminal_debt_micros"] += 1
        two = N.adapt_native_snapshot(raw, registry=registry, liabilities=liabilities, **args)
        self.assertNotEqual(one["account_input"]["source_sha256"], two["account_input"]["source_sha256"])
        self.assertFalse(one["registry_semantics_independently_verified"])
        self.assertEqual(registry, before[0])
        one["account_input"]["holdings"][0]["lots"][0]["quantity"] = 999
        self.assertEqual(two["account_input"]["holdings"][0]["quantity"], 2)

    def test_bad_basis_prices_types_and_zero_side_shape_reject(self):
        d, _, args, _, _ = fixture(1)
        for field, value, code in [("price", "1.000001", "NATIVE_LOT_PRICE_EXCEEDS_PAYOUT"),
                                   ("quantity", True, "NATIVE_LOT_QUANTITY"),
                                   ("price", 0.4, "NATIVE_LOT_PRICE")]:
            copy = deepcopy(d)
            copy["report"]["positions"]["1"]["YES"][0][field] = value
            raw = seal(copy)
            with self.assertRaisesRegex(N.NativeSnapshotError, code):
                N.adapt_native_snapshot(raw, **{**args, "expected_report_sha256": hashlib.sha256(raw).hexdigest()})
        _, raw, args = changed(d, args, ownedCost="0.700000")
        with self.assertRaisesRegex(N.NativeSnapshotError, "NATIVE_REMAINING_BASIS_MISMATCH"):
            N.adapt_native_snapshot(raw, **args)


if __name__ == "__main__":
    unittest.main()
