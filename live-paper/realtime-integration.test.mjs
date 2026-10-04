import test from 'node:test';
import assert from 'node:assert/strict';
import {createRealtimeHost} from './realtime-host.mjs';
import {ReadClient} from './runtime.mjs';
import {OrderTransport} from '../execution/transport.mjs';

const tournamentId = 'bda92870-621e-47b0-bc3c-3602c5c26f55';
const scope = {tournamentId, markets: [{marketId: '377', exchangeId: '1066'}, {marketId: '354', exchangeId: '1043'}]};
const budget = {schemaVersion: 1, ownerId: 'Codex', enabled: true, intervalMs: 3000, soleAccountClient: true, formerWriterStopCommentId: 5947535773};
const secret = 'SYNTHETIC-PRIVATE-TOKEN-DO-NOT-CAPTURE';
function fixture(t, {stopAtAdmission, rateLimitToken = false, tournamentMode = false} = {}) {
  let time = Date.parse('2026-10-02T12:00:00Z'), active = true, host, admissions = 0;
  const calls = [], records = [], sessions = [], server = new Map(scope.markets.map(m => [m.marketId, {sequence: 1, bid: .5, ask: .505}]));
  const at = () => new Date(time).toISOString();
  const order = new OrderTransport({scope: {...scope, tournamentSlug: 'midterm-elections', exchanges: scope.markets}, dispatch: spec => api.dispatch(spec),
    canWrite: () => false, ownsOrder: () => false, validateLease: () => false, validatePermit: () => false, now: () => time});
  const api = new ReadClient('SYNTHETIC-API-KEY', {networkBudget: budget, now: () => time, sleepFn: async ms => { time += ms; },
    // This is the real runner ordering: OrderTransport throws for token routes.
    authorizeDispatch: spec => host?.authorizes(spec) === true || order.authorizeRequest(spec) === true,
    onAdmission: () => { if (++admissions === stopAtAdmission) active = false; },
    fetchFn: async (url, options) => {
      const endpoint = url.replace('https://sig.thesuper.market/api/v1', ''); calls.push({endpoint, method: options.method, at: time});
      if (endpoint === '/realtime/token') {
        if (rateLimitToken) return {ok: false, status: 429, headers: new Headers({'Retry-After': '90'}), json: async () => ({error: {message: secret}})};
        return {ok: true, status: 200, json: async () => ({token: secret, anonKey: 'SYNTHETIC-ANON-KEY', supabaseUrl: 'https://fixture.example.test', expiresAt: new Date(time + 10800000).toISOString(), channels: {user: 'user:DO-NOT-CAPTURE'}})};
      }
      if (endpoint === '/tournaments/midterm-elections') return {ok: true, status: 200, json: async () => ({})};
      const id = /^\/markets\/(\d+)\/orderbook/.exec(endpoint)?.[1], market = scope.markets.find(m => m.marketId === id), book = server.get(id);
      assert.ok(market); assert.equal(options.method, 'GET'); assert.ok(endpoint.endsWith('&depth=200'));
      return {ok: true, status: 200, json: async () => ({contexts: [{type: 'tournament', tournament: {id: tournamentId}, orderbook: {exchanges: [{exchangeId: market.exchangeId, option: 'YES', asOf: {sequence: book.sequence, at: at()}, bids: [{price: book.bid, quantity: 100}], asks: [{price: book.ask, quantity: 100}]}]}}]})};
    }});
  const createClient = (_url, _key, options) => {
    const session = {channels: [], authenticated: false, removed: false, disconnected: false}; sessions.push(session);
    assert.equal(options.auth.persistSession, false);
    session.realtime = {async setAuth(token) { assert.equal(token, secret); session.authenticated = true; }, disconnect() { session.disconnected = true; }};
    session.removeAllChannels = async () => { session.removed = true; };
    session.channel = (topic, configuration) => {
      assert.equal(session.authenticated, true); assert.equal(configuration.config.private, true);
      assert(tournamentMode?topic===`tournament:${tournamentId}:markets`:topic.startsWith('tournament:' + tournamentId + ':market:'));
      const channel = {topic, handlers: new Map()};
      channel.on = (kind, filter, callback) => { assert.equal(kind, 'broadcast'); channel.handlers.set(filter.event, callback); return channel; };
      channel.subscribe = callback => { channel.notify = callback; queueMicrotask(() => callback('SUBSCRIBED')); return channel; };
      channel.push = (event, payload) => channel.handlers.get(event)?.({payload}); session.channels.push(channel); return channel;
    };
    return session;
  };
  host = createRealtimeHost({dispatch: spec => api.dispatch(spec), createClient, scope:{...scope,channelMode:tournamentMode?'tournament':'per_market'}, canOperate: () => active, now: () => time, capture: {append: record => records.push(structuredClone(record))}});
  t.after(async () => { await host.stop(); api.close(); });
  function batch(marketId, revision, previousRevision, sequence) {
    const m = scope.markets.find(x => x.marketId === marketId); server.get(marketId).sequence = sequence;
    return {delivery: {model: 'best-effort-authoritative-resync', correlationId: secret, revision, previousRevision, sourceSequenceFrom: sequence, sourceSequenceThrough: sequence},
      books: [{exchangeId: Number(m.exchangeId), asOf: {sequence, at: at()}, nextExpiryAt: null, bids: [{price: .5, quantity: 100}], asks: [{price: .505, quantity: 100}]}], trades: [], bookDirty: [], marketSettled: [], token: secret};
  }
  return {api, order, host, calls, records, sessions, batch, advance: ms => { time += ms; }, stop: () => { active = false; }};
}

