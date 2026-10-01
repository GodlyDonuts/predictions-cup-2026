// Integer micro-SUSQies. Decimal strings only; no binary floating-point money.
export const SCALE = 1_000_000n;
export const TICK = 5_000n;
export function amount(value) {
  if (typeof value !== 'string' || !/^-?(0|[1-9]\d*)(\.\d{1,6})?$/.test(value)) {
    throw new Error('Money must be a canonical decimal string with at most six places');
  }
  const negative = value.startsWith('-');
  const [whole, fraction = ''] = (negative ? value.slice(1) : value).split('.');
  return (negative ? -1n : 1n) * (BigInt(whole) * SCALE + BigInt(fraction.padEnd(6, '0')));
}
export function money(value) {
  const sign = value < 0n ? '-' : ''; const x = value < 0n ? -value : value;
  return `${sign}${x / SCALE}.${String(x % SCALE).padStart(6, '0')}`;
}
export function price(value) {
  const result = amount(value);
  if (result < TICK || result > SCALE - TICK || result % TICK !== 0n) throw new Error('Price must be 0.005..0.995 on the 0.005 tick');
  return result;
}
export function uint(value, name, minimum = 0) {
  if (!Number.isSafeInteger(value) || value < minimum) throw new Error(`${name} must be a safe integer >= ${minimum}`);
  return value;
}
export function instant(value) {
  if (typeof value !== 'string' || !/^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d{3}Z$/.test(value)) throw new Error('Timestamp must be canonical UTC ISO milliseconds');
  const result = Date.parse(value);
  if (!Number.isFinite(result) || new Date(result).toISOString() !== value) throw new Error('Invalid timestamp');
  return result;
}
export function sequence(value) {
  if (typeof value !== 'string' || !/^(0|[1-9]\d*)$/.test(value)) throw new Error('Sequence must be an unsigned integer string');
  return BigInt(value);
}
export function fee(notional, quantity, costs) {
  // Round each fill's proportional charge UP to one micro; never assume rebates.
  return (notional * BigInt(costs.feeBps) + 9_999n) / 10_000n + BigInt(quantity) * amount(costs.feePerShare);
}
