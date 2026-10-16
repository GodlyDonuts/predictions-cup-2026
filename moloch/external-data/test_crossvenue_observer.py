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


def websocket(at, sequence=1, sid=1, sha="c"*64, **kwargs):
    e = external(at, **kwargs)
    e["source"] = "kalshi_official_ws_v2"
    e["markets"][0].update(valid=True, sid=sid, sequence=sequence, stateChainSha256=sha, sourceTsMs=at)
    return e


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
        e["markets"][0].update(sid=1, sequence=1, sourceTsMs=9000)
        s.external_update(e, 10000)
        self.assertEqual(s.external["1"]["hashKind"], "state_chain")
        self.assertEqual(s.external["1"]["bodySha256"], "c" * 64)

    def test_same_millisecond_websocket_advances_require_increasing_original_cursor(self):
        s = Study(lambda v: None)
        s.external_update(websocket(9000), 10000)
        s.external_update(websocket(9000, sequence=2, sha="d"*64, bid="0.420000"), 10001)
        self.assertEqual(s.external["1"]["sequence"], 2)
        self.assertEqual(s.external["1"]["receivedMs"], 9000)
        self.assertEqual(s.external["1"]["availableMs"], 10001)
        s.external_update(websocket(9000, sequence=2, sha="d"*64, bid="0.420000"), 11000)
        self.assertEqual(s.external["1"]["availableMs"], 10001)
        self.assertEqual(len(s.history["1"]), 2)

    def test_conflicting_same_cursor_quarantines_market_without_global_crash(self):
        s = Study(lambda v: None)
        s.external_update(websocket(9000), 10000)
        s.external_update(websocket(9000, sha="d"*64), 10001)
        self.assertNotIn("1", s.external)
        self.assertEqual(s.counts["externalConflicts"], 1)
        s.external_update(websocket(9000), 10002)
        self.assertNotIn("1", s.external)
        s.external_update(websocket(9000, sequence=2, sha="e"*64), 10003)
        self.assertEqual(s.external["1"]["sequence"], 2)

    def test_newer_rest_does_not_replace_valid_fresh_websocket_or_clear_history(self):
        s = Study(lambda v: None)
        s.external_update(websocket(10000), 10000)
        for at in (11000, 12000, 13000):
            s.external_update(external(at), at)
            self.assertEqual(s.external["1"]["source"], "kalshi_official_ws_v2")
        self.assertEqual(len(s.history["1"]), 1)
        self.assertEqual(s.external["1"]["availableMs"], 10000)

    def test_invalid_ws_withdraws_immediately_falls_back_then_return_clears_once(self):
        s = Study(lambda v: None)
        s.external_update(external(9000), 10000)
        s.external_update(websocket(10000), 10000)
        s.external_update(websocket(11000, sequence=2, sha="d"*64), 11000)
        self.assertEqual(len(s.history["1"]), 2)
        invalid = websocket(11000, sequence=2, sha="d"*64)
        invalid["markets"][0]["valid"] = False
        s.external_update(invalid, 12000)
        self.assertEqual(s.external["1"]["source"], "kalshi_official_v2")
        self.assertEqual(s.external["1"]["availableMs"], 10000)
        self.assertEqual(len(s.history["1"]), 1)
        s.external_update(websocket(13000, sequence=3, sha="e"*64), 13000)
        self.assertEqual(s.external["1"]["source"], "kalshi_official_ws_v2")
        self.assertEqual(len(s.history["1"]), 1)
        s.external_update(websocket(14000, sequence=4, sha="f"*64), 14000)
        self.assertEqual(len(s.history["1"]), 2)

    def test_stale_ws_original_source_uses_original_rest_without_clock_renewal(self):
        s = Study(lambda v: None)
        s.external_update(websocket(10000), 10000)
        s.external_update(external(29000), 29000)
        self.assertEqual(s.external["1"]["source"], "kalshi_official_ws_v2")
        s.accept(row(30001), 30001)
        self.assertEqual(s.external["1"]["source"], "kalshi_official_v2")
        self.assertEqual(s.external["1"]["receivedMs"], 29000)
        self.assertEqual(s.external["1"]["availableMs"], 29000)
        future = websocket(30001, sequence=2)
        future["markets"][0]["sourceTsMs"] = 30002
        s.external_update(future, 30001)
        self.assertEqual(s.external["1"]["source"], "kalshi_official_v2")

    def test_source_clock_ahead_of_receipt_can_be_used_only_after_original_clock_catches_up(self):
        s = Study(lambda v: None)
        e = websocket(10000)
        e["markets"][0]["sourceTsMs"] = 10014
        s.external_update(e, 10014)
        self.assertEqual(s.external["1"]["sourceTsMs"], 10014)
        self.assertEqual(s.external["1"]["receivedMs"], 10000)

    def test_invalid_ws_without_rest_removes_previous_selected_state(self):
        s = Study(lambda v: None)
        s.external_update(websocket(10000), 10000)
        e = websocket(10000); e["markets"][0]["valid"] = False
        s.external_update(e, 10001)
        self.assertEqual(s.external, {})
        self.assertEqual(len(s.history["1"]), 0)

    def test_reordered_ws_cursor_does_not_replace_or_renew_current_observation(self):
        s = Study(lambda v: None)
        s.external_update(websocket(10000, sequence=3), 10000)
        s.external_update(websocket(10000, sequence=2, sha="d"*64), 10001)
        self.assertEqual(s.external["1"]["sequence"], 3)
        self.assertEqual(s.external["1"]["availableMs"], 10000)
        self.assertEqual(s.counts["externalClockRegression"], 1)
        s.external_update(websocket(11000, sequence=1, sid=2, sha="e"*64), 11000)
        self.assertEqual(len(s.history["1"]), 1)
        self.assertEqual(s.external["1"]["sid"], 2)

    def test_conflict_affects_only_bound_market_and_caches_are_bounded(self):
        s = Study(lambda v: None)
        e = websocket(10000)
        for i in range(238):
            e["markets"][0]["sigMarketIds"] = [str(i)]
            e["markets"][0]["ticker"] = f'RACE-{i}'
            s.external_update(e, 10000)
        self.assertEqual(len(s.external_cache['kalshi_official_ws_v2']), 237)
        self.assertEqual(len(s.external_tickers), 237)
        self.assertNotIn('237', s.external)
        e["markets"][0]["sigMarketIds"] = ['1']
        e["markets"][0]["ticker"] = 'RACE-1'
        e["markets"][0]["stateChainSha256"] = 'd'*64
        s.external_update(e, 10001)
        self.assertNotIn('1', s.external)
        self.assertIn('2', s.external)

    def test_connection_epoch_scopes_same_sid_cursor_and_resets_history_once(self):
        s = Study(lambda v: None)
        def e(at, seq, epoch, sha):
            value = websocket(at, sequence=seq, sha=sha)
            value['websocket'] = {'coverage': {'connections': epoch}}
            return value
        s.external_update(e(10000, 200, 1, 'a'*64), 10000)
        s.external_update(e(11000, 201, 1, 'b'*64), 11000)
        self.assertEqual(len(s.history['1']), 2)
        s.external_update(e(12000, 1, 2, 'c'*64), 12000)
        self.assertEqual(s.external['1']['wsEpoch'], 2)
        self.assertEqual(len(s.history['1']), 1)
        s.external_update(e(13000, 2, 2, 'd'*64), 13000)
        self.assertEqual(len(s.history['1']), 2)
        s.external_update(e(13000, 2, 2, 'd'*64), 14000)
        self.assertEqual(s.external['1']['availableMs'], 13000)

    def test_process_epoch_reuse_or_missing_epoch_lower_sequence_is_unknown_boundary(self):
        for epoch in (None, 1):
            s = Study(lambda v: None)
            def e(at, seq, sha):
                value = websocket(at, sequence=seq, sha=sha)
                if epoch is not None: value['websocket'] = {'coverage': {'connections': epoch}}
                return value
            s.external_update(e(10000, 900, 'a'*64), 10000)
            s.external_update(e(11000, 901, 'b'*64), 11000)
            invalid = e(11000, 901, 'b'*64); invalid['markets'][0]['valid'] = False
            s.external_update(invalid, 12000)
            s.external_update(e(13000, 1, 'c'*64), 13000)
            self.assertEqual(s.external['1']['sequence'], 1)
            self.assertEqual(s.counts['externalUnknownNewStreams'], 1)
            self.assertEqual(len(s.history['1']), 1)
            s.external_update(e(14000, 2, 'd'*64), 14000)
            self.assertEqual(len(s.history['1']), 2)
            s.external_update(e(14000, 2, 'd'*64), 15000)
            self.assertEqual(s.external['1']['availableMs'], 14000)
            s.external_update(e(13000, 0, 'e'*64), 15000)
            self.assertEqual(s.external['1']['sequence'], 2)

    def test_later_valid_exported_sequence_reset_without_epoch_is_not_stuck_forever(self):
        s = Study(lambda v: None)
        s.external_update(websocket(10000, sequence=500), 10000)
        s.external_update(websocket(11000, sequence=1, sha='d'*64), 11000)
        self.assertEqual(s.external['1']['sequence'], 1)
        self.assertEqual(s.external['1']['streamBoundary'], 1)
        self.assertEqual(len(s.history['1']), 1)
        self.assertEqual(s.counts['externalUnknownNewStreams'], 1)

    def test_ws_thirty_second_history_survives_rest_fallback_without_source_splicing(self):
        out = []; s = Study(out.append)
        s.external_update(websocket(10000), 10000)
        s.external_update(external(25000, bid='0.600000', ask='0.620000'), 25000)
        self.assertEqual(s.external['1']['source'], 'kalshi_official_ws_v2')
        s.external_update(external(35000, bid='0.610000', ask='0.630000'), 35000)
        self.assertEqual(s.external['1']['source'], 'kalshi_official_v2')
        s.external_update(websocket(40000, sequence=2, sha='d'*64, bid='0.420000', ask='0.440000'), 40000)
        s.accept(row(40000), 40000)
        sample = out[-1]; anchor = sample['externalLagAnchors']['30000']
        self.assertEqual(anchor['receivedMs'], 10000)
        self.assertEqual(anchor['availableMs'], 10000)
        self.assertEqual(anchor['bodySha256'], 'c'*64)
        self.assertEqual(anchor['source'], 'kalshi_official_ws_v2')
        self.assertAlmostEqual(anchor['midpoint'], .42)
        self.assertIsNotNone(sample['externalLogitChanges']['30000'])
        self.assertIsNone(sample['externalLagAnchors']['5000'])
        self.assertEqual(len(s.source_histories[('1','kalshi_official_v2')][1]), 2)
        self.assertEqual(len(s.source_histories[('1','kalshi_official_ws_v2')][1]), 2)
        from crossvenue_economics import feature_vector
        from crossvenue_toxicity import feature_state
        self.assertIsNone(feature_vector(sample))  # Other missing lags are not imputed.
        self.assertEqual(feature_state(sample, 'scoped_v2'), (None, 'missingInnovation'))

    def test_reconnect_and_conflict_clear_only_ws_history_not_retained_rest(self):
        s = Study(lambda v: None)
        s.external_update(external(10000), 10000)
        s.external_update(websocket(11000), 11000)
        s.external_update(external(12000), 12000)
        s.external_update(websocket(13000, sequence=1, sid=2, sha='d'*64), 13000)
        self.assertEqual(len(s.source_histories[('1','kalshi_official_ws_v2')][1]), 1)
        self.assertEqual(len(s.source_histories[('1','kalshi_official_v2')][1]), 2)
        s.external_update(websocket(13000, sequence=1, sid=2, sha='e'*64), 14000)
        self.assertEqual(len(s.source_histories[('1','kalshi_official_ws_v2')][1]), 0)
        self.assertEqual(len(s.source_histories[('1','kalshi_official_v2')][1]), 2)

    def test_stale_or_future_first_available_lag_anchor_stays_missing(self):
        for mode in ('stale', 'future'):
            out = []; s = Study(out.append)
            s.external_update(websocket(10000), 10000)
            at = 70000 if mode == 'stale' else 40000
            if mode == 'future':
                s.source_histories[('1','kalshi_official_ws_v2')][1][0]['availableMs'] = at+1
            s.external_update(websocket(at, sequence=2, sha='d'*64), at)
            s.accept(row(at), at)
            self.assertIsNone(out[-1]['externalLagAnchors']['30000'])
            self.assertIsNone(out[-1]['externalLogitChanges']['30000'])


if __name__ == "__main__":
    unittest.main()
