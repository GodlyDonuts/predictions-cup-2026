import test from 'node:test';
import assert from 'node:assert/strict';
import {RealtimeBooks} from './realtime-books.mjs';
import {scope as originalScope, start, at, book, batch, snapshot} from '../feed/test/helpers.mjs';

const marketId = '277', exchangeId = '701', connectionId = 'connection-1';
function fixture(options = {}) {
  let now = start;
  const bridge = new RealtimeBooks({tournamentId: originalScope.tournamentId, markets: [{marketId, exchangeId}, {marketId: '278', exchangeId: '702'}]}, {now: () => now, ...options});
  const send = (payload, extra = {}) => bridge.ingest({marketId, connectionId, event: 'market_batch', payload, receivedAt: at(now - start), ...extra});
  const subscribe = (extra = {}) => bridge.subscribed({marketId, connectionId, receivedAt: at(now - start), ...extra});
  const response = (ticket, books = [book(701)], extra = {}) => ({endpoint: ticket.endpoint, requestStartedAt: at(ticket.startedMs - start), receivedAt: at(now - start), data: snapshot(books), ...extra});
  const recover = (books, extra) => { const ticket = bridge.beginSnapshot({marketId, connectionId}); return bridge.completeSnapshot(ticket, response(ticket, books, extra)); };
  const ready = () => { subscribe(); assert.equal(recover(), true); };
  return {bridge, send, subscribe, response, recover, ready, advance: ms => { now = start + ms; }, view: () => bridge.view(marketId)};
}

test('market-only bridge requires actual subscription before its exact full-depth REST recovery', () => {
  const f = fixture();
  assert.throws(() => f.bridge.beginSnapshot({marketId, connectionId}), /STALE_CONNECTION/);
  assert.equal(f.send(batch(1, 0, [book(701)])), false);
  f.subscribe(); assert.equal(f.send(batch(1, 0, [book(701)])), true);
  assert.equal(f.view().ready, false); assert.deepEqual(f.view().books, []);
  const ticket = f.bridge.beginSnapshot({marketId, connectionId});
  assert.equal(ticket.endpoint, '/markets/277/orderbook?tournamentId=' + originalScope.tournamentId + '&depth=200');
  assert.equal(f.bridge.completeSnapshot(ticket, f.response(ticket)), true);
  assert.equal(f.view().executionAuthorized, false); assert.equal(f.view().books[0].exchangeId, exchangeId);
  assert.equal(f.bridge.view('278').ready, false);
});

test('topic revisions deduplicate arrays but newer books in duplicate revisions still apply', () => {
  const f = fixture(); f.ready();
  f.send(batch(10, 0, [book(701, 110)]));
  f.advance(1); f.send(batch(10, 0, [book(701, 111, 1)]));
  assert.equal(f.view().revision, 10); assert.equal(f.view().books[0].asOf.sequence, 111);
  assert.equal(f.view().metrics.duplicateBatches, 1);
  f.send(batch(9, 8, [book(701, 109)]));
  assert.equal(f.view().books[0].asOf.sequence, 111);
  assert.equal(f.view().metrics.outOfOrderBatches, 1);
});

test('merged revisions and engine jumps remain valid; actual topic gaps hide books until new REST', () => {
  const f = fixture(); f.ready();
  f.send(batch(10, 0, [book(701, 200)])); f.send(batch(30, 10, [book(701, 300)]));
  assert.equal(f.view().ready, true); assert.equal(f.view().metrics.gaps, 0);
  f.send(batch(40, 35, [book(701, 400)]));
  assert.equal(f.view().ready, false); assert.deepEqual(f.view().books, []); assert.ok(f.view().reasons.includes('GAP'));
  assert.equal(f.recover([book(701, 400)]), true);
});

test('compact resync and null-sequence trades invalidate even when retaining newer books', () => {
  for (const mode of ['compact', 'unsequenced']) {
    const f = fixture(); f.ready();
    const payload = mode === 'compact' ? {delivery: batch().delivery, resyncRequired: true} : batch(1, 0, [book(701)], {trades: [{id: null, sequence: null, exchangeId: 701, marketId: 277, tournamentId: originalScope.tournamentId, price: .5, quantity: 1, executedAt: at(0)}]});
    assert.equal(f.send(payload), true); assert.equal(f.view().ready, false); assert.deepEqual(f.view().books, []);
  }
});

