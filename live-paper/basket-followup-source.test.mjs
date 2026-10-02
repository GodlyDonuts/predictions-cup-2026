import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {digest} from './model.mjs';
import {createBasketCapture, advanceBasketRecord} from './basket-watch.mjs';
import {createBasketFollowupSource} from './basket-followup-source.mjs';

const tid = 'bda92870-621e-47b0-bc3c-3602c5c26f55', base = Date.parse('2026-10-02T10:00:00.000Z');
const stamp = n => new Date(base + n).toISOString(), pairId = 'a'.repeat(64);
const universe = {tournamentId: tid, exchanges: [{marketId: '1', exchangeId: '101'}, {marketId: '2', exchangeId: '102'}]};
const definitions = [{id: pairId, tournamentId: tid, legs: [
  {marketId: '1', exchangeId: '101', predicateHash: '1'.repeat(64)},
  {marketId: '2', exchangeId: '102', predicateHash: '2'.repeat(64)}]}];
const encoded = row => JSON.stringify(row) + '\n';
function book(marketId, ms, seq = 1) {
  const bid = marketId === '1' ? .19 : .835;
  return {endpoint: '/markets/' + marketId + '/orderbook?tournamentId=' + tid + '&depth=20',
    requestStartedAt: stamp(ms - 100), receivedAt: stamp(ms), data: {contexts: [{type: 'tournament', tournament: {id: tid},
      orderbook: {exchanges: [{exchangeId: marketId === '1' ? '101' : '102', option: 'YES', asOf: {sequence: seq, at: stamp(ms - 200)},
        bids: [{price: bid, quantity: 1000}], asks: [{price: bid + .01, quantity: 1000}]}]}}]}};
}
function fixture(t) {
  const runtimeDir = fs.mkdtempSync(path.join(os.tmpdir(), 'basket-followup-source-'));
  t.after(() => fs.rmSync(runtimeDir, {recursive: true, force: true}));
  const sourceFile = path.join(runtimeDir, 'journal.jsonl'), observerFile = path.join(runtimeDir, 'basket-journal.jsonl');
  const sources = [], rows = []; let replay, bank, current = base + 11000;
  const add = (ms, event) => {
    const body = {sequence: sources.length + 1, previous: sources.at(-1)?.hash || '', at: stamp(ms), event: {clockAt: stamp(ms), ...event}, state: {fixture: true}};
    const r = {...body, hash: digest(body)}; sources.push(r); fs.appendFileSync(sourceFile, encoded(r)); return r;
  };
  const first = add(0, {type: 'started'}), records = [first];
  records.prefixHashes = [createHash('sha256').update(encoded(first)).digest('hex')];
  ({bank, state: replay} = createBasketCapture(records, definitions, stamp(1000)));
  fs.writeFileSync(observerFile, '');
  const observe = () => {
    for (const r of sources.slice(bank.sourceRecords + rows.length)) {
      const output = advanceBasketRecord(replay, r, bank), body = {sequence: rows.length + 1, previous: rows.at(-1)?.hash || digest(bank),
        sourceSequence: r.sequence, sourceHash: r.hash, stateHash: digest(output.state), observationsHash: digest(output.observations), observations: output.observations};
      const row = {...body, hash: digest(body)}; rows.push(row); fs.appendFileSync(observerFile, encoded(row)); replay = output.state;
    }
  };
  const cycle = (ms = 10000, extra = {}) => add(ms, {type: 'cycle', sourceSnapshots: [book('1', ms - 4000), book('2', ms - 2000)],
    supplementalDepth: [], accountExclusions: [], exclusionObservedAt: stamp(ms - 5000),
    basketMeasurement: {reason: 'paired_depth_measurement', marketIds: ['1', '2'], exchangeIds: ['101', '102'], receivedAt: stamp(ms - 5500), cycle: 4, every: 4}, ...extra});
  return {runtimeDir, sourceFile, observerFile, sources, rows, bank, add, observe, cycle,
    setNow(ms) { current = base + ms; }, open(overrides = {}) { return createBasketFollowupSource({runtimeDir, bank, universe, now: () => current, ...overrides}); }};
}

test('exact paired-depth origin is replayed, incrementally extended and restart deterministic', t => {
  const f = fixture(t), r = f.cycle(); f.observe(); const h = f.open(), initial = h.poll();
  assert.equal(h.status().status, 'verified'); assert.equal(initial.length, 1);
  assert.equal(initial[0].sourceSequence, r.sequence); assert.equal(initial[0].sourceHash, r.hash);
  assert.deepEqual(initial[0].sourceMeasurement, r.event.basketMeasurement);
  assert.deepEqual(initial[0].observation, f.rows[0].observations.at(-1));
  assert.deepEqual(initial[0].observation.pairReceipts, [stamp(6000), stamp(8000)]);
  const bytes = h.status().sourceBytesRead;
  initial[0].observation.basket.quantity = 999; initial[0].sourceMeasurement.reason = 'tampered';
  assert.equal(h.poll()[0].observation.basket.quantity, 10); assert.equal(h.status().sourceBytesRead, bytes);
  f.add(12000, {type: 'heartbeat', note: 'évidence'}); f.observe(); f.setNow(13000);
  const subsequent = h.poll(); assert.equal(subsequent.length, 1); assert.equal(h.status().sourceSequence, 3);
  assert.equal(h.status().sourceBytesRead - bytes, Buffer.byteLength(encoded(f.sources.at(-1))));
  const restarted = f.open(); assert.deepEqual(restarted.poll(), subsequent);
  h.close(); assert.deepEqual(h.poll(), []); assert.equal(h.status().status, 'closed');
});

