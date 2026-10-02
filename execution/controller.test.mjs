import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {CompetitionController} from './controller.mjs';
import {digest} from '../live-paper/model.mjs';
import {validateExecutionState} from './core.mjs';

const TID = 'bda92870-621e-47b0-bc3c-3602c5c26f55';
const stamp = n => new Date(n).toISOString();
const page = rows => ({data: rows, pagination: {limit: 200, hasMore: false, nextCursor: null}});

function fixture(t, options = {}) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'sig-controller-test-'));
  let time = Date.now() + 1000, stopped = false, controller;
  const universe = {exchanges: [{marketId: '377', exchangeId: '700', option: 'YES'}, {marketId: '378', exchangeId: '701', option: 'YES'}]};
  const activation = {schemaVersion: 1, ownerId: 'Codex', tournamentId: TID, runId: 'sig-pilot-fixture', enabled: true, formerWriterStopCommentId: 5947535773, feeBps: 0, maxOrderCost: '25.000000', maxAggregateCost: '100.000000', maxCumulativeBuyCost: '100.000000', maxManagedMarkets: 3, depthFractionBps: 100, startedAt: stamp(time - 1000), entryExpiresAt: stamp(time + 6 * 3600000 - 1000)};
  fs.writeFileSync(path.join(dir, 'competition-activation.json'), JSON.stringify(activation));
  const policy = {...JSON.parse(fs.readFileSync(new URL('../live-paper/dip-policy.json', import.meta.url))), executionPolicy: 'responsive_v2', requireScheduledEntry: true, maxOpenMarkets: 3, maximumHoldMs: options.maximumHoldMs || 1000, cooldownMs: 1};
  const bank = {policy, identities: {'377': {state: 'FL', office: 'Senate'}, '378': {state: 'GA', office: 'Senate'}}};
  fs.writeFileSync(path.join(dir, 'dip-responsive-bank.json'), JSON.stringify(bank));
  function seedHistory(center = .55) {
    const candles = Array.from({length: 8}, (_, n) => ({time: stamp(time - (45 - n * 5) * 60000), vwap: center}));
    fs.writeFileSync(path.join(dir, 'dip-responsive-state.json'), JSON.stringify({bankHash: digest(bank), histories: {'700': {receivedAt: stamp(time - 5000), coverage: {complete: true}, candles}}, bookHistory: {}}));
  }
  seedHistory(options.noSignal ? .50 : .55);
  const server = {cash: 100000, quantities: {...(options.externalQuantity ? {'701': options.externalQuantity} : {})}, orders: new Map(), operations: new Map(), posts: [], deletes: [], fraction: options.partial ? .5 : 1, ambiguity: options.ambiguous === true, beforeGuard: null, afterGuard: null, marketOpen: true, ignoreExpiry: false, staleFills: false, cancelAmbiguous: false, fillOnCancel: false, rejectNext: false, wrongBootstrapBook: false};
  function response(endpoint, data) { return {status: 200, endpoint, requestStartedAt: stamp(time - 20), receivedAt: stamp(time), data}; }
  function positions() {
    return response('/tournaments/midterm-elections/portfolio/positions', {positions: Object.entries(server.quantities).filter(([, q]) => q !== 0).map(([exchangeId, quantity]) => ({exchangeId, marketId: exchangeId === '700' ? '377' : '378', quantity, lots: []})), summary: {totalMarketValue: 0, totalCostBasis: 0, totalUnrealizedPnl: 0}});
  }
  function expireOrders() { if (server.ignoreExpiry) return; for (const o of server.orders.values()) if (time >= Date.parse(o.body.expirationDate)) o.open = false; }
  function officialOrder(o) { return {id: o.id, exchangeId: o.body.exchangeId, tournamentId: TID, side: 'yes', action: o.body.action, quantity: o.open ? o.body.quantity - o.filled : o.body.quantity, priceLimit: o.body.price, open: o.open, createdAt: o.createdAt, expirationDate: o.body.expirationDate}; }
  function openOrders() { expireOrders(); return [...server.orders.values()].filter(o => o.open).map(officialOrder); }
  const api = {async dispatch(spec) {
    time += 30;
    if (spec.method === 'DELETE') {
      if (server.beforeGuard) server.beforeGuard(); await spec.beforeDispatch(); if (server.afterGuard) server.afterGuard(); await spec.beforeNetwork();
      const id = Number(spec.endpoint.split('/').at(-1)), o = server.orders.get(id); assert.ok(o); server.deletes.push(id);
      if (server.fillOnCancel && o.filled < o.body.quantity) {
        const quantity = o.body.quantity - o.filled; o.fills.push({id: o.fillId + 10000, price: o.body.price, quantity, side: 'yes', filledAt: stamp(time)}); o.filled += quantity;
        server.quantities[o.body.exchangeId] += (o.body.action === 'buy' ? quantity : -quantity); server.cash += (o.body.action === 'buy' ? -1 : 1) * quantity * o.body.price;
      }
      o.open = false;
      if (server.cancelAmbiguous) {server.cancelAmbiguous = false; const e = Error('lost cancel response'); e.status = 502; e.apiCode = 'ORDER_STATUS_UNKNOWN'; throw e;}
      return response(spec.endpoint, {orderId: id, tournamentId: TID});
    }
    if (spec.method === 'POST') {
      if (server.beforeGuard) server.beforeGuard();
      await spec.beforeDispatch();
      if (server.afterGuard) server.afterGuard();
      await spec.beforeNetwork();
      server.posts.push({body: structuredClone(spec.body), bodyJson: spec.bodyJson});
      if (server.rejectNext) {server.rejectNext = false; const e = Error('invented rejection'); e.status = 400; e.apiCode = 'VALIDATION_ERROR'; throw e;}
      let operation = server.operations.get(spec.body.idempotencyKey);
      if (operation) {
        assert.equal(operation.bodyJson, spec.bodyJson, 'Server idempotency payload must remain byte-identical');
        return response(spec.endpoint, operation.result);
      }
      const body = spec.body, filled = Math.max(0, Math.floor(body.quantity * server.fraction));
      const order = {id: 1001 + server.orders.size, body: structuredClone(body), filled, open: filled < body.quantity, createdAt: stamp(time), fillId: 2001 + server.orders.size};
      order.fills = filled ? [{id: order.fillId, price: body.price, quantity: filled, side: 'yes', filledAt: order.createdAt}] : [];
      server.orders.set(order.id, order);
      server.quantities[body.exchangeId] = (server.quantities[body.exchangeId] || 0) + (body.action === 'buy' ? filled : -filled);
      server.cash += (body.action === 'buy' ? -1 : 1) * filled * body.price;
      const result = {orderId: order.id, exchangeId: body.exchangeId, open: order.open, remainingQuantity: body.quantity - filled, action: body.action, side: 'yes', price: body.price, quantity: body.quantity, terminalReasonCode: null, quantityTraded: filled, totalCost: filled * body.price, fillPrice: body.price, all: null};
      server.operations.set(body.idempotencyKey, {bodyJson: spec.bodyJson, result});
      if (server.ambiguity) { server.ambiguity = false; const e = Error('lost response'); e.status = 502; e.apiCode = 'ORDER_STATUS_UNKNOWN'; throw e; }
      return response(spec.endpoint, result);
    }
    assert.equal(spec.method, 'GET'); expireOrders();
    if (spec.endpoint === '/tournaments/midterm-elections') return response(spec.endpoint, {id: TID, slug: 'midterm-elections', status: 'active', myBalance: server.cash, isPendingEnrolment: false});
    if (spec.endpoint === '/tournaments/midterm-elections/portfolio/positions') return positions();
    if (spec.endpoint.startsWith('/orders?')) return response(spec.endpoint, page(openOrders()));
    if (spec.endpoint.startsWith('/markets/377/orderbook?')) {const r = book(); if (server.wrongBootstrapBook) r.data.contexts[0].orderbook.exchanges[0].exchangeId = '701'; return r;}
    const market = /^\/markets\/(\d+)\?/.exec(spec.endpoint);
    if (market) return response(spec.endpoint, {id: market[1], title: 'Invented contract', status: server.marketOpen ? 'open' : 'closed', isComposite: false, isMultiOutcome: false, exchanges: [{id: market[1] === '377' ? '700' : '701', option: 'YES'}], contexts: [{type: 'tournament', tournament: {id: TID}, status: server.marketOpen ? 'open' : 'closed', settledWith: null}]});
    const own = /^\/orders\/(\d+)(\/fills)?/.exec(spec.endpoint);
    if (own) {
      const o = server.orders.get(Number(own[1])); assert.ok(o, 'Only a real fixture order is readable');
      if (!own[2]) return response(spec.endpoint, officialOrder(o));
      const r = response(spec.endpoint, {...page(o.fills), orderId: o.id, exchangeId: o.body.exchangeId, tournamentId: TID, totalQuantityFilled: o.filled, avgFillPrice: o.filled ? o.body.price : null, coverage: {complete: true, projectedThroughSequence: o.fillId}});
      if (server.staleFills) r.requestStartedAt = stamp(time - 180001); return r;
    }
    throw Error('Unexpected fake endpoint: ' + spec.endpoint);
  }};
  function start() { controller = new CompetitionController({runtimeDir: dir, api, universe, config: {}, isStopped: () => stopped, activation, now: () => time}); return controller; }
  function book({bid = .495, ask = .500, depth = 2000} = {}) { return response('/markets/377/orderbook?tournamentId=' + TID + '&depth=20', {contexts: [{type: 'tournament', tournament: {id: TID}, orderbook: {exchanges: [{exchangeId: '700', option: 'YES', asOf: {sequence: time, at: stamp(time - 1000)}, bids: [{price: bid, quantity: depth}], asks: [{price: ask, quantity: depth}]}]}}]}); }
  async function before({stale = false} = {}) { const p = positions(); if (stale) p.receivedAt = stamp(time - 200001); await controller.beforeBooks({positions: p, orders: openOrders(), ordersAt: stamp(time), candidateMarketIds: ['377']}); }
  async function cycle(options = {}) { await before(options); await controller.onBook(book(options)); }
  t.after(() => { try { controller?.close(); } finally { const target = path.resolve(dir); assert.ok(target.startsWith(path.resolve(os.tmpdir()) + path.sep)); fs.rmSync(target, {recursive: true, force: true}); } });
  start();
  return {get controller() { return controller; }, server, api, dir, activation, before, cycle, book, seedHistory, stop() { stopped = true; }, advance(ms) { time += ms; }, restart() { controller.close(); return start(); }};
}

