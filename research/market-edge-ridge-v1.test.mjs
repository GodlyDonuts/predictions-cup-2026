import test from 'node:test';
import assert from 'node:assert/strict';
import {
  captureLifecycleCanBeAppliedV1,
  MARKET_EDGE_RIDGE_PROTOCOL,
  candidatesForSeriesV1,
  fitMarketEdgeRidgeV1,
  marketBatchUsableAfterRecoveryV1,
  predictMarketEdgeRidgeV1,
  scoreMarketEdgeRidgeV1,
  segmentSeriesGapsV1,
} from './market-edge-ridge-v1.mjs';

function points({ n = 40, marketId = 'm1', priceStep = 0.005, segmentAt = null, gapAt = null, sourceAgeMs = 0 } = {}) {
  return Array.from({ length: n }, (_, i) => {
    const mid = 0.4 + i * priceStep;
    return {
      marketId, observedMs: i * 1_000, sourceMs: i * 1_000 - sourceAgeMs, sourceAgeMs,
      segment: segmentAt !== null && i >= segmentAt ? 2 : 1,
      bid: mid - 0.005, ask: mid + 0.005,
      bidDepth1: 10 + i, askDepth1: 20 + i,
      bidDepth5: 50 + i, askDepth5: 70 + i,
      bidDepth20: 100 + i, askDepth20: 140 + i,
      provenance: { captureId: 'fixture', captureIndex: i },
    };
  }).map((p, i) => gapAt !== null && i >= gapAt ? { ...p, observedMs: p.observedMs + 11_000, sourceMs: p.sourceMs + 11_000 } : p);
}

test('protocol is frozen to one 3s-latency, 5s-hold, zero-fee, positive-net policy', () => {
  assert.equal(MARKET_EDGE_RIDGE_PROTOCOL.decisionLatencyMs, 3_000);
  assert.equal(MARKET_EDGE_RIDGE_PROTOCOL.holdingHorizonMs, 5_000);
  assert.equal(MARKET_EDGE_RIDGE_PROTOCOL.feesPerShare, 0);
  assert.equal(MARKET_EDGE_RIDGE_PROTOCOL.positivePredictionThreshold, 0);
  assert.equal(MARKET_EDGE_RIDGE_PROTOCOL.officialTick, 0.005);
});

test('market batches cannot seed features until recovery is ready and no invalidation applies', () => {
  assert.equal(marketBatchUsableAfterRecoveryV1({ready: false, invalidated: false}), false);
  assert.equal(marketBatchUsableAfterRecoveryV1({ready: true, invalidated: true}), false);
  assert.equal(marketBatchUsableAfterRecoveryV1({ready: true, invalidated: false}), true);
});

test('initial lifecycle and recovery rows are accepted after configs even before the first market batch', () => {
  assert.equal(captureLifecycleCanBeAppliedV1({configRows: 236, expectedMarkets: 237}), false);
  assert.equal(captureLifecycleCanBeAppliedV1({configRows: 237, expectedMarkets: 237}), true);
  assert.equal(captureLifecycleCanBeAppliedV1({configRows: 237, expectedMarkets: 0}), false);
});

test('side-relative lag returns and imbalance use only prior same-segment observations', () => {
  const rows = points();
  const found = candidatesForSeriesV1(rows, { includeLabels: false }).filter(x => x.decisionMs === 20_000);
  const yes = found.find(x => x.side === 'YES'), no = found.find(x => x.side === 'NO');
  assert(yes && no);
  assert(Math.abs(yes.features[1] - 0.005) < 1e-12);
  assert(Math.abs(no.features[1] + 0.005) < 1e-12);
  assert.equal(yes.features[5], -no.features[5]);
  assert.deepEqual(yes.features.slice(8), no.features.slice(8));
});

test('missing lag context and stale current source suppress candidates', () => {
  const rows = points({ n: 25 });
  rows.forEach((row, i) => { if (i < 15) row.segment = 7; });
  rows[20].sourceAgeMs = MARKET_EDGE_RIDGE_PROTOCOL.maximumSourceAgeMs + 1;
  const found = candidatesForSeriesV1(rows, { includeLabels: false });
  assert(!found.some(x => x.decisionMs === 20_000));
});

test('labels use first post-latency entry and 5s exit with correct YES and NO cashflows', () => {
  const rows = points({ n: 40, priceStep: 0.0025 });
  const found = candidatesForSeriesV1(rows).filter(x => x.decisionMs === 15_000);
  const yes = found.find(x => x.side === 'YES'), no = found.find(x => x.side === 'NO');
  assert(yes?.label && no?.label);
  const entry = rows[18], exit = rows[23];
  assert.equal(yes.label.entryObservedMs, 18_000);
  assert.equal(yes.label.exitObservedMs, 23_000);
  assert(Math.abs(yes.label.net - (exit.bid - entry.ask)) < 1e-12);
  assert(Math.abs(no.label.net - (entry.bid - exit.ask)) < 1e-12);
  assert.equal(yes.label.realizedObservedHoldingMs, 5_000);
  assert.equal(yes.label.decisionSegment, yes.label.entrySegment);
  assert.equal(yes.label.entrySegment, yes.label.exitSegment);
  assert.equal(yes.label.decisionProvenance.captureIndex, 15);
  assert.equal(yes.label.entryProvenance.captureIndex, 18);
  assert.equal(yes.label.exitProvenance.captureIndex, 23);
});

