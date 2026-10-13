import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {
  PRESPECIFIED_CELLS,
  aggregatePrespecifiedCells,
  captureHeader,
  comparableScope,
  createMarketBatchStudy,
  frozenCaptureLines,
  identityKey,
  summarizeMarketClusters,
} from './run-multiday-passive-markout-20261013.mjs';

function member(tournamentId, marketId, exchangeId) {
  return { tournamentId, marketId, exchangeId, topic: 'tournament:' + tournamentId + ':market:' + marketId };
}

test('fixed source plan keeps October 13 last and refuses earlier holdout placement', () => {
  const plan = createMarketBatchStudy();
  assert.equal(plan.holdoutIndex, 2);
  assert.deepEqual(plan.sourceIds, ['2026-10-05', '2026-10-06', '2026-10-13']);
  assert.throws(() => createMarketBatchStudy(['2026-10-13', '2026-10-14', '2026-10-15']), /MULTIDAY_HOLDOUT_ORDER/);
});

test('scope intersection is exact and rejects exchange-id remapping', () => {
  const a = member('t', 'm1', 'x1'), b = member('t', 'm2', 'x2');
  const c = member('t', 'm1', 'x1');
  const overlap = comparableScope([
    { members: new Map([[identityKey(a), a], [identityKey(b), b]]) },
    { members: new Map([[identityKey(c), c]]) },
  ]);
  assert.equal(overlap.comparableIdentityCount, 1);
  assert.deepEqual(overlap.keys, [identityKey(a)]);
  const remapped = member('t', 'different-market', 'x1');
  assert.throws(() => comparableScope([
    { members: new Map([[identityKey(a), a]]) },
    { members: new Map([[identityKey(remapped), remapped]]) },
  ]), /MULTIDAY_EXCHANGE_ID_COLLISION/);
});

test('header scanner stops at the event kind before reading its large held-out payload', t => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'multiday-header-'));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  const file = path.join(dir, 'capture.jsonl');
  const scope = { tournamentId: 't1', marketId: 'm1', exchangeIds: ['x1'], topic: 'tournament:t1:market:m1' };
  const config = {
    format: 'sig-feed-capture', schemaVersion: 1, captureIndex: 0, kind: 'config',
    observedAt: '2026-10-13T13:00:00.000Z', topic: scope.topic, epoch: 0, scope,
  };
  const configLine = JSON.stringify(config) + '\n';
  const eventPrefix = '{"format":"sig-feed-capture","schemaVersion":1,"captureIndex":1,"kind":"event","observedAt":"2026-10-13T13:00:01.000Z","payload":';
  fs.writeFileSync(file, configLine + eventPrefix + JSON.stringify({ huge: 'not-read-'.repeat(100_000) }) + '}\n');
  const header = captureHeader(file);
  assert.equal(header.headerRecords, 1);
  assert.equal(header.headerPrefixBytes, Buffer.byteLength(configLine));
  assert.equal(header.firstObservedAt, config.observedAt);
  assert.equal(header.stoppedAtKind, 'event');
});

test('frozen reader hashes exact complete lines and rejects partial prefixes', t => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'multiday-lines-'));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  const file = path.join(dir, 'capture.jsonl');
  const bytes = Buffer.from('one\n二\n');
  fs.writeFileSync(file, bytes);
  const rows = [...frozenCaptureLines(file, bytes.length, { chunkBytes: 4, maxRecordBytes: 16 })];
  assert.deepEqual(rows.map(row => row.text), ['one', '二']);
  assert.equal(Buffer.concat(rows.map(row => row.bytes)).toString('utf8'), bytes.toString('utf8'));
  assert.throws(() => [...frozenCaptureLines(file, bytes.length - 1, { chunkBytes: 4, maxRecordBytes: 16 })], /MULTIDAY_INCOMPLETE_PREFIX/);
});

