import { basename, resolve, join } from 'node:path';
import { statSync, writeFileSync } from 'node:fs';
import {
  POLICY,
  buildReplayData,
  completeLineBound,
  evaluateCapture,
  percentile,
  readCapturePrefix,
} from '../research/taker-reversal-latency-v1.mjs';

const freezeOnly = process.argv[2] === '--freeze-only';
const defaultCapture = process.env.LOCALAPPDATA
  ? join(process.env.LOCALAPPDATA, 'SigTradingCup', 'paper-runtime', 'public-books.jsonl')
  : null;
const capturePath = resolve((freezeOnly ? process.argv[3] : process.argv[2]) ?? defaultCapture ?? '');
const outPath = freezeOnly ? null : resolve(process.argv[3] ?? 'data/taker-reversal-latency-20261013.json');
if (!capturePath || capturePath === resolve('.')) throw new Error('provide a local captured JSONL path or set LOCALAPPDATA');

const started = performance.now();
const requestedByteBound = freezeOnly ? null : Number(process.argv[4]);
if (!freezeOnly && (!Number.isSafeInteger(requestedByteBound) || requestedByteBound <= 0)) {
  throw new Error('freeze the source first, then pass its exact byte bound and SHA-256 as arguments 3 and 4');
}
const byteBound = requestedByteBound ?? completeLineBound(capturePath);
const sourceStat = statSync(capturePath);
const captured = await readCapturePrefix(capturePath, byteBound);
const expectedHash = freezeOnly ? null : process.argv[5];
if (!freezeOnly && (!/^[a-f0-9]{64}$/i.test(expectedHash ?? '') || captured.sha256 !== expectedHash.toLowerCase())) {
  throw new Error('frozen source-prefix SHA-256 mismatch; refusing to score');
}
const replayData = buildReplayData(captured);
if (freezeOnly) {
  const discoveryEndMs = replayData.captureStartMs + Math.floor(
    (replayData.captureEndMs - replayData.captureStartMs) * POLICY.discoveryFraction,
  );
  const discoveryLatency = captured.latencyByTime
    .filter(x => x.at < discoveryEndMs)
    .map(x => x.latencyMs)
    .filter(Number.isFinite);
  console.log(JSON.stringify({
    sourcePath: capturePath,
    observedFileSizeBytes: sourceStat.size,
    immutableCompleteLinePrefixBytes: captured.bytesRead,
    sha256: captured.sha256,
    prefixFirstBookAt: new Date(replayData.captureStartMs).toISOString(),
    prefixLastBookAt: new Date(replayData.captureEndMs).toISOString(),
    captureDurationMs: replayData.captureEndMs - replayData.captureStartMs,
    completeJsonlLines: captured.parsedLines,
    validBookRecords: captured.parsedSnapshots,
    uniqueMarketExchangeStreams: replayData.groups.size,
    duplicateBookSequencesIgnored: captured.duplicateSequences,
    invalidBookRecords: captured.invalidSnapshots,
    discoveryEndAt: new Date(discoveryEndMs).toISOString(),
    holdoutStartAt: new Date(Math.max(discoveryEndMs, Date.parse(POLICY.taskCutoffAt))).toISOString(),
    discoveryLatencySampleCount: discoveryLatency.length,
    discoveryLatencyMs: Object.fromEntries(POLICY.latencyQuantiles.map(q => [String(q), percentile(discoveryLatency, q)])),
    fixedPolicy: POLICY,
  }, null, 2));
  process.exit(0);
}
const evaluation = evaluateCapture(replayData);
let postCutoff = 0;
const holdoutStreamCounts = [];
let firstHoldoutBookMs = Infinity, lastHoldoutBookMs = -Infinity;
for (const rows of replayData.groups.values()) {
  let count = 0;
  for (const row of rows) {
    if (row.at < Date.parse(POLICY.taskCutoffAt)) continue;
    postCutoff++;
    count++;
    firstHoldoutBookMs = Math.min(firstHoldoutBookMs, row.at);
    lastHoldoutBookMs = Math.max(lastHoldoutBookMs, row.at);
  }
  if (count) holdoutStreamCounts.push(count);
}
holdoutStreamCounts.sort((a, b) => a - b);
const output = {
  schemaVersion: 1,
  task: 45,
  title: 'Latency-aware taker reversal on fresh captured books',
  paperCounterfactualOnly: true,
  actualOrders: 0,
  executionAuthority: false,
  profitabilityEstablished: false,
  generatedAt: new Date().toISOString(),
  source: {
    basename: basename(capturePath),
    runtimeRelativePath: 'SigTradingCup/paper-runtime/public-books.jsonl',
    choiceReason: 'Selected before inspecting outcomes because the existing sanitized public-book journal had broader market/time coverage than the narrow realtime topic capture.',
    sourceFileSizeObservedBytes: sourceStat.size,
    immutableCompleteLinePrefixBytes: captured.bytesRead,
    sha256: captured.sha256,
    prefixFirstBookAt: new Date(replayData.captureStartMs).toISOString(),
    prefixLastBookAt: new Date(replayData.captureEndMs).toISOString(),
    prefixDurationMs: replayData.captureEndMs - replayData.captureStartMs,
    completeJsonlLines: captured.parsedLines,
    invalidJsonlLines: captured.invalidLines,
    validBookRecords: captured.parsedSnapshots,
    uniqueMarketExchangeStreams: replayData.groups.size,
    duplicateBookSequencesIgnored: replayData.duplicateSequences,
    invalidBookSnapshots: replayData.invalidSnapshots,
    postTaskCutoffBookPoints: postCutoff,
  },
  holdoutCoverage: {
    bookPoints: postCutoff,
    streamsWithAtLeastOnePoint: holdoutStreamCounts.length,
    streamsWithAtLeastTwoPoints: holdoutStreamCounts.filter(x => x >= 2).length,
    streamsWithAtLeastThreePoints: holdoutStreamCounts.filter(x => x >= 3).length,
    maxPointsPerStream: holdoutStreamCounts.at(-1) ?? 0,
    medianPointsPerActiveStream: holdoutStreamCounts.length
      ? holdoutStreamCounts[Math.floor(holdoutStreamCounts.length / 2)]
      : null,
    firstBookAt: Number.isFinite(firstHoldoutBookMs) ? new Date(firstHoldoutBookMs).toISOString() : null,
    lastBookAt: Number.isFinite(lastHoldoutBookMs) ? new Date(lastHoldoutBookMs).toISOString() : null,
  },
  runtime: { elapsedMs: Math.round(performance.now() - started) },
  evaluation,
};
writeFileSync(outPath, `${JSON.stringify(output, null, 2)}\n`, { flag: 'wx' });
console.log(JSON.stringify({
  resultPath: outPath,
  sourceSha256: captured.sha256,
  prefixBytes: captured.bytesRead,
  prefixLastBookAt: output.source.prefixLastBookAt,
  postTaskCutoffBookPoints: postCutoff,
  signalCount: evaluation.results[0]?.reversal?.eligibleSignalCount ?? 0,
  elapsedMs: output.runtime.elapsedMs,
}, null, 2));
