const TICK = 0.005;

function finite(value) {
  const number = Number(value);
  return Number.isFinite(number) ? number : null;
}

function parseStatewideTitle(title) {
  if (typeof title !== 'string') return null;
  const match = /^Will the (Democratic|Republican) Party win the ([A-Za-z ]+) (Governor|Senate)\?$/.exec(title);
  if (!match) return null;
  return { party: match[1], state: match[2], office: match[3] };
}

export function crossMarketStatePairsV1(markets) {
  if (!Array.isArray(markets)) throw new TypeError('CMSTATE_MARKETS');
  const seenMarkets = new Set();
  const seenExchanges = new Set();
  const grouped = new Map();
  let statewideContracts = 0;

  for (const market of markets) {
    const marketId = String(market?.id ?? '');
    const exchangeId = String(market?.exchanges?.[0]?.id ?? '');
    if (!/^\d+$/.test(marketId) || !/^\d+$/.test(exchangeId) || seenMarkets.has(marketId) || seenExchanges.has(exchangeId)) {
      throw new Error('CMSTATE_MARKET_IDENTITY');
    }
    seenMarkets.add(marketId);
    seenExchanges.add(exchangeId);
    const identity = parseStatewideTitle(market.title);
    if (!identity) continue;
    statewideContracts++;
    const groupKey = `${identity.state}|${identity.party}`;
    const offices = grouped.get(groupKey) ?? new Map();
    if (offices.has(identity.office)) throw new Error('CMSTATE_DUPLICATE_OFFICE_CONTRACT');
    offices.set(identity.office, { marketId, exchangeId, title: market.title, ...identity });
    grouped.set(groupKey, offices);
  }

  const pairs = [];
  for (const [groupKey, offices] of grouped) {
    if (!offices.has('Senate') || !offices.has('Governor')) continue;
    const senate = offices.get('Senate');
    const governor = offices.get('Governor');
    if (senate.marketId === governor.marketId || senate.exchangeId === governor.exchangeId) throw new Error('CMSTATE_PAIR_IDENTITY');
    pairs.push({
      id: groupKey,
      state: senate.state,
      party: senate.party,
      marketA: senate.marketId,
      exchangeA: senate.exchangeId,
      titleA: senate.title,
      marketB: governor.marketId,
      exchangeB: governor.exchangeId,
      titleB: governor.title,
    });
  }
  pairs.sort((a, b) => a.state.localeCompare(b.state) || a.party.localeCompare(b.party));
  return { pairs, statewideContracts, totalMarkets: markets.length };
}

export function stateClusterCoverageV1(candidatePairs, analyzedPairs) {
  if (!Array.isArray(candidatePairs) || !Array.isArray(analyzedPairs)) throw new TypeError('CMSTATE_CLUSTER_INPUT');
  const summarize = pairs => {
    const ids = new Set();
    const states = new Set();
    const stateById = new Map();
    for (const pair of pairs) {
      if (!pair || typeof pair.id !== 'string' || !pair.id || typeof pair.state !== 'string' || !pair.state) {
        throw new Error('CMSTATE_CLUSTER_IDENTITY');
      }
      if (ids.has(pair.id)) throw new Error('CMSTATE_CLUSTER_DUPLICATE_PAIR');
      ids.add(pair.id);
      states.add(pair.state);
      stateById.set(pair.id, pair.state);
    }
    return { ids, states, stateById };
  };
  const candidate = summarize(candidatePairs);
  const analyzed = summarize(analyzedPairs);
  const missingPairIds = [...candidate.ids].filter(id => !analyzed.ids.has(id)).sort();
  const extraPairIds = [...analyzed.ids].filter(id => !candidate.ids.has(id)).sort();
  const candidateStates = [...candidate.states].sort();
  const analyzedStates = [...analyzed.states].sort();
  const missingStates = candidateStates.filter(state => !analyzed.states.has(state));
  const extraStates = analyzedStates.filter(state => !candidate.states.has(state));
  const pairStateMismatches = [...candidate.ids]
    .filter(id => analyzed.stateById.has(id) && candidate.stateById.get(id) !== analyzed.stateById.get(id))
    .sort()
    .map(id => ({ id, candidateState: candidate.stateById.get(id), analyzedState: analyzed.stateById.get(id) }));
  return {
    candidatePairs: candidate.ids.size,
    analyzedPairs: analyzed.ids.size,
    candidateStateClusters: candidate.states.size,
    analyzedStateClusters: analyzed.states.size,
    candidateStates,
    analyzedStates,
    missingPairIds,
    extraPairIds,
    missingStates,
    extraStates,
    pairStateMismatches,
    matches: missingPairIds.length === 0 && extraPairIds.length === 0
      && missingStates.length === 0 && extraStates.length === 0 && pairStateMismatches.length === 0,
  };
}

