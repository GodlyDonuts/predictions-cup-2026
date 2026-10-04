import {numericId, parseEvent} from '../feed/contract.mjs';
import {realtimeScopes} from './realtime-scope.mjs';

const TOURNAMENT = 'bda92870-621e-47b0-bc3c-3602c5c26f55';
const CORE = new Set(['377', '354', '277', '305']);
const EVENTS = ['market_batch', 'book_dirty', 'market_settled'];
const CODES = new Set(['REALTIME_STOPPED', 'REALTIME_TOKEN_BUDGET_BLOCKED', 'REALTIME_TOKEN_UNAVAILABLE', 'REALTIME_TOKEN_INVALID', 'REALTIME_CLOCK_INVALID',
  'REALTIME_SDK_INVALID', 'REALTIME_JOIN_REFUSED', 'REALTIME_HANDSHAKE_TIMEOUT', 'REALTIME_SUBSCRIPTION_FAILED', 'REALTIME_SUBSCRIPTION_CLOSED', 'REALTIME_CALLBACK_FAILED']);
const freeze = value => { if (value && typeof value === 'object') { for (const v of Object.values(value)) freeze(v); Object.freeze(value); } return value; };
const error = code => Object.assign(new Error(code), {code});
const requireThat = (ok, code) => { if (!ok) throw error(code); };

// The sole account queue must additionally call instance.authorizes(spec).
// Shape alone is intentionally insufficient authority for a token write.
export function isRealtimeTokenDispatch(spec) {
  return spec?.method === 'POST' && spec.endpoint === '/realtime/token' && spec.body === undefined && spec.bodyJson === undefined
    && typeof spec.beforeDispatch === 'function' && spec.beforeNetwork === spec.beforeDispatch;
}

