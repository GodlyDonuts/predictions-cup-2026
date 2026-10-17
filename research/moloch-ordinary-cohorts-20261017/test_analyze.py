"""Targeted era attribution checks; no private data or market-value fixtures."""
import copy
import json
from pathlib import Path
import tempfile
import unittest

import analyze as M


def fixture():
    A = M.A
    def at(hm):
        return "2026-10-16T"+hm+":00.000Z"
    orders, fills = [], []
    for qid, born, executed, closed, sale in [(1, "14:30", "14:53", "14:55", "0.390000"),
                                              (3, "18:00", "18:01", "18:02", "0.405000")]:
        for ident, action, p, created, when in [(qid, "buy", "0.400000", born, executed),
                                               (qid+1, "sell", sale, executed, closed)]:
            q = {"id": ident, "marketId": "201", "exchangeId": "101", "side": "YES", "action": action,
                 "price": p, "quantity": 1, "filled": 1, "createdAt": A.clock(at(created))//10**6}
            if action == "sell":
                q["inventoryExitReason"] = "maximumAge"
            orders.append(q)
            fills.append({k: q[k] for k in ("marketId", "exchangeId", "side", "action", "price")} |
                         {"orderId": ident, "quantity": 1, "fee": "0.000000", "executedAt": at(when),
                          "at": at(when), "counterfactual": False, "actualOrderId": str(ident+10),
                          "sourceId": str(ident+20), "receiptHash": "a"*64})
    r = {"at": at("19:00"), "events": 17, "orders": 4, "fills": 4, "executionMode": "live",
         "fillModel": "actualAccountFills", "continuation": None, "positions": {}, "cash": "9.995000",
         "ownedCost": "0.000000", "fees": "0.000000", "realized": "-0.005000", "newTradingRealized": "-0.005000",
         "inheritedRealizedGross": "0.000000", "openingInventoryCost": "0.000000",
         "ownerCapitalAllocation": {"originalInitialCash": "10.000000", "effectiveInitialCash": "10.000000",
                                    "netContributions": "0.000000", "contributions": []},
         "inheritedInventory": {"managementStartedAt": at("13:00"), "historicalFillsImported": False,
                                "feesRemainInNewTradingRealized": True, "openingCost": "0.000000", "sourcePositions": []}}
    return reseal({"report": r, "orders": orders, "fills": fills, "sequence": 17,
                   "manifestHash": "a"*64, "previous": "b"*64})


def reseal(v):
    v = copy.deepcopy(v)
    v.pop("hash", None)
    v["hash"] = M.A.sha(M.A.canonical(v))
    return v


class CohortTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.evidence, _ = M.evidence_verified(Path(M.__file__).with_name("activation-evidence.json"))

    def test_old_resting_execution_after_activation_remains_old_quote_era(self):
        x = M.cohorts(fixture(), self.evidence, "a"*64)
        self.assertEqual(x["entries"][0]["releaseEra"], "pre_first_enforce_quote")
        self.assertEqual(x["entries"][1]["releaseEra"], "after_first_defensive_fresh_quote")
        self.assertEqual(x["ordinaryRealized"], "-0.005000")
        self.assertEqual(x["eras"]["pre_first_enforce_quote"]["grossRealized"], "-0.010000")
        self.assertEqual(x["eras"]["after_first_defensive_fresh_quote"]["grossRealized"], "0.005000")
        self.assertTrue(all(q["entryAdmissionGuardWitness"].startswith("unknown") for q in x["entries"]))

    def test_startup_interval_never_claims_fresh_guard_admission(self):
        v = fixture()
        v["orders"][0]["createdAt"] = M.A.clock("2026-10-16T14:40:00Z")//10**6
        x = M.cohorts(reseal(v), self.evidence, "a"*64)
        self.assertEqual(x["entries"][0]["releaseEra"], "enforce_startup_transition_unknown")

    def test_exact_original_fresh_boundary_and_missing_execution_age(self):
        m = self.evidence["milestones"][1]
        self.assertEqual(M.regime(M.A.clock(m["deployedAt"]), self.evidence), "defensive_startup_transition_unknown")
        self.assertEqual(M.regime(M.A.clock(m["firstFreshAt"]), self.evidence), "after_first_defensive_fresh_quote")
        v = fixture()
        v["fills"][1]["executedAt"] = "2026-10-16T14:52:00Z"
        # The original SELL creation still fences this fixture independently.
        v["orders"][1]["createdAt"] = M.A.clock("2026-10-16T14:31:00Z")//10**6
        x = M.cohorts(reseal(v), self.evidence, "a"*64)
        self.assertEqual(x["entries"][0]["ageMissingExecutionOrderingShares"], 1)
        self.assertEqual(x["entries"][0]["ageKnownShares"], 0)
        self.assertEqual(x["ordinaryRealized"], "-0.005000")

    def test_arbitrary_cutoff_cannot_replace_original_deployment_membership(self):
        e = copy.deepcopy(self.evidence)
        e["milestones"][0]["firstFreshAt"] = "2026-10-16T00:00:00Z"
        with tempfile.TemporaryDirectory() as directory:
            p = Path(directory)/"changed.json"
            p.write_text(json.dumps(e), encoding="utf-8")
            with self.assertRaisesRegex(M.A.AuditError, "MILESTONE_ORIGINAL_MEMBERSHIP"):
                M.evidence_verified(p)

    def test_all_entry_fill_quantities_must_match_before_cohort_analysis(self):
        v = fixture()
        v["orders"][0]["filled"] = 0
        with self.assertRaisesRegex(M.A.AuditError, "ORDER_FILLED_PARITY"):
            M.cohorts(reseal(v), self.evidence, "a"*64)


if __name__ == "__main__":
    unittest.main()