export function aggregateStateClustersV1(pairRows) {
  if (!Array.isArray(pairRows)) throw new TypeError('CMSTATE_AGGREGATE_INPUT');
  const states = new Map();
  for (const pair of pairRows) {
    if (!pair || typeof pair.state !== 'string' || !pair.state || !Number.isFinite(pair.marks)
      || pair.marks < 0 || !Number.isFinite(pair.grossPnl) || !Number.isFinite(pair.stressedPnl)) {
      throw new Error('CMSTATE_AGGREGATE_ROW');
    }
    const profitable = Number.isFinite(pair.profitable) ? pair.profitable : 0;
    if (!Number.isSafeInteger(profitable) || profitable < 0 || profitable > pair.marks) throw new Error('CMSTATE_AGGREGATE_PROFITABLE');
    const aggregate = states.get(pair.state) ?? { state: pair.state, pairClusters: 0, marks: 0, profitable: 0, grossPnl: 0, stressedPnl: 0 };
    aggregate.pairClusters++;
    aggregate.marks += pair.marks;
    aggregate.profitable += profitable;
    aggregate.grossPnl += pair.grossPnl;
    aggregate.stressedPnl += pair.stressedPnl;
    states.set(pair.state, aggregate);
  }
  return [...states.values()].sort((a, b) => a.state.localeCompare(b.state));
}

export function fitStateFactorV1(points, {
  minObservations = 100,
  maxIncrementGapMs = 10_000,
  minBeta = 0.25,
  maxBeta = 4,
  minSigma = 2 * TICK,
  betaOverride = null,
} = {}) {
  if (!Array.isArray(points)) throw new TypeError('CMSTATE_POINTS');
  const valid = points.filter(point => Number.isFinite(point.atMs)
      && Number.isFinite(point.aMid) && Number.isFinite(point.bMid)
      && point.aMid > 0 && point.aMid < 1 && point.bMid > 0 && point.bMid < 1)
    .sort((a, b) => a.atMs - b.atMs);
  if (valid.length < minObservations) return { usable: false, reason: 'too_few_development_points', observations: valid.length };

  let covarianceNumerator = 0;
  let varianceDenominator = 0;
  let increments = 0;
  let beta;
  if (betaOverride !== null) {
    beta = Number(betaOverride);
    if (!Number.isFinite(beta) || beta < minBeta || beta > maxBeta) {
      return { usable: false, reason: 'invalid_fixed_hedge', observations: valid.length };
    }
  } else {
    for (let i = 1; i < valid.length; i++) {
      const prior = valid[i - 1];
      const current = valid[i];
      if (current.atMs <= prior.atMs || current.atMs - prior.atMs > maxIncrementGapMs) continue;
      const dA = current.aMid - prior.aMid;
      const dB = current.bMid - prior.bMid;
      covarianceNumerator += dA * dB;
      varianceDenominator += dB * dB;
      increments++;
    }
    if (increments < minObservations - 1 || varianceDenominator <= 1e-10) {
      return { usable: false, reason: 'insufficient_joint_price_variation', observations: valid.length, increments };
    }
    beta = covarianceNumerator / varianceDenominator;
    if (!Number.isFinite(beta) || beta < minBeta || beta > maxBeta) {
      return { usable: false, reason: 'unstable_or_wrong_sign_hedge', observations: valid.length, increments, beta };
    }
  }

  const meanA = valid.reduce((sum, point) => sum + point.aMid, 0) / valid.length;
  const meanB = valid.reduce((sum, point) => sum + point.bMid, 0) / valid.length;
  const intercept = meanA - beta * meanB;
  const residuals = valid.map(point => point.aMid - intercept - beta * point.bMid);
  const residualMean = residuals.reduce((sum, value) => sum + value, 0) / residuals.length;
  const variance = residuals.reduce((sum, value) => sum + (value - residualMean) ** 2, 0) / (residuals.length - 1);
  const observedSigma = Math.sqrt(variance);
  const sigma = Math.max(observedSigma, minSigma);
  if (!Number.isFinite(sigma)) return { usable: false, reason: 'invalid_residual_scale', observations: valid.length, increments };
  return {
    usable: true,
    betaSource: betaOverride === null ? 'development_ols' : 'fixed_equal_share_comparator',
    observations: valid.length,
    increments,
    beta,
    intercept,
    residualMean,
    observedSigma,
    sigma,
    minSigmaApplied: sigma !== observedSigma,
    firstAt: new Date(valid[0].atMs).toISOString(),
    lastAt: new Date(valid.at(-1).atMs).toISOString(),
  };
}