test('qualifying signal journals one official BUY, reconciles it, then sells only owned inventory', async t => {
  const f = fixture(t); await f.cycle();
  assert.equal(f.server.posts.length, 1); assert.equal(f.controller.active().status, 'acknowledged');
  assert.equal(f.server.posts[0].body.action, 'buy'); assert.equal(f.server.posts[0].body.quantity, 20);
  await f.before(); assert.equal(f.controller.state.owned['700'].quantity, 20); assert.equal(f.controller.active(), undefined);
  f.advance(2000); await f.cycle({bid: .520, ask: .525});
  assert.equal(f.server.posts.length, 2); assert.equal(f.server.posts[1].body.action, 'sell'); assert.equal(f.server.posts[1].body.quantity, 20);
  await f.before(); assert.deepEqual(f.controller.state.owned, {});
  assert.equal(JSON.parse(fs.readFileSync(path.join(f.dir, 'competition-stats.json'))).realizedPnl, '0.400000');
  assert.deepEqual(validateExecutionState(f.controller.state), f.controller.state);
});

test('partial entry remains pending until terminal reconciliation; sale cannot oversell the filled shares', async t => {
  const f = fixture(t, {partial: true}); await f.cycle(); await f.before();
  assert.equal(f.controller.active().status, 'reconciled'); assert.equal(f.controller.state.owned['700'].quantity, 10);
  await f.cycle(); assert.equal(f.server.posts.length, 1);
  for (const order of f.server.orders.values()) order.open = false;
  await f.before(); assert.equal(f.controller.active(), undefined);
  f.server.fraction = 1; f.advance(2000); await f.cycle({bid: .520, ask: .525}); await f.before();
  assert.equal(f.server.posts[1].body.action, 'sell'); assert.equal(f.server.posts[1].body.quantity, 10); assert.deepEqual(f.controller.state.owned, {});
});

