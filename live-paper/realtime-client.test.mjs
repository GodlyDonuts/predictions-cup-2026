import test from 'node:test';
import assert from 'node:assert/strict';
import {createRealtimeClient, isRealtimeTokenDispatch} from './realtime-client.mjs';
import {createClient as createOfficialClient} from '@supabase/supabase-js';

const tournamentId = 'bda92870-621e-47b0-bc3c-3602c5c26f55';
const startTime = Date.parse('2026-10-02T12:00:00Z');
const markets = [{marketId: '377', exchangeId: '1066'}, {marketId: '354', exchangeId: '1043'}];
const TOKEN = 'mock-sensitive-token-never-publish', ANON = 'mock-sensitive-anon-never-publish';
const settle = async () => { for (let i = 0; i < 12; i++) await Promise.resolve(); };
function fixture(options = {}) {
  const x = {now: startTime, allowed: true, requests: [], writes: 0, sdkCalls: [], subscribed: [], events: [], disconnected: [], resync: [], timers: new Map(), ...options};
  let nextTimer = 0;
  x.setTimer = (fn, ms) => { const id = {id: ++nextTimer, unref() {}}; x.timers.set(id, {fn, ms}); return id; };
  x.clearTimer = id => x.timers.delete(id);
  x.fire = async ms => { const found = [...x.timers.entries()].find(([,v]) => v.ms === ms); assert.ok(found, 'timer exists'); x.timers.delete(found[0]); found[1].fn(); await settle(); };
  x.dispatch = async spec => {
    assert.equal(x.client.authorizes(spec), true); x.requests.push(spec);
    await spec.beforeDispatch(); x.afterAdmission?.(); await spec.beforeNetwork(); x.writes++;
    if (x.tokenError) throw Object.assign(Error(TOKEN), {code: 'REALTIME_' + TOKEN});
    return {status: 200, data: {token: TOKEN + x.writes, anonKey: ANON, supabaseUrl: 'https://project.example.test',
      expiresAt: new Date(x.now + 10800000).toISOString(), channels: {user: 'user:private-account'}, ...x.tokenOverride}};
  };
  x.createClient = (url, key, opts) => {
    if (x.factoryError) throw Object.assign(Error(TOKEN), {code: 'REALTIME_' + TOKEN});
    const sdk = {url, key, opts, channels: [], auth: [], removed: 0, disconnected: 0};
    sdk.realtime = {setAuth: async token => { sdk.auth.push(token); if (x.authHang) return new Promise(() => {}); }, disconnect: () => { sdk.disconnected++; },
      connectionState: () => 'open', onHeartbeat: callback => { sdk.heartbeat = callback; }};
    sdk.removeAllChannels = async () => { sdk.removed++; };
    sdk.channel = (topic, config) => {
      if (x.channelError) throw Error(TOKEN);
      const ch = {topic, config, handlers: new Map()};
      ch.on = (kind, filter, fn) => { assert.equal(kind, 'broadcast'); ch.handlers.set(filter.event, fn); return ch; };
      ch.subscribe = fn => { ch.notify = fn; if (x.autoJoin !== false) queueMicrotask(() => fn('SUBSCRIBED')); return ch; };
      ch.push = (event, payload) => { ch.handlers.get('*')?.({event, payload}); ch.handlers.get(event)?.({event, payload}); };
      sdk.channels.push(ch); return ch;
    };
    x.sdkCalls.push(sdk); return sdk;
  };
  x.client = createRealtimeClient({dispatch: x.dispatch, createClient: x.createClient, tournamentId, markets,
    canOperate: () => x.allowed, now: () => x.now, setTimer: x.setTimer, clearTimer: x.clearTimer,
    onSubscribed: e => x.subscribed.push(e), onEvent: e => x.events.push(e), onDisconnected: e => x.disconnected.push(e), onResyncRequired: e => x.resync.push(e), ...x.clientOptions});
  return x;
}
function batch(revision = 1, previousRevision = 0, override = {}) {
  return {delivery: {model: 'best-effort-authoritative-resync', correlationId: 'opaque-do-not-publish', revision, previousRevision,
    sourceSequenceFrom: 30, sourceSequenceThrough: 30}, trades: [], bookDirty: [], marketSettled: [],
    books: [{exchangeId: 1066, asOf: {sequence: 30, at: new Date(startTime).toISOString()}, nextExpiryAt: null,
      bids: [{price: .5, quantity: 100}], asks: [{price: .505, quantity: 100}]}], ...override};
}

