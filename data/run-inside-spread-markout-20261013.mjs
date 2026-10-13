import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { amount, money, TICK } from '../paper/fixed.mjs';
import { instantNs } from '../feed/contract.mjs';
import { normalizeOfficialFillTick } from '../execution/api-tick-price.mjs';
import { journalLines } from '../live-paper/journal-lines.mjs';
import { engineBookPointsV1 } from '../paper/maker-engine-book-point-v1.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const RESULT_PATH = path.join(HERE, 'inside-spread-markout-20261013.json');
const RUNTIME_DIR = path.join(process.env.LOCALAPPDATA ?? '', 'SigTradingCup', 'paper-runtime');
const TICK_MICROS = TICK;
const HORIZONS_MS = Object.freeze([1000, 5000, 30000]);
const QUOTE_CADENCE_MS = 30000;
const ACTIVATION_LATENCY_MS = 3000;
const QUOTE_TTL_MS = 10000;
const EXIT_RESPONSE_MS = 3000;
const MAX_MARKOUT_DELAY_MS = 10000;
const MAX_BOOK_AGE_MS = 10000;
const MIN_SPREAD_TICKS = 2;
const DEVELOPMENT_SPLIT_AT = '2026-10-13T06:18:44.458Z';
const DEVELOPMENT_PURGE_MS = 60000;
const DEVELOPMENT_DECISION_CUTOFF = '2026-10-13T06:17:44.458Z';

const SOURCES = Object.freeze({
  development: Object.freeze({
    file: 'realtime-2026-10-13T02-55-28-953Z-d53812dd-ec75-4852-9267-59c7d5bb7898.jsonl',
    bytes: 1747459094,
    sha256: 'f9a20ef98e8e9c96d1229dbbc6a8eb17dd669e7c6f3f8d6e49337439945f4418',
    captureRecords: 467044,
    from: '2026-10-13T02:55:28.956Z',
    through: '2026-10-13T07:45:51.103Z',
  }),
  heldOut: Object.freeze({
    file: 'realtime-2026-10-13T19-52-33-253Z-9ecf5d7b-070f-4c01-a991-28a67a9f4a29.jsonl',
    bytes: 279458961,
    sha256: 'fd6a9552a5f70365ee78706fa1efff70e2a36e7f3dd55c1a932cc3795e1979e7',
  }),
});

const NS_PER_MS = 1000000n;
const isoNs = ns => {
  const ms = Number(ns / NS_PER_MS);
  return new Date(ms).toISOString();
};
const quoteKey = q => `${q.marketId}\u0000${q.exchangeId}\u0000${q.side}`;
const pairKey = (marketId, exchangeId) => `${marketId}\u0000${exchangeId}`;

export function insideLimitTicks({ bidTick, askTick, policy }) {
  if (!Number.isSafeInteger(bidTick) || !Number.isSafeInteger(askTick) || bidTick < 0 || askTick > 200 || askTick <= bidTick) throw Error('INSIDE_SPREAD_BOOK');
  const spreadTicks = askTick - bidTick;
  if (spreadTicks < MIN_SPREAD_TICKS) return null;
  if (policy === 'atBest') return { bidTick, askTick, spreadTicks };
  if (policy !== 'insideOneTick') throw Error('INSIDE_SPREAD_POLICY');
  const improvedBidTick = bidTick + 1;
  const improvedAskTick = askTick - 1;
  if (improvedBidTick < 1 || improvedBidTick > 199 || improvedAskTick < 1 || improvedAskTick > 199 || improvedBidTick > improvedAskTick) {
    return null;
  }
  return { bidTick: improvedBidTick, askTick: improvedAskTick, spreadTicks };
}

export function strictThrough({ side, tradeTick, limitTick }) {
  if (!Number.isSafeInteger(tradeTick) || !Number.isSafeInteger(limitTick)) return false;
  if (side === 'buy') return tradeTick <= limitTick - 1;
  if (side === 'sell') return tradeTick >= limitTick + 1;
  throw Error('INSIDE_SPREAD_SIDE');
}

