import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {payloadHash} from '../execution/ledger.mjs';
import {analyzeQuoteReservationTurnoverV1} from '../research/quote-reservation-turnover-v1.mjs';

const WORKSPACE = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const COHORT = 'maker-book-point-scoped-20261013-v13';
const sha = value => createHash('sha256').update(value).digest('hex');
const fail = code => { throw Error(`QUOTE_TURNOVER_RUN_${code}`); };

function parseArgs(argv) {
  const out = {runtimeDir: path.join(process.env.LOCALAPPDATA ?? '', 'SigTradingCup', 'paper-runtime'),
    workspace: WORKSPACE, output: path.join(WORKSPACE, 'data', 'quote-reservation-turnover-20261013.json')};
  for (let i = 0; i < argv.length; i++) {
    const key = argv[i];
    if (key === '--runtime') out.runtimeDir = path.resolve(argv[++i] ?? fail('RUNTIME_ARG'));
    else if (key === '--workspace') out.workspace = path.resolve(argv[++i] ?? fail('WORKSPACE_ARG'));
    else if (key === '--output') out.output = path.resolve(argv[++i] ?? fail('OUTPUT_ARG'));
    else if (key === '--live' || key === '--orders' || key === '--network') fail('OFFLINE_ONLY');
    else fail('ARGUMENT');
  }
  if (!process.env.LOCALAPPDATA && !argv.includes('--runtime')) fail('LOCALAPPDATA_MISSING');
  return out;
}

function readJson(file) { return JSON.parse(fs.readFileSync(file, 'utf8')); }

function readLastCompleteRecord(file) {
  const before = fs.statSync(file, {bigint: true});
  const size = Number(before.size);
  if (!Number.isSafeInteger(size) || size < 2) fail('JOURNAL_SIZE');
  const fd = fs.openSync(file, 'r');
  let start = -1;
  let end = -1;
  const block = Buffer.alloc(1024 * 1024);
  try {
    let pos = size;
    let firstNewline = -1;
    let previousNewline = -1;
    while (pos > 0 && previousNewline < 0) {
      const n = Math.min(block.length, pos);
      pos -= n;
      fs.readSync(fd, block, 0, n, pos);
      for (let i = n - 1; i >= 0; i--) {
        if (block[i] !== 10) continue;
        const absolute = pos + i;
        if (firstNewline < 0) firstNewline = absolute;
        else { previousNewline = absolute; break; }
      }
    }
    if (firstNewline < 0 || previousNewline < 0) fail('JOURNAL_BOUNDARIES');
    // Journal rows are newline terminated. Ignore a possible uncommitted tail,
    // and read the last fully terminated record inside this fixed file prefix.
    if (firstNewline === size - 1) {
      end = firstNewline;
      start = previousNewline + 1;
    } else {
      end = firstNewline;
      start = previousNewline + 1;
    }
    if (end <= start || end - start > 16 * 1024 * 1024) fail('JOURNAL_RECORD_LENGTH');
    const buf = Buffer.alloc(end - start);
    fs.readSync(fd, buf, 0, buf.length, start);
    const after = fs.statSync(file, {bigint: true});
    if (String(before.dev) !== String(after.dev) || String(before.ino) !== String(after.ino)
      || after.size < BigInt(size)) fail('JOURNAL_REPLACED');
    const line = buf.toString('utf8').replace(/\r$/, '');
    const match = /,"hash":"([a-f0-9]{64})"}$/.exec(line);
    if (!match) fail('JOURNAL_HASH_FIELD');
    const hashEnd = line.length - match[0].length;
    const calculated = sha(line.slice(0, hashEnd) + '}');
    if (calculated !== match[1]) fail('JOURNAL_HASH');
    const row = JSON.parse(line);
    if (!Number.isSafeInteger(row.sequence) || typeof row.previous !== 'string' || !row.state) fail('JOURNAL_ROW');
    return {row, snapshotBytes: end + 1, fileBytes: size, fileDevice: String(before.dev),
      fileInode: String(before.ino), recordHashVerified: true};
  } finally { fs.closeSync(fd); }
}

function verifyBank(bank, pointer, initialState, workspace) {
  const {bankHash, ...body} = bank;
  if (payloadHash(body) !== bankHash || pointer.bankHash !== bankHash || pointer.cohort !== COHORT
    || bank.cohort !== COHORT || bank.executionAuthority !== false) fail('BANK_IDENTITY');
  if (initialState.bankHash !== bankHash || !bank.continuation?.seedStateHash) fail('CONTINUATION_IDENTITY');
  const {bankHash: initialBankHash, ...seedState} = initialState;
  if (initialBankHash !== bankHash || payloadHash(seedState) !== bank.continuation.seedStateHash) fail('CONTINUATION_SEED');
  const identity = Object.entries(bank.codeIdentity ?? {});
  if (!identity.length) fail('CODE_IDENTITY_EMPTY');
  for (const [relative, expected] of identity) {
    const file = path.resolve(workspace, relative);
    if (!file.startsWith(workspace + path.sep) || !fs.existsSync(file) || sha(fs.readFileSync(file)) !== expected)
      fail('FROZEN_CODE_MISMATCH');
  }
  return {bankHash, cohort: bank.cohort, cutoff: bank.cutoff, codeIdentityCount: identity.length,
    codeIdentityDigest: payloadHash(bank.codeIdentity), continuationSeedHash: bank.continuation.seedStateHash};
}

