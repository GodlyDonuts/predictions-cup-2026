import hashlib
import io
import contextlib
import gzip
import copy
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import crossvenue_observer as observer
from crossvenue_observer import Study, Tail, compact, stamp, decimal, FLOW_FIELDS, validate_trade_flow


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


def public_flow(at, direction=1, epoch=1):
    record = dict(zip(FLOW_FIELDS, ['public-test-trade', 'RACE-26-D', 'b'*64, at-104, at-90, at-100,
                                  3, 1, epoch, '1.250000', '0.3900', '0.6100', direction, False]))
    evidence = [record]
    digest = hashlib.sha256(json.dumps(evidence, separators=(',', ':'), ensure_ascii=False).encode()).hexdigest()
    window = {'observedTradeCount': 1, 'observedQuantity': '1.250000', 'knownSignedQuantity': '1.250000' if direction == 1 else '-1.250000' if direction == -1 else '0.000000',
              'signedQuantity': '1.250000' if direction == 1 else '-1.250000' if direction == -1 else None,
              'unknownDirectionQuantity': '1.250000' if direction is None else '0.000000', 'missingExecutionClockCount': 0,
              'receiptOnlyActivityQuantity': '0.000000', 'blockTradeCount': 0, 'unknownBlockClassificationCount': 0,
              'observedSubscriptionDurationMs': 30000, 'windowObservedSinceSubscription': True, 'completeTradeTape': False,
              'evidenceSha256': digest, 'evidence': evidence, 'evidenceComplete': True}
    return {'source': 'kalshi_official_ws_v2_public_trade', 'ticker': 'RACE-26-D', 'streamEpoch': epoch,
            'sid': 3, 'asOfMs': at, 'observing': True, 'windows': {str(ms): copy.deepcopy(window) for ms in (1000,5000,30000)},
            'bookFreshnessRenewed': False, 'pricingAuthority': False, 'executionAuthority': False}


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

    def test_duplicate_exact_version_and_coverage_boundary_do_not_create_markout_path(self):
        out = []
        s = Study(out.append, horizons=(1000,))
        s.external_update(external(9000), 10000)
        s.accept(row(10000), 10000)
        s.accept(row(11000, source=10000), 11000)
        self.assertEqual(s.counts["sigDuplicateOrReordered"], 1)
        s.accept({"event": {"type": "coverage"}}, 11000)
        s.accept(row(12000, 2), 12000)
        self.assertFalse(any(v["kind"] == "horizon_label" for v in out))

    def test_same_engine_sequence_new_original_read_time_supplies_later_label(self):
        out=[]; s=Study(out.append,horizons=(1000,))
        s.external_update(external(9000),10000); s.accept(row(10000,7),10000)
        original=row(11000,7)
        s.accept(original,11000)
        labels=[v for v in out if v['kind']=='horizon_label']
        self.assertEqual(len(labels),1)
        self.assertEqual((labels[0]['sourceAt'],labels[0]['receivedAt']),('1970-01-01T00:00:11.000Z',)*2)
        self.assertEqual(s.counts['sigBooks'],2)
        self.assertEqual(original,row(11000,7))

    def test_duplicate_batch_still_applies_new_book_version_without_revising_coverage(self):
        out=[]; s=Study(out.append,horizons=(1000,))
        s.external_update(external(9000),10000)
        def frame(at, revision=4, previous=3):
            return {'hash':'b'*64,'sequence':at,'event':{'source':'official_sig_realtime','event':'markets_batch','receivedAt':stamp(at),
                    'payload':{'markets':[{'marketId':'1','batch':{'delivery':{'model':'best-effort-authoritative-resync','revision':revision,'previousRevision':previous},
                    'books':[{'exchangeId':101,'asOf':{'sequence':7,'at':stamp(at)},'bids':[{'price':.4,'quantity':10}],'asks':[{'price':.42,'quantity':10}]}],
                    'trades':[{'id':'original-public-trade'}]}}]}}}
        s.breadth(frame(10000),10000)
        before=dict(s.revisions); boundaries=s.counts['coverageBoundaries']
        s.breadth(frame(11000),11000)
        self.assertEqual(s.revisions,before)
        self.assertEqual(s.counts['coverageBoundaries'],boundaries)
        self.assertEqual(s.counts['labels'],1)
        s.breadth(frame(12000,3,2),12000)
        self.assertEqual(s.revisions,before)
        self.assertEqual(s.counts['sigBooks'],3)

    def test_version_order_keeps_exact_submicrosecond_time_and_offset_equivalence(self):
        s=Study(lambda _:None,sampling_mode='trade')
        first=row(40000,7); first['event']['book']['sourceAt']='1970-01-01T00:00:39.123456700Z'
        s.accept(first,40000)
        newer=copy.deepcopy(first); newer['event']['book']['sourceAt']='1970-01-01T00:00:39.123456701Z'
        s.accept(newer,40001)
        self.assertEqual(s.counts['sigBooks'],2)
        equal=copy.deepcopy(newer); equal['event']['book']['sourceAt']='1969-12-31T19:00:39.123456701-05:00'
        equal['event']['book']['receivedAt']=stamp(40002)
        s.accept(equal,40002)
        self.assertEqual(s.counts['sigDuplicateOrReordered'],1)
        self.assertEqual(s.sig_current[('1','101')]['row'],newer)

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

    def test_original_flow_survives_rest_book_fallback_without_renewing_evidence(self):
        out=[];s=Study(out.append);s.external_update(websocket(10000),10000)
        s.external_update(external(40000),40000)
        e=websocket(10000);e['markets'][0].update(valid=False,invalidReason='stale',tradeFlow=public_flow(40000))
        e['websocket']={'coverage':{'connections':1}}
        s.external_update(e,40000);s.accept(row(40000),40000)
        sample=out[-1];flow=sample['externalTradeFlow']
        self.assertEqual(sample['external']['source'],'kalshi_official_v2')
        self.assertEqual(flow['inputs']['1000']['observedSignedQuantity'],'1.250000')
        self.assertEqual(flow['sourceSnapshot']['windows']['1000']['evidence'][0]['receivedMs'],39896)
        s.external_update(e,40100)
        self.assertEqual(s.external_flow['1']['availableMs'],40000)
        self.assertIsNone(s._flow_for_decision('1',42001))
        later=s._flow_for_decision('1',41000)
        self.assertIsNone(later['inputs']['1000']['observedSignedQuantity'])  # Actual execution aged out.
        self.assertEqual(later['inputs']['5000']['observedSignedQuantity'],'1.250000')

    def test_unknown_partial_or_missing_flow_never_becomes_complete_signed_input(self):
        for mode in ('unknown','partial','missingclock'):
            flow=public_flow(40000,direction=None if mode=='unknown' else 1)
            for w in flow['windows'].values():
                if mode=='partial':
                    w.update(observedTradeCount=2,observedQuantity='2.500000',knownSignedQuantity='2.500000',signedQuantity='2.500000',evidenceComplete=False,evidenceSha256='c'*64)
                if mode=='missingclock':w.update(missingExecutionClockCount=1,receiptOnlyActivityQuantity='1.000000',signedQuantity=None)
            e=websocket(40000);e['markets'][0]['tradeFlow']=flow;s=Study(lambda _:None);s.external_update(e,40000)
            result=s._flow_for_decision('1',40000)
            self.assertIsNotNone(result)
            self.assertIsNone(result['inputs']['1000']['observedSignedQuantity'])
            self.assertFalse(result['inputs']['1000']['completeTradeTape'])
            self.assertFalse(result['executionAuthority'])

    def test_invalid_flow_is_local_missing_not_observer_or_book_failure(self):
        for mode in ('future','receipt','hash','quantity','scope','epoch','coverage','sid'):
            flow=public_flow(40000);w=flow['windows']['1000'];r=w['evidence'][0]
            if mode=='future':flow['asOfMs']=40001
            elif mode=='receipt':r['availableMs']=40001
            elif mode=='hash':w['evidenceSha256']='d'*64
            elif mode=='quantity':w['signedQuantity']='2.000000'
            elif mode=='scope':flow['ticker']='OTHER'
            elif mode=='epoch':flow['streamEpoch']=2
            elif mode=='coverage':w['completeTradeTape']=True
            else:r['sid']=4
            s=Study(lambda _:None);e=websocket(40000);e['websocket']={'coverage':{'connections':1}};e['markets'][0]['tradeFlow']=flow
            s.external_update(e,40000)
            self.assertIsNone(s._flow_for_decision('1',40000),mode)
            self.assertIn('1',s.external)
            self.assertEqual(s.counts['tradeFlowInvalid'],1)

    def test_flow_disconnection_withdrawal_and_source_snapshot_digest_are_explicit(self):
        s=Study(lambda _:None);e=websocket(40000);flow=public_flow(40000);e['markets'][0]['tradeFlow']=flow
        s.external_update(e,40000);cached=s._flow_for_decision('1',40000)
        self.assertEqual(cached['sourceSnapshotSha256'],hashlib.sha256(compact(flow).encode()).hexdigest())
        flow['observing']=False;s.external_update(e,40001)
        self.assertIsNone(s._flow_for_decision('1',40001))


