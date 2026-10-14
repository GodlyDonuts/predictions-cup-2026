import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {
  MARKET_EDGE_RIDGE_BANK_SOURCES_V1,
  buildFixedRidgeBankV1,
  readCaptureV1,
  verifyCaptureSourceAfterV1,
  verifyCaptureSourceV1,
} from '../research/market-edge-ridge-bank-v1.mjs';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const RUNTIME = path.join(process.env.LOCALAPPDATA ?? '', 'SigTradingCup', 'paper-runtime');
const REPORT_PATH = path.join(ROOT, 'data', 'market-edge-ridge-bank-20261013.json');
function invariant(ok, code) { if (!ok) throw Error(code); }

async function main() {
  invariant(process.env.LOCALAPPDATA, 'RIDGE_BANK_LOCALAPPDATA_MISSING');
  const writeReport = process.argv.includes('--write-report');
  const replaceLocalReport = process.argv.includes('--replace-local-report');
  invariant(!replaceLocalReport || writeReport, 'RIDGE_BANK_REPLACE_REQUIRES_WRITE');

  // Pin all three exact sources before parsing any JSON row.
  const pins = [];
  for (const spec of MARKET_EDGE_RIDGE_BANK_SOURCES_V1) {
    const file = path.join(RUNTIME, spec.file);
    pins.push(await verifyCaptureSourceV1(spec, file));
  }

  const captures = [];
  for (let i = 0; i < MARKET_EDGE_RIDGE_BANK_SOURCES_V1.length; i++) {
    captures.push(await readCaptureV1(MARKET_EDGE_RIDGE_BANK_SOURCES_V1[i], pins[i]));
  }

  // Rehash and revalidate all source identities before fitting/scoring or writing.
  const sourceIntegrity = [];
  for (let i = 0; i < MARKET_EDGE_RIDGE_BANK_SOURCES_V1.length; i++) {
    sourceIntegrity.push(await verifyCaptureSourceAfterV1(MARKET_EDGE_RIDGE_BANK_SOURCES_V1[i], pins[i]));
  }
  invariant(sourceIntegrity.length === 3 && sourceIntegrity.every(x => x.identityStable), 'RIDGE_BANK_SOURCE_BINDING');

  const bank = buildFixedRidgeBankV1(captures);
  const report = {
    ...bank,
    generatedAt: new Date().toISOString(),
    method: 'fixed ridge model; three independently segmented chronological training captures; expanding walk-forward diagnostics; final bank fit on all three; no later capture read',
    sourceIntegrity,
    captureDiagnostics: captures.map(c => ({
      id: c.capture.id, capture: c.capture, diagnostics: c.diagnostics,
      labelProvenance: c.labelProvenance,
      candidateCount: c.candidates.length,
      labeledCount: c.candidates.filter(x => Number.isFinite(x.label?.net)).length,
    })),
    noNetworkRequests: true,
    noAccountOrRuntimeServiceAccess: true,
    contestOrders: 0,
    actualFillsProven: false,
    profitabilityEstablished: false,
  };

  if (writeReport) {
    if (replaceLocalReport) invariant(fs.existsSync(REPORT_PATH), 'RIDGE_BANK_LOCAL_REPORT_MISSING_FOR_REPLACE');
    else invariant(!fs.existsSync(REPORT_PATH), 'RIDGE_BANK_REPORT_ALREADY_EXISTS');
    fs.writeFileSync(REPORT_PATH, JSON.stringify(report, null, 2) + '\n', {flag: replaceLocalReport ? 'w' : 'wx'});
  }

  console.log(JSON.stringify({
    reportPath: writeReport ? path.relative(ROOT, REPORT_PATH) : null,
    sourceCount: sourceIntegrity.length,
    perCapture: report.perCaptureReadiness.map(x => ({
      id: x.id, acceptedBookPoints: x.readiness.acceptedBookPoints,
      candidates: x.readiness.candidateCount, labels: x.readiness.labeledCount, censored: x.readiness.censoredCount,
    })),
    walkForward: report.expandingWalkForward.map(x => ({
      trainingCaptureIds: x.trainingCaptureIds, scoredCaptureId: x.scoredCaptureId,
      labels: x.scoredLabeledCount, selected: x.score.selectedCandidateCount,
      selectedMean: x.score.selected.meanPerShare,
      selectedOneTickStressMean: x.score.selected.oneTickPerLegMeanPerShare,
      unconditionalMean: x.score.unconditionalAllLabeledCandidateControl.meanPerShare,
      predictionRmse: x.score.predictionRmse,
    })),
    finalTrainingRows: report.finalModel.trainingRows,
    finalBankSha256: report.frozenBankSha256,
    modelSha256: report.modelIdentityHash,
    cutoff: report.futureTest.frozenCutoffObservedAt,
    written: writeReport,
    actualFillsProven: false,
    profitabilityEstablished: false,
  }));
}

main().catch(error => { console.error(error.stack ?? error.message); process.exitCode = 1; });

