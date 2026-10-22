"""Matched synthetic source packets; no API or physical permission."""
from copy import deepcopy
from hashlib import sha256
import json
import unittest

from .current_portfolio import compile_current, digest


def seal(v):
    body = json.dumps(v, sort_keys=True, separators=(",", ":")).encode()
    return (json.dumps({**v, "hash": sha256(body).hexdigest()}, sort_keys=True,
                       separators=(",", ":")) + "\n").encode()


def fixture():
    h = "a" * 64
    positions = {"x": {"YES": [{"quantity": 2, "price": "0.200000"}], "NO": []},
                 "y": {"YES": [], "NO": []}, "z": {"YES": [], "NO": []}}
    fills = [{"sourceId": str(i), "counterfactual": False, "exchangeId": "x", "side": side,
              "action": action, "quantity": 1} for i, (side, action) in enumerate(
                  [("YES", "sell"), ("NO", "buy")])]
    head = json.loads(seal({"manifestHash": h, "sequence": 7, "previous": "b" * 64}))
    source = {"receiptHash": "c" * 64, "freshMarketLimit": 1,
              "managementStartedAt": "2026-10-15T08:44:12.730Z",
              "sourcePositions": [{"exchangeId": "x", "basisClassification": "unknown",
                  "lots": [{"quantity": 4, "entryPrice": "0.200000", "side": "YES"}],
                  "metadata": {"reportedAverageCost": 0.2}}]}
    active = {"id": 1, "retired": False, "exchangeId": "y"}
    report = {"manifestHash": h, "sequence": 7, "previous": "b" * 64,
              "fills": fills, "orders": [active], "report": {"positions": positions,
              "cash": "7.000000", "reservedCash": "1.000000", "ownedCost": "0.400000",
              "at": "2026-10-18T11:00:00Z", "inheritedInventory": source}}
    original = {"schemaVersion": 1, "producer": "moloch_original_cohort_owner",
        "exchangeId": "x", "generationSha256": "d" * 64, "bankManifestHash": h,
        "handoffEstablished": False,
        "positionsSha256": digest(positions["x"])}
    body = json.dumps(original, separators=(",", ":"))
    ownership = {**original, "sourceBodyText": body, "sourceSha256": sha256(body.encode()).hexdigest(),
                 "positions": deepcopy(positions["x"]), "rankPaused": False}
    view = {"schema": "tartarus_native_research_view_v1", "researchOnly": True,
        "executionAuthority": False, "nativeHead": head, "manifestHash": h,
        "writtenAt": "2026-10-18T11:00:01Z", "generationSha256": "d" * 64,
        "account": {"positions": positions, "cash": "7.000000", "reservedCash": "1.000000",
                    "fillCount": 2, "actualFillsSha256": digest(fills), "positionsSha256": digest(positions)},
        "scope": {"exchanges": {"x": "mx", "y": "my", "z": "mz"}},
        "managedOwnership": [ownership], "activeNativeOrders": [active],
        "managedTargetAdmissionProjections": {"x": {}, "z": {}},
        "policy": {"maximumCommitted": "10.000000", "maximumMarketCost": "5.000000",
                   "maximumMarkets": 3, "quantity": 10, "feePerShare": "0.000000", "feeBps": 0},
        "unresolvedOrdinaryAttempt": False, "knownUnclosedAcknowledgments": [],
        "books": [{"exchangeId": "x", "receivedAt": "2026-10-18T10:59:59Z", "invalid": True}],
        "originalObservationReceiptClocks": [{"receivedAt": "2026-10-18T10:59:58Z"}]}
    report["report"]["policyHash"] = digest(view["policy"])
    registry = [{"exchangeId": ex, "contractId": ex, "marketId": "m" + ex} for ex in ("x", "y", "z")]
    return report, view, registry


def compile_fixture(report, view, registry):
    r = seal(report)
    v = json.dumps(view, separators=(",", ":")).encode()
    return compile_current(r, v, expected_report_sha256=sha256(r).hexdigest(),
        expected_view_sha256=sha256(v).hexdigest(), registry=registry, conditional_future_legs=128)


