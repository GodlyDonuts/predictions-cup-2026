const POLICY = 'binary-market-maker-hjb-grid-v1';
const HASH = /^[a-f0-9]{64}$/;
const EPS = 1e-9;
const MAX_CELLS = 2_000_000;
const MAX_EVALUATIONS = 40_000_000;

function finite(value) {
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

function validTransitionModel(model) {
  return model?.status === 'validated'
    && model?.source === 'chronological_market_history'
    && typeof model?.evidenceId === 'string' && model.evidenceId.length > 0
    && HASH.test(model?.modelHash || '')
    && typeof model?.cutoffUtc === 'string' && Number.isFinite(Date.parse(model.cutoffUtc));
}

function quoteCount(tick) {
  if (!finite(tick) || tick <= 0) return null;
  const count = Math.round(1 / tick);
  if (!Number.isSafeInteger(count) || count < 2 || count > 1000
      || Math.abs(count * tick - 1) > EPS) return null;
  return count;
}

function validProbabilityGrid(grid) {
  return Array.isArray(grid) && grid.length >= 2 && grid.length <= 1001
    && grid[0] === 0 && grid.at(-1) === 1
    && grid.every((p, i) => finite(p) && p >= 0 && p <= 1
      && (i === 0 || p > grid[i - 1]));
}

function validTransitionMatrix(matrix, size) {
  if (!Array.isArray(matrix) || matrix.length !== size) return false;
  return matrix.every(row => Array.isArray(row) && row.length === size
    && row.every(x => finite(x) && x >= 0 && x <= 1)
    && Math.abs(row.reduce((a, x) => a + x, 0) - 1) <= EPS);
}

function validBook(book, tickCount) {
  const aligned = p => finite(p) && Math.abs(p * tickCount - Math.round(p * tickCount)) <= EPS;
  return book?.coherent === true && book?.complete === true
    && Number.isSafeInteger(book?.sequence) && book.sequence >= 0
    && finite(book?.ageMs) && book.ageMs >= 0
    && finite(book?.maxAgeMs) && book.maxAgeMs >= 0 && book.ageMs <= book.maxAgeMs
    && aligned(book?.bestBid) && aligned(book?.bestAsk)
    && book.bestBid > 0 && book.bestAsk < 1 && book.bestBid < book.bestAsk;
}

function interpolate(values, grid, x) {
  if (!finite(x) || x < grid[0] - EPS || x > grid.at(-1) + EPS) return null;
  if (x <= grid[0]) return values[0];
  if (x >= grid.at(-1)) return values.at(-1);
  let lo = 0;
  let hi = grid.length - 1;
  while (hi - lo > 1) {
    const mid = (lo + hi) >> 1;
    if (grid[mid] <= x) lo = mid;
    else hi = mid;
  }
  const w = (x - grid[lo]) / (grid[hi] - grid[lo]);
  return values[lo] * (1 - w) + values[hi] * w;
}

/**
 * Research-only backward-Euler value solver for a finite-horizon binary maker.
 *
 * The probability transition matrix, running variance-rate model, book-context
 * provider, and fill-intensity model are explicit caller inputs. The solver
 * validates structure but cannot certify that those inputs are causal or
 * calibrated. It does not connect to an exchange or place orders.
 */
export function solveBinaryMakerValueSurface({
  probabilityGrid,
  transitionModel,
  evaluationStartUtc,
  maxInventory,
  lotSize = 1,
  tick,
  steps,
  dtSeconds,
  gammaTerminal,
  gammaRunning,
  varianceRatePerSecond,
  maxIntensityStep = 0.25,
  quoteContext,
  fillModel,
} = {}) {
  const nPrice = probabilityGrid?.length;
  const nQuotes = quoteCount(tick);
  const evaluationStartMs = Date.parse(evaluationStartUtc);
  const transitionCutoffMs = Date.parse(transitionModel?.cutoffUtc);
  const fillCutoffMs = Date.parse(fillModel?.calibration?.cutoffUtc);
  if (!validProbabilityGrid(probabilityGrid)
      || !nQuotes || !Number.isSafeInteger(maxInventory) || maxInventory < 1
      || !Number.isSafeInteger(lotSize) || lotSize < 1 || maxInventory % lotSize !== 0
      || !Number.isSafeInteger(steps) || steps < 0 || steps > 10000
      || !finite(dtSeconds) || dtSeconds <= 0
      || !finite(gammaTerminal) || gammaTerminal < 0
      || !finite(gammaRunning) || gammaRunning < 0
      || !(finite(varianceRatePerSecond) && varianceRatePerSecond >= 0
        || typeof varianceRatePerSecond === 'function')
      || !finite(maxIntensityStep) || maxIntensityStep <= 0 || maxIntensityStep > 1
      || !finite(steps * dtSeconds)
      || typeof quoteContext !== 'function' || !validCalibration(fillModel)
      || !validTransitionModel(transitionModel)
      || !Number.isFinite(evaluationStartMs)
      || !(transitionCutoffMs < evaluationStartMs && fillCutoffMs < evaluationStartMs)
      || !validTransitionMatrix(transitionModel.matrix, nPrice)) return null;

  const inventoryCount = 2 * maxInventory / lotSize + 1;
  const cellCount = nPrice * inventoryCount * (steps + 1);
  const evalCount = nPrice * inventoryCount * steps * 2 * (nQuotes - 1);
  if (!Number.isSafeInteger(inventoryCount) || cellCount > MAX_CELLS
      || evalCount > MAX_EVALUATIONS) return null;

  const inventoryAt = index => -maxInventory + index * lotSize;
  const layers = Array.from({length: steps + 1},
    () => new Float64Array(nPrice * inventoryCount));
  const offset = (priceIndex, inventoryIndex) => priceIndex * inventoryCount + inventoryIndex;

  for (let priceIndex = 0; priceIndex < nPrice; priceIndex++) {
    const probability = probabilityGrid[priceIndex];
    for (let inventoryIndex = 0; inventoryIndex < inventoryCount; inventoryIndex++) {
      const inventory = inventoryAt(inventoryIndex);
      const terminal = -gammaTerminal * inventory ** 2 * probability * (1 - probability);
      if (!finite(terminal)) return null;
      layers[0][offset(priceIndex, inventoryIndex)] = terminal === 0 ? 0 : terminal;
    }
  }

  for (let layer = 1; layer <= steps; layer++) {
    const remainingSeconds = layer * dtSeconds;
    const future = layers[layer - 1];
    const current = layers[layer];
    const expected = new Float64Array(nPrice * inventoryCount);
    for (let priceIndex = 0; priceIndex < nPrice; priceIndex++) {
      const transition = transitionModel.matrix[priceIndex];
      for (let inventoryIndex = 0; inventoryIndex < inventoryCount; inventoryIndex++) {
        let value = 0;
        for (let nextPriceIndex = 0; nextPriceIndex < nPrice; nextPriceIndex++)
          value += transition[nextPriceIndex] * future[offset(nextPriceIndex, inventoryIndex)];
        if (!finite(value)) return null;
        expected[offset(priceIndex, inventoryIndex)] = value;
      }
    }

    for (let priceIndex = 0; priceIndex < nPrice; priceIndex++) {
      const probability = probabilityGrid[priceIndex];
      let book;
      try { book = quoteContext({probability, remainingSeconds}); } catch { return null; }
      if (!validBook(book, nQuotes)) return null;

      let varianceRate;
      try {
        varianceRate = typeof varianceRatePerSecond === 'function'
          ? varianceRatePerSecond({probability, remainingSeconds, book})
          : varianceRatePerSecond;
      } catch { return null; }
      if (!finite(varianceRate) || varianceRate < 0) return null;

      for (let inventoryIndex = 0; inventoryIndex < inventoryCount; inventoryIndex++) {
        const inventory = inventoryAt(inventoryIndex);
        const base = expected[offset(priceIndex, inventoryIndex)];
        let buyBest = 0;
        let sellBest = 0;

        for (const side of ['buy', 'sell']) {
          const nextIndex = inventoryIndex + (side === 'buy' ? 1 : -1);
          if (nextIndex < 0 || nextIndex >= inventoryCount) continue;
          const continuationDelta = expected[offset(priceIndex, nextIndex)] - base;
          const nextInventory = inventoryAt(nextIndex);
          for (let tickIndex = 1; tickIndex < nQuotes; tickIndex++) {
            const quotePrice = tickIndex / nQuotes;
            if (side === 'buy' && quotePrice >= book.bestAsk) continue;
            if (side === 'sell' && quotePrice <= book.bestBid) continue;
            let intensity;
            try {
              intensity = fillModel.ratePerSecond({
                side, probability, quotePrice, remainingSeconds,
                inventory, nextInventory, lotSize, book,
              });
            } catch { return null; }
            if (!finite(intensity) || intensity < 0 || intensity * dtSeconds > maxIntensityStep) return null;

            const immediateMarkEdge = lotSize * (side === 'buy'
              ? probability - quotePrice
              : quotePrice - probability);
            const netIncrementalValue = immediateMarkEdge + continuationDelta;
            if (!finite(netIncrementalValue)) return null;
            const valueRate = intensity * Math.max(0, netIncrementalValue);
            if (!finite(valueRate)) return null;
            if (side === 'buy') buyBest = Math.max(buyBest, valueRate);
            else sellBest = Math.max(sellBest, valueRate);
          }
        }

        const runningRisk = gammaRunning * inventory ** 2 * varianceRate * dtSeconds;
        const value = base - runningRisk + dtSeconds * (buyBest + sellBest);
        if (!finite(value)) return null;
        current[offset(priceIndex, inventoryIndex)] = value;
      }
    }
  }

  const valueAt = ({probability, remainingSeconds, inventory} = {}) => {
    if (!finite(probability) || probability < 0 || probability > 1
        || !finite(remainingSeconds) || remainingSeconds < 0 || remainingSeconds > steps * dtSeconds
        || !Number.isSafeInteger(inventory) || Math.abs(inventory) > maxInventory
        || inventory % lotSize !== 0) return null;
    const inventoryIndex = (inventory + maxInventory) / lotSize;
    const timePosition = remainingSeconds / dtSeconds;
    const lowerLayer = Math.min(steps, Math.floor(timePosition));
    const upperLayer = Math.min(steps, lowerLayer + 1);
    const timeWeight = upperLayer === lowerLayer ? 0 : timePosition - lowerLayer;

    const valueAtLayer = layer => {
      const acrossPrices = probabilityGrid.map((_, priceIndex) =>
        layers[layer][offset(priceIndex, inventoryIndex)]);
      return interpolate(acrossPrices, probabilityGrid, probability);
    };
    const lowerValue = valueAtLayer(lowerLayer);
    const upperValue = valueAtLayer(upperLayer);
    if (lowerValue === null || upperValue === null) return null;
    return lowerValue * (1 - timeWeight) + upperValue * timeWeight;
  };

  const valueSurface = layers.map((layer, index) => ({
    remainingSeconds: index * dtSeconds,
    values: probabilityGrid.map((_, priceIndex) =>
      Array.from({length: inventoryCount}, (_, inventoryIndex) =>
        layer[offset(priceIndex, inventoryIndex)])),
  }));

  return {
    policy: POLICY,
    researchOnly: true,
    executionEligible: false,
    calibrationEvidenceId: fillModel.calibration.evidenceId,
    fillIntensityModelHash: fillModel.calibration.modelHash,
    transitionEvidenceId: transitionModel.evidenceId,
    transitionModelHash: transitionModel.modelHash,
    trainingCutoffs: {
      transition: transitionModel.cutoffUtc,
      fillIntensity: fillModel.calibration.cutoffUtc,
    },
    evaluationStartUtc,
    contractGrid: {tick, lotSize, maxInventory},
    timeGrid: {steps, dtSeconds, horizonSeconds: steps * dtSeconds},
    riskParameters: {
      gammaTerminal,
      gammaRunning,
      varianceRateSource: typeof varianceRatePerSecond === 'function' ? 'state_function' : 'constant',
    },
    dimensions: {probabilities: nPrice, inventories: inventoryCount, timeLayers: steps + 1},
    inventoryGrid: Array.from({length: inventoryCount}, (_, index) => inventoryAt(index)),
    dtSeconds,
    maxIntensityStep,
    valueSurface,
    valueAt,
  };
}