test('genuine follow-up and incidental positives never seed a new measurement', t => {
  for (const reason of ['paired_depth_followup', 'protected_capacity', undefined]) {
    const f = fixture(t); f.cycle(10000, {basketMeasurement: reason ? {reason, marketIds: ['1', '2'], exchangeIds: ['101', '102'], receivedAt: stamp(4000), cycle: 4, every: 4} : undefined}); f.observe();
    assert.equal(f.rows[0].observations.at(-1).status, 'apparent_positive_basket');
    const h = f.open(); assert.deepEqual(h.poll(), []); assert.equal(h.status().status, 'verified');
  }
});

test('cached prior-cycle leg and mismatched origin pair cannot supply same-cycle proof', t => {
  const f = fixture(t);
  f.cycle(10000, {sourceSnapshots: [book('1', 6000)], basketMeasurement: undefined});
  f.cycle(15000, {sourceSnapshots: [book('2', 13000, 2)], exclusionObservedAt: stamp(7000)});
  f.observe(); f.setNow(16000);
  assert.equal(f.rows[1].observations.at(-1).status, 'apparent_positive_basket');
  assert.deepEqual(f.open().poll(), []);
  const g = fixture(t); g.cycle(10000, {basketMeasurement: {reason: 'paired_depth_measurement', marketIds: ['1', '2'], exchangeIds: ['102', '101'], receivedAt: stamp(4000), cycle: 4, every: 4}}); g.observe();
  assert.deepEqual(g.open().poll(), []);
});

test('rehashing fabricated observer contents cannot pass deterministic source replay', t => {
  const f = fixture(t); f.cycle(); f.observe();
  const forged = structuredClone(f.rows[0]); forged.observations.at(-1).basket.quantity = 99;
  forged.observationsHash = digest(forged.observations); const {hash, ...body} = forged; forged.hash = digest(body);
  fs.writeFileSync(f.observerFile, encoded(forged));
  const h = f.open(); assert.deepEqual(h.poll(), []); assert.equal(h.status().status, 'BASKET_REPLAY_MISMATCH');
  fs.writeFileSync(f.observerFile, encoded(f.rows[0])); assert.deepEqual(h.poll(), []); // Failure remains latched.
});

test('source corruption, foreign scope, frozen-code and initial-state changes fail closed', t => {
  const f = fixture(t); f.cycle(); f.observe();
  assert.throws(() => f.open({universe: {...universe, tournamentId: 'foreign'}}), /FOREIGN/);
  assert.throws(() => f.open({universe: {...universe, exchanges: [universe.exchanges[0]]}}), /FOREIGN/);
  const b = structuredClone(f.bank); b.pairs[0].initialState.counts.receipts = 1;
  assert.throws(() => f.open({bank: b}), /INITIAL_STATE/);
  const c = structuredClone(f.bank); c.codeIdentity['model.mjs'] = 'f'.repeat(64);
  assert.throws(() => f.open({bank: c}), /CODE_CHANGED/);
  const bad = structuredClone(f.sources[1]); bad.event.basketMeasurement.reason = 'paired_depth_followup';
  fs.writeFileSync(f.sourceFile, encoded(f.sources[0]) + encoded(bad));
  const h = f.open(); assert.deepEqual(h.poll(), []); assert.equal(h.status().status, 'SOURCE_INTEGRITY_FAILURE');
});

test('a coherently rehashed prefix cannot change the frozen prospective starting point', t => {
  const f = fixture(t); f.cycle(); f.observe();
  const first = structuredClone(f.sources[0]); first.event.note = 'changed';
  const {hash, ...body} = first; first.hash = digest(body);
  const next = structuredClone(f.sources[1]); next.previous = first.hash;
  const {hash: nextHash, ...nextBody} = next; next.hash = digest(nextBody);
  fs.writeFileSync(f.sourceFile, encoded(first) + encoded(next));
  const h = f.open(); assert.deepEqual(h.poll(), []); assert.equal(h.status().status, 'BASKET_SOURCE_PREFIX_CHANGED');
});