class PretriggerDrainTests(unittest.TestCase):
    @staticmethod
    def encoded(event, seq=1, previous=''):
        body={'event':event,'previous':previous,'sequence':seq}
        digest=hashlib.sha256(compact(body).encode()).hexdigest()
        return (compact({**body,'hash':digest})+'\n').encode(),digest
    def test_completed_ws_body_drains_original_sig_before_trade_and_uses_actual_later_clock(self):
        with tempfile.TemporaryDirectory() as d, contextlib.ExitStack() as stack, patch.object(observer.shutil,'disk_usage',return_value=type('D',(),{'free':40*1024**3})()):
            root=Path(d);journal=root/'sig.jsonl';journal.write_bytes(b'');tail=Tail(journal);stack.callback(tail.close)
            raw,_=self.encoded(row(39950,2)['event']);journal.write_bytes(raw)
            mapping=root/'map.json';mapping.write_bytes(b'{}');output=root/'capture';output.mkdir();clock=[40010]
            trace=observer.InputTrace(output,mapping,{'samplingMode':'trade'},tail.bookmark(),clock=lambda:clock[0]);stack.callback(trace.close)
            out=[];study=Study(out.append,sampling_mode='trade');study.accept(row(35000),35000)
            body=websocket(40000);body['markets'][0]['tradeFlow']=public_flow(40000);original=compact(body).encode()
            class Response(io.BytesIO):status=200
            def drain():
                events=[json.loads(x)['event'] for x in gzip.decompress((output/'input-trace-v1.jsonl.gz').read_bytes()).splitlines()]
                self.assertEqual(events[-1]['kind'],'poll_body');self.assertEqual(events[-1]['readCompletedAtMs'],clock[0])
                clock[0]+=40;observer.drain_sig_once(tail,study.accept,trace=trace,clock=lambda:clock[0]);clock[0]+=10
            observer.trace_cache_poll(study,'/v1/ws/quotes',trace,lambda *a,**k:Response(original),before_trade_apply=drain)
            self.assertEqual(len(out),1);sample=out[0];self.assertEqual(sample['decisionMs'],40060);self.assertEqual(sample['trigger']['sigBookObservedMs'],40050)
            self.assertEqual(sample['sig']['receivedAt'],stamp(39950));self.assertEqual(sample['trigger']['record']['sourceTsMs'],39900);self.assertEqual(sample['trigger']['sourceExportMs'],40000)
            self.assertEqual(sample['trigger']['observerFirstAvailableMs'],40060);self.assertEqual(len(study.trigger_seen),1)
            events=[json.loads(x)['event'] for x in gzip.decompress((output/'input-trace-v1.jsonl.gz').read_bytes()).splitlines()]
            self.assertEqual([e['inputKind'] for e in events if e['kind']=='apply_begin'],['sig_row','cache_body'])
            self.assertEqual(events[0]['sigDrainPolicy'],observer.SIG_DRAIN_POLICY)
            observer.trace_cache_poll(study,'/v1/ws/quotes',trace,lambda *a,**k:Response(original),before_trade_apply=drain)
            self.assertEqual(len(out),1);self.assertGreater(study.counts['tradeDuplicateSkip'],0)
    def test_drain_delay_cannot_backdate_decision_or_rescue_expired_trade(self):
        with tempfile.TemporaryDirectory() as d, contextlib.ExitStack() as stack, patch.object(observer.shutil,'disk_usage',return_value=type('D',(),{'free':40*1024**3})()):
            root=Path(d);journal=root/'sig';journal.write_bytes(b'');tail=Tail(journal);stack.callback(tail.close)
            raw,_=self.encoded(row(41850)['event']);journal.write_bytes(raw);mapping=root/'map';mapping.write_bytes(b'{}');output=root/'out';output.mkdir();clock=[40010]
            trace=observer.InputTrace(output,mapping,{'samplingMode':'trade'},tail.bookmark(),clock=lambda:clock[0]);stack.callback(trace.close)
            out=[];study=Study(out.append,sampling_mode='trade');body=websocket(40000);body['markets'][0]['tradeFlow']=public_flow(40000)
            class Response(io.BytesIO):status=200
            def drain():clock[0]=41901;observer.drain_sig_once(tail,study.accept,trace=trace,clock=lambda:clock[0])
            observer.trace_cache_poll(study,'/v1/ws/quotes',trace,lambda *a,**k:Response(compact(body).encode()),before_trade_apply=drain)
            self.assertEqual(out,[]);self.assertEqual(study.counts['tradeOldSkip'],1);self.assertEqual(len(study.trigger_seen),1)
    def test_invalid_body_and_book_mode_never_invoke_trade_drain(self):
        with tempfile.TemporaryDirectory() as d, contextlib.ExitStack() as stack, patch.object(observer.shutil,'disk_usage',return_value=type('D',(),{'free':40*1024**3})()):
            root=Path(d);mapping=root/'map';mapping.write_bytes(b'{}');output=root/'out';output.mkdir();trace=observer.InputTrace(output,mapping,{'samplingMode':'book'},{},clock=lambda:40000);stack.callback(trace.close)
            calls=[]
            class Response(io.BytesIO):status=200
            with self.assertRaises(json.JSONDecodeError):observer.trace_cache_poll(Study(lambda _:None,sampling_mode='trade'),'/v1/ws/quotes',trace,lambda *a,**k:Response(b'{'),before_trade_apply=lambda:calls.append(1))
            self.assertEqual(calls,[])
            observer.trace_cache_poll(Study(lambda _:None),'/v1/ws/quotes',trace,lambda *a,**k:Response(compact(websocket(40000)).encode()),before_trade_apply=lambda:calls.append(1))
            self.assertEqual(calls,[])
    def test_bounded_drain_does_not_skip_whole_lines_before_existing_rotation(self):
        with tempfile.TemporaryDirectory() as d, contextlib.ExitStack() as stack:
            base=Path(d)/'breadth.jsonl';base.write_bytes(b'');tail=Tail(base);stack.callback(lambda:tail.close());previous='';rows=[]
            for seq in range(1,1026):
                event={'type':'tick','receivedAt':stamp(0),'padding':''};encoded,_=self.encoded(event,seq,previous)
                event['padding']='x'*(4096-len(encoded));encoded,previous=self.encoded(event,seq,previous);self.assertEqual(len(encoded),4096);rows.append(encoded)
            base.write_bytes(b''.join(rows));next_path=base.with_name('breadth-part-0001.jsonl');next_path.write_bytes(self.encoded({'type':'tick'},1)[0]);seen=[];segment=0
            tail,segment=observer.drain_sig_once(tail,lambda r,now:seen.append((segment,r['sequence'])),base=base,clock=lambda:10000)
            self.assertEqual(segment,0);self.assertEqual(len(seen),1024);self.assertEqual(tail.position,4*1024*1024);self.assertEqual(tail.buffer,b'')
            tail,segment=observer.drain_sig_once(tail,lambda r,now:seen.append((0,r['sequence'])),base=base,segment=segment,clock=lambda:10001)
            self.assertEqual(segment,1);self.assertEqual(len(seen),1026);self.assertEqual([n for _,n in seen[:1025]],list(range(1,1026)));self.assertEqual(seen[-1][1],1)
    def test_rotated_callback_failure_closes_new_tail_and_retains_original_for_failure_seal(self):
        with tempfile.TemporaryDirectory() as d:
            base=Path(d)/'breadth.jsonl';base.write_bytes(b'');tail=Tail(base)
            try:
                next_path=base.with_name('breadth-part-0001.jsonl');next_path.write_bytes(self.encoded({'type':'tick'},1)[0])
                def fail(*args):raise ValueError('test callback failure')
                with self.assertRaisesRegex(ValueError,'test callback failure'):observer.drain_sig_once(tail,fail,base=base,clock=lambda:10000)
                self.assertEqual(tail.bookmark()['readOffset'],0)
                next_path.unlink()  # Windows also proves the newly opened file was closed.
            finally:tail.close()
    def test_tail_read_bound_refuses_zero_bool_or_excess_without_moving_cursor(self):
        with tempfile.TemporaryDirectory() as d, contextlib.ExitStack() as stack:
            p=Path(d)/'sig';p.write_bytes(b'');tail=Tail(p)
            try:
                for value in (0,True,4*1024*1024+1):
                    with self.subTest(value=value),self.assertRaisesRegex(ValueError,'OBSERVER_READ_BOUND'):list(tail.read(value))
                    self.assertEqual(tail.position,0)
            finally:tail.close()


if __name__ == "__main__":
    unittest.main()