const T_CRITICAL_975 = Object.freeze([0, 12.706, 4.303, 3.182, 2.776, 2.571, 2.447, 2.365, 2.306, 2.262, 2.228, 2.201, 2.179, 2.160, 2.145, 2.131, 2.120, 2.110, 2.101, 2.093, 2.086, 2.080, 2.074, 2.069, 2.064, 2.060, 2.056, 2.052, 2.048, 2.045, 2.042]);
function studentTCritical(df) {
  if (!Number.isSafeInteger(df) || df < 1) return null;
  if (df <= 30) return T_CRITICAL_975[df];
  // Cornish-Fisher expansion about the normal 97.5th percentile. At df > 30
  // this avoids the too-narrow fixed 1.96 interval while remaining stable.
  const z = 1.959963984540054, z2 = z * z;
  return z
    + (z * z2 + z) / (4 * df)
    + (5 * z * z2 * z2 + 16 * z * z2 + 3 * z) / (96 * df ** 2)
    + (3 * z * z2 ** 3 + 19 * z * z2 * z2 + 17 * z * z2 - 15 * z) / (384 * df ** 3)
    + (79 * z * z2 ** 4 + 776 * z * z2 ** 3 + 1482 * z * z2 * z2 - 1920 * z * z2 - 945 * z) / (92160 * df ** 4);
}

function clusterSummary(rows) {
  const byMarket = new Map();
  for (const row of rows) {
    const list = byMarket.get(row.marketId) ?? [];
    list.push(row.netMicros);
    byMarket.set(row.marketId, list);
  }
  const clusterMeans = [...byMarket.values()].map(values => values.reduce((sum, x) => sum + Number(x), 0) / values.length);
  const clusters = clusterMeans.length;
  if (!clusters) return {
    effectiveIndependentMarketCount: 0,
    equalWeightedMarketMeanNet: null,
    marketClusterStandardError: null,
    marketClusterCi95StudentT: null,
    uncertaintyNote: 'no marked market clusters',
  };
  const mean = clusterMeans.reduce((sum, x) => sum + x, 0) / clusters;
  if (clusters < 2) return {
    effectiveIndependentMarketCount: clusters,
    equalWeightedMarketMeanNet: money(BigInt(Math.trunc(mean))),
    marketClusterStandardError: null,
    marketClusterCi95StudentT: null,
    uncertaintyNote: 'one market cluster; clustered uncertainty is not estimable',
  };
  const varianceOfMean = clusterMeans.reduce((sum, x) => sum + (x - mean) ** 2, 0) / (clusters * (clusters - 1));
  const se = Math.sqrt(varianceOfMean);
  const margin = studentTCritical(clusters - 1) * se;
  return {
    effectiveIndependentMarketCount: clusters,
    equalWeightedMarketMeanNet: money(BigInt(Math.trunc(mean))),
    marketClusterStandardError: money(BigInt(Math.ceil(se))),
    marketClusterCi95StudentT: [money(BigInt(Math.floor(mean - margin))), money(BigInt(Math.ceil(mean + margin)))],
    uncertaintyNote: clusters < 5
      ? 'descriptive small-cluster t interval; too few markets for a reliable tail probability'
      : 'market-level t interval; assumes independent market clusters and is descriptive, not a profitability test',
  };
}

export function summarizeCell({ candidates, fills, horizonMs, fold, policy, side }) {
  const selectedCandidates = candidates.filter(x => x.fold === fold && x.policy === policy && x.side === side);
  const marked = fills.filter(x => x.fold === fold && x.policy === policy && x.side === side && x.marks[horizonMs]);
  const netMicros = marked.map(x => x.marks[horizonMs].netMicros);
  const total = netMicros.reduce((sum, x) => sum + x, 0n);
  const meanNet = netMicros.length ? total / BigInt(netMicros.length) : null;
  const stress = meanNet === null ? null : meanNet - 2n * TICK_MICROS;
  const candidatesByMarket = new Set(selectedCandidates.map(x => x.marketId)).size;
  const proxyFills = fills.filter(x => x.fold === fold && x.policy === policy && x.side === side);
  const quoteHours = selectedCandidates.reduce((sum, x) => sum + Number(x.activeDurationNs ?? 0n), 0) / 3.6e12;
  return {
    fold,
    policy,
    side,
    horizonMs,
    eligibleCandidateQuotes: selectedCandidates.length,
    candidateMarketClusters: candidatesByMarket,
    quoteHoursAtRisk: Number(quoteHours.toFixed(6)),
    strictThroughProxyFills: proxyFills.length,
    markedProxyFills: marked.length,
    unresolvedOrCensoredMarkouts: proxyFills.length - marked.length,
    proxyFillRatePerCandidate: selectedCandidates.length ? proxyFills.length / selectedCandidates.length : null,
    meanExecutableNetPerShareZeroFee: meanNet === null ? null : money(meanNet),
    meanOneTickEachWayStressPerShare: stress === null ? null : money(stress),
    positiveMarkoutFraction: netMicros.length ? netMicros.filter(x => x > 0n).length / netMicros.length : null,
    marketClusterUncertainty: clusterSummary(marked.map(x => ({ marketId: x.marketId, netMicros: x.marks[horizonMs].netMicros }))),
  };
}

