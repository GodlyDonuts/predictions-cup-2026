// Replace an existing recovery slot, never add a client or a second GET slot.
import {createRealtimeHost} from './realtime-host.mjs';
import {instant} from '../paper/fixed.mjs';

const MAX_PRIORITY_MARKETS = 4;
const MAX_PRIORITY_BURST = 3;
const DEFAULT_ACTIVITY_HALF_LIFE_MS = 120000;
const DEFAULT_ACTIVITY_WINDOW_MS = 120000;

function activityCount(value) {
  return Number.isSafeInteger(value) && value >= 0 ? value : 0;
}

export function createRealtimePriorityHostV2({getRecoveryPriority = () => null,
  maximumPriorityAgeMs = 120000, activityHalfLifeMs = DEFAULT_ACTIVITY_HALF_LIFE_MS,
  activityWindowMs = DEFAULT_ACTIVITY_WINDOW_MS, ...options}) {
  if (typeof getRecoveryPriority !== 'function' || !Number.isSafeInteger(maximumPriorityAgeMs)
    || maximumPriorityAgeMs < 1 || maximumPriorityAgeMs > 900000
    || !Number.isSafeInteger(activityHalfLifeMs) || activityHalfLifeMs < 1000 || activityHalfLifeMs > 900000
    || !Number.isSafeInteger(activityWindowMs) || activityWindowMs < 1000 || activityWindowMs > 900000)
    throw Error('REALTIME_PRIORITY_POLICY');

  const base = createRealtimeHost(options);
  const members = new Set(options.scope.markets.map(m => m.marketId));
  const now = options.now ?? Date.now;
  const activity = new Map();
  let busy = false, closed = false, streak = 0, cursor = 0;
  let attempts = 0, successes = 0, backgroundCalls = 0, staleReports = 0, lastCode = null;
  let lastPrioritySource = 'none', lastPriorityMarketIds = [];

  function sampleActivity(markets, at) {
    for (const market of markets) {
      // Raw events and accepted empty batches can still be noise. Only newly
      // applied books and deduplicated trades are evidence of market activity.
      const books = activityCount(market.metrics?.streamBookUpdates);
      const trades = activityCount(market.metrics?.trades);
      const previous = activity.get(market.marketId);
      if (!previous || !Number.isSafeInteger(at) || at < previous.at) {
        // The initial cumulative counters do not reveal when past activity
        // happened, so only deltas observed by this process earn priority.
        activity.set(market.marketId, {books, trades, at, bookScore: 0, tradeScore: 0, lastActiveAt: null});
        continue;
      }

      if (at - previous.at > activityWindowMs) {
        // We cannot locate events inside an unobserved interval. Discard them
        // instead of falsely treating old accumulated counts as fresh.
        activity.set(market.marketId, {books, trades, at, bookScore: 0, tradeScore: 0, lastActiveAt: null});
        continue;
      }

      const decay = 2 ** (-(at - previous.at) / activityHalfLifeMs);
      const bookDelta = books >= previous.books ? books - previous.books : books;
      const tradeDelta = trades >= previous.trades ? trades - previous.trades : trades;
      const active = bookDelta > 0 || tradeDelta > 0;
      activity.set(market.marketId, {
        books,
        trades,
        at,
        bookScore: previous.bookScore * decay + bookDelta,
        tradeScore: previous.tradeScore * decay + tradeDelta,
        lastActiveAt: active ? at : previous.lastActiveAt
      });
    }
  }

  function liveActivityMarkets(markets, at) {
    return markets
      .filter(market => !market.ready && market.needsSnapshot && !market.reasons.includes('SETTLEMENT'))
      .map(market => ({marketId: market.marketId, score: activity.get(market.marketId)}))
      .filter(({score}) => score?.lastActiveAt !== null && score?.lastActiveAt !== undefined
        && at - score.lastActiveAt <= activityWindowMs && (score.tradeScore > 0 || score.bookScore > 0))
      .sort((a, b) => b.score.tradeScore - a.score.tradeScore
        || b.score.bookScore - a.score.bookScore || a.marketId.localeCompare(b.marketId))
      .slice(0, MAX_PRIORITY_MARKETS)
      .map(candidate => candidate.marketId);
  }

  const background = async () => {
    streak = 0;
    backgroundCalls++;
    lastPrioritySource = 'background_rotation';
    lastPriorityMarketIds = [];
    return base.service();
  };

  async function service() {
    if (closed || busy) return false;
    busy = true;
    try {
      const status = base.status();
      const at = now();
      sampleActivity(status.markets, at);
      if (status.transport.state !== 'subscribed' || streak >= MAX_PRIORITY_BURST)
        return await background();

      const report = getRecoveryPriority();
      let priorityMarketIds = [];
      let source = 'live_activity';
      if (report !== null) {
        if (!Array.isArray(report.marketIds) || report.marketIds.length > MAX_PRIORITY_MARKETS
          || new Set(report.marketIds).size !== report.marketIds.length
          || report.marketIds.some(id => !members.has(id))) throw Error('REALTIME_PRIORITY_SCOPE');
        const age = at - instant(report.at);
        if (age < 0 || age > maximumPriorityAgeMs) staleReports++;
        else if (report.marketIds.length > 0) {
          priorityMarketIds = report.marketIds;
          source = 'session_flow_report';
        }
      }

      if (priorityMarketIds.length === 0) priorityMarketIds = liveActivityMarkets(status.markets, at);
      else if (!priorityMarketIds.some(id => status.markets.some(market => market.marketId === id
        && !market.ready && market.needsSnapshot && !market.reasons.includes('SETTLEMENT')))) {
        priorityMarketIds = liveActivityMarkets(status.markets, at);
        source = 'live_activity';
      }
      if (priorityMarketIds.length === 0) return await background();

      for (let i = 0; i < priorityMarketIds.length; i++) {
        const index = (cursor + i) % priorityMarketIds.length;
        const marketId = priorityMarketIds[index];
        const view = status.markets.find(market => market.marketId === marketId);
        if (!view || view.ready || !view.needsSnapshot || view.reasons.includes('SETTLEMENT')) continue;
        const ticket = base.beginSharedSnapshot(marketId);
        if (!ticket) continue;

        cursor = (index + 1) % priorityMarketIds.length;
        streak++;
        attempts++;
        lastPrioritySource = source;
        lastPriorityMarketIds = priorityMarketIds;
        try {
          const response = await base.readSharedSnapshot(ticket);
          if (base.completeSharedSnapshot(ticket, response)) {
            successes++;
            lastCode = null;
          } else lastCode = 'PRIORITY_RECOVERY_REJECTED';
        } catch {
          base.failSharedSnapshot(ticket);
          lastCode = 'PRIORITY_RECOVERY_UNAVAILABLE';
        }
        return true; // Reuses the existing recovery slot; no second GET.
      }
      return await background();
    } finally {
      busy = false;
    }
  }

  function status() {
    return {...base.status(), priorityRecovery: {
      attempts,
      successes,
      backgroundCalls,
      staleReports,
      priorityStreak: streak,
      maximumPriorityBurst: MAX_PRIORITY_BURST,
      maximumPriorityMarkets: MAX_PRIORITY_MARKETS,
      maximumPriorityAgeMs,
      activityHalfLifeMs,
      activityWindowMs,
      liveActivityMarketsTracked: activity.size,
      lastPrioritySource,
      lastPriorityMarketIds,
      lastCode,
      additionalApiClients: 0,
      additionalRecoverySlots: 0
    }};
  }

  async function stop() {
    closed = true;
    await base.stop();
  }

  return Object.freeze({...base, service, status, stop});
}
