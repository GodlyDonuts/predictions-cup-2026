import {isDeepStrictEqual} from 'node:util';
import {amount, money, price, instant, sequence, fee} from './fixed.mjs';
import {validateConfig, validateEvent} from './schema.mjs';

const live = o => ['pending', 'open', 'pending_cancel'].includes(o.status);
const eligible = o => ['open', 'pending_cancel'].includes(o.status) && o.activatedAt !== null;
const min = (a, b) => a < b ? a : b;

/** Offline, long-only, isolated per-outcome instrument replay. No networking imports. */
export class PaperEngine {
  constructor(config) {
    this.config = validateConfig(config); this.initialCash = amount(config.initialCash); this.cash = this.initialCash;
    this.books = new Map(); this.orders = new Map(); this.positions = new Map(); this.marks = new Map(); this.seen = new Map(); this.tradeEvidence = new Map();
    this.now = null; this.actions = []; this.ordinal = 0; this.fills = []; this.audit = [];
    this.realized = 0n; this.totalFees = 0n; this.turnover = 0n; this.submittedShares = 0n; this.filledShares = 0n;
    this.highWater = this.initialCash; this.maxDrawdown = 0n; this.equityObservations = 0; this.incompleteEquityObservations = 0;
    this.stats = {duplicates: 0, staleVersions: 0, staleEvidence: 0, gaps: 0, rejectedOrders: 0, ownAggressorExcluded: 0, simulatedWriteRequests: 0};
  }
  note(reason, details = {}) { this.audit.push({at: new Date(this.now).toISOString(), reason, ...details}); }
  quantity(instrument) { return (this.positions.get(instrument) || []).reduce((n, l) => n + l.quantity, 0); }
  basis(instrument) { return (this.positions.get(instrument) || []).reduce((n, l) => n + l.cost, 0n); }
  reserveFor(o) {
    // Per-share fee rounding upper-bounds every possible partition into partial fills.
    const unitFee = fee(o.limit, 1, this.config.costs);
    const unit = o.action === 'buy' ? o.limit + unitFee : unitFee > o.limit ? unitFee - o.limit : 0n;
    return BigInt(o.remaining) * unit;
  }
  reservedCash() { return [...this.orders.values()].filter(live).reduce((n, o) => n + this.reserveFor(o), 0n); }
  reservedShares(instrument) { return [...this.orders.values()].filter(o => live(o) && o.instrument === instrument && o.action === 'sell').reduce((n, o) => n + o.remaining, 0); }
  bookFresh(instrument, at = this.now) {
    const b = this.books.get(instrument);
    return b && !b.invalid && at >= b.received && at - b.received <= this.config.maxDataAgeMs &&
      at - b.occurred <= this.config.maxDataAgeMs && (b.expiry === null || at < b.expiry);
  }
  schedule(at, kind, order) {
    if (!Number.isSafeInteger(at)) throw new Error('Scheduled timestamp overflow');
    this.actions.push({at, kind, order, ordinal: this.ordinal++});
  }
  advance(at) {
    if (this.now !== null && at < this.now) throw new Error('Replay must be chronological by receivedAt');
    const priority = {expire: 0, cancel: 1, activate: 2};
    this.actions.sort((a, b) => a.at - b.at || priority[a.kind] - priority[b.kind] || a.ordinal - b.ordinal);
    while (this.actions.length && this.actions[0].at <= at) {
      const a = this.actions.shift(); this.now = a.at; const o = this.orders.get(a.order);
      if (!live(o)) continue;
      if (a.kind === 'expire') this.close(o, 'expired');
      else if (a.kind === 'cancel') this.close(o, 'cancelled');
      else if (o.activatedAt === null) this.activate(o);
    }
    this.now = at;
  }
  close(o, status) { o.status = status; o.terminalAt = this.now; this.note(status, {orderId: o.id}); }
  reject(o, reason) { o.status = 'rejected'; o.reason = reason; o.terminalAt = this.now; this.stats.rejectedOrders++; this.note(reason, {orderId: o.id}); }
  normalizeQueue(instrument, action, limit) {
    let floor = 0n;
    for (const o of [...this.orders.values()].filter(x => eligible(x) && x.instrument === instrument && x.action === action && x.limit === limit).sort((a, b) => a.ordinal - b.ordinal)) {
      if (o.queueAhead < floor) o.queueAhead = floor;
      floor = o.queueAhead + BigInt(o.remaining);
    }
  }
  activate(o) {
    if (!this.bookFresh(o.instrument)) return this.reject(o, 'book_not_fresh_at_activation');
    const b = this.books.get(o.instrument); const opposing = o.action === 'buy' ? b.asks : b.bids;
    // This implementation deliberately supports resting orders only. Crossing is rejected,
    // not represented as participant post-only/IOC support or an assumed taker fill.
    if ([...opposing.entries()].some(([p, level]) => level.total > 0 && (o.action === 'buy' ? p <= o.limit : p >= o.limit))) return this.reject(o, 'marketable_order_unsupported');
    if ([...this.orders.values()].some(x => eligible(x) && x.instrument === o.instrument && x.action !== o.action &&
      (o.action === 'buy' ? x.limit <= o.limit : x.limit >= o.limit))) return this.reject(o, 'crosses_own_paper_order');
    const same = o.action === 'buy' ? b.bids : b.asks;
    const earlier = [...this.orders.values()].filter(x => x !== o && eligible(x) && x.instrument === o.instrument && x.action === o.action && x.limit === o.limit);
    o.queueAhead = BigInt(same.get(o.limit)?.total || 0) + earlier.reduce((n, x) => n + BigInt(x.remaining), 0n);
    o.activatedAt = this.now; if (o.status !== 'pending_cancel') o.status = 'open';
    this.normalizeQueue(o.instrument, o.action, o.limit);
    this.note('activated', {orderId: o.id, queueAhead: String(o.queueAhead)});
  }
  submit(e) {
    if (this.orders.has(e.orderId)) throw new Error('Order ID already used; only identical eventId replay is idempotent');
    const o = {id: e.orderId, instrument: e.instrument, action: e.action, quantity: e.quantity, remaining: e.quantity,
      limit: price(e.limitPrice), status: 'pending', requestedAt: this.now, activatedAt: null, terminalAt: null,
      expiresAt: e.expiresAt === null ? null : instant(e.expiresAt), queueAhead: 0n, ordinal: this.ordinal++};
    this.stats.simulatedWriteRequests++; this.submittedShares += BigInt(e.quantity);
    const reject = reason => { this.orders.set(o.id, o); this.reject(o, reason); };
    if (!this.bookFresh(o.instrument)) return reject('book_not_fresh_at_submission');
    if (o.action === 'buy') {
      const incoming = [...this.orders.values()].filter(x => live(x) && x.action === 'buy' && x.instrument === o.instrument).reduce((n, x) => n + BigInt(x.remaining), 0n);
      if (BigInt(this.quantity(o.instrument)) + incoming + BigInt(o.quantity) > BigInt(this.config.maxPositionShares)) return reject('inventory_limit');
      if (this.cash - this.reservedCash() < this.reserveFor(o)) return reject('insufficient_unreserved_cash');
    } else {
      if (this.quantity(o.instrument) - this.reservedShares(o.instrument) < o.quantity) return reject('insufficient_unreserved_inventory');
      if (this.cash - this.reservedCash() < this.reserveFor(o)) return reject('insufficient_unreserved_cash_for_sell_fees');
    }
    this.orders.set(o.id, o);
    this.schedule(this.now + this.config.orderLatencyMs, 'activate', o.id);
    if (o.expiresAt !== null) this.schedule(o.expiresAt, 'expire', o.id);
    this.advance(this.now);
  }
  cancel(e) {
    const o = this.orders.get(e.orderId);
    if (!o) throw new Error('Cannot cancel unknown order');
    if (!live(o) || o.status === 'pending_cancel') { this.note('cancel_noop', {orderId: o.id}); return; }
    this.stats.simulatedWriteRequests++; o.status = 'pending_cancel'; o.cancelRequestedAt = this.now;
    this.schedule(this.now + this.config.cancelLatencyMs, 'cancel', o.id); this.advance(this.now);
  }
  snapshot(e) {
    const old = this.books.get(e.instrument); const seq = sequence(e.sequence); const occurred = instant(e.occurredAt);
    if (old && (seq < old.watermark || (seq === old.watermark && occurred < old.watermarkTime))) { this.stats.staleVersions++; return; }
    if (old && occurred < old.watermarkTime) {
      old.invalid = true; this.stats.gaps++; this.note('backward_snapshot_time_requires_resync', {eventId: e.eventId}); return;
    }
    if (old && seq === old.sequence && occurred === old.occurred) {
      const content = x => ({instrument: x.instrument, sequence: x.sequence, occurredAt: x.occurredAt, bids: x.bids, asks: x.asks, nextExpiryAt: x.nextExpiryAt});
      if (!isDeepStrictEqual(content(old.source), content(e))) throw new Error('Conflicting snapshot at identical version');
      this.stats.staleVersions++; return;
    }
    if ((!old || old.invalid) && !e.authoritative) { this.note('authoritative_snapshot_required', {instrument: e.instrument}); return; }
    const make = levels => new Map(levels.map(l => [price(l.price), {external: l.quantity - l.ownQuantity, total: l.quantity}]));
    const b = {sequence: seq, watermark: seq, watermarkTime: occurred, received: this.now, occurred,
      expiry: e.nextExpiryAt === null ? null : instant(e.nextExpiryAt), invalid: false, bids: make(e.bids), asks: make(e.asks), source: structuredClone(e)};
    this.books.set(e.instrument, b);
    // Disappearing/cancelled depth is never inferred to have traded. Replenishment is
    // pessimistically placed ahead; historical order-level priority is unavailable.
    for (const o of this.orders.values()) {
      if (!eligible(o) || o.instrument !== e.instrument) continue;
      const same = o.action === 'buy' ? b.bids : b.asks;
      const earlier = [...this.orders.values()].filter(x => eligible(x) && x.ordinal < o.ordinal && x.instrument === o.instrument && x.action === o.action && x.limit === o.limit);
      const ahead = BigInt(same.get(o.limit)?.total || 0) + earlier.reduce((n, x) => n + BigInt(x.remaining), 0n);
      if (ahead > o.queueAhead) o.queueAhead = ahead;
      this.normalizeQueue(o.instrument, o.action, o.limit);
    }
  }
  fill(o, quantity, e) {
    const notional = o.limit * BigInt(quantity); const charge = fee(notional, quantity, this.config.costs);
    if (o.action === 'buy') {
      this.cash -= notional + charge;
      if (!this.positions.has(o.instrument)) this.positions.set(o.instrument, []);
      this.positions.get(o.instrument).push({quantity, cost: notional + charge});
    } else {
      this.cash += notional - charge;
      let left = quantity, basis = 0n; const lots = this.positions.get(o.instrument);
      while (left > 0) {
        const lot = lots[0]; if (!lot) throw new Error('Inventory invariant violated');
        const n = Math.min(left, lot.quantity);
        // FIFO basis allocation floors partial micro-units and leaves the remainder
        // in the lot, guaranteeing exact conservation on full liquidation.
        const used = n === lot.quantity ? lot.cost : lot.cost * BigInt(n) / BigInt(lot.quantity);
        lot.cost -= used; lot.quantity -= n; basis += used; left -= n; if (!lot.quantity) lots.shift();
      }
      this.realized += notional - charge - basis;
    }
    o.remaining -= quantity; this.totalFees += charge; this.turnover += notional; this.filledShares += BigInt(quantity);
    this.fills.push({orderId: o.id, eventId: e.eventId, instrument: o.instrument, action: o.action, quantity,
      price: money(o.limit), notional: money(notional), fee: money(charge), occurredAt: e.occurredAt, receivedAt: e.receivedAt,
      model: 'explicit_trade_pessimistic_queue'});
    if (o.remaining === 0) this.close(o, 'filled');
    if (this.cash < 0n || this.cash < this.reservedCash()) throw new Error('Cash reservation invariant violated');
  }
  trade(e) {
    const b = this.books.get(e.instrument); const seq = sequence(e.sequence); const occurred = instant(e.occurredAt);
    const p = price(e.price); const action = e.aggressor === 'sell' ? 'buy' : 'sell';
    const evidenceKey = `${e.instrument}/${e.sequence}`;
    const evidence = {occurredAt: e.occurredAt, price: e.price, quantity: e.quantity, aggressor: e.aggressor,
      bookSequence: e.bookSequence, externalAggressor: e.externalAggressor};
    if (this.tradeEvidence.has(evidenceKey)) {
      if (!isDeepStrictEqual(this.tradeEvidence.get(evidenceKey), evidence)) throw new Error('Conflicting trade evidence sequence');
      this.stats.staleVersions++; return;
    }
    // A delayed pre-cancel/expiry print could invalidate previously released resources.
    // Refuse an apparently complete result; an adapter must supply ordered evidence.
    for (const o of this.orders.values()) {
      if (e.externalAggressor && o.instrument === e.instrument && o.action === action && o.limit === p && o.remaining > 0 &&
          ['cancelled', 'expired'].includes(o.status) && o.activatedAt !== null && occurred > o.activatedAt && occurred < o.terminalAt) {
        throw new Error('Ambiguous late cancel/expiry fill race; ordered recovery evidence required');
      }
    }
    if (b && seq <= b.watermark) { this.stats.staleVersions++; return; }
    if (!this.bookFresh(e.instrument) || this.now - occurred > this.config.maxDataAgeMs) { this.stats.staleEvidence++; this.note('stale_trade_evidence', {eventId: e.eventId}); return; }
    if (sequence(e.bookSequence) !== b.sequence || occurred < b.watermarkTime) {
      b.invalid = true; this.stats.gaps++; this.note('trade_book_mismatch_requires_resync', {eventId: e.eventId}); return;
    }
    this.tradeEvidence.set(evidenceKey, evidence);
    b.watermark = seq; b.watermarkTime = occurred;
    const side = action === 'buy' ? b.bids : b.asks; const level = side.get(p);
    if (level) { level.total = Math.max(0, level.total - e.quantity); level.external = Math.max(0, level.external - e.quantity); }
    if (!e.externalAggressor) { this.stats.ownAggressorExcluded++; return; }
    this.normalizeQueue(e.instrument, action, p);
    const orders = [...this.orders.values()].filter(o => eligible(o) && o.instrument === e.instrument && o.action === action && o.limit === p && occurred > o.activatedAt).sort((a, z) => a.ordinal - z.ordinal);
    for (const o of orders) {
      const volume = BigInt(e.quantity); const blocked = min(volume, o.queueAhead); o.queueAhead -= blocked;
      const filled = Number(min(volume - blocked, BigInt(o.remaining)));
      if (filled > 0) this.fill(o, filled, e);
    }
  }
  apply(event) {
    if (this.failed) throw new Error('Engine is failed closed; start a new replay after fixing the dataset');
    try { return this.applyValidated(event); }
    catch (error) { this.failed = true; throw error; }
  }
  applyValidated(event) {
    const e = validateEvent(structuredClone(event));
    if (this.seen.has(e.eventId)) {
      if (!isDeepStrictEqual(this.seen.get(e.eventId), e)) throw new Error('Conflicting duplicate event ID');
      this.stats.duplicates++; return;
    }
    this.advance(instant(e.receivedAt));
    if (e.type === 'snapshot') this.snapshot(e);
    if (e.type === 'trade') this.trade(e);
    if (e.type === 'submit') this.submit(e);
    if (e.type === 'cancel') this.cancel(e);
    if (e.type === 'gap') {
      const b = this.books.get(e.instrument); if (b) b.invalid = true;
      this.stats.gaps++; this.note('gap_requires_authoritative_resync', {instrument: e.instrument, detail: e.reason});
    }
    if (e.type === 'mark') {
      const key = `${e.instrument}/${e.source}`; const old = this.marks.get(key);
      if (!old || instant(e.occurredAt) > instant(old.occurredAt)) this.marks.set(key, e);
    }
    this.seen.set(e.eventId, e);
    const valuation = this.valuation();
    if (valuation.referenceEquity !== null) {
      const equity = amount(valuation.referenceEquity); if (equity > this.highWater) this.highWater = equity;
      if (this.highWater - equity > this.maxDrawdown) this.maxDrawdown = this.highWater - equity;
      this.equityObservations++;
    } else this.incompleteEquityObservations++;
  }
  valuation() {
    let refTotal = 0n, refComplete = true, executable = 0n, depthComplete = true, remainingBasis = 0n, feeFundingRequired = 0n;
    const positions = [];
    for (const [instrument] of this.positions) {
      const quantity = this.quantity(instrument); if (!quantity) continue;
      const basis = this.basis(instrument); remainingBasis += basis;
      const marks = {};
      for (const source of ['reference', 'last', 'tournament']) {
        const mark = this.marks.get(`${instrument}/${source}`);
        marks[source] = mark ? {price: mark.price, occurredAt: mark.occurredAt, receivedAt: mark.receivedAt,
          stale: this.now - instant(mark.occurredAt) > this.config.maxDataAgeMs, value: money(amount(mark.price) * BigInt(quantity))} : null;
      }
      const ref = marks.reference; if (!ref || ref.stale) refComplete = false; else refTotal += amount(ref.value);
      let unsold = quantity, gross = 0n, costs = 0n; const levels = [];
      if (this.bookFresh(instrument)) {
        for (const [p, level] of [...this.books.get(instrument).bids].sort((a, b) => a[0] > b[0] ? -1 : 1)) {
          const q = Math.min(unsold, level.external); if (!q) continue;
          const notional = p * BigInt(q); const charge = fee(p, 1, this.config.costs) * BigInt(q);
          gross += notional; costs += charge; unsold -= q; levels.push({price: money(p), quantity: q}); if (!unsold) break;
        }
      }
      const proceeds = gross - costs; executable += proceeds; if (unsold) depthComplete = false;
      if (proceeds < 0n) feeFundingRequired -= proceeds;
      positions.push({instrument, quantity, costBasis: money(basis), marks,
        referenceUnrealizedPnl: ref && !ref.stale ? money(amount(ref.value) - basis) : null,
        executable: {bookFresh: Boolean(this.bookFresh(instrument)), estimatedNetProceeds: money(proceeds), estimatedFees: money(costs),
          soldQuantity: quantity - unsold, unsoldQuantity: unsold, fullPositionValue: unsold === 0 ? money(proceeds) : null, levels}});
    }
    const availableCash = this.cash - this.reservedCash();
    const feeFundingShortfall = feeFundingRequired > availableCash ? feeFundingRequired - availableCash : 0n;
    const executableComplete = depthComplete && feeFundingShortfall === 0n;
    return {positions, remainingCostBasis: money(remainingBasis), referenceEquity: refComplete ? money(this.cash + refTotal) : null,
      referenceUnrealizedPnl: refComplete ? money(refTotal - remainingBasis) : null,
      estimatedExecutableEquity: executableComplete ? money(this.cash + executable) : null,
      partialExecutableCash: money(this.cash + executable), executableComplete, depthComplete,
      estimatedLiquidationFeeFundingRequired: money(feeFundingRequired), liquidationFeeFundingShortfall: money(feeFundingShortfall)};
  }
  report() {
    if (this.failed) throw new Error('Engine is failed closed; no valid completed report');
    const valuation = this.valuation();
    return {schemaVersion: 1, mode: 'offline_paper_only', asOf: this.now === null ? null : new Date(this.now).toISOString(),
      assumptions: {fillModel: 'explicit_trade_pessimistic_queue', costs: this.config.costs,
        noProfitabilityClaim: true, syntheticUntilRealDatasetValidated: true, liveExecutionSupported: false,
        eligibleForStrategyPromotion: false, queuePessimismIsNotAPnlLowerBound: true},
      cash: money(this.cash), reservedCash: money(this.reservedCash()), realizedPnl: money(this.realized), totalFees: money(this.totalFees),
      ...valuation, orders: [...this.orders.values()].map(o => ({orderId: o.id, instrument: o.instrument, action: o.action,
        quantity: o.quantity, remaining: o.remaining, limitPrice: money(o.limit), status: o.status, reason: o.reason || null,
        queueAhead: String(o.queueAhead)})), fills: this.fills,
      metrics: {...this.stats, submittedShares: String(this.submittedShares), filledShares: String(this.filledShares),
        fillRatio: this.submittedShares ? `${this.filledShares}/${this.submittedShares}` : null,
        turnover: money(this.turnover), observedReferenceMaxDrawdown: money(this.maxDrawdown),
        equityObservations: this.equityObservations, incompleteEquityObservations: this.incompleteEquityObservations,
        drawdownComplete: this.incompleteEquityObservations === 0,
        realizedPnlPerWriteRequest: this.stats.simulatedWriteRequests ? {numerator: money(this.realized), denominator: this.stats.simulatedWriteRequests} : null,
        sharpe: null, adverseSelection: null, fillCalibration: null,
        unavailableMetricsReason: 'No regular-return series, fixed-horizon post-fill labels, or authoritative real order/fill calibration dataset'},
      noTradeBaseline: {cash: money(this.initialCash), pnl: money(0n)}, audit: this.audit};
  }
}

export function replay(events, config) {
  const engine = new PaperEngine(config); for (const event of events) engine.apply(event); return engine.report();
}