test('market-cluster uncertainty counts independent markets rather than quote volume', () => {
  const rows = [
    { marketId: 'm1', candidateQuotes: 80, strictThroughProxies: 4, executable: { samples: 2, meanExecutableNet: '0.020000', meanMidMarkout: '0.030000', oneTickEachWayStressMean: '0.010000', positiveFraction: 1 }, eventTimeDiagnostic: { samples: 2, meanExecutableNet: '0.020000', meanMidMarkout: '0.030000', positiveFraction: 1 } },
    { marketId: 'm1', candidateQuotes: 40, strictThroughProxies: 2, executable: { samples: 1, meanExecutableNet: '0.020000', meanMidMarkout: '0.030000', oneTickEachWayStressMean: '0.010000', positiveFraction: 1 }, eventTimeDiagnostic: { samples: 1, meanExecutableNet: '0.020000', meanMidMarkout: '0.030000', positiveFraction: 1 } },
    { marketId: 'm2', candidateQuotes: 20, strictThroughProxies: 1, executable: { samples: 1, meanExecutableNet: '-0.010000', meanMidMarkout: '0.000000', oneTickEachWayStressMean: '-0.020000', positiveFraction: 0 }, eventTimeDiagnostic: { samples: 1, meanExecutableNet: '-0.010000', meanMidMarkout: '0.000000', positiveFraction: 0 } },
  ];
  const result = summarizeMarketClusters(rows);
  assert.equal(result.candidateQuotes, 140);
  assert.equal(result.quoteMarketClusters, 2);
  assert.equal(result.markoutSamples, 4);
  assert.equal(result.markoutMarketClusters, 2);
  assert.equal(result.effectiveIndependentQuoteMarkets, 140 * 140 / (120 * 120 + 20 * 20));
  assert.equal(result.effectiveIndependentMarkoutMarkets, 1.6);
  assert.equal(result.executable.meanExecutableNet, '0.012500');
  assert.equal(result.executable.equalMarketMeanExecutableNet, '0.005000');
  assert.ok(result.executable.marketClusterStandardError > 0.011 && result.executable.marketClusterStandardError < 0.012);
  assert.equal(result.executable.approximate95PercentCI.length, 2);
});

test('aggregator emits every prespecified cell, including empty cells', () => {
  const bins = family => PRESPECIFIED_CELLS.find(spec => spec.family === family).bins.map(bin => ({
    bin, candidateQuotes: 0, strictThroughProxies: 0,
    eventTimeDiagnostic: { samples: 0, meanExecutableNet: null, meanMidMarkout: null, positiveFraction: null },
    executable: { samples: 0, meanExecutableNet: null, meanMidMarkout: null, oneTickEachWayStressMean: null, positiveFraction: null },
  }));
  const table = {
    fold: 'heldOut', action: 'buy', horizonMs: 1000, candidateQuotes: 1, strictThroughProxies: 1,
    eventTimeDiagnostic: { samples: 1, meanExecutableNet: '0.020000', meanMidMarkout: '0.030000', positiveFraction: 1 },
    executable: { samples: 1, meanExecutableNet: '0.020000', meanMidMarkout: '0.030000', oneTickEachWayStressMean: '0.010000', positiveFraction: 1 },
    bySpread: bins('bySpread'), byQueueAhead: bins('byQueueAhead'), byAlignedFlowTercile: bins('byAlignedFlowTercile'),
  };
  table.bySpread[0] = { ...table.bySpread[0], candidateQuotes: 1, strictThroughProxies: 1, executable: table.executable, eventTimeDiagnostic: table.eventTimeDiagnostic };
  const cells = aggregatePrespecifiedCells([{ marketId: 'm1', tables: [table] }], ['heldOut']);
  assert.equal(cells.length, 2 * 3 * PRESPECIFIED_CELLS.reduce((sum, spec) => sum + spec.bins.length, 0));
  assert.equal(cells.find(cell => cell.action === 'buy' && cell.horizonMs === 1000 && cell.cellFamily === 'overall').candidateQuotes, 1);
  assert.equal(cells.find(cell => cell.action === 'buy' && cell.horizonMs === 1000 && cell.cellFamily === 'bySpread' && cell.cell === '2 ticks').candidateQuotes, 0);
  assert.equal(cells.every(cell => Object.hasOwn(cell, 'effectiveIndependentMarkoutMarkets')), true);
});
