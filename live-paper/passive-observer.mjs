import {createHash} from 'node:crypto';
import {amount, money, price, TICK, SCALE, instant, sequence} from '../paper/fixed.mjs';
import {instantNs, timestamp} from '../feed/contract.mjs';

/* Pure market research, with no orders, fill simulation, cash or PnL.
 * Adapter contract:
 * {index:1..N, kind:'book'|'trade'|'invalidate'|'clock', receivedAt:ISO-ms,
 *  continuity:{epoch:string, revision:integer, previousRevision:integer,
 *              ready:boolean, continuous:boolean, recovered?:boolean},
 *  book?: model.normalizeBook-shaped YES book,
 *  trade?:{id,sequence,marketId,exchangeId,tournamentId,sourceAt,price,quantity},
 *  reason?:'gap'|'disconnect'|'stale'|'resync'|'settlement'|'schema'|'book_dirty'}
 * The adapter must forward every lifecycle invalidation. Set recovered only
 * after an accepted recovery_finished with booksReady, never from a snapshot
 * alone. Epoch/revision are topic delivery metadata, not engine sequences.
 * For a stream book, requestStartedAt=receivedAt means no REST request exists;
 * for REST retain its real request start. Decimal price strings are required.
 * Process books before trades from the same batch; revision/source checks then
 * prevent that batch from supplying its own future activation/trade evidence.
 * Only ONE representation of each trade (batch OR derived row) is needed.
 * Local continuity is never asserted to be a complete historical trade tape.
 * Persist the returned sanitized record in a separate fsynced caller journal;
 * restore by replaying those records against this immutable bank. No worker or
 * Capture adapter is installed here, and no existing cohort is changed.
 */
export const PASSIVE_DEFAULTS = Object.freeze({
  latencyMs: 500, maxDataAgeMs: 20000, maxSourceClockSkewMs: 2000,
  quoteLifetimeMs: 60000, quoteEveryMs: 60000, horizonsMs: Object.freeze([60000, 120000, 300000]),
  markoutLatenessMs: 20000, orderQuantity: 10, slippageTicks: 1,
  maxQuotePairs: 1000, maxTradeIds: 100000,
});
const canonical = x => Array.isArray(x) ? '[' + x.map(canonical).join(',') + ']' : x && typeof x === 'object'
  ? '{' + Object.keys(x).sort().map(k => JSON.stringify(k) + ':' + canonical(x[k])).join(',') + '}' : JSON.stringify(x);
