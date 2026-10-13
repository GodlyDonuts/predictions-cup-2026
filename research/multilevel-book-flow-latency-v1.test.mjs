import test from 'node:test';
import assert from 'node:assert/strict';
import {
  createMultiLevelBookFlowLatencyV1,
  FLOW_ENTRY_LATENCIES_MS_V1,
} from './multilevel-book-flow-latency-v1.mjs';

const base = Date.parse('2026-10-13T05:00:00.000Z');
const at = ms => new Date(base + ms).toISOString();
const tournamentId = 'T';
const scope = { tournamentId, exchanges: [{ marketId: 'M', exchangeId: 'E' }] };
const ladders = ({ bidTick = 100, askTick = 102, bidQty = 100, askQty = 100, depth = 20 }) => ({
  bids: Array.from({ length: depth }, (_, rank) => ({ price: ((bidTick - rank) * 0.005).toFixed(3), quantity: rank === 0 ? bidQty : 100 })),
  asks: Array.from({ length: depth }, (_, rank) => ({ price: ((askTick + rank) * 0.005).toFixed(3), quantity: rank === 0 ? askQty : 100 })),
});
const book = (sourceMs, receivedMs, sequence, options = {}) => ({
  tournamentId, marketId: 'M', exchangeId: 'E', sequence: String(sequence),
  sourceAt: at(sourceMs), receivedAt: at(receivedMs),
  ...ladders(options),
});
const observe = (study, value, observedMs = Date.parse(value.receivedAt) - base + 1) =>
  study.observe(value, { observedAt: at(observedMs) });

test('fixed response latency requires a strictly later source book after local signal receipt', () => {
  const study = createMultiLevelBookFlowLatencyV1({ scope, splitAt: at(20000) });
  observe(study, book(0, 100, 1));
  observe(study, book(110, 120, 2, { bidQty: 200 })); // positive signal becomes available at receipt 120ms
  observe(study, book(119, 121, 3, { bidQty: 200 })); // later sequence, but source is before 0ms entry target
  let result = study.report();
  let cell = result.cells.find(row => row.period === 'development' && row.responseLatencyMs === 0
    && row.horizonMs === 100 && row.feature === 'ofiL1');
  assert.equal(cell.entryProxies, 0);
  observe(study, book(130, 140, 4, { bidQty: 200 })); // first eligible 0ms entry; its receipt is 140ms
  result = study.report();
  cell = result.cells.find(row => row.period === 'development' && row.responseLatencyMs === 0
    && row.horizonMs === 100 && row.feature === 'ofiL1');
  assert.ok(cell.entryProxies >= 1);
  assert.equal(cell.labeledOutcomes, 0);
  observe(study, book(239, 241, 5, { bidQty: 200 })); // 100ms from entry receipt has not elapsed in source time
  result = study.report();
  cell = result.cells.find(row => row.period === 'development' && row.responseLatencyMs === 0
    && row.horizonMs === 100 && row.feature === 'ofiL1');
  assert.equal(cell.labeledOutcomes, 0);
  observe(study, book(245, 250, 6, { bidQty: 200 }));
  result = study.report();
  cell = result.cells.find(row => row.period === 'development' && row.responseLatencyMs === 0
    && row.horizonMs === 100 && row.feature === 'ofiL1');
  assert.ok(cell.labeledOutcomes >= 1);
  assert.equal(cell.nonzeroSignals, 1);
  assert.equal(result.definition.decisionAvailability.includes('receivedAt'), true);
  assert.equal(result.definition.holding.includes('entry book local receivedAt'), true);
  assert.deepEqual(result.definition.responseLatencyMs, [0, 1000, 3000]);
});

test('1s and 3s entries use the first eligible later book and top-one shares each entry', () => {
  const study = createMultiLevelBookFlowLatencyV1({ scope, splitAt: at(20000) });
  observe(study, book(0, 100, 1));
  observe(study, book(110, 120, 2, { bidQty: 200 }));
  observe(study, book(130, 140, 3, { bidQty: 200 })); // zero-latency entry only
  observe(study, book(1120, 1130, 4, { bidQty: 200 })); // first 1s entry after receipt 120ms
  observe(study, book(3130, 3140, 5, { bidQty: 200 })); // first 3s entry after receipt 120ms
  const result = study.report();
  assert.deepEqual(FLOW_ENTRY_LATENCIES_MS_V1, [0, 1000, 3000]);
  assert.ok(result.counts.entryProxies[1000] >= 1);
  assert.ok(result.counts.entryProxies[3000] >= 1);
  for (const latency of [1000, 3000]) {
    const ofi = result.cells.find(row => row.period === 'development' && row.responseLatencyMs === latency
      && row.horizonMs === 100 && row.feature === 'ofiL1');
    assert.ok(ofi.entryProxies >= 1);
    if (latency === 1000) assert.ok(ofi.labeledOutcomes >= 1);
    else assert.equal(ofi.labeledOutcomes, 0);
  }
});

