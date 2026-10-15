"""Synthetic economic fixtures with independently specified ending ledgers."""
import copy
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from analyze import AnalysisError, analyze, instant, units


def stamp(minutes=0, seconds=0):
    return f"2026-10-15T00:{minutes:02d}:{seconds:02d}Z"


def adoption(lots=((3, "0.4"), (2, "0.6")), s="YES"):
    principal = sum(n * units(p) for n, p in lots)
    from analyze import money
    return {"type": "actual_position_adoption", "source": "official_sig_api_v1",
            "authority": "human_moloch_inherited_management_20261015", "complete": True,
            "requestStartedAt": stamp(), "receivedAt": stamp(), "processedAt": stamp(),
            "openingCost": money(principal), "receiptHash": "a" * 64,
            "positions": [{"exchangeId": "21", "marketId": "10",
                           "quantity": sum(n for n, _ in lots) * (-1 if s == "NO" else 1),
                           "lotPrincipal": money(principal), "reportedCostBasis": money(principal),
                           "reportedMinusLotPrincipal": "0", "basisClassification": "exact",
                           "lots": [{"lotId": str(50 + i), "quantity": n, "side": s,
                                     "entryPrice": p, "openedAt": "2026-10-09T02:23:38.1711908+00:00"}
                                    for i, (n, p) in enumerate(lots)]}]}


def fill(qid, action, n, p, *, s="YES", fee="0", executed=1, observed=None):
    return {"orderId": qid, "actualOrderId": str(991234500 + qid), "sourceId": str(881234500 + qid),
            "marketId": "10", "exchangeId": "21", "side": s, "action": action,
            "quantity": n, "price": p, "fee": fee, "at": stamp(observed if observed is not None else executed),
            "executedAt": stamp(executed), "counterfactual": False, "receiptHash": "b" * 64}


def envelope(fills, *, cash, cost, realized, inherited, new, fees="0", positions=None, forced=()):
    orders = [{"id": f["orderId"], "marketId": f["marketId"], "exchangeId": f["exchangeId"],
               "side": f["side"], "action": f["action"], "quantity": f["quantity"], "filled": f["quantity"],
               "price": f["price"], "createdAt": 0,
               **({"inventoryExitReason": "maximumAge", "executionStyle": "inventoryAON"}
                  if f["orderId"] in forced else {})} for f in fills]
    return {"sequence": 123, "orders": orders, "fills": fills,
            "report": {"at": stamp(59), "executionMode": "live", "fillModel": "actualAccountFills",
                       "cash": cash, "ownedCost": cost, "realized": realized, "fees": fees,
                       "inheritedRealizedGross": inherited, "newTradingRealized": new,
                       "openingInventoryCost": "2.4", "fills": len(fills), "positions": positions or {}}}


def bind_opening(env, adopt):
    env["report"]["openingInventoryCost"] = adopt["openingCost"]
    return env