export function createSpreadPolicyReplay({ policy, fold, decisionCutoffAt = null }) {
  if (!['atBest', 'insideOneTick'].includes(policy)) throw Error('INSIDE_SPREAD_POLICY');
  if (!['development', 'heldOut'].includes(fold)) throw Error('INSIDE_SPREAD_FOLD');
  const cutoffNs = decisionCutoffAt === null ? null : instantNs(decisionCutoffAt);
  const candidates = [];
  const fills = [];
  const pending = new Map();
  const lastQuoteSlot = new Map();
  let lastObservedNs = -1n;
  const counts = { candidateQuotes: 0, ineligibleSpreadPoints: 0, expiredUnfilledQuotes: 0, boundaryCancelledQuotes: 0, strictThroughProxies: 0, invalidTrades: 0 };

  function finishQuote(quote, endNs, status) {
    quote.status = status;
    const clippedEnd = endNs < quote.expiresAtNs ? endNs : quote.expiresAtNs;
    quote.activeDurationNs = clippedEnd > quote.activationAtNs ? clippedEnd - quote.activationAtNs : 0n;
  }

  function expire(exchangeId, atNs) {
    const queue = pending.get(exchangeId) ?? [];
    const retained = [];
    for (const quote of queue) {
      if (atNs >= quote.expiresAtNs) {
        finishQuote(quote, quote.expiresAtNs, 'expired_unfilled');
        counts.expiredUnfilledQuotes++;
      } else retained.push(quote);
    }
    pending.set(exchangeId, retained);
  }

  function boundary(exchangeId, at) {
    const atNs = at === null ? lastObservedNs : instantNs(at);
    for (const quote of pending.get(exchangeId) ?? []) {
      finishQuote(quote, atNs, atNs >= quote.activationAtNs ? 'boundary_cancelled_after_activation' : 'boundary_cancelled_before_activation');
      counts.boundaryCancelledQuotes++;
    }
    pending.delete(exchangeId);
    lastQuoteSlot.delete(exchangeId);
  }

  function observeTrade(trade, receivedAt) {
    const receivedNs = instantNs(receivedAt);
    const executedNs = instantNs(trade.executedAt);
    if (executedNs > receivedNs || receivedNs < lastObservedNs) { counts.invalidTrades++; return; }
    if (cutoffNs !== null && receivedNs >= cutoffNs) return;
    let tradeTick;
    try { tradeTick = normalizeOfficialFillTick(trade.price).tick; }
    catch { counts.invalidTrades++; return; }
    lastObservedNs = receivedNs;
    expire(trade.exchangeId, executedNs);
    const queue = pending.get(trade.exchangeId) ?? [];
    const index = queue.findIndex(quote => quote.marketId === trade.marketId
      && receivedNs > quote.decisionNs
      && executedNs >= quote.activationAtNs
      && executedNs < quote.expiresAtNs
      && strictThrough({ side: quote.side, tradeTick, limitTick: quote.limitTick }));
    if (index < 0) return;
    const [quote] = queue.splice(index, 1);
    pending.set(trade.exchangeId, queue);
    finishQuote(quote, executedNs, 'strict_through_proxy');
    const fill = { quote, fold: quote.fold, policy: quote.policy, side: quote.side, marketId: quote.marketId, exchangeId: quote.exchangeId, fillAtNs: executedNs, reportReceivedNs: receivedNs, marks: Object.fromEntries(HORIZONS_MS.map(h => [h, null])) };
    fills.push(fill);
    counts.strictThroughProxies++;
  }

  function observeBook(book, observedAt) {
    const observedNs = instantNs(observedAt);
    const sourceNs = instantNs(book.sourceAt), receivedNs = instantNs(book.receivedAt);
    if (observedNs < lastObservedNs || sourceNs > observedNs || receivedNs > observedNs) throw Error('INSIDE_SPREAD_BOOK_CLOCK');
    lastObservedNs = observedNs;
    const exchangeId = book.exchangeId;
    expire(exchangeId, observedNs);
    const bidTick = normalizeOfficialFillTick(book.bids[0]?.price).tick;
    const askTick = normalizeOfficialFillTick(book.asks[0]?.price).tick;
    if (askTick <= bidTick) throw Error('INSIDE_SPREAD_BOOK_CROSSED');
    const spreadTicks = askTick - bidTick;

    for (const fill of fills) {
      if (fill.exchangeId !== exchangeId || observedNs <= fill.reportReceivedNs) continue;
      // Development labels must be fully available before the declared cutoff.
      // Gate the source event, callback receipt, and capture processing time so
      // a buffered or late-arriving book cannot leak a post-cutoff outcome.
      if (cutoffNs !== null && (sourceNs >= cutoffNs || receivedNs >= cutoffNs || observedNs >= cutoffNs)) continue;
      for (const horizonMs of HORIZONS_MS) {
        if (fill.marks[horizonMs]) continue;
        const eventTargetNs = fill.fillAtNs + BigInt(horizonMs) * NS_PER_MS;
        const responseTargetNs = fill.reportReceivedNs + BigInt(EXIT_RESPONSE_MS) * NS_PER_MS;
        if (sourceNs < eventTargetNs || receivedNs < responseTargetNs) continue;
        const eventDelayNs = sourceNs - eventTargetNs;
        const responseDelayNs = receivedNs - responseTargetNs;
        const delayNs = eventDelayNs > responseDelayNs ? eventDelayNs : responseDelayNs;
        if (delayNs > BigInt(MAX_MARKOUT_DELAY_MS) * NS_PER_MS || observedNs - sourceNs > BigInt(MAX_BOOK_AGE_MS) * NS_PER_MS) continue;
        const exitTick = fill.side === 'buy' ? bidTick : askTick;
        const netTicks = fill.side === 'buy' ? exitTick - fill.quote.limitTick : fill.quote.limitTick - exitTick;
        fill.marks[horizonMs] = {
          netMicros: BigInt(netTicks) * TICK_MICROS,
          sourceAt: book.sourceAt,
          receivedAt: book.receivedAt,
          observedAt,
          eventDelayMs: Number(eventDelayNs / NS_PER_MS),
          responseDelayMs: Number(responseDelayNs / NS_PER_MS),
        };
      }
    }

    if (spreadTicks < MIN_SPREAD_TICKS) { counts.ineligibleSpreadPoints++; return; }
    if (cutoffNs !== null && observedNs >= cutoffNs) return;
    const slot = Math.floor(Number(observedNs / NS_PER_MS) / QUOTE_CADENCE_MS);
    if (lastQuoteSlot.get(exchangeId) === slot) return;
    lastQuoteSlot.set(exchangeId, slot);
    const prices = insideLimitTicks({ bidTick, askTick, policy });
    if (!prices) return;
    const activationAtNs = observedNs + BigInt(ACTIVATION_LATENCY_MS) * NS_PER_MS;
    const expiresAtNs = activationAtNs + BigInt(QUOTE_TTL_MS) * NS_PER_MS;
    const queue = pending.get(exchangeId) ?? [];
    for (const side of ['buy', 'sell']) {
      const limitTick = side === 'buy' ? prices.bidTick : prices.askTick;
      const quote = {
        id: `${policy}:${candidates.length + 1}`,
        fold,
        policy,
        side,
        marketId: book.marketId,
        exchangeId,
        decisionAt: observedAt,
        decisionNs: observedNs,
        activationAt: isoNs(activationAtNs),
        activationAtNs,
        expiresAt: isoNs(expiresAtNs),
        expiresAtNs,
        limitTick,
        limit: money(BigInt(limitTick) * TICK_MICROS),
        observedSpreadTicks: spreadTicks,
        status: 'resting',
        activeDurationNs: 0n,
      };
      candidates.push(quote);
      queue.push(quote);
      counts.candidateQuotes++;
    }
    pending.set(exchangeId, queue);
  }

  function report({ throughAt }) {
    const throughNs = instantNs(throughAt);
    for (const [exchangeId] of pending) expire(exchangeId, throughNs);
    for (const [exchangeId, queue] of pending) {
      for (const quote of queue) finishQuote(quote, throughNs, 'open_at_capture_end');
      pending.delete(exchangeId);
    }
    const publicFills = fills.map(fill => ({
      fold: fill.fold,
      policy: fill.policy,
      side: fill.side,
      marketId: fill.marketId,
      limit: fill.quote.limit,
      fillAt: isoNs(fill.fillAtNs),
      reportReceivedAt: isoNs(fill.reportReceivedNs),
      marks: Object.fromEntries(HORIZONS_MS.map(h => [h, fill.marks[h] ? { ...fill.marks[h], netMicros: undefined, executableNetZeroFee: money(fill.marks[h].netMicros), oneTickEachWayStress: money(fill.marks[h].netMicros - 2n * TICK_MICROS) } : null])),
    }));
    const cells = [];
    for (const side of ['buy', 'sell']) {
      for (const horizonMs of HORIZONS_MS) {
        cells.push(summarizeCell({ candidates, fills, horizonMs, fold, policy, side }));
      }
    }
    return {
      counts: { ...counts, candidates: candidates.length, filledProxyQuotes: fills.length },
      cells,
      fillProxies: publicFills,
    };
  }

  return { observeTrade, observeBook, boundary, report, counts };
}