test('only four scoped private tournament market channels can be constructed', () => {
  for (const change of [{tournamentId: '00000000-0000-0000-0000-000000000000'}, {markets: [{marketId: '999', exchangeId: '1'}]}, {markets: [...markets, ...markets]},
    {markets: [{marketId: '377', exchangeId: '1066'}, {marketId: '354', exchangeId: '1066'}]}])
    assert.throws(() => createRealtimeClient({dispatch() {}, createClient() {}, canOperate: () => true, tournamentId, markets, ...change}), /SCOPE/);
});
test('one token write uses both unforgeable queue fences, authenticates before private subscription and never exposes credentials', async () => {
  const x = fixture(); await x.client.start();
  assert.equal(x.writes, 1); assert.equal(x.requests[0].endpoint, '/realtime/token'); assert.equal(x.requests[0].body, undefined);
  assert.equal(isRealtimeTokenDispatch(x.requests[0]), true); assert.equal(x.client.authorizes(x.requests[0]), false, 'spent fence cannot authorize another request');
  const sdk = x.sdkCalls[0]; assert.deepEqual(sdk.auth, [TOKEN + '1']);
  assert.equal(sdk.opts.auth.persistSession, false); assert.equal(sdk.opts.auth.autoRefreshToken, false); assert.equal(sdk.opts.realtime.reconnectAfterMs(1), 60000);
  assert.deepEqual(sdk.channels.map(c => c.topic), markets.map(m => `tournament:${tournamentId}:market:${m.marketId}`));
  assert.ok(sdk.channels.every(c => c.config.config.private && c.handlers.size === 4));
  assert.equal(x.subscribed.length, 2); assert.ok(x.resync.every(r => r.reason === 'INITIAL'));
  assert.equal(x.client.status().state, 'subscribed'); assert.equal(JSON.stringify(x.client.status()).includes('sensitive'), false);
  assert.equal(x.client.authorizes({method: 'POST', endpoint: '/realtime/token', beforeDispatch() {}, beforeNetwork() {}}), false);
  for (const spec of [{...x.requests[0], endpoint: '/orders'}, {...x.requests[0], method: 'GET'}, {...x.requests[0], body: {}}, {...x.requests[0], endpoint: '/realtime/token?x=1'}]) assert.equal(isRealtimeTokenDispatch(spec), false);
  await x.client.stop(); assert.equal(sdk.removed, 1); assert.equal(sdk.disconnected, 1);
});
test('installed Supabase SDK keeps the minted token through callback refresh without auth HTTP or socket creation', async () => {
  const x = fixture(); await x.client.start(); const options = x.sdkCalls[0].opts; let requests = 0, sockets = 0;
  class NeverConnect { constructor() { sockets++; throw Error('Unexpected socket'); } }
  const official = createOfficialClient('https://project.example.test', ANON, {...options,
    global: {fetch: async () => { requests++; throw Error('Unexpected HTTP'); }}, realtime: {...options.realtime, transport: NeverConnect}});
  await settle(); await official.realtime.setAuth(TOKEN + '1');
  await official.realtime.setAuth(); // What SDK connect/heartbeat callbacks do.
  assert.equal(official.realtime.accessTokenValue, TOKEN + '1'); assert.equal(await options.accessToken(), TOKEN + '1');
  assert.equal(requests, 0); assert.equal(sockets, 0); assert.equal(x.writes, 1);
  await x.client.stop(); assert.equal(await options.accessToken(), null, 'retired SDK epoch loses token access');
  official.realtime.disconnect(); await official.removeAllChannels();
});
test('STOP after admission prevents token network write and old channel callbacks after STOP are inert', async () => {
  const denied = fixture({afterAdmission() { denied.allowed = false; }});
  await assert.rejects(denied.client.start(), /REALTIME_STOPPED/); assert.equal(denied.writes, 0); assert.equal(denied.sdkCalls.length, 0);
  const x = fixture(); await x.client.start(); const ch = x.sdkCalls[0].channels[0]; await x.client.stop();
  ch.notify('SUBSCRIBED'); ch.push('market_batch', batch()); assert.equal(x.events.length, 0); assert.equal(x.client.status().state, 'stopped');
  await assert.rejects(x.client.start(), /REALTIME_STOPPED/);
});
test('receipt is captured once, deeply frozen, scoped and stripped of unrecognized and opaque content', async () => {
  const x = fixture(); await x.client.start(); const ch = x.sdkCalls[0].channels[0];
  const raw = batch(1, 0, {token: TOKEN}); ch.push('market_batch', raw); const e = x.events[0];
  x.now += 5000; raw.books[0].bids[0].quantity = 999;
  assert.equal(e.receivedAt, new Date(startTime).toISOString()); assert.equal(e.payload.books[0].bids[0].quantity, 100);
  assert.equal(e.payload.token, undefined); assert.equal(e.payload.delivery.correlationId, '');
  assert.ok(Object.isFrozen(e.payload.books[0].bids[0])); assert.throws(() => { e.receivedAt = 'changed'; }, TypeError);
  ch.push('market_settled', {marketId: '377', tournamentId, settledWith: 'arbitrary-private-text', at: new Date(x.now).toISOString()});
  assert.equal(x.events.at(-1).payload.settledWith, 'SETTLED'); assert.equal(JSON.stringify(x.events).includes('arbitrary-private-text'), false);
  const foreign = batch(2, 1); foreign.books[0].exchangeId = 1043; ch.push('market_batch', foreign);
  assert.equal(x.events.length, 2); assert.equal(x.resync.at(-1).reason, 'SCHEMA'); await x.client.stop();
});
test('topic gaps, compact batches and duplicate revisions preserve required recovery semantics', async () => {
  const x = fixture(); await x.client.start(); const ch = x.sdkCalls[0].channels[0];
  ch.push('market_batch', batch(8, 7)); ch.push('market_batch', batch(10, 9)); assert.equal(x.resync.at(-1).reason, 'GAP');
  const duplicate = batch(10, 9); duplicate.books[0].asOf.sequence = 99; ch.push('market_batch', duplicate);
  assert.equal(x.events.length, 3); assert.equal(x.events.at(-1).payload.books[0].asOf.sequence, 99);
  ch.push('market_batch', {delivery: batch(11, 10).delivery, resyncRequired: true});
  assert.equal(x.resync.at(-1).reason, 'RESYNC_REQUIRED'); assert.deepEqual(x.events.at(-1).payload.books, []);
  ch.push('book_dirty', {marketId: '377', exchangeId: '1066', tournamentId, at: new Date(x.now).toISOString()}); assert.equal(x.resync.at(-1).reason, 'BOOK_DIRTY');
  await x.client.stop();
});
test('SDK error and rejoin create a new receipt epoch and require fresh REST without minting another token', async () => {
  const x = fixture(); await x.client.start(); const ch = x.sdkCalls[0].channels[0], initial = x.subscribed[0].connectionId;
  ch.notify('CHANNEL_ERROR', Error(TOKEN)); ch.push('market_batch', batch()); assert.equal(x.events.length, 0);
  assert.equal(x.disconnected.at(-1).reason, 'SOCKET_ERROR'); ch.notify('SUBSCRIBED');
  assert.notEqual(x.subscribed.at(-1).connectionId, initial); assert.equal(x.subscribed.at(-1).reason, 'RECONNECT'); assert.equal(x.writes, 1);
  ch.push('market_batch', batch(200, 199)); assert.equal(x.events.length, 1); assert.equal(x.resync.at(-1).reason, 'RECONNECT'); await x.client.stop();
});
test('explicit reconnect reuses valid ephemeral token; refresh reacquires shared budget and creates clean subscription epochs', async () => {
  const x = fixture(); await x.client.start(); const old = x.sdkCalls[0].channels[0];
  await x.client.reconnect(); assert.equal(x.writes, 1); assert.equal(x.subscribed.at(-1).reason, 'RECONNECT');
  await x.client.refresh(); assert.equal(x.writes, 2); assert.equal(x.subscribed.at(-1).reason, 'REFRESH');
  old.push('market_batch', batch()); assert.equal(x.events.length, 0);
  assert.equal(x.sdkCalls[2].auth[0], TOKEN + '2'); await x.client.stop();
});
test('automatic refresh makes one attempt; unavailable token or provider errors never leak secrets or retry in a loop', async () => {
  const x = fixture(); await x.client.start(); x.tokenError = true; x.now += 10500000;
  await x.fire(10500000); await settle(); assert.equal(x.writes, 2); assert.equal(x.client.status().state, 'failed');
  assert.equal(x.client.status().lastCode, 'REALTIME_TOKEN_UNAVAILABLE'); assert.equal(JSON.stringify(x.client.status()).includes(TOKEN), false);
  assert.equal([...x.timers.values()].filter(t => t.ms === 10500000).length, 0); await x.client.stop();
  const failed = fixture({factoryError: true}); await assert.rejects(failed.client.start(), e => e.message === 'REALTIME_CONNECTION_FAILED');
  assert.equal(JSON.stringify(failed.client.status()).includes(TOKEN), false); await failed.client.stop();
});
test('subscription handshake must succeed, and STOP interrupts a hung auth handshake', async () => {
  const x = fixture({autoJoin: false}); const starting = x.client.start(); await settle(); assert.equal(x.subscribed.length, 0);
  const failure = assert.rejects(starting, /REALTIME_HANDSHAKE_TIMEOUT/); await x.fire(15000); await failure;
  assert.equal(x.client.status().state, 'failed'); await x.client.stop();
  const auth = fixture({authHang: true}); const pending = auth.client.start(); await settle(); const halted = assert.rejects(pending, /REALTIME_STOPPED/);
  await auth.client.stop(); await halted; assert.equal(auth.sdkCalls[0].channels.length, 0);
});
test('synchronous provider channel failure cleans pending handshakes and returns only a sanitized code', async () => {
  const x = fixture({channelError: true}); await assert.rejects(x.client.start(), e => e.message === 'REALTIME_CONNECTION_FAILED');
  assert.equal(x.sdkCalls[0].removed, 1); assert.equal(x.sdkCalls[0].disconnected, 1);
  assert.equal(JSON.stringify(x.client.status()).includes(TOKEN), false); await x.client.stop();
});
test('malformed, non-HTTPS and overlong token lifetimes fail closed before SDK receives credentials', async () => {
  for (const tokenOverride of [{supabaseUrl: 'http://project.example.test'}, {supabaseUrl: 'https://user:pass@project.example.test'}, {supabaseUrl: 'https://project.example.test/?token=x'},
    {expiresAt: new Date(startTime + 4 * 3600000).toISOString()}, {expiresAt: new Date(startTime + 1000).toISOString()}, {token: 'bad'}]) {
    const x = fixture({tokenOverride}); await assert.rejects(x.client.start(), /REALTIME_TOKEN_INVALID/); assert.equal(x.sdkCalls.length, 0); await x.client.stop();
  }
});