class EconomicsTests(unittest.TestCase):
    def test_partial_fifo_sell_splits_two_inherited_lots_and_one_new_lot_with_fees(self):
        a = adoption()
        e = envelope([fill(1, "buy", 4, "0.5", fee="0.04"),
                      fill(2, "sell", 6, "0.7", fee="0.06", executed=12)],
                     cash="99.7", cost="1.5", realized="1.2", inherited="1.1", new="0.1", fees="0.1",
                     positions={"21": {"YES": [{"quantity": 3, "price": "0.5"}], "NO": []}})
        r = analyze(e, a)
        self.assertEqual(r["accounting"]["initialCashDerivedFromBankIdentity"], "100.000000")
        self.assertEqual(r["accounting"]["cashImmediatelyAfterAdoption"], "97.600000")
        self.assertEqual(r["accounting"]["newTradingRealized"], "0.100000")
        self.assertEqual(r["newBuyLotCohorts"]["partiallyClosed"], 1)
        self.assertEqual(r["newBuyLotCohorts"]["closedFragments"]["matured600000msShares"], 1)
        self.assertEqual(r["perMarket"][0]["remainingUnmarkedCost"], {"inherited": "0.000000", "new": "1.500000"})
        mix = r["perMarket"][0]["exitMix"]
        self.assertEqual(sum(m["closedShares"] for m in mix if m["cohort"] == "inherited"), 5)

    def test_opposite_buy_redeems_pairs_immediately_and_uses_complement_for_inherited(self):
        a = adoption(((3, "0.4"),))
        e = bind_opening(envelope([fill(1, "buy", 5, "0.5", s="NO", fee="0.05")],
                                 cash="99.25", cost="1", realized="0.25", inherited="0.3", new="-0.05", fees="0.05",
                                 positions={"21": {"YES": [], "NO": [{"quantity": 2, "price": "0.5"}]}}), a)
        r = analyze(e, a)
        self.assertEqual(r["perMarket"][0]["pairRedeemedShares"], 3)
        self.assertEqual(r["accounting"]["inheritedRealizedGross"], "0.300000")
        self.assertEqual(r["newBuyLotCohorts"]["closedFragments"]["grossRealized"], "0.000000")
        self.assertEqual(r["newBuyLotCohorts"]["closedFragments"]["freshUnder600000msShares"], 3)

    def test_new_against_new_pair_profit_belongs_older_side_not_double_counted(self):
        a = adoption(((1, "0.4"),))
        fs = [fill(1, "sell", 1, "0.4"), fill(2, "buy", 4, "0.3", fee="0.04", executed=2),
              fill(3, "buy", 2, "0.6", s="NO", fee="0.02", executed=12)]
        e = bind_opening(envelope(fs, cash="99.54", cost="0.6", realized="0.14", inherited="0", new="0.14", fees="0.06",
                                 positions={"21": {"YES": [{"quantity": 2, "price": "0.3"}], "NO": []}}), a)
        r = analyze(e, a)
        self.assertEqual(r["newBuyLotCohorts"]["fullyClosedGrossFlat"], 1)
        self.assertEqual(r["newBuyLotCohorts"]["closedFragments"]["grossRealized"], "0.200000")
        self.assertEqual(r["newBuyLotCohorts"]["closedFragments"]["matured600000msShares"], 2)
        self.assertEqual(r["newBuyLotCohorts"]["closedFragments"]["freshUnder600000msShares"], 2)

    def test_reverse_and_tied_execution_clocks_keep_bank_order_and_flag_negative_age(self):
        a = adoption(((1, "0.4"),))
        fs = [fill(1, "buy", 1, "0.5", executed=10, observed=20),
              fill(2, "sell", 1, "0.6", executed=9, observed=21),
              fill(3, "sell", 1, "0.55", executed=9, observed=22)]
        e = bind_opening(envelope(fs, cash="100.25", cost="0", realized="0.25", inherited="0.2", new="0.05"), a)
        r = analyze(e, a)
        self.assertEqual(r["fills"]["recordedActualFills"], 3)
        self.assertEqual(r["fills"]["adjacentExecutionClockReversals"], 1)
        self.assertEqual(r["fills"]["adjacentExecutionClockTies"], 1)
        self.assertEqual(r["fills"]["executedBeforePreviouslyRecordedFill"], 2)
        self.assertEqual(r["newBuyLotCohorts"]["closedFragments"]["executionClockReversalShares"], 1)
        self.assertEqual(r["newBuyLotCohorts"]["closedFragments"]["entryObservedAfterExitExecutedShares"], 1)
        sorted_e = copy.deepcopy(e)
        sorted_e["fills"].sort(key=lambda f: f["executedAt"])
        with self.assertRaises(AnalysisError):
            analyze(sorted_e, a)

    def test_maximum_age_exit_can_close_fresh_and_matured_new_lots_plus_inherited(self):
        a = adoption(((1, "0.4"),))
        e = bind_opening(envelope([fill(1, "buy", 1, "0.5"), fill(2, "buy", 1, "0.6", executed=12),
                                  fill(3, "sell", 3, "0.45", executed=13)],
                                 cash="99.85", cost="0", realized="-0.15", inherited="0.05", new="-0.2", forced=(3,)), a)
        r = analyze(e, a)
        forced = r["newBuyLotCohorts"]["maximumAgeExitFragments"]
        self.assertEqual((forced["shares"], forced["freshUnder600000msShares"], forced["matured600000msShares"]), (2, 1, 1))
        self.assertEqual(forced["nonnegativeHoldingAgeMsMin"], "60000.000000")
        self.assertEqual(forced["grossRealized"], "-0.200000")
        self.assertEqual(r["newBuyLotCohorts"]["fullyClosedGrossLosses"], 2)

    def test_partial_fills_share_one_order_without_losing_any_records(self):
        a = adoption(((1, "0.4"),))
        fs = [fill(1, "buy", 1, "0.5"), fill(1, "buy", 2, "0.5", executed=2)]
        fs[1]["sourceId"] = "881234599"
        e = bind_opening(envelope(fs, cash="98.1", cost="1.9", realized="0", inherited="0", new="0",
                                 positions={"21": {"YES": [{"quantity": 1, "price": "0.4"}, {"quantity": 1, "price": "0.5"},
                                                           {"quantity": 2, "price": "0.5"}], "NO": []}}), a)
        e["orders"] = [dict(e["orders"][0], quantity=3, filled=3)]
        r = analyze(e, a)
        self.assertEqual(r["fills"]["recordedActualFills"], 2)
        self.assertEqual(r["newBuyLotCohorts"]["total"], 2)

    def test_reentry_count_is_after_actual_flattening_not_every_extra_purchase(self):
        a = adoption(((1, "0.4"),))
        fs = [fill(1, "buy", 1, "0.5"), fill(2, "sell", 2, "0.6", executed=2),
              fill(3, "buy", 1, "0.3", s="NO", executed=3),
              fill(4, "buy", 1, "0.3", s="NO", executed=4)]
        e = bind_opening(envelope(fs, cash="99.7", cost="0.6", realized="0.3", inherited="0.2", new="0.1",
                                 positions={"21": {"YES": [], "NO": [{"quantity": 1, "price": "0.3"}, {"quantity": 1, "price": "0.3"}]}}), a)
        r = analyze(e, a)
        self.assertEqual(r["perMarket"][0]["reentriesAfterFlat"], 1)

    def test_negative_no_adoption_and_reported_basis_difference_are_preserved_as_classification(self):
        a = adoption(((2, "0.4"),), s="NO")
        row = a["positions"][0]
        row.update(reportedCostBasis="0.91", reportedMinusLotPrincipal="0.11", basisClassification="reported_discrepancy")
        e = bind_opening(envelope([fill(1, "sell", 1, "0.6", s="NO")], cash="99.8", cost="0.4", realized="0.2",
                                 inherited="0.2", new="0", positions={"21": {"YES": [], "NO": [{"quantity": 1, "price": "0.4"}]}}), a)
        r = analyze(e, a)
        self.assertEqual(r["perMarket"][0]["basisClassifications"], {"reported_discrepancy": 1})
        self.assertEqual(r["accounting"]["inheritedRealizedGross"], "0.200000")

    def test_rejects_adoption_after_first_recorded_fill(self):
        a = adoption(((1, "0.4"),))
        a["processedAt"] = stamp(2)
        e = bind_opening(envelope([fill(1, "sell", 1, "0.5")], cash="100.1", cost="0", realized="0.1", inherited="0.1", new="0"), a)
        with self.assertRaisesRegex(AnalysisError, "FILLS_PRECEDE_ADOPTION"):
            analyze(e, a)

    def test_rejects_bad_principal_fee_realized_and_fifo_amounts(self):
        a = adoption(((1, "0.4"),))
        e = bind_opening(envelope([fill(1, "sell", 1, "0.5")], cash="100.1", cost="0", realized="0.1", inherited="0.1", new="0"), a)
        cases = []
        wrong = copy.deepcopy(a)
        wrong["positions"][0]["lotPrincipal"] = "0.5"
        cases.append((e, wrong, "ADOPTION_PRINCIPAL_MISMATCH"))
        for field, value, error in (("fees", "0.01", "REPORT_FEES_MISMATCH"),
                                    ("realized", "0.11", "REPORT_REALIZED_MISMATCH"),
                                    ("inheritedRealizedGross", "0.09", "REPORT_INHERITEDREALIZEDGROSS_MISMATCH")):
            wrong = copy.deepcopy(e)
            wrong["report"][field] = value
            cases.append((wrong, a, error))
        wrong = copy.deepcopy(e)
        wrong["report"]["positions"] = {"21": {"YES": [{"quantity": 1, "price": "0.4"}], "NO": []}}
        cases.append((wrong, a, "ENDING_FIFO_POSITIONS_MISMATCH"))
        for report, adoption_event, error in cases:
            with self.subTest(error=error), self.assertRaisesRegex(AnalysisError, error):
                analyze(report, adoption_event)

    def test_rejects_duplicate_fill_scope_mismatch_and_incomplete_order_join(self):
        a = adoption(((2, "0.4"),))
        e = bind_opening(envelope([fill(1, "sell", 1, "0.5")], cash="99.7", cost="0.4", realized="0.1", inherited="0.1", new="0",
                                 positions={"21": {"YES": [{"quantity": 1, "price": "0.4"}], "NO": []}}), a)
        bad = copy.deepcopy(e)
        bad["fills"].append(copy.deepcopy(bad["fills"][0]))
        bad["report"]["fills"] = 2
        with self.assertRaisesRegex(AnalysisError, "DUPLICATE_ACTUAL_FILL"):
            analyze(bad, a)
        bad = copy.deepcopy(e)
        bad["orders"][0]["side"] = "NO"
        with self.assertRaisesRegex(AnalysisError, "FILL_ORDER_SCOPE"):
            analyze(bad, a)
        bad = copy.deepcopy(e)
        bad["orders"] = []
        with self.assertRaisesRegex(AnalysisError, "FILL_ORDER_MISSING"):
            analyze(bad, a)

    def test_86_records_are_not_reduced_to_orders_or_trades(self):
        a = adoption(((86, "0.4"),))
        fs = [fill(i + 1, "sell", 1, "0.5", executed=1) for i in range(86)]
        e = bind_opening(envelope(fs, cash="108.6", cost="0", realized="8.6", inherited="8.6", new="0"), a)
        r = analyze(e, a)
        self.assertEqual(r["fills"]["recordedActualFills"], 86)
        self.assertEqual(r["fills"]["adjacentExecutionClockTies"], 85)
        self.assertEqual(r["perMarket"][0]["sellShares"], 86)

    def test_seven_digit_clock_and_exact_money_reject_float_and_fractional_micro(self):
        self.assertEqual(instant("2026-10-09T02:23:38.1711908+00:00") - instant("2026-10-09T02:23:38Z"), 171190800)
        self.assertEqual(instant("2026-10-09T03:23:38.1711908+01:00"), instant("2026-10-09T02:23:38.1711908Z"))
        with self.assertRaises(AnalysisError):
            units(0.4)
        with self.assertRaises(AnalysisError):
            units("0.0000001")

    def test_cli_hashes_raw_private_inputs_and_emits_no_order_fill_or_profile(self):
        a = adoption(((1, "0.4"),))
        e = bind_opening(envelope([fill(1, "sell", 1, "0.5")], cash="100.1", cost="0", realized="0.1", inherited="0.1", new="0"), a)
        e["privateProfile"] = "SYNTHETIC_PRIVATE_PROFILE"
        with tempfile.TemporaryDirectory() as folder:
            rp, ap, out = (Path(folder) / name for name in ("private-report.json", "private-adoption.json", "public.json"))
            rp.write_text(json.dumps(e), encoding="utf-8")
            ap.write_text(json.dumps({"event": a}), encoding="utf-8")
            result = subprocess.run([sys.executable, str(Path(__file__).with_name("analyze.py")), "--report", str(rp),
                                     "--adoption", str(ap), "--output", str(out)], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            raw = out.read_text(encoding="utf-8")
            r = json.loads(raw)
            self.assertEqual(r["sourceSha256"]["report"], hashlib.sha256(rp.read_bytes()).hexdigest())
            for private in ("991234501", "881234501", "SYNTHETIC_PRIVATE_PROFILE", "actualOrderId", "sourceId", "receiptHash", folder):
                self.assertNotIn(private, raw)
            self.assertEqual(result.stdout, "")


if __name__ == "__main__":
    unittest.main()
