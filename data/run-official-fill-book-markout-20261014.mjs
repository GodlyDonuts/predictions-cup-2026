import fs from 'node:fs';
import path from 'node:path';
import readline from 'node:readline';
import {createReadStream} from 'node:fs';
import {createOfficialFillBookTracker, createPublicBookJournalVerifier, verifyOrderAckReceiptChain} from '../research/official-fill-book-markout-v1.mjs';

const need = (ok, code) => { if (!ok) throw Error('OFFICIAL_FILL_MARKOUT_RUN_' + code); };
const runtime = process.env.LOCALAPPDATA && path.join(process.env.LOCALAPPDATA, 'SigTradingCup', 'paper-runtime');
need(runtime && path.isAbsolute(runtime) && fs.existsSync(runtime), 'RUNTIME');

function identity(file) {
  const stat = fs.statSync(file);
  need(stat.isFile(), 'SOURCE_FILE');
  return {size: stat.size, mtimeMs: stat.mtimeMs, ino: stat.ino};
}
function sameIdentity(a, b) { return a.size === b.size && a.mtimeMs === b.mtimeMs && a.ino === b.ino; }
function stableJson(file) {
  const before = identity(file);
  const value = JSON.parse(fs.readFileSync(file, 'utf8'));
  need(sameIdentity(before, identity(file)), 'INPUT_CHANGED');
  return value;
}

const accountFile = path.join(runtime, 'official-account.json');
const receiptFile = path.join(runtime, 'exact-money-receipts.jsonl');
const bookFile = path.join(runtime, 'public-books.jsonl');
const universeFile = new URL('../live-paper/price-universe.json', import.meta.url);
const account = stableJson(accountFile);
need(Array.isArray(account.fills?.rows) && typeof account.fills.hasMore === 'boolean', 'FILL_CACHE');
const receiptIdentity = identity(receiptFile);
const receiptText = fs.readFileSync(receiptFile, 'utf8');
need(sameIdentity(receiptIdentity, identity(receiptFile)) && receiptText.endsWith('\n'), 'RECEIPT_CHANGED_OR_PARTIAL');
const receiptRows = receiptText.trimEnd().split('\n').map(line => JSON.parse(line));
const acknowledgements = verifyOrderAckReceiptChain(receiptRows);
const tracker = createOfficialFillBookTracker({fillRows: account.fills.rows, fillPageHasMore: account.fills.hasMore,
  orderAcknowledgements: acknowledgements});
const universe = JSON.parse(fs.readFileSync(universeFile, 'utf8'));
const verifier = createPublicBookJournalVerifier(universe);
const sourceBefore = identity(bookFile);
need(sourceBefore.size > 0, 'EMPTY_BOOK_ARCHIVE');
const tailFd = fs.openSync(bookFile, 'r');
try {
  const last = Buffer.alloc(1);
  fs.readSync(tailFd, last, 0, 1, sourceBefore.size - 1);
  need(last[0] === 10, 'BOOK_ARCHIVE_PARTIAL_TAIL');
} finally { fs.closeSync(tailFd); }

let sourceBytes = 0;
const input = createReadStream(bookFile, {encoding: 'utf8'});
const lines = readline.createInterface({input, crlfDelay: Infinity});
for await (const line of lines) {
  need(line.length > 0, 'BOOK_ARCHIVE_EMPTY_RECORD');
  sourceBytes += Buffer.byteLength(line, 'utf8') + 1;
  const record = JSON.parse(line);
  const book = verifier.observe(record);
  if (book) tracker.observeBook(book);
}
const sourceAfter = identity(bookFile);
need(sourceBytes === sourceBefore.size && sameIdentity(sourceBefore, sourceAfter), 'BOOK_ARCHIVE_CHANGED');
need(sameIdentity(receiptIdentity, identity(receiptFile)), 'RECEIPTS_CHANGED');
const report = {
  ...tracker.finish(),
  inputIntegrity: {
    publicBookJournal: verifier.stats(),
    publicBookBytes: sourceBytes,
    exactMoneyReceiptRowsVerified: acknowledgements.receiptRowsVerified,
    orderAcknowledgementRowsMatched: acknowledgements.matchedOrderAckRows,
    conflictingOrderAcknowledgementIds: acknowledgements.conflictingOrderIds,
    conflictingOrderAcknowledgementRows: acknowledgements.conflictingRows,
    accountFillPageAt: account.fills.at ?? null,
    accountFillCacheAt: account.updatedAt ?? null,
  },
  producedAt: new Date().toISOString(),
};
const outDir = path.join(runtime, 'local-research');
fs.mkdirSync(outDir, {recursive: true, mode: 0o700});
const stamp = new Date().toISOString().replace(/[:.]/g, '-');
const outFile = path.join(outDir, `official-fill-book-markout-v1-${stamp}.json`);
fs.writeFileSync(outFile, JSON.stringify(report, null, 2) + '\n', {flag: 'wx', mode: 0o600});
console.log(JSON.stringify({reportFile: outFile, coverage: report.coverage,
  horizons: report.horizons, inputIntegrity: report.inputIntegrity, assumptions: report.assumptions}, null, 2));

