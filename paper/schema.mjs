import {amount, price, uint, instant, sequence} from './fixed.mjs';

const kinds = new Set(['snapshot', 'trade', 'gap', 'submit', 'cancel', 'mark', 'clock']);
function id(value, name) {
  if (typeof value !== 'string' || !/^[A-Za-z0-9][A-Za-z0-9:._-]{0,127}$/.test(value)) throw new Error(`Invalid ${name}`);
}
export function validateConfig(c) {
  if (c?.schemaVersion !== 1) throw new Error('Unsupported config schema');
  if (amount(c.initialCash) <= 0n) throw new Error('Initial cash must be positive');
  uint(c.maxDataAgeMs, 'maxDataAgeMs', 1); uint(c.orderLatencyMs, 'orderLatencyMs'); uint(c.cancelLatencyMs, 'cancelLatencyMs');
  uint(c.maxPositionShares, 'maxPositionShares', 1);
  if (!c.costs || c.costs.assumption !== 'unverified_scenario') throw new Error('Costs must be labeled unverified_scenario');
  uint(c.costs.feeBps, 'feeBps');
  if (c.costs.feeBps > 10_000 || amount(c.costs.feePerShare) < 0n) throw new Error('Invalid fee scenario');
  return structuredClone(c);
}
export function validateEvent(e) {
  if (!e || typeof e !== 'object' || Array.isArray(e) || e.schemaVersion !== 1 || !kinds.has(e.type)) throw new Error('Unsupported event schema/type');
  id(e.eventId, 'eventId'); const received = instant(e.receivedAt); const occurred = instant(e.occurredAt);
  if (occurred > received) throw new Error('Event occurred after receipt');
  if (['snapshot', 'trade', 'gap', 'submit', 'mark'].includes(e.type)) id(e.instrument, 'instrument');
  if (e.type === 'snapshot') {
    sequence(e.sequence);
    if (typeof e.authoritative !== 'boolean') throw new Error('Snapshot authoritative flag required');
    if (e.nextExpiryAt !== null && instant(e.nextExpiryAt) <= occurred) throw new Error('Snapshot has already-expired depth');
    for (const side of ['bids', 'asks']) {
      if (!Array.isArray(e[side])) throw new Error('Both depth arrays required');
      const prices = new Set();
      for (const level of e[side]) {
        const p = price(level.price); uint(level.quantity, 'depth quantity', 1); uint(level.ownQuantity, 'own quantity');
        if (level.ownQuantity > level.quantity || prices.has(String(p))) throw new Error('Duplicate price or own quantity exceeds total');
        prices.add(String(p));
      }
    }
    if (e.bids.length && e.asks.length && Math.max(...e.bids.map(x => Number(price(x.price)))) >= Math.min(...e.asks.map(x => Number(price(x.price))))) throw new Error('Crossed or locked snapshot');
  }
  if (e.type === 'trade') {
    sequence(e.sequence); sequence(e.bookSequence); price(e.price); uint(e.quantity, 'trade quantity', 1);
    if (!['buy', 'sell'].includes(e.aggressor) || e.evidence !== 'explicit_trade') throw new Error('Explicit trade and aggressor evidence required');
    if (typeof e.externalAggressor !== 'boolean') throw new Error('Trade externalAggressor classification required');
  }
  if (e.type === 'submit') {
    id(e.orderId, 'orderId'); price(e.limitPrice); uint(e.quantity, 'order quantity', 1);
    if (!['buy', 'sell'].includes(e.action)) throw new Error('Invalid action');
    if (e.occurredAt !== e.receivedAt) throw new Error('Paper actions use availability time');
    if (e.expiresAt !== null && instant(e.expiresAt) <= received) throw new Error('Order expiry must follow submission');
  }
  if (e.type === 'cancel') { id(e.orderId, 'orderId'); if (occurred !== received) throw new Error('Paper actions use availability time'); }
  if (e.type === 'mark') {
    const p = amount(e.price);
    if (p < 0n || p > 1_000_000n || !['reference', 'last', 'tournament'].includes(e.source)) throw new Error('Invalid mark');
  }
  if (e.type === 'gap' && (typeof e.reason !== 'string' || !e.reason.trim())) throw new Error('Gap reason required');
  return e;
}
