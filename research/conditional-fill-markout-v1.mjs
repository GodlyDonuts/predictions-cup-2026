import { amount, money, TICK } from '../paper/fixed.mjs';
import { instantNs } from '../feed/contract.mjs';
import { normalizeOfficialFillTick } from '../execution/api-tick-price.mjs';

const DEFAULTS = Object.freeze({
  latencyMs: 3000,
  quoteTtlMs: 10000,
  sampleEveryMs: 30000,
  markoutHorizonsMs: [1000, 5000, 30000],
  maximumMarkoutDelayMs: 10000,
  exitResponseLatencyMs: 3000,
  maximumExecutableBookAgeMs: 3000,
  purgeMs: 60000,
});

const ms = value => Number(instantNs(value) / 1000000n);
const NS_PER_MS = 1000000n;
const compareNs = (a, b) => a < b ? -1 : a > b ? 1 : 0;
const elapsedMs = (startNs, endNs) => endNs > startNs ? Number(endNs - startNs) / Number(NS_PER_MS) : 0;
const quantile = (values, p) => values.length ? values[Math.max(0, Math.ceil(values.length * p) - 1)] : null;

function summarize(rows) {
  if (!rows.length) return { samples: 0, markets: 0, meanExecutableNet: null, medianExecutableNet: null, meanMidMarkout: null, oneTickEachWayStressMean: null, positiveFraction: null };
  const nets = rows.map(x => x.executableNet).sort((a, b) => a < b ? -1 : a > b ? 1 : 0);
  const mean = values => values.reduce((sum, x) => sum + x, 0n) / BigInt(values.length);
  const mid = mean(rows.map(x => x.midMarkout));
  const net = mean(nets);
  const median = nets.length % 2 ? nets[(nets.length - 1) / 2] : (nets[nets.length / 2 - 1] + nets[nets.length / 2]) / 2n;
  return {
    samples: rows.length,
    markets: new Set(rows.map(x => x.marketId ?? x.candidate?.marketId).filter(x => typeof x === 'string')).size,
    meanExecutableNet: money(net),
    medianExecutableNet: money(median),
    meanMidMarkout: money(mid),
    oneTickEachWayStressMean: money(net - 2n * TICK),
    positiveFraction: rows.filter(x => x.executableNet > 0n).length / rows.length,
    p10ExecutableNet: money(nets[Math.floor((nets.length - 1) * 0.10)]),
    p90ExecutableNet: money(nets[Math.floor((nets.length - 1) * 0.90)]),
  };
}

function spreadBin(ticks) { return ticks <= 1 ? '1 tick' : ticks === 2 ? '2 ticks' : '3+ ticks'; }
function depthBin(depth) { return depth <= 10 ? '1-10' : depth <= 50 ? '11-50' : depth <= 250 ? '51-250' : '251+'; }
function flowBin(value, thresholds) {
  if (!Number.isFinite(value) || !thresholds) return 'missing';
  if (thresholds[0] === thresholds[1]) return value < thresholds[0] ? 'lower tercile' : value > thresholds[1] ? 'upper tercile' : 'middle tercile';
  return value <= thresholds[0] ? 'lower tercile' : value <= thresholds[1] ? 'middle tercile' : 'upper tercile';
}

function grouped({ candidates, fills, diagnosticFills = fills, horizonMs, feature, bins, thresholds }) {
  return bins.map(bin => {
    const inBin = row => feature === 'flow' ? flowBin(row.alignedFlow, thresholds) === bin : row[feature] === bin;
    const opportunities = candidates.filter(inBin);
    const proxyFills = opportunities.filter(x => x.proxyFilled).length;
    const markRows = rows => rows.filter(x => inBin(x.candidate))
      .map(x => ({ ...x, marketId: x.candidate.marketId }));
    const eventTimeDiagnostic = summarize(markRows(diagnosticFills));
    const executable = summarize(markRows(fills));
    return {
      bin,
      candidateQuotes: opportunities.length,
      strictThroughProxies: proxyFills,
      proxyFillRate: opportunities.length ? proxyFills / opportunities.length : null,
      eventTimeDiagnostic,
      executable,
      markout: executable,
    };
  });
}

