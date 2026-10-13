import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createPassiveRoundtripV1, hashFixedPrefix, verifyFixedPrefix } from './passive-roundtrip-v1.mjs';

const base = Date.parse('2026-10-13T13:00:00.000Z');
const at = ms => new Date(base + ms).toISOString();
const member = (marketId = 'M', exchangeId = 'E') => ({ marketId, exchangeId });
const book = (ms, { marketId = 'M', exchangeId = 'E', bid = '0.500000', ask = '0.510000', bidQty = 17, askQty = 13, sequence = String(ms + 1) } = {}) => ({
  marketId, exchangeId, sequence, sourceAt: at(ms), requestStartedAt: at(ms), receivedAt: at(ms),
  bids: [{ price: bid, quantity: bidQty }], asks: [{ price: ask, quantity: askQty }],
});
const trade = (ms, price, { marketId = 'M', exchangeId = 'E', id = ms, quantity = 1 } = {}) => ({ marketId, exchangeId, id: String(id), sequence: Number(id), price, quantity, executedAt: at(ms) });
const create = (members = [member()]) => createPassiveRoundtripV1({ members });

test('strict-through proxy requires activation, later observation, and a strictly crossed price; queue is context only', () => {
  const replay = create();
  replay.observeBatch({ observedAt: at(0), books: [book(0, { bidQty: 29 })] });
  replay.observeBatch({ observedAt: at(2500), trades: [trade(2000, '0.495000', { id: 1 })] });
  replay.observeBatch({ observedAt: at(3500), trades: [trade(3000, '0.500000', { id: 2 })] });
  replay.observeBatch({ observedAt: at(4000), trades: [trade(3500, '0.495000', { id: 3 })] });
  const report = replay.report({ throughAt: at(4000) });
  assert.equal(report.counts.entryProxies, 1);
  assert.equal(report.perMarket[0].unresolvedInventory, 1);
  assert.equal(report.counts.boundaryPurgedExitQuotes, 1);
  assert.equal(report.completions.length, 0);
});

test('passive maker-to-maker exit earns one tick fee-free; primary stress is one tick per fill', () => {
  const replay = create();
  replay.observeBatch({ observedAt: at(0), books: [book(0, { bidQty: 9, askQty: 7 })] });
  replay.observeBatch({ observedAt: at(4000), trades: [trade(3500, '0.495000', { id: 1 })] });
  replay.observeBatch({ observedAt: at(5000), books: [book(5000, { bid: '0.500000', ask: '0.510000', sequence: '99' })] });
  replay.observeBatch({ observedAt: at(8000), trades: [trade(7500, '0.510000', { id: 2 })] });
  const report = replay.report({ throughAt: at(18000) });
  assert.equal(report.counts.entryProxies, 1);
  assert.equal(report.counts.completedExits, 1);
  assert.equal(report.completions[0].entryDisplayedQueueAhead, 9);
  assert.equal(report.completions[0].exitDisplayedQueueAhead, 0);
  assert.equal(report.aggregate.realizedGrossPnl, '0.005000');
  assert.equal(report.aggregate.realizedStressOneTickPerFillPnl, '-0.005000');
  assert.equal(report.aggregate.realizedStressOneTickTotalRoundTripPnl, '0.000000');
  assert.equal(report.aggregate.allFees, '0.000000');
});

test('market isolation prevents one market print from filling another market quote', () => {
  const replay = create([member('A', 'EA'), member('B', 'EB')]);
  replay.observeBatch({ observedAt: at(0), books: [book(0, { marketId: 'A', exchangeId: 'EA' }), book(0, { marketId: 'B', exchangeId: 'EB', bid: '0.600000', ask: '0.610000' })] });
  replay.observeBatch({ observedAt: at(4000), trades: [trade(3500, '0.595000', { marketId: 'B', exchangeId: 'EB', id: 7 })] });
  const report = replay.report({ throughAt: at(5000) });
  assert.equal(report.counts.entryProxies, 1);
  assert.equal(report.perMarket.find(row => row.marketId === 'A').entryProxies, 0);
  assert.equal(report.perMarket.find(row => row.marketId === 'B').entryProxies, 1);
});

