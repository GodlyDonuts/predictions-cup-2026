const POLICY = 'binary-inventory-maker-v1';
const HASH = /^[a-f0-9]{64}$/;

function finiteNumber(value) {
  return typeof value === 'number' && Number.isFinite(value);
}

function validCalibration(model) {
  const c = model?.calibration;
  return typeof model?.ratePerSecond === 'function'
    && c?.status === 'validated'
    && c?.source === 'own_fill_receipts'
    && typeof c?.evidenceId === 'string' && c.evidenceId.length > 0
    && HASH.test(c?.modelHash || '')
    && Number.isSafeInteger(c?.minimumClustersRequired) && c.minimumClustersRequired >= 32
    && Number.isSafeInteger(c?.independentClusters) && c.independentClusters >= c.minimumClustersRequired
    && typeof c?.cutoffUtc === 'string' && Number.isFinite(Date.parse(c.cutoffUtc));
}

function validContinuation(fn, state) {
  if (typeof fn !== 'function') return null;
  const value = fn(state);
  return finiteNumber(value) ? value : null;
}

function quoteTicks(tick) {
  if (!finiteNumber(tick) || tick <= 0) return null;
  const count = Math.round(1 / tick);
  if (!Number.isSafeInteger(count) || count < 2 || Math.abs(count * tick - 1) > 1e-9) return null;
  return count;
}

function alignedToTick(price, count) {
  return finiteNumber(price) && Math.abs(price * count - Math.round(price * count)) <= 1e-9;
}

export function binaryTerminalInventoryPenalty(probability, inventory, gammaTerminal) {
  if (!finiteNumber(probability) || probability < 0 || probability > 1
      || !Number.isSafeInteger(inventory) || !finiteNumber(gammaTerminal) || gammaTerminal < 0)
    throw new RangeError('BINARY_MAKER_TERMINAL_INPUT');
  const penalty = -gammaTerminal * inventory ** 2 * probability * (1 - probability);
  return penalty === 0 ? 0 : penalty;
}

export function complementaryBinaryOrder(action, yesPrice, tick) {
  const count = quoteTicks(tick);
  if (!count || !['buy', 'sell'].includes(action) || !finiteNumber(yesPrice)
      || yesPrice <= 0 || yesPrice >= 1) throw new RangeError('BINARY_MAKER_COMPLEMENT_INPUT');
  const yesTick = Math.round(yesPrice * count);
  if (Math.abs(yesTick / count - yesPrice) > 1e-9) throw new RangeError('BINARY_MAKER_COMPLEMENT_TICK');
  return {
    outcome: 'no',
    action: action === 'buy' ? 'sell' : 'buy',
    price: (count - yesTick) / count,
  };
}

/**
 * Research-only discrete HJB quote selector for a binary YES claim.
 * `continuationValue` must be a frozen value surface and `fillModel` must carry
 * an own-fill calibration certificate. This function never enables execution.
 */
export function selectBinaryInventoryMakerQuotes({
  probability,
  remainingSeconds,
  inventory,
  maxInventory,
  lotSize = 1,
  tick,
  book,
  maxBookAgeMs,
  continuationValue,
  fillModel,
}) {
  const count = quoteTicks(tick);
  if (!count || !finiteNumber(probability) || probability <= 0 || probability >= 1
      || !finiteNumber(remainingSeconds) || remainingSeconds < 0
      || !Number.isSafeInteger(inventory) || !Number.isSafeInteger(maxInventory) || maxInventory < 1
      || !Number.isSafeInteger(lotSize) || lotSize < 1 || lotSize > maxInventory
      || Math.abs(inventory) > maxInventory
      || !finiteNumber(maxBookAgeMs) || maxBookAgeMs < 0
      || book?.coherent !== true || book?.complete !== true
      || !Number.isSafeInteger(book?.sequence) || book.sequence < 0
      || !finiteNumber(book?.ageMs) || book.ageMs < 0 || book.ageMs > maxBookAgeMs
      || !finiteNumber(book?.bestBid) || !finiteNumber(book?.bestAsk)
      || book.bestBid <= 0 || book.bestAsk >= 1 || book.bestBid >= book.bestAsk
      || !alignedToTick(book.bestBid, count) || !alignedToTick(book.bestAsk, count)
      || !validCalibration(fillModel) || typeof continuationValue !== 'function') return null;

  const state = {probability, remainingSeconds};
  const currentValue = validContinuation(continuationValue, {...state, inventory});
  if (currentValue === null) return null;

  const sides = [
    {action: 'buy', delta: lotSize, allowed: inventory + lotSize <= maxInventory},
    {action: 'sell', delta: -lotSize, allowed: inventory - lotSize >= -maxInventory},
  ];
  const quotes = {};

  for (const side of sides) {
    if (!side.allowed) { quotes[side.action] = null; continue; }
    const nextValue = validContinuation(continuationValue, {...state, inventory: inventory + side.delta});
    if (nextValue === null) return null;
    const nextInventory = inventory + side.delta;
    const continuationDelta = nextValue - currentValue;
    let best = null;

    for (let index = 1; index < count; index++) {
      const price = index / count;
      if (side.action === 'buy' && price >= book.bestAsk) continue;
      if (side.action === 'sell' && price <= book.bestBid) continue;

      const signedMarkEdge = side.action === 'buy'
        ? lotSize * (probability - price)
        : lotSize * (price - probability);
      const netValuePerLot = signedMarkEdge + continuationDelta;
      if (!finiteNumber(netValuePerLot)) return null;
      if (!(netValuePerLot > 0)) continue;

      const fillIntensityPerSecond = fillModel.ratePerSecond({
        side: side.action,
        probability,
        quotePrice: price,
        remainingSeconds,
        inventory,
        nextInventory,
        lotSize,
        book,
      });
      if (!finiteNumber(fillIntensityPerSecond) || fillIntensityPerSecond < 0) return null;
      const expectedValuePerSecond = fillIntensityPerSecond * netValuePerLot;
      if (!finiteNumber(expectedValuePerSecond)) return null;
      if (!(expectedValuePerSecond > 0)) continue;

      if (!best || expectedValuePerSecond > best.expectedValuePerSecond) {
        best = {
          outcome: 'yes',
          action: side.action,
          price,
          lotSize,
          fillIntensityPerSecond,
          netValuePerLot,
          expectedValuePerSecond,
          nextInventory,
          equivalentComplement: complementaryBinaryOrder(side.action, price, tick),
        };
      }
    }
    quotes[side.action] = best;
  }

  return {
    policy: POLICY,
    researchOnly: true,
    executionEligible: false,
    calibrationEvidenceId: fillModel.calibration.evidenceId,
    independentFillClusters: fillModel.calibration.independentClusters,
    quotes,
  };
}

