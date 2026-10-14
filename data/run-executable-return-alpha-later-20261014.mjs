import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {performance} from 'node:perf_hooks';
import {fileURLToPath} from 'node:url';
import {journalLines} from '../live-paper/journal-lines.mjs';
import {applyBookPublication, createBookPublicationState} from '../live-paper/book-publication-v2.mjs';
import {bookFeatures, resetBookEpochs} from '../research/book-alpha-v1.mjs';
import {
  EXECUTABLE_RETURN_POLICY_V1,
  executableReturnLabelsV1,
  scoreExecutableReturnModelV1,
} from '../research/executable-return-alpha-v1.mjs';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const PRIOR_REPORT_PATH = path.join(REPO_ROOT, 'data', 'executable-return-alpha-20261014.json');
export const FORWARD_OUTPUT_RELATIVE_PATH_V1 = 'data/executable-return-alpha-later-20261014-v2.json';
const OUTPUT_PATH = path.join(REPO_ROOT, FORWARD_OUTPUT_RELATIVE_PATH_V1);
const SOURCE_PATH = process.env.BAAL_EXECUTABLE_RETURN_SOURCE
  ?? path.join(process.env.LOCALAPPDATA, 'SigTradingCup', 'paper-runtime', 'public-books.jsonl');
const PRIOR_REPORT_GIT_BLOB = 'dbccbbe22681752ba0c59d6894651977e31a5f8b';
const PRIOR_REPORT_SHA256 = '8e5b605f5be384eb9eec0d405e164805a5afa91858ee099550e359726708d37d';
const PRICE_UNIVERSE_GIT_BLOB = 'c621ca2fb7a77646ab91d0de0cdfd66248461411';
const PRIOR_SOURCE_BYTES = 328_568_080;
const PRIOR_SOURCE_SHA256 = 'a6628a435a71f99fc702d887614059ea5da48be279b940b20450407108ea5b70';
const PRIOR_CUTOFF = '2026-10-14T14:31:30.476Z';
const NEED = (value, code) => { if (!value) throw Error(code); };

function sha256(bytes) { return createHash('sha256').update(bytes).digest('hex'); }
function gitBlobSha(bytes) {
  return createHash('sha1').update(`blob ${bytes.length}\0`).update(bytes).digest('hex');
}
function fileSha256(file) { return sha256(fs.readFileSync(file)); }
function sameIdentity(a, b) {
  return a.dev === b.dev && a.ino === b.ino && a.size === b.size
    && a.mtimeMs === b.mtimeMs && a.ctimeMs === b.ctimeMs;
}

export function assertStableSourceIdentityV1(before, after, errorCode = 'SOURCE_CHANGED_DURING_READ') {
  NEED(sameIdentity(before, after), errorCode);
  return true;
}

export function assertStableSourceHashesV1(before, after) {
  NEED(before?.sha256 === after?.sha256 && before?.prefixSha256 === after?.prefixSha256,
    'SOURCE_CHANGED_DURING_REPLAY');
  return true;
}

export function verifyPinnedPriorReportBytesV1(bytes) {
  NEED(Buffer.isBuffer(bytes), 'PRIOR_REPORT_BYTES');
  NEED(sha256(bytes) === PRIOR_REPORT_SHA256, 'PRIOR_REPORT_HASH');
  NEED(gitBlobSha(bytes) === PRIOR_REPORT_GIT_BLOB, 'PRIOR_REPORT_MAIN_BLOB');
  return JSON.parse(bytes.toString('utf8'));
}

export function frozenExecutableReturnModelV1(report) {
  NEED(report?.source?.bytes === PRIOR_SOURCE_BYTES && report.source.sha256 === PRIOR_SOURCE_SHA256,
    'PRIOR_SOURCE_BINDING');
  NEED(report.cutoff === PRIOR_CUTOFF, 'PRIOR_MODEL_CUTOFF');
  NEED(JSON.stringify(report.policy) === JSON.stringify(EXECUTABLE_RETURN_POLICY_V1), 'POLICY_CHANGED');
  const model = report.model;
  NEED(model?.trainingRows === 55_609 && Array.isArray(model.coefficients)
    && model.coefficients.length === 7 && Array.isArray(model.featureMean)
    && model.featureMean.length === 6 && Array.isArray(model.featureScale)
    && model.featureScale.length === 6, 'MODEL_SHAPE');
  NEED([...model.coefficients, ...model.featureMean, ...model.featureScale,
    model.trainingMean, model.trainingRmse].every(Number.isFinite), 'MODEL_NONFINITE');
  NEED(model.featureScale.every(value => value > 0), 'MODEL_SCALE');
  return Object.freeze({
    mean: Object.freeze([...model.featureMean]),
    scale: Object.freeze([...model.featureScale]),
    coefficients: Object.freeze([...model.coefficients]),
    trainingRows: model.trainingRows,
    trainingMean: model.trainingMean,
    trainingRmse: model.trainingRmse,
    cutoff: Date.parse(report.cutoff),
    ridgeAlpha: report.policy.ridgeAlpha,
  });
}