export function combineStudyCells({ development, heldOut }) {
  const reports = [development?.atBest, development?.insideOneTick, heldOut?.atBest, heldOut?.insideOneTick];
  if (reports.some(report => !Array.isArray(report?.cells))) throw Error('INSIDE_SPREAD_CELL_COVERAGE');
  const cells = [
    ...development.atBest.cells,
    ...development.insideOneTick.cells,
    ...heldOut.atBest.cells,
    ...heldOut.insideOneTick.cells,
  ];
  const keys = new Set(cells.map(x => `${x.fold}|${x.policy}|${x.side}|${x.horizonMs}`));
  if (cells.length !== 24 || keys.size !== 24) throw Error('INSIDE_SPREAD_CELL_COVERAGE');
  return cells;
}

function sourceDescriptor(kind) { return SOURCES[kind]; }

function addConfig(record, context) {
  const configured = record.scope;
  if (!configured || typeof configured.tournamentId !== 'string' || typeof configured.marketId !== 'string' || !configured.marketId
    || !Array.isArray(configured.exchangeIds) || !configured.exchangeIds.length) throw Error('INSIDE_SPREAD_CONFIG_SCOPE');
  if (context.tournamentId === null) context.tournamentId = configured.tournamentId;
  if (configured.tournamentId !== context.tournamentId) throw Error('INSIDE_SPREAD_CONFIG_TOURNAMENT');
  const canonicalTopic = `tournament:${context.tournamentId}:market:${configured.marketId}`;
  if (record.topic !== canonicalTopic || configured.topic !== undefined && configured.topic !== canonicalTopic) throw Error('INSIDE_SPREAD_CONFIG_TOPIC');
  for (const exchangeId of configured.exchangeIds) {
    if (typeof exchangeId !== 'string' || !exchangeId) throw Error('INSIDE_SPREAD_CONFIG_EXCHANGE');
    const key = pairKey(configured.marketId, exchangeId);
    if (context.finalized && !context.pairs.has(key)) throw Error('INSIDE_SPREAD_LATE_CONFIG');
    if (context.pairs.has(key)) { context.duplicateConfigPairs++; continue; }
    context.pairs.add(key);
    context.exchanges.push({ marketId: configured.marketId, exchangeId, topic: canonicalTopic });
  }
  const markets = context.marketExchanges.get(configured.marketId) ?? new Set();
  for (const exchangeId of configured.exchangeIds) markets.add(exchangeId);
  context.marketExchanges.set(configured.marketId, markets);
  const oldMarkets = context.exchangeMarkets.get(configured.exchangeIds[0]) ?? new Set();
  for (const exchangeId of configured.exchangeIds) {
    const linked = context.exchangeMarkets.get(exchangeId) ?? new Set();
    linked.add(configured.marketId);
    context.exchangeMarkets.set(exchangeId, linked);
  }
}