test('connection epoch, refresh, old callbacks and in-flight REST cannot bypass mandatory resync', () => {
  const f = fixture(); f.ready();
  const old = f.bridge.beginSnapshot({marketId, connectionId});
  f.bridge.disconnected({marketId, connectionId, reason: 'REFRESH'});
  assert.equal(f.send(batch(1, 0, [book(701, 200)])), false);
  f.subscribe({connectionId: 'connection-2', reason: 'REFRESH'});
  assert.equal(f.bridge.completeSnapshot(old, f.response(old)), false);
  assert.equal(f.send(batch(1, 0, [book(701, 200)])), false);
  assert.equal(f.view().ready, false);
  const ticket = f.bridge.beginSnapshot({marketId, connectionId: 'connection-2'});
  assert.equal(f.bridge.completeSnapshot(ticket, f.response(ticket)), true);
  f.bridge.disconnected({marketId, connectionId: 'connection-2'});
  assert.throws(() => f.subscribe({connectionId, reason: 'RECONNECT'}), /REUSED_CONNECTION_ID/);
});

test('a recovery invalidated by a new gap is discarded; a genuine newer concurrent push is retained', () => {
  const f = fixture(); f.ready(); f.send(batch(1, 0, [book(701)]));
  const invalidated = f.bridge.beginSnapshot({marketId, connectionId});
  f.send(batch(3, 2, [book(701, 103)]));
  assert.equal(f.bridge.completeSnapshot(invalidated, f.response(invalidated, [book(701, 103)])), false);
  const ticket = f.bridge.beginSnapshot({marketId, connectionId});
  f.advance(1); f.send(batch(4, 3, [book(701, 110, 1)]));
  assert.equal(f.bridge.completeSnapshot(ticket, f.response(ticket, [book(701, 105)])), true);
  assert.equal(f.view().books[0].asOf.sequence, 110);
});

test('authoritative recovery cannot regress a pre-recovery version or accept null versions', () => {
  for (const books of [[book(701, 90)], [book(701, 100, 0, {asOf: null})]]) {
    const f = fixture(); f.ready(); assert.equal(f.recover(books), false); assert.equal(f.view().ready, false);
  }
});

test('book depth conflicts at the same nanosecond version require recovery, while a newer timestamp applies', () => {
  const f = fixture(); f.ready();
  const newer = book(701); newer.asOf.at = '2026-10-01T21:00:00.000000001Z';
  f.send(batch(1, 0, [newer]));
  assert.equal(f.view().books[0].asOf.at, newer.asOf.at);
  f.send(batch(2, 1, [{...newer, bids: [{price: .395, quantity: 10}]}]));
  assert.equal(f.view().ready, false); assert.ok(f.view().reasons.includes('VERSION_CONFLICT'));
});

test('bounded engine/receipt clock skew, monotonic receipts, and safe integer depth fail closed', () => {
  const invalid = [
    {payload: batch(1, 0, [book(701, 101, 1001)])},
    {payload: batch(1, 0, [book(701)]), receivedAt: at(1001)},
    {payload: batch(1, 0, [book(701)]), receivedAt: at(-1)},
    {payload: batch(1, 0, [book(701, 101, 0, {bids: [{price: .4, quantity: 1.5}]})])},
    {payload: batch(1, 0, [book(701, 101, 0, {bids: [{price: .4, quantity: Number.MAX_SAFE_INTEGER + 1}]})])},
    {payload: batch(1, 0, [book(701, 101, 0, {bids: [{price: .401, quantity: 1}]})])},
    {payload: batch(1, 0, [book(701, 101, 0, {bids: [{price: .6, quantity: 1}]})])},
  ];
  for (const {payload, ...extra} of invalid) { const f = fixture(); f.ready(); assert.equal(f.send(payload, extra), false); assert.equal(f.view().ready, false); }
  const f = fixture(); f.ready(); assert.equal(f.send(batch(1, 0, [book(701, 101, 1000)])), true);
});

