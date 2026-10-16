"""Independent synthetic endpoint ledgers for original actual FIFO replay."""
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "analyze.py"
SPEC = importlib.util.spec_from_file_location("actual_economics", SCRIPT)
MOD = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MOD
SPEC.loader.exec_module(MOD)


def stamp(seconds=0):
    return f"2026-10-15T00:{seconds // 60:02d}:{seconds % 60:02d}Z"


def adoption(lots=((3, "0.4"), (2, "0.6"))):
    basis = sum(n * MOD.units(p) for n, p in lots)
    return {"type": "actual_position_adoption", "source": "official_sig_api_v1",
            "authority": "human_moloch_inherited_management_20261015", "complete": True,
            "requestStartedAt": stamp(), "receivedAt": stamp(), "processedAt": stamp(),
            "openingCost": MOD.money(basis), "receiptHash": "a" * 64,
            "positions": [{"exchangeId": "21", "marketId": "10", "quantity": sum(n for n, _ in lots),
                           "lotPrincipal": MOD.money(basis), "lots": [
                               {"lotId": str(i + 51), "quantity": n, "side": "YES", "entryPrice": p,
                                "openedAt": "2026-10-14T00:00:00.1234567Z"}
                               for i, (n, p) in enumerate(lots)]}]}


def fill(qid, action, n, p, *, side="YES", fee="0", at=10, executed=None, fid=None):
    return {"type": "actual_fill", "source": "official_sig_api_v1", "orderId": qid,
            "actualOrderId": str(991234500 + qid), "fillId": str(fid or 881234500 + qid),
            "marketId": "10", "exchangeId": "21", "side": side, "action": action,
            "quantity": n, "price": p, "fee": fee, "processedAt": stamp(at), "receivedAt": stamp(at),
            "executedAt": stamp(at if executed is None else executed), "receiptHash": "b" * 64}


def payload(events, *, cash, cost, total, inherited, new, fees="0", positions=None, forecast=(), forced=()):
    unique, rows, orders = {}, [], {}
    for i, e in enumerate(events):
        rows.append({"sequence": i + 1, "hash": "d" * 64, "event": e})
        if e["type"] != "actual_fill":
            continue
        key = (e["actualOrderId"], e["fillId"])
        if key in unique:
            continue
        f = dict(e)
        f["sourceId"] = f.pop("fillId")
        f["at"] = f.pop("processedAt")
        f["counterfactual"] = False
        unique[key] = f
        qid = e["orderId"]
        q = orders.setdefault(qid, {"id": qid, "marketId": e["marketId"], "exchangeId": e["exchangeId"],
                                   "side": e["side"], "action": e["action"], "quantity": 0, "filled": 0,
                                   "price": e["price"], "createdAt": 0})
        q["quantity"] += e["quantity"]
        q["filled"] += e["quantity"]
        if qid in forced:
            q["inventoryExitReason"] = "maximumAge"
    return {"schemaVersion": 1, "observedAt": stamp(180), "startingCash": "100",
            "source": {"bankSequence": 100, "bankPrevious": "a" * 64, "reportEnvelopeHash": "b" * 64,
                       "journalPrefixSha256": "c" * 64, "journalPrefixBytes": 1000, "everyOriginalRowChainVerified": True},
            "events": rows, "orders": list(orders.values()), "fills": list(unique.values()),
            "forecastQids": list(forecast), "expectedReport": {
                "positions": positions or {}, "cash": cash, "ownedCost": cost, "fees": fees, "realized": total,
                "inheritedRealizedGross": inherited, "newTradingRealized": new,
                "openingInventoryCost": events[0]["openingCost"], "continuation": None}}


def mixed():
    return payload([adoption(), fill(1, "buy", 4, "0.5", fee="0.04"),
                    fill(2, "sell", 6, "0.7", fee="0.06", at=80)],
                   cash="99.7", cost="1.5", total="1.2", inherited="1.1", new="0.1", fees="0.1",
                   forecast=(1,), forced=(2,),
                   positions={"21": {"YES": [{"quantity": 3, "price": "0.5", "executedAt": stamp(10)}], "NO": []}})