test('no qualifying reference creates no intent or order', async t => {
  const f = fixture(t, {noSignal: true}); await f.cycle(); assert.equal(f.server.posts.length, 0); assert.equal(Object.keys(f.controller.state.intents).length, 0);
});

test('STOP after queue wait prevents a network write and preserves a closed undispatched intent', async t => {
  const f = fixture(t); f.server.beforeGuard = () => f.stop(); await f.cycle();
  assert.equal(f.server.posts.length, 0); const intent = Object.values(f.controller.state.intents)[0]; assert.equal(intent.status, 'closed'); assert.equal(intent.attempts, 0);
});

test('STOP after durable admission prevents network and retains uncertainty for recovery', async t => {
  const f = fixture(t); f.server.afterGuard = () => f.stop(); await f.cycle();
  assert.equal(f.server.posts.length, 0); assert.equal(f.controller.active().status, 'uncertain'); assert.equal(f.controller.active().attempts, 1);
});

test('a book that becomes stale in the shared queue cannot dispatch its prepared intent', async t => {
  const f = fixture(t); f.server.beforeGuard = () => f.advance(21000); await f.cycle();
  assert.equal(f.server.posts.length, 0); const intent = Object.values(f.controller.state.intents)[0];
  assert.equal(intent.status, 'closed'); assert.equal(intent.attempts, 0); assert.equal(f.controller.reason, 'STALE_BOOK_EVIDENCE');
});

