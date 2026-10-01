// Supported API v1 market-feed subset, verified 2026-10-01. No transport/auth.
export const CAPTURE_SCHEMA_VERSION = 1;
export const DELIVERY_MODEL = 'best-effort-authoritative-resync';
export class ContractError extends Error {
  constructor(code = 'INVALID_SCHEMA') { super(code); this.name = 'ContractError'; this.code = code; }
}
export function requireThat(value, code = 'INVALID_SCHEMA') { if (!value) throw new ContractError(code); }
export function object(v) { requireThat(v !== null && typeof v === 'object' && !Array.isArray(v)); return v; }
export function integer(v, min = 0) { requireThat(Number.isSafeInteger(v) && v >= min); return v; }
export function timestamp(v) {
  requireThat(typeof v === 'string');
  const m = /^(\d{4})-(\d\d)-(\d\d)T(\d\d):(\d\d):(\d\d)(?:\.(\d{1,9}))?(Z|([+-])(\d\d):(\d\d))$/.exec(v);
  requireThat(m);
  const [, y, mo, d, h, mi, se, fraction = '', zone, , zh = '0', zm = '0'] = m;
  const year = +y, month = +mo, day = +d;
  const leap = year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0);
  const days = [31, leap ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
  requireThat(year >= 1970 && month >= 1 && month <= 12 && day >= 1 && day <= days[month - 1] && +h < 24 && +mi < 60 && +se < 60 && +zh < 24 && +zm < 60);
  const ms = Date.parse(`${y}-${mo}-${d}T${h}:${mi}:${se}.${fraction.padEnd(3, '0').slice(0, 3)}${zone}`);
  requireThat(Number.isFinite(ms));
  const iso = new Date(ms).toISOString();
  return iso.slice(0, 19) + '.' + fraction.padEnd(9, '0') + 'Z';
}
export function instantNs(v) {
  const iso = timestamp(v);
  return BigInt(Date.parse(iso)) * 1000000n + BigInt(iso.slice(23, 29));
}
export function numericId(v) {
  requireThat((typeof v === 'string' && /^[1-9]\d{0,15}$/.test(v)) || typeof v === 'number');
  return String(integer(Number(v), 1));
}
function uuid(v) { requireThat(typeof v === 'string' && /^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/i.test(v)); return v.toLowerCase(); }
function array(v) { requireThat(Array.isArray(v) && v.length <= 100000); return v; }
export function config(v) {
  object(v); const tournamentId = uuid(v.tournamentId); const marketId = numericId(v.marketId);
  const exchangeIds = array(v.exchangeIds).map(numericId);
  requireThat(exchangeIds.length > 0 && new Set(exchangeIds).size === exchangeIds.length);
  return {tournamentId, marketId, exchangeIds, topic: `tournament:${tournamentId}:market:${marketId}`};
}
export function version(v, nullable = false) {
  if (v === null && nullable) return null;
  object(v); return {sequence: integer(v.sequence), at: timestamp(v.at)};
}
export function compareVersion(a, b) {
  if (a === null) return b === null ? 0 : -1;
  if (b === null) return 1;
  return Math.sign(a.sequence - b.sequence) || (instantNs(a.at) > instantNs(b.at) ? 1 : instantNs(a.at) < instantNs(b.at) ? -1 : 0);
}
function level(v) {
  object(v); requireThat(typeof v.price === 'number' && Number.isFinite(v.price) && v.price >= 0 && v.price <= 1);
  requireThat(typeof v.quantity === 'number' && Number.isFinite(v.quantity) && v.quantity > 0 && v.quantity <= Number.MAX_SAFE_INTEGER);
  return {price: v.price, quantity: v.quantity};
}
function levels(v, direction) {
  const result = array(v).map(level);
  for (let i = 1; i < result.length; i++) requireThat(direction * (result[i].price - result[i - 1].price) > 0, 'UNSORTED_LEVELS');
  return result;
}
function exchange(v, scope) { const id = numericId(v); requireThat(scope.exchangeIds.includes(id), 'UNEXPECTED_EXCHANGE'); return id; }
function scopeCheck(v, scope) {
  requireThat(numericId(v.marketId) === scope.marketId && uuid(v.tournamentId) === scope.tournamentId, 'CONTEXT_MISMATCH');
}
export function book(v, scope, rest = false) {
  object(v); const exchangeId = exchange(v.exchangeId, scope);
  // REST does not currently promise nextExpiryAt; do not invent it.
  if (!rest) requireThat(Object.hasOwn(v, 'nextExpiryAt'));
  return {exchangeId, asOf: version(v.asOf, rest), nextExpiryAt: v.nextExpiryAt == null ? null : timestamp(v.nextExpiryAt),
    expiryKnown: !rest && Object.hasOwn(v, 'nextExpiryAt'), bids: levels(v.bids, -1), asks: levels(v.asks, 1)};
}
function trade(v, scope) {
  object(v); scopeCheck(v, scope);
  requireThat(v.id === null || (typeof v.id === 'string' && /^[A-Za-z0-9_-]{1,128}$/.test(v.id)));
  const sequence = v.sequence === null ? null : integer(v.sequence);
  requireThat((v.id === null) === (sequence === null), 'UNORDERED_TRADE');
  const l = level(v); return {id: v.id, sequence, exchangeId: exchange(v.exchangeId, scope), marketId: scope.marketId,
    tournamentId: scope.tournamentId, ...l, executedAt: timestamp(v.executedAt)};
}
function dirty(v, scope) { object(v); scopeCheck(v, scope); return {exchangeId: exchange(v.exchangeId, scope), marketId: scope.marketId, tournamentId: scope.tournamentId, at: timestamp(v.at)}; }
function settled(v, scope) {
  object(v); scopeCheck(v, scope); requireThat(typeof v.settledWith === 'string' && v.settledWith.length <= 256);
  // Settlement label is free text; capture an invalidation, not arbitrary text.
  return {marketId: scope.marketId, tournamentId: scope.tournamentId, refund: v.settledWith === 'REFUND', at: timestamp(v.at)};
}
export function parseEvent(event, payload, scope) {
  object(payload);
  if (event === 'book_dirty') return {event, payload: dirty(payload, scope)};
  if (event === 'market_settled') return {event, payload: settled(payload, scope)};
  requireThat(event === 'market_batch', 'UNSUPPORTED_EVENT');
  const d = object(payload.delivery);
  requireThat(d.model === DELIVERY_MODEL, 'UNKNOWN_DELIVERY_MODEL');
  const revision = integer(d.revision, 1), previousRevision = integer(d.previousRevision);
  requireThat(revision > previousRevision, 'INVALID_REVISION');
  const sourceSequenceFrom = d.sourceSequenceFrom === null ? null : integer(d.sourceSequenceFrom);
  const sourceSequenceThrough = d.sourceSequenceThrough === null ? null : integer(d.sourceSequenceThrough);
  requireThat((sourceSequenceFrom === null) === (sourceSequenceThrough === null));
  requireThat(sourceSequenceFrom === null || sourceSequenceFrom <= sourceSequenceThrough);
  // correlationId is diagnostic free text; deliberately not persisted.
  requireThat(typeof d.correlationId === 'string' && d.correlationId.length <= 256);
  requireThat(payload.resyncRequired === undefined || typeof payload.resyncRequired === 'boolean');
  const compact = payload.resyncRequired === true;
  const out = {delivery: {model: DELIVERY_MODEL, revision, previousRevision, sourceSequenceFrom, sourceSequenceThrough}, resyncRequired: compact};
  out.books = array(payload.books ?? []).map(v => book(v, scope));
  requireThat(new Set(out.books.map(b => b.exchangeId)).size === out.books.length, 'DUPLICATE_BOOK');
  out.trades = array(compact ? payload.trades ?? [] : payload.trades).map(v => trade(v, scope));
  out.bookDirty = array(compact ? payload.bookDirty ?? [] : payload.bookDirty).map(v => dirty(v, scope));
  out.marketSettled = array(compact ? payload.marketSettled ?? [] : payload.marketSettled).map(v => settled(v, scope));
  return {event, payload: out};
}
export function parseSnapshot(raw, scope) {
  object(raw); const contexts = array(raw.contexts);
  const selected = contexts.filter(c => c?.type === 'tournament' && c.tournament?.id === scope.tournamentId);
  requireThat(selected.length === 1, 'CONTEXT_MISMATCH');
  const result = array(object(selected[0].orderbook).exchanges).map(v => book(v, scope, true));
  requireThat(result.length === scope.exchangeIds.length && new Set(result.map(x => x.exchangeId)).size === result.length, 'INCOMPLETE_SNAPSHOT');
  return result;
}
export function snapshotPath(scope) {
  return `/markets/${scope.marketId}/orderbook?tournamentId=${scope.tournamentId}&depth=200`;
}
