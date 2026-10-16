import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from crossvenue_observer import Study, Tail, compact, stamp, decimal


def row(at, seq=1, market="1", bid="0.400000", ask="0.420000", source=None):
    return {"sequence": seq, "hash": str(seq) * 64, "event": {"type": "book", "book": {
        "marketId": market, "exchangeId": "101", "sequence": str(seq), "sourceAt": stamp(source if source is not None else at),
        "receivedAt": stamp(at), "bids": [{"price": bid, "quantity": 10}], "asks": [{"price": ask, "quantity": 10}]}}}


def external(at, bid="0.410000", ask="0.430000", mapping="direct_party_reference"):
    return {"markets": [{"ticker": "RACE-26-D", "sigMarketIds": ["1"], "mappingKind": mapping,
            "receivedAt": stamp(at), "bodySha256": "a" * 64,
            "normalized": {"yesBids": [{"price": bid, "quantity": "2.500000"}],
                           "yesAsks": [{"price": ask, "quantity": "3.250000"}]}}]}


class ObserverTests(unittest.TestCase):
    def test_later_original_source_book_labels_exact_crossing_cost_without_fill_claim(self):
        out = []
        s = Study(out.append, horizons=(1000,))
        s.external_update(external(9000), 10000)
        s.accept(row(10000), 10000)
        s.accept(row(11000, 2, bid="0.430000", ask="0.450000"), 11000)
        label = next(v for v in out if v["kind"] == "horizon_label")
        self.assertEqual(label["yesOneShareTakerRoundTrip"], "0.010000")
        self.assertEqual(label["noOneShareTakerRoundTrip"], "-0.050000")
        self.assertFalse(label["actualFill"])

    def test_delayed_old_source_and_early_received_do_not_supply_future_label(self):
        out = []
        s = Study(out.append, horizons=(1000,))
        s.external_update(external(9000), 10000)
        s.accept(row(10000), 10000)
        s.accept(row(11500, 2, source=10500), 11500)
        self.assertFalse(any(v["kind"] == "horizon_label" for v in out))

    def test_missing_timely_future_book_is_missing_not_zero_return(self):
        out = []
        s = Study(out.append, horizons=(1000,))
        s.external_update(external(9000), 10000)
        s.accept(row(10000), 10000)
        s.accept(row(16000, 2), 16000)
        self.assertEqual(s.counts["labelsMissingTimelyBook"], 1)
        self.assertFalse(any(v["kind"] == "horizon_label" for v in out))

    def test_external_observation_keeps_original_local_availability(self):
        out = []
        s = Study(out.append)
        s.external_update(external(9000), 10000)
        s.external_update(external(9000), 11000)
        self.assertEqual(s.external["1"]["availableMs"], 10000)
        s.external_update(external(8000), 12000)
        self.assertEqual(s.external["1"]["receivedMs"], 9000)
        s.external_update(external(13000), 12000)
        self.assertEqual(s.external["1"]["receivedMs"], 9000)

    def test_stale_source_is_never_retimed_or_used_for_decision(self):
        out = []
        s = Study(out.append)
        s.external_update(external(9000), 10000)
        s.accept(row(10000), 14000)
        self.assertEqual(out, [])
        s.accept(row(40000, 2), 40000)
        self.assertEqual(out, [])

    def test_candidate_mapping_stays_analog_without_pricing_authority(self):
        out = []
        s = Study(out.append)
        s.external_update(external(9000, mapping="candidate_reference_analog"), 10000)
        s.accept(row(10000), 10000)
        self.assertEqual(out[0]["external"]["mappingKind"], "candidate_reference_analog")
        self.assertFalse(out[0]["pricingAuthority"])
        self.assertFalse(out[0]["executionAuthority"])

    def test_duplicate_sequence_and_coverage_boundary_do_not_create_markout_path(self):
        out = []
        s = Study(out.append, horizons=(1000,))
        s.external_update(external(9000), 10000)
        s.accept(row(10000), 10000)
        s.accept(row(11000), 11000)
        self.assertEqual(s.counts["sigDuplicateOrReordered"], 1)
        s.accept({"event": {"type": "coverage"}}, 11000)
        s.accept(row(12000, 2), 12000)
        self.assertFalse(any(v["kind"] == "horizon_label" for v in out))

    def test_complete_trade_tape_coverage_does_not_invalidate_prospective_paths(self):
        out = []
        s = Study(out.append, horizons=(1000,))
        s.external_update(external(9000), 10000)
        s.accept(row(10000), 10000)
        s.accept({"event": {"type": "coverage", "complete": True, "marketId": "1", "exchangeId": "101"}}, 10500)
        s.accept(row(11000, 2), 11000)
        self.assertEqual(s.counts["labels"], 1)

    def test_empty_depth_and_fractional_source_preserve_exact_decimal(self):
        self.assertEqual(str(decimal("0.010001")), "0.010001")
        for bad in [True, .5, "NaN", "-1", "0.0000001"]:
            with self.assertRaises(ValueError):
                decimal(bad)
        s = Study(lambda v: None)
        e = external(9000)
        e["markets"][0]["normalized"]["yesAsks"] = []
        s.external_update(e, 10000)
        self.assertEqual(s.external, {})

    def test_tail_checks_fresh_suffix_hash_and_chain_and_rejects_truncation(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "journal"
            p.write_bytes(b"historical bytes are not training features\n")
            tail = Tail(p)
            try:
                r = {"sequence": 12, "previous": "a" * 64, "event": {"type": "coverage"}}
                r["hash"] = hashlib.sha256(compact(r).encode()).hexdigest()
                with p.open("ab") as f:
                    f.write((compact(r) + "\n").encode())
                self.assertEqual(list(tail.read()), [r])
                r2 = {"sequence": 14, "previous": r["hash"], "event": {"type": "coverage"}}
                r2["hash"] = hashlib.sha256(compact(r2).encode()).hexdigest()
                with p.open("ab") as f:
                    f.write((compact(r2) + "\n").encode())
                with self.assertRaisesRegex(ValueError, "GAP"):
                    list(tail.read())
                p.write_bytes(b"")
                with self.assertRaisesRegex(ValueError, "CHANGED"):
                    list(tail.read())
            finally:
                tail.close()

    def test_tail_does_not_parse_half_written_record_or_replay_old_partial(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "journal"
            p.write_bytes(b"old partial")
            tail = Tail(p)
            try:
                r = {"sequence": 1, "previous": "a" * 64, "event": {"type": "coverage"}}
                r["hash"] = hashlib.sha256(compact(r).encode()).hexdigest()
                encoded = (compact(r) + "\n").encode()
                with p.open("ab") as f:
                    f.write(b" remainder\n" + encoded[:20])
                self.assertEqual(list(tail.read()), [])
                with p.open("ab") as f:
                    f.write(encoded[20:])
                self.assertEqual(list(tail.read()), [r])
            finally:
                tail.close()

    def test_tail_preserves_original_nested_number_bytes_for_hash_verification(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "journal"
            p.write_bytes(b"")
            tail = Tail(p)
            try:
                unsigned = b'{"event":{"type":"book","p":0.30000000000000004,"q":1e-6},"previous":"","sequence":1}'
                digest = hashlib.sha256(unsigned).hexdigest().encode()
                encoded = unsigned.replace(b',"previous":', b',"hash":"' + digest + b'","previous":')
                with p.open("ab") as f:
                    f.write(encoded + b"\n")
                self.assertEqual(len(list(tail.read())), 1)
            finally:
                tail.close()

    def test_breadth_converts_only_official_tick_roundoff_and_preserves_revision_gaps(self):
        out = []
        s = Study(out.append, horizons=(1000,))
        s.external_update(external(9000), 10000)
        def frame(at, revision, previous, price=.30000000000000004):
            return {"hash": "b" * 64, "sequence": revision, "event": {"source": "official_sig_realtime", "event": "markets_batch", "receivedAt": stamp(at),
                    "payload": {"markets": [{"marketId": "1", "batch": {"delivery": {"model": "best-effort-authoritative-resync", "revision": revision, "previousRevision": previous},
                    "books": [{"exchangeId": 101, "asOf": {"sequence": revision, "at": stamp(at)}, "bids": [{"price": price, "quantity": 2}], "asks": [{"price": .32, "quantity": 2}]}]}}]}}}
        s.breadth(frame(10000, 1, 0), 10000)
        self.assertEqual(out[0]["sig"]["features"]["bid"], "0.300000")
        s.breadth(frame(11000, 3, 2), 11000)
        self.assertFalse(any(v["kind"] == "horizon_label" for v in out))
        with self.assertRaisesRegex(ValueError, "OFFICIAL_TICK"):
            s.breadth(frame(12000, 4, 3, .30001), 12000)

    def test_invalid_websocket_state_is_never_treated_as_current_external_book(self):
        s = Study(lambda v: None)
        e = external(9000)
        e["source"] = "kalshi_official_ws_v2"
        e["markets"][0]["valid"] = False
        s.external_update(e, 10000)
        self.assertEqual(s.external, {})
        e["markets"][0]["valid"] = True
        e["markets"][0]["stateChainSha256"] = "c" * 64
        s.external_update(e, 10000)
        self.assertEqual(s.external["1"]["hashKind"], "state_chain")
        self.assertEqual(s.external["1"]["bodySha256"], "c" * 64)


if __name__ == "__main__":
    unittest.main()
