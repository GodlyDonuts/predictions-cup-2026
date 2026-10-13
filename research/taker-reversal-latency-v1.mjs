import { createHash } from 'node:crypto';
import { createReadStream, openSync, closeSync, fstatSync, readSync } from 'node:fs';

// Preregistered for task #45 before reading the held-out outcomes.
export const POLICY = Object.freeze({
  taskCutoffAt: '2026-10-13T14:14:39Z',
  discoveryFraction: 0.70,
  lookbackMs: 10_000,
  maxLookbackStalenessMs: 2_000,
  displacement: 0.02,
  sizeShares: 1,
  horizonsMs: Object.freeze([1_000, 3_000, 10_000]),
  latencyQuantiles: Object.freeze([0.50, 0.90, 0.99]),
  maxExecutionObservationLagMs: 2_000,
  maxQuoteAgeMs: 2_000,
  minimumInterpretationTrades: 100,
  minimumInterpretationMarkets: 20,
  bootstrapReplicates: 2_000,
  bootstrapSeed: 45_2026,
  feePerShare: 0,
});

const MARKET_TOPIC = /^tournament:([^:]+):market:([^:]+)$/;

function ms(value) {
  const n = Date.parse(value);
  return Number.isFinite(n) ? n : null;
}

function finitePositive(value) {
  return Number.isFinite(value) && value > 0;
}

export function percentile(values, p) {
  if (!values.length) return null;
  const xs = [...values].sort((a, b) => a - b);
  return xs[Math.min(xs.length - 1, Math.max(0, Math.ceil(p * xs.length) - 1))];
}

export function parseSnapshot(record) {
  if (!record) return [];
  let marketId, receivedAt, requestStartedAt, books;
  if (record.kind === 'book' && record.payload?.book) {
    const book = record.payload.book;
    marketId = String(book.marketId ?? '');
    receivedAt = ms(book.receivedAt ?? record.at);
    requestStartedAt = ms(book.requestStartedAt);
    books = [book];
  } else if (record.kind === 'snapshot' && Array.isArray(record.books)) {
    const topic = String(record.topic ?? '').match(MARKET_TOPIC);
    if (!topic) return [];
    marketId = topic[2];
    receivedAt = ms(record.receivedAt ?? record.observedAt);
    requestStartedAt = ms(record.requestStartedAt);
    books = record.books;
  } else return [];
  if (!marketId || receivedAt == null) return [];
  const requestLatencyMs = requestStartedAt != null && receivedAt >= requestStartedAt
    ? receivedAt - requestStartedAt
    : null;
  const out = [];
  for (const book of books) {
    const exchangeId = String(book?.exchangeId ?? '');
    const sequence = book?.sequence ?? book?.asOf?.sequence;
    const sourceAt = ms(book?.sourceAt ?? book?.asOf?.at ?? record.observedAt ?? record.at);
    const bid = book?.bids?.[0];
    const ask = book?.asks?.[0];
    if (!exchangeId || sequence == null || sourceAt == null || !bid || !ask) continue;
    const bp = Number(bid.price), bq = Number(bid.quantity);
    const ap = Number(ask.price), aq = Number(ask.quantity);
    if (!finitePositive(bp) || !finitePositive(ap) || bp > ap || ap > 1 || bp > 1) continue;
    if (!Number.isFinite(bq) || bq < 0 || !Number.isFinite(aq) || aq < 0) continue;
    out.push({
      marketId,
      exchangeId,
      sequence: String(sequence),
      at: Math.max(receivedAt, sourceAt),
      sourceAt,
      requestLatencyMs,
      bid: bp,
      bidQty: bq,
      ask: ap,
      askQty: aq,
      mid: (bp + ap) / 2,
    });
  }
  return out;
}