function decisionKey(row) { return `${row.marketId}:${row.sequence}`; }

export function selectPostBoundaryRowsV1(decisions, labels, boundaryByte) {
  NEED(Array.isArray(decisions) && Array.isArray(labels)
    && Number.isSafeInteger(boundaryByte) && boundaryByte >= 0, 'BOUNDARY_INPUT');
  const laterDecisions = decisions.filter(row => Number.isSafeInteger(row.byteEnd)
    && row.byteEnd > boundaryByte);
  const keys = new Set(laterDecisions.map(decisionKey));
  NEED(keys.size === laterDecisions.length, 'DUPLICATE_DECISION_KEY');
  const laterLabels = labels.filter(row => keys.has(decisionKey(row)));
  return {decisions: laterDecisions, labels: laterLabels};
}

export function sealedEndByte(file) {
  const fd = fs.openSync(file, 'r');
  try {
    const size = fs.fstatSync(fd).size;
    const buffer = Buffer.alloc(64 * 1024);
    for (let cursor = size; cursor > 0;) {
      const start = Math.max(0, cursor - buffer.length), length = cursor - start;
      fs.readSync(fd, buffer, 0, length, start);
      for (let i = length - 1; i >= 0; i--) if (buffer[i] === 10) return start + i + 1;
      cursor = start;
    }
    return 0;
  } finally { fs.closeSync(fd); }
}

export async function hashRangeV1(file, endByte, prefixByteCount) {
  const whole = createHash('sha256'), prefix = createHash('sha256');
  let offset = 0;
  for await (const chunk of fs.createReadStream(file, {start: 0, end: endByte - 1})) {
    whole.update(chunk);
    if (offset < prefixByteCount) prefix.update(chunk.subarray(0, Math.min(chunk.length, prefixByteCount - offset)));
    offset += chunk.length;
  }
  NEED(offset === endByte, 'SOURCE_HASH_SHORT_READ');
  return {sha256: whole.digest('hex'), prefixSha256: prefix.digest('hex')};
}

function readAndVerifyPriorReport() {
  const bytes = fs.readFileSync(PRIOR_REPORT_PATH);
  const report = verifyPinnedPriorReportBytesV1(bytes);
  frozenExecutableReturnModelV1(report);
  const verifiedCode = report.code.map(({path: relative, sha256: expected}) => {
    const actual = fileSha256(path.resolve(REPO_ROOT, relative));
    NEED(actual === expected, 'PRIOR_CODE_CHANGED');
    return {path: relative, sha256: actual};
  });
  return {report, bytes, verifiedCode};
}

function aggregate(rows) {
  if (!rows.length) return {n: 0, meanNet: null, medianNet: null, positiveFraction: null,
    stressedMeanNet: null, marketClusters: 0, positiveMarketClusters: 0,
    equalWeightedMarketMean: null, marketClusterStandardError: null};
  const sorted = rows.map(row => row.net).sort((a, b) => a - b), market = new Map();
  for (const row of rows) {
    const cell = market.get(row.marketId) ?? {sum: 0, n: 0};
    cell.sum += row.net; cell.n++; market.set(row.marketId, cell);
  }
  const means = [...market.values()].map(cell => cell.sum / cell.n);
  const average = means.reduce((sum, value) => sum + value, 0) / means.length;
  const variance = means.length > 1
    ? means.reduce((sum, value) => sum + (value - average) ** 2, 0) / (means.length - 1) : null;
  return {n: rows.length, meanNet: rows.reduce((sum, row) => sum + row.net, 0) / rows.length,
    medianNet: sorted.length % 2 ? sorted[(sorted.length - 1) / 2]
      : (sorted[sorted.length / 2 - 1] + sorted[sorted.length / 2]) / 2,
    positiveFraction: rows.filter(row => row.net > 0).length / rows.length,
    stressedMeanNet: rows.reduce((sum, row) => sum + row.stressedNet, 0) / rows.length,
    marketClusters: means.length, positiveMarketClusters: means.filter(value => value > 0).length,
    equalWeightedMarketMean: average,
    marketClusterStandardError: variance === null ? null : Math.sqrt(variance / means.length)};
}