function finalizeScope(context) {
  if (context.finalized) return;
  if (!context.tournamentId || !context.exchanges.length) throw Error('INSIDE_SPREAD_INITIAL_CONFIG_MISSING');
  if ([...context.exchangeMarkets.values()].some(markets => markets.size !== 1)) throw Error('INSIDE_SPREAD_AMBIGUOUS_EXCHANGE');
  context.finalized = true;
  for (const member of context.exchanges) {
    const members = context.membersByTopic.get(member.topic) ?? [];
    members.push(member);
    context.membersByTopic.set(member.topic, members);
  }
  const canonicalPairs = context.exchanges.map(({ marketId, exchangeId }) => [marketId, exchangeId]).sort((a, b) => a[0].localeCompare(b[0]) || a[1].localeCompare(b[1]));
  context.scopeHash = createHash('sha256').update(JSON.stringify({ tournamentId: context.tournamentId, pairs: canonicalPairs })).digest('hex');
  context.scope = { tournamentId: context.tournamentId, exchanges: context.exchanges };
}

function createSourceContext(kind) {
  const descriptor = sourceDescriptor(kind);
  const file = path.join(RUNTIME_DIR, descriptor.file);
  const before = fs.statSync(file);
  if (!before.isFile() || before.size < descriptor.bytes) throw Error('INSIDE_SPREAD_SOURCE_TOO_SHORT');
  const prefixHash = createHash('sha256');
  const context = {
    kind, descriptor, file, before, prefixHash,
    tournamentId: null, pairs: new Set(), exchanges: [], marketExchanges: new Map(), exchangeMarkets: new Map(),
    duplicateConfigPairs: 0, finalized: false, membersByTopic: new Map(), scope: null, scopeHash: null,
    sampledAt: new Map(), revisions: new Map(), epochs: new Map(), tradeSeen: new Map(),
    recordIndex: 0, bytesRead: 0, firstObservedAt: null, through: null, acceptedPoints: 0, rejectedPoints: 0,
    arms: {
      atBest: createSpreadPolicyReplay({ policy: 'atBest', fold: kind === 'development' ? 'development' : 'heldOut', decisionCutoffAt: kind === 'development' ? DEVELOPMENT_DECISION_CUTOFF : null }),
      insideOneTick: createSpreadPolicyReplay({ policy: 'insideOneTick', fold: kind === 'development' ? 'development' : 'heldOut', decisionCutoffAt: kind === 'development' ? DEVELOPMENT_DECISION_CUTOFF : null }),
    },
  };
  return context;
}