class EconomicsTests(unittest.TestCase):
    def test_partial_fifo_inherited_forecast_overlap_exact_fees_and_open_basis(self):
        r = MOD.analyze(mixed(), inventory_age_ms=60000)
        self.assertEqual(r["accounting"]["newTradingRealized"], "0.100000")
        self.assertEqual(r["origins"]["inherited"]["grossRealized"], "1.100000")
        self.assertEqual(r["origins"]["forecast"]["netRealizedAttribution"], "0.150000")
        self.assertEqual(r["origins"]["new"]["netRealizedAttribution"], "-0.050000")
        self.assertEqual(r["accounting"]["feesNotAssignedToAcquisitionCohort"], "0.050000")
        self.assertEqual(r["origins"]["forecast"]["remainingUnmarkedCost"], "1.500000")
        self.assertEqual(r["origins"]["forecast"]["maximumAgeExits"]["shares"], 1)
        self.assertEqual(r["origins"]["forecast"]["closed"]["belowExplicitAgeThresholdShares"], 0)
        self.assertEqual(MOD.analyze(mixed(), inventory_age_ms=600000)["origins"]["forecast"]["closed"]["belowExplicitAgeThresholdShares"], 1)

    def test_opposite_buy_pairs_once_with_old_side_profit_and_actual_open_remainder(self):
        e = payload([adoption(((3, "0.4"),)), fill(1, "buy", 5, "0.5", side="NO", fee="0.05")],
                    cash="99.25", cost="1", total="0.25", inherited="0.3", new="-0.05", fees="0.05",
                    positions={"21": {"YES": [], "NO": [{"quantity": 2, "price": "0.5", "executedAt": stamp(10)}]}})
        r = MOD.analyze(e)
        self.assertEqual(r["origins"]["inherited"]["grossRealized"], "0.300000")
        self.assertEqual(r["origins"]["new"]["closed"]["grossRealized"], "0.000000")
        sides = {row["side"]: row for row in r["perExchangeSide"]}
        self.assertEqual(sides["YES"]["fifoOriginRealized"], "0.300000")
        self.assertEqual(sides["NO"]["fifoOriginRealized"], "-0.050000")
        self.assertEqual(r["openPositions"][0]["quantity"], 2)

    def test_partial_roundtrip_keeps_remaining_cost_and_never_marks_future_profit(self):
        e = payload([adoption(((1, "0.4"),)), fill(1, "sell", 1, "0.4", at=5),
                     fill(2, "buy", 4, "0.3", fee="0.01", at=10),
                     fill(3, "sell", 1, "0.4", fee="0.01", at=80)],
                    cash="99.18", cost="0.9", total="0.08", inherited="0", new="0.08", fees="0.02",
                    positions={"21": {"YES": [{"quantity": 3, "price": "0.3", "executedAt": stamp(10)}], "NO": []}})
        r = MOD.analyze(e)
        self.assertEqual(r["origins"]["new"]["netRealizedAttribution"], "0.080000")
        self.assertEqual(r["origins"]["new"]["fullyClosedCohorts"], 0)
        self.assertEqual(r["origins"]["new"]["closed"]["minimumHoldingMs"], "70000.000000")
        self.assertEqual(r["openPositions"][0]["unmarkedCost"], "0.900000")
        self.assertFalse(r["profitabilityEstablished"])

    def test_exact_economic_duplicate_with_later_receipt_deduplicates_but_changed_fill_rejects(self):
        e = mixed()
        duplicate = copy.deepcopy(e["events"][-1])
        duplicate["sequence"] += 1
        duplicate["event"]["processedAt"] = stamp(81)
        duplicate["event"]["receivedAt"] = stamp(81)
        duplicate["event"]["receiptHash"] = "e" * 64
        e["events"].append(duplicate)
        r = MOD.analyze(e)
        self.assertEqual(r["uniqueActualFills"], 2)
        self.assertEqual(r["duplicateOriginalFillEvents"], 1)
        e["events"][-1]["event"]["quantity"] = 5
        with self.assertRaisesRegex(MOD.AnalysisError, "CONFLICTING_DUPLICATE"):
            MOD.analyze(e)

    def test_uncovered_sell_missing_basis_negative_age_and_rebound_fill_reject(self):
        for case in ("uncovered", "basis", "negative_age", "rebound"):
            e = mixed()
            if case == "uncovered":
                e["events"][-1]["event"]["quantity"] = 10
                e["orders"][-1]["quantity"] = 10
            elif case == "basis":
                del e["events"][0]["event"]["positions"][0]["lots"][0]["entryPrice"]
            elif case == "negative_age":
                e["events"][-1]["event"]["executedAt"] = stamp(9)
            else:
                e["events"][-1]["event"]["fillId"] = e["events"][1]["event"]["fillId"]
            with self.assertRaises((MOD.AnalysisError, KeyError), msg=case):
                MOD.analyze(e)

    def test_native_end_cash_cost_positions_fees_and_exact_fill_tape_must_all_match(self):
        for field in ("cash", "ownedCost", "realized", "newTradingRealized", "inheritedRealizedGross", "fees"):
            e = mixed()
            e["expectedReport"][field] = "999"
            with self.assertRaises(MOD.AnalysisError, msg=field):
                MOD.analyze(e)
        e = mixed()
        e["expectedReport"]["positions"]["21"]["YES"][0]["quantity"] = 2
        with self.assertRaisesRegex(MOD.AnalysisError, "ENDING_FIFO"):
            MOD.analyze(e)
        e = mixed()
        e["fills"] = list(reversed(e["fills"]))
        with self.assertRaisesRegex(MOD.AnalysisError, "NATIVE_UNIQUE_FILL"):
            MOD.analyze(e)

    def test_cli_hashes_private_input_and_emits_no_order_fill_or_lot_ids(self):
        with tempfile.TemporaryDirectory() as td:
            source, target = Path(td) / "private.json", Path(td) / "aggregate.json"
            raw = json.dumps(mixed()).encode()
            source.write_bytes(raw)
            run = subprocess.run([sys.executable, str(SCRIPT), "--input", str(source), "--output", str(target), "--inventory-age-ms", "60000"], capture_output=True, text=True)
            self.assertEqual(run.returncode, 0, run.stderr)
            output = target.read_text()
            self.assertEqual(json.loads(output)["inputSha256"], hashlib.sha256(raw).hexdigest())
            for secret_id in ("991234501", "881234501", '"lotId"', '"actualOrderId"', '"orderId"'):
                self.assertNotIn(secret_id, output)
            self.assertIn("extractor_attestation_only", output)


if __name__ == "__main__":
    unittest.main()
