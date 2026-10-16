"""Deterministic synthetic original-shaped lifecycle tests; no actual cohort IO."""
import copy
from datetime import datetime, timezone
from decimal import Decimal
from fractions import Fraction
import gzip
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('lifecycle_estimator', HERE/'estimator.py')
m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m)
BASE = 1792022400000
POLICY = {'inventoryAgeClock': 'execution', 'maximumInventoryAgeMs': 60000,
          'bookAgeMs': 10000, 'inventoryExitDepthBps': 5000}


def stamp(ms): return datetime.fromtimestamp((BASE+ms)/1000, timezone.utc).isoformat(timespec='milliseconds')


def quote(qid=1, side='YES', action='buy', quantity=2, price='0.400000'):
    return {'id': qid, 'marketId': '201', 'exchangeId': '101', 'side': side, 'action': action,
            'quantity': quantity, 'price': price, 'createdAt': BASE-10000, 'activationAt': BASE-7000,
            'expiresAt': BASE+600000, 'filled': 0, 'retired': False}


def log(q, closed=False, attempt=True):
    out = [{'kind': 'intent', 'qid': q['id'], 'quote': q, 'fundamental': None}]
    if attempt:
        out.extend([{'kind': 'attempt', 'qid': q['id'], 'at': stamp(-2000)},
                    {'kind': 'ack', 'qid': q['id'], 'receipt': {'endpoint': '/orders', 'status': 200,
                       'requestStartedAt': stamp(-1900), 'receivedAt': stamp(-1000), 'data': {'orderId': str(900+q['id'])}}}])
        if closed: out.append({'kind': 'closed', 'qid': q['id'], 'orderReceiptHash': 'd'*64, 'fillReceiptHash': 'c'*64})
    return out


def event(kind, at=0, **fields):
    return {'type': kind, 'receivedAt': stamp(at), 'processedAt': stamp(at), **fields}


def actual(q, kind, at=0, n=None, fid='1001', executed=None, fee='0.000000', price=None, status='filled'):
    return event(kind, at, source='official_sig_api_v1', receiptHash='c'*64, orderReceiptHash='d'*64,
                 orderId=q['id'], actualOrderId=str(900+q['id']), marketId=q['marketId'], exchangeId=q['exchangeId'],
                 side=q['side'], action=q['action'], price=price or q['price'], quantity=n if n is not None else q['quantity'],
                 fillId=fid, executedAt=stamp(at if executed is None else executed), fee=fee,
                 status=status, filledQuantity=n if n is not None else q['quantity'], fillsComplete=True)


def book(at=60000, bid='0.400000', ask='0.450000', qty=10, received=None, invalid=False):
    received = at if received is None else received
    return event('book', received, sourceRef={'id': 'original-source', 'sequence': 1, 'hash': 'a'*64}, book={
        'marketId': '201', 'exchangeId': '101', 'sourceAt': stamp(at), 'requestStartedAt': stamp(at),
        'receivedAt': stamp(received), 'bids': [{'price': bid, 'quantity': qty}],
        'asks': [{'price': ask, 'quantity': qty}], 'invalid': invalid})


def coverage(at=60000):
    return event('coverage', at, exchangeId='101', marketId='201', complete=True, **{'from': stamp(-3000), 'to': stamp(at)})