export function createConditionalFillMarkoutV1(options = {}) {
  const config = { ...DEFAULTS, ...options };
  const { latencyMs, quoteTtlMs, sampleEveryMs, maximumMarkoutDelayMs, exitResponseLatencyMs, maximumExecutableBookAgeMs, purgeMs, markoutHorizonsMs } = config;
  if (!Array.isArray(markoutHorizonsMs) || !markoutHorizonsMs.length || !markoutHorizonsMs.every(n => Number.isSafeInteger(n) && n > 0) || new Set(markoutHorizonsMs).size !== markoutHorizonsMs.length || markoutHorizonsMs.some((n, i) => i > 0 && n <= markoutHorizonsMs[i - 1])) throw Error('MARKOUT_HORIZONS');
  const minimumPurgeMs = Math.max(...markoutHorizonsMs) + quoteTtlMs + latencyMs + exitResponseLatencyMs + maximumMarkoutDelayMs;
  if (![latencyMs, quoteTtlMs, sampleEveryMs, maximumMarkoutDelayMs, exitResponseLatencyMs, maximumExecutableBookAgeMs, purgeMs].every(Number.isSafeInteger) || latencyMs < 0 || quoteTtlMs < 1 || sampleEveryMs < quoteTtlMs + latencyMs || maximumMarkoutDelayMs < 0 || exitResponseLatencyMs < 0 || maximumExecutableBookAgeMs < 0 || purgeMs < minimumPurgeMs) throw Error('MARKOUT_POLICY');

  const candidates = [], fills = [], previous = new Map(), lastSlot = new Map(), pending = new Map(), versions = new Map(), tradeSeen = new Map();
  let lastObservedNs = -1n;
  const counts = { records: 0, books: 0, acceptedBookPoints: 0, staleBookPoints: 0, tradesSeen: 0, duplicateTrades: 0, invalidTrades: 0, candidateQuotes: 0, expiredUnfilledQuotes: 0, boundaryCancelledQuotes: 0, strictThroughProxies: 0 };

  function expire(exchangeId, atNs) {
    const orders = pending.get(exchangeId) ?? [];
    const keep = [];
    for (const order of orders) {
      if (atNs >= order.expiresAtNs) {
        order.candidate.status = 'expired_unfilled';
        order.candidate.activeDurationMs = elapsedMs(order.activationAtNs, order.expiresAtNs);
        counts.expiredUnfilledQuotes++;
      } else keep.push(order);
    }
    pending.set(exchangeId, keep);
  }

  function boundary(exchangeId, at = null) {
    previous.delete(exchangeId);
    versions.delete(exchangeId);
    const atNs = at === null ? lastObservedNs : instantNs(at);
    for (const order of pending.get(exchangeId) ?? []) {
      const activated = atNs >= order.activationAtNs;
      order.candidate.status = activated ? 'boundary_cancelled_after_activation' : 'boundary_cancelled_before_activation';
      const endNs = atNs < order.expiresAtNs ? atNs : order.expiresAtNs;
      order.candidate.activeDurationMs = elapsedMs(order.activationAtNs, endNs);
      counts.boundaryCancelledQuotes++;
    }
    pending.delete(exchangeId);
  }

  function resetFeatures(exchangeId) { previous.delete(exchangeId); }

  function observeBatch({ observedAt, books = [], trades = [] }) {
    const observedNs = instantNs(observedAt), observedMs = Number(observedNs / 1000000n);
    if (observedNs < lastObservedNs) throw Error('MARKOUT_CLOCK_ROLLBACK');
    lastObservedNs = observedNs;
    counts.records++;

    const orderedTrades = [...trades].sort((a, b) => compareNs(instantNs(a.executedAt), instantNs(b.executedAt)) || String(a.id).localeCompare(String(b.id)));
    for (const trade of orderedTrades) {
      counts.tradesSeen++;
      try {
        if (!trade || typeof trade.marketId !== 'string' || typeof trade.exchangeId !== 'string' || typeof trade.id !== 'string' || !Number.isSafeInteger(trade.sequence) || String(trade.sequence) !== trade.id || !Number.isSafeInteger(trade.quantity) || trade.quantity < 1) throw Error('TRADE_SCHEMA');
        const executedNs = instantNs(trade.executedAt);
        if (executedNs > observedNs) throw Error('TRADE_FUTURE');
        const normalized = normalizeOfficialFillTick(trade.price);
        const signature = `${trade.marketId}|${trade.exchangeId}|${trade.executedAt}|${normalized.tick}|${trade.quantity}`;
        const identity = `${trade.marketId}|${trade.exchangeId}|${trade.id}`;
        const priorSignature = tradeSeen.get(identity);
        if (priorSignature !== undefined) {
          if (priorSignature !== signature) throw Error('TRADE_IDENTITY_CONFLICT');
          counts.duplicateTrades++;
          continue;
        }
        tradeSeen.set(identity, signature);
        const price = amount(normalized.price);
        expire(trade.exchangeId, executedNs);
        const orders = pending.get(trade.exchangeId) ?? [];
        const index = orders.findIndex(order => order.candidate.marketId === trade.marketId && observedNs > order.candidate.decisionNs && executedNs >= order.activationAtNs && executedNs < order.expiresAtNs && (order.candidate.action === 'buy' ? price <= order.candidate.limit - TICK : price >= order.candidate.limit + TICK));
        if (index < 0) continue;
        const [order] = orders.splice(index, 1);
        pending.set(trade.exchangeId, orders);
        order.candidate.status = 'strict_through_proxy';
        order.candidate.proxyFilled = true;
        order.candidate.fillAt = trade.executedAt;
        order.candidate.activeDurationMs = elapsedMs(order.activationAtNs, executedNs);
        const fill = {
          candidate: order.candidate,
          fillAtNs: executedNs,
          fillKnownAtNs: observedNs,
          eventTimeDiagnosticMarks: Object.fromEntries(markoutHorizonsMs.map(h => [h, null])),
          executableMarks: Object.fromEntries(markoutHorizonsMs.map(h => [h, null])),
        };
        fills.push(fill);
        counts.strictThroughProxies++;
      } catch (error) {
        if (error.message === 'TRADE_IDENTITY_CONFLICT') throw error;
        counts.invalidTrades++;
      }
    }

    for (const book of books) {
      counts.books++;
      const exchangeId = book.exchangeId;
      expire(exchangeId, observedNs);
      const seq = BigInt(book.sequence), sourceNs = instantNs(book.sourceAt), receivedNs = instantNs(book.receivedAt);
      if (sourceNs > observedNs || receivedNs > observedNs || instantNs(book.requestStartedAt) > receivedNs) throw Error('MARKOUT_BOOK_CLOCK');
      const oldVersion = versions.get(exchangeId);
      if (oldVersion && (seq < oldVersion.sequence || seq === oldVersion.sequence && sourceNs <= oldVersion.sourceNs)) { counts.staleBookPoints++; continue; }
      versions.set(exchangeId, { sequence: seq, sourceNs });
      const bid = amount(book.bids[0]?.price), ask = amount(book.asks[0]?.price), bidQty = BigInt(book.bids[0]?.quantity), askQty = BigInt(book.asks[0]?.quantity);
      if (bid <= 0n || ask <= bid || bidQty <= 0n || askQty <= 0n) throw Error('MARKOUT_BOOK');
      counts.acceptedBookPoints++;

      for (const fill of fills) {
        if (fill.candidate.exchangeId !== exchangeId || observedNs <= fill.fillKnownAtNs) continue;
        for (const horizon of markoutHorizonsMs) {
          const eventMarks = fill.eventTimeDiagnosticMarks;
          const executableMarks = fill.executableMarks;
          const eventTargetNs = fill.fillAtNs + BigInt(horizon) * NS_PER_MS;
          if (eventMarks[horizon] === null && sourceNs >= eventTargetNs) {
            const eventDelayNs = sourceNs - eventTargetNs;
            if (eventDelayNs > BigInt(maximumMarkoutDelayMs) * NS_PER_MS) {
              eventMarks[horizon] = { censored: true, sourceAt: book.sourceAt, availableAt: book.receivedAt };
            } else {
              const candidate = fill.candidate, midpoint = (bid + ask) / 2n;
              const executableExit = candidate.action === 'buy' ? bid : ask;
              const executableNet = candidate.action === 'buy' ? executableExit - candidate.limit : candidate.limit - executableExit;
              const midMarkout = candidate.action === 'buy' ? midpoint - candidate.limit : candidate.limit - midpoint;
              eventMarks[horizon] = { executableNet, midMarkout, sourceAt: book.sourceAt, availableAt: book.receivedAt, observedDelayMs: elapsedMs(eventTargetNs, sourceNs) };
            }
          }

          const responseTargetNs = fill.fillKnownAtNs + BigInt(exitResponseLatencyMs) * NS_PER_MS;
          if (executableMarks[horizon] !== null || sourceNs < eventTargetNs || receivedNs < responseTargetNs) continue;
          const eventDelayNs = sourceNs - eventTargetNs;
          const responseDelayNs = receivedNs - responseTargetNs;
          const exitDelayNs = eventDelayNs > responseDelayNs ? eventDelayNs : responseDelayNs;
          if (exitDelayNs > BigInt(maximumMarkoutDelayMs) * NS_PER_MS) {
            executableMarks[horizon] = { censored: true, sourceAt: book.sourceAt, availableAt: book.receivedAt };
            continue;
          }
          if (observedNs - sourceNs > BigInt(maximumExecutableBookAgeMs) * NS_PER_MS) continue;
          const candidate = fill.candidate, midpoint = (bid + ask) / 2n;
          const executableExit = candidate.action === 'buy' ? bid : ask;
          const executableNet = candidate.action === 'buy' ? executableExit - candidate.limit : candidate.limit - executableExit;
          const midMarkout = candidate.action === 'buy' ? midpoint - candidate.limit : candidate.limit - midpoint;
          executableMarks[horizon] = { executableNet, midMarkout, sourceAt: book.sourceAt, availableAt: book.receivedAt, observedDelayMs: Math.max(elapsedMs(eventTargetNs, sourceNs), elapsedMs(responseTargetNs, receivedNs)) };
        }
      }

      const prior = previous.get(exchangeId);
      let flow = null;
      if (prior && observedMs > prior.at && observedMs - prior.at <= 120000) {
        const pressure = (bid >= prior.bid ? bidQty : 0n) - (bid <= prior.bid ? prior.bidQty : 0n) - (ask <= prior.ask ? askQty : 0n) + (ask >= prior.ask ? prior.askQty : 0n);
        const denominator = bidQty + askQty + prior.bidQty + prior.askQty;
        flow = denominator > 0n ? 2 * Number(pressure) / Number(denominator) : null;
      }
      const imbalance = Number(bidQty - askQty) / Number(bidQty + askQty);
      const spreadTicks = Number((ask - bid) / TICK);
      const slot = Math.floor(observedMs / sampleEveryMs), priorSlot = lastSlot.get(exchangeId);
      previous.set(exchangeId, { at: observedMs, bid, ask, bidQty, askQty });
      expire(exchangeId, observedNs);
      if (priorSlot === slot) continue;
      lastSlot.set(exchangeId, slot);
      for (const action of ['buy', 'sell']) {
        const limit = action === 'buy' ? bid : ask, queueAhead = Number(action === 'buy' ? bidQty : askQty);
        const activationAtNs = observedNs + BigInt(latencyMs) * NS_PER_MS;
        const expiresAtNs = activationAtNs + BigInt(quoteTtlMs) * NS_PER_MS;
        const candidate = {
          id: candidates.length + 1,
          marketId: book.marketId,
          exchangeId,
          action,
          decisionAt: observedAt,
          decisionMs: observedMs,
          decisionNs: observedNs,
          activationAtNs,
          expiresAtNs,
          limit,
          spreadTicks,
          queueAhead,
          alignedImbalance: action === 'buy' ? imbalance : -imbalance,
          alignedFlow: Number.isFinite(flow) ? action === 'buy' ? flow : -flow : null,
          status: 'resting',
          proxyFilled: false,
        };
        candidates.push(candidate);
        const orders = pending.get(exchangeId) ?? [];
        orders.push({ candidate, activationAtNs, expiresAtNs });
        pending.set(exchangeId, orders);
        counts.candidateQuotes++;
      }
    }
  }

  function report({ splitAt, source = null } = {}) {
    const splitMs = ms(splitAt), devEnd = splitMs - purgeMs;
    const flowCalibration = candidates.filter(x => x.decisionMs < devEnd && Number.isFinite(x.alignedFlow)).map(x => x.alignedFlow).sort((a, b) => a - b);
    const flowThresholds = flowCalibration.length >= 10 ? [quantile(flowCalibration, 1 / 3), quantile(flowCalibration, 2 / 3)] : null;
    for (const candidate of candidates) {
      candidate.spreadBin = spreadBin(candidate.spreadTicks);
      candidate.depthBin = depthBin(candidate.queueAhead);
    }
    const foldFor = candidate => candidate.decisionMs < devEnd ? 'development' : candidate.decisionMs >= splitMs ? 'heldOut' : 'purged';
    const candidateCounts = Object.fromEntries(['development', 'heldOut', 'purged'].map(fold => [fold, candidates.filter(x => foldFor(x) === fold).length]));
    const tables = [], fillByCandidate = new Map(fills.map(fill => [fill.candidate, fill]));
    for (const fold of ['development', 'heldOut']) for (const action of ['buy', 'sell']) for (const horizon of markoutHorizonsMs) {
      const selectedCandidates = candidates.filter(x => foldFor(x) === fold && x.action === action);
      const selectFills = marksKey => fills.filter(fill => {
        const label = fill[marksKey][horizon];
        if (!label || label.censored || foldFor(fill.candidate) !== fold || fill.candidate.action !== action) return false;
        if (fold === 'development' && ms(label.availableAt ?? label.sourceAt) >= devEnd) return false;
        return true;
      }).map(fill => ({ ...fill, executableNet: fill[marksKey][horizon].executableNet, midMarkout: fill[marksKey][horizon].midMarkout }));
      const selectedEventTimeFills = selectFills('eventTimeDiagnosticMarks');
      const selectedExecutableFills = selectFills('executableMarks');
      const allFilledCandidates = selectedCandidates.filter(x => x.proxyFilled);
      const unresolvedMarks = allFilledCandidates.filter(candidate => {
        const label = fillByCandidate.get(candidate)?.executableMarks[horizon];
        return !label || label.censored || fold === 'development' && label && !label.censored && ms(label.availableAt ?? label.sourceAt) >= devEnd;
      }).length;
      const activeDurationMs = candidate => {
        if (candidate.activeDurationMs !== undefined) return candidate.activeDurationMs;
        const endNs = lastObservedNs < candidate.expiresAtNs ? lastObservedNs : candidate.expiresAtNs;
        return elapsedMs(candidate.activationAtNs, endNs);
      };
      const activeCandidates = selectedCandidates.filter(candidate => {
        return activeDurationMs(candidate) > 0;
      });
      const quoteHoursMs = activeCandidates.reduce((sum, candidate) => sum + activeDurationMs(candidate), 0);
      const eventDiagnosticSummary = summarize(selectedEventTimeFills);
      const executableSummary = summarize(selectedExecutableFills);
      tables.push({
        fold, action, horizonMs: horizon,
        candidateQuotes: selectedCandidates.length,
        activatedQuotes: activeCandidates.length,
        quoteHoursAtRisk: quoteHoursMs / 3600000,
        strictThroughProxies: allFilledCandidates.length,
        markoutSamples: selectedExecutableFills.length,
        eventTimeDiagnosticSamples: selectedEventTimeFills.length,
        unresolvedOrPurgedMarkouts: unresolvedMarks,
        proxyFillRate: activeCandidates.length ? allFilledCandidates.length / activeCandidates.length : null,
        proxiesPer100QuoteHours: quoteHoursMs > 0 ? allFilledCandidates.length / (quoteHoursMs / 3600000) * 100 : null,
        eventTimeDiagnostic: eventDiagnosticSummary,
        executable: executableSummary,
        overall: executableSummary,
        bySpread: grouped({ candidates: selectedCandidates, fills: selectedExecutableFills, diagnosticFills: selectedEventTimeFills, horizonMs: horizon, feature: 'spreadBin', bins: ['1 tick', '2 ticks', '3+ ticks'], thresholds: flowThresholds }),
        byQueueAhead: grouped({ candidates: selectedCandidates, fills: selectedExecutableFills, diagnosticFills: selectedEventTimeFills, horizonMs: horizon, feature: 'depthBin', bins: ['1-10', '11-50', '51-250', '251+'], thresholds: flowThresholds }),
        byAlignedFlowTercile: grouped({ candidates: selectedCandidates, fills: selectedExecutableFills, diagnosticFills: selectedEventTimeFills, horizonMs: horizon, feature: 'flow', bins: ['lower tercile', 'middle tercile', 'upper tercile', 'missing'], thresholds: flowThresholds }),
      });
    }
    return {
      schemaVersion: 1,
      mode: 'causal_captured_conditional_maker_fill_proxy_markout_v1',
      source,
      definition: {
        quote: 'hypothetical YES bid/ask at the captured best price; one paired candidate per exchange every 30 seconds',
        activationLatencyMs: latencyMs,
        quoteTtlAfterActivationMs: quoteTtlMs,
        fillProxy: 'captured scoped trade at least one tick through limit after activation; one unit at limit; no aggressor/queue rank known',
        markout: 'legacy event-time diagnostic only: first captured book source point at or after fill plus horizon; not an executable response-time mark',
        eventTimeDiagnostic: 'first eligible captured book source point at or after fill plus horizon, within maximumMarkoutDelayMs; buy exits at future bid, sell exits at future ask; retrospective diagnostic only',
        executableExit: 'first fresh captured book with source time at or after fill time plus horizon and local receipt time at or after trade report receipt plus exitResponseLatencyMs; maximumMarkoutDelayMs bounds both event-time horizon delay and local response-time delay; development labels must also have been available before the development cutoff',
        spreadIncluded: 'entry joins the observed best side; liquidation crosses the later spread using its executable contra quote',
        fees: '0.000000',
        stress: 'subtract one tick from entry and one tick from exit (0.010000 total per unit) from executable net; shown separately',
        horizonMs: markoutHorizonsMs,
        maximumMarkoutDelayMs,
        exitResponseLatencyMs,
        maximumExecutableBookAgeMs,
        receiptLatencyBoundMs: null,
        chronologicalSplit: { splitAt, purgeMs, minimumPurgeMs, developmentDecisionBefore: new Date(devEnd).toISOString(), heldOutDecisionFrom: splitAt, purgedCandidateQuotes: candidateCounts.purged, developmentMarkAvailabilityBefore: new Date(devEnd).toISOString() },
        bins: { spreadTicks: ['1', '2', '3+'], queueAheadShares: ['1-10', '11-50', '51-250', '251+'], alignedFlow: 'tercile cutpoints from development quote features only; when both cutpoints tie, split strictly below / equal / strictly above the tied value; no outcome-guided selection' },
      },
      counts: {
        ...counts,
        expiredUnfilledQuotes: counts.expiredUnfilledQuotes + [...pending.values()].flat().filter(x => x.expiresAtNs <= lastObservedNs).length,
        candidateQuotesByFold: candidateCounts,
        unfilledPendingAtEnd: [...pending.values()].flat().filter(x => x.expiresAtNs > lastObservedNs).length,
        filledProxyCount: fills.length,
        flowCalibrationQuotes: flowCalibration.length,
      },
      flowThresholds,
      tables,
      limitations: [
        'Trade-through is a counterfactual fill proxy, not proof an order was accepted or filled; queue priority and aggressor side are unknown.',
        'One unit at the captured limit is assumed; deeper fills, capital, inventory, venue matching rules, cancellations, and latency variation are not simulated.',
        'Event-time markouts are retained as retrospective diagnostics; executable marks require both the event-time horizon and local receipt after the trade report plus modeled exit response, with a fresh source timestamp. Exchange source time and local receipt time are intentionally gated in their own clock domains. Trade-report receipt delay has no assumed upper bound; development marks are excluded unless their captured book receipt/availability is before the development cutoff.',
        'Spread is reflected in executable liquidation; zero platform fees are assumed. One-tick-each-way stress is an additional sensitivity, not an observed fee.',
        'Candidates overlap across markets and outcomes are not independent; small samples and shared market shocks preclude a profitability claim.',
        'Conditional bins are descriptive and prespecified; no strategy selection or actual contest execution is authorized by this analysis.',
      'Candidate quote exposure ends at capture disconnect/invalidation boundaries; fill rates use observed active quote time but captured trade coverage is incomplete.',
      ],
      actualFillsProven: false,
      profitabilityEstablished: false,
      contestOrders: 0,
      networkRequests: 0,
    };
  }

  return { observeBatch, boundary, resetFeatures, report };
}

