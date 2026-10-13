import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {performance} from 'node:perf_hooks';
import {analyzeExecutableTrendReversionV1, EXECUTABLE_TREND_REVERSION_V1} from '../research/executable-trend-reversion-v1.mjs';
import {applyBookPublication, createBookPublicationState} from '../live-paper/book-publication-v2.mjs';

const runtime = process.env.LOCALAPPDATA && path.join(process.env.LOCALAPPDATA, 'SigTradingCup', 'paper-runtime');
if (!runtime) throw Error('LOCALAPPDATA_REQUIRED');
const sourceFile = path.join(runtime, 'public-books.jsonl');
const outputFile = path.resolve('data/executable-trend-reversion-20261013.json');
if (fs.existsSync(outputFile)) throw Error('RESULT_EXISTS: preserve the first sealed replay; create a new version for a new prefix');
if (!fs.existsSync(sourceFile)) throw Error('PUBLIC_BOOK_CAPTURE_MISSING');

function lastCompletePrefixBytes(file, size) {
  if (size === 0) return 0;
  const fd = fs.openSync(file, 'r');
  try {
    const end = Buffer.alloc(1);
    fs.readSync(fd, end, 0, 1, size - 1);
    if (end[0] === 0x0a) return size;
    const chunkSize = 64 * 1024;
    for (let endAt = size; endAt > 0;) {
      const startAt = Math.max(0, endAt - chunkSize), length = endAt - startAt, bytes = Buffer.allocUnsafe(length);
      fs.readSync(fd, bytes, 0, length, startAt);
      const newline = bytes.lastIndexOf(0x0a);
      if (newline >= 0) return startAt + newline + 1;
      endAt = startAt;
    }
    throw Error('NO_COMPLETE_JSONL_RECORD');
  } finally { fs.closeSync(fd); }
}

async function readSealedPrefix(file, prefixBytes) {
  const hash = createHash('sha256'), observations = [], counts = {records: 0, books: 0, invalidations: 0,
    sessions: 0, sessionStops: 0, duplicateSequence: 0};
  let chain = null, pending = Buffer.alloc(0), firstEventAt = null, cutoffAt = null;
  let globalSegment = 0;
  const marketSegments = new Map(), lastSourceSequence = new Map();
  const record = line => {
    if (!line.length) throw Error('EMPTY_JSONL_RECORD');
    const value = JSON.parse(line.toString('utf8'));
    if (!chain) {
      if (value.kind !== 'session_start' || value.sequence !== 1) throw Error('PUBLIC_BOOK_CHAIN_MUST_START_WITH_SESSION');
      chain = createBookPublicationState(value.payload?.scope);
    }
    chain = applyBookPublication(chain, value);
    counts.records++;
    firstEventAt ??= value.at;
    cutoffAt = value.at;
    if (value.kind === 'session_start') { counts.sessions++; globalSegment++; return; }
    if (value.kind === 'session_stop') { counts.sessionStops++; globalSegment++; return; }
    if (value.kind === 'invalidation') {
      counts.invalidations++;
      if (value.payload.marketId === null) globalSegment++;
      else marketSegments.set(value.payload.marketId, (marketSegments.get(value.payload.marketId) ?? 0) + 1);
      return;
    }
    if (value.kind !== 'book') return;
    const book = value.payload.book, old = lastSourceSequence.get(book.marketId);
    if (old === book.sequence) { counts.duplicateSequence++; return; }
    lastSourceSequence.set(book.marketId, book.sequence);
    observations.push({marketId: book.marketId, exchangeId: book.exchangeId, receivedAt: book.receivedAt,
      sourceAt: book.sourceAt, sequence: book.sequence, sessionId: value.sessionId,
      segment: `${globalSegment}:${marketSegments.get(book.marketId) ?? 0}`,
      bids: book.bids, asks: book.asks});
    counts.books++;
  };
  let bytesRead = 0;
  for await (const chunk of fs.createReadStream(file, {start: 0, end: prefixBytes - 1, highWaterMark: 1024 * 1024})) {
    bytesRead += chunk.length;
    hash.update(chunk);
    const joined = pending.length ? Buffer.concat([pending, chunk]) : chunk;
    let begin = 0, newline;
    while ((newline = joined.indexOf(0x0a, begin)) >= 0) {
      record(joined.subarray(begin, newline));
      begin = newline + 1;
    }
    pending = Buffer.from(joined.subarray(begin));
  }
  if (bytesRead !== prefixBytes || pending.length !== 0 || !chain || !chain.closed && !chain.sequence) throw Error('SEALED_PREFIX_CHANGED_OR_INCOMPLETE');
  const availableAt = observations.map(row => row.receivedAt).sort();
  return {hash: hash.digest('hex'), observations, counts, firstEventAt, cutoffAt,
    firstBookAt: availableAt[0] ?? null, lastBookAt: availableAt.at(-1) ?? null,
    tournamentId: chain.tournamentId, scopeHash: chain.scopeHash};
}

