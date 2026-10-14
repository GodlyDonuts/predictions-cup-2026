import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {performance} from 'node:perf_hooks';
import {journalLines} from '../live-paper/journal-lines.mjs';
import {applyBookPublication, createBookPublicationState} from '../live-paper/book-publication-v2.mjs';
import {bookFeatures, resetBookEpochs} from '../research/book-alpha-v1.mjs';
import {
  EXECUTABLE_RETURN_POLICY_V1,
  executableReturnLabelsV1,
  fitExecutableReturnModelV1,
  scoreExecutableReturnModelV1,
} from '../research/executable-return-alpha-v1.mjs';

const OUTPUT = process.env.BAAL_EXECUTABLE_RETURN_OUTPUT ?? 'data/executable-return-alpha-20261014.json';
const SOURCE = process.env.BAAL_EXECUTABLE_RETURN_SOURCE
  ?? path.join(process.env.LOCALAPPDATA, 'SigTradingCup', 'paper-runtime', 'public-books.jsonl');
const REPO_ROOT_URL = new URL('../', import.meta.url);
// This is the last receipt in the prefix already used by the prior book-alpha
// diagnostic. All rows after it are reserved for this later chronological test.
const CUTOFF = Date.parse('2026-10-14T14:31:30.476Z');

function sealedEnd(file) {
  const fd = fs.openSync(file, 'r');
  try {
    const size = fs.fstatSync(fd).size;
    const buffer = Buffer.alloc(64 * 1024);
    let cursor = size;
    while (cursor > 0) {
      const start = Math.max(0, cursor - buffer.length), length = cursor - start;
      fs.readSync(fd, buffer, 0, length, start);
      for (let i = length - 1; i >= 0; i--) if (buffer[i] === 10) return start + i + 1;
      cursor = start;
    }
    return 0;
  } finally { fs.closeSync(fd); }
}

async function hashPrefix(file, bytes) {
  const hash = createHash('sha256');
  for await (const chunk of fs.createReadStream(file, {start: 0, end: bytes - 1})) hash.update(chunk);
  return hash.digest('hex');
}

function hashFile(file) {
  return createHash('sha256').update(fs.readFileSync(file)).digest('hex');
}

function aggregateAll(labels) {
  if (!labels.length) return {n: 0, meanNet: null, positiveFraction: null, marketClusters: 0};
  const perMarket = new Map();
  let sum = 0, wins = 0;
  for (const row of labels) {
    sum += row.net;
    if (row.net > 0) wins++;
    const current = perMarket.get(row.marketId) ?? {sum: 0, n: 0};
    current.sum += row.net; current.n++;
    perMarket.set(row.marketId, current);
  }
  return {n: labels.length, meanNet: sum / labels.length, positiveFraction: wins / labels.length,
    marketClusters: perMarket.size,
    equalWeightedMarketMean: [...perMarket.values()].reduce((n, row) => n + row.sum / row.n, 0) / perMarket.size};
}

if (fs.existsSync(OUTPUT)) throw Error('Frozen executable-return result already exists; refusing overwrite');
if (!Number.isFinite(CUTOFF)) throw Error('EXECUTABLE_RETURN_CUTOFF');
const started = performance.now();
const before = fs.statSync(SOURCE), bytes = sealedEnd(SOURCE), afterFreeze = fs.statSync(SOURCE);
if (!bytes || before.dev !== afterFreeze.dev || before.ino !== afterFreeze.ino
  || bytes > afterFreeze.size) throw Error('EXECUTABLE_RETURN_SOURCE_IDENTITY');
const sourceHash = await hashPrefix(SOURCE, bytes);

const universe = JSON.parse(fs.readFileSync(new URL('../live-paper/price-universe.json', import.meta.url), 'utf8'));
let publication = createBookPublicationState(universe);
const active = new Map(), streams = [];
let rows = 0, books = 0, firstAt = null, lastAt = null;
for (const line of journalLines(SOURCE, {endByte: bytes})) {
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
  stream.push({...features, marketId: book.marketId, exchangeId: book.exchangeId, at, requestAt,
    sequence: row.sequence});
  firstAt ??= at; lastAt = at; books++;
}
if (!Number.isFinite(firstAt) || lastAt <= CUTOFF + EXECUTABLE_RETURN_POLICY_V1.horizonMs) {
  throw Error('EXECUTABLE_RETURN_INSUFFICIENT_FUTURE_WINDOW');
}