test('stale full-account snapshot prevents preparation even with a fresh book', async t => {
  const f = fixture(t); await f.controller.bootstrap(); await f.cycle({stale: true}); assert.equal(f.server.posts.length, 0); assert.equal(f.controller.reason, 'STALE_ACCOUNT_EVIDENCE');
});

test('accepted-but-unconfirmed restart blocks new entries and recovers exact same key after expiry', async t => {
  const f = fixture(t, {ambiguous: true}); await f.cycle();
  assert.equal(f.controller.active().status, 'uncertain'); const original = f.server.posts[0]; assert.equal(f.server.orders.size, 1);
  f.restart(); await f.cycle(); assert.equal(f.server.posts.length, 1); assert.equal(f.controller.active().status, 'uncertain');
  f.advance(120000); await f.before();
  assert.equal(f.server.posts.length, 2); assert.equal(f.server.posts[1].bodyJson, original.bodyJson); assert.equal(f.server.orders.size, 1); assert.equal(f.controller.active().status, 'acknowledged');
  await f.before(); assert.equal(f.controller.state.owned['700'].quantity, 20); assert.equal(f.controller.active(), undefined);
});

test('unrelated legacy holdings are neither adopted nor sold', async t => {
  const f = fixture(t, {externalQuantity: 250}); await f.cycle(); await f.before();
  assert.equal(f.controller.state.owned['701'], undefined); assert.equal(f.server.quantities['701'], 250);
  f.advance(2000); await f.cycle(); await f.before(); assert.equal(f.server.quantities['701'], 250); assert.ok(f.server.posts.every(x => x.body.exchangeId === '700'));
});

test('100 SUSQ cumulative entry budget survives completed exits and blocks a fifth 25 SUSQ buy', async t => {
  const f = fixture(t);
  for (let n = 0; n < 4; n++) {
    f.seedHistory(); await f.cycle({depth: 10000}); await f.before(); assert.equal(f.controller.state.owned['700']?.quantity, 50, 'bounded 25 SUSQ buy ' + n);
    f.advance(2000); await f.cycle({depth: 10000}); await f.before(); assert.deepEqual(f.controller.state.owned, {});
    f.advance(2000);
  }
  f.restart(); f.seedHistory(); await f.cycle({depth: 10000});
  const buys = f.server.posts.filter(x => x.body.action === 'buy'); assert.equal(buys.length, 4); assert.equal(buys.reduce((n, x) => n + x.body.price * x.body.quantity, 0), 100);
  assert.equal(f.server.posts.length, 8); assert.equal(f.controller.active(), undefined);
});