test('real host + client + ReadClient serialize token/recovery behind collector traffic with actual OrderTransport authorization', async t => {
  const f = fixture(t);
  assert.throws(() => f.order.authorizeRequest({method: 'POST', endpoint: '/realtime/token'}));
  await Promise.all([...Array.from({length: 100}, () => f.api.request('/tournaments/midterm-elections')), f.host.service()]);
  assert.equal(f.calls.length, 102); assert.equal(f.calls[100].endpoint, '/realtime/token'); assert.ok(f.calls[101].endpoint.includes('depth=200'));
  for (let i = 1; i < f.calls.length; i++) assert.ok(f.calls[i].at - f.calls[i - 1].at >= 3000);
  assert.equal(f.calls.filter(call => call.method === 'POST').length, 1);
  assert.equal(f.host.status().recoverySuccesses, 1); assert.equal(f.host.status().markets.filter(m => m.ready).length, 1);
  await f.host.service(); assert.equal(f.host.status().recoverySuccesses, 2);
  assert.equal(JSON.stringify(f.host.status()).includes(secret), false); assert.equal(JSON.stringify(f.records).includes(secret), false);
  assert.equal(f.calls.some(call => call.endpoint === '/orders'), false);
});

test('actual SDK callbacks survive client sanitization, trigger gap recovery and require REST again on rejoin', async t => {
  const f = fixture(t); await f.host.service(); await f.host.service();
  const channel = f.sessions[0].channels[0];
  f.advance(1); channel.push('market_batch', f.batch('377', 1, 0, 2));
  assert.equal(f.host.status().markets[0].ready, true);
  assert.ok(f.records.some(row => row.kind === 'event' && row.payload.books[0].asOf.sequence === 2));
  channel.push('market_batch', f.batch('377', 3, 2, 3));
  assert.equal(f.host.status().markets[0].ready, false);
  await f.host.service(); assert.equal(f.host.status().markets[0].ready, true);
  const before = f.host.status().markets[0].connectionId;
  channel.notify('CHANNEL_ERROR'); assert.equal(f.host.status().markets[0].ready, false);
  channel.notify('SUBSCRIBED'); assert.notEqual(f.host.status().markets[0].connectionId, before);
  assert.equal(f.host.status().markets[0].ready, false); await f.host.service();
  assert.equal(f.host.status().markets[0].ready, true);
  assert.equal(f.calls.filter(call => call.endpoint === '/realtime/token').length, 1);
  assert.equal(JSON.stringify(f.records).includes(secret), false);
});

