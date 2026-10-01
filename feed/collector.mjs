import {config, parseEvent, parseSnapshot, compareVersion, timestamp, requireThat} from './contract.mjs';
const REASONS = new Set(['INITIAL', 'RECONNECT', 'REFRESH', 'SOCKET_ERROR', 'DISCONNECTED', 'GAP', 'RESYNC_REQUIRED', 'UNSEQUENCED_TRADE', 'SCHEMA', 'STALE', 'EXPIRY', 'MISSING_BOOK', 'SETTLEMENT', 'BOOK_DIRTY', 'VERSION_CONFLICT', 'RECOVERY_FAILED']);
export class MarketCollector {
  constructor(scope, {staleMs = 10000, now = Date.now, capture = null, maxTradeIds = 100000} = {}) {
    this.scope = config(scope); requireThat(Number.isFinite(staleMs) && staleMs > 0); requireThat(Number.isSafeInteger(maxTradeIds) && maxTradeIds > 0);
    this.staleMs = staleMs; this.now = now; this.capture = capture; this.maxTradeIds = maxTradeIds;
    this.state = 'disabled'; this.epoch = 0; this.generation = 0; this.revision = null;
    this.bookOrdinal = 0; this.needsMarketState = false; this.books = new Map(); this.seenTrades = new Map(); this.reasons = new Set(); this.tapeComplete = false;
    this.metrics = {events: 0, acceptedBatches: 0, duplicateBatches: 0, outOfOrderBatches: 0, gaps: 0, schemaErrors: 0, booksApplied: 0, olderBooks: 0, trades: 0, duplicateTrades: 0, tradeIdEvictions: 0, reconnects: 0, recoveries: 0, recoveryDiscards: 0, clockSkewSamples: 0, eventAge: {count: 0, sumMs: 0, maxMs: null}, recoveryLatency: {count: 0, sumMs: 0, maxMs: null}};
    this.record('config', {scope: this.scope, staleMs: this.staleMs});
  }
  record(kind, data = {}) { this.capture?.append({kind, observedAt: new Date(this.now()).toISOString(), topic: this.scope.topic, epoch: this.epoch, ...data}); }
  sample(metric, value) {
    if (value < 0) { this.metrics.clockSkewSamples++; return; }
    const m = this.metrics[metric]; m.count++; m.sumMs += value; m.maxMs = Math.max(m.maxMs ?? 0, value);
  }
  invalidate(reason) {
    requireThat(REASONS.has(reason), 'INVALID_REASON');
    if (this.state === 'stopped' || this.state === 'disabled') return;
    if (reason === 'SETTLEMENT') this.needsMarketState = true;
    this.generation++; this.reasons.add(reason); if (this.state !== 'disconnected') this.state = 'needs_resync';
    this.record('invalidation', {reason, generation: this.generation});
  }
  subscribed(reason = 'INITIAL') {
    requireThat(['INITIAL','RECONNECT','REFRESH'].includes(reason), 'INVALID_REASON');
    if (this.state === 'stopped') return false;
    this.epoch++; this.revision = null; this.state = 'needs_resync';
    if (reason !== 'INITIAL') this.metrics.reconnects++;
    // Retain versions to prevent rollback, but every epoch requires REST again.
    this.record('subscribed', {reason}); this.invalidate(reason); return true;
  }
  disconnected(reason = 'DISCONNECTED') {
    if (this.state === 'stopped' || this.state === 'disabled') return;
    this.invalidate(reason); this.state = 'disconnected'; this.record('disconnected', {reason});
  }
  stop() { this.state = 'stopped'; this.generation++; this.record('stopped'); }
  applyBook(book, observedMs, source) {
    const old = this.books.get(book.exchangeId);
    const cmp = old ? compareVersion(book.asOf, old.book.asOf) : 1;
    if (old && cmp === 0 && JSON.stringify([book.bids, book.asks]) !== JSON.stringify([old.book.bids, old.book.asks])) {
      this.invalidate('VERSION_CONFLICT'); return false;
    }
    if (cmp < 0) { this.metrics.olderBooks++; return false; }
    if (cmp > 0 || !old) { this.books.set(book.exchangeId, {book, observedMs, source, ordinal: ++this.bookOrdinal}); this.metrics.booksApplied++; }
    // Authoritative read of identical version revalidates quiet books; duplicate
    // broadcast never extends freshness of an old book or clears expiry.
    else if (source === 'rest') this.books.set(book.exchangeId, {book, observedMs, source, ordinal: ++this.bookOrdinal});
    return true;
  }
  ingest(event, raw, {receivedAt = new Date(this.now()).toISOString(), epoch = this.epoch} = {}) {
    if (['stopped','disabled','disconnected'].includes(this.state) || epoch !== this.epoch) return false;
    this.tick();
    this.metrics.events++;
    let parsed, receivedMs;
    try { parsed = parseEvent(event, raw, this.scope); receivedMs = Date.parse(timestamp(receivedAt)); requireThat(receivedMs <= this.now() + 1000, 'FUTURE_RECEIPT'); }
    catch { this.metrics.schemaErrors++; this.invalidate('SCHEMA'); this.record('rejected', {code: 'INVALID_SCHEMA'}); return false; }
    this.record('event', {receivedAt: new Date(receivedMs).toISOString(), ...parsed});
    const p = parsed.payload;
    if (event !== 'market_batch') { this.invalidate(event === 'book_dirty' ? 'BOOK_DIRTY' : 'SETTLEMENT'); return true; }
    for (const b of p.books) { this.applyBook(b, receivedMs, 'websocket'); this.sample('eventAge', receivedMs - Date.parse(b.asOf.at)); }
    this.tick();
    const previous = this.revision, revision = p.delivery.revision;
    if (p.resyncRequired) { this.revision = Math.max(this.revision ?? 0, revision); this.invalidate('RESYNC_REQUIRED'); return true; }
    if (previous !== null && revision <= previous) { this.metrics.duplicateBatches++; if (revision < previous) this.metrics.outOfOrderBatches++; return true; }
    this.revision = revision; this.metrics.acceptedBatches++;
    if (previous !== null && p.delivery.previousRevision > previous) { this.metrics.gaps++; this.tapeComplete = false; this.invalidate('GAP'); }
    const named = new Set([...p.trades, ...p.bookDirty].map(x => x.exchangeId));
    if ([...named].some(id => !p.books.some(b => b.exchangeId === id))) this.invalidate('MISSING_BOOK');
    if (p.marketSettled.length) this.invalidate('SETTLEMENT');
    for (const t of p.trades) {
      this.sample('eventAge', receivedMs - Date.parse(t.executedAt));
      if (t.sequence === null) { this.invalidate('UNSEQUENCED_TRADE'); continue; }
      if (this.seenTrades.has(t.id)) { this.metrics.duplicateTrades++; continue; }
      this.seenTrades.set(t.id, t.sequence); this.metrics.trades++;
      if (this.seenTrades.size > this.maxTradeIds) { this.seenTrades.delete(this.seenTrades.keys().next().value); this.metrics.tradeIdEvictions++; }
      // Captured batch contains the raw normalized trade; this entry signals
      // deduplication only, not that a historical tape is complete or executable.
      this.record('trade_observed', {trade: t, continuousAtReceipt: this.state === 'ready'});
    }
    return true;
  }
  beginRecovery() {
    requireThat(!['disabled','stopped','disconnected'].includes(this.state), 'NOT_SUBSCRIBED');
    this.state = 'resyncing'; this.record('recovery_started', {generation: this.generation});
    return Object.freeze({epoch: this.epoch, generation: this.generation, startedMs: this.now(), bookOrdinal: this.bookOrdinal});
  }
  completeRecovery(ticket, raw, {receivedAt = new Date(this.now()).toISOString()} = {}) {
    if (ticket.epoch !== this.epoch || ticket.generation !== this.generation || this.state !== 'resyncing') { this.metrics.recoveryDiscards++; return false; }
    let books, observedMs;
    try { books = parseSnapshot(raw, this.scope); observedMs = Date.parse(timestamp(receivedAt)); requireThat(observedMs >= ticket.startedMs && observedMs <= this.now() + 1000, 'INVALID_RECEIPT'); }
    catch { this.metrics.schemaErrors++; this.invalidate('SCHEMA'); return false; }
    this.record('snapshot', {receivedAt: new Date(observedMs).toISOString(), books});
    for (const b of books) {
      const held = this.books.get(b.exchangeId);
      // An ordinal distinguishes pushes DURING recovery, even in the same ms;
      // otherwise it cannot revalidate pre-recovery stale state.
      if (held && compareVersion(b.asOf, held.book.asOf) < 0 && held.ordinal <= ticket.bookOrdinal) { this.invalidate('RECOVERY_FAILED'); return false; }
      this.applyBook(b, observedMs, 'rest');
    }
    if (ticket.generation !== this.generation) return false;
    if (this.needsMarketState) { this.invalidate('SETTLEMENT'); return false; }
    if ([...this.books.values()].some(x => x.book.asOf === null)) { this.invalidate('RECOVERY_FAILED'); return false; }
    this.reasons.clear(); this.state = 'ready'; this.metrics.recoveries++;
    this.sample('recoveryLatency', observedMs - ticket.startedMs); this.tick(); this.record('recovery_finished', {booksReady: this.state === 'ready', generation: this.generation});
    return this.state === 'ready';
  }
  tick() {
    if (this.state !== 'ready') return;
    const now = this.now();
    if ([...this.books.values()].some(x => now - x.observedMs >= this.staleMs)) this.invalidate('STALE');
    if ([...this.books.values()].some(x => x.book.nextExpiryAt !== null && Date.parse(x.book.nextExpiryAt) <= now)) this.invalidate('EXPIRY');
  }
  status() {
    this.tick();
    const metrics = structuredClone(this.metrics);
    for (const name of ['eventAge','recoveryLatency']) metrics[name].meanMs = metrics[name].count ? metrics[name].sumMs / metrics[name].count : null;
    return {schemaVersion: 1, mode: 'read_only_scaffold', state: this.state, topic: this.scope.topic, epoch: this.epoch, revision: this.revision,
      reasons: [...this.reasons].sort(), coverage: {configuredExchanges: this.scope.exchangeIds.length, observedExchanges: this.books.size,
        versionedExchanges: [...this.books.values()].filter(x => x.book.asOf !== null).length,
        booksReady: this.state === 'ready', tradeTapeComplete: false, universeComplete: false}, metrics};
  }
}