class LifecycleTests(unittest.TestCase):
    def observer(self, *quotes, closed=False):
        quotes = quotes or (quote(),)
        original = [e for q in quotes for e in log(q, closed)]
        o = m.Observer(POLICY, m.dispatch_metadata(original))
        return o

    def start(self, q=None, closed=False):
        q = q or quote(); o = self.observer(q, closed=closed)
        o.accept(actual(q, 'actual_order_ack', -500)); return o, q

    def test_partial_at_zero_full_at59_marks_at_oldest_lot60_not119(self):
        o, q = self.start()
        o.accept(actual(q, 'actual_fill', 0, n=1))
        o.accept(actual(q, 'actual_fill', 59000, n=1, fid='1002'))
        o.accept(coverage()); o.accept(book())
        p = o.summary()['episodes'][0]; mark = p['mark']
        self.assertEqual(mark['deadlineNs'], (BASE+60000)*m.NS)
        self.assertEqual(mark['quantity'], 2); self.assertEqual(mark['oldestLotExecutedNs'], BASE*m.NS)
        self.assertEqual(p['markState'], 'full_depth_gross_mark')
        self.assertNotEqual(mark['deadlineNs'], (BASE+119000)*m.NS)
        self.assertIsNone(mark['netHypotheticalProfit'])

    def test_never_full_partial_order_still_has_its_actual_exposure_mark(self):
        o, q = self.start(); o.accept(actual(q, 'actual_fill', 0, n=1))
        o.accept(coverage()); o.accept(book(qty=10))
        s = o.summary(); self.assertEqual(s['episodes'][0]['mark']['quantity'], 1)
        self.assertEqual(s['attemptOutcomes'], {'filled_with_terminal_evidence_pending': 1})

    def test_fifo_oldest_consumption_retargets_next_actual_lot_without_reset(self):
        buy = quote(); sell = quote(2, action='sell', quantity=1, price='0.400000')
        o = self.observer(buy, sell)
        o.accept(actual(buy, 'actual_order_ack', -500)); o.accept(actual(buy, 'actual_fill', 0, n=1))
        o.accept(actual(buy, 'actual_fill', 30000, n=1, fid='1002'))
        o.accept(actual(sell, 'actual_order_ack', 31000)); o.accept(actual(sell, 'actual_fill', 32000, n=1, fid='1003', price='0.450000'))
        o.accept(coverage()); o.accept(book())
        p = o.summary()['episodes'][0]
        self.assertIsNone(p['mark']); self.assertEqual(p['deadlineNs'], (BASE+90000)*m.NS)
        self.assertEqual(p['actualRealizedGrossPnl'], m.exact(50000))
        o.accept(coverage(90000)); o.accept(book(90000))
        self.assertEqual(o.summary()['episodes'][0]['mark']['quantity'], 1)

    def test_actual_close_before_age_is_realized_only_not_fictitious_mark(self):
        buy, sell = quote(quantity=1), quote(2, action='sell', quantity=1)
        o = self.observer(buy, sell)
        for e in (actual(buy, 'actual_order_ack', -500), actual(buy, 'actual_fill', 0, n=1),
                  actual(sell, 'actual_order_ack', 1000), actual(sell, 'actual_fill', 2000, n=1, fid='1002', price='0.450000')): o.accept(e)
        p = o.summary()['episodes'][0]
        self.assertEqual(p['markState'], 'closed_before_age_mark'); self.assertIsNone(p['mark'])
        self.assertEqual(p['actualRealizedNetPnl'], m.exact(50000))

    def test_opposite_buy_nets_fifo_with_actual_fill_not_ack_or_new_fake_fill(self):
        yes, no = quote(quantity=1), quote(2, side='NO', quantity=1, price='0.550000')
        o = self.observer(yes, no)
        o.accept(actual(yes, 'actual_order_ack', -500)); o.accept(actual(yes, 'actual_fill', 0, n=1))
        o.accept(actual(no, 'actual_order_ack', 1000)); self.assertTrue(o.summary()['remainingPositions'])
        o.accept(actual(no, 'actual_fill', 2000, n=1, fid='1002'))
        p = o.summary()['episodes'][0]
        self.assertEqual(p['actualRealizedGrossPnl'], m.exact(50000)); self.assertFalse(o.summary()['remainingPositions'])
        self.assertEqual(p['actualCloses'][0]['kind'], 'opposite_buy_netting'); self.assertEqual(len(o.fills), 2)

    def test_unknown_entry_or_exit_fee_never_becomes_zero_net_profit(self):
        buy, sell = quote(quantity=1), quote(2, action='sell', quantity=1)
        o = self.observer(buy, sell)
        for e in (actual(buy, 'actual_order_ack', -500), actual(buy, 'actual_fill', 0, n=1, fee=None),
                  actual(sell, 'actual_order_ack', 1000), actual(sell, 'actual_fill', 2000, n=1, fid='1002', price='0.450000')): o.accept(e)
        p = o.summary()['episodes'][0]
        self.assertEqual(p['actualRealizedGrossPnl'], m.exact(50000)); self.assertIsNone(p['actualRealizedNetPnl'])
        self.assertFalse(p['realizedFeesKnown'])

    def test_fee_allocation_is_exact_rational_no_rounding_or_double_charge(self):
        buy, sell = quote(quantity=3), quote(2, action='sell', quantity=1)
        o = self.observer(buy, sell)
        for e in (actual(buy, 'actual_order_ack', -500), actual(buy, 'actual_fill', 0, n=3, fee='0.000001'),
                  actual(sell, 'actual_order_ack', 1000), actual(sell, 'actual_fill', 2000, n=1, fid='1002', fee='0.000001', price='0.450000')): o.accept(e)
        self.assertEqual(o.summary()['episodes'][0]['actualRealizedNetPnl'], m.exact(Fraction(50000)-Fraction(1,3)-1))

    def test_duplicate_fill_new_receipt_clock_is_idempotent_and_conflict_poisoned(self):
        o, q = self.start(); first = actual(q, 'actual_fill', 0, n=1)
        o.accept(first); duplicate = copy.deepcopy(first); duplicate.update(receivedAt=stamp(1000), processedAt=stamp(1000), receiptHash='e'*64)
        o.accept(duplicate)
        self.assertEqual(sum(l['remaining'] for l in o.positions[('101','YES')]), 1)
        self.assertEqual(o.fills['1001']['firstObservedNs'], BASE*m.NS)
        duplicate['quantity'] = 2
        with self.assertRaisesRegex(ValueError, 'FILL_ID_CONFLICT'): o.accept(duplicate)
        with self.assertRaisesRegex(ValueError, 'POISONED'): o.summary()

    def test_gap_irreversibly_censors_existing_episode_even_with_recovery_book(self):
        o, q = self.start(); o.accept(actual(q, 'actual_fill', 0, n=1))
        o.accept(event('gap', 1000, exchangeId='101')); o.accept(coverage()); o.accept(book())
        self.assertEqual(o.summary()['episodes'][0]['markState'], 'gap_during_exposure')
        self.assertIsNone(o.summary()['episodes'][0]['mark'])

    def test_absent_complete_tape_coverage_is_unavailable_not_zero_return(self):
        o, q = self.start(); o.accept(actual(q, 'actual_fill', 0, n=1)); o.accept(book())
        self.assertEqual(o.summary()['episodes'][0]['markState'], 'coverage_unavailable')
        o.accept(coverage(61000)); o.accept(book(61000, bid='0.500000', ask='0.550000'))
        self.assertEqual(o.summary()['episodes'][0]['markState'], 'coverage_unavailable')

    def test_own_pending_buy_depth_removed_first_shallow_book_never_rescued(self):
        o, q = self.start(); o.accept(actual(q, 'actual_fill', 0, n=1))
        o.accept(coverage()); o.accept(book(qty=1))
        p = o.summary()['episodes'][0]; self.assertEqual(p['markState'], 'insufficient_full_external_depth')
        self.assertEqual(p['mark']['coveredQuantity'], 0)
        o.accept(book(61000, qty=100)); self.assertEqual(o.summary()['episodes'][0]['mark']['coveredQuantity'], 0)

    def test_native_retired_or_cancel_not_terminal_cannot_remove_own_shadow(self):
        q = quote(); q['retired'] = True
        o, q = self.start(q); o.accept(actual(q, 'actual_fill', 0, n=1))
        o.accept(coverage()); o.accept(book(qty=1))
        self.assertEqual(o.summary()['episodes'][0]['markState'], 'insufficient_full_external_depth')

    def test_no_liquidation_walks_complement_asks_and_exposes_native_half_depth(self):
        q = quote(side='NO', quantity=2, price='0.550000'); o, q = self.start(q)
        o.accept(actual(q, 'actual_fill', 0, n=2)); o.accept(coverage())
        o.accept(book(ask='0.450000', qty=10))
        p = o.summary()['episodes'][0]['mark']
        self.assertEqual(p['fullDisplayedProceedsMicros'], 1100000); self.assertEqual(p['nativeExitSliceCapacityShares'], 5)

    def test_late_fill_observation_does_not_retroactively_use_prior_book(self):
        o, q = self.start(); o.accept(coverage()); o.accept(book())
        o.accept(actual(q, 'actual_fill', 61000, executed=0, n=1))
        self.assertEqual(o.summary()['episodes'][0]['markState'], 'fill_observed_after_age_deadline')

    def test_late_economic_fill_invalidates_already_frozen_exposure_not_rewrites(self):
        o, q = self.start(); o.accept(actual(q, 'actual_fill', 0, n=1)); o.accept(coverage()); o.accept(book())
        old = copy.deepcopy(o.summary()['episodes'][0]['mark'])
        o.accept(actual(q, 'actual_fill', 62000, executed=59000, n=1, fid='1002'))
        p = o.summary()['episodes'][0]
        self.assertEqual(p['markState'], 'late_fill_after_frozen_label')
        self.assertEqual(p['mark']['quantity'], old['quantity']); self.assertTrue(p['mark']['invalidated'])

    def test_zero_fill_requires_actual_complete_terminal_and_original_matching_closed_receipts(self):
        o, q = self.start(closed=True)
        self.assertNotIn('verified_terminal_zero_fill', o.summary()['attemptOutcomes'])
        o.accept(actual(q, 'actual_order_terminal', 1000, n=0, status='canceled'))
        self.assertEqual(o.summary()['attemptOutcomes'], {'verified_terminal_zero_fill': 1})
        other, q = self.start()
        other.accept(actual(q, 'actual_order_terminal', 1000, n=0, status='expired'))
        self.assertEqual(other.summary()['attemptOutcomes'], {'acknowledged_open_or_terminal_unproved': 1})

    def test_terminal_quantity_or_receipt_contradiction_is_fatal(self):
        o, q = self.start(closed=True); terminal = actual(q, 'actual_order_terminal', 1000, n=0, status='canceled')
        terminal['receiptHash'] = 'f'*64
        with self.assertRaisesRegex(ValueError, 'TERMINAL_RECEIPT_MISMATCH'): o.accept(terminal)
        o, q = self.start(); terminal = actual(q, 'actual_order_terminal', 1000, n=1, status='canceled')
        with self.assertRaisesRegex(ValueError, 'INCOMPLETE_TERMINAL'): o.accept(terminal)

    def test_replay_restart_matches_all_outputs_and_preserves_unknown_attempts(self):
        q, unknown = quote(), quote(2, quantity=1)
        metadata = m.dispatch_metadata(log(q)+log(unknown)[:2])
        events = [actual(q, 'actual_order_ack', -500), actual(q, 'actual_fill', 0, n=1), coverage(), book()]
        first, restarted = m.Observer(POLICY, metadata), m.Observer(POLICY, metadata)
        for e in events: first.accept(e)
        for e in events: restarted.accept(copy.deepcopy(e))
        self.assertEqual(first.summary(), restarted.summary())
        self.assertEqual(first.summary()['attemptOutcomes']['unknown_no_native_ack'], 1)

    def test_inherited_only_has_no_new_age_clock_and_mixed_never_qualifies(self):
        o, q = self.start()
        adoption = event('actual_position_adoption', -250, source='official_sig_api_v1', receiptHash='a'*64, complete=True,
            positions=[{'exchangeId':'101','marketId':'201','quantity':1,'lots':[{'lotId':'71','side':'YES','quantity':1,'entryPrice':'0.300000'}]}])
        o.accept(adoption); o.accept(coverage()); o.accept(book())
        self.assertEqual(o.summary()['episodes'], [])
        o.accept(actual(q, 'actual_fill', 61000, n=1)); o.accept(coverage(121000)); o.accept(book(121000))
        self.assertEqual(o.summary()['episodes'][0]['markState'], 'mixed_nonordinary_inventory')

    def test_source_clock_and_foreign_scope_failure_never_grants_mark(self):
        o, q = self.start(); o.accept(actual(q, 'actual_fill', 0, n=1)); o.accept(coverage())
        bad = book(); bad['book']['marketId'] = '999'
        with self.assertRaisesRegex(ValueError, 'BOOK_MARKET_SCOPE'): o.accept(bad)
        o, q = self.start(); o.accept(actual(q, 'actual_fill', 0, n=1)); o.accept(coverage())
        stale = book(); stale['processedAt'] = stamp(71000); o.accept(stale)
        self.assertEqual(o.summary()['episodes'][0]['markState'], 'stale_or_future_horizon_book')

    def test_unsupported_settlement_never_uses_wallet_absence_as_profit(self):
        o, q = self.start()
        with self.assertRaisesRegex(ValueError, 'UNSUPPORTED_INVENTORY'): o.accept(event('actual_collateral_settlement'))

    def test_protocol_is_frozen_and_oldest_lot_policy_not_changed_by_caller(self):
        self.assertEqual(m.h((HERE/'protocol.json').read_bytes().replace(b'\r\n', b'\n')), m.PROTOCOL_SHA)
        p = dict(POLICY); p['maximumInventoryAgeMs'] = 120000
        with self.assertRaisesRegex(ValueError, 'POLICY_MISMATCH'): m.Observer(p, {})

    def test_sealed_original_shaped_gzip_prefix_end_to_end_and_tamper_rejection(self):
        def canonical(v): return json.dumps(v, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode()
        def seal(v): return canonical({**v, 'hash': m.h(canonical(v))})+b'\n'
        def chain(events, initial_previous, native=False):
            previous = initial_previous; out = []
            for seq, e in enumerate(events, 1):
                if native:
                    # Session::accept hashes the original event, adds sourceRef,
                    # then seals the native Journal row; no source body rewriting.
                    e = {k:v for k,v in e.items() if k != 'sourceRef'}
                    e['sourceRef'] = {'id':'fixture','sequence':seq,'hash':m.h(canonical(e))}
                raw = seal({'event':e,'previous':previous,'sequence':seq}); previous = json.loads(raw)['hash']; out.append(raw)
            return b''.join(out)
        q = quote(); events = [actual(q, 'actual_order_ack', -500), actual(q, 'actual_fill', 0, n=1), coverage(), book()]
        with tempfile.TemporaryDirectory() as d:
            p = Path(d); files = {'bankManifest': ('bank.json', seal({'schemaVersion':1,'mode':'rust_live_session','seed':{'kind':'actual_account_allocation'},'policy':POLICY})),
                                  'dispatch': ('dispatch.gz', gzip.compress(chain(log(q), '0'*64), mtime=0)),
                                  'events': ('events.gz', gzip.compress(chain(events, '', native=True), mtime=0))}
            manifest = {'schemaVersion':1,'sealed':True,'protocolSha256':m.PROTOCOL_SHA,'asOfUtc':stamp(61000)}
            for key, (name, raw) in files.items():
                (p/name).write_bytes(raw); manifest[key] = {'path':name,'bytes':len(raw),'sha256':m.h(raw)}
            source = p/'manifest.json'; source.write_bytes(canonical(manifest)); digest = m.h(source.read_bytes())
            result = m.analyze(source, digest)
            self.assertEqual(result['descriptiveTotals']['markedShares'], 1)
            last_native = json.loads(gzip.decompress(files['events'][1]).splitlines()[-1])
            self.assertEqual(result['episodes'][0]['mark']['readerVerifiedNativeJournalRowSha256'], last_native['hash'])
            self.assertFalse(result['sourceRefContentIndependentlyVerified'])
            self.assertFalse(result['nativeFinancialReplayIndependentlyPerformed'])
            self.assertFalse(result['executionAuthority'])
            native_first = json.loads(gzip.decompress(files['events'][1]).splitlines()[0])
            self.assertEqual(native_first['previous'], '')
            ref = native_first['event']['sourceRef']
            self.assertEqual(ref['hash'], m.h(canonical({k:v for k,v in native_first['event'].items() if k != 'sourceRef'})))
            dispatch_first = json.loads(gzip.decompress(files['dispatch'][1]).splitlines()[0])
            self.assertEqual(dispatch_first['previous'], '0'*64)
            broken = bytearray((p/'events.gz').read_bytes()); broken[20] ^= 1; (p/'events.gz').write_bytes(broken)
            with self.assertRaisesRegex(ValueError, 'INPUT_PIN'): m.analyze(source, digest)

    def test_native_and_dispatch_genesis_cannot_be_cross_seeded(self):
        def canonical(x): return json.dumps(x,sort_keys=True,separators=(',',':')).encode()
        with tempfile.TemporaryDirectory() as d:
            for previous, correct, wrong in (('', '', '0'*64), ('0'*64, '0'*64, '')):
                body = {'event':event('tick'), 'previous':previous, 'sequence':1}
                raw = canonical({**body,'hash':m.h(canonical(body))})+b'\n'
                p = Path(d)/('native' if previous == '' else 'dispatch'); p.write_bytes(raw)
                sig = (p.stat().st_dev,p.stat().st_ino,p.stat().st_size,p.stat().st_mtime_ns)
                self.assertEqual(len(list(m.journal(p,sig,correct))),1)
                with self.assertRaisesRegex(ValueError,'ORIGINAL_CHAIN'): list(m.journal(p,sig,wrong))

    def test_same_price_depth_multiple_levels_and_unknown_attempt_shadow(self):
        q = quote(quantity=4); unknown = quote(2, quantity=2)
        o = m.Observer(POLICY, m.dispatch_metadata(log(q)+log(unknown)[:2]))
        o.accept(actual(q, 'actual_order_ack', -500)); o.accept(actual(q, 'actual_fill', 0, n=4)); o.accept(coverage())
        b = book(qty=3); b['book']['bids'].append({'price':'0.395000','quantity':3}); o.accept(b)
        mark = o.summary()['episodes'][0]['mark']
        # Unknown2 shares removed from .400 level:1*.400 +3*.395.
        self.assertEqual(mark['fullDisplayedProceedsMicros'], 1585000)
        self.assertEqual(mark['grossMarkLessPrincipalMicros'], -15000)

    def test_future_receipt_and_malformed_terminal_do_not_publish_partial_analysis(self):
        o, q = self.start(); e = actual(q, 'actual_fill', 0, n=1); e['receivedAt'] = stamp(1)
        with self.assertRaisesRegex(ValueError, 'SOURCE_RECEIPT_AFTER_PROCESSING'): o.accept(e)
        with self.assertRaisesRegex(ValueError, 'POISONED'): o.summary()

    def test_old_book_cannot_count_own_depth_after_later_known_terminal(self):
        q, other = quote(quantity=1), quote(2, quantity=1)
        o = self.observer(q, other, closed=True)
        o.accept(actual(q, 'actual_order_ack', -500)); o.accept(actual(other, 'actual_order_ack', -500))
        o.accept(actual(q, 'actual_fill', 0, n=1)); o.accept(coverage())
        o.accept(actual(other, 'actual_order_terminal', 60500, n=0, status='canceled'))
        o.accept(book(60000, received=61000, qty=1))
        self.assertEqual(o.summary()['episodes'][0]['markState'], 'insufficient_full_external_depth')

    def test_horizon_snapshot_before_new_known_execution_cannot_value_that_inventory(self):
        o, q = self.start(); o.accept(actual(q, 'actual_fill', 0, n=1)); o.accept(coverage(60500))
        o.accept(actual(q, 'actual_fill', 60600, executed=60200, n=1, fid='1002'))
        o.accept(book(60000, received=61000))
        self.assertEqual(o.summary()['episodes'][0]['markState'], 'book_predates_known_exposure_change')

    def test_foreign_market_coverage_cannot_certify_same_exchange_book(self):
        o, q = self.start(); o.accept(actual(q, 'actual_fill', 0, n=1))
        foreign = coverage(); foreign['marketId'] = '999'
        with self.assertRaisesRegex(ValueError, 'COVERAGE_MARKET_SCOPE'): o.accept(foreign)
        with self.assertRaisesRegex(ValueError, 'POISONED'): o.summary()

    def test_coverage_tournament_must_match_original_bound_context(self):
        o, q = self.start(); o.tournament = 'original-cup'
        foreign = coverage(); foreign['tournamentId'] = 'another-cup'
        with self.assertRaisesRegex(ValueError, 'COVERAGE_TOURNAMENT_SCOPE'): o.accept(foreign)

    def test_future_dispatch_attempt_and_original_post_ack_fail_cutoff(self):
        original = log(quote()); original[1]['at'] = stamp(120000)
        with self.assertRaisesRegex(ValueError, 'DISPATCH_AFTER_CUTOFF'):
            m.dispatch_metadata(original, cutoff=m.clock(stamp(61000)))
        original = log(quote()); original[2]['receipt'].update(requestStartedAt=stamp(60999), receivedAt=stamp(61001))
        with self.assertRaisesRegex(ValueError, 'POST_ACK_AFTER_CUTOFF'):
            m.dispatch_metadata(original, cutoff=m.clock(stamp(61000)))
        original[2]['receipt']['receivedAt'] = stamp(61000)
        self.assertTrue(m.dispatch_metadata(original, cutoff=m.clock(stamp(61000))))

    def test_recorded_source_reference_is_not_claimed_as_independent_body_proof(self):
        o, q = self.start(); o.accept(actual(q, 'actual_fill', 0, n=1)); o.accept(coverage())
        b = book(); b['sourceRef']['hash'] = 'f'*64
        o.accept(b)
        mark = o.summary()['episodes'][0]['mark']
        self.assertEqual(mark['recordedSourceRefSha256'], 'f'*64)
        self.assertIsNone(mark['readerVerifiedNativeJournalRowSha256'])
        self.assertEqual(mark['sourceRefVerification'], 'syntax_only_native_accepted_chain_reference')
        self.assertFalse(mark['underlyingBookBodyIndependentlyVerified'])
        self.assertNotIn('bookEventSha256', mark)


if __name__ == '__main__': unittest.main()
