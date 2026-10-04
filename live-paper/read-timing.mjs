import fs from 'node:fs';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';

const stages = ['queueWaitMs', 'admissionWaitMs', 'headersMs', 'bodyMs', 'serviceMs', 'totalMs'];
const loopStages = ['eventLoopP99Ms', 'eventLoopMaxMs'];
const baseFields = ['schemaVersion', 'kind', 'marketId', 'queuedAt', 'admittedAt', 'finishedAt', ...stages, 'statusCode', 'outcome'];
const v2Fields = ['durationClock', 'eventLoopSamples', ...loopStages];

export function timingDistribution(values) {
  const a = values.filter(Number.isFinite).sort((x, y) => x - y);
  const percentile = q => a.length ? a[Math.max(0, Math.ceil(a.length * q) - 1)] : null;
  return {count: a.length, median: percentile(.5), p95: percentile(.95), p99: percentile(.99), max: a.at(-1) ?? null};
}

export function timingSummary(events) {
  const groups = {}, clocks = {};
  for (const e of events) {
    validateTiming(e);
    const clock = e.schemaVersion === 1 ? 'legacy-wall-clock' : e.durationClock;
    clocks[clock] = (clocks[clock] || 0) + 1;
    const g = groups[e.kind] ??= {requests: 0, outcomes: {}, ...Object.fromEntries([...stages, ...loopStages].map(k => [k, []]))};
    g.requests++;
    g.outcomes[e.outcome] = (g.outcomes[e.outcome] || 0) + 1;
    for (const k of [...stages, ...loopStages]) g[k].push(e[k]);
  }
  for (const g of Object.values(groups)) for (const k of [...stages, ...loopStages]) g[k] = timingDistribution(g[k]);
  return {records: events.length, clocks, percentileMethod: 'nearest-rank', groups};
}

export function validateTiming(e) {
  const fields = e?.schemaVersion === 2 ? [...baseFields, ...v2Fields] : baseFields;
  if (!e || typeof e !== 'object' || Array.isArray(e) || Object.keys(e).some(k => !fields.includes(k))
      || ![1, 2].includes(e.schemaVersion)
      || !['tournament', 'positions', 'orders', 'book', 'market', 'prices', 'history', 'fills', 'pnl'].includes(e.kind)
      || !['success', 'connection_error', 'http_error', 'body_error', 'aborted', 'blocked'].includes(e.outcome)) throw Error('Invalid timing schema');
  if (e.marketId !== null && (typeof e.marketId !== 'string' || !/^([1-9]\d*)$/.test(e.marketId))) throw Error('Invalid timing market');
  for (const k of ['queuedAt', 'finishedAt']) if (typeof e[k] !== 'string' || !Number.isFinite(Date.parse(e[k]))) throw Error('Invalid timing clock');
  if (e.admittedAt !== null && (typeof e.admittedAt !== 'string' || !Number.isFinite(Date.parse(e.admittedAt)))) throw Error('Invalid timing admission');
  for (const k of stages) if (e[k] !== null && (!Number.isFinite(e[k]) || e[k] < 0)) throw Error('Invalid timing duration');
  if (e.statusCode !== null && (!Number.isInteger(e.statusCode) || e.statusCode < 100 || e.statusCode > 599)) throw Error('Invalid timing status');
  if (e.schemaVersion === 2) {
    if (!['monotonic', 'injected'].includes(e.durationClock)
        || !Number.isSafeInteger(e.eventLoopSamples) || e.eventLoopSamples < 0) throw Error('Invalid timing instrumentation');
    for (const k of loopStages) {
      if (e.eventLoopSamples === 0 ? e[k] !== null : !Number.isFinite(e[k]) || e[k] < 0) throw Error('Invalid event-loop duration');
    }
    if (e.eventLoopSamples > 0 && e.eventLoopP99Ms > e.eventLoopMaxMs) throw Error('Invalid event-loop quantiles');
  }
  return e;
}

export function analyzeReadTimings(raw) {
  const lastNewline = raw.lastIndexOf('\n'), complete = raw.slice(0, lastNewline + 1);
  const events = complete ? complete.trimEnd().split('\n').map(line => validateTiming(JSON.parse(line))) : [];
  const outsideClientGaps = [];
  for (let i = 1; i < events.length; i++) outsideClientGaps.push(Math.max(0, Date.parse(events[i].queuedAt) - Date.parse(events[i - 1].finishedAt)));
  return {
    schemaVersion: 2,
    sourceHash: createHash('sha256').update(complete).digest('hex'),
    sourceBytes: Buffer.byteLength(complete), partialTailExcluded: complete.length !== raw.length,
    firstQueuedAt: events[0]?.queuedAt || null, lastFinishedAt: events.at(-1)?.finishedAt || null,
    ...timingSummary(events), outsideClientGapMs: timingDistribution(outsideClientGaps),
    interpretation: {
      networkRequests: 0,
      headersMs: 'Time to headers or failed/cancelled header wait',
      bodyMs: 'Body reading and JSON parsing, including failures; v2 transport also measures HTTP-error body reads',
      queueWaitMs: 'Waiting behind earlier calls on this same serialized client',
      admissionWaitMs: 'Pacing/cooldown wait plus admission publication',
      outsideClientGapMs: 'Wall-clock caller dependency/processing/explicit sleep or restart gap; not an isolated CPU measurement',
      eventLoopP99Ms: 'Node event-loop timer delay in the interval since the preceding timing record; 20ms sampling resolution, not per-request CPU time or pure blocking overhead',
      rateOrConcurrencyChanged: false,
      limitations: [
        'V1 durations use wall time. V2 production durations use a monotonic clock; persisted dates and outside-client gaps still use wall time.',
        'Injected clocks are marked separately. Mixed historical/production samples require care when comparing latencies.',
        'Empty event-loop samples are null, not proof of zero delay. Event-loop statistics are process-wide window measurements.',
        'Small correlated samples do not justify a higher request budget or reliable tail extrapolation.',
        'No credentials, URLs, cursors, headers, account bodies or error messages are recorded.',
        'These measurements are not exchange matching latency, complete signal-to-fill tracing, or evidence of profit.',
      ],
    },
  };
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const i = process.argv.indexOf('--file');
  if (i < 0 || !process.argv[i + 1]) throw Error('Required --file');
  console.log(JSON.stringify(analyzeReadTimings(fs.readFileSync(process.argv[i + 1], 'utf8')), null, 2));
}
