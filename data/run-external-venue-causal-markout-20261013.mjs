import fs from 'node:fs';
import path from 'node:path';
import readline from 'node:readline';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {consumeCupRow, consumeExternalRow, createCupAudit, createExternalAudit, summarizeCoverage} from '../research/external-venue-causal-markout-v1.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const runtime = path.join(process.env.LOCALAPPDATA ?? '', 'SigTradingCup', 'paper-runtime');
const externalPath = path.join(runtime, 'external-venue-history-v1.jsonl');
const cupPath = path.join(runtime, 'public-books.jsonl');
const reviewPath = path.join(root, 'research', 'external-venue-value-20261009', 'contract-basis-review.json');
const universePath = path.join(root, 'live-paper', 'price-universe.json');
const outputPath = path.join(root, 'data', 'external-venue-causal-markout-20261013.json');
const digest = text => createHash('sha256').update(text).digest('hex');
const requireValue = (condition, message) => { if (!condition) throw new Error(message); };

function freezePrefix(file) {
  const stat = fs.statSync(file);
  requireValue(stat.isFile() && stat.size > 0, `input not a nonempty file: ${path.basename(file)}`);
  const fd = fs.openSync(file, 'r');
  try {
    const tail = Buffer.alloc(1);
    fs.readSync(fd, tail, 0, 1, stat.size - 1);
    requireValue(tail[0] === 0x0a, `captured prefix is not a complete JSONL row: ${path.basename(file)}`);
  } finally { fs.closeSync(fd); }
  return {pathLabel: path.basename(file), prefixBytes: stat.size, observedSizeBytes: stat.size,
    mtimeAtFreeze: stat.mtime.toISOString()};
}

async function scanFrozenPrefix(file, snapshot, consume) {
  const hash = createHash('sha256');
  const stream = fs.createReadStream(file, {start: 0, end: snapshot.prefixBytes - 1});
  let bytesRead = 0, rows = 0;
  stream.on('data', chunk => { bytesRead += chunk.length; hash.update(chunk); });
  const lines = readline.createInterface({input: stream, crlfDelay: Infinity});
  for await (const line of lines) {
    requireValue(line.length > 0, `blank JSONL row in ${snapshot.pathLabel}`);
    let record;
    try { record = JSON.parse(line); } catch { throw new Error(`invalid JSONL row ${rows + 1} in ${snapshot.pathLabel}`); }
    consume(record);
    rows++;
  }
  requireValue(bytesRead === snapshot.prefixBytes, `frozen prefix changed or was truncated: ${snapshot.pathLabel}`);
  requireValue(fs.statSync(file).size >= snapshot.prefixBytes, `frozen prefix truncated during scan: ${snapshot.pathLabel}`);
  return {...snapshot, sha256: hash.digest('hex'), recordsRead: rows};
}

async function main() {
  requireValue(process.env.LOCALAPPDATA, 'LOCALAPPDATA is unavailable; refusing to guess a data path');
  const external = createExternalAudit(), cup = createCupAudit();
  const contractReviewRaw = fs.readFileSync(reviewPath, 'utf8');
  const contractReview = JSON.parse(contractReviewRaw);
  requireValue(contractReview.contractEquivalenceEstablished !== true || contractReview.tiePolicyEquivalenceVerified !== true,
    'Contract review now passes the payout gate; this blocker-only runner must not be used to claim a negative result.');
  const universeRaw = fs.readFileSync(universePath, 'utf8');
  const universe = JSON.parse(universeRaw);
  const selected = universe.exchanges.filter(row => row.marketId === '386' && row.exchangeId === '1075');
  requireValue(selected.length === 1, 'expected one static Cup mapping for market 386 / exchange 1075');
  const target = {tournamentId: universe.tournamentId, ...selected[0]};
  const externalSnapshot = freezePrefix(externalPath);
  const cupSnapshot = freezePrefix(cupPath);
  const [externalInput, cupInput] = await Promise.all([
    scanFrozenPrefix(externalPath, externalSnapshot, row => consumeExternalRow(external, row)),
    scanFrozenPrefix(cupPath, cupSnapshot, row => consumeCupRow(cup, row, target))
  ]);
  const coverage = summarizeCoverage({external, cup, contractReview});
  const report = {
    schemaVersion: 1,
    study: 'External venue capture freshness and subsequent Cup executable-book coverage audit',
    generatedAt: new Date().toISOString(),
    result: 'blocked_contract_semantics_unresolved',
    inputs: {externalJournal: {...externalInput, integrity: 'full frozen-prefix SHA-256, sequential journal chain, and embedded response hashes verified'},
      cupPublicBooks: {...cupInput, integrity: 'full frozen-prefix SHA-256 and canonical sequential public-book chain verified'},
      contractReview: {path: 'research/external-venue-value-20261009/contract-basis-review.json', sha256: digest(contractReviewRaw),
        contractEquivalenceEstablished: contractReview.contractEquivalenceEstablished,
        tiePolicyEquivalenceVerified: contractReview.tiePolicyEquivalenceVerified,
        contestPredicate: contractReview.publicContestFacts?.contestPredicate ?? null,
        exceptionExamples: (contractReview.exceptionExamples ?? []).map(row => ({id: row.id,
          predicatesAgree: row.predicatesAgree, contestNoPayout: row.contestNoPayout, externalNoPayout: row.externalNoPayout}))},
      staticCupMapping: {path: 'live-paper/price-universe.json', sha256: digest(universeRaw), marketId: target.marketId,
        exchangeId: target.exchangeId, tournamentId: target.tournamentId}},
    externalVenueIdentity: {question: external.identity?.question ?? null, conditionId: external.identity?.conditionId ?? null,
      conditionMapsToCapturedNoToken: external.identity?.noTokenId != null && external.noTokenMismatches === 0,
      exactCupPayoutPredicateEstablished: false,
      semanticEvidence: {questionAsksCandidateWinnerButDescriptionSpecifiesMarginMarket: coverage.external.descriptionSaysMarginWhileQuestionAsksWinner,
        differingNoPayoutExceptions: (contractReview.exceptionExamples ?? []).filter(row => row.predicatesAgree === false).map(row => row.id),
        contestRuleRepresentation: contractReview.publicContestFacts?.representation ?? null}},
    coverage,
    controls: {marketDataNetworkRequests: 0, contestApiClients: 0, accountStateRead: false, contestOrders: 0,
      runtimeFilesChanged: false, feeAssumption: '0; no fee or strategy calculation was run'},
    conclusion: 'Do not use the external probability as a Cup price or signal: the captured venue condition is internally stable, but payout and tie-rule equivalence to the Cup contract are unverified and the existing review documents NO-payout counterexamples. Coverage and clock metrics are descriptive only; no markout, backtest, fill, or P&L result is produced.'
  };
  fs.writeFileSync(outputPath, `${JSON.stringify(report, null, 2)}\n`, 'utf8');
  console.log(JSON.stringify({result: report.result, externalRows: external.rows, targetBooks: cup.targetBooks.length,
    laterBooks: coverage.joins.laterBookAfterLatency, markout: coverage.markout.status,
    externalPrefixSha256: externalInput.sha256, cupPrefixSha256: cupInput.sha256,
    report: path.relative(root, outputPath)}));
}

main().catch(error => { console.error(error.message); process.exitCode = 1; });