test('full REST recovery rejects scope, options, pre-subscription requests and slow or stale receipts', () => {
  for (const mutate of [r => r.endpoint = r.endpoint.replace('depth=200', 'depth=20'), r => r.data.contexts[0].tournament.id = '22222222-2222-4222-8222-222222222222', r => r.data.contexts[0].orderbook.exchanges[0].option = 'NO', r => r.data.contexts[0].orderbook.exchanges[0].exchangeId = '702', r => r.requestStartedAt = at(-1), r => r.receivedAt = at(1001)]) {
    const f = fixture(); f.subscribe(); const ticket = f.bridge.beginSnapshot({marketId, connectionId}), response = f.response(ticket); mutate(response);
    assert.equal(f.bridge.completeSnapshot(ticket, response), false); assert.equal(f.view().ready, false);
  }
  const f = fixture(); f.subscribe(); const ticket = f.bridge.beginSnapshot({marketId, connectionId}); f.advance(20001);
  assert.equal(f.bridge.completeSnapshot(ticket, f.response(ticket)), false);
});

test('quiet duplicate broadcasts cannot renew freshness, while identical authoritative REST can', () => {
  const f = fixture({staleMs: 1000}); f.ready(); f.advance(900); f.send(batch(1, 0, [book(701)]));
  f.advance(1000); assert.equal(f.view().ready, false); assert.deepEqual(f.view().books, []);
  assert.equal(f.recover(), true); f.advance(1999); assert.equal(f.view().ready, true);
});

test('expiry and settlement remain unavailable until the required authoritative reconciliation', () => {
  const f = fixture(); f.ready(); f.send(batch(1, 0, [book(701, 101, 0, {nextExpiryAt: at(1000)})]));
  f.advance(1000); assert.equal(f.view().ready, false); assert.ok(f.view().reasons.includes('EXPIRY'));
  assert.equal(f.recover([book(701, 102, 1000)]), true);
  f.bridge.ingest({marketId, connectionId, event: 'market_settled', receivedAt: at(1000), payload: {marketId: 277, tournamentId: originalScope.tournamentId, settledWith: 'REFUND', at: at(1000)}});
  assert.equal(f.recover([book(701, 102, 1000)]), false); assert.ok(f.view().reasons.includes('SETTLEMENT'));
});

test('account payloads, foreign channels and unknown markets never become capture output', () => {
  const f = fixture(); f.ready();
  assert.equal(f.bridge.ingest({marketId, connectionId, event: 'account_batch', payload: {token: 'DO_NOT_CAPTURE', fills: []}, receivedAt: at(0)}), false);
  assert.equal(JSON.stringify(f.bridge.status()).includes('DO_NOT_CAPTURE'), false);
  assert.equal(f.send(batch(1, 0, [book(701)]), {topic: 'user:foreign'}), false);
  assert.throws(() => f.bridge.view('999'), /UNKNOWN_MARKET/);
  f.bridge.stop(); assert.deepEqual(f.view().books, []); assert.equal(f.send(batch(1, 0, [book(701)])), false);
});

test('returned book copies cannot mutate held state; empty depth remains capture-only valid', () => {
  const f = fixture(); f.ready(); const view = f.view(); view.books[0].bids[0].quantity = 999;
  assert.equal(f.view().books[0].bids[0].quantity, 100);
  f.send(batch(1, 0, [book(701, 101, 0, {bids: [], asks: []})]));
  assert.equal(f.view().ready, true); assert.deepEqual(f.view().books[0].bids, []); assert.equal(f.view().executionAuthorized, false);
});

test('normalized capture records source chronology and full books without arbitrary payload fields', () => {
  const records = [], f = fixture({capture: {append: row => records.push(structuredClone(row))}}); f.ready();
  const payload = batch(1, 0, [book(701, 101)]); payload.token = 'DO_NOT_CAPTURE'; payload.books[0].headers = 'DO_NOT_CAPTURE'; payload.delivery.correlationId = 'DO_NOT_CAPTURE';
  f.send(payload);
  assert.ok(records.some(row => row.kind === 'subscribed'));
  assert.ok(records.some(row => row.kind === 'snapshot'));
  assert.ok(records.some(row => row.kind === 'event' && row.payload.books[0].asOf.sequence === 101));
  assert.equal(JSON.stringify(records).includes('DO_NOT_CAPTURE'), false);
});