test('entry/exit labels cannot cross settlement/gap segment boundaries', () => {
  const settlement = candidatesForSeriesV1(points({ n: 40, segmentAt: 23 })).find(x => x.decisionMs === 15_000 && x.side === 'YES');
  assert.equal(settlement?.label, null);
  const gapRows = points({ n: 40, gapAt: 23 });
  const segmented = segmentSeriesGapsV1(gapRows);
  assert.equal(segmented.gapBreaks, 1);
  assert.notEqual(gapRows[22].segment, gapRows[23].segment);
  assert.equal(gapRows[23].segment, gapRows[39].segment);
  const gap = candidatesForSeriesV1(gapRows).find(x => x.decisionMs === 15_000 && x.side === 'YES');
  assert.equal(gap?.label, null);
  assert.equal(candidatesForSeriesV1(gapRows, { includeLabels: false }).some(x => x.decisionMs === 35_000), false);
});

test('segment construction rejects out-of-order per-market observations', () => {
  const rows = points({ n: 40 });
  [rows[20], rows[21]] = [rows[21], rows[20]];
  assert.throws(() => segmentSeriesGapsV1(rows), /MARKET_EDGE_SERIES_ORDER/);
  assert.throws(() => candidatesForSeriesV1(rows, { includeLabels: false }), /MARKET_EDGE_SERIES_ORDER/);
});

test('positive source age cannot shorten the five-second holding interval', () => {
  const rows = points({ n: 40, priceStep: 0.0025, sourceAgeMs: 200 });
  const sample = candidatesForSeriesV1(rows).find(x => x.decisionMs === 15_000 && x.side === 'YES');
  assert(sample?.label);
  assert(sample.label.entryObservedMs >= sample.decisionMs + MARKET_EDGE_RIDGE_PROTOCOL.decisionLatencyMs);
  assert(sample.label.exitObservedMs >= sample.label.entryObservedMs + MARKET_EDGE_RIDGE_PROTOCOL.holdingHorizonMs);
  assert(sample.label.exitSourceMs >= sample.label.entryObservedMs + MARKET_EDGE_RIDGE_PROTOCOL.holdingHorizonMs);
});

test('ridge normalization and coefficients depend only on supplied training rows', () => {
  const base = candidatesForSeriesV1(points({ n: 48 })).filter(x => x.label);
  const a = fitMarketEdgeRidgeV1(base);
  const alteredHoldout = [...base, { features: Array(MARKET_EDGE_RIDGE_PROTOCOL.featureNames.length).fill(1e6), label: { net: 0.99 } }];
  const b = fitMarketEdgeRidgeV1(base);
  assert.deepEqual(a.mean, b.mean);
  assert.deepEqual(a.scale, b.scale);
  assert.deepEqual(a.coefficients, b.coefficients);
  assert(Number.isFinite(predictMarketEdgeRidgeV1(a, base[0].features)));
  assert.notEqual(alteredHoldout.length, base.length);
});

test('selection is fixed at predicted net strictly above zero and preserves censored outcomes', () => {
  const candidates = candidatesForSeriesV1(points({ n: 48 }));
  const model = fitMarketEdgeRidgeV1(candidates.filter(x => x.label));
  const score = scoreMarketEdgeRidgeV1(model, candidates.map(x => ({ ...x, label: null })));
  assert.equal(score.candidatePredictions, candidates.length);
  assert.equal(score.selectedCandidateCount, score.predictions.positive);
  assert.equal(score.selectedWithOutcomeCount, 0);
  assert.equal(score.selectedCensoredOutcomeCount, score.selectedCandidateCount);
  assert.equal(score.selected.meanPerShare, null);
  assert.equal(score.predictionRows.length, candidates.length);
  assert.deepEqual(score.predictionRows.map(x => x.rowIndex), candidates.map((_, i) => i));
  assert(score.predictionRows.every(x => x.outcomeStatus === 'censored' && x.outcomeNetPerShare === null));
  assert.equal(score.profitabilityEstablished, false);
});

test('score reports unconditional and market-clustered controls with one-tick-per-leg stress', () => {
  const rows = [
    ...candidatesForSeriesV1(points({ n: 48, marketId: 'a' })),
    ...candidatesForSeriesV1(points({ n: 48, marketId: 'b', priceStep: -0.0025 })),
  ];
  const model = fitMarketEdgeRidgeV1(rows.filter(x => x.label));
  const score = scoreMarketEdgeRidgeV1(model, rows);
  assert.equal(score.predictionRows.length, rows.length);
  const labeledIndex = rows.findIndex(x => Number.isFinite(x.label?.net));
  assert.equal(score.predictionRows[labeledIndex].outcomeStatus, 'labeled');
  assert.equal(score.predictionRows[labeledIndex].outcomeNetPerShare, rows[labeledIndex].label.net);
  assert.equal(score.predictionRows[labeledIndex].captureId, 'fixture');
  assert.equal(score.predictionRows[labeledIndex].decisionCaptureIndex, rows[labeledIndex].decisionProvenance.captureIndex);
  assert.equal(score.predictionRows[labeledIndex].entryCaptureIndex, rows[labeledIndex].label.entryProvenance.captureIndex);
  assert.equal(score.predictionRows[labeledIndex].exitCaptureIndex, rows[labeledIndex].label.exitProvenance.captureIndex);
  assert.equal(score.unconditionalAllLabeledCandidateControl.marketClusters, 2);
  assert.equal(score.selected.oneTickPerLegStressPerShare, 0.01);
  assert.equal(score.actualFillsProven, false);
  assert.equal(score.realizedAccountPnl, null);
  assert.equal(score.profitabilityEstablished, false);
});

