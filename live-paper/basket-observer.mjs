import {createHash} from 'node:crypto';
import {amount, money, price, instant, sequence, SCALE, TICK} from '../paper/fixed.mjs';
import {instantNs, timestamp} from '../feed/contract.mjs';

// Measurement only: no I/O, orders, portfolio accounting or simulated fills.
// Inputs are model-normalized authoritative REST books, plus tournamentId.
export const BASKET_POLICY = Object.freeze({name: 'dual-no-measurement-v1', depthFractionBps: 100,
  totalBudget: '25.000000', maxBasketQuantity: 100, adverseTicksPerLeg: 1,
  maxDataAgeMs: 20000, maxReceiptSkewMs: 20000, maxSourceClockSkewMs: 2000, latencyMs: 1000});
const canonical = x => Array.isArray(x) ? '[' + x.map(canonical).join(',') + ']' : x && typeof x === 'object'
  ? '{' + Object.keys(x).sort().map(k => JSON.stringify(k) + ':' + canonical(x[k])).join(',') + '}' : JSON.stringify(x);
const hash = x => createHash('sha256').update(canonical(x)).digest('hex');
const need = (x, message) => { if (!x) throw Error(message); };
const hex = x => typeof x === 'string' && /^[a-f0-9]{64}$/.test(x);
const id = x => typeof x === 'string' && /^[1-9]\d*$/.test(x) && Number.isSafeInteger(Number(x));
const uuid = x => typeof x === 'string' && /^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/.test(x);
const fields = (x, keys) => x && typeof x === 'object' && !Array.isArray(x) && Object.keys(x).length === keys.length && keys.every(k => Object.hasOwn(x, k));
function policy(overrides = {}) {
  need(overrides && Object.keys(overrides).every(k => Object.hasOwn(BASKET_POLICY, k)), 'Unknown basket policy');
  const p = {...BASKET_POLICY, ...overrides};
  need(p.name === BASKET_POLICY.name && amount(p.totalBudget) > 0n && amount(p.totalBudget) <= 25000000n, 'Unsafe basket budget');
  for (const k of ['depthFractionBps', 'maxBasketQuantity', 'adverseTicksPerLeg', 'maxDataAgeMs', 'maxReceiptSkewMs', 'latencyMs'])
    need(Number.isSafeInteger(p[k]) && p[k] > 0, 'Invalid basket policy');
  need(p.depthFractionBps <= 100 && p.maxBasketQuantity <= 100 && p.adverseTicksPerLeg <= 5
    && p.maxDataAgeMs <= 20000 && p.maxReceiptSkewMs <= p.maxDataAgeMs && p.latencyMs >= 1000 && p.latencyMs <= 60000
    && Number.isSafeInteger(p.maxSourceClockSkewMs) && p.maxSourceClockSkewMs >= 0 && p.maxSourceClockSkewMs <= 2000, 'Unsafe basket policy');
  return p;
}
function proof(input, tournamentId, legs, cutoff) {
  if (input === undefined || input === null) return {status: 'unverified', reason: 'metadata_only'};
  if (input.status === 'unverified') {
    need(fields(input, ['status', 'reason']) && ['metadata_only', 'missing_authoritative_relationship', 'unverified_settlement_rules'].includes(input.reason), 'Invalid unverified proof');
    return structuredClone(input);
  }
  need(fields(input, ['status', 'tournamentId', 'legs', 'relationship', 'minimumCombinedNoPayout', 'normalResolutionOnly', 'refundFloorVerified', 'evidenceHash', 'verifiedAt'])
    && input.status === 'verified' && input.tournamentId === tournamentId && hash(input.legs) === hash(legs)
    && input.minimumCombinedNoPayout === '1.000000' && input.normalResolutionOnly === true && input.refundFloorVerified === false
    && hex(input.evidenceHash) && instant(input.verifiedAt) <= instant(cutoff), 'Invalid scoped settlement proof');
  const r = input.relationship;
  need(fields(r, ['id', 'version', 'status', 'type', 'isExhaustive', 'members']) && uuid(r.id) && Number.isSafeInteger(r.version) && r.version > 0
    && r.status === 'active' && r.type === 'mutually_exclusive' && typeof r.isExhaustive === 'boolean'
    && Array.isArray(r.members) && r.members.length === 2, 'Invalid relationship proof');
  for (let i = 0; i < 2; i++) need(fields(r.members[i], ['marketId', 'exchangeId', 'outcome', 'role'])
    && r.members[i].marketId === legs[i].marketId && r.members[i].exchangeId === legs[i].exchangeId
    && r.members[i].outcome === 'YES' && r.members[i].role === 'member', 'Relationship member mismatch');
  return structuredClone(input);
}
export function createBasketObserver({tournamentId, legs, sourceHash, cutoff, settlementProof, policy: overrides = {}}) {
  need(uuid(tournamentId) && hex(sourceHash), 'Invalid frozen basket scope'); instant(cutoff);
  need(Array.isArray(legs) && legs.length === 2 && legs.every(x => fields(x, ['marketId', 'exchangeId', 'predicateHash'])
    && id(x.marketId) && id(x.exchangeId) && hex(x.predicateHash)) && legs[0].marketId !== legs[1].marketId
    && legs[0].exchangeId !== legs[1].exchangeId && legs[0].predicateHash !== legs[1].predicateHash, 'Invalid scoped pair identity');
  const p = policy(overrides), bank = {schemaVersion: 1, name: 'dual-no-basket-observer-v1', tournamentId, legs: structuredClone(legs), sourceHash, cutoff,
    policy: p, policyHash: hash(p), settlementProof: proof(settlementProof, tournamentId, legs, cutoff)};
  return {bank, state: {schemaVersion: 1, bankHash: hash(bank), lastAt: null, books: {}, lastSeen: {}, invalidatedAt: {}, lastPairHash: null,
    positiveAnchor: null, positivePersistenceCount: 0, counts: {receipts: 0, cofreshPairs: 0, apparentPositivePairs: 0, independentPositiveConfirmations: 0, invalidations: 0}, lastObservation: null}};
}
function checkState(s, bank) {
  need(bank?.name === 'dual-no-basket-observer-v1' && bank.policyHash === hash(policy(bank.policy)) && s?.schemaVersion === 1 && s.bankHash === hash(bank), 'Frozen basket bank changed');
  need(s.books && s.invalidatedAt && s.counts && Object.values(s.counts).every(x => Number.isSafeInteger(x) && x >= 0)
    && Number.isSafeInteger(s.positivePersistenceCount) && s.positivePersistenceCount >= 0, 'Invalid basket state');
}
function checkedBook(input, bank, s) {
  const leg = bank.legs.find(x => x.marketId === input?.marketId), p = bank.policy;
  need(leg && input.tournamentId === bank.tournamentId && input.exchangeId === leg.exchangeId
    && input.instrument === leg.exchangeId + ':YES' && input.freshnessPolicy === 'authoritative_receipt_v1', 'Foreign or nonauthoritative basket book');
  const received = instant(input.receivedAt), started = instant(input.requestStartedAt), source = instantNs(input.sourceAt), seq = sequence(input.sequence);
  need(started <= received && received - started <= p.maxDataAgeMs && (s.lastAt === null || received >= instant(s.lastAt))
    && source <= (BigInt(received) + BigInt(p.maxSourceClockSkewMs)) * 1000000n, 'Invalid basket receipt/source clocks');
  need(input.versionKey === input.sequence + ':' + timestamp(input.sourceAt), 'Basket version mismatch');
  const old = s.lastSeen[leg.exchangeId];
  if (old) need(started >= instant(old.requestStartedAt) && received >= instant(old.receivedAt)
    && seq >= sequence(old.sequence) && source >= instantNs(old.sourceAt), 'Basket clock/version rollback');
  const out = {tournamentId: bank.tournamentId, marketId: leg.marketId, exchangeId: leg.exchangeId, instrument: input.instrument,
    receivedAt: input.receivedAt, requestStartedAt: input.requestStartedAt, sourceAt: timestamp(input.sourceAt), sequence: input.sequence,
    versionKey: input.versionKey, freshnessPolicy: input.freshnessPolicy};
  for (const [side, direction] of [['bids', -1], ['asks', 1]]) {
    need(Array.isArray(input[side]) && input[side].length > 0 && input[side].length <= 1000, 'Missing basket depth');
    let previous;
    out[side] = input[side].map(l => { const v = price(l.price); need(Number.isSafeInteger(l.quantity) && l.quantity > 0, 'Invalid basket quantity');
      if (previous !== undefined) need(direction < 0 ? v < previous : v > previous, 'Unsorted basket depth'); previous = v;
      return {price: money(v), quantity: l.quantity}; });
  }
  need(price(out.bids[0].price) < price(out.asks[0].price), 'Crossed basket book');
  out.contentHash = hash({bids: out.bids, asks: out.asks});
  if (old?.versionKey === out.versionKey) need(old.contentHash === out.contentHash, 'Changed depth under unchanged basket version');
  return out;
}
function walk(books, p) {
  const ladders = books.map(book => book.bids.map(l => ({yesBid: l.price, cost: SCALE - price(l.price) + TICK * BigInt(p.adverseTicksPerLeg),
    quantity: Number(BigInt(l.quantity) * BigInt(p.depthFractionBps) / 10000n)})).filter(l => l.quantity > 0 && l.cost < SCALE));
  const segments = [], legCosts = [0n, 0n], indices = [0, 0], available = ladders.map(rows => rows[0]?.quantity ?? 0);
  let quantity = 0, total = 0n, marginal = null, stopReason = 'displayed_depth_depleted';
  while (ladders[0][indices[0]] && ladders[1][indices[1]]) {
    const levels = ladders.map((rows, i) => rows[indices[i]]), unit = levels[0].cost + levels[1].cost;
    if (unit >= SCALE) { stopReason = 'nonpositive_marginal_margin'; break; }
    const affordable = Number((amount(p.totalBudget) - total) / unit), qty = Math.min(...available, p.maxBasketQuantity - quantity, affordable);
    if (qty < 1) { stopReason = quantity >= p.maxBasketQuantity ? 'quantity_cap' : 'budget_cap'; break; }
    const costs = levels.map(l => l.cost * BigInt(qty)); total += costs[0] + costs[1]; quantity += qty; marginal = unit;
    for (let i = 0; i < 2; i++) { legCosts[i] += costs[i]; available[i] -= qty; if (!available[i]) available[i] = ladders[i][++indices[i]]?.quantity ?? 0; }
    segments.push({quantity: qty, yesBids: levels.map(l => l.yesBid), noUnitCostsAfterCushion: levels.map(l => money(l.cost)),
      marginalUnitCost: money(unit), cumulativeQuantity: quantity, cumulativeCost: money(total), cumulativeConditionalMargin: money(BigInt(quantity) * SCALE - total)});
  }
  const bestUnitCost = books.reduce((sum, book) => sum + SCALE - price(book.bids[0].price) + TICK * BigInt(p.adverseTicksPerLeg), 0n);
  return {quantity, totalCost: money(total), legCosts: legCosts.map(money), marginalUnitCost: marginal === null ? null : money(marginal),
    bestUnitCostAfterCushion: money(bestUnitCost), conditionalMargin: money(BigInt(quantity) * SCALE - total), segments, stopReason,
    depthFractionBps: p.depthFractionBps, displayedDepthUnits: ladders.map(rows => String(rows.reduce((sum, l) => sum + BigInt(l.quantity), 0n))),
    unmatchedLegRisk: {modeledFills: false, worstCaseUnmatchedCostByLeg: legCosts.map(money), simultaneousOrEqualFillsGuaranteed: false}};
}
const execution = () => ({eligibleForExecution: false, ordersSubmitted: 0, fills: null, pnl: null, profitabilityEstablished: false});
export function observeBasket(input, rawBook, bank) {
  checkState(input, bank); const book = checkedBook(rawBook, bank, input), s = structuredClone(input), p = bank.policy, at = instant(book.receivedAt);
  s.lastAt = book.receivedAt; s.counts.receipts++;
  const finish = (status, extra = {}) => {
    const observation = {at: book.receivedAt, status, classification: bank.settlementProof.status === 'verified' ? 'verified_normal_resolution_floor' : 'unverified_metadata_lead',
      proofStatus: bank.settlementProof.status, conditionalFloor: '1.000000', verifiedFloor: bank.settlementProof.status === 'verified' ? '1.000000' : null,
      unconditionalFloor: null, refundVoidTreatment: 'not_covered_by_floor', independentPair: false, positivePersistenceCount: s.positivePersistenceCount,
      ...extra, ...execution()}; s.lastObservation = observation; return {state: s, observation};
  };
  const invalidated = s.invalidatedAt[book.exchangeId];
  if (invalidated && instant(book.requestStartedAt) <= instant(invalidated)) return finish('request_before_invalidation');
  s.books[book.exchangeId] = book;
  s.lastSeen[book.exchangeId] = {requestStartedAt: book.requestStartedAt, receivedAt: book.receivedAt, sourceAt: book.sourceAt,
    sequence: book.sequence, versionKey: book.versionKey, contentHash: book.contentHash};
  const books = bank.legs.map(leg => s.books[leg.exchangeId]);
  if (books.some(b => !b)) return finish('waiting_for_pair');
  if (books.some(b => instant(b.requestStartedAt) <= instant(bank.cutoff))) return finish('before_cutoff');
  if (books.some(b => at - instant(b.receivedAt) > p.maxDataAgeMs)
    || Math.abs(instant(books[0].receivedAt) - instant(books[1].receivedAt)) > p.maxReceiptSkewMs) {
    s.positiveAnchor = null; s.positivePersistenceCount = 0; return finish('stale_pair');
  }
  const pairHash = hash(books.map(b => ({versionKey: b.versionKey, contentHash: b.contentHash}))), newPairEvidence = pairHash !== s.lastPairHash;
  if (newPairEvidence) s.counts.cofreshPairs++;
  s.lastPairHash = pairHash;
  const basket = walk(books, p), apparentPositive = basket.quantity > 0 && amount(basket.conditionalMargin) > 0n;
  if (!apparentPositive) { s.positiveAnchor = null; s.positivePersistenceCount = 0; return finish('no_positive_sized_basket', {basket, apparentPositive, newPairEvidence}); }
  if (newPairEvidence) s.counts.apparentPositivePairs++;
  const anchor = s.positiveAnchor;
  const independent = !!anchor && books.every((b, i) => (b.versionKey !== anchor.legs[i].versionKey || b.contentHash !== anchor.legs[i].contentHash)
    && instant(b.requestStartedAt) > instant(anchor.at) + p.latencyMs && instant(b.receivedAt) > instant(anchor.at) + p.latencyMs);
  if ((!anchor && newPairEvidence) || independent) {
    s.positivePersistenceCount = independent ? s.positivePersistenceCount + 1 : 1;
    if (independent) s.counts.independentPositiveConfirmations++;
    s.positiveAnchor = {at: book.receivedAt, legs: books.map(b => ({versionKey: b.versionKey, contentHash: b.contentHash}))};
  }
  return finish(independent ? 'independent_positive_persistence' : newPairEvidence ? 'apparent_positive_basket' : 'repeated_pair',
    {basket, apparentPositive, newPairEvidence, independentPair: independent, pairReceipts: books.map(b => b.receivedAt), pairVersions: books.map(b => b.versionKey)});
}
export function invalidateBasket(input, marketId, at, bank) {
  checkState(input, bank); const clock = instant(at); need(input.lastAt === null || clock >= instant(input.lastAt), 'Basket invalidation rollback');
  need(marketId === null || bank.legs.some(leg => leg.marketId === marketId), 'Foreign basket invalidation');
  const s = structuredClone(input); s.lastAt = at; s.counts.invalidations++; s.positiveAnchor = null; s.positivePersistenceCount = 0;
  for (const leg of bank.legs) if (marketId === null || marketId === leg.marketId) { delete s.books[leg.exchangeId]; s.invalidatedAt[leg.exchangeId] = at; }
  s.lastObservation = null; return s;
}
export function basketReport(state, bank) {
  checkState(state, bank);
  return {mode: 'dual_no_basket_measurement',bankHash: state.bankHash, sourceHash: bank.sourceHash, cutoff: bank.cutoff, asOf: state.lastAt,
    marketIds: bank.legs.map(x => x.marketId), proofStatus: bank.settlementProof.status, counts: structuredClone(state.counts),
    positivePersistenceCount: state.positivePersistenceCount, lastObservation: structuredClone(state.lastObservation), ...execution(),
    limitations: ['Bulk or metadata similarity cannot prove a settlement payoff.', 'Displayed depth is not a fill or reserved inventory.',
      'Atomic multi-leg placement does not establish equal fills.', 'Normal binary-resolution floor excludes cancellation, refund and override scenarios.']};
}