const labelsResult = executableReturnLabelsV1(streams, EXECUTABLE_RETURN_POLICY_V1);
const model = fitExecutableReturnModelV1(labelsResult.labels, CUTOFF, EXECUTABLE_RETURN_POLICY_V1);
const decisions = streams.flat();
const scored = scoreExecutableReturnModelV1(model, decisions, labelsResult.labels, CUTOFF,
  EXECUTABLE_RETURN_POLICY_V1);
const heldout = labelsResult.labels.filter(row => row.decisionAt >= CUTOFF);
const afterRead = fs.statSync(SOURCE), afterHash = await hashPrefix(SOURCE, bytes);
if (before.dev !== afterRead.dev || before.ino !== afterRead.ino || afterRead.size < bytes
  || sourceHash !== afterHash) throw Error('EXECUTABLE_RETURN_SOURCE_CHANGED');

const report = {
  schemaVersion: 1,
  generatedAt: new Date().toISOString(),
  mode: 'fixed executable-return ridge; later chronological capture slice',
  policy: EXECUTABLE_RETURN_POLICY_V1,
  cutoff: new Date(CUTOFF).toISOString(),
  source: {kind: 'sealed public-book prefix', file: path.basename(SOURCE), bytes, sha256: sourceHash,
    rows, acceptedBookRows: books, streams: streams.length,
    firstAt: new Date(firstAt).toISOString(), lastAt: new Date(lastAt).toISOString(),
    prefixIdentityStable: true, prefixHashStable: true},
  model: {trainingRows: model.trainingRows, trainingMean: model.trainingMean,
    trainingRmse: model.trainingRmse, coefficients: model.coefficients,
    featureMean: model.mean, featureScale: model.scale},
  code: [
    'data/run-executable-return-alpha-20261014.mjs',
    'research/executable-return-alpha-v1.mjs',
    'research/book-alpha-v1.mjs',
    'live-paper/journal-lines.mjs',
    'live-paper/book-publication-v2.mjs',
  ].map(relative => ({path: relative, sha256: hashFile(new URL(relative, REPO_ROOT_URL))})),
  heldout: {decisionBookRows: decisions.filter(row => row.at >= CUTOFF).length,
    labeledExecutableOutcomes: heldout.length, unconditional: aggregateAll(heldout),
    selectedCounterfactual: scored.selected, selectedDecisions: scored.selectedDecisions,
    selectedWithOutcome: scored.selectedWithOutcome, selectedCensored: scored.selectedCensored,
    censoredRate: scored.censoredRate},
  exclusions: labelsResult.excluded,
  accountReads: 0,
  networkRequests: 0,
  runtimeChanges: 0,
  contestOrders: 0,
  actualFillsProven: false,
  profitabilityEstablished: false,
  wallMs: performance.now() - started,
  limitations: [
    'Counterfactual marketable one-share outcomes use displayed best-price depth; they are not confirmed fills.',
    'The test window is short and observations share markets and time regimes; cluster summaries are descriptive, not an iid significance test.',
    'The candidate features were informed by prior research; this one later source slice is a falsification check, not a production qualification.',
    'One tick of adverse execution is charged on both entry and exit; fees are zero per the contest specification.',
  ],
};
fs.writeFileSync(OUTPUT, JSON.stringify(report, null, 2) + '\n', {flag: 'wx'});
console.log(JSON.stringify({report: OUTPUT, sourceBytes: bytes, sourceSha256: sourceHash,
  cutoff: report.cutoff, lastAt: report.source.lastAt, trainLabels: model.trainingRows,
  heldoutLabels: heldout.length, selected: scored.selectedDecisions,
  selectedWithOutcome: scored.selectedWithOutcome, meanCounterfactualNet: scored.selected.meanNet,
  meanStressedNet: scored.selected.stressedMeanNet, marketClusters: scored.selected.marketClusters,
  profitabilityEstablished: false, wallMs: report.wallMs}));