test('retained initiating row is reread even when an old interior edit accompanies a valid append', t => {
  const f = fixture(t);
  f.add(2000, {type: 'heartbeat', padding: 'x'.repeat(70000)});
  f.cycle();
  f.add(12000, {type: 'heartbeat', padding: 'y'.repeat(70000)}); f.observe(); f.setNow(13000);
  const h = f.open(); assert.equal(h.poll().length, 1);
  const old = fs.readFileSync(f.sourceFile, 'utf8');
  const start = Buffer.byteLength(encoded(f.sources[0]) + encoded(f.sources[1]));
  const original = encoded(f.sources[2]), changed = original.replace('paired_depth_measurement', 'paired_depth_measuremenX');
  assert.equal(Buffer.byteLength(changed), Buffer.byteLength(original));
  const fd = fs.openSync(f.sourceFile, 'r+'); try { fs.writeSync(fd, changed, start, 'utf8'); } finally { fs.closeSync(fd); }
  f.add(14000, {type: 'heartbeat'}); f.observe(); f.setNow(15000);
  assert.ok(old.length > 140000); assert.deepEqual(h.poll(), []); assert.equal(h.status().status, 'JOURNAL_CHANGED');
});

test('torn source or observer tails suppress every candidate, then complete append resumes', t => {
  for (const torn of ['source', 'observer']) {
    const f = fixture(t); f.cycle(); f.observe(); const h = f.open(); assert.equal(h.poll().length, 1);
    f.add(12000, {type: 'heartbeat'}); f.observe();
    const file = torn === 'source' ? f.sourceFile : f.observerFile;
    const complete = fs.readFileSync(file); fs.writeFileSync(file, complete.subarray(0, -1)); f.setNow(13000);
    assert.deepEqual(h.poll(), []); assert.equal(h.status().status, 'awaiting_complete_append');
    fs.appendFileSync(file, '\n'); assert.equal(h.poll().length, 1); assert.equal(h.status().status, 'verified');
  }
});

test('truncation, same-size rewrite and replacement invalidate prior evidence', t => {
  for (const action of ['truncate', 'rewrite', 'replace']) {
    const f = fixture(t); f.cycle(); f.observe(); const h = f.open(); assert.equal(h.poll().length, 1);
    if (action === 'truncate') fs.truncateSync(f.sourceFile, 0);
    if (action === 'rewrite') { const raw = fs.readFileSync(f.sourceFile, 'utf8'); fs.writeFileSync(f.sourceFile, raw.replace('started', 'corrupt')); }
    if (action === 'replace') { fs.renameSync(f.sourceFile, f.sourceFile + '.old'); fs.copyFileSync(f.sourceFile + '.old', f.sourceFile); }
    assert.deepEqual(h.poll(), []); assert.match(h.status().status, /JOURNAL_(TRUNCATED|CHANGED|REPLACED)/);
  }
});

test('long historical replay is bounded and observer catch-up cannot deadlock at capacity', t => {
  const f = fixture(t); f.cycle();
  for (let i = 0; i < 300; i++) f.add(12000 + i, {type: 'heartbeat'});
  f.setNow(13000); const h = f.open();
  assert.deepEqual(h.poll(), []); assert.equal(h.status().pendingRows, 128); assert.equal(h.status().status, 'awaiting_observer_backlog');
  const paused = h.status().sourceBytesRead; assert.deepEqual(h.poll(), []); assert.equal(h.status().sourceBytesRead, paused);
  f.observe(); assert.equal(h.poll().length, 1); assert.equal(h.status().pendingRows, 0); assert.equal(h.status().journalSequence, 301);
  const resumed = f.open(); assert.deepEqual(resumed.poll(), h.poll()); assert.equal(resumed.status().sourceSequence, 302);
});

test('proof retention is bounded, expires by caller clock, and future clocks cannot emit', t => {
  const f = fixture(t); f.cycle();
  for (let i = 1; i <= 70; i++) {
    const ms = 10000 + i * 1000;
    f.add(ms - 600, {type: 'account_read_outage'});
    f.cycle(ms, {sourceSnapshots: [book('1', ms - 300, i + 1), book('2', ms - 100, i + 1)], exclusionObservedAt: stamp(ms - 500),
      basketMeasurement: {reason: 'paired_depth_measurement', marketIds: ['1', '2'], exchangeIds: ['101', '102'], receivedAt: stamp(ms - 500), cycle: 4, every: 4}});
  }
  f.observe(); f.setNow(82000); const h = f.open(); assert.equal(h.poll().length, 64); assert.equal(h.status().retainedProofs, 64);
  f.setNow(700000); assert.deepEqual(h.poll(), []); assert.equal(h.status().retainedProofs, 0);
  f.setNow(600000); assert.deepEqual(h.poll(), []); assert.equal(h.status().status, 'INVALID_FOLLOWUP_CLOCK');
  const g = fixture(t); g.cycle(); g.observe(); g.setNow(7000); const early = g.open();
  assert.deepEqual(early.poll(), []); assert.equal(early.status().status, 'awaiting_source_clock');
  g.setNow(11000); assert.equal(early.poll().length, 1);
});
