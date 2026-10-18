import unittest
from copy import deepcopy
from .sequence_audit import audit


def fixture():
    financial = {"cash": "0", "ownedCost": "1", "positions": {
        "a": {"YES": [{"quantity": 1, "price": ".9"}, {"quantity": 1, "price": ".1"}]}},
        "conditionalFutureLegs": 128, "physicalOrderQuantity": 10000,
        "inheritedExchanges": [], "freshMarketLimit": 12,
        "maximumCommitted": "100", "maximumMarketCost": "100", "maximumMarkets": 40}
    registry = [{"exchangeId": "a", "contractId": "1"}]
    books = {"a": {"bids": [{"price": ".5", "quantity": 2}],
                   "asks": [{"price": ".6", "quantity": 10}]}}
    model = {"scenarios": [{"id": "refund", "payouts": {"1": "refund_principal"}},
                           {"id": "yes", "payouts": {"1": "resolved_yes"}}],
             "probabilityModels": [{"id": "p", "weightsPpm": [500000, 500000]}]}
    fields = [{"id": "unknown", "completeField": False, "thresholds": ["1", "1"]}]
    assumptions = {"zeroFees": True, "zeroTerminalDebt": True,
        "conditionalReservationTreatment": "Synthetic no active reservations",
        "datedBooksAreFutureLiquidityHypotheses": True}
    return dict(financial=financial, registry=registry, books=books, model=model,
                fields=fields, legs=[], assumptions=assumptions)


class AuditTests(unittest.TestCase):
    def test_fifo_refund_interior_breakpoint_and_strict_ties(self):
        args = fixture()
        before = deepcopy(args)
        args["legs"] = [{"ex": "a", "side": "YES", "action": "SELL", "quantity": 2, "price": 500000}]
        result = audit(**args)
        self.assertEqual(result["fifoBreakpointsScored"], 2)
        self.assertEqual(result["minimumPrefixWealthMicros"], 600000)
        self.assertEqual(result["terminalWealthMicros"], [1000000, 1000000])
        self.assertEqual(result["models"][0]["expectedChangeMicrosNumerator"], -500000000000)
        self.assertEqual(result["fieldPairs"][0]["afterStrictMassPpm"], 0)
        self.assertEqual(result["classification"], "negative_declared_money")
        self.assertEqual(args["financial"], before["financial"])
        self.assertEqual(args["books"], before["books"])

    def test_complement_actions_cannot_double_spend_one_book(self):
        args = fixture()
        args["legs"] = [{"ex": "a", "side": "YES", "action": "SELL", "quantity": 2, "price": 500000},
                        {"ex": "a", "side": "NO", "action": "BUY", "quantity": 1, "price": 500000}]
        with self.assertRaisesRegex(ValueError, "SHARED_DEPTH_EXHAUSTED"):
            audit(**args)

    def test_buy_cannot_use_unexecuted_future_sale(self):
        args = fixture()
        args["legs"] = [{"ex": "a", "side": "YES", "action": "BUY", "quantity": 1, "price": 600000},
                        {"ex": "a", "side": "YES", "action": "SELL", "quantity": 2, "price": 500000}]
        with self.assertRaisesRegex(ValueError, "PREFIX_FUNDING"):
            audit(**args)

    def test_hold_is_an_explicit_comparator_without_execution_authority(self):
        result = audit(**fixture())
        self.assertEqual(result["models"][0]["expectedChangeMicrosNumerator"], 0)
        self.assertEqual(result["fieldPairs"][0]["beforeStrictMassPpm"], 500000)
        self.assertFalse(result["executionAuthority"])
        self.assertFalse(result["actualProceedsCredited"])

    def test_existing_noninherited_market_already_consumes_a_fresh_slot(self):
        args = fixture()
        args["financial"].update(cash="1", freshMarketLimit=1)
        args["registry"].append({"exchangeId": "b", "contractId": "2"})
        for world in args["model"]["scenarios"]:
            world["payouts"]["2"] = "resolved_yes"
        args["books"]["b"] = deepcopy(args["books"]["a"])
        args["legs"] = [{"ex": "b", "side": "YES", "action": "BUY", "quantity": 1, "price": 600000}]
        with self.assertRaisesRegex(ValueError, "MARKET_COUNT"):
            audit(**args)
        args["financial"]["inheritedExchanges"] = ["a"]
        self.assertEqual(audit(**args)["trades"], 1)

    def test_omitted_holding_bad_mass_and_unacknowledged_hypotheses_reject(self):
        for kind, reason in [("holding", "UNMODELED_HOLDING"), ("mass", "MODEL_MASS"),
                             ("assumption", "EXPLICIT_HYPOTHESES")]:
            args = fixture()
            if kind == "holding":
                args["financial"]["positions"]["b"] = args["financial"]["positions"].pop("a")
            elif kind == "mass":
                args["model"]["probabilityModels"][0]["weightsPpm"] = [1, 1]
            else:
                args["assumptions"]["zeroFees"] = False
            with self.assertRaisesRegex(ValueError, reason):
                audit(**args)


if __name__ == "__main__":
    unittest.main()