const HARD_INVALIDATIONS = new Set(['EXPIRY', 'STALE', 'BOOK_DIRTY', 'SCHEMA', 'MISSING_BOOK', 'SETTLEMENT', 'RESYNC_REQUIRED', 'SOCKET_ERROR', 'RECONNECT', 'DISCONNECTED', 'REFRESH', 'INITIAL', 'RECOVERY_FAILED']);

function scopedTrade(trade, context) {
  if (!trade || trade.tournamentId !== context.tournamentId || typeof trade.marketId !== 'string' || typeof trade.exchangeId !== 'string'
    || !context.pairs.has(pairKey(trade.marketId, trade.exchangeId))) return null;
  const id = typeof trade.id === 'string' ? trade.id : String(trade.sequence);
  const sequence = Number(trade.sequence), quantity = trade.quantity;
  if (!Number.isSafeInteger(sequence) || sequence < 0 || String(sequence) !== id || !Number.isSafeInteger(quantity) || quantity < 1) return null;
  let normalized;
  try { normalized = normalizeOfficialFillTick(trade.price); } catch { return null; }
  const identity = `${trade.marketId}|${trade.exchangeId}|${id}`;
  const signature = `${trade.executedAt}|${normalized.tick}|${quantity}`;
  const seen = context.tradeSeen.get(identity);
  if (seen !== undefined) {
    if (seen !== signature) throw Error('INSIDE_SPREAD_TRADE_IDENTITY_CONFLICT');
    return null;
  }
  context.tradeSeen.set(identity, signature);
  return { marketId: trade.marketId, exchangeId: trade.exchangeId, id, sequence, price: normalized.price, quantity, executedAt: trade.executedAt };
}

function processTrades(trades, receivedAt, context) {
  const orderedTrades = [...trades].sort((a, b) => {
    const left = instantNs(a.executedAt), right = instantNs(b.executedAt);
    return left < right ? -1 : left > right ? 1 : String(a.id).localeCompare(String(b.id));
  });
  for (const raw of orderedTrades) {
    const trade = scopedTrade(raw, context);
    if (!trade) continue;
    for (const arm of Object.values(context.arms)) arm.observeTrade(trade, receivedAt);
  }
}

function clearTopic(context, topic, at) {
  for (const member of context.membersByTopic.get(topic) ?? []) for (const arm of Object.values(context.arms)) arm.boundary(member.exchangeId, at);
}

function clearMarket(context, topic, exchangeIds, at) {
  const members = context.membersByTopic.get(topic) ?? [];
  const selected = exchangeIds === null ? members : members.filter(x => exchangeIds.has(x.exchangeId));
  for (const member of selected) for (const arm of Object.values(context.arms)) arm.boundary(member.exchangeId, at);
}

function resetTopicFeatures(context, topic) {
  // This study has no flow-state feature to reset. The receipt-causal module treats
  // sequence gaps as feature resets, not as order-cancel acknowledgements.
  void context;
  void topic;
}

