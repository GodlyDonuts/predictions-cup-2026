export const timestamp = milliseconds => new Date(Date.UTC(2026, 0, 1) + milliseconds).toISOString();
export const config = overrides => ({schemaVersion: 1, initialCash: '100.000000', maxDataAgeMs: 10_000,
  orderLatencyMs: 0, cancelLatencyMs: 50, maxPositionShares: 100,
  costs: {feeBps: 0, feePerShare: '0', assumption: 'unverified_scenario'}, ...overrides});
export const event = (type, at, fields = {}) => ({schemaVersion: 1, eventId: `${type}-${at}`, type,
  occurredAt: timestamp(at), receivedAt: timestamp(at), ...fields});
export const level = (price, quantity, ownQuantity = 0) => ({price, quantity, ownQuantity});
export const snapshot = (at = 0, fields = {}) => event('snapshot', at, {instrument: 'synthetic:YES', sequence: '1', authoritative: true,
  nextExpiryAt: null, bids: [level('0.400', 10)], asks: [level('0.600', 10)], ...fields});
export const order = (at = 1, fields = {}) => event('submit', at, {instrument: 'synthetic:YES', orderId: `o-${at}`,
  action: 'buy', quantity: 5, limitPrice: '0.400', expiresAt: null, ...fields});
export const trade = (at = 2, fields = {}) => event('trade', at, {instrument: 'synthetic:YES', sequence: String(at), bookSequence: '1',
  price: '0.400', quantity: 11, aggressor: 'sell', evidence: 'explicit_trade', externalAggressor: true, ...fields});
