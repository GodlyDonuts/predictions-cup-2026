import {createPassiveObserver, observePassive, passiveReport} from './passive-observer.mjs';
import {timestamp, instantNs, compareVersion, parseEvent, config as feedConfig, book as feedBook} from '../feed/contract.mjs';
import {money, price, instant} from '../paper/fixed.mjs';

// One immutable capture session. observedAt is local availability; receivedAt
// remains source metadata. A buffered earlier receipt never backdates a decision.
export function createPassiveCapture({scope, sourceHash, cutoff, sessionId}) {
  if (!/^[A-Za-z0-9_-]{1,60}$/.test(sessionId) || !Array.isArray(scope?.markets) || !scope.markets.length) throw Error('Invalid passive capture scope');
  const seen = new Set(), exchanges = new Set(), contexts = {};
  for (const market of scope.markets) {
    const checked = feedConfig({...market, tournamentId: scope.tournamentId, exchangeIds: [market.exchangeId]});
    if (seen.has(checked.marketId) || exchanges.has(checked.exchangeIds[0])) throw Error('Duplicate passive capture market');
    seen.add(checked.marketId); exchanges.add(checked.exchangeIds[0]);
    contexts[checked.marketId] = {epoch: 0, revision: null, ready: false, snapshot: null, recovery: null, latestBook: null,
      knownExpiry: null, recoveredAt: null, staleMs: 20000, sourceEnded: false,
      observer: createPassiveObserver({...market, tournamentId: scope.tournamentId, sourceHash, cutoff})};
  }
  return {schemaVersion: 1, sessionId, cutoff, captureIndex: -1, lastObservedAt: null, lastObservedNs: null, sourceEnded: false, contexts};
}
function toBook(raw, market, availableAt, requestStartedAt) {
  const sourceAt = timestamp(raw.asOf.at), sequence = String(raw.asOf.sequence);
  return {marketId: market.bank.marketId, exchangeId: raw.exchangeId, instrument: raw.exchangeId + ':YES',
    receivedAt: availableAt, requestStartedAt, sourceAt, sequence, versionKey: sequence + ':' + sourceAt,
    bids: raw.bids.map(l => ({price: money(price(String(l.price))), quantity: l.quantity})),
    asks: raw.asks.map(l => ({price: money(price(String(l.price))), quantity: l.quantity}))};
}
function capturedEvent(row, scope) {
  // Capture already removed diagnostic free text and settlement labels. Restore
  // fixed placeholders solely to reuse the public shape/scope validator; never
  // pretend that these placeholders are original wire values.
  const payload = structuredClone(row.payload);
  if (row.event === 'market_batch') {
    payload.delivery = {...payload.delivery, correlationId: ''};
    payload.marketSettled = (payload.marketSettled || []).map(x => ({...x, settledWith: x.refund === true ? 'REFUND' : 'SETTLED'}));
  } else if (row.event === 'market_settled') payload.settledWith = payload.refund === true ? 'REFUND' : 'SETTLED';
  return parseEvent(row.event, payload, scope);
}
export function consumePassiveCapture(input, row) {
  if (row?.format !== 'sig-feed-capture' || row.schemaVersion !== 1 || !Number.isSafeInteger(row.captureIndex)
    || row.captureIndex !== input.captureIndex + 1) throw Error('Missing or reordered realtime capture');
  const observedNs = instantNs(row.observedAt), receivedNs = row.receivedAt === undefined ? observedNs : instantNs(row.receivedAt);
  const observed = Number(observedNs / 1000000n), received = Number(receivedNs / 1000000n);
  if (input.lastObservedNs !== null && observedNs < BigInt(input.lastObservedNs)) throw Error('Capture observation clock rollback');
  if (receivedNs > observedNs + 1000000000n) throw Error('Future capture receipt');
  const state = structuredClone(input); state.captureIndex = row.captureIndex;
  state.lastObservedAt = new Date(observed).toISOString(); state.lastObservedNs = String(observedNs);
  const id = /^tournament:[a-f0-9-]{36}:market:([1-9]\d*)$/.exec(row.topic || '')?.[1], ctx = state.contexts[id];
  if (!ctx) throw Error('Unknown capture market');
  const bank = ctx.observer.bank, scope = feedConfig({tournamentId: bank.tournamentId, marketId: id, exchangeIds: [bank.exchangeId]});
  if (row.topic !== scope.topic || !Number.isSafeInteger(row.epoch) || row.epoch < ctx.epoch) throw Error('Capture scope or epoch rollback');
  const at = new Date(Math.max(observed, received)).toISOString(), records = [], events = [];
  function emit(kind, extra = {}, recovered = false, previousRevision = ctx.revision ?? 0) {
    // Raw delivery gaps are checked below. Derived records may omit unchanged
    // books, so their revision boundary cannot imply a gap across those rows.
    previousRevision = Math.min(previousRevision, ctx.observer.state.revision ?? 0);
    const result = observePassive(ctx.observer.state, {index: ctx.observer.state.index + 1, kind, receivedAt: at,
      continuity: {epoch: state.sessionId + '-' + ctx.epoch, revision: ctx.revision ?? 0, previousRevision, ready: ctx.ready, continuous: ctx.ready, recovered}, ...extra}, bank);
    ctx.observer.state = result.state;
    if (result.record) records.push({marketId: id, sourceCaptureIndex: row.captureIndex, sourceReceivedAt: row.receivedAt ?? null, ...result.record});
    events.push(...result.events.map(e => ({marketId: id, ...e})));
  }
  function invalidate(reason, clearRecovery = true) {
    ctx.ready = false; ctx.recoveredAt = null;
    if (clearRecovery) { ctx.snapshot = null; ctx.recovery = null; }
    emit('invalidate', {reason});
  }
  function remember(raw, origin) {
    if (!raw?.asOf) return false;
    const old = ctx.latestBook, cmp = old ? compareVersion(raw.asOf, old.book.asOf) : 1;
    if (cmp < 0) return false;
    if (cmp === 0 && JSON.stringify([raw.bids, raw.asks]) !== JSON.stringify([old.book.bids, old.book.asks])) { invalidate('schema'); return false; }
    if (raw.expiryKnown && (!ctx.knownExpiry || compareVersion(raw.asOf, ctx.knownExpiry.version) >= 0))
      ctx.knownExpiry = {at: raw.nextExpiryAt, version: structuredClone(raw.asOf)};
    // Broadcast duplicates cannot refresh a quiet book's receipt age.
    if (cmp === 0 && origin === 'stream') return false;
    ctx.latestBook = {book: structuredClone(raw), receivedAt: row.receivedAt || row.observedAt, availableAt: at,
      epoch: row.epoch, captureIndex: row.captureIndex, origin};
    return true;
  }
  function expired() { return ctx.knownExpiry?.at != null && Date.parse(ctx.knownExpiry.at) <= Date.parse(at); }
  if (row.epoch !== ctx.epoch) {
    ctx.epoch = row.epoch; ctx.revision = null; invalidate('disconnect');
  }
  if (row.kind === 'config') {
    if (row.scope?.topic !== scope.topic || row.scope?.exchangeIds?.length !== 1 || row.scope.exchangeIds[0] !== bank.exchangeId
      || !Number.isSafeInteger(row.staleMs) || row.staleMs <= 0 || row.staleMs > 60000) throw Error('Capture config mismatch');
    ctx.staleMs = Math.min(row.staleMs, bank.config.maxDataAgeMs);
  } else if (['invalidation', 'disconnected', 'stopped', 'rejected', 'subscribed'].includes(row.kind)) {
    const reason = row.reason === 'SETTLEMENT' ? 'settlement' : row.reason === 'GAP' ? 'gap' : ['STALE', 'EXPIRY'].includes(row.reason) ? 'stale'
      : row.reason === 'BOOK_DIRTY' ? 'book_dirty' : ['disconnected', 'subscribed'].includes(row.kind) ? 'disconnect' : 'resync';
    invalidate(reason);
    if (row.kind === 'stopped') ctx.sourceEnded = true;
  } else if (row.kind === 'recovery_started') {
    if (!Number.isSafeInteger(row.generation) || row.generation < 0) throw Error('Invalid recovery generation');
    invalidate('resync'); ctx.recovery = {generation: row.generation, captureIndex: row.captureIndex, startedAt: at,
      startedNs: String(observedNs > receivedNs ? observedNs : receivedNs)};
  } else if (row.kind === 'snapshot') {
    if (!ctx.recovery || !row.receivedAt || !row.requestStartedAt || !Array.isArray(row.books) || row.books.length !== 1) { invalidate('schema'); }
    else {
      const requestNs = instantNs(row.requestStartedAt);
      if (requestNs < BigInt(ctx.recovery.startedNs) || requestNs > receivedNs
        || receivedNs - requestNs > BigInt(bank.config.maxDataAgeMs) * 1000000n) { invalidate('schema'); }
      else {
        const raw = feedBook(row.books[0], scope, true);
        // The observer's local decision clocks use milliseconds. Floor the true
        // HTTP start (never make it later) after validating exact nanoseconds.
        ctx.snapshot = {book: raw, receivedAt: row.receivedAt,
          requestStartedAt: new Date(Number(requestNs / 1000000n)).toISOString(),
          originalRequestStartedAt: row.requestStartedAt, captureIndex: row.captureIndex};
      }
    }
  } else if (row.kind === 'recovery_finished') {
    if (row.booksReady !== true || !ctx.recovery || !ctx.snapshot || row.generation !== ctx.recovery.generation) invalidate('resync');
    else {
      let raw = ctx.snapshot.book, request = ctx.snapshot.requestStartedAt, receipt = ctx.snapshot.receivedAt;
      const pushed = ctx.latestBook;
      if (raw?.asOf && pushed?.epoch === ctx.epoch && pushed.origin === 'stream' && pushed.captureIndex > ctx.recovery.captureIndex
        && Date.parse(pushed.receivedAt) >= Date.parse(ctx.recovery.startedAt)
        && compareVersion(pushed.book.asOf, raw.asOf) > 0) { raw = pushed.book; request = new Date(Date.parse(pushed.receivedAt)).toISOString(); receipt = pushed.receivedAt; }
      if (!raw?.asOf || !raw.bids.length || !raw.asks.length || Date.parse(at) - Date.parse(receipt) >= ctx.staleMs
        || Date.parse(at) - Date.parse(request) > bank.config.maxDataAgeMs || expired()
        || ctx.latestBook && compareVersion(raw.asOf, ctx.latestBook.book.asOf) < 0) invalidate('stale');
      else {
        ctx.ready = true; ctx.recoveredAt = at;
        emit('book', {book: toBook(raw, ctx.observer, at, request)}, true);
        ctx.latestBook = {book: structuredClone(raw), receivedAt: receipt, availableAt: at, epoch: ctx.epoch, captureIndex: row.captureIndex, origin: 'recovery'};
      }
    }
    ctx.snapshot = null; ctx.recovery = null;
  } else if (row.kind === 'event') {
    const parsed = capturedEvent(row, scope);
    if (row.event !== 'market_batch') invalidate(row.event === 'market_settled' ? 'settlement' : 'book_dirty');
    else {
      const p = parsed.payload, d = p.delivery, previous = ctx.revision;
      // A callback from before accepted recovery must not become a new causal
      // observation just because its buffered capture row was drained later.
      if (ctx.ready && ctx.recoveredAt && received < Date.parse(ctx.recoveredAt)
        || ctx.recovery && received < Date.parse(ctx.recovery.startedAt)) {
        events.push({marketId: id, kind: 'ignored_pre_recovery_receipt', at});
        return {state, records, events};
      }
      const missingNamedBook = [...p.trades, ...p.bookDirty].some(t => !p.books.some(b => b.exchangeId === t.exchangeId));
      if (ctx.ready && (Date.parse(at) - Date.parse(ctx.latestBook?.receivedAt || '') >= ctx.staleMs || expired())) invalidate('stale');
      if (p.resyncRequired || p.marketSettled.length || p.trades.some(t => t.sequence === null) || missingNamedBook
        || previous !== null && d.revision > previous && d.previousRevision > previous) invalidate('gap');
      ctx.revision = Math.max(previous ?? 0, d.revision);
      // The first delivery after REST establishes the topic revision baseline;
      // its historical previousRevision says nothing about pre-subscription tape.
      const boundary = previous === null ? 0 : Math.min(d.previousRevision, ctx.revision);
      for (const raw of p.books) {
        if (!raw.bids.length || !raw.asks.length) { remember(raw, 'stream'); invalidate('stale'); continue; }
        if (remember(raw, 'stream') && ctx.ready) {
          if (expired()) invalidate('stale');
          else emit('book', {book: toBook(raw, ctx.observer, at, new Date(received).toISOString())}, false, boundary);
        }
      }
      if ((previous === null || d.revision > previous) && ctx.ready) for (const t of p.trades)
        if (t.id !== null && t.sequence !== null) emit('trade', {trade: {...t, sourceAt: t.executedAt, sequence: String(t.sequence),
          price: money(BigInt(Math.round(t.price * 1000000)))}}, false, boundary);
    }
  } else if (!['trade_observed', 'summary'].includes(row.kind)) throw Error('Unknown normalized capture kind');
  state.sourceEnded = Object.values(state.contexts).every(c => c.sourceEnded);
  return {state, records, events};
}
export function passiveCaptureReport(state) {
  return {schemaVersion: 1, mode: 'passive_opportunity_measurement', sessionId: state.sessionId, cutoff: state.cutoff,
    captureIndex: state.captureIndex, sourceEnded: state.sourceEnded, markets: Object.values(state.contexts).map(c => passiveReport(c.observer.state, c.observer.bank)),
    ordersSubmitted: 0, simulatedFills: null, pnl: null};
}
