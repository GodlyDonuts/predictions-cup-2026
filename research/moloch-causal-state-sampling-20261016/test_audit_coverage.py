"""Synthetic genuine InputTrace/normal-seal fixtures; never actual V5 inputs."""
import copy
import gzip
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parents[1]/'moloch/external-data'))
import audit_coverage as a
from test_crossvenue_observer import row, external, public_flow
from test_trade_triggered_observer import envelope

V = a.checked_replay()
P_BYTES = (HERE/'protocol.json').read_bytes().replace(b'\r\n', b'\n')
P = json.loads(P_BYTES)
START = a.clock(P['enrollment']['startInclusiveUtc'])
END = a.clock(P['enrollment']['endExclusiveUtc'])
TAIL = a.clock(P['enrollment']['maximumMetadataObservationUtc'])


def pin(path):
    return {'path': str(path.absolute()), 'bytes': path.stat().st_size, 'sha256': a.sha(path.read_bytes())}


class Response(io.BytesIO):
    status = 200


class Fixture:
    def __init__(self, root, mapping=None):
        self.root = root; self.clock = START-300000
        self.s = V.known_sampler(V.BOOK_VERSION_LF)
        producer = root/'producer.py'
        producer.write_bytes(Path(self.s.__file__).read_bytes().replace(b'\r\n', b'\n'))
        self.s.__file__ = str(producer)
        self.mapping = root/'mapping.json'; self.mapping.write_text(self.s.compact(mapping or {'markets': []}), encoding='utf-8')
        self.source = root/'source.jsonl'; self.source.write_bytes(b'')
        self.tail = self.s.Tail(self.source)
        self.output = root/'capture'; self.output.mkdir()
        config = {'samplingMode': 'trade', 'format': 'native', 'journal': str(self.source),
                  'durationSeconds': 3600, 'horizonsMs': [1000, 3000, 10000, 30000, 120000],
                  'sigMaximumAgeMs': 3000, 'externalMaximumAgeMs': 20000, 'labelSlackMs': 3000}
        self.trace = self.s.InputTrace(self.output, self.mapping, config, self.tail.bookmark(), clock=lambda: self.clock)
        self.study = self.s.Study(self.trace.emit, sampling_mode='trade')

    def sig(self, at, source=None, gap=False, invalid=False):
        self.clock = at
        n = 1 if self.tail.sequence is None else self.tail.sequence+1
        event = {'type': 'gap', 'marketId': '1'} if gap else row(at, n, source=source)['event']
        if invalid:
            event['book']['invalid'] = True
        unsigned = {'event': event, 'previous': self.tail.previous or '', 'sequence': n}
        record = {**unsigned, 'hash': a.sha(self.s.compact(unsigned).encode())}
        with self.source.open('ab') as out:
            out.write((self.s.compact(record)+'\n').encode())
        for original in self.tail.read():
            self.trace.consume_sig(self.tail, original, self.study.accept, self.clock)

    def polls(self, at, identity='public-test-trade', flow=None, **kwargs):
        self.clock = at
        flow = copy.deepcopy(flow or public_flow(at))
        for window in flow['windows'].values():
            for record in window['evidence']:
                record['tradeId'] = identity
            window['evidenceSha256'] = a.sha(json.dumps(window['evidence'], separators=(',', ':'), ensure_ascii=False).encode())
        for route, body in [('/v1/quotes', external(at)), ('/v1/ws/quotes', envelope(at, flow=flow, **kwargs))]:
            self.s.trace_cache_poll(self.study, route, self.trace, lambda *args, body=body, **kw: Response(self.s.compact(body).encode()))

    def finish(self, at=TAIL+1):
        self.clock = at
        self.trace.seal(True, 'normal_stop', self.tail.bookmark())
        manifest = {'version': 1, 'seal': pin(self.output/'input-trace-seal.json'),
                    'trace': pin(self.output/'input-trace-v1.jsonl.gz'), 'samples': pin(self.output/'trace-samples-v1.jsonl.gz'),
                    'producer': pin(Path(self.s.__file__)), 'mapping': pin(self.mapping),
                    'sigSources': [{**pin(self.source), 'recordedPath': str(self.source),
                                    'recordedDevice': self.tail.identity[0], 'recordedInode': self.tail.identity[1]}]}
        self.original = self.root/'original-manifest.json'
        self.original.write_text(self.s.compact(manifest)+'\n', encoding='utf-8')
        result = V.verify(self.original, pin(self.original)['sha256'])
        self.proof = self.root/'replay-result.json'; self.proof.write_text(self.s.compact(result)+'\n', encoding='utf-8')
        self.protocol = self.root/'protocol.json'; self.protocol.write_bytes(P_BYTES)
        self.audit_manifest = self.root/'audit-manifest.json'
        self.spec = {'schemaVersion': 1, 'protocol': pin(self.protocol),
                     'inputReplay': {'manifest': pin(self.original), 'result': pin(self.proof)}}
        self.save()

    def save(self):
        self.audit_manifest.write_text(self.s.compact(self.spec)+'\n', encoding='utf-8')

    def audit(self):
        with patch.object(a, '_now_ms', return_value=TAIL+1):
            return a.audit(self.audit_manifest, pin(self.audit_manifest)['sha256'])

    def close(self):
        self.trace.close(); self.tail.close()


class AuditTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(); self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        disk = patch('shutil.disk_usage', return_value=type('Disk', (), {'free': 40*1024**3})())
        disk.start(); self.addCleanup(disk.stop)

    def fixture(self, **kwargs):
        f = Fixture(self.root, **kwargs); self.addCleanup(f.close); return f

    def test_stale_original_state_is_denominator_not_executable_and_no_prices_escape(self):
        f = self.fixture(); f.sig(START-10000); f.polls(START+10); f.finish()
        result = f.audit(); self.assertEqual(result['witnessedExchangeUnitDenominator'], 1)
        u = result['units'][0]
        self.assertEqual(u['ages']['sourceAgeMs'], 10010); self.assertEqual(u['sourceAgeBand'], '60000')
        self.assertFalse(u['oldThreeSecondBookEligible']); self.assertFalse(result['executionAuthority'])
        self.assertEqual(u['state']['firstAvailableMs'], START-10000)
        self.assertEqual((u['tradeSourceAgeMs'], u['tradeReceiptAgeMs'], u['tradeCollectorAvailabilityAgeMs']), (100, 104, 90))
        self.assertEqual(u['originalCacheInput']['samplerNowMs'], START+10)
        self.assertEqual(len(u['originalCacheInput']['bodySha256']), 64)
        def keys(obj):
            if isinstance(obj, dict):
                return set(obj).union(*(keys(v) for v in obj.values()))
            if isinstance(obj, list):
                return set().union(*(keys(v) for v in obj))
            return set()
        self.assertFalse({'bid', 'ask', 'price', 'midpoint', 'logitMidpointChange', 'yesOneShareTakerRoundTrip'} & keys(result))

    def test_missing_exact_exchange_witness_abstains_and_cannot_backfill_from_future(self):
        f = self.fixture(); f.polls(START+10); f.sig(START+1010); f.finish()
        result = f.audit()
        self.assertEqual(result['marketTriggerCounts']['valid_market_trigger_observations'], 1)
        self.assertEqual(result['marketTriggerCounts']['target_exchange_witness_missing'], 1)
        self.assertEqual(result['witnessedExchangeUnitDenominator'], 0)

    def test_explicit_original_mapping_allows_missing_state_denominator(self):
        f = self.fixture(mapping={'markets': [{'sigMappings': [{'sigMarketId': '1', 'sigExchangeId': '101'}]}]})
        f.polls(START+10); f.sig(START+1010); f.finish()
        u = f.audit()['units'][0]
        self.assertEqual(u['state']['state'], 'missing'); self.assertIsNone(u['ages'])
        self.assertEqual(u['availability']['1000']['status'], 'available')

    def test_gap_prior_state_counts_but_price_label_path_is_censored(self):
        f = self.fixture(); f.sig(START-1000); f.sig(START-100, gap=True)
        f.polls(START+10); f.sig(START+1010); f.finish()
        u = f.audit()['units'][0]
        self.assertEqual(u['state']['state'], 'gapped'); self.assertFalse(u['oldThreeSecondBookEligible'])
        self.assertEqual(u['availability']['1000']['reason'], 'gap_crossing')
        self.assertEqual(u['nextObservation']['status'], 'available'); self.assertTrue(u['nextObservation']['gapObserved'])

    def test_gap_after_trigger_censors_only_pending_horizons(self):
        f = self.fixture(); f.sig(START-10); f.polls(START+10)
        f.sig(START+1010); f.sig(START+1500, gap=True); f.sig(START+3010); f.finish()
        u = f.audit()['units'][0]
        self.assertEqual(u['availability']['1000']['status'], 'available')
        self.assertEqual(u['availability']['3000']['reason'], 'gap_crossing')

    def test_original_warmup_seen_identity_and_duplicate_poll_never_retrigger(self):
        f = self.fixture(); f.sig(START-900)
        flow = public_flow(START-300); f.polls(START-290, flow=flow)
        flow['asOfMs'] = START+10
        f.polls(START+20, flow=flow); f.polls(START+30, flow=flow); f.finish()
        r = f.audit()
        self.assertEqual(r['witnessedExchangeUnitDenominator'], 0)
        self.assertGreater(r['pathDiagnosticsNotDenominators']['duplicate_reobservations'], 0)

    def test_first_future_book_clocks_and_identity_preserved_and_late_is_not_backdated(self):
        f = self.fixture(); f.sig(START-10); f.polls(START+10)
        f.sig(START+1010); f.sig(START+9010); f.finish()
        u = f.audit()['units'][0]; obs = u['availability']['1000']['observation']
        self.assertEqual((obs['sourceMs'], obs['receivedMs'], obs['firstAvailableMs']), (START+1010,)*3)
        self.assertEqual(obs['journalSequence'], 2)
        self.assertEqual(u['availability']['3000']['reason'], 'no_timely_target_book')
        self.assertEqual(u['nextObservation']['delayMs'], 1000)

    def test_exact_half_open_enrollment_and_terminal_censoring(self):
        f = self.fixture(); f.sig(START-20)
        f.polls(START, identity='first'); f.polls(END, identity='end'); f.finish(at=END+1)
        r = f.audit(); self.assertEqual(r['witnessedExchangeUnitDenominator'], 1)
        self.assertEqual(r['units'][0]['decisionMs'], START)
        self.assertEqual(r['horizonAvailability']['1000'], {'no_timely_target_book': 1})

    def test_early_seal_keeps_pending_as_right_censored_not_zero(self):
        f = self.fixture(); f.sig(START-20); f.polls(START+10); f.finish(at=START+100)
        u = f.audit()['units'][0]
        self.assertTrue(all(v['status'] == 'censored' for v in u['availability'].values()))
        self.assertEqual(u['nextObservation']['reason'], 'terminal_right_censoring')

    def test_no_analysis_before_frozen_tail_finishes(self):
        f = self.fixture(); f.finish()
        with patch.object(a, '_now_ms', return_value=TAIL-1):
            with self.assertRaisesRegex(ValueError, 'PROSPECTIVE_INTERVAL_NOT_FINISHED'):
                a.audit(f.audit_manifest, pin(f.audit_manifest)['sha256'])

    def test_unchanged_old_book_version_does_not_renew_first_availability(self):
        f = self.fixture(); f.sig(START-10000)
        # Direct kernel check uses reviewed original normalizer; repeated engine
        # version keeps the original receipt even though a new callback arrived.
        S = a.study_class(f.s); s = S(P, {}, START, END)
        first = row(START-10000, 1); s.accept(first, START-10000)
        repeated = copy.deepcopy(first); repeated['event']['book']['receivedAt'] = f.s.stamp(START-1)
        s.accept(repeated, START-1)
        self.assertEqual(s.meta[('1', '101')]['firstAvailableMs'], START-10000)

    def test_bad_retained_positive_boolean_is_not_replay_proof(self):
        f = self.fixture(); f.sig(START-20); f.polls(START+10); f.finish()
        fake = {'samplerInputReplayVerified': True, 'executionAuthority': False}
        f.proof.write_text(json.dumps(fake), encoding='utf-8')
        f.spec['inputReplay']['result'] = pin(f.proof); f.save()
        with self.assertRaisesRegex(ValueError, 'RETAINED_REPLAY_RESULT_MISMATCH'):
            f.audit()

    def test_changed_original_source_and_unpinned_protocol_are_rejected(self):
        f = self.fixture(); f.sig(START-20); f.polls(START+10); f.finish()
        f.source.write_bytes(f.source.read_bytes()+b'x')
        with self.assertRaisesRegex(ValueError, 'INPUT_HASH_OR_LENGTH'):
            f.audit()

    def test_protocol_mutation_cannot_retune_the_interval(self):
        f = self.fixture(); f.finish()
        body = copy.deepcopy(P); body['enrollment']['startInclusiveUtc'] = '2026-10-16T17:54:00Z'
        f.protocol.write_text(json.dumps(body), encoding='utf-8'); f.spec['protocol'] = pin(f.protocol); f.save()
        with self.assertRaisesRegex(ValueError, 'FROZEN_PROTOCOL_BYTES'):
            f.audit()

    def test_partial_evidence_never_enters_unique_valid_denominator(self):
        f = self.fixture(); f.sig(START-20); flow = public_flow(START+10)
        for w in flow['windows'].values():
            w.update(observedTradeCount=2, evidenceComplete=False)
        f.polls(START+10, flow=flow); f.finish()
        r = f.audit(); self.assertEqual(r['witnessedExchangeUnitDenominator'], 0)
        self.assertEqual(r['pathDiagnosticsNotDenominators']['partial_evidence_windows'], 3)

    def test_invalid_future_flow_never_promotes_embedded_trade_ids(self):
        f = self.fixture(); f.sig(START-20); f.polls(START+10, flow=public_flow(START+1000)); f.finish()
        r = f.audit(); self.assertEqual(r['witnessedExchangeUnitDenominator'], 0)
        self.assertEqual(r['pathDiagnosticsNotDenominators']['poll_market_no_valid_flow'], 1)

    def test_unknown_direction_is_explicit_exclusion_not_zero_signed_flow(self):
        f = self.fixture(); f.sig(START-20); f.polls(START+10, flow=public_flow(START+10, direction=None)); f.finish()
        r = f.audit(); self.assertEqual(r['witnessedExchangeUnitDenominator'], 0)
        self.assertEqual(r['uniqueUnitExclusions']['unknown_original_direction'], 1)

    def test_spacing_excludes_labels_but_not_denominator(self):
        f = self.fixture(); f.sig(START-20)
        f.polls(START+10, identity='a'); f.polls(START+110, identity='b', sequence=2, sha='d'*64); f.finish()
        r = f.audit(); self.assertEqual(r['witnessedExchangeUnitDenominator'], 2)
        self.assertEqual(r['units'][1]['labelNomination'], 'spacing_exclusion')

    def test_future_sig_invalidates_numeric_state_without_deleting_prior_scope_binding(self):
        f = self.fixture(); f.sig(START-20)
        f.sig(START-10, source=START+1000); f.polls(START+10); f.finish()
        u = f.audit()['units'][0]
        self.assertEqual(u['state']['state'], 'invalid'); self.assertIsNone(u['ages'])
        self.assertFalse(u['oldThreeSecondBookEligible'])

    def test_delayed_observer_availability_cannot_supply_nominally_timely_book(self):
        f = self.fixture(); f.sig(START-20); f.polls(START+10)
        f.sig(START+5010, source=START+1010); f.finish()
        u = f.audit()['units'][0]
        self.assertEqual(u['availability']['1000']['reason'], 'no_timely_target_book')
        self.assertEqual(u['nextObservation']['status'], 'missing')

    def test_identity_bound_never_evicts_old_trade_or_retries_it(self):
        f = self.fixture(); s = a.study_class(f.s)(P, {}, START, END)
        s.seen = {('1', 'RACE-26-D', str(i)): 'a'*64 for i in range(32768)}
        with self.assertRaisesRegex(ValueError, 'IDENTITY_BOUND'):
            s.external_update(envelope(START+10), START+10)
        self.assertEqual(len(s.seen), 32768)

    def test_scope_bound_is_exact_even_when_other_books_are_historical(self):
        f = self.fixture(); s = a.study_class(f.s)(P, {}, START, END)
        for i in range(237):
            book = row(START-10000, i+1, market=str(i+1))
            s.accept(book, START-10000)
        with self.assertRaisesRegex(ValueError, 'SCOPE_BOUND'):
            s.accept(row(START-10000, 238, market='238'), START-10000)

    def test_hard_five_thousand_unit_limit_stops_enrollment_and_bounds_tail(self):
        f = self.fixture(); s = a.study_class(f.s)(P, {}, START, END)
        s.accept(row(START-20), START-20)
        for i in range(5001):
            now = START+10+i*10
            body = envelope(now, sequence=i+1, sha=f'{i+1:064x}')
            flow = body['markets'][0]['tradeFlow']
            for window in flow['windows'].values():
                window['evidence'][0]['tradeId'] = str(i)
                window['evidenceSha256'] = a.sha(json.dumps(window['evidence'], separators=(',', ':'), ensure_ascii=False).encode())
            s.external_update(body, now)
        self.assertEqual(len(s.unit_rows), 5000)
        self.assertEqual(s.first_cap_at, START+10+4999*10)
        self.assertEqual(s.cutoff, s.first_cap_at+123000)

    def test_optional_original_request_preserved_without_claiming_engine_lower_bound(self):
        f = self.fixture(); s = a.study_class(f.s)(P, {}, START, END)
        book = row(START-10, source=START-1000)
        book['event']['book']['requestStartedAt'] = f.s.stamp(START-100)
        s.accept(book, START-10)
        self.assertEqual(s.meta[('1', '101')]['sourceMs'], START-1000)
        self.assertEqual(s.meta[('1', '101')]['requestStartedAt'], f.s.stamp(START-100))
        s.accept(row(START, 2), START)
        self.assertNotIn('requestStartedAt', s.meta[('1', '101')])

    def test_horizon_exact_inclusive_deadline_is_missing_not_premature_right_censor(self):
        f = self.fixture(); f.sig(START-20); f.polls(START+10); f.finish(at=START+4010)
        u = f.audit()['units'][0]
        self.assertEqual(u['availability']['1000']['reason'], 'no_timely_target_book')
        self.assertEqual(u['availability']['3000']['reason'], 'terminal_right_censoring')

    def test_gap_resync_same_original_read_does_not_renew_receipt_or_availability(self):
        f = self.fixture(); s = a.study_class(f.s)(P, {}, START, END)
        first = row(START-10000, 7); s.accept(first, START-10000)
        s.accept({'event': {'type': 'gap', 'marketId': '1'}}, START-1)
        repeated = copy.deepcopy(first)
        repeated['event']['book']['receivedAt'] = f.s.stamp(START)
        s.accept(repeated, START)
        s.external_update(envelope(START+10), START+10)
        u = s.unit_rows[0]
        self.assertEqual(u['state']['state'], 'gapped')
        self.assertEqual(u['state']['firstAvailableMs'], START-10000)
        self.assertEqual(u['state']['receivedMs'], START-10000)
        self.assertEqual(u['ages']['receiptAgeMs'], 10010)

    def test_gap_does_not_hide_conflicting_identical_engine_version(self):
        f = self.fixture(); s = a.study_class(f.s)(P, {}, START, END)
        first = row(START-1000, 7); s.accept(first, START-1000)
        s.accept({'event': {'type': 'gap', 'marketId': '1'}}, START-1)
        conflict = copy.deepcopy(first)
        conflict['event']['book']['bids'][0]['quantity'] = 11
        s.accept(conflict, START)
        s.external_update(envelope(START+10), START+10)
        u = s.unit_rows[0]
        self.assertEqual(u['state']['state'], 'conflicted'); self.assertIsNone(u['ages'])
        self.assertFalse(u['oldThreeSecondBookEligible'])

    def test_cross_gap_rollback_abstains_without_inventing_a_new_epoch(self):
        f = self.fixture(); s = a.study_class(f.s)(P, {}, START, END)
        s.accept(row(START-1000, 9), START-1000)
        s.accept({'event': {'type': 'gap', 'marketId': '1'}}, START-1)
        s.accept(row(START, 2), START)
        s.external_update(envelope(START+10), START+10)
        s.accept(row(START+1010, 10), START+1010)
        u = s.unit_rows[0]
        self.assertEqual(u['state']['state'], 'ordering_unknown'); self.assertIsNone(u['ages'])
        self.assertEqual(u['state']['originalVersion']['sequence'], '9')
        self.assertTrue(all(x['reason'] == 'unprovable_book_ordering' for x in u['availability'].values()))
        self.assertEqual(s.meta[('1', '101')]['state'], 'ordering_unknown')

    def test_first_book_at_exact_slack_deadline_is_available(self):
        f = self.fixture(); f.sig(START-20); f.polls(START+10)
        f.sig(START+4010); f.finish()
        outcome = f.audit()['units'][0]['availability']['1000']
        self.assertEqual(outcome['status'], 'available')
        self.assertEqual(outcome['observation']['firstAvailableMs'], START+4010)


if __name__ == '__main__':
    unittest.main()
