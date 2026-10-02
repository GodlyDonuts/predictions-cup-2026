import test from 'node:test';
import assert from 'node:assert/strict';
import {createPassiveCapture, consumePassiveCapture, passiveCaptureReport} from './passive-capture.mjs';
import {RealtimeBooks} from './realtime-books.mjs';

const tid = 'bda92870-621e-47b0-bc3c-3602c5c26f55', topic = 'tournament:' + tid + ':market:377';
const base = Date.parse('2026-10-02T12:00:00.000Z'), at = ms => new Date(base + ms).toISOString();
const scope = {tournamentId: tid, markets: [{marketId: '377', exchangeId: '1066'}]};
const rawBook = (seq, ms, bid = .4, ask = .42) => ({exchangeId: '1066', asOf: {sequence: seq, at: at(ms)},
  nextExpiryAt: null, expiryKnown: true, bids: [{price: bid, quantity: 1000}], asks: [{price: ask, quantity: 1000}]});
const rawTrade = (id, seq, ms) => ({id, sequence: seq, exchangeId: '1066', marketId: '377', tournamentId: tid, price: .4, quantity: 3, executedAt: at(ms)});
function batch(revision, previousRevision, books = [], trades = [], extra = {}) {
  return {event: 'market_batch', payload: {delivery: {model: 'best-effort-authoritative-resync', revision, previousRevision,
    sourceSequenceFrom: null, sourceSequenceThrough: null}, books, trades, bookDirty: [], marketSettled: [], resyncRequired: false, ...extra}};
}
function fixture() {
  let state = createPassiveCapture({scope, sourceHash: 'a'.repeat(64), cutoff: at(0), sessionId: 'fixture'});
  const apply = (kind, ms, extra = {}, epoch = 1) => {
    const out = consumePassiveCapture(state, {format: 'sig-feed-capture', schemaVersion: 1, captureIndex: state.captureIndex + 1,
      topic, epoch, kind, observedAt: at(ms), ...extra}); state = out.state; return out;
  };
  return {apply, get state() { return state; }, get ctx() { return state.contexts['377']; }};
}
function recover(f, offset = 0, seq = 10, generation = 1, epoch = 1) {
  f.apply('recovery_started', offset + 100, {generation}, epoch);
  f.apply('snapshot', offset + 200, {receivedAt: at(offset + 190), requestStartedAt: at(offset + 100), books: [rawBook(seq, offset + 180)]}, epoch);
  return f.apply('recovery_finished', offset + 210, {booksReady: true, generation}, epoch);
}
test('snapshot alone does not recover; decision uses recovery availability and actual request start', () => {
  const f = fixture(); f.apply('recovery_started', 100, {generation: 1});
  f.apply('snapshot', 200, {receivedAt: at(190), requestStartedAt: at(100), books: [rawBook(10, 180)]});
  assert.equal(f.ctx.ready, false); assert.equal(f.ctx.observer.state.pairs, 0);
  const out = f.apply('recovery_finished', 210, {booksReady: true, generation: 1});
  assert.equal(f.ctx.ready, true); assert.equal(f.ctx.observer.state.quotes[0].createdAt, at(210));
  const recorded = out.records.find(r => r.event.kind === 'book').event.book;
  assert.equal(recorded.requestStartedAt, at(100)); assert.equal(recorded.receivedAt, at(210));
  assert.equal(f.ctx.latestBook.receivedAt, at(190));
});
test('first high stream revision, overlapping merged delivery and same revision newer book remain continuous', () => {
  const f = fixture(); recover(f);
  f.apply('event', 900, {receivedAt: at(900), ...batch(100, 99, [rawBook(20, 900)])});
  assert.equal(f.ctx.ready, true); assert.equal(f.ctx.observer.state.ready, true);
  assert.equal(f.ctx.observer.state.quotes[0].activationStatus, 'observed_nonmarketable');
  f.apply('event', 1000, {receivedAt: at(1000), ...batch(103, 98, [rawBook(22, 1000)], [rawTrade('trade_1', 21, 950)])});
  assert.equal(f.ctx.observer.state.quotes[0].qualifiedTradeThroughQuantity, '3');
  f.apply('event', 1100, {receivedAt: at(1100), ...batch(103, 98, [rawBook(23, 1100)])});
  assert.equal(f.ctx.observer.state.ready, true); assert.equal(f.ctx.observer.state.lastBook.sequence, '23');
  f.apply('event', 1200, {receivedAt: at(1200), ...batch(105, 104, [rawBook(25, 1200)], [rawTrade('trade_2', 24, 1150)])});
  assert.equal(f.ctx.ready, false); assert.equal(f.ctx.observer.state.quotes[0].qualifiedTradeThroughQuantity, '3');
});
test('recovery_started suppresses subsequent event evidence and selects a newer push received during recovery', () => {
  const f = fixture(); recover(f);
  f.apply('recovery_started', 300, {generation: 2}); assert.equal(f.ctx.ready, false);
  f.apply('event', 400, {receivedAt: at(400), ...batch(100, 99, [rawBook(30, 400)], [rawTrade('recovering_trade', 29, 390)])});
  assert.equal(f.ctx.observer.state.quotes[0].qualifiedTradeThroughQuantity, '0');
  f.apply('snapshot', 500, {receivedAt: at(490), requestStartedAt: at(300), books: [rawBook(20, 450)]});
  f.apply('recovery_finished', 510, {booksReady: true, generation: 2});
  assert.equal(f.ctx.ready, true); assert.equal(f.ctx.observer.state.lastBook.sequence, '30');
  assert.equal(f.ctx.observer.state.lastBook.requestStartedAt, at(400));
});
test('missing, failed or interrupted recovery cannot promote snapshot state', () => {
  for (const mode of ['missing_request', 'failed', 'invalidated']) {
    const f = fixture(); f.apply('recovery_started', 100, {generation: 1});
    f.apply('snapshot', 200, {receivedAt: at(190), ...(mode === 'missing_request' ? {} : {requestStartedAt: at(100)}), books: [rawBook(10, 180)]});
    if (mode === 'invalidated') f.apply('invalidation', 205, {reason: 'GAP'});
    f.apply('recovery_finished', 210, {booksReady: mode !== 'failed', generation: 1});
    assert.equal(f.ctx.ready, false); assert.equal(f.ctx.observer.state.pairs, 0);
  }
});
test('batch-wide invalidation precedes evidence, including missing book and unsequenced trade', () => {
  for (const mode of ['missing_book', 'unknown_sequence', 'settled', 'empty_book']) {
    const f = fixture(); recover(f); f.apply('event', 900, {receivedAt: at(900), ...batch(100, 99, [rawBook(20, 900)])});
    const t = rawTrade('bad_boundary', 21, 950), b = rawBook(22, 1000);
    if (mode === 'unknown_sequence') { t.id = null; t.sequence = null; }
    if (mode === 'empty_book') b.asks = [];
    const extra = mode === 'settled' ? {marketSettled: [{marketId: '377', tournamentId: tid, at: at(950), settledWith: 'YES'}]} : {};
    f.apply('event', 1000, {receivedAt: at(1000), ...batch(101, 100, mode === 'missing_book' ? [] : [b], [t], extra)});
    assert.equal(f.ctx.ready, false, mode); assert.equal(f.ctx.observer.state.quotes[0].qualifiedTradeThroughQuantity, '0', mode);
    assert.equal(f.ctx.observer.state.quotes[0].markouts['60000'].status, 'censored_continuity', mode);
  }
});
test('buffered earlier receipt cannot backdate an observation or sneak into a later recovery', () => {
  const f = fixture(); recover(f);
  const ignored = f.apply('event', 300, {receivedAt: at(200), ...batch(100, 99, [rawBook(20, 200)])});
  assert.equal(ignored.events[0].kind, 'ignored_pre_recovery_receipt'); assert.equal(f.ctx.observer.state.lastAt, at(210));
  f.apply('invalidation', 400, {reason: 'GAP'}); f.apply('recovery_started', 500, {generation: 2});
  f.apply('event', 600, {receivedAt: at(450), ...batch(101, 100, [rawBook(30, 450)])});
  f.apply('snapshot', 700, {receivedAt: at(690), requestStartedAt: at(500), books: [rawBook(20, 650)]});
  f.apply('recovery_finished', 710, {booksReady: true, generation: 2});
  assert.equal(f.ctx.observer.state.lastBook.sequence, '20'); assert.equal(f.ctx.observer.state.lastBook.receivedAt, at(710));
});
test('duplicate broadcasts neither refresh depth freshness nor double-count derived trade rows', () => {
  const f = fixture(); recover(f);
  f.apply('event', 900, {receivedAt: at(900), ...batch(100, 99, [rawBook(20, 900)])});
  f.apply('event', 1000, {receivedAt: at(1000), ...batch(101, 100, [rawBook(22, 1000)], [rawTrade('once', 21, 950)])});
  f.apply('trade_observed', 1000, {trade: rawTrade('once', 21, 950), continuousAtReceipt: true});
  assert.equal(f.ctx.observer.state.quotes[0].qualifiedTradeThroughQuantity, '3');
  f.apply('event', 15000, {receivedAt: at(15000), ...batch(102, 101, [rawBook(22, 1000)])});
  assert.equal(f.ctx.latestBook.receivedAt, at(1000));
  f.apply('event', 21000, {receivedAt: at(21000), ...batch(103, 102, [rawBook(22, 1000)], [rawTrade('stale', 23, 20900)])});
  assert.equal(f.ctx.ready, false); assert.equal(f.ctx.observer.state.quotes[0].qualifiedTradeThroughQuantity, '3');
});
test('retained known expiry blocks unknown-expiry REST from falsely restoring readiness', () => {
  const f = fixture(); recover(f);
  const expiring = {...rawBook(20, 900), nextExpiryAt: at(1000)};
  f.apply('event', 900, {receivedAt: at(900), ...batch(100, 99, [expiring])});
  f.apply('recovery_started', 1000, {generation: 2});
  f.apply('snapshot', 1100, {receivedAt: at(1090), requestStartedAt: at(1000), books: [{...rawBook(20, 900), expiryKnown: false}]});
  f.apply('recovery_finished', 1110, {booksReady: true, generation: 2});
  assert.equal(f.ctx.ready, false);
});
test('stopped source is explicit and ordering, scope and epoch violations fail closed', () => {
  const f = fixture(); recover(f); f.apply('stopped', 300);
  assert.equal(f.state.sourceEnded, true); assert.equal(passiveCaptureReport(f.state).sourceEnded, true);
  assert.equal(passiveCaptureReport(f.state).pnl, null);
  assert.throws(() => f.apply('event', 299, batch(100, 99)), /clock rollback/);
  assert.throws(() => f.apply('event', 400, batch(100, 99), 0), /epoch rollback/);
  assert.throws(() => consumePassiveCapture(f.state, {format: 'sig-feed-capture', schemaVersion: 1, captureIndex: 999}), /reordered/);
});
test('actual RealtimeBooks normalized capture replays without stripped wire-only fields', () => {
  let now = base;
  const rows = [], capture = {append: row => rows.push({format: 'sig-feed-capture', schemaVersion: 1, captureIndex: rows.length, ...row})};
  const bridge = new RealtimeBooks(scope, {now: () => now, capture});
  const connectionId = 'fixture_connection';
  bridge.subscribed({marketId: '377', connectionId, receivedAt: at(0)});
  now = base + 100; const ticket = bridge.beginSnapshot({marketId: '377', connectionId});
  now = base + 190;
  assert.equal(bridge.completeSnapshot(ticket, {endpoint: ticket.endpoint, status: 200, requestStartedAt: at(100), receivedAt: at(190),
    data: {contexts: [{type: 'tournament', tournament: {id: tid}, orderbook: {exchanges: [{...rawBook(10, 180), option: 'YES'}]}}]}}), true);
  assert.match(rows.find(r => r.kind === 'snapshot').requestStartedAt, /\.100000000Z$/);
  now = base + 900;
  const e = batch(100, 99, [rawBook(20, 900)]); e.payload.delivery.correlationId = 'diagnostic_not_captured';
  assert.equal(bridge.ingest({marketId: '377', connectionId, receivedAt: at(900), ...e}), true);
  const normalized = rows.find(r => r.kind === 'event');
  assert.equal(Object.hasOwn(normalized.payload.delivery, 'correlationId'), false);
  let state = createPassiveCapture({scope, sourceHash: 'a'.repeat(64), cutoff: at(0), sessionId: 'actual_capture'});
  for (const row of rows) state = consumePassiveCapture(state, row).state;
  assert.equal(state.contexts['377'].ready, true);
  assert.equal(state.contexts['377'].observer.state.quotes[0].activationStatus, 'observed_nonmarketable');
  assert.equal(state.contexts['377'].observer.state.quotes[0].createdAt, at(190));
  now = base + 1000;
  const traded = batch(101, 100, [rawBook(22, 1000)], [rawTrade('actual_trade', 21, 950)]);
  traded.payload.delivery.correlationId = 'diagnostic_not_captured';
  assert.equal(bridge.ingest({marketId: '377', connectionId, receivedAt: at(1000), ...traded}), true);
  for (const row of rows.slice(state.captureIndex + 1)) state = consumePassiveCapture(state, row).state;
  assert.equal(state.contexts['377'].observer.state.quotes[0].qualifiedTradeThroughQuantity, '3');
  now = base + 1100;
  assert.equal(bridge.ingest({marketId: '377', connectionId, receivedAt: at(1100), event: 'market_settled',
    payload: {marketId: '377', tournamentId: tid, at: at(1100), settledWith: 'YES'}}), true);
  const settled = rows.find(r => r.kind === 'event' && r.event === 'market_settled');
  assert.equal(Object.hasOwn(settled.payload, 'settledWith'), false); assert.equal(settled.payload.refund, false);
  for (const row of rows.slice(state.captureIndex + 1)) state = consumePassiveCapture(state, row).state;
  assert.equal(state.contexts['377'].ready, false);
});