export function groupAndDeduplicate(records) {
  const groups = new Map();
  const seenSequences = new Map();
  const latencyByTime = [];
  let parsedSnapshots = 0;
  let duplicateSequences = 0;
  let invalidSnapshots = 0;
  for (const record of records) {
    if (record?.kind !== 'snapshot' && record?.kind !== 'book') continue;
    const rows = parseSnapshot(record);
    if (!rows.length) { invalidSnapshots++; continue; }
    parsedSnapshots++;
    for (const row of rows) {
      if (row.requestLatencyMs != null) latencyByTime.push({ at: row.at, latencyMs: row.requestLatencyMs });
      const key = `${row.marketId}:${row.exchangeId}`;
      let group = groups.get(key);
      if (!group) groups.set(key, group = []);
      let seen = seenSequences.get(key);
      if (!seen) seenSequences.set(key, seen = new Set());
      if (seen.has(row.sequence)) {
        duplicateSequences++;
        continue;
      }
      seen.add(row.sequence);
      group.push(row);
    }
  }
  for (const rows of groups.values()) rows.sort((a, b) => a.at - b.at || Number(a.sequence) - Number(b.sequence));
  return { groups, latencyByTime, parsedSnapshots, duplicateSequences, invalidSnapshots };
}

function lowerBound(rows, target) {
  let lo = 0, hi = rows.length;
  while (lo < hi) {
    const mid = (lo + hi) >>> 1;
    if (rows[mid].at < target) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

export function makeHoldoutSignals(groups, holdoutStartMs, policy = POLICY) {
  const signals = [];
  const firstEligible = holdoutStartMs + policy.lookbackMs;
  for (const rows of groups.values()) {
    let refIndex = -1;
    let armed = true;
    for (let i = 0; i < rows.length; i++) {
      const current = rows[i];
      const target = current.at - policy.lookbackMs;
      while (refIndex + 1 < i && rows[refIndex + 1].at <= target) refIndex++;
      if (refIndex < 0) continue;
      const ref = rows[refIndex];
      if (target - ref.at > policy.maxLookbackStalenessMs) continue;
      const move = current.mid - ref.mid;
      if (Math.abs(move) < policy.displacement - 1e-12) {
        armed = true;
        continue;
      }
      if (!armed) continue;
      // A held-out signal's entire lookback is also held out; no split-edge leakage.
      if (current.at < firstEligible) { armed = false; continue; }
      signals.push({ marketId: current.marketId, exchangeId: current.exchangeId, at: current.at, move });
      armed = false;
    }
  }
  return signals.sort((a, b) => a.at - b.at || a.marketId.localeCompare(b.marketId));
}

function nonOverlappingSignals(signals, horizonMs, latencyMs, policy) {
  const lastBusyUntil = new Map();
  const selected = [];
  let skipped = 0;
  const busyDuration = 2 * latencyMs + horizonMs + 2 * policy.maxExecutionObservationLagMs;
  for (const signal of signals) {
    const key = signal.marketId;
    if ((lastBusyUntil.get(key) ?? -Infinity) > signal.at) { skipped++; continue; }
    selected.push(signal);
    lastBusyUntil.set(key, signal.at + busyDuration);
  }
  return { selected, skipped };
}

function findExecutionRow(rows, targetAt, policy) {
  const i = lowerBound(rows, targetAt);
  if (i >= rows.length) return { reason: 'no_later_book' };
  const row = rows[i];
  if (row.at - targetAt > policy.maxExecutionObservationLagMs) return { reason: 'observation_gap' };
  if (row.at - row.sourceAt > policy.maxQuoteAgeMs) return { reason: 'stale_quote' };
  return { row };
}

export function simulateRoundTrip(rows, signal, direction, horizonMs, latencyMs, policy = POLICY) {
  const entry = findExecutionRow(rows, signal.at + latencyMs, policy);
  if (!entry.row) return { status: entry.reason };
  const e = entry.row;
  let entryPrice, entryDepth;
  if (direction === 'YES') {
    entryPrice = e.ask;
    entryDepth = e.askQty;
  } else {
    // In a binary contract, buying NO is the executable complement of selling YES.
    entryPrice = 1 - e.bid;
    entryDepth = e.bidQty;
  }
  if (entryDepth < policy.sizeShares) return { status: 'insufficient_entry_depth', entryAt: e.at };

  const exit = findExecutionRow(rows, e.at + horizonMs + latencyMs, policy);
  if (!exit.row) return { status: `exit_${exit.reason}`, entryAt: e.at };
  const x = exit.row;
  let exitPrice, exitDepth;
  if (direction === 'YES') {
    exitPrice = x.bid;
    exitDepth = x.bidQty;
  } else {
    exitPrice = 1 - x.ask;
    exitDepth = x.askQty;
  }
  if (exitDepth < policy.sizeShares) return { status: 'insufficient_exit_depth', entryAt: e.at, exitAt: x.at };
  const grossPnl = exitPrice - entryPrice;
  return {
    status: 'complete',
    marketId: signal.marketId,
    entryAt: e.at,
    exitAt: x.at,
    entryPrice,
    exitPrice,
    grossPnl,
    netPnl: grossPnl - 2 * policy.feePerShare,
    entrySpread: e.ask - e.bid,
    exitSpread: x.ask - x.bid,
  };
}

function random32(seed) {
  let x = seed >>> 0;
  return () => {
    x ^= x << 13; x ^= x >>> 17; x ^= x << 5;
    return (x >>> 0) / 0x1_0000_0000;
  };
}

function clusterBootstrap(trades, policy) {
  const byMarket = new Map();
  for (const t of trades) {
    let xs = byMarket.get(t.marketId);
    if (!xs) byMarket.set(t.marketId, xs = []);
    xs.push(t.netPnl);
  }
  const clusters = [...byMarket.values()];
  if (trades.length < 2 || clusters.length < 2) return { lower95: null, upper95: null, clusterCount: clusters.length };
  const rand = random32(policy.bootstrapSeed);
  const means = [];
  for (let b = 0; b < policy.bootstrapReplicates; b++) {
    let sum = 0, n = 0;
    for (let c = 0; c < clusters.length; c++) {
      const sample = clusters[Math.floor(rand() * clusters.length)];
      for (const pnl of sample) { sum += pnl; n++; }
    }
    if (n) means.push(sum / n);
  }
  return { lower95: percentile(means, .025), upper95: percentile(means, .975), clusterCount: clusters.length };
}

function summarizeOutcomes(outcomes, signals, skippedOverlap, policy) {
  const counts = Object.create(null);
  const trades = outcomes.filter(x => x.status === 'complete');
  for (const x of outcomes) counts[x.status] = (counts[x.status] ?? 0) + 1;
  const pnls = trades.map(x => x.netPnl);
  const sorted = [...pnls].sort((a, b) => a - b);
  const mean = pnls.length ? pnls.reduce((a, b) => a + b, 0) / pnls.length : null;
  const bootstrap = clusterBootstrap(trades, policy);
  const marketCount = new Set(trades.map(x => x.marketId)).size;
  return {
    eligibleSignalCount: signals.length,
    nonOverlapSkipped: skippedOverlap,
    attempted: outcomes.length,
    completeRoundTrips: trades.length,
    marketsWithTrades: marketCount,
    completionRate: outcomes.length ? trades.length / outcomes.length : null,
    statuses: counts,
    meanNetPnlPerShare: mean,
    medianNetPnlPerShare: percentile(sorted, .5),
    p10NetPnlPerShare: percentile(sorted, .1),
    p90NetPnlPerShare: percentile(sorted, .9),
    totalNetPnlAcrossIndependentCounterfactuals: pnls.length ? pnls.reduce((a, b) => a + b, 0) : 0,
    meanEntrySpread: trades.length ? trades.reduce((s, x) => s + x.entrySpread, 0) / trades.length : null,
    meanExitSpread: trades.length ? trades.reduce((s, x) => s + x.exitSpread, 0) / trades.length : null,
    marketClusterBootstrap95: bootstrap,
    economicallyInterpretableSample: trades.length >= policy.minimumInterpretationTrades && marketCount >= policy.minimumInterpretationMarkets,
  };
}

export function evaluateCapture({ groups, latencyByTime, captureStartMs, captureEndMs }, policy = POLICY) {
  if (!(captureEndMs > captureStartMs)) throw new Error('capture requires an increasing chronological time span');
  const span = captureEndMs - captureStartMs;
  const discoveryEndMs = captureStartMs + Math.floor(span * policy.discoveryFraction);
  const taskCutoffMs = Date.parse(policy.taskCutoffAt);
  const holdoutStartMs = Math.max(discoveryEndMs, taskCutoffMs);
  const discoveryLatencies = latencyByTime.filter(x => x.at < discoveryEndMs).map(x => x.latencyMs).filter(Number.isFinite);
  const latencies = Object.fromEntries(policy.latencyQuantiles.map(p => [String(p), percentile(discoveryLatencies, p)]));
  const signals = makeHoldoutSignals(groups, holdoutStartMs, policy);
  const results = [];
  for (const horizonMs of policy.horizonsMs) {
    for (const q of policy.latencyQuantiles) {
      const latencyMs = latencies[String(q)];
      if (latencyMs == null) {
        results.push({ horizonMs, latencyQuantile: q, latencyMs: null, reversal: null, naiveAlwaysTakeBaseline: null, status: 'no_discovery_latency_samples' });
        continue;
      }
      const { selected, skipped } = nonOverlappingSignals(signals, horizonMs, latencyMs, policy);
      const reversalOutcomes = [], momentumOutcomes = [];
      for (const signal of selected) {
        const rows = groups.get(`${signal.marketId}:${signal.exchangeId}`);
        const continuation = signal.move > 0 ? 'YES' : 'NO';
        const reversal = signal.move > 0 ? 'NO' : 'YES';
        reversalOutcomes.push(simulateRoundTrip(rows, signal, reversal, horizonMs, latencyMs, policy));
        momentumOutcomes.push(simulateRoundTrip(rows, signal, continuation, horizonMs, latencyMs, policy));
      }
      results.push({
        horizonMs,
        latencyQuantile: q,
        latencyMs,
        latencySource: 'discovery p-quantile of requestStartedAt-to-receivedAt; used as both entry and exit post-decision delay proxy, not observed order-write latency',
        reversal: summarizeOutcomes(reversalOutcomes, selected, skipped, policy),
        naiveAlwaysTakeBaseline: summarizeOutcomes(momentumOutcomes, selected, skipped, policy),
        noTradeBaseline: { attempted: selected.length, meanNetPnlPerShare: 0, totalNetPnlAcrossIndependentCounterfactuals: 0 },
        naiveBaselineDefinition: 'at each same trigger, always take the direction of the preceding 10s move; it is an execution benchmark, not a fitted strategy',
      });
    }
  }
  return {
    split: {
      discoveryEndAt: new Date(discoveryEndMs).toISOString(),
      discoveryFraction: policy.discoveryFraction,
      taskCutoffAt: policy.taskCutoffAt,
      holdoutStartAt: new Date(holdoutStartMs).toISOString(),
      firstSignalAtOrAfter: new Date(holdoutStartMs + policy.lookbackMs).toISOString(),
      holdoutEndAt: new Date(captureEndMs).toISOString(),
      discoveryLatencySamples: discoveryLatencies.length,
      discoveryLatencyMs: latencies,
      latencyMethod: 'requestStartedAt to receivedAt in discovery capture; used as a conservative measured-delay proxy for entry and exit arrival; write latency is unobserved',
    },
    signalPolicy: {
      definition: 'first snapshot in each market excursion where current YES midpoint differs from the last observed midpoint at/before 10s lookback by at least 0.02; a new signal requires a later return inside the same 0.02 band',
      lookbackMs: policy.lookbackMs,
      maxLookbackStalenessMs: policy.maxLookbackStalenessMs,
      absoluteMidpointMove: policy.displacement,
      reversalDirection: 'move up: buy NO at 1-YES bid; move down: buy YES at ask',
      momentumControl: 'same signal times, take the move direction instead of reversing',
      sizing: `${policy.sizeShares} share per trigger; one active counterfactual per market until horizon plus both latency proxies and the maximum entry/exit observation-lag allowance`,
      horizonsMs: policy.horizonsMs,
      feesPerShare: policy.feePerShare,
      depthRule: 'require displayed top-of-book quantity >= one share on both legs; first book no later than configured observation lag; stale/unknown depth is not filled',
      executableCost: 'entry and exit use the opposing displayed YES bid/ask; NO is the binary complement; no fees; price impact beyond one share at top level is not modeled',
      noTradeBaseline: 'zero P&L; does not incur a position',
      purge: 'only post-cutoff holdout signals with a full 10s held-out lookback; missing later exit observations are censored, not assigned a mark or fill',
    },
    results,
    limitations: [
      'This is a one-share counterfactual markout, not a shared-capital portfolio return; simultaneous markets are not capital-constrained.',
      'The measured read request round trip is only a proxy for order-write arrival latency; actual write latency and fill acknowledgment are not observed.',
      'Market orders are modeled at the first later captured top-of-book within a fixed observation window; uncaptured intrainterval changes and market impact are unknown.',
      'YES/NO complement execution assumes the platform binary book mapping; settlement exceptions and market-level account limits are outside this short-horizon replay.',
      'A positive tiny-sample mean is not a profitability claim; interpretation requires the preregistered completed-trade and market-cluster minimums and the bootstrap interval.',
      'No parameter was chosen from holdout outcomes; the 0.02 move threshold is fixed in POLICY before scoring.',
    ],
  };
}

export async function readCapturePrefix(filePath, byteBound) {
  const fd = openSync(filePath, 'r');
  let before;
  try {
    before = fstatSync(fd);
    if (!Number.isSafeInteger(byteBound) || byteBound <= 0 || byteBound > before.size) throw new Error('invalid byte prefix bound');
  } finally { closeSync(fd); }
  const hash = createHash('sha256');
  const groups = new Map();
  const seenSequences = new Map();
  const latencyByTime = [];
  let pending = Buffer.alloc(0), parsedLines = 0, invalidLines = 0, bytesRead = 0;
  let parsedSnapshots = 0, invalidSnapshots = 0, duplicateSequences = 0;
  const stream = createReadStream(filePath, { start: 0, end: byteBound - 1, highWaterMark: 1 << 20 });
  for await (const chunk of stream) {
    bytesRead += chunk.length;
    hash.update(chunk);
    const data = pending.length ? Buffer.concat([pending, chunk]) : chunk;
    let start = 0;
    for (let i = 0; i < data.length; i++) {
      if (data[i] !== 0x0a) continue;
      const line = data.subarray(start, i).toString('utf8').trimEnd();
      start = i + 1;
      if (!line) continue;
      try {
        const record = JSON.parse(line);
        parsedLines++;
        if (record?.kind !== 'snapshot' && record?.kind !== 'book') continue;
        const rows = parseSnapshot(record);
        if (!rows.length) { invalidSnapshots++; continue; }
        parsedSnapshots++;
        if (rows[0].requestLatencyMs != null) latencyByTime.push({ at: rows[0].at, latencyMs: rows[0].requestLatencyMs });
        for (const row of rows) {
          const key = `${row.marketId}:${row.exchangeId}`;
          let group = groups.get(key);
          if (!group) groups.set(key, group = []);
          let seen = seenSequences.get(key);
          if (!seen) seenSequences.set(key, seen = new Set());
          if (seen.has(row.sequence)) { duplicateSequences++; continue; }
          seen.add(row.sequence);
          group.push(row);
        }
      }
      catch { invalidLines++; }
    }
    pending = Buffer.from(data.subarray(start));
  }
  if (bytesRead !== byteBound || pending.length) throw new Error('prefix did not end at a complete newline-delimited record');
  for (const rows of groups.values()) rows.sort((a, b) => a.at - b.at || Number(a.sequence) - Number(b.sequence));
  return { groups, latencyByTime, parsedSnapshots, invalidSnapshots, duplicateSequences, bytesRead, sha256: hash.digest('hex'), parsedLines, invalidLines };
}

export function completeLineBound(filePath) {
  const fd = openSync(filePath, 'r');
  try {
    const { size } = fstatSync(fd);
    if (!size) throw new Error('empty capture');
    const last = Buffer.alloc(1);
    readSync(fd, last, 0, 1, size - 1);
    if (last[0] === 0x0a) return size;
    const blockSize = 64 * 1024;
    for (let end = size; end > 0;) {
      const start = Math.max(0, end - blockSize);
      const buf = Buffer.alloc(end - start);
      readSync(fd, buf, 0, buf.length, start);
      const idx = buf.lastIndexOf(0x0a);
      if (idx >= 0) return start + idx + 1;
      end = start;
    }
    throw new Error('capture has no complete newline-delimited record');
  } finally { closeSync(fd); }
}

export function buildReplayData(records) {
  const parsed = Array.isArray(records) ? groupAndDeduplicate(records) : records;
  let captureStartMs = Infinity, captureEndMs = -Infinity, rowCount = 0;
  for (const rows of parsed.groups.values()) {
    for (const row of rows) {
      captureStartMs = Math.min(captureStartMs, row.at);
      captureEndMs = Math.max(captureEndMs, row.at);
      rowCount++;
    }
  }
  if (!rowCount) throw new Error('capture contains no valid public-book snapshots');
  return { ...parsed, captureStartMs, captureEndMs };
}