test('HTTP snapshot failure and explicit known invalidations retain the socket and require fresh recovery', () => {
  const f = fixture(); f.ready(); const ticket = f.bridge.beginSnapshot({marketId, connectionId});
  assert.equal(f.bridge.snapshotFailed(ticket), true); assert.equal(f.view().state, 'needs_resync');
  assert.equal(f.view().connectionId, connectionId); assert.equal(f.bridge.needsRecovery().length, 1);
  assert.equal(f.bridge.completeSnapshot(ticket, f.response(ticket)), false);
  assert.equal(f.recover(), true);
  assert.equal(f.bridge.invalidate({marketId, connectionId: 'stale-connection', reason: 'SCHEMA'}), false);
  assert.throws(() => f.bridge.invalidate({marketId, connectionId, reason: 'UNTRUSTED_RAW_ERROR'}), /INVALID_INVALIDATION_REASON/);
  assert.equal(f.bridge.invalidate({marketId, connectionId, reason: 'SETTLEMENT'}), true);
  assert.equal(f.view().ready, false); assert.equal(f.view().needsSnapshot, false); assert.deepEqual(f.bridge.needsRecovery(), []);
});

test('clock rollback with an attached capture fails closed without recursive logging', () => {
  const f = fixture({capture: {append() {}}}); f.ready(); f.advance(-1);
  assert.throws(() => f.view(), /CLOCK_ROLLBACK/);
  f.advance(0); assert.equal(f.view().ready, false); assert.ok(f.view().reasons.includes('SCHEMA'));
});

test('equal-version REST cannot erase a known pushed expiry deadline', () => {
  const f = fixture(); f.ready(); f.send(batch(1, 0, [book(701, 101, 0, {nextExpiryAt: at(1000)})]));
  f.advance(500); assert.equal(f.recover([book(701, 101)]), true);
  assert.equal(f.view().nextKnownExpiryAt, '2026-10-01T21:00:01.000000000Z');
  f.advance(1000); assert.equal(f.view().ready, false); assert.ok(f.view().reasons.includes('EXPIRY'));
  assert.equal(f.recover([book(701, 102, 1000)]), true); assert.equal(f.view().nextKnownExpiryAt, null);
});

test('snapshot capture retains actual REST dispatch time separately from queue ticket and receipt', () => {
  const records = [], f = fixture({capture: {append: row => records.push(structuredClone(row))}}); f.subscribe();
  const ticket = f.bridge.beginSnapshot({marketId, connectionId});
  f.advance(12000); const response = f.response(ticket, [book(701, 101, 11900)], {requestStartedAt: at(10000)});
  assert.equal(f.bridge.completeSnapshot(ticket, response), true);
  const captured = records.find(row => row.kind === 'snapshot');
  assert.equal(captured.requestStartedAt, '2026-10-01T21:00:10.000000000Z');
  assert.equal(captured.receivedAt, at(12000));
  assert.equal(captured.books[0].asOf.at, '2026-10-01T21:00:11.900000000Z');
  assert.ok(records.some(row => row.kind === 'recovery_finished' && row.booksReady));
  assert.ok(records.filter(row => row.kind !== 'snapshot').every(row => row.requestStartedAt === undefined));
});

test('failed or invalidated snapshot attempts never acquire another request receipt or recovery success', () => {
  const records = [], f = fixture({capture: {append: row => records.push(structuredClone(row))}}); f.subscribe();
  const invalid = f.bridge.beginSnapshot({marketId, connectionId});
  assert.equal(f.bridge.completeSnapshot(invalid, f.response(invalid, undefined, {requestStartedAt: at(-1)})), false);
  const stale = f.bridge.beginSnapshot({marketId, connectionId});
  f.bridge.invalidate({marketId, connectionId, reason: 'GAP'});
  assert.equal(f.bridge.completeSnapshot(stale, f.response(stale)), false);
  assert.equal(records.some(row => row.kind === 'snapshot'), false);
  assert.equal(records.some(row => row.kind === 'recovery_finished' && row.booksReady), false);
  f.advance(1000); assert.equal(f.recover(), true);
  assert.equal(records.filter(row => row.kind === 'snapshot').length, 1);
  assert.equal(records.find(row => row.kind === 'snapshot').requestStartedAt, '2026-10-01T21:00:01.000000000Z');
});