test('diagnostics count known, unknown, filtered callbacks and heartbeat health without arbitrary text', async () => {
  const x = fixture(); await x.client.start(); const sdk = x.sdkCalls[0], ch = sdk.channels[0];
  sdk.opts.realtime.logger('receive', TOKEN, {token: TOKEN}); sdk.opts.realtime.logger('error', TOKEN);
  sdk.heartbeat('sent', TOKEN); sdk.heartbeat('ok', TOKEN); sdk.heartbeat(TOKEN);
  ch.push(TOKEN, {token: TOKEN}); ch.push('market_batch', batch());
  ch.notify('CHANNEL_ERROR'); ch.push('market_batch', batch());
  let d = x.client.status().diagnostics;
  assert.equal(d.socketMessages, 1); assert.equal(d.sdkErrors, 1); assert.equal(d.broadcastCallbacks, 3);
  assert.equal(d.knownBroadcasts.market_batch, 2); assert.equal(d.unknownBroadcasts, 1); assert.equal(d.namedCallbacks, 2);
  assert.equal(d.droppedCallbacks.notJoined, 1); assert.equal(d.heartbeat.ok, 1); assert.equal(d.heartbeat.unknown, 1);
  assert.equal(d.connectionState, 'open'); assert.equal(JSON.stringify(d).includes(TOKEN), false);
  await x.client.stop(); ch.push('market_batch', batch());
  d = x.client.status().diagnostics; assert.equal(d.droppedCallbacks.retired, 1); assert.equal(x.events.length, 1);
});