class CurrentPortfolioTests(unittest.TestCase):
    def test_owner_produced_same_side_handoff_removes_sale_prerequisite_without_changing_money(self):
        report, view, registry = fixture()
        owner = view['managedOwnership'][0]
        source = json.loads(owner['sourceBodyText'])
        source.update(sameSideBuyHandoffAllowed=True, sameSideBuyHandoffSide='YES')
        raw = json.dumps(source, separators=(',', ':'))
        owner.update(source, sourceBodyText=raw, sourceSha256=sha256(raw.encode()).hexdigest())
        out = compile_fixture(report, view, registry)
        self.assertEqual(out['financial']['buyRequiresPriorCoveredSellExchanges'], [])
        self.assertEqual(out['financial']['cash'], '7.000000')
        self.assertEqual(out['financial']['ownedCost'], '0.400000')
        self.assertEqual(out['inheritedAttribution']['basisClassifications']['x'], 'unknown')
        source['sameSideBuyHandoffSide'] = 'NO'
        raw = json.dumps(source, separators=(',', ':'))
        owner.update(source, sourceBodyText=raw, sourceSha256=sha256(raw.encode()).hexdigest())
        with self.assertRaisesRegex(ValueError, 'CURRENT_OWNERSHIP_HANDOFF_SIDE'):
            compile_fixture(report, view, registry)

    def test_actual_sell_and_opposite_buy_preserve_remaining_inherited_fifo(self):
        report, view, registry = fixture()
        before = deepcopy((report, view))
        out = compile_fixture(report, view, registry)
        self.assertEqual(out["status"], "available_research_input")
        self.assertEqual(out["financial"]["inheritedExchanges"], ["x"])
        self.assertEqual(out["financial"]["inheritedRemainingQuantities"], {"x": 2})
        self.assertEqual(out["financial"]["ownedCost"], "0.400000")
        self.assertEqual(out["financial"]["reservedCash"], "1.000000")
        self.assertEqual(out["financial"]["retainedMarketExchanges"], ["y"])
        self.assertEqual(out["financial"]["excludedBuyExchanges"], ["z"])
        self.assertEqual(out["financial"]["buyRequiresPriorCoveredSellExchanges"], ["x"])
        self.assertEqual(out["inheritedAttribution"]["basisClassifications"]["x"], "unknown")
        self.assertEqual(out["originalBooks"], view["books"])
        self.assertEqual(out["originalObservationReceiptClocks"], view["originalObservationReceiptClocks"])
        self.assertEqual((report, view), before)
        self.assertFalse(out["executionAuthority"])
        self.assertFalse(out["retirementPerformed"])
        json.dumps(out)  # Original metadata floats do not become unserializable Decimal projections.

    def test_mismatched_native_head_or_positions_cannot_plan(self):
        report, view, registry = fixture()
        view["nativeHead"] = json.loads(seal({"manifestHash": "a" * 64, "sequence": 8, "previous": "b" * 64}))
        with self.assertRaisesRegex(ValueError, "MATCHED_NATIVE_BOUNDARY_REQUIRED"):
            compile_fixture(report, view, registry)
        report, view, registry = fixture()
        view["account"]["positions"] = {}
        with self.assertRaisesRegex(ValueError, "CURRENT_FINANCIAL_BINDING"):
            compile_fixture(report, view, registry)

    def test_known_orders_preserve_no_fill_branch_but_unknown_acceptance_is_unavailable(self):
        report, view, registry = fixture()
        view["knownUnclosedAcknowledgments"] = [9]
        out = compile_fixture(report, view, registry)
        self.assertEqual(out["status"], "conditional_research_input_known_orders")
        self.assertEqual(out["financial"]["cash"], "7.000000")
        self.assertEqual(out["financial"]["reservedCash"], "1.000000")
        self.assertIn("ORIGINAL_PHYSICAL_ORDERS_REQUIRE_RECONCILIATION", out["physicalDeferrals"])
        self.assertFalse(out["allocationBoundaryQualified"])
        view["unresolvedOrdinaryAttempt"] = True
        out = compile_fixture(report, view, registry)
        self.assertEqual(out["status"], "unavailable")
        self.assertIsNone(out["financial"])
        self.assertIn("ORIGINAL_UNRESOLVED_ATTEMPT", out["unavailableReasons"])

    def test_missing_holding_and_mutated_original_ownership_reject(self):
        report, view, registry = fixture()
        with self.assertRaisesRegex(ValueError, "CURRENT_UNMODELED_HOLDING"):
            compile_fixture(report, view, registry[1:])
        view["managedOwnership"][0]["sourceBodyText"] += " "
        with self.assertRaisesRegex(ValueError, "ORIGINAL_OWNERSHIP_BODY"):
            compile_fixture(report, view, registry)

    def test_original_inherited_prefix_cannot_be_relabelled_as_new_principal(self):
        report, view, registry = fixture()
        report["report"]["inheritedInventory"]["sourcePositions"][0]["lots"][0]["entryPrice"] = "0.300000"
        with self.assertRaisesRegex(ValueError, "INHERITED_FIFO_PREFIX_CHANGED"):
            compile_fixture(report, view, registry)

    def test_policy_and_contract_mapping_are_bound_to_original_source(self):
        report, view, registry = fixture()
        view["policy"]["maximumMarketCost"] = "500.000000"
        with self.assertRaisesRegex(ValueError, "CURRENT_POLICY_BINDING"):
            compile_fixture(report, view, registry)
        report, view, registry = fixture()
        registry[0]["marketId"] = "wrong-race"
        with self.assertRaisesRegex(ValueError, "CURRENT_CONTRACT_MAPPING_CHANGED"):
            compile_fixture(report, view, registry)


if __name__ == "__main__":
    unittest.main()