test('development/held-out split is receipt-time based and development labels stop at purged cutoff', () => {
  const study = createMultiLevelBookFlowLatencyV1({ scope, splitAt: at(10000), purgeMs: 5000 });
  observe(study, book(0, 0, 1));
  observe(study, book(4900, 5010, 2, { bidQty: 200 })); // source predates cutoff, receipt is inside purge
  let result = study.report();
  assert.ok(result.cells.some(row => row.period === 'purged' && row.responseLatencyMs === 0 && row.feature === 'ofiL1'));

  const dev = createMultiLevelBookFlowLatencyV1({ scope, splitAt: at(20000), purgeMs: 5000 });
  observe(dev, book(13800, 13800, 1));
  observe(dev, book(14000, 14000, 2, { bidQty: 200 })); // signal remains before the 15s development cutoff
  observe(dev, book(14100, 14100, 3, { bidQty: 200 })); // 0ms entry
  observe(dev, book(19100, 19100, 4, { bidQty: 200 })); // 5s mark arrives after the 15s development cutoff
  result = dev.report();
  const row = result.cells.find(cell => cell.period === 'development' && cell.responseLatencyMs === 0
    && cell.horizonMs === 5000 && cell.feature === 'ofiL1');
  assert.ok(row.censoredByReason.development_cutoff > 0);
  assert.ok(result.counts.labelAfterDevelopmentCutoff > 0);
});

test('duplicates, rollbacks, gaps, boundaries, and capture end censor pending entries/labels', () => {
  const study = createMultiLevelBookFlowLatencyV1({ scope, splitAt: at(30000) });
  observe(study, book(0, 0, 1));
  observe(study, book(10, 10, 2, { bidQty: 200 }));
  assert.equal(observe(study, book(20, 20, 2, { bidQty: 300 })), false);
  assert.equal(observe(study, book(30, 30, 1)), false);
  study.boundary('E', at(40), 'disconnect');
  observe(study, book(50, 50, 3));
  observe(study, book(60, 60, 4, { bidQty: 200 }));
  observe(study, book(10061, 10061, 5)); // over 10s observed gap censors queued work
  study.finish();
  const result = study.report();
  assert.equal(result.counts.duplicateSequences, 1);
  assert.equal(result.counts.sequenceRollbacks, 1);
  assert.equal(result.counts.boundaryReasons.disconnect, 1);
  assert.equal(result.counts.observationGaps, 1);
  assert.ok(result.counts.censoredLabels > 0);
  assert.equal(result.actualFillsProven, false);
  assert.equal(result.profitabilityEstablished, false);
});

test('receipt-time rollback cannot create a signal from reordered packets', () => {
  const study = createMultiLevelBookFlowLatencyV1({ scope, splitAt: at(30000) });
  observe(study, book(0, 100, 1));
  observe(study, book(10, 110, 2, { bidQty: 200 }));
  const reordered = book(20, 105, 3, { bidQty: 300 }); // later source sequence but older local receipt
  assert.equal(study.observe(reordered, { observedAt: at(120) }), false);
  const result = study.report();
  assert.equal(result.counts.receiptClockRollbacks, 1);
  assert.equal(result.counts.censoredLabels > 0, true);
});

test('stale and wrong-scope book points fail closed', () => {
  const study = createMultiLevelBookFlowLatencyV1({ scope, splitAt: at(30000) });
  assert.throws(() => observe(study, book(0, 4000, 1), 4001), /MLOFI_LATENCY_STALE_BOOK/);
  assert.throws(() => observe(study, { ...book(10, 10, 2), marketId: 'OTHER' }), /MLOFI_LATENCY_SCOPE_MISMATCH/);
});
