"""Synthetic portfolio checks; no captured market or forecast calibration."""
import copy
import unittest

from .sequential_frontier import solve

U = 1_000_000


def fixture():
    fin = dict(cash="1.000000", ownedCost="0.000000", positions={},
               maximumCommitted="1.000000", maximumMarketCost="1.000000",
               inheritedExchanges=[], freshMarketLimit=2, maximumMarkets=2,
               conditionalFutureLegs=4, physicalOrderQuantity=10)
    registry = [dict(exchangeId=e, contractId=e) for e in ("safe", "rare")]
    books = {
        "safe": {"bids": [{"price": "0.095000", "quantity": 10}],
                 "asks": [{"price": "0.100000", "quantity": 10}]},
        "rare": {"bids": [{"price": "0.035000", "quantity": 20}],
                 "asks": [{"price": "0.040000", "quantity": 20}]}}
    model = dict(scenarios=[
        dict(id="ordinary", kind="ordinary", assumptionLabel="Synthetic",
             payouts={"safe": "resolved_yes", "rare": "resolved_no"}),
        dict(id="rare", kind="ordinary", assumptionLabel="Synthetic",
             payouts={"safe": "resolved_no", "rare": "resolved_yes"}),
        dict(id="refund", kind="exceptional", assumptionLabel="Synthetic",
             payouts={"safe": "refund_principal", "rare": "refund_principal"})],
        probabilityModels=[dict(id="view", weightsPpm=[989000, 10000, 1000], assumptionLabel="Synthetic")])
    fields = [dict(id="flat", thresholds=["10.000000"] * 3,
                   completeField=False, assumptionLabel="Synthetic incomplete field")]
    return fin, registry, books, model, fields


