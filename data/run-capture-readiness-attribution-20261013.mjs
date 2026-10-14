import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {createReadStream} from 'node:fs';
import readline from 'node:readline';
import {fileURLToPath} from 'node:url';
import {
  assertCaptureReadinessReconciliationV1,
  createCaptureReadinessAttributionV1,
} from '../research/capture-readiness-attribution-v1.mjs';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const RUNTIME = path.join(process.env.LOCALAPPDATA ?? '', 'SigTradingCup', 'paper-runtime');
const REPORT_PATH = path.join(ROOT, 'data', 'capture-readiness-attribution-20261013.json');
const SOURCE = Object.freeze({
  id: 'train-d53812dd',
  file: 'realtime-2026-10-13T02-55-28-953Z-d53812dd-ec75-4852-9267-59c7d5bb7898.jsonl',
  frozenPrefixBytes: 1_747_459_094,
  frozenPrefixSha256: 'f9a20ef98e8e9c96d1229dbbc6a8eb17dd669e7c6f3f8d6e49337439945f4418',
  pinnedBackingBytes: 1_747_507_916,
  expectedCaptureRows: 467_044,
  expectedTournamentId: 'bda92870-621e-47b0-bc3c-3602c5c26f55',
  expectedMarkets: 237,
});
const EXPECTED = Object.freeze({
  notReadyBatches: 441_465,
  validRecoveryCompletions: 1_276,
  acceptedBookPoints: 3_994,
  lifecycleInvalidations: 10_778,
  staleRecoveryCompletions: 0,
  lifecycleInvalidationsByKind: Object.freeze({subscribed: 1_185, invalidation: 7_221, recovery_started: 1_280, disconnected: 1_092}),
});

function invariant(ok, code) { if (!ok) throw Error(`CAPTURE_READINESS_RUN_${code}`); }

function identity(stat) {
  return {dev: String(stat.dev), ino: String(stat.ino), size: stat.size, mtimeMs: stat.mtimeMs};
}

function sameIdentity(a, b) {
  return a.dev === b.dev && a.ino === b.ino && a.size === b.size && a.mtimeMs === b.mtimeMs;
}

function finalByteIsNewline(file, offset) {
  const fd = fs.openSync(file, 'r'), oneByte = Buffer.alloc(1);
  try { return fs.readSync(fd, oneByte, 0, 1, offset) === 1 && oneByte[0] === 0x0a; }
  finally { fs.closeSync(fd); }
}

async function hashExactPrefix(file, bytes) {
  const hash = createHash('sha256');
  let count = 0;
  for await (const chunk of createReadStream(file, {start: 0, end: bytes - 1, highWaterMark: 4 * 1024 * 1024})) {
    count += chunk.length;
    hash.update(chunk);
  }
  invariant(count === bytes, 'PREFIX_TRUNCATED');
  return hash.digest('hex');
}

async function readPrefix(file, tracker) {
  const input = createReadStream(file, {encoding: 'utf8', start: 0, end: SOURCE.frozenPrefixBytes - 1, highWaterMark: 4 * 1024 * 1024});
  const lines = readline.createInterface({input, crlfDelay: Infinity});
  let lineCount = 0;
  for await (const line of lines) {
    invariant(line.length > 0, 'BLANK_LINE');
    let row;
    try { row = JSON.parse(line); } catch { invariant(false, 'INVALID_JSON'); }
    tracker.push(row);
    lineCount++;
  }
  invariant(lineCount === SOURCE.expectedCaptureRows, 'ROW_COUNT');
}