const started = performance.now();
const sourceStat = fs.statSync(sourceFile), prefixBytes = lastCompletePrefixBytes(sourceFile, sourceStat.size);
const captured = await readSealedPrefix(sourceFile, prefixBytes);
const firstBookMs = captured.firstBookAt ? Date.parse(captured.firstBookAt) : NaN;
const lastBookMs = captured.lastBookAt ? Date.parse(captured.lastBookAt) : NaN;
if (!Number.isFinite(firstBookMs) || !Number.isFinite(lastBookMs) || lastBookMs <= firstBookMs) throw Error('INSUFFICIENT_BOOK_TIMESPAN');
const splitAt = new Date(firstBookMs + Math.floor((lastBookMs - firstBookMs) * 0.7)).toISOString();
const analysisStarted = performance.now();
const result = analyzeExecutableTrendReversionV1({observations: captured.observations, splitAt});
const completedAt = new Date().toISOString();
const report = {
  schemaVersion: 1,
  id: 'executable-trend-reversion-20261013-v1',
  generatedAt: completedAt,
  source: {kind: 'append-only validated public depth-20 book journal', path: '%LOCALAPPDATA%/SigTradingCup/paper-runtime/public-books.jsonl',
    sourceBytesObservedAtStart: sourceStat.size, sealedPrefixBytes: prefixBytes,
    incompleteTailBytesExcluded: sourceStat.size - prefixBytes, sha256: captured.hash, firstEventAt: captured.firstEventAt,
    cutoffAt: captured.cutoffAt, firstBookAt: captured.firstBookAt, lastBookAt: captured.lastBookAt,
    tournamentId: captured.tournamentId, scopeHash: captured.scopeHash, chainVerified: true,
    counts: captured.counts},
  replay: {splitRule: 'first 70% of global captured book-time range for development, final 30% held out', splitAt,
    methodology: result.methodology, observations: result.observations, results: result.results,
    counterfactualOnly: true, actualFills: false, profitabilityEstablished: false},
  runtime: {chainAndReadMs: Math.round(analysisStarted - started), analysisMs: Math.round(performance.now() - analysisStarted),
    totalMs: Math.round(performance.now() - started), recordsPerSecond: Math.round(captured.counts.records / ((analysisStarted - started) / 1000)),
    booksPerSecond: Math.round(captured.counts.books / ((analysisStarted - started) / 1000))},
  decision: 'paper research only; no API, account, collector, live worker, or contest-order operation',
};
fs.writeFileSync(outputFile, JSON.stringify(report, null, 2) + '\n', {flag: 'wx'});
console.log(JSON.stringify({output: outputFile, source: report.source, replaySummary: result.results.filter(row => row.period === 'heldout')
  .map(({period, lookbackMs, holdingMs, rule, decisions, signals, entered, completed, totalNetPnlOneShare, meanNetPnlPerShare, winRate, censoring}) =>
    ({period, lookbackMs, holdingMs, rule, decisions, signals, entered, completed, totalNetPnlOneShare, meanNetPnlPerShare, winRate, censoring})), runtime: report.runtime}));