// No SDK import, API key, filesystem, account channel, order endpoint or logging.
// createClient is the official @supabase/supabase-js factory, injected by owner.
export function createRealtimeClient({dispatch, createClient, tournamentId, markets, channelMode = 'per_market', canOperate, canMintToken = () => true, signal,
  onSubscribed = () => {}, onEvent = () => {}, onDisconnected = () => {}, onResyncRequired = () => {},
  now = Date.now, setTimer = setTimeout, clearTimer = clearTimeout,
  handshakeTimeoutMs = 15000, cleanupTimeoutMs = 3000, refreshLeadMs = 300000, stopPollMs = 250} = {}) {
  requireThat(typeof dispatch === 'function' && typeof createClient === 'function' && typeof canOperate === 'function', 'REALTIME_DEPENDENCIES_REQUIRED');
  requireThat(tournamentId === TOURNAMENT && typeof canMintToken === 'function', 'REALTIME_SCOPE_INVALID');
  const {scopes, topic} = realtimeScopes({tournamentId, markets, channelMode});
  requireThat(channelMode === 'tournament' || scopes.every(s => CORE.has(s.marketId)), 'REALTIME_SCOPE_INVALID');
  const watchedEvents = channelMode === 'tournament' ? ['markets_batch', 'book_dirty', 'market_settled'] : EVENTS;
  for (const [v, max] of [[handshakeTimeoutMs, 60000], [cleanupTimeoutMs, 10000], [refreshLeadMs, 600000], [stopPollMs, 1000]])
    requireThat(Number.isSafeInteger(v) && v > 0 && v <= max, 'REALTIME_TIMING_INVALID');
  for (const fn of [onSubscribed, onEvent, onDisconnected, onResyncRequired, now, setTimer, clearTimer]) requireThat(typeof fn === 'function', 'REALTIME_CALLBACK_INVALID');

  // Credentials and SDK objects never appear in a returned object or callback.
  let credentials = null, sdk = null, sdkTokenSlot = null, channels = new Map(), generation = 0, connectionCounter = 0;
  let stopped = false, state = 'idle', lastCode = null, tail = Promise.resolve(), refreshTimer = null, stopTimer = null;
  let tokenRequests = 0, eventsReceived = 0, rejectedEvents = 0, callbackFailures = 0, ignoredMarkets = 0;
  const diagnostics = {socketMessages: 0, sdkErrors: 0, broadcastCallbacks: 0, namedCallbacks: 0,
    knownBroadcasts: {market_batch: 0, markets_batch: 0, book_dirty: 0, market_settled: 0}, unknownBroadcasts: 0,
    droppedCallbacks: {retired: 0, stopped: 0, notJoined: 0},
    heartbeat: {sent: 0, ok: 0, error: 0, timeout: 0, unknown: 0, lastStatus: null, lastAt: null}};
  const connectionState = () => { try { const v = sdk?.realtime?.connectionState?.();
    return ['connecting', 'open', 'closing', 'closed'].includes(v) ? v : 'unknown'; } catch { return 'unknown'; } };
  const fences = new WeakSet();
  const abortWaiters = new Set();
  const allowed = () => { try { return !stopped && !signal?.aborted && canOperate() === true; } catch { return false; } };
  const guard = () => { if (!allowed()) throw error('REALTIME_STOPPED'); };
  const tokenAllowed = () => { try { return canMintToken() === true; } catch { return false; } };
  const tokenGuard = () => { guard(); requireThat(tokenAllowed(), 'REALTIME_TOKEN_BUDGET_BLOCKED'); };
  const clock = () => { const t = now(); requireThat(Number.isSafeInteger(t) && t >= 0, 'REALTIME_CLOCK_INVALID'); return t; };
  const stamp = () => new Date(clock()).toISOString();
  const emit = (fn, envelope) => {
    try { const result = fn(freeze(structuredClone(envelope))); if (result?.then) result.catch(() => failCallback()); }
    catch { failCallback(); }
  };
  const failCallback = () => { callbackFailures++; lastCode = 'REALTIME_CALLBACK_FAILED'; void stop(); };
  const clear = name => { if (name !== null) clearTimer(name); };
  const arm = (fn, ms, unref = true) => { const timer = setTimer(fn, ms); if (unref) timer?.unref?.(); return timer; };
  const status = () => freeze({schemaVersion: 1, mode: 'realtime_market_capture_only', state, lastCode, tournamentId,
    tokenRequests, eventsReceived, rejectedEvents, callbackFailures, ignoredMarkets, channelMode,
    desiredChannels: channelMode === 'tournament' ? 1 : scopes.length,
    activeChannels: new Set([...channels.values()].filter(c => c.joined).map(c => c.channel)).size,
    diagnostics: {...structuredClone(diagnostics), connectionState: connectionState()},
    markets: scopes.map(s => { const c = channels.get(s.marketId); return {marketId: s.marketId, exchangeId: s.exchangeIds[0],
      subscribed: c?.joined === true, connectionId: c?.connectionId ?? null, revision: c?.revision ?? null}; }),
    accountChannels: 0, orderWrites: 0});
  const authorizes = spec => isRealtimeTokenDispatch(spec) && fences.has(spec.beforeDispatch) && allowed() && tokenAllowed();
  const resync = (record, reason, receivedAt) => emit(onResyncRequired, {marketId: record.scope.marketId, connectionId: record.connectionId, reason, receivedAt});
  async function bounded(promise) {
    let timer, rejectWait;
    try {
      const interrupted = new Promise((_, reject) => { rejectWait = reject; abortWaiters.add(reject); timer = arm(() => reject(error('REALTIME_HANDSHAKE_TIMEOUT')), handshakeTimeoutMs, false); });
      return await Promise.race([promise, interrupted]);
    } finally { clear(timer); abortWaiters.delete(rejectWait); }
  }

  async function closeActive(reason = 'DISCONNECTED') {
    const oldSdk = sdk, oldChannels = channels; sdk = null; channels = new Map(); generation++;
    if (sdkTokenSlot) sdkTokenSlot.token = null; sdkTokenSlot = null;
    clear(refreshTimer); refreshTimer = null;
    for (const c of oldChannels.values()) {
      clear(c.timer); c.reject?.(error('REALTIME_SUBSCRIPTION_CLOSED')); c.reject = null;
      if (c.joined) emit(onDisconnected, {marketId: c.scope.marketId, connectionId: c.connectionId, reason, receivedAt: stamp()});
      c.joined = false;
    }
    if (!oldSdk) return;
    let timer;
    try {
      const closing = Promise.resolve().then(() => oldSdk.removeAllChannels()).catch(() => {});
      try { oldSdk.realtime.disconnect(); } catch { /* No provider error text escapes. */ }
      await Promise.race([closing, new Promise(resolve => { timer = arm(resolve, cleanupTimeoutMs, false); })]);
    } finally { clear(timer); }
  }
  async function mint() {
    tokenGuard(); const fence = () => { tokenGuard(); return true; }; fences.add(fence);
    let response;
    try {
      tokenRequests++;
      response = await dispatch({method: 'POST', endpoint: '/realtime/token', beforeDispatch: fence, beforeNetwork: fence});
      guard();
    } catch { throw error(!allowed() ? 'REALTIME_STOPPED' : !tokenAllowed() ? 'REALTIME_TOKEN_BUDGET_BLOCKED' : 'REALTIME_TOKEN_UNAVAILABLE'); }
    finally { fences.delete(fence); }
    const d = response?.data;
    try {
      requireThat((response.status === undefined || response.status === 200) && d && typeof d.token === 'string' && d.token.length >= 16 && d.token.length <= 16384
        && typeof d.anonKey === 'string' && d.anonKey.length >= 8 && d.anonKey.length <= 16384, 'REALTIME_TOKEN_INVALID');
      const url = new URL(d.supabaseUrl), expires = Date.parse(d.expiresAt), t = clock();
      requireThat(url.protocol === 'https:' && !url.username && !url.password && !url.search && !url.hash && url.pathname === '/'
        && Number.isFinite(expires) && expires > t + refreshLeadMs + 1000 && expires <= t + 10860000, 'REALTIME_TOKEN_INVALID');
      // channels.user is deliberately ignored: market-only authority is fixed.
      return {token: d.token, anonKey: d.anonKey, url: url.origin, expires};
    } catch { throw error('REALTIME_TOKEN_INVALID'); }
  }
  function acceptEvent(record, event, raw, receivedAt) {
    let payload;
    try {
      payload = parseEvent(event, raw, record.scope).payload;
      // Keep only contract fields; opaque diagnostics are never forwarded.
      if (event === 'market_batch') {
        payload.delivery.correlationId = '';
        payload.marketSettled = payload.marketSettled.map(v => ({...v, settledWith: v.refund ? 'REFUND' : 'SETTLED'}));
      } else if (event === 'market_settled') payload.settledWith = payload.refund ? 'REFUND' : 'SETTLED';
    } catch { rejectedEvents++; resync(record, 'SCHEMA', receivedAt); return; }
    eventsReceived++;
    if (event === 'market_batch') {
      const previous = record.revision, d = payload.delivery;
      record.revision = Math.max(previous ?? 0, d.revision);
      if (payload.resyncRequired) resync(record, 'RESYNC_REQUIRED', receivedAt);
      else if (previous !== null && d.revision > previous && d.previousRevision > previous) resync(record, 'GAP', receivedAt);
      else if (payload.trades.some(t => t.sequence === null)) resync(record, 'UNSEQUENCED_TRADE', receivedAt);
      if (payload.marketSettled.length) resync(record, 'SETTLEMENT', receivedAt);
    } else resync(record, event === 'book_dirty' ? 'BOOK_DIRTY' : 'SETTLEMENT', receivedAt);
    // Duplicate revisions still carry independently newer books.
    emit(onEvent, {marketId: record.scope.marketId, connectionId: record.connectionId, event, payload, receivedAt});
  }
  function wire(record, client, localGeneration, reason, members = [record]) {
    let resolve;
    const ready = new Promise((yes, no) => { resolve = yes; record.reject = no; });
    // If provider channel setup throws synchronously, closeActive rejects this
    // waiter before Promise.all can receive it. Keep that cleanup rejection
    // handled; the original setup failure still rejects establish().
    ready.catch(() => {});
    const current = () => localGeneration === generation && sdk === client && allowed();
    record.timer = arm(() => {
      if (!current() || record.joined) return;
      lastCode = 'REALTIME_HANDSHAKE_TIMEOUT'; record.reject?.(error(lastCode)); record.reject = null;
    }, handshakeTimeoutMs, false);
    let channel = client.channel(topic ?? record.scope.topic, {config: {private: true}});
    // Diagnostics only: never forward or retain a wildcard event's name/payload.
    // Count before admission guards so a retired or unjoined callback is visible.
    channel = channel.on('broadcast', {event: '*'}, message => {
      diagnostics.broadcastCallbacks++;
      const name = typeof message?.event === 'string' ? message.event.toLowerCase() : null;
      if (watchedEvents.includes(name)) diagnostics.knownBroadcasts[name]++; else diagnostics.unknownBroadcasts++;
      if (localGeneration !== generation || sdk !== client) diagnostics.droppedCallbacks.retired++;
      else if (!allowed()) diagnostics.droppedCallbacks.stopped++;
      else if (!record.joined) diagnostics.droppedCallbacks.notJoined++;
    });
    for (const event of watchedEvents) channel = channel.on('broadcast', {event}, message => {
      diagnostics.namedCallbacks++;
      if (!current() || !record.joined) return;
      const receivedAt = stamp();
      if (channelMode !== 'tournament') return acceptEvent(record, event, message?.payload, receivedAt);
      if (event !== 'markets_batch') {
        let member;
        try { member = channels.get(numericId(message?.payload?.marketId)); } catch { /* Invalid identity never reaches capture. */ }
        if (!member) { ignoredMarkets++; return; }
        return acceptEvent(member, event, message.payload, receivedAt);
      }
      let entries;
      try {
        const raw = message?.payload;
        requireThat(raw && typeof raw === 'object' && !Array.isArray(raw) && Array.isArray(raw.markets) && raw.markets.length <= 300, 'REALTIME_ENVELOPE_INVALID');
        entries = raw.markets.map(v => ({marketId: numericId(v?.marketId), batch: v?.batch}));
        requireThat(new Set(entries.map(v => v.marketId)).size === entries.length, 'REALTIME_ENVELOPE_INVALID');
      } catch { rejectedEvents++; for (const member of members) resync(member, 'SCHEMA', receivedAt); return; }
      for (const entry of entries) {
        if (!current() || !record.joined) break;
        const member = channels.get(entry.marketId);
        if (!member) { ignoredMarkets++; continue; }
        acceptEvent(member, 'market_batch', entry.batch, receivedAt);
      }
    });
    for (const member of members) member.channel = channel;
    channel.subscribe((statusCode, providerError) => {
      if (!current()) return;
      const receivedAt = stamp();
      if (statusCode === 'SUBSCRIBED') {
        clear(record.timer); record.timer = null;
        for (const member of members) {
          if (!current()) return;
          const why = member.joins++ ? 'RECONNECT' : reason;
          member.joined = true; member.revision = null; member.connectionId = 'realtime-' + (++connectionCounter) + '-' + member.scope.marketId;
          emit(onSubscribed, {marketId: member.scope.marketId, connectionId: member.connectionId, reason: why, receivedAt});
          if (!current()) return;
          resync(member, why, receivedAt);
        }
        if (!current()) return;
        record.reject = null; resolve();
        if ([...channels.values()].every(c => c.joined)) state = 'subscribed';
      } else if (['CHANNEL_ERROR', 'TIMED_OUT', 'CLOSED'].includes(statusCode)) {
        // SDK channel rejoin retries are distinct from socket reconnect pacing.
        // A refused join never retries until a fresh token is explicitly applied.
        if (statusCode === 'CHANNEL_ERROR' && typeof providerError?.message === 'string'
          && /Unauthorized|Too many channels/i.test(providerError.message)) {
          lastCode = 'REALTIME_JOIN_REFUSED'; state = 'refused';
          record.reject?.(error(lastCode)); record.reject = null;
          void closeActive('SOCKET_ERROR'); return;
        }
        const why = statusCode === 'CLOSED' ? 'DISCONNECTED' : 'SOCKET_ERROR';
        for (const member of members) {
          if (member.joined) emit(onDisconnected, {marketId: member.scope.marketId, connectionId: member.connectionId, reason: why, receivedAt});
          member.joined = false; resync(member, why, receivedAt);
        }
        state = 'reconnecting';
        if (!record.joins) { record.reject?.(error('REALTIME_SUBSCRIPTION_FAILED')); record.reject = null; }
      }
    });
    return ready;
  }
  function monitorStop() {
    clear(stopTimer);
    stopTimer = arm(() => { if (!allowed()) void stop(); else monitorStop(); }, stopPollMs);
  }
  async function establish(reason, renew) {
    guard(); state = 'connecting'; lastCode = null; monitorStop(); await closeActive(); guard();
    try {
      if (renew || !credentials || credentials.expires - clock() <= refreshLeadMs) credentials = await mint();
      guard();
      const memory = new Map(), sessionStorage = {get length() { return memory.size; }, key: i => [...memory.keys()][i] ?? null,
        getItem: key => memory.get(key) ?? null, setItem: (key, value) => memory.set(key, value), removeItem: key => memory.delete(key), clear: () => memory.clear()};
      // Supabase's accessToken callback is authoritative again on connect and
      // heartbeats, even after setAuth(token). Pin it to this private SDK epoch;
      // it must never fall back to the anon key or mint tokens on heartbeats.
      const tokenSlot = {token: credentials.token}; sdkTokenSlot = tokenSlot;
      const localGeneration = generation;
      sdk = createClient(credentials.url, credentials.anonKey, {accessToken: async () => tokenSlot.token,
        auth: {persistSession: false, autoRefreshToken: false, detectSessionInUrl: false},
        realtime: {logger: kind => {
          // The SDK supplies sensitive text and payload as later arguments;
          // this callback deliberately does not accept or inspect either.
          if (generation !== localGeneration || !allowed()) return;
          if (kind === 'receive') diagnostics.socketMessages++;
          else if (kind === 'error') diagnostics.sdkErrors++;
        }, reconnectAfterMs: () => 60000, sessionStorage}});
      requireThat(sdk && typeof sdk.channel === 'function' && typeof sdk.removeAllChannels === 'function'
        && typeof sdk.realtime?.setAuth === 'function' && typeof sdk.realtime?.disconnect === 'function', 'REALTIME_SDK_INVALID');
      const client = sdk;
      client.realtime.onHeartbeat?.(value => {
        if (generation !== localGeneration || sdk !== client || !allowed()) return;
        const key = ['sent', 'ok', 'error', 'timeout'].includes(value) ? value : 'unknown';
        diagnostics.heartbeat[key]++; diagnostics.heartbeat.lastStatus = key; diagnostics.heartbeat.lastAt = stamp();
      });
      await bounded(Promise.resolve().then(() => client.realtime.setAuth(credentials.token))); guard();
      requireThat(client === sdk && generation === localGeneration, 'REALTIME_STOPPED');
      channels = new Map(scopes.map(scope => [scope.marketId, {scope, joined: false, joins: 0, connectionId: null, revision: null, timer: null, reject: null}]));
      const members = [...channels.values()];
      await (channelMode === 'tournament' ? wire(members[0], client, localGeneration, reason, members)
        : Promise.all(members.map(c => wire(c, client, localGeneration, reason)))); guard();
      state = 'subscribed';
      requireThat(credentials.expires - clock() > refreshLeadMs, 'REALTIME_TOKEN_INVALID');
      refreshTimer = arm(() => { refreshTimer = null; void refresh().catch(() => {}); }, credentials.expires - clock() - refreshLeadMs);
      monitorStop(); return status();
    } catch (e) {
      lastCode = CODES.has(e?.code) ? e.code : 'REALTIME_CONNECTION_FAILED';
      state = !allowed() ? 'stopped' : lastCode === 'REALTIME_TOKEN_BUDGET_BLOCKED' ? 'blocked' : lastCode === 'REALTIME_JOIN_REFUSED' ? 'refused' : 'failed'; await closeActive(); credentials = null;
      throw error(lastCode);
    }
  }
  const queue = fn => { const job = tail.then(fn); tail = job.catch(() => {}); return job; };
  function start() { return queue(() => { guard(); requireThat(state !== 'refused', 'REALTIME_JOIN_REFUSED'); return sdk && state === 'subscribed' ? status() : establish('INITIAL', true); }); }
  function refresh() { return queue(() => establish('REFRESH', true)); }
  function reconnect() { return queue(() => { requireThat(state !== 'refused', 'REALTIME_JOIN_REFUSED'); return establish('RECONNECT', false); }); }
  async function stop() {
    stopped = true; state = 'stopped'; clear(stopTimer); stopTimer = null; clear(refreshTimer); refreshTimer = null;
    for (const reject of abortWaiters) reject(error('REALTIME_STOPPED')); abortWaiters.clear();
    signal?.removeEventListener('abort', onAbort); credentials = null; await closeActive(); return status();
  }
  const onAbort = () => { void stop(); };
  signal?.addEventListener('abort', onAbort, {once: true});
  return Object.freeze({start, refresh, reconnect, stop, status, authorizes});
}