test('installed SDK routes documented private broadcast envelopes to wildcard and named bindings offline', async () => {
  let official, requests = 0, sockets = 0;
  class NeverConnect { constructor() { sockets++; throw Error('Unexpected socket'); } }
  const x = fixture({clientOptions: {createClient: (url, key, options) => {
    official = createOfficialClient(url, key, {...options, global: {fetch: async () => { requests++; throw Error('Unexpected HTTP'); }},
      realtime: {...options.realtime, transport: NeverConnect}});
    const channel = official.channel.bind(official);
    official.channel = (...args) => { const c = channel(...args); c.subscribe = cb => { queueMicrotask(() => cb('SUBSCRIBED')); return c; };
      c.unsubscribe = async () => 'ok'; return c; };
    return official;
  }}});
  await x.client.start();
  const c = official.getChannels()[0], socket = official.realtime.socketAdapter.getSocket();
  const incoming = (event, payload) => socket.onConnMessage({data: JSON.stringify([null, null, c.topic, 'broadcast', {type: 'broadcast', event, payload}])});
  incoming('market_batch', batch()); incoming(TOKEN, {token: TOKEN});
  const d = x.client.status().diagnostics;
  assert.equal(d.socketMessages, 2); assert.equal(d.broadcastCallbacks, 2); assert.equal(d.namedCallbacks, 1);
  assert.equal(d.knownBroadcasts.market_batch, 1); assert.equal(d.unknownBroadcasts, 1); assert.equal(x.events.length, 1);
  assert.equal(JSON.stringify(x.client.status()).includes(TOKEN), false); assert.equal(requests, 0); assert.equal(sockets, 0);
  await x.client.stop();
});
