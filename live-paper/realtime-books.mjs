import {MarketCollector} from '../feed/collector.mjs';
import {config, parseEvent, parseSnapshot, snapshotPath, instantNs, timestamp, requireThat, numericId, compareVersion} from '../feed/contract.mjs';
import {price} from '../paper/fixed.mjs';
import {realtimeScopes} from './realtime-scope.mjs';
const invalidationReasons = new Set(['INITIAL', 'RECONNECT', 'REFRESH', 'SOCKET_ERROR', 'DISCONNECTED', 'GAP', 'RESYNC_REQUIRED', 'UNSEQUENCED_TRADE', 'SCHEMA', 'STALE', 'PERIODIC', 'EXPIRY', 'MISSING_BOOK', 'SETTLEMENT', 'BOOK_DIRTY', 'VERSION_CONFLICT', 'RECOVERY_FAILED']);

// Market-only, in-memory bridge. The host owns sockets and the shared REST queue.
// Readiness permits capture only: account reconciliation and execution stay absent.
export class RealtimeBooks {
  #markets = new Map();
  #now;
  #lastNow = 0;
  #staleMs;
  #skewMs;
  #snapshotMs;
  constructor({tournamentId, markets, channelMode = 'per_market'}, {now = Date.now, staleMs = 20000, maxClockSkewMs = 1000, maxSnapshotMs = 20000, capture = null} = {}) {
    realtimeScopes({tournamentId, markets, channelMode});
    for (const n of [staleMs, maxSnapshotMs]) requireThat(Number.isSafeInteger(n) && n > 0 && n <= 60000, 'INVALID_FRESHNESS_POLICY');
    requireThat(Number.isSafeInteger(maxClockSkewMs) && maxClockSkewMs >= 0 && maxClockSkewMs <= 1000 && typeof now === 'function', 'INVALID_CLOCK_POLICY');
    requireThat(capture === null || typeof capture?.append === 'function', 'INVALID_CAPTURE_SINK');
    this.#now = now; this.#staleMs = staleMs; this.#skewMs = maxClockSkewMs; this.#snapshotMs = maxSnapshotMs;
    const exchangeIds = new Set();
    for (const market of markets) {
      const scope = config({tournamentId, marketId: market.marketId, exchangeIds: [market.exchangeId]});
      requireThat(!this.#markets.has(scope.marketId) && !exchangeIds.has(scope.exchangeIds[0]), 'DUPLICATE_MARKET_SCOPE');
      exchangeIds.add(scope.exchangeIds[0]);
      const snapshotReceipt = {requestStartedAt: null};
      const normalizedCapture = capture === null ? null : {append: record => capture.append(record.kind === 'snapshot' && snapshotReceipt.requestStartedAt !== null
        ? {...record, requestStartedAt: snapshotReceipt.requestStartedAt} : record)};
      const collector = new MarketCollector(scope, {now: () => this.#clock(), staleMs, capture: normalizedCapture});
      this.#markets.set(scope.marketId, {scope, collector, snapshotReceipt, connectionId: null, connections: new Set(), subscribedNs: null, lastReceiptNs: null, ticket: null, knownExpiry: null});
    }
  }
  #clock() {
    const now = this.#now();
    if (!Number.isSafeInteger(now) || now < this.#lastNow) {
      // Do not call record()/invalidate() with an invalid clock: a capture sink
      // asks for the clock again. Hide readiness directly, then fail closed.
      for (const {collector} of this.#markets.values()) if (!['disabled', 'stopped'].includes(collector.state)) {
        collector.generation++; collector.reasons.add('SCHEMA'); if (collector.state !== 'disconnected') collector.state = 'needs_resync';
      }
      requireThat(false, 'CLOCK_ROLLBACK');
    }
    this.#lastNow = now; return now;
  }
  #entry(marketId) {
    const entry = this.#markets.get(numericId(marketId)); requireThat(entry, 'UNKNOWN_MARKET'); return entry;
  }
  #connection(id) { requireThat(typeof id === 'string' && /^[A-Za-z0-9_-]{1,80}$/.test(id), 'INVALID_CONNECTION_ID'); return id; }
  #expire(entry) {
    if (entry.knownExpiry?.at !== null && entry.knownExpiry && instantNs(entry.knownExpiry.at) <= BigInt(this.#clock()) * 1000000n
      && entry.collector.state === 'ready') entry.collector.invalidate('EXPIRY');
  }
  #receipt(value, entry, {advance = false} = {}) {
    const ns = instantNs(value), now = BigInt(this.#clock()) * 1000000n;
    requireThat(ns <= now + BigInt(this.#skewMs) * 1000000n && now - ns < BigInt(this.#staleMs) * 1000000n, 'INVALID_RECEIPT_CLOCK');
    requireThat(entry.lastReceiptNs === null || ns >= entry.lastReceiptNs, 'RECEIPT_ROLLBACK');
    if (advance) entry.lastReceiptNs = ns;
    return ns;
  }
  #books(books, receivedNs) {
    for (const book of books) {
      requireThat(book.asOf !== null && instantNs(book.asOf.at) <= receivedNs + BigInt(this.#skewMs) * 1000000n, 'INVALID_BOOK_CLOCK');
      for (const levels of [book.bids, book.asks]) for (const level of levels) {
        requireThat(Number.isSafeInteger(level.quantity) && level.quantity > 0, 'INVALID_INTEGER_DEPTH');
        // Local capture policy accepts only documented resting limit ticks.
        price(String(level.price));
      }
      requireThat(!book.bids.length || !book.asks.length || book.bids[0].price < book.asks[0].price, 'CROSSED_BOOK');
    }
  }
  subscribed({marketId, connectionId, reason = 'INITIAL', receivedAt = new Date(this.#clock()).toISOString()}) {
    const entry = this.#entry(marketId); this.#connection(connectionId);
    if (entry.connectionId === connectionId && !['disabled', 'disconnected', 'stopped'].includes(entry.collector.state)) return this.view(marketId);
    requireThat(!entry.connections.has(connectionId) && entry.connections.size < 10000, 'REUSED_CONNECTION_ID');
    requireThat(['INITIAL', 'RECONNECT', 'REFRESH'].includes(reason), 'INVALID_SUBSCRIPTION_REASON');
    const ns = this.#receipt(receivedAt, entry, {advance: true});
    entry.connections.add(connectionId); entry.connectionId = connectionId; entry.subscribedNs = ns; entry.ticket = null;
    entry.collector.subscribed(reason); return this.view(marketId);
  }
  disconnected({marketId, connectionId, reason = 'DISCONNECTED'}) {
    const entry = this.#entry(marketId);
    if (connectionId !== entry.connectionId) return false;
    entry.ticket = null;
    entry.collector.disconnected(['REFRESH', 'SOCKET_ERROR', 'RECOVERY_FAILED'].includes(reason) ? reason : 'DISCONNECTED');
    return true;
  }
  invalidate({marketId, connectionId, reason}) {
    const entry = this.#entry(marketId);
    requireThat(invalidationReasons.has(reason), 'INVALID_INVALIDATION_REASON');
    if (connectionId !== entry.connectionId) return false;
    entry.ticket = null; entry.collector.invalidate(reason); return true;
  }
  ingest({marketId, connectionId, event, payload, receivedAt = new Date(this.#clock()).toISOString(), topic}) {
    const entry = this.#entry(marketId), collector = entry.collector;
    if (connectionId !== entry.connectionId || ['disabled', 'disconnected', 'stopped'].includes(collector.state)) return false;
    try {
      this.#expire(entry);
      requireThat(topic === undefined || topic === entry.scope.topic, 'CONTEXT_MISMATCH');
      const receivedNs = this.#receipt(receivedAt, entry), parsed = parseEvent(event, payload, entry.scope);
      if (event === 'market_batch') {
        this.#books(parsed.payload.books, receivedNs);
        for (const trade of parsed.payload.trades) requireThat(instantNs(trade.executedAt) <= receivedNs + BigInt(this.#skewMs) * 1000000n, 'INVALID_TRADE_CLOCK');
        for (const hint of [...parsed.payload.bookDirty, ...parsed.payload.marketSettled]) requireThat(instantNs(hint.at) <= receivedNs + BigInt(this.#skewMs) * 1000000n, 'INVALID_HINT_CLOCK');
      } else requireThat(instantNs(parsed.payload.at) <= receivedNs + BigInt(this.#skewMs) * 1000000n, 'INVALID_HINT_CLOCK');
      entry.lastReceiptNs = receivedNs;
      const accepted = collector.ingest(event, payload, {receivedAt: timestamp(receivedAt), epoch: collector.epoch});
      if (event === 'market_batch') for (const book of parsed.payload.books) {
        const held = collector.books.get(book.exchangeId)?.book;
        if (held && compareVersion(book.asOf, held.asOf) === 0 && (!entry.knownExpiry || compareVersion(book.asOf, entry.knownExpiry.version) > 0))
          entry.knownExpiry = {version: book.asOf, at: book.nextExpiryAt};
      }
      return accepted;
    } catch {
      collector.metrics.schemaErrors++; collector.invalidate('SCHEMA'); return false;
    }
  }
  beginSnapshot({marketId, connectionId}) {
    const entry = this.#entry(marketId);
    requireThat(connectionId === entry.connectionId, 'STALE_CONNECTION');
    const recovery = entry.collector.beginRecovery();
    const ticket = Object.freeze({marketId: entry.scope.marketId, connectionId, method: 'GET', endpoint: snapshotPath(entry.scope), ...recovery});
    entry.ticket = ticket; return ticket;
  }
  completeSnapshot(ticket, response) {
    const entry = this.#entry(ticket?.marketId);
    if (ticket !== entry.ticket || ticket.connectionId !== entry.connectionId) return false;
    entry.ticket = null;
    try {
      requireThat(response?.endpoint === ticket.endpoint && (response.status === undefined || response.status === 200), 'SNAPSHOT_SCOPE_MISMATCH');
      const receivedNs = this.#receipt(response.receivedAt, entry), requestNs = instantNs(response.requestStartedAt);
      requireThat(requestNs >= entry.subscribedNs && requestNs >= BigInt(ticket.startedMs) * 1000000n && requestNs <= receivedNs
        && receivedNs - requestNs <= BigInt(this.#snapshotMs) * 1000000n, 'INVALID_SNAPSHOT_CLOCK');
      const books = parseSnapshot(response.data, entry.scope); this.#books(books, receivedNs);
      const raw = response.data.contexts.find(c => c?.type === 'tournament' && c.tournament?.id === entry.scope.tournamentId).orderbook.exchanges;
      requireThat(raw.every(book => book.option === 'YES'), 'SNAPSHOT_OPTION_MISMATCH');
      entry.lastReceiptNs = receivedNs;
      // The existing collector owns normalized snapshot/recovery ordering. Add
      // the actual queued REST dispatch time only to the snapshot it records.
      entry.snapshotReceipt.requestStartedAt = timestamp(response.requestStartedAt);
      let recovered;
      try { recovered = entry.collector.completeRecovery(ticket, response.data, {receivedAt: response.receivedAt}); }
      finally { entry.snapshotReceipt.requestStartedAt = null; }
      // REST omits expiry metadata. An equal engine version cannot erase a
      // deadline already learned from the full pushed book of that version.
      const held = entry.collector.books.get(entry.scope.exchangeIds[0])?.book;
      if (recovered && entry.knownExpiry && compareVersion(held.asOf, entry.knownExpiry.version) > 0) entry.knownExpiry = null;
      return recovered && this.view(entry.scope.marketId).ready;
    } catch {
      entry.collector.metrics.schemaErrors++; entry.collector.invalidate('RECOVERY_FAILED'); return false;
    }
  }
  snapshotFailed(ticket) {
    const entry = this.#entry(ticket?.marketId);
    if (ticket !== entry.ticket || ticket.connectionId !== entry.connectionId) return false;
    entry.ticket = null; entry.collector.invalidate('RECOVERY_FAILED'); return true;
  }
  view(marketId) {
    const entry = this.#entry(marketId); this.#expire(entry);
    const status = entry.collector.status(), ready = status.coverage.booksReady;
    return {mode: 'read_only_market_capture', executionAuthorized: false, marketId: entry.scope.marketId, exchangeId: entry.scope.exchangeIds[0],
      connectionId: entry.connectionId, topic: entry.scope.topic, ready, needsSnapshot: !entry.collector.needsMarketState && ['needs_resync', 'resyncing'].includes(status.state),
      state: status.state, epoch: status.epoch, revision: status.revision, reasons: status.reasons, nextKnownExpiryAt: entry.knownExpiry?.at ?? null,
      books: ready ? [...entry.collector.books.values()].map(({book, observedMs, source}) => ({...structuredClone(book), receivedAt: new Date(observedMs).toISOString(), source})) : [], metrics: status.metrics};
  }
  status() { return {mode: 'read_only_market_capture', executionAuthorized: false, markets: [...this.#markets.keys()].map(id => this.view(id))}; }
  needsRecovery() { return this.status().markets.filter(market => market.needsSnapshot); }
  stop() { for (const entry of this.#markets.values()) { entry.ticket = null; entry.collector.stop(); } }
}
