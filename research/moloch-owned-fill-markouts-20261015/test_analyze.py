"""Synthetic causal-book and exact-depth fixtures; no account data."""
from datetime import datetime, timedelta, timezone
import copy
import hashlib
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from analyze import Rejected, analyze, native_row, native_sealed_object

BASE = datetime(2026, 10, 15, tzinfo=timezone.utc)
BASE_MS = int(BASE.timestamp()) * 1000


def stamp(ms):
    return (BASE + timedelta(milliseconds=ms)).isoformat(timespec="milliseconds").replace("+00:00", "Z")


def tick(ms):
    return {"type": "tick", "receivedAt": stamp(ms), "processedAt": stamp(ms)}


def book(source, *, received=None, processed=None, bid="0.4", ask="0.6", quantity=10, bids=None, asks=None):
    received = source if received is None else received
    processed = received if processed is None else processed
    return {"type": "book", "receivedAt": stamp(received), "processedAt": stamp(processed),
            "book": {"tournamentId": "synthetic", "marketId": "10", "exchangeId": "21", "sequence": str(source + 1),
                     "sourceAt": stamp(source), "requestStartedAt": stamp(received), "receivedAt": stamp(received),
                     "transport": "official_sig_realtime",
                     "bids": bids or [{"price": bid, "quantity": quantity}],
                     "asks": asks or [{"price": ask, "quantity": quantity}]}}


def actual(*, qid=1, side="YES", action="buy", quantity=3, price="0.45", executed=10000, observed=12000, decision=1000):
    shared = {"orderId": qid, "actualOrderId": str(991234500 + qid), "marketId": "10", "exchangeId": "21",
              "side": side, "action": action, "quantity": quantity, "price": price, "fee": "0.000000",
              "executedAt": stamp(executed), "receiptHash": "b" * 64}
    event = {**shared, "type": "actual_fill", "source": "official_sig_api_v1", "fillId": str(881234500 + qid),
             "receivedAt": stamp(observed), "processedAt": stamp(observed)}
    fill = {**shared, "sourceId": event["fillId"], "counterfactual": False, "at": stamp(observed)}
    order = {"id": qid, "exchangeId": "21", "marketId": "10", "side": side, "action": action,
             "quantity": quantity, "filled": quantity, "price": price, "createdAt": BASE_MS + decision}
    return event, fill, order


