import {RealtimeBooks} from './realtime-books.mjs';
import {createRealtimeClient} from './realtime-client.mjs';

// One host, one existing serialized account dispatcher. Socket data is captured
// separately and never passed to the official order controller by this module.
export function createRealtimeHost({dispatch, createClient, scope, canOperate, canMintToken = () => true, signal, capture,
  now = Date.now, clientFactory = createRealtimeClient, silentRecoveryIntervalMs = 120000} = {}) {
  if (typeof dispatch !== 'function' || typeof canOperate !== 'function' || typeof canMintToken !== 'function' || !capture?.append) throw Error('REALTIME_HOST_DEPENDENCIES');
  if (!Number.isSafeInteger(silentRecoveryIntervalMs) || silentRecoveryIntervalMs < 60000 || silentRecoveryIntervalMs > 120000) throw Error('REALTIME_SILENT_RECOVERY_POLICY');
  const books = new RealtimeBooks(scope, {now, capture});
  const fences = new WeakMap();
  let closed = false, servicing = false, cursor = 0, retryAt = 0;
  let recoveryAttempts = 0, recoverySuccesses = 0, lastRecoveryAt = null, lastCode = null;
  let quietRecoveryDeferrals = 0;
  const recovered = new Map();
  const allowed = () => !closed && !signal?.aborted && canOperate() === true;
  const tokenAllowed = () => { try { return canMintToken() === true; } catch { return false; } };
  const client = clientFactory({dispatch, createClient, ...scope, canOperate: allowed, canMintToken: tokenAllowed, signal, now,
    onSubscribed: event => books.subscribed(event), onEvent: event => books.ingest(event),
    onDisconnected: event => books.disconnected(event), onResyncRequired: event => books.invalidate(event)});
  function marketStatus() {
    const markets = books.status().markets;
    for (const market of markets) {
      const prior = recovered.get(market.marketId);
      if (market.ready && prior?.connectionId === market.connectionId && now() - prior.at >= silentRecoveryIntervalMs)
        books.invalidate({...market, reason:'PERIODIC'});
    }
    return books.status().markets;
  }
  function authorizes(spec) {
    if (!allowed()) return false;
    if (client.authorizes(spec)) return true;
    return spec?.method === 'GET' && spec.body === undefined && spec.bodyJson === undefined
      && spec.beforeNetwork === spec.beforeDispatch && fences.has(spec.beforeDispatch)
      && fences.get(spec.beforeDispatch) === spec.endpoint;
  }
  function status() {
    const blocked = !closed && allowed() && !tokenAllowed();
    const transport=blocked ? {...client.status(), state:'blocked', lastCode:'REALTIME_TOKEN_BUDGET_BLOCKED'} : client.status();
    return {schemaVersion: 1, mode: 'read_only_market_capture', executionAuthorized: false,
      at: new Date(now()).toISOString(), transport, marketStreamObserved:transport.eventsReceived>0,
      dataEvidence:blocked?'blocked_shared_read_only_budget':transport.eventsReceived>0?'market_frames_observed':'awaiting_first_market_frame',recoveryAttempts,recoverySuccesses,
      silentRecoveryIntervalMs,quietRecoveryDeferrals,
      lastRecoveryAt, lastCode:blocked?'REALTIME_TOKEN_BUDGET_BLOCKED':lastCode, retryAt: blocked ? null : retryAt ? new Date(retryAt).toISOString() : null,
      markets: marketStatus().map(({books: rows, ...market}) => ({...market, availableBooks: rows.length}))};
  }
  async function service() {
    if (!allowed() || servicing) return false;
    servicing = true;
    try {
      // Budget v3 forbids every POST, including ephemeral socket auth. Do not
      // turn an intentional local policy block into recurring provider retries.
      if (!tokenAllowed()) {
        lastCode = 'REALTIME_TOKEN_BUDGET_BLOCKED'; retryAt = 0;
        if (!['idle', 'stopped', 'blocked'].includes(client.status().state)) await client.stop();
        return false;
      }
      const state = client.status().state;
      if (['idle', 'failed', 'reconnecting', 'blocked'].includes(state)) {
        if (now() < retryAt) return false;
        // Retry at most once a minute, independent of the provider's socket
        // retries, while the shared dispatcher also enforces persistent backoff.
        retryAt = now() + 60000;
        try { await (state === 'idle' ? client.start() : client.reconnect()); }
        catch { lastCode = 'REALTIME_CONNECTION_UNAVAILABLE'; return false; }
      }
      if (!allowed()) return false;
      if (client.status().state === 'refused') { lastCode = 'REALTIME_JOIN_REFUSED'; retryAt = 0; return false; }
      if (client.status().state !== 'subscribed') return false;
      const markets = marketStatus();
      for (let i = 0; i < markets.length; i++) {
        const index = (cursor + i) % markets.length, market = markets[index];
        if (!market.needsSnapshot || market.reasons.includes('SETTLEMENT')) continue;
        // A subscribed but silent socket cannot refresh book evidence. Keep
        // stale readiness blocked, but avoid repeatedly polling the same book
        // faster than the documented periodic resync interval. Initial joins,
        // connection changes and explicit gap/error reasons still recover now.
        const prior = recovered.get(market.marketId);
        if (!(client.status().eventsReceived > 0) && market.reasons.length > 0
          && market.reasons.every(reason => reason === 'STALE')
          && prior?.connectionId === market.connectionId
          && now() - prior.at < silentRecoveryIntervalMs) {
          quietRecoveryDeferrals++;
          continue;
        }
        cursor = (index + 1) % markets.length;
        const ticket = books.beginSnapshot(market);
        const fence = () => {
          if (!allowed() || books.view(market.marketId).connectionId !== ticket.connectionId
            || !client.status().markets.find(m => m.marketId === market.marketId)?.subscribed) throw Error('REALTIME_RECOVERY_STOPPED');
        };
        fences.set(fence, ticket.endpoint); recoveryAttempts++;
        try {
          const response = await dispatch({method: 'GET', endpoint: ticket.endpoint, beforeDispatch: fence, beforeNetwork: fence});
          fence();
          if (books.completeSnapshot(ticket, response)) {
            recoverySuccesses++; lastRecoveryAt = response.receivedAt; lastCode = null;
            recovered.set(market.marketId, {connectionId: ticket.connectionId, at: now()});
          } else lastCode = 'REALTIME_RECOVERY_REJECTED';
        } catch {
          books.snapshotFailed(ticket); lastCode = 'REALTIME_RECOVERY_UNAVAILABLE';
        } finally { fences.delete(fence); }
        return true; // One recovery slot per collector cycle, never a burst.
      }
      return false;
    } finally { servicing = false; }
  }
  async function stop() {
    if (closed) return;
    closed = true; await client.stop(); books.stop();
  }
  return Object.freeze({service, status, authorizes, stop});
}
