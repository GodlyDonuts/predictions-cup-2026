"""Hand-calculated netting/fee reference and adversarial source checks."""
import copy
import unittest

import audit


def seal(v):
    v = copy.deepcopy(v)
    v.pop("hash", None)
    v["hash"] = audit.sha(audit.canonical(v))
    return v


def reference():
    # Initial10; adopted YES2*.40=.80. BUY NO3*.30 plus .01 fee:
    # net YES/NO pairs2 at payout2 => cash10.29, NO1 basis.30.
    # SELL NO1 at.35 less .02 fee => cash10.62, no inventory.
    # Gross inherited+.60, ordinary+.05, fees.03, total+.62/new+.02.
    def at(s):
        return f"2026-10-16T00:00:{s:02d}.000Z"
    orders = [{"id": 1, "exchangeId": "101", "marketId": "201", "side": "NO", "action": "buy",
               "price": "0.300000", "quantity": 3, "filled": 3, "createdAt": audit.clock(at(1))//10**6},
              {"id": 2, "exchangeId": "101", "marketId": "201", "side": "NO", "action": "sell",
               "price": "0.350000", "quantity": 1, "filled": 1, "createdAt": audit.clock(at(1))//10**6,
               "inventoryExitReason": "maximumAge"}]
    fills = []
    for q, n, fee, executed, observed in [(orders[0], 3, "0.010000", 10, 11),
                                         (orders[1], 1, "0.020000", 20, 21)]:
        fills.append({k: q[k] for k in ("exchangeId", "marketId", "side", "action", "price")} |
                     {"orderId": q["id"], "actualOrderId": str(q["id"]+10), "sourceId": str(q["id"]+20),
                      "quantity": n, "fee": fee, "executedAt": at(executed), "at": at(observed),
                      "counterfactual": False, "receiptHash": "b"*64})
    r = {"at": at(40), "executionMode": "live", "fillModel": "actualAccountFills", "events": 17,
         "orders": 2, "fills": 2, "continuation": None, "positions": {"101": {"YES": [], "NO": []}},
         "cash": "10.620000", "ownedCost": "0.000000", "fees": "0.030000", "realized": "0.620000",
         "newTradingRealized": "0.020000", "inheritedRealizedGross": "0.600000", "openingInventoryCost": "0.800000",
         "ownerCapitalAllocation": {"originalInitialCash": "10.000000", "effectiveInitialCash": "10.000000",
                                    "netContributions": "0.000000", "contributions": []},
         "inheritedInventory": {"managementStartedAt": at(0), "historicalFillsImported": False,
                                "feesRemainInNewTradingRealized": True, "openingCost": "0.800000",
                                "sourcePositions": [{"exchangeId": "101", "marketId": "201", "quantity": 2,
                                                     "lotPrincipal": "0.800000", "lots": [{"lotId": "1", "side": "YES",
                                                     "quantity": 2, "entryPrice": "0.400000", "openedAt": at(0)}]}]}}
    return seal({"report": r, "orders": orders, "fills": fills, "sequence": 17,
                 "manifestHash": "c"*64, "previous": "d"*64})


class AuditTests(unittest.TestCase):
    def test_hand_calculated_inherited_netting_partial_new_lot_and_fee_split(self):
        x = audit.audit(reference(), "a"*64)
        self.assertEqual(x["accounting"]["realized"], "0.620000")
        self.assertEqual(x["accounting"]["newTradingRealized"], "0.020000")
        self.assertEqual(x["entryClasses"]["inherited"]["grossRealized"], "0.600000")
        self.assertEqual(x["entryClasses"]["ordinary"]["grossRealized"], "0.050000")
        self.assertEqual(x["feesByExecutingOrderClass"]["ordinary"], "0.030000")
        self.assertEqual(x["entryNativeQids"][0]["closedFilledShares"], 3)
        self.assertTrue(x["entryNativeQids"][0]["filledInventoryFullyClosed"])

    def test_exact_duplicate_is_deduplicated_changed_duplicate_refuses(self):
        v = reference()
        v["fills"].append(copy.deepcopy(v["fills"][0]))
        self.assertEqual(audit.audit(seal(v), "a"*64)["duplicateRows"], 1)
        v["fills"][-1]["price"] = "0.305000"
        with self.assertRaisesRegex(audit.AuditError, "CHANGED_DUPLICATE_FILL"):
            audit.audit(seal(v), "a"*64)

    def test_late_earlier_execution_keeps_commit_fifo_money_and_missing_age(self):
        v = reference()
        v["fills"][1]["executedAt"] = "2026-10-16T00:00:09.000Z"
        x = audit.audit(seal(v), "a"*64)
        self.assertEqual(x["accounting"]["realized"], "0.620000")
        self.assertEqual(x["executionClockInversionsPreserved"], 1)
        close = next(b for b in x["closedFifoByExit"] if b["mechanism"] == "sell")
        self.assertEqual(close["ageMissingExecutionOrderingShares"], 1)
        self.assertEqual(close["ageKnownShares"], 0)

    def test_pre_adoption_fill_requires_original_events_not_seed_reordering(self):
        v = reference()
        v["report"]["inheritedInventory"]["managementStartedAt"] = "2026-10-16T00:00:12.000Z"
        with self.assertRaisesRegex(audit.AuditError, "NEED_ORIGINAL_EVENT_CHRONOLOGY"):
            audit.audit(seal(v), "a"*64)

    def test_counterfactual_uncovered_sale_and_missing_economic_transition_refuse(self):
        for case, code in [("counterfactual", "COUNTERFACTUAL_FILL"), ("uncovered", "UNCOVERED_CLOSE"),
                           ("fee", "NATIVE_PARITY"), ("packages", "PACKAGE_ATTRIBUTION")]:
            with self.subTest(case=case):
                v = reference()
                if case == "counterfactual":
                    v["fills"][0]["counterfactual"] = True
                elif case == "uncovered":
                    v["fills"][1]["quantity"] = 4
                    v["orders"][1]["quantity"] = v["orders"][1]["filled"] = 4
                elif case == "fee":
                    v["fills"][1]["fee"] = "0.010000"
                else:
                    v["report"]["livePackages"] = {"records": [{}]}
                with self.assertRaisesRegex(audit.AuditError, code):
                    audit.audit(seal(v), "a"*64)

    def test_sealed_report_mutation_and_endpoint_inventory_mismatch_refuse(self):
        v = reference()
        v["report"]["cash"] = "100.000000"
        with self.assertRaisesRegex(audit.AuditError, "SEALED_REPORT_HASH"):
            audit.audit(v, "a"*64)
        v = reference()
        v["report"]["positions"]["101"]["NO"] = [{"quantity": 1, "price": "0.300000"}]
        with self.assertRaisesRegex(audit.AuditError, "EXACT_ENDPOINT_FIFO_POSITIONS"):
            audit.audit(seal(v), "a"*64)


if __name__ == "__main__":
    unittest.main()
