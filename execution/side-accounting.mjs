import {decimalMicros, formatMicros, payloadHash, canonicalPayload} from './core.mjs';

const SCALE = 1_000_000n;
const requireThat = (ok, code) => { if (!ok) throw Error(code); };
const identity = x => typeof x === 'string' && /^[1-9]\d*$/.test(x) && Number.isSafeInteger(Number(x));
const time = x => typeof x === 'string' && Number.isFinite(Date.parse(x));
const sideOf = x => { requireThat(x === 'yes' || x === 'no', 'INVALID_OUTCOME_SIDE'); return x; };
const priceOf = x => {
  requireThat(typeof x === 'string' || typeof x === 'number' && Number.isFinite(x), 'MISSING_EXECUTION_PRICE');
  const p = decimalMicros(String(x));
  requireThat(p >= 0n && p <= SCALE, 'INVALID_EXECUTION_PRICE');
  return p;
};

// REST schemas specify signed quantities but do not specify the NO price basis.
// Do not infer that convention from the realtime guide or from a plausible price.
export function effectiveFillPrice(price, side, source) {
  sideOf(side);
  requireThat(source === 'realtime_account_batch' || source === 'rest_order_fills', 'UNKNOWN_FILL_SOURCE');
  requireThat(side === 'yes' || source === 'realtime_account_batch', 'UNVERIFIED_REST_NO_PRICE_BASIS');
  const raw = priceOf(price);
  return formatMicros(side === 'no' ? SCALE - raw : raw);
}

// Pure, non-authoritative projection. It neither creates nor dispatches orders.
// The binding is an already canonical BUY, with a side-relative limit; canonical
// SELL/netting responses need a separate validated action mapping.
export function projectBoundBuyFills(binding, rows, {
  source, readAt, complete = false, prior = null,
} = {}) {
  requireThat(binding && identity(binding.orderId) && identity(binding.exchangeId) && identity(binding.marketId)
    && (binding.tournamentId === null || typeof binding.tournamentId === 'string' && binding.tournamentId.length > 0)
    && binding.action === 'buy' && time(binding.createdAt), 'INVALID_BUY_BINDING');
  sideOf(binding.side);
  requireThat(Number.isSafeInteger(binding.quantity) && binding.quantity > 0 && binding.quantity <= 100,
    'INVALID_BOUND_QUANTITY');
  const limit = priceOf(binding.sideRelativeLimit);
  requireThat(limit >= 5_000n && limit <= 995_000n && limit % 5_000n === 0n, 'INVALID_BOUND_LIMIT');
  requireThat(time(readAt) && Date.parse(readAt) >= Date.parse(binding.createdAt), 'INVALID_FILL_READ_TIME');
  requireThat(Array.isArray(rows) && rows.length <= 10_000 && typeof complete === 'boolean', 'INVALID_FILL_ROWS');
  effectiveFillPrice('0.5', binding.side, source);
  const bound = {
    orderId: binding.orderId, exchangeId: binding.exchangeId, marketId: binding.marketId,
    tournamentId: binding.tournamentId, action: 'buy', side: binding.side,
    quantity: binding.quantity, sideRelativeLimit: formatMicros(limit), createdAt: binding.createdAt,
  };
  const bindingHash = payloadHash(bound), seen = new Map();
  for (const row of rows) {
    requireThat(row && Number.isSafeInteger(row.id) && row.id > 0 && time(row.filledAt)
      && row.orderId === bound.orderId && row.exchangeId === bound.exchangeId
      && row.marketId === bound.marketId && row.tournamentId === bound.tournamentId
      && row.side === bound.side, 'FILL_IDENTITY_MISMATCH');
    requireThat(Number.isSafeInteger(row.quantity) && row.quantity !== 0
      && (row.side === 'yes' ? row.quantity > 0 : row.quantity < 0), 'FILL_SIDE_SIGN_MISMATCH');
    requireThat(Date.parse(row.filledAt) >= Date.parse(bound.createdAt)
      && Date.parse(row.filledAt) <= Date.parse(readAt), 'INVALID_FILL_TIME');
    const effectivePrice = effectiveFillPrice(row.price, row.side, source), p = decimalMicros(effectivePrice);
    requireThat(p <= limit, 'FILL_OUTSIDE_SIDE_RELATIVE_BUY_LIMIT');
    const fill = {id: row.id, signedQuantity: row.quantity, quantity: Math.abs(row.quantity),
      rawPrice: formatMicros(priceOf(row.price)), effectivePrice, filledAt: row.filledAt};
    if (seen.has(row.id)) requireThat(canonicalPayload(seen.get(row.id)) === canonicalPayload(fill), 'CONFLICTING_DUPLICATE_FILL');
    else seen.set(row.id, fill);
  }
  const fills = [...seen.values()].sort((a,b) => Date.parse(a.filledAt) - Date.parse(b.filledAt) || a.id - b.id);
  const quantity = fills.reduce((n,f) => n + f.quantity, 0);
  requireThat(Number.isSafeInteger(quantity) && quantity <= bound.quantity, 'OVERFILLED_BOUND_ORDER');
  if (prior !== null) {
    requireThat(prior.bindingHash === bindingHash && prior.source === source
      && payloadHash(prior.binding) === bindingHash && Array.isArray(prior.fills), 'PRIOR_BINDING_MISMATCH');
    requireThat(time(prior.readAt) && Date.parse(prior.readAt) <= Date.parse(readAt), 'FILL_READ_CLOCK_REGRESSION');
    const {projectionHash, ...previousProjection} = prior;
    requireThat(projectionHash === payloadHash(previousProjection), 'INVALID_PRIOR_PROJECTION');
    requireThat(!prior.complete || complete, 'COMPLETE_HISTORY_REGRESSION');
    for (const old of prior.fills) requireThat(fills.some(f => canonicalPayload(f) === canonicalPayload(old)), 'FILL_HISTORY_CHANGED');
  }
  const cost = fills.reduce((n,f) => n + BigInt(f.quantity) * decimalMicros(f.effectivePrice), 0n);
  const projection = {schemaVersion: 1, binding: bound, bindingHash, source, readAt, complete,
    fills, filledQuantity: quantity, signedPositionDelta: bound.side === 'yes' || quantity === 0 ? quantity : -quantity,
    principalCost: formatMicros(cost), maximumRemainingReservation: formatMicros(BigInt(bound.quantity - quantity) * limit),
    actualOrderAuthority: false, authoritativeReconciliation: false};
  return {...projection, projectionHash: payloadHash(projection)};
}
