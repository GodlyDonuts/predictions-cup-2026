import { amount, money } from '../paper/fixed.mjs';

const UNIT = 1_000_000n;
const TICK = 5_000n;
const POLICY = Object.freeze({
  lookbackMs: 30 * 60_000,
  minimumHistoryPoints: 20,
  minimumHistorySpanMs: 20 * 60_000,
  maximumGapMs: 120_000,
  sampleEveryMs: 10_000,
  dipTicks: 2,
  entryLatencyMs: 3_000,
  entryTtlMs: 30_000,
  reboundTicks: 1,
  maximumHoldMs: 15 * 60_000,
  exitLatencyMs: 3_000,
  exitTtlMs: 30_000,
  cooldownMs: 10 * 60_000,
  purgeMs: 35 * 60_000,
});

const toMs = value => {
  const ms = typeof value === 'number' ? value : Date.parse(value);
  if (!Number.isSafeInteger(ms) || ms < 0) throw Error('DIP_CLOCK');
  return ms;
};
const medianUpper = values => {
  const sorted = [...values].sort((a, b) => a < b ? -1 : a > b ? 1 : 0);
  return sorted[Math.floor(sorted.length / 2)];
};
const compare = (a, b) => a < b ? -1 : a > b ? 1 : 0;
const foldAt = (at, splitAt) => at < splitAt - POLICY.purgeMs ? 'development' : at < splitAt ? 'purged' : 'heldOut';

function emptyFold() {
  return { signals: 0, entryFillProxies: 0, entryExpired: 0, completed: 0, reboundExits: 0,
    horizonExits: 0, censoredAfterEntry: 0, censoredAfterEntryByReason: new Map(),
    entryExpiredByReason: new Map(), pendingAtEnd: 0, netPerShare: [], markets: new Set() };
}

function incrementReason(counts, reason) {
  counts.set(reason, (counts.get(reason) ?? 0) + 1);
}

function summarize(fold) {
  const rows = fold.netPerShare.slice().sort(compare);
  const n = rows.length;
  const mean = n ? rows.reduce((sum, x) => sum + x, 0n) / BigInt(n) : null;
  const median = n % 2 ? rows[Math.floor(n / 2)] : n ? (rows[n / 2 - 1] + rows[n / 2]) / 2n : null;
  return {
    signals: fold.signals,
    entryFillProxies: fold.entryFillProxies,
    entryExpired: fold.entryExpired,
    completed: fold.completed,
    reboundExits: fold.reboundExits,
    horizonExits: fold.horizonExits,
    censoredAfterEntry: fold.censoredAfterEntry,
    censoredAfterEntryByReason: Object.fromEntries([...fold.censoredAfterEntryByReason].sort(([a], [b]) => a.localeCompare(b))),
    entryExpiredByReason: Object.fromEntries([...fold.entryExpiredByReason].sort(([a], [b]) => a.localeCompare(b))),
    pendingAtEnd: fold.pendingAtEnd,
    marketsWithSignals: fold.markets.size,
    closedMarkouts: n,
    meanExecutableNetPerShare: mean === null ? null : money(mean),
    medianExecutableNetPerShare: median === null ? null : money(median),
    oneTickEachWayStressMean: mean === null ? null : money(mean - 2n * TICK),
    positiveFraction: n ? rows.filter(x => x > 0n).length / n : null,
  };
}

/**
 * Causal one-share dip/rebound study. Signal anchors use only earlier sampled asks.
 * A delayed, displayed-top-depth limit entry and a delayed executable-bid exit are
 * explicitly counterfactual proxies; neither implies queue priority or actual fills.
 */