export function stateFactorSignalV1(model, aMid, bMid, thresholdZ = 2) {
  if (!model?.usable || !Number.isFinite(aMid) || !Number.isFinite(bMid) || !Number.isFinite(thresholdZ) || thresholdZ <= 0) {
    return { direction: 0, zScore: null, residual: null };
  }
  const residual = aMid - model.intercept - model.beta * bMid - model.residualMean;
  const zScore = residual / model.sigma;
  return { direction: zScore <= -thresholdZ ? -1 : zScore >= thresholdZ ? 1 : 0, zScore, residual };
}

export function synchronizedPairSnapshotV1(a, b, decisionAtMs, {
  maxBookAgeMs = 3000,
  maxCrossMarketSkewMs = 2000,
} = {}) {
  if (!a || !b || !Number.isFinite(decisionAtMs)) return null;
  const atA = Date.parse(a.receivedAt), atB = Date.parse(b.receivedAt);
  const sourceA = Date.parse(a.sourceAt), sourceB = Date.parse(b.sourceAt);
  if (![atA, atB, sourceA, sourceB].every(Number.isFinite)) return null;
  if (a.segment !== b.segment) return null;
  if (sourceA > atA || sourceB > atB || atA > decisionAtMs || atB > decisionAtMs) return null;
  if (decisionAtMs - atA > maxBookAgeMs || decisionAtMs - atB > maxBookAgeMs) return null;
  if (atA - sourceA > maxBookAgeMs || atB - sourceB > maxBookAgeMs) return null;
  if (Math.abs(atA - atB) > maxCrossMarketSkewMs || Math.abs(sourceA - sourceB) > maxCrossMarketSkewMs) return null;
  const aBid = finite(a.bids?.[0]?.price), aAsk = finite(a.asks?.[0]?.price);
  const bBid = finite(b.bids?.[0]?.price), bAsk = finite(b.asks?.[0]?.price);
  if ([aBid, aAsk, bBid, bAsk].some(value => value === null) || !(aBid > 0 && aBid < aAsk && aAsk < 1) || !(bBid > 0 && bBid < bAsk && bAsk < 1)) return null;
  return {
    atMs: Math.max(atA, atB),
    sourceAMs: sourceA,
    sourceBMs: sourceB,
    receivedAMs: atA,
    receivedBMs: atB,
    seqA: String(a.sequence),
    seqB: String(b.sequence),
    segment: a.segment,
    aMid: (aBid + aAsk) / 2,
    bMid: (bBid + bAsk) / 2,
    a: { bids: a.bids, asks: a.asks },
    b: { bids: b.bids, asks: b.asks },
  };
}

export function pairExecutableMarkoutV1(direction, beta, entry, exit, { tick = TICK, stressTicksPerFill = 1 } = {}) {
  if (![direction, beta, tick, stressTicksPerFill].every(Number.isFinite) || ![-1, 1].includes(direction) || beta <= 0 || tick < 0 || stressTicksPerFill < 0) {
    throw new TypeError('CMSTATE_MARKOUT_INPUT');
  }
  if (!entry?.a || !entry?.b || !exit?.a || !exit?.b) throw new TypeError('CMSTATE_MARKOUT_BOOK');
  const long = direction < 0 ? entry.a : entry.b;
  const rich = direction < 0 ? entry.b : entry.a;
  const longExit = direction < 0 ? exit.a : exit.b;
  const richExit = direction < 0 ? exit.b : exit.a;
  const longAsk = finite(long.asks?.[0]?.price), longAskQty = finite(long.asks?.[0]?.quantity);
  const richBid = finite(rich.bids?.[0]?.price), richBidQty = finite(rich.bids?.[0]?.quantity);
  const longBidExit = finite(longExit.bids?.[0]?.price), longBidExitQty = finite(longExit.bids?.[0]?.quantity);
  const richAskExit = finite(richExit.asks?.[0]?.price), richAskExitQty = finite(richExit.asks?.[0]?.quantity);
  if ([longAsk, longAskQty, richBid, richBidQty, longBidExit, longBidExitQty, richAskExit, richAskExitQty].some(value => value === null)) {
    return { eligible: false, reason: 'missing_top_level' };
  }
  if (longAskQty < 1 || richBidQty < beta || longBidExitQty < 1 || richAskExitQty < beta) {
    return { eligible: false, reason: 'insufficient_top_level_depth' };
  }
  const entryCost = longAsk + beta * (1 - richBid);
  const exitValue = longBidExit + beta * (1 - richAskExit);
  const grossPnl = exitValue - entryCost;
  const stress = 2 * stressTicksPerFill * tick * (1 + beta);
  return {
    eligible: true,
    entryCost,
    exitValue,
    grossPnl,
    oneTickEachFillStressPnl: grossPnl - stress,
    stressDeduction: stress,
    matchedUnits: 1 + beta,
  };
}