test('STOP inside durable token admission prevents HTTP and SDK creation through the complete stack', async t => {
  const f = fixture(t, {stopAtAdmission: 1});
  assert.equal(await f.host.service(), false); assert.deepEqual(f.calls, []); assert.deepEqual(f.sessions, []);
  assert.equal(f.host.status().executionAuthorized, false);
});

test('STOP inside recovery admission permits only the already completed token write', async t => {
  // Faster mode publishes both pre-network and completion admission reservations.
  const f = fixture(t, {stopAtAdmission: 3}); await f.host.service();
  assert.deepEqual(f.calls.map(call => call.endpoint), ['/realtime/token']);
  assert.equal(f.host.status().recoverySuccesses, 0); assert.equal(f.records.some(row => row.kind === 'snapshot'), false);
});

test('token429 enforces the same persistent90second cooldown on subsequent ordinary collector reads', async t => {
  const f = fixture(t, {rateLimitToken: true}); assert.equal(await f.host.service(), false);
  await f.api.request('/tournaments/midterm-elections');
  assert.equal(f.calls[1].at - f.calls[0].at, 90000);
  assert.equal(JSON.stringify(f.host.status()).includes(secret), false); assert.equal(f.sessions.length, 0);
});

test('tournament envelope runs through client, host, validated books and one shared REST recovery per service',async t=>{
  const f=fixture(t,{tournamentMode:true});await f.host.service();
  assert.equal(f.sessions[0].channels.length,1);assert.equal(f.host.status().recoverySuccesses,1);
  const channel=f.sessions[0].channels[0];f.advance(1);
  channel.push('markets_batch',{markets:[{marketId:'377',batch:f.batch('377',1,0,2)},{marketId:'354',batch:f.batch('354',40,39,2)}]});
  assert.equal(f.host.status().markets[0].ready,true);assert.equal(f.host.status().markets[1].ready,false,'push cannot replace initial REST');
  await f.host.service();assert.equal(f.host.status().recoverySuccesses,2);
  assert(f.host.status().markets.every(m=>m.ready));
  channel.push('markets_batch',{markets:[{marketId:'377',batch:f.batch('377',3,2,3)},{marketId:'354',batch:f.batch('354',41,40,3)}]});
  assert.equal(f.host.status().markets[0].ready,false);assert.equal(f.host.status().markets[1].ready,true);
  await f.host.service();assert.equal(f.host.status().recoverySuccesses,3);
  assert.equal(f.calls.filter(c=>c.endpoint==='/realtime/token').length,1);
  assert.equal(f.host.status().transport.activeChannels,1);assert.equal(JSON.stringify(f.records).includes(secret),false);
});

test('fresh continuous broadcasts cannot postpone mandatory periodic REST reconciliation',async t=>{
  const f=fixture(t,{tournamentMode:true});await f.host.service();await f.host.service();
  const channel=f.sessions[0].channels[0];
  for(let i=1;i<=8;i++){
    f.advance(15000);
    channel.push('markets_batch',{markets:[{marketId:'377',batch:f.batch('377',i,i-1,i+1)},{marketId:'354',batch:f.batch('354',i,i-1,i+1)}]});
  }
  const due=f.host.status();assert(due.markets.every(m=>!m.ready&&m.reasons.includes('PERIODIC')));
  const prior=f.calls.length;await f.host.service();assert.equal(f.calls.length,prior+1);
  assert.equal(f.host.status().markets.filter(m=>m.ready).length,1);await f.host.service();
  assert(f.host.status().markets.every(m=>m.ready));assert(f.records.some(r=>r.kind==='invalidation'&&r.reason==='PERIODIC'));
});