function replaySource(kind) {
  const context = createSourceContext(kind);
  for (const line of journalLines(context.file, { endByte: context.descriptor.bytes })) {
    context.prefixHash.update(line);
    context.prefixHash.update('\n');
    context.bytesRead += Buffer.byteLength(line, 'utf8') + 1;
    const record = JSON.parse(line);
    if (record.format !== 'sig-feed-capture' || record.captureIndex !== context.recordIndex++) throw Error('INSIDE_SPREAD_CAPTURE_CHAIN');
    if (typeof record.observedAt !== 'string' || !Number.isFinite(Date.parse(record.observedAt))) throw Error('INSIDE_SPREAD_CAPTURE_CLOCK');
    context.firstObservedAt ??= record.observedAt;
    context.through = record.observedAt;

    if (record.kind === 'config') {
      addConfig(record, context);
      continue;
    }
    finalizeScope(context);

    if (record.kind === 'trade_observed') {
      processTrades([record.trade], record.observedAt, context);
      continue;
    }
    const topic = record.topic;
    if (typeof topic !== 'string' || !context.membersByTopic.has(topic)) continue;
    if (['disconnected', 'stopped', 'subscribed', 'rejected'].includes(record.kind)) {
      clearTopic(context, topic, record.observedAt);
      context.epochs.delete(topic);
      continue;
    }
    if (record.kind === 'invalidation') {
      const reason = String(record.reason ?? '');
      const only = record.exchangeId ? new Set([String(record.exchangeId)]) : null;
      if (HARD_INVALIDATIONS.has(reason)) clearMarket(context, topic, only, record.observedAt);
      else if (reason === 'GAP' || reason === 'UNSEQUENCED') resetTopicFeatures(context, topic);
      continue;
    }
    if (record.kind !== 'event' || record.event !== 'market_batch') {
      if (record.kind === 'event') clearTopic(context, topic, record.observedAt);
      continue;
    }
    const key = `${topic}:${record.epoch}`;
    const previousEpoch = context.epochs.get(topic);
    if (previousEpoch !== undefined && previousEpoch !== record.epoch) clearTopic(context, topic, record.observedAt);
    context.epochs.set(topic, record.epoch);
    const payload = record.payload;
    if (payload?.resyncRequired !== false || payload.marketSettled?.length) {
      clearTopic(context, topic, record.observedAt);
      continue;
    }
    const delivery = payload.delivery;
    if (!Number.isSafeInteger(delivery?.revision) || !Number.isSafeInteger(delivery?.previousRevision)) throw Error('INSIDE_SPREAD_REVISION');
    const priorRevision = context.revisions.get(key);
    if (priorRevision !== undefined && delivery.previousRevision !== priorRevision) resetTopicFeatures(context, topic);
    if (priorRevision !== undefined && delivery.revision <= priorRevision) continue;
    context.revisions.set(key, delivery.revision);

    const observedMs = Date.parse(record.observedAt), lastSample = context.sampledAt.get(key) ?? -Infinity;
    const shouldSample = observedMs - lastSample >= 1000;
    let books = [];
    if (shouldSample) {
      context.sampledAt.set(key, observedMs);
      const result = engineBookPointsV1(record, context.scope, record.observedAt);
      context.rejectedPoints += result.rejected.length;
      books = result.books;
      context.acceptedPoints += books.length;
    }
    processTrades(payload.trades ?? [], record.observedAt, context);
    for (const book of books) for (const arm of Object.values(context.arms)) arm.observeBook(book, record.observedAt);
  }
  finalizeScope(context);
  const after = fs.statSync(context.file);
  if (after.dev !== context.before.dev || after.ino !== context.before.ino || after.size < context.descriptor.bytes) throw Error('INSIDE_SPREAD_SOURCE_REPLACED');
  const sha256 = context.prefixHash.digest('hex');
  if (context.bytesRead !== context.descriptor.bytes || sha256 !== context.descriptor.sha256
    || context.kind === 'development' && context.recordIndex !== context.descriptor.captureRecords
    || context.kind === 'development' && context.firstObservedAt !== context.descriptor.from
    || context.kind === 'development' && context.through !== context.descriptor.through) throw Error('INSIDE_SPREAD_FROZEN_PREFIX_MISMATCH');
  const arms = Object.fromEntries(Object.entries(context.arms).map(([key, arm]) => [key, arm.report({ throughAt: context.through })]));
  return {
    kind,
    source: {
      ...context.descriptor,
      captureRecords: context.recordIndex,
      from: context.firstObservedAt,
      through: context.through,
      uniqueMarketExchangePairs: context.exchanges.length,
      uniqueMarkets: context.marketExchanges.size,
      uniqueExchanges: context.exchangeMarkets.size,
      duplicateConfigPairs: context.duplicateConfigPairs,
      normalizedMembershipSha256: context.scopeHash,
    },
    replay: {
      bytesRead: context.bytesRead,
      acceptedBookPoints: context.acceptedPoints,
      rejectedBookPoints: context.rejectedPoints,
      scopedUniqueTrades: context.tradeSeen.size,
      decisionCutoff: kind === 'development' ? DEVELOPMENT_DECISION_CUTOFF : null,
      arms,
    },
  };
}