test('bootstrap uses only official scoped reads and blocks writes until schema compatibility succeeds', async t => {
  const f = fixture(t); assert.equal(f.controller.canWrite(), false);
  f.server.wrongBootstrapBook = true; let r = await f.controller.bootstrap(); assert.equal(r.complete, false); assert.equal(f.controller.canWrite(), false); assert.equal(f.server.posts.length, 0);
  f.server.wrongBootstrapBook = false; r = await f.controller.bootstrap(); assert.equal(r.complete, true); assert.equal(r.marketId, '377'); assert.equal(r.exchangeId, '700'); assert.equal(f.controller.canWrite(), true); assert.equal(f.server.posts.length, 0);
});

test('fresh bootstrap account is not overwritten by stale caller observations on the first cycle', async t => {
  const f = fixture(t); await f.cycle({stale: true}); assert.equal(f.controller.readiness.complete, true); assert.equal(f.server.posts.length, 1);
});

test('wrong exchange under an otherwise valid tournament book cannot submit', async t => {
  const f = fixture(t); await f.before(); const b = f.book(); b.data.contexts[0].orderbook.exchanges[0].exchangeId = '701';
  await assert.rejects(f.controller.onBook(b), /BOOK_EXCHANGE_IDENTITY_MISMATCH/); assert.equal(f.server.posts.length, 0); assert.equal(Object.keys(f.controller.state.intents).length, 0);
});

test('a fresh market closure invalidates previously cached open metadata', async t => {
  const f = fixture(t, {noSignal: true}); await f.before(); assert.ok(f.controller.markets['377']);
  f.server.marketOpen = false; await f.before(); assert.equal(f.controller.markets['377'], undefined);
  f.seedHistory(.55); await f.controller.onBook(f.book()); assert.equal(f.server.posts.length, 0);
});

test('fill reads older than180 seconds block reconciliation despite fresh account reads', async t => {
  const f = fixture(t); await f.cycle(); f.server.staleFills = true; await f.before();
  assert.equal(f.controller.status, 'reconciliation_blocked'); assert.equal(f.controller.reason, 'STALE_ORDER_EVIDENCE'); assert.deepEqual(f.controller.state.owned, {}); assert.ok(f.controller.active());
});

test('old execution timestamps remain valid when current official lifecycle reads are fresh', async t => {
  const f = fixture(t); await f.cycle(); f.advance(240000); await f.before();
  assert.equal(f.controller.active(), undefined); assert.equal(f.controller.state.owned['700'].quantity, 20);
});

test('a stop obligation survives no executable size, restart, partial expiry and price recovery until flat', async t => {
  const f = fixture(t, {maximumHoldMs: 3600000}); await f.cycle(); await f.before();
  f.advance(2000); await f.cycle({bid: .465, ask: .470, depth: 10});
  assert.equal(f.server.posts.length, 1); assert.equal(f.controller.state.control.exits['700'].reason, 'stop_loss');
  f.restart(); f.server.fraction = .5; await f.cycle({bid: .495, ask: .500}); assert.equal(f.server.posts.length, 2); assert.equal(f.server.posts[1].body.action, 'sell');
  await f.before(); assert.equal(f.controller.state.owned['700'].quantity, 10); assert.ok(f.controller.state.control.exits['700']);
  f.advance(31000); await f.before(); assert.equal(f.controller.active(), undefined); assert.ok(f.controller.state.control.exits['700']);
  f.restart(); f.server.fraction = 1; await f.cycle({bid: .495, ask: .500}); await f.before();
  assert.equal(f.server.posts[2].body.quantity, 10); assert.deepEqual(f.controller.state.owned, {}); assert.deepEqual(f.controller.state.control.exits, {});
});

test('a rejected stop order leaves its durable obligation active through a rebound', async t => {
  const f = fixture(t, {maximumHoldMs: 3600000}); await f.cycle(); await f.before(); f.server.rejectNext = true;
  await f.cycle({bid: .465, ask: .470}); assert.equal(f.controller.active(), undefined); assert.ok(f.controller.state.control.exits['700']);
  f.restart(); await f.cycle({bid: .495, ask: .500}); await f.before(); assert.equal(f.server.posts.length, 3); assert.deepEqual(f.controller.state.control.exits, {});
});