function verifyCursor(runtimeDir, name, cursor) {
  const file = path.join(runtimeDir, name);
  const stat = fs.statSync(file, {bigint: true});
  if (Number(cursor.bytes) > Number(stat.size) || Number(cursor.bytes) < 0
    || Number(cursor.dev) !== Number(stat.dev)) fail('SOURCE_CURSOR');
  return {sequence: cursor.sequence, sourceBytes: cursor.bytes, sourceChainHead: cursor.hash,
    sourcePrefixHash: cursor.sourcePrefixHash, currentFileBytes: Number(stat.size),
    sameDevice: Number(cursor.dev) === Number(stat.dev)};
}

function safeWalletStateDigest(wallet) {
  const quotes = wallet.quotes.map(q => ({id: q.id, marketId: q.marketId, exchangeId: q.exchangeId,
    side: q.side, action: q.action, quantity: q.quantity, remaining: q.remaining, limit: q.limit,
    createdAt: q.createdAt, activationAt: q.activationAt, expiresAt: q.expiresAt,
    cancelEffectiveAt: q.cancelEffectiveAt ?? null, closedAt: q.closedAt ?? null,
    decisionVersion: q.decisionVersion, decisionBookHash: q.decisionBookHash ?? null,
    queueDecisionBookHash: q.queueDecisionBook ? payloadHash(q.queueDecisionBook) : null}));
  const fills = wallet.fills.map(f => ({quoteId: f.quoteId, action: f.action, side: f.side,
    quantity: f.quantity, price: f.price, at: f.at, actualFill: f.actualFill,
    sourceCreatedAt: f.source?.createdAt, sourceKnownAt: f.source?.knownAt,
    strictTradeThrough: f.source?.strictTradeThrough, queuePositionProven: f.source?.queuePositionProven}));
  return payloadHash({bankHash: wallet.bankHash ?? null, cash: wallet.cash, realized: wallet.realized,
    quoteCount: quotes.length, quotes, fillCount: fills.length, fills});
}

function run() {
  const args = parseArgs(process.argv.slice(2));
  const runtime = path.resolve(args.runtimeDir);
  const cohortDir = path.join(runtime, 'maker-prospective-cohorts', COHORT);
  const pointer = readJson(path.join(runtime, 'maker-prospective-active-v13.json'));
  const bank = readJson(path.join(cohortDir, 'bank.json'));
  const initialState = readJson(path.join(cohortDir, 'initial-state.json'));
  const journalFile = path.join(cohortDir, 'journal.jsonl');
  const journal = readLastCompleteRecord(journalFile);
  const state = journal.row.state;
  const walletAt = state.wallet?.wallet?.lastAt;
  const walletAtMs = Date.parse(walletAt);
  const journalAtMs = Date.parse(journal.row.at);
  // The hash-chained wrapper timestamp is recorded just after the wallet
  // snapshot. Preserve the wallet clock as the analysis cutoff and reject
  // future/stale state rather than requiring millisecond identity.
  if (state.bankHash !== bank.bankHash || !Number.isFinite(walletAtMs) || !Number.isFinite(journalAtMs)
    || walletAtMs > journalAtMs || journalAtMs - walletAtMs > 5_000) fail('STATE_CLOCK_OR_BANK');
  const identity = verifyBank(bank, pointer, initialState, args.workspace);
  const sourceBooks = verifyCursor(runtime, 'public-books.jsonl', state.books);
  const sourceTape = verifyCursor(runtime, 'anonymous-trades.jsonl', state.tape);
  const wallet = state.wallet.wallet;
  const statsFile = path.join(cohortDir, 'stats.json');
  const stats = fs.existsSync(statsFile) ? readJson(statsFile) : null;
  const computed = analyzeQuoteReservationTurnoverV1({bank, state, asOf: walletAt});
  const statsSameCheckpoint = stats?.at === walletAt;
  const statsCheck = stats ? {sameBank: stats.bankHash === bank.bankHash,
    checkpointAt: stats.at ?? null, sameWalletCheckpoint: statsSameCheckpoint,
    reservedCashMatches: statsSameCheckpoint ? stats.report?.reservedCash === computed.wallet.reservedCash : null,
    fillCountMatches: statsSameCheckpoint ? stats.report?.fills === computed.wallet.paperFillEvents : null,
    realizedMatches: statsSameCheckpoint ? stats.report?.realized === computed.wallet.realized : null} : null;
  const report = {schemaVersion: 1, generatedAt: new Date().toISOString(),
    provenance: {...identity,
      stateJournal: {sequence: journal.row.sequence, at: journal.row.at, previousHash: journal.row.previous,
        recordHash: journal.row.hash, recordHashVerified: journal.recordHashVerified,
        snapshotEndByte: journal.snapshotBytes, fileBytesAtRead: journal.fileBytes,
        wholeJournalRehashed: false, walletStateAt: walletAt},
      initialState: {lastAt: initialState.wallet.wallet.lastAt, updateCount: initialState.updates,
        quoteCount: initialState.wallet.wallet.quotes.length, fillCount: initialState.wallet.wallet.fills.length,
        seedHashVerified: true},
      sourceBooks, sourceTape,
      sourceCaptureBookStateDigest: safeWalletStateDigest(wallet),
      statsCheck,
      inputReadMode: 'read-only local paper files; latest complete frozen journal checkpoint plus its embedded exact quote/fill/source bindings',
      ordersSent: 0, networkRequests: 0},
    results: computed};
  fs.writeFileSync(args.output, JSON.stringify(report, null, 2) + '\n', {flag: 'w'});
  process.stdout.write(JSON.stringify({output: args.output, at: walletAt,
    bankHash: bank.bankHash, journalSequence: journal.row.sequence,
    openBuyReservations: computed.quoteLifecycle.openBuyReservations,
    reservedCash: computed.wallet.reservedCash, paperFillEvents: computed.wallet.paperFillEvents,
    retiredBlockers: computed.quoteLifecycle.retirementBlockers,
    reportSha256: sha(fs.readFileSync(args.output))}) + '\n');
}

run();