async function main() {
  invariant(Boolean(process.env.LOCALAPPDATA), 'LOCALAPPDATA_REQUIRED');
  const file = path.join(RUNTIME, SOURCE.file);
  const before = identity(fs.statSync(file));
  invariant(fs.statSync(file).isFile(), 'SOURCE_NOT_FILE');
  invariant(before.size === SOURCE.pinnedBackingBytes && before.size >= SOURCE.frozenPrefixBytes, 'BACKING_SIZE_CHANGED');
  invariant(finalByteIsNewline(file, SOURCE.frozenPrefixBytes - 1), 'PREFIX_NOT_LINE_ALIGNED');
  const prefixHashBefore = await hashExactPrefix(file, SOURCE.frozenPrefixBytes);
  invariant(prefixHashBefore === SOURCE.frozenPrefixSha256, 'PREFIX_HASH_BEFORE');
  invariant(sameIdentity(before, identity(fs.statSync(file))), 'SOURCE_CHANGED_DURING_PREHASH');

  const tracker = createCaptureReadinessAttributionV1({
    tournamentId: SOURCE.expectedTournamentId,
    expectedMarkets: SOURCE.expectedMarkets,
  });
  await readPrefix(file, tracker);
  const analysis = tracker.finish();

  invariant(finalByteIsNewline(file, SOURCE.frozenPrefixBytes - 1), 'PREFIX_NOT_LINE_ALIGNED_AFTER');
  const prefixHashAfter = await hashExactPrefix(file, SOURCE.frozenPrefixBytes);
  const after = identity(fs.statSync(file));
  invariant(sameIdentity(before, after), 'SOURCE_CHANGED_DURING_REPLAY');
  invariant(prefixHashAfter === SOURCE.frozenPrefixSha256 && prefixHashAfter === prefixHashBefore, 'PREFIX_HASH_AFTER');
  invariant(analysis.capture.captureRows === SOURCE.expectedCaptureRows, 'CAPTURE_ROW_RECONCILIATION');
  invariant(analysis.capture.configRows === SOURCE.expectedMarkets, 'MARKET_CONFIG_RECONCILIATION');
  assertCaptureReadinessReconciliationV1(analysis, EXPECTED);

  const report = {
    schemaVersion: 1,
    generatedAt: new Date().toISOString(),
    status: 'bounded_training_prefix_source_attribution_complete',
    scope: {
      onlyFrozenTrainingPrefix: true,
      laterCaptureRead: false,
      rawRowsRetainedOrOutput: false,
      holdoutReopened: false,
      modelFitOrScoring: false,
      profitabilityEstablished: false,
    },
    source: {
      captureId: SOURCE.id,
      runtimeRelativePath: `LOCALAPPDATA/SigTradingCup/paper-runtime/${SOURCE.file}`,
      frozenPrefixBytes: SOURCE.frozenPrefixBytes,
      frozenPrefixSha256: SOURCE.frozenPrefixSha256,
      prefixHashVerifiedBeforeAndAfter: true,
      completePrefixEndsAtNewline: true,
      backingFileBytes: before.size,
      backingFileIdentityStable: true,
      bytesBeyondFrozenPrefixReadOrParsed: false,
      excludedTailBytes: before.size - SOURCE.frozenPrefixBytes,
    },
    sourceSemantics: {
      documentedDeliveryModel: 'best-effort-authoritative-resync',
      documentedReadiness: 'A market is ready only after a successful authoritative recovery for the current subscription epoch; local invalidations block use until a later successful recovery.',
      documentedInvalidationTriggers: ['initial subscription', 'reconnect or refresh', 'socket error/disconnect', 'revision gap', 'resync-required batch', 'unknown trade sequence', 'expiry or stale state', 'dirty book or settlement state'],
      documentedCompletenessLimit: 'booksReady describes recovered configured books, not a complete trade tape or complete market universe; the capture does not promise continuous historical observations.',
      lifecycleRowsRepresentLocalCollectorState: true,
      lifecycleRowsDoNotIdentifyUnderlyingNetworkOrProviderFaultByThemselves: true,
      lifecycleInvalidationCountRule: 'Count subscribed, invalidation, disconnected, and recovery_started rows; recovery_finished rows are tracked separately.',
      evidenceFiles: ['feed/CONTRACT.md', 'feed/SCHEMA.md', 'feed/README.md', 'feed/collector.mjs'],
    },
    attribution: {
      parserStateMismatchRows: analysis.totals.missingStateRows + analysis.totals.epochMismatchRows,
      excludedWhileRecoveryNotReady: analysis.totals.notReadyBatches,
      readyRecoveryCompletions: analysis.totals.validRecoveryCompletions,
      acceptedBookPoints: analysis.totals.acceptedBookPoints,
      lifecycleInvalidations: analysis.totals.lifecycleInvalidations,
      separateSameMillisecondInvalidatedBatches: analysis.totals.sameMillisecondInvalidationBatches,
      batchExclusionsByActiveBlocker: analysis.totals.notReadyBatchesByCause,
      readinessBlockerApplicationsByCause: analysis.totals.readinessBlockerApplicationsByCause,
      lifecycleInvalidationsByKind: analysis.totals.lifecycleInvalidationsByKind,
      lifecycleInvalidationKindSum: Object.values(analysis.totals.lifecycleInvalidationsByKind).reduce((sum, count) => sum + count, 0),
      parserCountReconciliation: {
        expectedFromCorrectedIssue80Run: EXPECTED,
        observed: {
          notReadyBatches: analysis.totals.notReadyBatches,
          validRecoveryCompletions: analysis.totals.validRecoveryCompletions,
          acceptedBookPoints: analysis.totals.acceptedBookPoints,
          lifecycleInvalidations: analysis.totals.lifecycleInvalidations,
          staleRecoveryCompletions: analysis.totals.staleRecoveryCompletions,
          lifecycleInvalidationsByKind: analysis.totals.lifecycleInvalidationsByKind,
        },
        exactMatch: true,
      },
    },
    findings: {
      rootCauseClassification: 'The corrected parser accounts for the excluded batches as capture-logged local not-ready intervals after subscription/invalidation/recovery events; the bounded replay reproduces all four #80 counts and has zero missing-state or epoch-mismatch rows. The records exist in the capture, but the local feed contract does not treat their books as continuity-valid before authoritative recovery. These lifecycle rows do not prove whether the trigger originated in provider delivery, transport, recovery service, or conservative collector policy.',
      isTheCaptureBroadlyUsableForModelTraining: false,
      reason: 'Only 3,994 accepted book points remain across 237 markets, with fragmented readiness and sparse labeled 3s/5s candidates in the prior training-only report. The stream is usable for narrow diagnostics on accepted segments, but not as a continuous broad-market history or a reliable, representative 237-market model-training window.',
      notAProfitabilityResult: true,
    },
    perMarketReadiness: analysis.perMarketSummary,
    markets: analysis.markets,
    aggregate: {
      ...analysis.capture,
      totals: analysis.totals,
    },
  };
  const writeFlag = process.argv.includes('--replace-report') ? 'w' : 'wx';
  fs.writeFileSync(REPORT_PATH, `${JSON.stringify(report, null, 2)}\n`, {flag: writeFlag});
  process.stdout.write(`${JSON.stringify({status: report.status, report: 'data/capture-readiness-attribution-20261013.json', prefixHashVerifiedBeforeAndAfter: true, totals: report.attribution.parserCountReconciliation.observed, markets: report.perMarketReadiness.markets})}\n`);
}

main().catch(error => {
  process.stderr.write(`${error.message}\n`);
  process.exitCode = 1;
});