const digest = x => createHash('sha256').update(canonical(x)).digest('hex');
const check = (v, message) => { if (!v) throw Error(message); };
const id = v => typeof v === 'string' && /^[1-9]\d*$/.test(v);
const identity = v => typeof v === 'string' && /^[A-Za-z0-9_-]{1,100}$/.test(v);
const inc = (s, key) => { s.counts[key] = (s.counts[key] || 0) + 1; };
function checkedConfig(input = {}) {
  check(Object.keys(input).every(k => Object.hasOwn(PASSIVE_DEFAULTS, k)), 'Unknown passive observer config');
  const c = structuredClone({...PASSIVE_DEFAULTS, ...input});
  for (const [key, n] of Object.entries(c)) if (key !== 'horizonsMs') check(Number.isSafeInteger(n) && n > 0, 'Invalid config ' + key);
  check(c.maxSourceClockSkewMs <= 2000 && c.maxDataAgeMs <= 60000 && c.latencyMs < c.quoteLifetimeMs
    && c.quoteLifetimeMs <= 300000 && c.quoteEveryMs >= c.quoteLifetimeMs && c.orderQuantity <= 100
    && c.slippageTicks <= 5 && c.maxQuotePairs <= 10000 && c.maxTradeIds <= 100000, 'Unsafe observer bounds');
  check(Array.isArray(c.horizonsMs) && c.horizonsMs.length > 0 && c.horizonsMs.length <= 10
    && c.horizonsMs.every((v, i) => Number.isSafeInteger(v) && v > c.latencyMs && v <= 3600000 && (!i || v > c.horizonsMs[i - 1])), 'Invalid markout horizons');
  return c;
}
export function createPassiveObserver({marketId, exchangeId, tournamentId = 'bda92870-621e-47b0-bc3c-3602c5c26f55', sourceHash, cutoff, config}) {
  check(id(marketId) && id(exchangeId) && /^[a-f0-9-]{36}$/.test(tournamentId), 'Invalid market scope');
  check(/^[a-f0-9]{64}$/.test(sourceHash), 'Frozen source hash required'); instant(cutoff);
  const c = checkedConfig(config), bank = {schemaVersion: 1, name: 'passive-opportunity-observer-v1', mode: 'measurement_only',
    marketId, exchangeId, tournamentId, cutoff, sourceHash, config: c, configHash: digest(c)};
  return {bank, state: {schemaVersion: 1, bankHash: digest(bank), index: 0, chainHash: sourceHash, lastInputHash: null,
    lastAt: null, epoch: null, revision: null, ready: false, lastBook: null, quotes: [], lastQuoteAt: null,
    seenTrades: {}, lastTradeSequence: null, lastTradeSourceNs: null, pairs: 0, counts: {}}};
}
function assertState(s, bank) {
  check(bank?.name === 'passive-opportunity-observer-v1' && bank.mode === 'measurement_only'
    && bank.configHash === digest(checkedConfig(bank.config)) && s?.bankHash === digest(bank) && s.schemaVersion === 1, 'Frozen observer bank changed');
  check(Number.isSafeInteger(s.index) && s.index >= 0 && Array.isArray(s.quotes), 'Invalid observer state');
}
function normalized(input, bank) {
  check(Number.isSafeInteger(input?.index) && input.index >= 1 && ['book', 'trade', 'invalidate', 'clock'].includes(input.kind), 'Invalid observer event');
  const at = instant(input.receivedAt), c = input.continuity;
  check(c && identity(c.epoch) && Number.isSafeInteger(c.revision) && c.revision >= 0
    && Number.isSafeInteger(c.previousRevision) && c.previousRevision >= 0 && c.previousRevision <= c.revision
    && typeof c.ready === 'boolean' && typeof c.continuous === 'boolean'
    && (c.recovered === undefined || typeof c.recovered === 'boolean'), 'Invalid continuity metadata');
  const e = {index: input.index, kind: input.kind, receivedAt: input.receivedAt,
    continuity: {epoch: c.epoch, revision: c.revision, previousRevision: c.previousRevision, ready: c.ready, continuous: c.continuous, recovered: c.recovered === true}};
  if (e.kind === 'invalidate') {
    check(['gap', 'disconnect', 'stale', 'resync', 'settlement', 'schema', 'book_dirty'].includes(input.reason), 'Invalid invalidation reason'); e.reason = input.reason;
  }
  if (e.kind === 'book') {
    const b = input.book; check(b?.marketId === bank.marketId && b.exchangeId === bank.exchangeId
      && b.instrument === bank.exchangeId + ':YES' && b.receivedAt === e.receivedAt, 'Book scope mismatch');
    const request = instant(b.requestStartedAt); sequence(b.sequence);
    check(request <= at && at - request <= bank.config.maxDataAgeMs, 'Invalid book request clock');
    const sourceAt = timestamp(b.sourceAt);
    check(b.versionKey === b.sequence + ':' + sourceAt && instantNs(sourceAt) <= BigInt(at + bank.config.maxSourceClockSkewMs) * 1000000n, 'Invalid book source clock');
    e.book = {marketId: b.marketId, exchangeId: b.exchangeId, instrument: b.instrument, requestStartedAt: b.requestStartedAt,
      receivedAt: b.receivedAt, sourceAt, sequence: b.sequence, versionKey: b.versionKey};
    for (const [side, direction] of [['bids', -1], ['asks', 1]]) {
      check(Array.isArray(b[side]) && b[side].length > 0 && b[side].length <= 200, 'Missing bounded depth');
      e.book[side] = b[side].map((l, i) => {
        const p = price(l.price); check(Number.isSafeInteger(l.quantity) && l.quantity > 0, 'Invalid depth quantity');
        if (i) check(direction === -1 ? p < price(b[side][i - 1].price) : p > price(b[side][i - 1].price), 'Unsorted depth');
        return {price: money(p), quantity: l.quantity};
      });
    }
    check(price(e.book.bids[0].price) < price(e.book.asks[0].price), 'Crossed or locked book');
  }
  if (e.kind === 'trade') {
    const t = input.trade; check(t?.marketId === bank.marketId && t.exchangeId === bank.exchangeId && t.tournamentId === bank.tournamentId, 'Trade scope mismatch');
    check(identity(t.id) && Number.isSafeInteger(t.quantity) && t.quantity > 0, 'Ordered stable trade identity and integer size required');
    sequence(t.sequence); const p = amount(t.price), sourceAt = timestamp(t.sourceAt);
    check(p >= 0n && p <= SCALE && instantNs(sourceAt) <= BigInt(at + bank.config.maxSourceClockSkewMs) * 1000000n, 'Invalid trade price or clock');
    e.trade = {id: t.id, sequence: t.sequence, marketId: t.marketId, exchangeId: t.exchangeId, tournamentId: t.tournamentId,
      price: money(p), quantity: t.quantity, sourceAt};
  }
  return e;
}
function censor(s, reason, at, events) {
  s.ready = false;
  for (const q of s.quotes) {
    if (!q.closedAt) { q.closedAt = at; q.closedReason = reason; }
    for (const [h, m] of Object.entries(q.markouts)) if (m.status === 'pending') q.markouts[h] = {status: 'censored_continuity', reason, at};
  }
  inc(s, 'continuityInvalidations'); events.push({kind: 'continuity_invalidated', reason, at});
}
function age(s, at, bank) {
  const now = instant(at);
  for (const q of s.quotes) {
    if (!q.closedAt && now >= instant(q.expiresAt)) { q.closedAt = q.expiresAt; q.closedReason = 'hypothesis_expired'; }
    for (const [h, m] of Object.entries(q.markouts)) if (m.status === 'pending' && now > instant(q.createdAt) + Number(h) + bank.config.markoutLatenessMs)
      q.markouts[h] = {status: 'missing_fresh_followup', at};
  }
}
function sameOrBetter(book, q) {
  return (q.side === 'bid' ? book.bids : book.asks).reduce((n, l) =>
    n + ((q.side === 'bid' ? amount(l.price) >= amount(q.price) : amount(l.price) <= amount(q.price)) ? BigInt(l.quantity) : 0n), 0n);
}
function processBook(s, e, bank, events) {
  const b = e.book, c = bank.config, at = instant(e.receivedAt), sourceNs = instantNs(b.sourceAt), previous = s.lastBook;
  if (previous) {
    check(BigInt(b.sequence) >= BigInt(previous.sequence) && sourceNs >= instantNs(previous.sourceAt), 'Book source rollback');
    if (previous.versionKey === b.versionKey) check(digest({bids: previous.bids, asks: previous.asks}) === digest({bids: b.bids, asks: b.asks}), 'Conflicting equal book version');
  }
  const independent = !previous || sourceNs > instantNs(previous.sourceAt) && b.versionKey !== previous.versionKey;
  s.lastBook = b; inc(s, 'bookObservations');
  if (!s.ready || !independent) { inc(s, !s.ready ? 'unreadyBooks' : 'repeatedBooks'); return; }
  const bid = amount(b.bids[0].price), ask = amount(b.asks[0].price);
  for (const q of s.quotes) {
    if (q.epoch !== s.epoch || at <= instant(q.createdAt) || sourceNs <= BigInt(q.sourceNs) || BigInt(b.sequence) <= BigInt(q.sequence)
      || e.continuity.revision <= q.revision || instant(b.requestStartedAt) < instant(q.activationAt)) continue;
    if (!q.closedAt) {
      const visible = sameOrBetter(b, q); q.visibleSameOrBetterNow = String(visible);
      if (visible > BigInt(q.visibleQueueAheadHighWater)) q.visibleQueueAheadHighWater = String(visible);
      const crossed = q.side === 'bid' ? ask <= amount(q.price) : bid >= amount(q.price);
      if (crossed && !q.firstOppositeQuoteTouchOrCrossAt) { q.firstOppositeQuoteTouchOrCrossAt = e.receivedAt; inc(s, 'oppositeQuoteTouchesOrCrossings'); }
      if (q.activationStatus === 'awaiting_independent_book' && at >= instant(q.activationAt)) {
        q.activationStatus = crossed ? 'marketable_at_first_followup' : 'observed_nonmarketable';
        q.activationObservedAt = e.receivedAt; q.activationSourceNs = String(sourceNs); q.activationSequence = b.sequence; q.activationRevision = e.continuity.revision;
        if (crossed) { q.closedAt = e.receivedAt; q.closedReason = 'not_passive_at_activation_observation'; }
        events.push({kind: 'activation_observation', quoteId: q.id, status: q.activationStatus, at: e.receivedAt});
      }
    }
    for (const [h, m] of Object.entries(q.markouts)) if (m.status === 'pending' && at >= instant(q.createdAt) + Number(h)) {
      const quotePrice = amount(q.price), slip = BigInt(c.slippageTicks) * TICK;
      const midpointMoveTwice = q.side === 'bid' ? bid + ask - 2n * quotePrice : 2n * quotePrice - bid - ask;
      const executableSideMove = q.side === 'bid' ? bid - slip - quotePrice : quotePrice - ask - slip;
      q.markouts[h] = {status: 'observed_unexecuted_quote_markout', at: e.receivedAt, sourceVersion: b.versionKey,
        latenessMs: at - instant(q.createdAt) - Number(h), signedMidpointMove: money(midpointMoveTwice / 2n),
        signedOppositeExecutableMoveAfterCushion: money(executableSideMove),
        afterQuoteCross: q.firstOppositeQuoteTouchOrCrossAt !== null, afterQualifiedTradeThrough: q.firstQualifiedTradeThroughAt !== null};
      inc(s, 'observedMarkouts'); events.push({kind: 'quote_markout', quoteId: q.id, horizonMs: Number(h), ...q.markouts[h]});
    }
  }
  if (at <= instant(bank.cutoff) || instant(b.requestStartedAt) <= instant(bank.cutoff)) { inc(s, 'developmentBooks'); return; }
  if (s.lastQuoteAt && at - instant(s.lastQuoteAt) < c.quoteEveryMs) return;
  if (s.pairs >= c.maxQuotePairs) { inc(s, 'quoteCapacityReached'); return; }
  const insideBid = bid + TICK, insideAsk = ask - TICK;
  if (ask - bid < 2n * TICK || insideBid >= insideAsk || insideBid < TICK || insideAsk > SCALE - TICK) { inc(s, 'noDistinctInteriorPrices'); return; }
  for (const [side, p] of [['bid', insideBid], ['ask', insideAsk]]) {
    const q = {id: digest({bankHash: s.bankHash, version: b.versionKey, at: e.receivedAt, side}), side, price: money(p), quantity: c.orderQuantity,
      createdAt: e.receivedAt, activationAt: new Date(at + c.latencyMs).toISOString(), expiresAt: new Date(at + c.quoteLifetimeMs).toISOString(),
      epoch: s.epoch, revision: e.continuity.revision, sequence: b.sequence, sourceNs: String(sourceNs), sourceVersion: b.versionKey,
      activationStatus: 'awaiting_independent_book', activationObservedAt: null, closedAt: null, closedReason: null,
      visibleSameOrBetterNow: '0', visibleQueueAheadHighWater: '0', qualifiedTradeThroughQuantity: '0', qualifiedTradeThroughEvents: 0,
      firstOppositeQuoteTouchOrCrossAt: null, firstQualifiedTradeThroughAt: null,
      markouts: Object.fromEntries(c.horizonsMs.map(h => [h, {status: 'pending'}]))};
    q.visibleSameOrBetterNow = q.visibleQueueAheadHighWater = String(sameOrBetter(b, q));
    s.quotes.push(q); events.push({kind: 'hypothetical_quote', quoteId: q.id, side, price: q.price, quantity: q.quantity,
      at: e.receivedAt, activationAt: q.activationAt, visibleQueueAhead: q.visibleQueueAheadHighWater, executionClaim: 'none'});
  }
  s.pairs++; s.lastQuoteAt = e.receivedAt; inc(s, 'quotePairs');
}
function processTrade(s, e, bank, events) {
  const t = e.trade, key = digest(t), old = s.seenTrades[t.id];
  if (old) { check(old === key, 'Conflicting duplicate trade'); inc(s, 'duplicateTrades'); return; }
  if (Object.keys(s.seenTrades).length >= bank.config.maxTradeIds) { censor(s, 'trade_identity_capacity', e.receivedAt, events); return; }
  s.seenTrades[t.id] = key; inc(s, 'uniqueTradesObserved');
  const ns = instantNs(t.sourceAt);
  if (s.lastTradeSequence !== null && (BigInt(t.sequence) <= BigInt(s.lastTradeSequence) || ns < BigInt(s.lastTradeSourceNs))) {
    censor(s, 'trade_ordering_ambiguous', e.receivedAt, events); return;
  }
  s.lastTradeSequence = t.sequence; s.lastTradeSourceNs = String(ns);
  if (!s.ready || !e.continuity.continuous) { inc(s, 'unqualifiedTrades'); return; }
  if (!s.lastBook || instant(e.receivedAt) - instant(s.lastBook.receivedAt) > bank.config.maxDataAgeMs) {
    censor(s, 'stale_book', e.receivedAt, events); inc(s, 'unqualifiedTrades'); return;
  }
  for (const q of s.quotes) {
    if (q.closedAt || q.epoch !== s.epoch || q.activationStatus !== 'observed_nonmarketable'
      || instant(e.receivedAt) <= instant(q.activationObservedAt) || ns <= BigInt(q.activationSourceNs)
      || BigInt(t.sequence) <= BigInt(q.activationSequence) || e.continuity.revision <= q.activationRevision) continue;
    const traded = amount(t.price), limit = amount(q.price);
    if (traded === limit) { inc(s, 'qualifiedTradeTouches'); continue; }
    if (!(q.side === 'bid' ? traded < limit : traded > limit)) continue;
    q.qualifiedTradeThroughQuantity = String(BigInt(q.qualifiedTradeThroughQuantity) + BigInt(t.quantity));
    q.qualifiedTradeThroughEvents++; q.firstQualifiedTradeThroughAt ??= e.receivedAt; inc(s, 'qualifiedTradeThroughEvents');
    events.push({kind: 'trade_through_evidence', quoteId: q.id, tradeId: t.id, at: e.receivedAt, observedQuantity: t.quantity,
      visibleQueueAheadHighWater: q.visibleQueueAheadHighWater, executionClaim: 'unverified', queueConsumed: false});
  }
}
export function observePassive(input, event, bank) {
  assertState(input, bank); const e = normalized(event, bank), inputHash = digest(e);
  if (e.index === input.index) { check(inputHash === input.lastInputHash, 'Conflicting replay index'); return {state: input, events: [], record: null}; }
  check(e.index === input.index + 1, 'Missing or reordered observer event');
  check(input.lastAt === null || instant(e.receivedAt) >= instant(input.lastAt), 'Receipt rollback');
  const s = structuredClone(input), events = [], co = e.continuity;
  const epochChanged = s.epoch !== null && s.epoch !== co.epoch;
  // A merged delivery may overlap revisions already received. Only a start
  // strictly beyond the watermark proves a forward gap; overlap is continuous.
  const revisionGap = s.revision !== null && !epochChanged && (co.revision < s.revision || co.revision > s.revision && co.previousRevision > s.revision);
  if (epochChanged || revisionGap || !co.ready || !co.continuous || e.kind === 'invalidate') censor(s, e.reason || (epochChanged ? 'epoch_changed' : revisionGap ? 'delivery_gap' : 'not_continuous'), e.receivedAt, events);
  s.epoch = co.epoch; s.revision = co.revision;
  if (e.kind === 'book' && co.recovered && co.ready && co.continuous) s.ready = true;
  age(s, e.receivedAt, bank);
  if (e.kind === 'book') processBook(s, e, bank, events);
  if (e.kind === 'trade') processTrade(s, e, bank, events);
  const record = {index: e.index, previousHash: s.chainHash, event: e}; record.hash = digest(record);
  s.index = e.index; s.chainHash = record.hash; s.lastInputHash = inputHash; s.lastAt = e.receivedAt;
  return {state: s, events, record};
}
export function passiveReport(state, bank) {
  assertState(state, bank);
  const markouts = {};
  for (const q of state.quotes) for (const [h, m] of Object.entries(q.markouts)) {
    markouts[h] ??= {}; markouts[h][m.status] = (markouts[h][m.status] || 0) + 1;
  }
  return {mode: bank.mode, name: bank.name, cutoff: bank.cutoff, sourceHash: bank.sourceHash, bankHash: state.bankHash,
    configHash: bank.configHash, throughIndex: state.index, chainHash: state.chainHash, updatedAt: state.lastAt,
    ready: state.ready, quotePairs: state.pairs, counts: structuredClone(state.counts), markoutStatuses: markouts,
    execution: {ordersSubmitted: 0, simulatedFills: null, partialFillQuantity: null, fillProbability: null, pnl: null},
    evidence: {localContinuityOnly: true, completeHistoricalTradeTape: false, aggressorSideKnown: false, queuePriorityKnown: false,
      visibleQueueNeverDecrementedByDisappearanceOrTrade: true, quoteCrossingIsNotFill: true, profitabilityEstablished: false}};
}
export function replayPassive(records, bank) {
  check(Array.isArray(records), 'Observer journal records required');
  const created = createPassiveObserver(bank);
  check(digest(created.bank) === digest(bank), 'Unexpected observer bank fields');
  let state = created.state;
  for (const record of records) {
    const out = observePassive(state, record?.event, bank);
    check(out.record !== null && digest(out.record) === digest(record), 'Observer journal hash or payload mismatch');
    state = out.state;
  }
  return state;
}