def encode(events):
    previous, rows = "", []
    for n, event in enumerate(events, 1):
        event = {**event, "sourceRef": {"id": "synthetic", "sequence": n, "hash": "d" * 64}}
        body = {"event": event, "previous": previous, "sequence": n}
        packed = json.dumps(body, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
        previous = hashlib.sha256(packed).hexdigest()
        rows.append(json.dumps({**body, "hash": previous}, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode() + b"\n")
    return b"".join(rows), previous


def seal(value):
    packed = json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
    return json.dumps({**value, "hash": hashlib.sha256(packed).hexdigest()}, ensure_ascii=False,
                      sort_keys=True, separators=(",", ":")).encode() + b"\n"


def fixture(events, fills, orders):
    raw, head = encode(events)
    snapshot = {"sequence": len(events), "previous": head, "fills": fills, "orders": orders,
                "report": {"at": events[-1].get("processedAt", events[-1]["receivedAt"]), "fees": "0",
                           "executionMode": "live", "fillModel": "actualAccountFills", "fills": len(fills)}}
    return raw, snapshot


def run(events, fill, order, *, age=5000, window=1000):
    raw, snapshot = fixture(events, [fill], [order])
    return analyze(io.BytesIO(raw), snapshot, max_book_age_ms=age, post_window_ms=window)


class MarkoutTests(unittest.TestCase):
    def test_yes_buy_full_quantity_marks_and_quoted_gap_are_not_top_of_book(self):
        e, f, q = actual()
        events = [book(500, asks=[{"price": "0.6", "quantity": 1}, {"price": "0.65", "quantity": 4}]),
                  book(9000, bid="0.45", bids=[{"price": "0.45", "quantity": 1}, {"price": "0.4", "quantity": 2}]),
                  book(11000, bids=[{"price": "0.4", "quantity": 1}, {"price": "0.35", "quantity": 2}]), e,
                  book(15000, bid="0.5"), book(40000, bid="0.3"), book(70000, bid="0.55"), tick(80000)]
        r = run(events, f, q)
        self.assertEqual(r["all"]["quotedGap"]["oppositeSideGapValue"], "0.550000")
        self.assertEqual(r["all"]["preExecutionImmediateClose"]["hypotheticalMarkout"], "-0.100000")
        self.assertEqual(r["all"]["horizons"]["1000"]["hypotheticalMarkout"], "-0.250000")
        self.assertEqual(r["all"]["horizons"]["5000"]["hypotheticalMarkout"], "0.150000")
        self.assertEqual(r["all"]["horizons"]["30000"]["hypotheticalMarkout"], "-0.450000")
        self.assertEqual(r["all"]["horizons"]["60000"]["hypotheticalMarkout"], "0.300000")
        self.assertEqual(r["all"]["horizons"]["1000"]["changeFromPreExecutionCloseValue"], "-0.150000")
        self.assertEqual(r["all"]["horizons"]["1000"]["postBookObservedBeforeFillObservation"], 1)
        self.assertEqual(r["all"]["fillObservationDelayMs"]["min"], "2000.000000")

    def test_no_buy_liquidation_uses_complement_of_yes_asks_at_each_depth(self):
        e, f, q = actual(side="NO", price="0.35")
        post = book(11000, asks=[{"price": "0.7", "quantity": 1}, {"price": "0.75", "quantity": 2}])
        r = run([book(500), book(9000), post, e, tick(80000)], f, q)
        mark = r["byActionSide"]["buy_NO"]["horizons"]["1000"]
        self.assertEqual(mark["hypotheticalMarkout"], "-0.250000")  # .3 + 2*.25 - 3*.35
        self.assertEqual(mark["fullQuantityCoveredShares"], 3)

    def test_no_sell_replacement_uses_complement_of_yes_bids(self):
        e, f, q = actual(side="NO", action="sell", price="0.9")
        post = book(11000, bids=[{"price": "0.2", "quantity": 1}, {"price": "0.15", "quantity": 2}])
        r = run([book(500), book(9000), post, e, tick(80000)], f, q)
        self.assertEqual(r["byActionSide"]["sell_NO"]["horizons"]["1000"]["hypotheticalMarkout"], "0.200000")

    def test_yes_sell_is_proceeds_minus_full_depth_replacement_cost(self):
        e, f, q = actual(action="sell", price="0.65")
        post = book(11000, asks=[{"price": "0.6", "quantity": 1}, {"price": "0.7", "quantity": 2}])
        r = run([book(500), book(9000), post, e, tick(80000)], f, q)
        self.assertEqual(r["all"]["horizons"]["1000"]["hypotheticalMarkout"], "-0.050000")

    def test_old_source_received_after_fill_never_gets_pre_fill_credit(self):
        e, f, q = actual()
        historical = book(9000, received=12100, processed=12100)
        r = run([e, historical, tick(80000)], f, q)
        self.assertEqual(r["all"]["preExecutionCoverage"], {"no_causally_earlier_book": 1})
        self.assertEqual(r["all"]["preDecisionCoverage"], {"no_causally_earlier_book": 1})
        self.assertEqual(r["all"]["historicalBookUpdatesExcludedFromPreExecution"], 1)

    def test_old_source_received_before_fill_but_processed_after_execution_is_excluded(self):
        e, f, q = actual()
        delayed = book(9000, received=9500, processed=10500)
        r = run([delayed, e, tick(80000)], f, q)
        self.assertEqual(r["all"]["preExecutionCoverage"], {"no_causally_earlier_book": 1})
        self.assertEqual(r["all"]["historicalBookUpdatesExcludedFromPreExecution"], 1)

    def test_first_shallow_post_book_is_censored_not_replaced_with_profitable_later_depth(self):
        e, f, q = actual()
        r = run([book(9000), book(11000, quantity=1), book(11200, bid="0.8", ask="0.9"), e, tick(80000)], f, q)
        first = r["all"]["horizons"]["1000"]
        self.assertEqual(first["coverageFills"], {"insufficient_full_quantity_depth": 1})
        self.assertEqual(first["fullQuantityCoveredShares"], 0)
        self.assertEqual(first["hypotheticalMarkout"], "0.000000")
        self.assertIsNone(first["quantityWeightedMarkoutPerShare"])

    def test_stale_source_request_or_processing_clock_blocks_credit(self):
        e, f, q = actual()
        for key, value in (("sourceAt", stamp(1000)), ("requestStartedAt", stamp(1000))):
            b = book(9000)
            b["book"][key] = value
            r = run([b, e, tick(80000)], f, q, age=2000)
            self.assertEqual(r["all"]["preExecutionCoverage"], {"stale_clock": 1})
        b = book(9000, received=9000, processed=11500)
        r = run([b, e, tick(80000)], f, q, age=2000)
        self.assertEqual(r["observedBookQualityCounts"], {"stale_clock": 1})

    def test_gap_invalidates_pre_execution_until_new_full_book(self):
        e, f, q = actual()
        gap = {"type": "gap", "exchangeId": "21", "receivedAt": stamp(9500), "processedAt": stamp(9500)}
        r = run([book(9000), gap, book(11000), e, tick(80000)], f, q)
        self.assertEqual(r["all"]["preExecutionCoverage"], {"invalidated_by_gap": 1})
        self.assertEqual(r["all"]["horizons"]["1000"]["coverageFills"], {"priced": 1})

    def test_right_censoring_and_completed_window_missing_are_distinct(self):
        e, f, q = actual()
        early = run([book(9000), e], f, q, window=2000)
        self.assertEqual(early["all"]["horizons"]["1000"]["coverageFills"], {"right_censored_inside_window": 1})
        self.assertEqual(early["all"]["horizons"]["5000"]["coverageFills"], {"right_censored_before_horizon": 1})
        completed = run([book(9000), e, tick(80000)], f, q, window=2000)
        self.assertEqual(completed["all"]["horizons"]["1000"]["coverageFills"], {"no_book_in_completed_window": 1})

    def test_duplicate_owned_event_is_not_a_second_fill_and_public_trade_is_ignored(self):
        e, f, q = actual()
        duplicate = {**e, "receivedAt": stamp(13000), "processedAt": stamp(13000), "receiptHash": "c" * 64}
        public = {"type": "trade", "receivedAt": stamp(14000), "processedAt": stamp(14000), "quantity": 99999}
        r = run([book(9000), e, duplicate, public, tick(80000)], f, q)
        self.assertEqual(r["all"]["fills"], 1)
        self.assertEqual(r["journalEventCounts"]["duplicateActualFillEvents"], 1)

    def test_changed_duplicate_snapshot_fill_or_reordered_chain_rejects(self):
        e, f, q = actual()
        duplicate = {**e, "quantity": 2, "processedAt": stamp(13000)}
        with self.assertRaisesRegex(Rejected, "JOURNAL_FILL_FACTS"):
            run([e, duplicate, tick(80000)], f, q)
        raw, snapshot = fixture([e, tick(80000)], [f], [q])
        corrupted = raw.replace(b'"quantity":3', b'"quantity":2', 1)
        with self.assertRaisesRegex(Rejected, "JOURNAL_BODY_HASH"):
            analyze(io.BytesIO(corrupted), snapshot, max_book_age_ms=5000, post_window_ms=1000)
        with self.assertRaisesRegex(Rejected, "JOURNAL_SEQUENCE_OR_PREVIOUS"):
            analyze(io.BytesIO(b"".join(raw.splitlines(keepends=True)[::-1])), snapshot, max_book_age_ms=5000, post_window_ms=1000)

    def test_raw_body_hash_keeps_nested_hashes_negative_zero_exponents_unicode_and_escapes(self):
        # Exact lexemes must survive. Standard Python JSON would render 1e-7
        # as 1e-07; float/string recanonicalization is deliberately not used.
        event = '{"hash":"nested","n":-0.0,"p":1e-7,"s":"quote\\\" hash\\n","u":"🐉"}'
        body = '{"event":' + event + ',"previous":"","sequence":1}'
        digest = hashlib.sha256(body.encode()).hexdigest()
        raw = ('{"event":' + event + ',"hash":"' + digest + '","previous":"","sequence":1}\n').encode()
        self.assertEqual(native_row(raw)["hash"], digest)
        with self.assertRaisesRegex(Rejected, "JOURNAL_BODY_HASH"):
            native_row(raw.replace(b"1e-7", b"1e-07"))
        with self.assertRaisesRegex(Rejected, "NATIVE_ROW_(LAYOUT|FIELDS)"):
            native_row(raw.replace(b'{"event":', b'{"event" :', 1))

    def test_stream_consumes_exact_snapshot_prefix_not_future_tail(self):
        e, f, q = actual()
        raw, snapshot = fixture([book(9000), e, tick(80000)], [f], [q])
        class ReadlineOnly(io.BytesIO):
            def read(self, *args):
                raise AssertionError("nonstreaming read")
            def seek(self, *args):
                raise AssertionError("seek")
        result = analyze(ReadlineOnly(raw + b"unconsumed live tail"), snapshot, max_book_age_ms=5000, post_window_ms=1000)
        self.assertEqual(result["source"]["journalPrefixBytes"], len(raw))
        self.assertEqual(result["source"]["journalPrefixSha256"], hashlib.sha256(raw).hexdigest())
        self.assertTrue(result["source"]["rawNativeBodyHashChainVerified"])
        self.assertFalse(result["source"]["nativeHelperRun"])

    def test_report_seal_checks_exact_source_bytes_and_detects_changed_snapshot(self):
        e, f, q = actual()
        _, snapshot = fixture([book(9000), e, tick(80000)], [f], [q])
        original = seal(snapshot)
        parsed = native_sealed_object(original)
        self.assertEqual(parsed["sequence"], snapshot["sequence"])
        with self.assertRaisesRegex(Rejected, "JOURNAL_BODY_HASH"):
            native_sealed_object(original.replace(b'"fills":1', b'"fills":2'))
        with self.assertRaisesRegex(Rejected, "NATIVE_ROW_FIELDS"):
            native_sealed_object(json.dumps(parsed).encode() + b"\n")

    def test_missing_snapshot_fill_nonzero_fee_and_future_book_are_rejected_or_unpriced(self):
        e, f, q = actual()
        with self.assertRaisesRegex(Rejected, "SNAPSHOT_FILL_MISSING_FROM_JOURNAL"):
            run([tick(80000)], f, q)
        bad = copy.deepcopy(f)
        bad["fee"] = "0.005"
        with self.assertRaisesRegex(Rejected, "FILL_PRICE_OR_FEE"):
            run([e, tick(80000)], bad, q)
        future = book(11000, received=9000, processed=9000)
        r = run([future, e, tick(80000)], f, q)
        self.assertEqual(r["all"]["preExecutionCoverage"], {"invalid_clock": 1})
        self.assertEqual(r["all"]["horizons"]["1000"]["coverageFills"], {"invalid_clock": 1})

    def test_two_actual_entries_have_quantity_weighted_marks_and_distinct_clocks(self):
        e1, f1, q1 = actual(quantity=1, price="0.45", observed=12000)
        e2, f2, q2 = actual(qid=2, quantity=3, price="0.5", observed=13000)
        events = [book(9000), book(11000, bid="0.4"), e1, e2, tick(80000)]
        raw, snapshot = fixture(events, [f1, f2], [q1, q2])
        r = analyze(io.BytesIO(raw), snapshot, max_book_age_ms=5000, post_window_ms=1000)
        horizon = r["all"]["horizons"]["1000"]
        self.assertEqual(horizon["hypotheticalMarkout"], "-0.350000")
        self.assertEqual(horizon["quantityWeightedMarkoutPerShare"], "-0.087500")
        self.assertEqual(r["all"]["fillObservationDelayMs"]["quantityWeightedMean"], "2750.000000")
        self.assertEqual(r["all"]["orderDecisionToFillExecutionDelayMs"]["min"], "9000.000000")
        self.assertEqual(r["all"]["fills"], 2)

    def test_pre_book_same_timestamp_after_recorded_fill_is_not_causal(self):
        e, f, q = actual(observed=10000)
        r = run([e, book(10000), tick(80000)], f, q)
        self.assertEqual(r["all"]["preExecutionCoverage"], {"no_causally_earlier_book": 1})

    def test_cli_public_output_has_no_private_identifiers_and_binds_input_hash(self):
        e, f, q = actual()
        raw, snapshot = fixture([book(9000), e, tick(80000)], [f], [q])
        snapshot["privateProfile"] = "SYNTHETIC_PRIVATE_PROFILE"
        with tempfile.TemporaryDirectory() as folder:
            jp, rp, op = (Path(folder) / n for n in ("private-events.ndjson", "private-report.json", "public.json"))
            jp.write_bytes(raw)
            rp.write_bytes(seal(snapshot))
            args = [sys.executable, str(Path(__file__).with_name("analyze.py")), "--journal", str(jp), "--report", str(rp),
                    "--max-book-age-ms", "5000", "--post-window-ms", "1000", "--output", str(op)]
            completed = subprocess.run(args, capture_output=True, text=True)
            self.assertEqual(completed.returncode, 0, completed.stderr)
            public = op.read_text(encoding="utf-8")
            result = json.loads(public)
            self.assertEqual(result["source"]["reportSnapshotSha256"], hashlib.sha256(rp.read_bytes()).hexdigest())
            self.assertTrue(result["source"]["reportRawBodySealVerified"])
            for private in ("991234501", "881234501", "SYNTHETIC_PRIVATE_PROFILE", folder, "actualOrderId", "sourceId", "receiptHash"):
                self.assertNotIn(private, public)


if __name__ == "__main__":
    unittest.main()