class SequentialFrontier(unittest.TestCase):
    def run_search(self, f, **kw):
        out = solve(*f, target_pairs=[("view", "flat")], seconds=3, **kw)
        self.assertEqual(out["status"], "exact_sequence_checked", out)
        self.assertFalse(out["executionAuthority"])
        return out

    def test_rank_search_retains_negative_mean_alternative_to_money_best(self):
        f = fixture(); f[0]["freshMarketLimit"] = 1
        money = self.run_search(f, objective="mean")
        rank = self.run_search(f, objective="rank")
        self.assertGreater(money["independentExactAudit"]["models"][0]["expectedChangeMicrosNumerator"], 0)
        self.assertEqual(money["selectedAverageStrictMassPpm"], 0)
        self.assertEqual(rank["selectedAverageStrictMassPpm"], 10000)
        self.assertLess(rank["independentExactAudit"]["models"][0]["expectedChangeMicrosNumerator"], 0)
        self.assertTrue(any(v["ex"] == "rare" and v["side"] == "YES" for v in rank["legs"]))

    def test_known_sale_can_fund_buys_with_original_fifo_refund_preserved(self):
        f = fixture(); f[0].update(cash="0.000000", ownedCost="1.000000",
            positions={"safe": {"YES": [{"quantity": 4, "price": "0.300000"}]}},
            inheritedExchanges=["safe"])
        f[0]["ownedCost"] = "1.200000"
        f[0]["maximumCommitted"] = "1.200000"
        f[0]["maximumMarketCost"] = "1.200000"
        f[2]["safe"]["bids"] = [{"price": "0.600000", "quantity": 4}]
        f[2]["safe"]["asks"] = [{"price": "0.650000", "quantity": 10}]
        f[4][0]["thresholds"] = ["10.000000"] * 3
        out = self.run_search(f, objective="rank")
        self.assertEqual(out["legs"][0]["action"], "SELL")
        self.assertEqual(out["legs"][0]["basis"], 1_200_000)
        self.assertGreater(out["selectedAverageStrictMassPpm"], 0)
        self.assertGreaterEqual(out["independentExactAudit"]["cashMicros"], 0)

    def test_both_complements_are_not_bought_and_existing_slots_count(self):
        f = fixture(); f[0]["freshMarketLimit"] = 1
        out = self.run_search(f, objective="rank")
        buys = {(v["ex"], v["side"]) for v in out["legs"] if v["action"] == "BUY"}
        self.assertLessEqual(len({ex for ex, side in buys}), 1)
        for ex in {ex for ex, side in buys}:
            self.assertLessEqual(sum(e == ex for e, side in buys), 1)

    def test_adverse_field_is_preserved_and_mean_floor_changes_choice(self):
        f = fixture(); f[4].append(dict(id="adverse", thresholds=["100.000000"] * 3,
            completeField=False, assumptionLabel="Synthetic adverse case"))
        out = self.run_search(f, objective="rank", minimum_expected_change="8.000000")
        self.assertGreaterEqual(out["independentExactAudit"]["models"][0]["expectedChangeMicrosNumerator"], 8 * U * U)
        self.assertTrue(any(p["fieldId"] == "adverse" for p in out["independentExactAudit"]["fieldPairs"]))
        self.assertEqual(out["selectedAverageStrictMassPpm"], 0)
        self.assertEqual(len(out["nativeProjection"]["fieldModels"]), 2)

    def test_zero_target_gain_never_recommends_arbitrary_losing_trades(self):
        f = fixture(); f[4][0]["thresholds"] = ["100.000000"] * 3
        out = self.run_search(f, objective="robust_rank")
        self.assertFalse(out["targetObjectiveImproved"])
        self.assertEqual(out["researchDisposition"], "retain_no_action_for_this_target_objective")
        self.assertEqual(out["baseline"]["trades"], 0)

    def test_strict_winner_threshold_rejects_exact_tie_but_accepts_one_micro_above(self):
        f = fixture(); f[0]["freshMarketLimit"] = 1
        f[4][0]["thresholds"] = ["20.200000"] * 3
        tied = self.run_search(f, objective="rank")
        self.assertEqual(tied["selectedAverageStrictMassPpm"], 0)
        f[4][0]["thresholds"] = ["20.199999"] * 3
        winning = self.run_search(f, objective="rank")
        self.assertEqual(winning["selectedAverageStrictMassPpm"], 10000)

    def test_original_managed_draft_keeps_slot_after_hypothetical_full_sale(self):
        f = fixture(); f[0].update(cash="0.000000", ownedCost="0.400000",
            positions={"safe": {"YES": [{"quantity": 4, "price": "0.100000"}]}},
            freshMarketLimit=1, retainedMarketExchanges=["safe"])
        f[2]["safe"]["bids"] = [{"price": "0.600000", "quantity": 4}]
        f[2]["safe"]["asks"] = [{"price": "0.650000", "quantity": 10}]
        retained = self.run_search(f, objective="rank")
        self.assertEqual(retained["selectedAverageStrictMassPpm"], 0)
        del f[0]["retainedMarketExchanges"]
        released = self.run_search(f, objective="rank")
        self.assertEqual(released["selectedAverageStrictMassPpm"], 10000)

    def test_missing_original_empty_cohort_ownership_excludes_purchase(self):
        f = fixture(); f[0]["excludedBuyExchanges"] = ["rare"]
        out = self.run_search(f, objective="rank")
        self.assertEqual(out["selectedAverageStrictMassPpm"], 0)
        self.assertTrue(all(v["ex"] != "rare" or v["action"] != "BUY" for v in out["legs"]))

    def test_managed_held_topup_requires_first_covered_sale_handoff(self):
        from .sequence_audit import audit
        from .sequential_frontier import ASSUMPTIONS
        f = fixture(); f[0].update(ownedCost="0.400000", maximumCommitted="2.000000",
            maximumMarketCost="2.000000", positions={"safe": {"YES": [
                {"quantity": 4, "price": "0.100000"}]}})
        unrestricted = self.run_search(f, objective="mean")
        self.assertEqual(unrestricted["legs"][0]["action"], "BUY")
        f[0]["buyRequiresPriorCoveredSellExchanges"] = ["safe"]
        with self.assertRaisesRegex(ValueError, "BUY_REQUIRES_PRIOR_COVERED_SELL_HANDOFF"):
            audit(*f, unrestricted["legs"], assumptions=ASSUMPTIONS)
        routed = self.run_search(f, objective="mean")
        self.assertEqual(routed["legs"][0]["action"], "SELL")
        self.assertEqual(routed["legs"][0]["ex"], "safe")
        self.assertTrue(any(v["action"] == "BUY" and v["ex"] == "safe" for v in routed["legs"]))

    def test_original_cash_reservation_is_wealth_but_cannot_fund_new_buys(self):
        f = fixture(); f[0]["reservedCash"] = "1.000000"
        result = self.run_search(f, objective="mean")
        self.assertEqual(result["legs"], [])
        self.assertEqual(result["baseline"]["terminalWealthMicros"], [U, U, U])
        self.assertEqual(result["independentExactAudit"]["cashMicros"], U)

    def test_consumed_inherited_origin_cannot_exempt_reacquisition_or_retained_slot(self):
        from .sequence_audit import audit
        from .sequential_frontier import ASSUMPTIONS
        f = fixture(); f[0].update(cash="0.000000", ownedCost="0.400000",
            positions={"safe": {"YES": [{"quantity": 4, "price": "0.100000"}]}},
            inheritedExchanges=["safe"], freshMarketLimit=1, retainedMarketExchanges=["safe"])
        f[2]["safe"]["bids"] = [{"price": "0.600000", "quantity": 4}]
        f[2]["safe"]["asks"] = [{"price": "0.650000", "quantity": 10}]
        static = self.run_search(f, objective="rank")
        self.assertEqual(static["selectedAverageStrictMassPpm"], 10000)
        f[0]["inheritedRemainingQuantities"] = {"safe": 4}
        with self.assertRaisesRegex(ValueError, "MARKET_COUNT"):
            audit(*f, static["legs"], assumptions=ASSUMPTIONS)
        dynamic = self.run_search(f, objective="rank")
        self.assertEqual(dynamic["selectedAverageStrictMassPpm"], 0)

    def test_exact_native_partition_budget_never_drops_scenarios(self):
        f = fixture(); original = copy.deepcopy(f)
        constrained = self.run_search(f, objective="rank", maximum_native_scenarios=2)
        self.assertEqual(constrained["legs"], [])
        admitted = self.run_search(f, objective="rank", maximum_native_scenarios=3)
        self.assertEqual(admitted["selectedAverageStrictMassPpm"], 10000)
        self.assertEqual(f, original)

    def test_target_conditioned_feasibility_preserves_worlds_and_models(self):
        f = fixture(); original = copy.deepcopy(f)
        found = self.run_search(f, required_target_hits=[("flat", "rare")],
                                maximum_native_scenarios=3)
        self.assertEqual(found["selectedAverageStrictMassPpm"], 10000)
        self.assertEqual(found["requiredTargetHits"], [("flat", "rare")])
        self.assertEqual(f, original)
        impossible = solve(*f, target_pairs=[("view", "flat")], seconds=3,
                           required_target_hits=[("flat", "refund")])
        self.assertEqual(impossible["status"], "no_incumbent")


if __name__ == "__main__":
    unittest.main()