export async function runExecutableReturnLaterV1({source = SOURCE_PATH, output = OUTPUT_PATH} = {}) {
  NEED(!fs.existsSync(output), 'OUTPUT_ALREADY_EXISTS');
  const started = performance.now();
  const {report: prior, bytes: priorBytes, verifiedCode} = readAndVerifyPriorReport();
  const universeBytes = fs.readFileSync(path.join(REPO_ROOT, 'live-paper', 'price-universe.json'));
  NEED(gitBlobSha(universeBytes) === PRICE_UNIVERSE_GIT_BLOB, 'PRICE_UNIVERSE_MAIN_BLOB');
  const model = frozenExecutableReturnModelV1(prior);

  const before = fs.statSync(source), endByte = sealedEndByte(source), afterFreeze = fs.statSync(source);
  NEED(endByte > PRIOR_SOURCE_BYTES && endByte === before.size, 'SOURCE_NOT_SEALED');
  assertStableSourceIdentityV1(before, afterFreeze, 'SOURCE_NOT_SEALED');
  const sourceHashes = await hashRangeV1(source, endByte, PRIOR_SOURCE_BYTES);
  const afterInitialHash = fs.statSync(source);
  assertStableSourceIdentityV1(before, afterInitialHash, 'SOURCE_CHANGED_DURING_HASH');
  NEED(sourceHashes.prefixSha256 === PRIOR_SOURCE_SHA256, 'PRIOR_PREFIX_MISMATCH');

  const universe = JSON.parse(universeBytes.toString('utf8'));
  let publication = createBookPublicationState(universe);
  const active = new Map(), streams = [], decisions = [];
  let rows = 0, acceptedBookRows = 0, firstAt = null, lastAt = null, consumedBytes = 0;
  for (const line of journalLines(source, {endByte})) {
    const rowStart = consumedBytes;
    consumedBytes += Buffer.byteLength(line, 'utf8') + 1;
    NEED(consumedBytes <= endByte, 'SOURCE_ROW_BOUNDARY');
    rows++;
    const row = JSON.parse(line);
    publication = applyBookPublication(publication, row);
    if (resetBookEpochs(active, row)) continue;
    const book = row.payload?.book;
    if (!book) continue;
    const at = Date.parse(row.at), requestAt = Date.parse(book.requestStartedAt), receivedAt = Date.parse(book.receivedAt);
    if (!Number.isFinite(at) || !Number.isFinite(requestAt) || !Number.isFinite(receivedAt)
      || at - receivedAt > 20_000 || requestAt > at) continue;
    let stream = active.get(book.marketId);
    if (!stream) { stream = []; active.set(book.marketId, stream); streams.push(stream); }
    const features = bookFeatures(book, stream.at(-1));
    if (!features) continue;
    const point = {...features, marketId: book.marketId, exchangeId: book.exchangeId,
      at, requestAt, sequence: row.sequence, byteStart: rowStart, byteEnd: consumedBytes};
    stream.push(point); decisions.push(point); acceptedBookRows++;
    firstAt ??= at; lastAt = at;
  }
  NEED(consumedBytes === endByte, 'SOURCE_BYTE_COUNT');

  const labelResult = executableReturnLabelsV1(streams, EXECUTABLE_RETURN_POLICY_V1);
  const later = selectPostBoundaryRowsV1(decisions, labelResult.labels, PRIOR_SOURCE_BYTES);
  NEED(later.decisions.length > 0, 'NO_POST_BOUNDARY_DECISIONS');
  const firstLater = later.decisions.reduce((best, row) => row.byteStart < best.byteStart ? row : best);
  const lastLater = later.decisions.reduce((best, row) => row.byteEnd > best.byteEnd ? row : best);
  const scored = scoreExecutableReturnModelV1(model, later.decisions, later.labels,
    Number.NEGATIVE_INFINITY, EXECUTABLE_RETURN_POLICY_V1);
  const priorDecisionLabels = labelResult.labels.length - later.labels.length;
  const uniqueEvents = new Set(later.labels.map(decisionKey));
  const sourceAfterHash = await hashRangeV1(source, endByte, PRIOR_SOURCE_BYTES);
  const afterRead = fs.statSync(source);
  assertStableSourceIdentityV1(before, afterRead, 'SOURCE_CHANGED_DURING_REPLAY');
  assertStableSourceHashesV1(sourceHashes, sourceAfterHash);

  const report = {
    schemaVersion: 1,
    id: 'executable-return-alpha-later-20261014-v2',
    generatedAt: new Date().toISOString(),
    mode: 'frozen_executable_return_model_forward_suffix_v1',
    model: {sourceReport: path.basename(PRIOR_REPORT_PATH), sourceReportSha256: PRIOR_REPORT_SHA256,
      frozenCutoff: prior.cutoff, trainingRows: model.trainingRows,
      fixedCoefficientSetSha256: sha256(Buffer.from(JSON.stringify({coefficients: model.coefficients,
        featureMean: model.mean, featureScale: model.scale}))), refit: false},
    source: {kind: 'sealed append-only public-book journal', file: path.basename(source),
      bytes: endByte, sha256: sourceHashes.sha256, rows, acceptedBookRows,
      firstAcceptedAt: firstAt === null ? null : new Date(firstAt).toISOString(),
      lastAcceptedAt: lastAt === null ? null : new Date(lastAt).toISOString(),
      identityStable: true, fullHashStable: true,
      previousReportBoundary: {byteEndExclusive: PRIOR_SOURCE_BYTES, prefixSha256: PRIOR_SOURCE_SHA256,
        previousLastAcceptedAt: prior.source.lastAt,
        firstLaterDecision: {byteStart: firstLater.byteStart, byteEnd: firstLater.byteEnd,
          at: new Date(firstLater.at).toISOString(), marketId: firstLater.marketId,
          exchangeId: firstLater.exchangeId, sequence: firstLater.sequence},
        lastLaterDecision: {byteStart: lastLater.byteStart, byteEnd: lastLater.byteEnd,
          at: new Date(lastLater.at).toISOString(), marketId: lastLater.marketId,
          exchangeId: lastLater.exchangeId, sequence: lastLater.sequence}},
      laterDecisionBookRows: later.decisions.length, laterUniqueDecisionEvents: uniqueEvents.size,
      laterUniqueMarkets: new Set(later.labels.map(row => row.marketId)).size},
    policy: EXECUTABLE_RETURN_POLICY_V1,
    heldout: {counterfactualOutcomes: aggregate(later.labels),
      selectedCounterfactual: scored.selected, selectedDecisions: scored.selectedDecisions,
      selectedWithOutcome: scored.selectedWithOutcome, selectedCensored: scored.selectedCensored,
      selectedRecords: scored.selectedRecords, priorDecisionLabelsExcluded: priorDecisionLabels,
      labelExclusions: labelResult.excluded},
    code: [{path: 'data/run-executable-return-alpha-later-20261014.mjs', sha256: fileSha256(fileURLToPath(import.meta.url))},
      {path: 'data/run-executable-return-alpha-later-20261014.test.mjs',
        sha256: fileSha256(path.join(REPO_ROOT, 'data', 'run-executable-return-alpha-later-20261014.test.mjs'))},
      ...verifiedCode,
      {path: 'live-paper/price-universe.json', gitBlobSha: PRICE_UNIVERSE_GIT_BLOB}],
    claims: {accountReads: 0, networkRequests: 0, runtimeChanges: 0, contestOrders: 0,
      actualFillsProven: false, profitabilityEstablished: false},
    durationMs: performance.now() - started,
    limitations: [
      'The frozen model and two-tick selection hurdle were not refit or changed; all decisions are scored only after the exact previous report byte boundary.',
      'Outcomes use displayed best-price depth and strict chronological receipts; they are counterfactual, not exchange-confirmed fills.',
      'The later suffix is from the same 237-market capture and was read by other descriptive screens, so this is a fixed-policy forward test, not a pristine independent tournament sample.',
      'Market/time clusters are correlated; a short suffix and any small count cannot establish durable profitability.',
      'Fees are zero as specified; the model retains one adverse tick on both entry and exit.',
    ],
  };
  fs.writeFileSync(output, JSON.stringify(report, null, 2) + '\n', {flag: 'wx'});
  return report;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  runExecutableReturnLaterV1().then(report => console.log(JSON.stringify({
    report: path.basename(OUTPUT_PATH), sourceBytes: report.source.bytes,
    sourceSha256: report.source.sha256, priorBoundaryBytes: PRIOR_SOURCE_BYTES,
    modelCutoff: report.model.frozenCutoff, refit: report.model.refit,
    outcomes: report.heldout.counterfactualOutcomes,
    selected: report.heldout.selectedCounterfactual,
    actualFillsProven: report.claims.actualFillsProven,
    profitabilityEstablished: report.claims.profitabilityEstablished,
    durationMs: report.durationMs,
  }))).catch(error => { console.error(error?.message === 'OUTPUT_ALREADY_EXISTS' ? error.message : error?.message ?? 'FORWARD_TEST_FAILED'); process.exitCode = 1; });
}