test('duplicate trade observations count once even when present in both feed paths', () => {
  const replay = create();
  replay.observeBatch({ observedAt: at(0), books: [book(0)] });
  const print = trade(3500, '0.495000', { id: 12 });
  replay.observeBatch({ observedAt: at(4000), trades: [print, print] });
  const report = replay.report({ throughAt: at(5000) });
  assert.equal(report.counts.uniqueTradePrints, 1);
  assert.equal(report.counts.duplicateTrades, 1);
  assert.equal(report.counts.entryProxies, 1);
});

test('unresolved position uses only a later same-market executable side with visible one-share depth', () => {
  const replay = create();
  replay.observeBatch({ observedAt: at(0), books: [book(0)] });
  replay.observeBatch({ observedAt: at(4000), trades: [trade(3500, '0.495000', { id: 1 })] });
  replay.observeBatch({ observedAt: at(5000), books: [book(5000, { bid: '0.495000', ask: '0.510000', bidQty: 4, sequence: '20' })] });
  const report = replay.report({ throughAt: at(6000) });
  assert.equal(report.counts.unresolvedInventory, 1);
  assert.equal(report.counts.valuedUnresolvedInventory, 1);
  assert.equal(report.perMarket[0].openInventoryExecutableMarkPnl, '-0.005000');
  assert.equal(report.perMarket[0].openInventoryStressOneTickPerFillPnl, '-0.015000');
});

test('chronology rejects rollback and end-of-prefix purges entry or exit lifecycles that cannot mature', () => {
  const replay = create();
  replay.observeBatch({ observedAt: at(0), books: [book(0)] });
  assert.throws(() => replay.observeBatch({ observedAt: at(-1), books: [] }), /ROUNDTRIP_CLOCK_ROLLBACK/);
  const report = replay.report({ throughAt: at(5000) });
  assert.equal(report.counts.boundaryPurgedEntryPairs, 1);
  assert.equal(report.counts.expiredUnfilledEntryPairs, 0);
});

test('malformed books are rejected and malformed or conflicting trades fail closed', () => {
  const replay = create();
  replay.observeBatch({ observedAt: at(0), books: [book(0, { bid: '0.505000', ask: '0.500000' })] });
  assert.throws(() => replay.observeBatch({ observedAt: at(1), trades: [trade(0, '0.501000', { id: 1 })] }));
  const ok = create();
  ok.observeBatch({ observedAt: at(0), books: [book(0)] });
  const first = trade(3500, '0.495000', { id: 4 });
  ok.observeBatch({ observedAt: at(4000), trades: [first] });
  assert.throws(() => ok.observeBatch({ observedAt: at(4100), trades: [{ ...first, price: '0.490000' }] }), /ROUNDTRIP_TRADE_IDENTITY_CONFLICT/);
});

test('fixed source prefix binds raw bytes and tolerates append-only growth', t => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'roundtrip-prefix-'));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  const file = path.join(directory, 'capture.jsonl');
  fs.writeFileSync(file, Buffer.from('immutable-prefix\n'));
  const frozen = hashFixedPrefix(file, Buffer.byteLength('immutable-prefix\n'));
  assert.equal(verifyFixedPrefix(file, frozen, frozen).sha256, frozen.sha256);
  fs.appendFileSync(file, Buffer.from('later-append\n'));
  assert.equal(verifyFixedPrefix(file, frozen, frozen).sha256, frozen.sha256);
  fs.writeFileSync(file, Buffer.from('mutated-prefix\nmore-bytes\n'));
  assert.throws(() => verifyFixedPrefix(file, frozen, frozen), /ROUNDTRIP_PREFIX_(HASH|REPLACED)/);
});