test('holding timeout is latched before the depth participation rule rejects a zero-size sale', async t => {
  const f = fixture(t); await f.cycle(); await f.before(); f.advance(2000); await f.cycle({depth: 10});
  assert.equal(f.server.posts.length, 1); assert.equal(f.controller.state.control.exits['700'].reason, 'holding_timeout');
  f.restart(); assert.equal(f.controller.state.control.exits['700'].reason, 'holding_timeout');
});

test('expired known order cleanup is durably admitted and stays pending until authoritative closure', async t => {
  const f = fixture(t, {partial: true}); f.server.ignoreExpiry = true; await f.cycle();
  const active = f.controller.active(); assert.equal(Date.parse(active.expirationDate) - Date.parse(active.preparedAt), 30000);
  f.server.afterGuard = () => {
    const rows = fs.readFileSync(path.join(f.dir, 'competition', 'core-journal.jsonl'), 'utf8').trim().split('\n').map(JSON.parse);
    assert.equal(rows.at(-1).event.type, 'cancel_dispatch_admitted'); assert.equal(rows.at(-1).state.control.cancellations[active.intentId].phase, 'dispatching');
  };
  f.advance(31000); await f.before(); assert.deepEqual(f.server.deletes, [active.orderId]); assert.equal(f.controller.active().status, 'reconciled'); assert.equal(f.controller.state.control.cancellations[active.intentId].phase, 'acknowledged');
  await f.before(); assert.equal(f.controller.active(), undefined); assert.equal(f.controller.state.control.cancellations[active.intentId].phase, 'reconciled'); assert.equal(f.controller.state.owned['700'].quantity, 10);
});

test('cancel/fill race with lost cancellation response never permits a new order before full reconciliation', async t => {
  const f = fixture(t, {partial: true}); f.server.ignoreExpiry = true; await f.cycle();
  const id = f.controller.active().intentId; f.server.cancelAmbiguous = true; f.server.fillOnCancel = true; f.advance(31000); await f.before();
  assert.equal(f.controller.state.control.cancellations[id].phase, 'uncertain'); assert.ok(f.controller.active()); assert.equal(f.server.posts.length, 1);
  f.restart(); await f.cycle(); assert.equal(f.server.posts.length, 1); assert.equal(f.controller.active(), undefined); assert.equal(f.controller.state.owned['700'].quantity, 20); assert.equal(f.controller.state.control.cancellations[id].phase, 'reconciled');
});

test('cleanup cannot cancel an unrelated order or a known order without an active expiry permit', async t => {
  const f = fixture(t, {partial: true}); await f.cycle(); const i = f.controller.active();
  await assert.rejects(f.controller.transport.cancel(9999, {permit: {intentId: i.intentId, runId: f.activation.runId, kind: 'cancel', orderId: 9999}, lease: f.controller.lease}), /ORDER_NOT_OWNED/);
  await assert.rejects(f.controller.transport.cancel(i.orderId, {permit: {intentId: i.intentId, runId: f.activation.runId, kind: 'cancel', orderId: i.orderId}, lease: f.controller.lease}), /INVALID_EXECUTION_PERMIT/);
  assert.equal(f.server.deletes.length, 0);
});

test('slow durable admission cannot dispatch stale evidence at the final network fence', async t => {
  const f = fixture(t); f.server.afterGuard = () => f.advance(25000); await f.cycle();
  assert.equal(f.server.posts.length, 0); assert.equal(f.controller.active().status, 'uncertain'); assert.equal(f.controller.reason, 'STALE_BOOK_EVIDENCE');
});

test('controller metadata is validated on durable replay', async t => {
  const f = fixture(t); await f.cycle(); const state = structuredClone(f.controller.state); state.control.cancellations.bad = {phase: 'dispatching'};
  f.controller.journal.commit(state, {type: 'invented_corruption'});
  assert.throws(() => f.restart(), /INVALID_CANCEL_CONTROL/);
});
