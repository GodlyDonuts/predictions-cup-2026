import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {performance} from 'node:perf_hooks';
import {journalLines} from '../live-paper/journal-lines.mjs';
import {sealedEnd} from './replay-dual-no-timelapse-20261005.mjs';
import {applyBookPublication, createBookPublicationState} from '../live-paper/book-publication-v2.mjs';
import {hashNativePrefix} from '../live-paper/native-dip-host-v1.mjs';
import {BOOK_ALPHA_POLICY, bookFeatures, resetBookEpochs} from '../research/book-alpha-v1.mjs';
import {BOOK_ALPHA_FRESH_CUTOFF, scoreBookAlphaFreshHoldoutV1, verifyPinnedBookPrefixV1} from '../research/book-alpha-fresh-holdout-v1.mjs';

const output = process.env.BAAL_BOOK_ALPHA_OUTPUT ?? 'data/book-alpha-fresh-holdout-20261014.json';
const runtimeDir = path.join(process.env.LOCALAPPDATA, 'SigTradingCup', 'paper-runtime');
const file = process.env.BAAL_BOOK_ALPHA_SOURCE ?? path.join(runtimeDir, 'public-books.jsonl');
if (fs.existsSync(output)) throw Error('Frozen diagnostic already exists; never overwrite');
const universe = JSON.parse(fs.readFileSync(new URL('../live-paper/price-universe.json', import.meta.url), 'utf8'));
const started = performance.now();
const initialStat = fs.statSync(file), bytes = sealedEnd(file), beforeStat = fs.statSync(file);
if (initialStat.dev !== beforeStat.dev || initialStat.ino !== beforeStat.ino || bytes <= 0 || bytes > beforeStat.size) throw Error('BOOK_SOURCE_IDENTITY');
const source = {kind: 'sealed_public_books_prefix', file: path.basename(file), bytes,
  sha256: (await hashNativePrefix(file, bytes, 0)).hash};
const before = {dev: beforeStat.dev, ino: beforeStat.ino, size: beforeStat.size};
const beforeHash = (await hashNativePrefix(file, bytes, 0)).hash;
if (beforeHash !== source.sha256) throw Error('BOOK_SOURCE_HASH_BEFORE');

let publication = createBookPublicationState(universe), rows = 0, bookRows = 0, first = null, last = null;
const active = new Map(), streams = [];
for (const line of journalLines(file, {endByte: bytes})) {
  rows++;
  const row = JSON.parse(line);
  publication = applyBookPublication(publication, row);
  if (resetBookEpochs(active, row)) continue;
  const book = row.payload.book, at = Date.parse(row.at), requestAt = Date.parse(book.requestStartedAt), receivedAt = Date.parse(book.receivedAt);
  if (!Number.isFinite(at) || !Number.isFinite(requestAt) || !Number.isFinite(receivedAt)
    || at - receivedAt > 20000 || requestAt > at) continue;
  let stream = active.get(book.marketId);
  if (!stream) { stream = []; active.set(book.marketId, stream); streams.push(stream); }
  const features = bookFeatures(book, stream.at(-1));
  if (!features) continue;
  stream.push({...features, marketId: book.marketId, exchangeId: book.exchangeId, at, requestAt, sequence: row.sequence});
  first ??= at; last = at; bookRows++;
}
if (first === null || last <= first) throw Error('BOOK_SOURCE_EMPTY');

const afterStat = fs.statSync(file), afterHash = (await hashNativePrefix(file, bytes, 0)).hash;
const sourceVerification = verifyPinnedBookPrefixV1({source, before, after: afterStat, beforeHash, afterHash});
const result = scoreBookAlphaFreshHoldoutV1(streams);
const runnerBytes = fs.readFileSync(fileURLToPath(import.meta.url));
const runnerGitBlobHeader = Buffer.concat([Buffer.from('blob ' + runnerBytes.length), Buffer.from([0])]);
const runnerGitBlobSha1 = createHash('sha1').update(runnerGitBlobHeader).update(runnerBytes).digest('hex');
const modelPath = fileURLToPath(new URL('../research/book-alpha-v1.mjs', import.meta.url));
const modelBytes = fs.readFileSync(modelPath);
const modelGitBlobSha1 = createHash('sha1').update(Buffer.concat([Buffer.from('blob ' + modelBytes.length), Buffer.from([0])])).update(modelBytes).digest('hex');
const report = {
  schemaVersion: 1,
  at: new Date().toISOString(),
  mode: 'frozen_book_alpha_later_chronological_holdout',
  policy: BOOK_ALPHA_POLICY,
  cutoff: BOOK_ALPHA_FRESH_CUTOFF,
  source: {...source, rows, bookRows, streams: streams.length,
    first: new Date(first).toISOString(), last: new Date(last).toISOString(),
    hours: (last - first) / 3600000, sequence: publication.sequence},
  sourceVerification,
  results: result.results,
  runtimeChanges: 0,
  accountReads: 0,
  apiRequests: 0,
  contestOrders: 0,
  actualFillsProven: false,
  realizedPnl: null,
  profitabilityEstablished: false,
  wallMs: performance.now() - started,
  code: {runnerGitBlobSha1, modelGitBlobSha1},
  limitations: [
    'This is a frozen offline model validation, not an actual-fill or portfolio-P&L test.',
    'Book observations and overlapping horizons are correlated across related election markets.',
    'Missing depth, invalidation, stale receipts and incomplete future horizons are censored by the frozen labeler; censored selected signals are counted separately.',
    'The 1-second latency, one-tick threshold and zero-fee assumption are inherited unchanged from the development policy.',
  ],
};
fs.writeFileSync(output, JSON.stringify(report, null, 2) + '\n', {flag: 'wx'});
console.log(JSON.stringify({report: output, cutoff: report.cutoff, sourceBytes: bytes,
  sourceSha256: source.sha256, rows, bookRows, hours: report.source.hours,
  horizons: report.results.map(row => ({horizonMs: row.horizonMs, trainingLabels: row.trainingLabels,
    heldOutLabels: row.heldOutLabels, modelRmse: row.modelRmse,
    unchangedBidRmse: row.unchangedBidRmse, selected: row.selectedQuoteSignals,
    measured: row.measuredSelectedSignals, positiveAfterOneTick: row.positiveAfterOneTickPerSide,
    unavailable: row.unavailable})), profitabilityEstablished: false}));