function publicStudy() {
  const started = performance.now();
  if (!process.env.LOCALAPPDATA) throw Error('INSIDE_SPREAD_LOCALDATA_MISSING');
  if (fs.existsSync(RESULT_PATH)) throw Error('INSIDE_SPREAD_RESULT_EXISTS');
  const development = replaySource('development');
  const heldOut = replaySource('heldOut');
  const cells = combineStudyCells({ development: development.replay.arms, heldOut: heldOut.replay.arms });
  const report = {
    schemaVersion: 1,
    mode: 'fixed_one_tick_inside_spread_captured_markout_v1',
    at: new Date().toISOString(),
    runtimeMs: performance.now() - started,
    policy: {
      eligibleSnapshot: `source spread at least ${MIN_SPREAD_TICKS} official ticks`,
      baseline: 'matched eligible decision snapshots; buy at captured best bid and sell at captured best ask',
      insideOneTick: 'matched eligible decision snapshots; buy exactly one tick above captured best bid and sell exactly one tick below captured best ask',
      cadenceMs: QUOTE_CADENCE_MS,
      activationLatencyMs: ACTIVATION_LATENCY_MS,
      quoteTtlAfterActivationMs: QUOTE_TTL_MS,
      fillProxy: 'first scoped captured print at least one full tick through limit, executed after activation and before TTL, observed after the quote decision; one hypothetical share at the limit',
      exitMark: 'first fresh captured book with source time at or after fill plus horizon and local report receipt at or after trade-report receipt plus 3,000ms; both delays capped at 10,000ms; development marks also require source, callback receipt, and capture observation strictly before the development cutoff',
      horizonsMs: HORIZONS_MS,
      fees: '0.000000; no fee charged',
      stress: 'separate one-tick against entry and one-tick against exit per share (0.010000 total); sensitivity only',
      sourceBoundaries: 'each capture replay starts with empty quote/fill state; no carry across source files',
      spreadEligibilityMatched: true,
    },
    split: {
      developmentSource: development.source,
      developmentSplitAt: DEVELOPMENT_SPLIT_AT,
      developmentPurgeMs: DEVELOPMENT_PURGE_MS,
      developmentDecisionCutoff: DEVELOPMENT_DECISION_CUTOFF,
      developmentOutcomesAvailableBefore: DEVELOPMENT_DECISION_CUTOFF,
      heldOutSource: heldOut.source,
      heldOutCutoffRule: 'separate later Oct 13 capture prefix; all its candidate decisions are held out; not used to set policy parameters',
      holdoutSourceWasReferencedByPriorReports: false,
      overlapBetweenCaptureSourceFiles: false,
    },
    cells,
    limitations: [
      'Strict-through prints are counterfactual fill proxies, not acknowledged orders or observed fills; queue priority, trade aggressor, partial fills, capital, inventory, cancels, matching priority and self-trade interactions are not modeled.',
      'The baseline and inside arms use the same eligible source decision snapshots but are separate hypothetical one-share policies; they are not combined portfolio returns.',
      'At a two-tick spread the two inside-side limits meet at the same tick; results are separate side marks and do not simulate a simultaneous paired portfolio or self-match.',
      'Market-cluster intervals use equal-weight per-market mean outcomes and Student-t uncertainty across market clusters. Correlated event outcomes within markets are clustered; few clusters make intervals descriptive and unreliable, and different markets may still share common shocks.',
      'The one-tick stress is not a platform fee. No actual fills, realized account P&L, or profitability are established.',
      'Captured candidate observation cadence is capped at one engine book point per topic per second; quote opportunities are additionally sampled once per exchange per 30-second slot at the first eligible spread snapshot.',
    ],
    actualFillsProven: false,
    observedOrderFills: 0,
    profitabilityEstablished: false,
    contestOrders: 0,
    networkRequests: 0,
  };
  const serialized = JSON.stringify(report) + '\n';
  fs.writeFileSync(RESULT_PATH, serialized, { flag: 'wx' });
  return report;
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const result = publicStudy();
  console.log(JSON.stringify({
    output: path.relative(process.cwd(), RESULT_PATH),
    runtimeMs: Math.round(result.runtimeMs),
    sources: [result.split.developmentSource, result.split.heldOutSource].map(x => ({ file: x.file, bytes: x.bytes, sha256: x.sha256, captureRecords: x.captureRecords, from: x.from, through: x.through, uniqueMarkets: x.uniqueMarkets })),
    cells: result.cells.map(x => ({ fold: x.fold, policy: x.policy, side: x.side, horizonMs: x.horizonMs, candidates: x.eligibleCandidateQuotes, proxies: x.strictThroughProxyFills, marks: x.markedProxyFills, markets: x.marketClusterUncertainty.effectiveIndependentMarketCount, meanNet: x.meanExecutableNetPerShareZeroFee, stress: x.meanOneTickEachWayStressPerShare, clusteredCi95: x.marketClusterUncertainty.marketClusterCi95StudentT })),
  }));
}