export function createCausalDipReversionV1({ splitAt }) {
  const split = toMs(splitAt);
  const byExchange = new Map();
  const folds = { development: emptyFold(), heldOut: emptyFold(), purged: emptyFold() };
  const counts = { observations: 0, sampledPoints: 0, invalidBooks: 0, boundaries: 0,
    gapResets: 0, duplicateSampleSlots: 0, entryFillProxies: 0, closedTrades: 0 };
  let lastObservedAt = -1;

  const getExchange = exchangeId => {
    let value = byExchange.get(exchangeId);
    if (!value) {
      value = { lastEventAt: -1, lastSampleAt: -1, sides: new Map() };
      for (const side of ['YES', 'NO']) value.sides.set(side, { history: [], active: null, cooldownUntil: -1 });
      byExchange.set(exchangeId, value);
    }
    return value;
  };

  function censorSide(state, reason) {
    const active = state.active;
    if (!active) return;
    const row = folds[active.fold];
    if (active.phase === 'open' || active.phase === 'pendingExit') {
      row.censoredAfterEntry++;
      incrementReason(row.censoredAfterEntryByReason, reason);
    } else {
      row.entryExpired++;
      incrementReason(row.entryExpiredByReason, reason);
    }
    active.censorReason = reason;
    state.active = null;
  }

  function boundary(exchangeId, at, reason = 'source_boundary') {
    const ms = at === undefined ? lastObservedAt : toMs(at);
    if (ms < lastObservedAt) throw Error('DIP_CLOCK_ROLLBACK');
    lastObservedAt = ms;
    const keys = exchangeId === null ? [...byExchange.keys()] : [String(exchangeId)];
    for (const id of keys) {
      const entry = byExchange.get(id);
      if (!entry) continue;
      for (const side of entry.sides.values()) {
        censorSide(side, reason);
        side.history.length = 0;
        side.cooldownUntil = ms + POLICY.cooldownMs;
      }
      entry.lastEventAt = ms;
      entry.lastSampleAt = -1;
    }
    counts.boundaries++;
  }

  function observeBook({ marketId, exchangeId, at, bid, ask, bidQuantity, askQuantity }) {
    const ms = toMs(at);
    if (ms < lastObservedAt) throw Error('DIP_CLOCK_ROLLBACK');
    lastObservedAt = ms;
    counts.observations++;
    if (typeof marketId !== 'string' || typeof exchangeId !== 'string') throw Error('DIP_SCOPE');
    const yesBid = amount(bid), yesAsk = amount(ask);
    if (!Number.isSafeInteger(bidQuantity) || bidQuantity < 0 || !Number.isSafeInteger(askQuantity) || askQuantity < 0
      || yesBid < 0n || yesAsk <= 0n || yesAsk >= UNIT || yesBid >= yesAsk) {
      counts.invalidBooks++;
      return;
    }
    const exchange = getExchange(exchangeId);
    if (exchange.lastEventAt >= 0 && ms < exchange.lastEventAt) throw Error('DIP_EXCHANGE_CLOCK_ROLLBACK');
    if (exchange.lastEventAt >= 0 && ms - exchange.lastEventAt > POLICY.maximumGapMs) {
      counts.gapResets++;
      for (const side of exchange.sides.values()) {
        censorSide(side, 'source_gap');
        side.history.length = 0;
        side.cooldownUntil = ms + POLICY.cooldownMs;
      }
      exchange.lastSampleAt = -1;
    }
    exchange.lastEventAt = ms;

    const sidePoints = [
      ['YES', yesBid, yesAsk, bidQuantity, askQuantity],
      ['NO', UNIT - yesAsk, UNIT - yesBid, askQuantity, bidQuantity],
    ];
    for (const [sideName, sideBid, sideAsk, sideBidQty, sideAskQty] of sidePoints) {
      const state = exchange.sides.get(sideName);
      const active = state.active;

      if (active?.phase === 'entryPending') {
        if (ms > active.entryExpiresAt) {
          folds[active.fold].entryExpired++;
          incrementReason(folds[active.fold].entryExpiredByReason, 'entry_ttl');
          state.active = null;
          state.cooldownUntil = ms + POLICY.cooldownMs;
        } else if (ms >= active.entryArrivesAt && sideAsk <= active.entryLimit && sideAskQty >= 1) {
          active.phase = 'open';
          active.entryAt = ms;
          active.entryPrice = sideAsk;
          active.targetPrice = sideAsk + BigInt(POLICY.reboundTicks) * TICK;
          folds[active.fold].entryFillProxies++;
          counts.entryFillProxies++;
        }
      } else if (active?.phase === 'open') {
        const heldMs = ms - active.entryAt;
        if (heldMs >= POLICY.maximumHoldMs) {
          active.phase = 'pendingExit';
          active.exitReason = 'horizon';
          active.exitRequestedAt = ms;
          active.exitExpiresAt = ms + POLICY.exitTtlMs;
          active.exitArrivesAt = ms + POLICY.exitLatencyMs;
        } else if (sideBidQty >= 1 && sideBid >= active.targetPrice) {
          active.phase = 'pendingExit';
          active.exitReason = 'rebound';
          active.exitRequestedAt = ms;
          active.exitExpiresAt = ms + POLICY.exitTtlMs;
          active.exitArrivesAt = ms + POLICY.exitLatencyMs;
        }
      }

      const pendingExit = state.active;
      if (pendingExit?.phase === 'pendingExit') {
        if (ms > pendingExit.exitExpiresAt) {
          folds[pendingExit.fold].censoredAfterEntry++;
          incrementReason(folds[pendingExit.fold].censoredAfterEntryByReason, 'exit_ttl');
          state.active = null;
          state.cooldownUntil = ms + POLICY.cooldownMs;
        } else if (ms >= pendingExit.exitArrivesAt && sideBidQty >= 1 && sideBid > 0n) {
          const pnl = sideBid - pendingExit.entryPrice;
          const row = folds[pendingExit.fold];
          row.completed++;
          row.netPerShare.push(pnl);
          row.markets.add(pendingExit.marketId);
          if (pendingExit.exitReason === 'rebound') row.reboundExits++;
          else row.horizonExits++;
          counts.closedTrades++;
          state.active = null;
          state.cooldownUntil = ms + POLICY.cooldownMs;
        }
      }

      const shouldSample = exchange.lastSampleAt < 0 || ms - exchange.lastSampleAt >= POLICY.sampleEveryMs;
      if (shouldSample) {
        const history = state.history;
        const floor = ms - POLICY.lookbackMs;
        while (history.length && history[0].at < floor) history.shift();
        const fold = foldAt(ms, split);
        if (!state.active && ms >= state.cooldownUntil && fold !== 'purged'
          && history.length >= POLICY.minimumHistoryPoints
          && history.at(-1).at - history[0].at >= POLICY.minimumHistorySpanMs
          && sideAskQty >= 1 && sideAsk < UNIT) {
          const reference = medianUpper(history.map(point => point.ask));
          const limit = sideAsk;
          if (reference - limit >= BigInt(POLICY.dipTicks) * TICK) {
            const row = folds[fold];
            row.signals++;
            row.markets.add(marketId);
            state.active = {
              phase: 'entryPending', fold, marketId, exchangeId, side: sideName,
              signalAt: ms, referencePrice: reference, entryLimit: limit,
              entryArrivesAt: ms + POLICY.entryLatencyMs,
              entryExpiresAt: ms + POLICY.entryLatencyMs + POLICY.entryTtlMs,
            };
          }
        }
        history.push({ at: ms, bid: sideBid, ask: sideAsk });
        exchange.lastSampleAt = ms;
        counts.sampledPoints++;
      } else {
        counts.duplicateSampleSlots++;
      }
    }
  }

  function report({ splitAt: reportedSplitAt, source = null } = {}) {
    if (reportedSplitAt !== undefined && toMs(reportedSplitAt) !== split) throw Error('DIP_SPLIT_MISMATCH');
    const rightCensoredAtEnd = { development: 0, heldOut: 0, purged: 0 };
    const pendingAtEnd = { development: 0, heldOut: 0, purged: 0 };
    for (const entry of byExchange.values()) for (const state of entry.sides.values()) {
      if (state.active?.phase === 'entryPending') pendingAtEnd[state.active.fold]++;
      else if (state.active) rightCensoredAtEnd[state.active.fold]++;
    }
    const summarizedFolds = Object.fromEntries(Object.entries(folds).map(([name, state]) => {
      const row = summarize(state);
      row.pendingAtEnd = pendingAtEnd[name];
      row.censoredAfterEntry += rightCensoredAtEnd[name];
      return [name, row];
    }));
    return {
      schemaVersion: 1,
      mode: 'causal_historical_dip_then_rebound',
      source,
      protocol: {
        ...POLICY,
        splitAt: new Date(split).toISOString(),
        feesPerShare: '0.000000',
        outcome: 'one-share ask-side entry proxy after 3s; one-share executable-bid exit proxy after 3s on +1 tick rebound; otherwise exit at first executable bid after 15m',
        signal: 'current ask at least 2 ticks below upper median of strictly earlier sampled asks in trailing 30m; requires 20 prior points spanning 20m',
        samplePeriod: 'one feature observation per market/side per 10s; every valid source point can execute a pending entry/exit after declared latency',
        developmentCutoff: new Date(split - POLICY.purgeMs).toISOString(),
        chronologicalPurgeMs: POLICY.purgeMs,
      },
      counts: { ...counts, marketCount: byExchange.size },
      folds: summarizedFolds,
      actualFillsProven: false,
      profitabilityEstablished: false,
      contestOrders: 0,
      networkRequests: 0,
      limitations: [
        'The saved capture is historical and has been examined for a separate microstructure question; the dip rule and outcomes here are a distinct descriptive replay, not untouched contest validation.',
        'Entry and exit are one-share top-of-book counterfactual fill proxies after fixed 3s delays; queue priority, matching uncertainty, partial fills, market impact, and order acceptance are unknown.',
        'Executable markouts buy at observed ask and sell at observed bid; platform fees are zero. The extra one-tick-each-way figure is a stress only, not a fee.',
        'Trades overlap in time across markets and YES/NO are complements; counts are not independent trials and do not establish scalable capital capacity.',
        'Positions without a source-supported exit before a boundary or capture end are censored, not marked at a stale price or counted as losses/wins.',
        'This offline analysis changes no paper or competition policy and does not imply actual execution.',
      ],
    };
  }

  return { observeBook, boundary, report, policy: POLICY };
}

export const causalDipReversionPolicyV1 = POLICY;
