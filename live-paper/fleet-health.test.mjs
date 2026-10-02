import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {assessWorker, fleetHealth, sourceMetadata} from './fleet-health.mjs';
const now = Date.parse('2026-10-02T10:00:00.000Z'), at = delta => new Date(now + delta).toISOString();
const bank = {name: 'fixture', cutoff: at(-600000), sourceHash: 'a'.repeat(64)}, bankHash = createHash('sha256').update(JSON.stringify(bank)).digest('hex');
const base = () => ({spec: {name: 'dip', script: 'live-paper/dip-shadow.mjs', cohortArgument: 'dip'}, reportPresent: true, bankPresent: true, bank,
  report: {status: 'running', updatedAt: at(-1000), sourceSequence: 20, bankHash}, claims: [{source: 'pid_file', pid: 123, modifiedAt: at(-60000)}],
  lock: {present: true}, processes: {123: {alive: true, commandLine: 'node live-paper/dip-shadow.mjs --name dip', startedAt: at(-61000)}},
  source: {available: true, sequence: 21, at: at(-100), clocks: {20: at(-3000)}}, stops: []});
test('failed exited worker stays failed even when its heartbeat is stale and collector advances', () => {
  const b = base(); b.report = {...b.report, status: 'failed', updatedAt: at(-1800000)}; b.processes[123].alive = false;
  b.source = {available: true, sequence: 150, at: at(-100), clocks: {20: at(-1800000)}};
  const r = assessWorker(b, now); assert.equal(r.status, 'failed'); assert.equal(r.reportedStatus, 'failed');
  assert.equal(r.process.status, 'exited'); assert.equal(r.source.lagRecords, 130); assert.ok(r.reasons.includes('source_replay_lag'));
});
test('fresh running report cannot conceal an exited, reused or wrong-cohort PID', () => {
  let b = base(); b.processes[123].alive = false; assert.equal(assessWorker(b, now).status, 'exited');
  b = base(); b.processes[123].startedAt = at(-1000); assert.equal(assessWorker(b, now).process.identity, 'pid_reused_after_claim');
  b = base(); b.processes[123].commandLine = 'node live-paper/dip-shadow.mjs --name dip-scale'; assert.equal(assessWorker(b, now).status, 'pid_conflict');
});
test('script identity requires a complete path token, including quoted absolute paths', () => {
  const b = base(); b.processes[123].commandLine = 'node live-paper/dip-shadow.mjs.backup --name dip';
  assert.equal(assessWorker(b, now).status, 'pid_conflict');
  b.processes[123].commandLine = 'node other-live-paper/dip-shadow.mjs --name dip';
  assert.equal(assessWorker(b, now).process.identity, 'different_process');
  b.processes[123].commandLine = '"C:\\Program Files\\nodejs\\node.exe" "C:\\Project Space\\live-paper\\dip-shadow.mjs" --name dip';
  assert.equal(assessWorker(b, now).process.identity, 'matches_expected_script');
});

test('staged dip hosts accept only the two exact scripts and one exact cohort argument', () => {
  const b = base(); b.spec.alternateScripts = ['live-paper/dip-worker.mjs'];
  for (const script of ['dip-shadow', 'dip-worker']) {
    b.processes[123].commandLine = 'node live-paper/' + script + '.mjs --name dip';
    assert.equal(assessWorker(b, now).process.identity, 'matches_expected_script');
  }
  for (const command of ['node live-paper/dip-worker.mjs --name dip-scale',
    'node live-paper/dip-worker.mjs --name dip-scale --name dip',
    'node live-paper/dip-worker.mjs --name dip --name dip',
    'node live-paper/dip-worker.mjs.backup --name dip',
    'node other-live-paper/dip-worker.mjs --name dip']) {
    b.processes[123].commandLine = command;
    assert.equal(assessWorker(b, now).process.identity, 'different_process', command);
  }
  b.processes[123].commandLine = '"C:\\Program Files\\node.exe" "C:\\Project Space\\live-paper\\dip-worker.mjs" --name "dip"';
  assert.equal(assessWorker(b, now).process.identity, 'matches_expected_script');
});
test('worker ahead of the captured source snapshot cannot report running', () => {
  const b = base(); b.report.sourceSequence = 99; b.source.sequence = 20;
  const r = assessWorker(b, now); assert.equal(r.status, 'source_snapshot_unverified');
  assert.equal(r.reportedStatus, 'running'); assert.ok(r.reasons.includes('worker_ahead_of_source_snapshot'));
});
test('bank mismatch, conflicting PID claims and source lag are independent of collector health', () => {
  let b = base(); b.report.bankHash = 'b'.repeat(64); assert.equal(assessWorker(b, now).status, 'lineage_mismatch');
  b = base(); b.claims.push({source: 'lock', pid: 456}); assert.equal(assessWorker(b, now).status, 'pid_conflict');
  b = base(); b.source = {available: true, sequence: 30, at: at(0), clocks: {20: at(-120000)}}; assert.equal(assessWorker(b, now).status, 'source_lag');
});
test('unknown process identity and future heartbeats never become healthy; STOP text is unnecessary', () => {
  let b = base(); b.processes[123].commandLine = null; assert.equal(assessWorker(b, now).status, 'unverified');
  b = base(); b.report.updatedAt = at(1000); assert.equal(assessWorker(b, now).status, 'invalid_clock');
  b = base(); b.stops = [{present: true, provenance: 'worker_stop', modifiedAt: at(-500)}]; const r = assessWorker(b, now);
  assert.equal(r.status, 'stop_pending'); assert.deepEqual(r.stops, [{provenance: 'worker_stop', modifiedAt: at(-500)}]);
});

test('reported source hashes must match their sequence; absent passive report is safely unverified', () => {
  const b = base(); b.report.sourceHash = 'c'.repeat(64); b.source.hashes = {20: 'd'.repeat(64)};
  assert.equal(assessWorker(b, now).source.identity, 'mismatch'); assert.equal(assessWorker(b, now).status, 'lineage_mismatch');
  b.source.hashes[20] = b.report.sourceHash; assert.equal(assessWorker(b, now).source.identity, 'matches_source_record');
  const r = assessWorker({spec: {name: 'passive', script: 'live-paper/passive-watch.mjs'}, bank: {sessionId: 'fixture'}, bankPresent: true}, now);
  assert.equal(r.lineage.status, 'mismatch'); assert.equal(r.source.identity, 'unverified');
});

test('caught-up worker with a fresh heartbeat still exposes a stopped source clock', () => {
  const b = base(); b.source.sequence = 20; b.source.at = at(-120000); b.source.clocks[20] = b.source.at;
  const r = assessWorker(b, now); assert.equal(r.status, 'source_stale'); assert.equal(r.source.lagRecords, 0);
  assert.equal(r.source.sourceAgeMs, 120000); assert.equal(r.reportedStatus, 'running');
});
test('a fresh process cannot hide a missing or malformed source', () => {
  const b = base(); b.source = {available: false}; assert.equal(assessWorker(b, now).status, 'source_unavailable');
  b.source = {...base().source, malformedMetadataRows: 1}; assert.equal(assessWorker(b, now).status, 'unverified');
});
test('source scan excludes crash tail and exports metadata without deserializing private payloads', async t => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'fleet-health-')); t.after(() => fs.rmSync(dir, {recursive: true, force: true}));
  const file = path.join(dir, 'journal.jsonl');
  const row = n => JSON.stringify({sequence: n, previous: n === 1 ? '' : 'a'.repeat(64), at: at(-1000 + n), event: {account: 'private-sentinel'}, hash: 'b'.repeat(64)});
  fs.writeFileSync(file, row(1) + '\n' + row(2) + '\n' + row(3));
  const result = await sourceMetadata(file, [1]); assert.equal(result.sequence, 2); assert.equal(result.partialTailExcluded, true);
  assert.equal(result.clocks[1], at(-999)); assert.ok(!JSON.stringify(result).includes('private-sentinel'));
});
test('fleet collector performs no writes and strips raw errors, accounts, process arguments and STOP contents', async t => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'fleet-health-')); t.after(() => fs.rmSync(dir, {recursive: true, force: true}));
  fs.writeFileSync(path.join(dir, 'dip-bank.json'), JSON.stringify(bank));
  fs.writeFileSync(path.join(dir, 'dip-stats.json'), JSON.stringify({...base().report, rawAccount: 'secret-sentinel', error: 'secret-sentinel'}));
  fs.writeFileSync(path.join(dir, 'dip-pid.txt'), '123'); fs.writeFileSync(path.join(dir, 'dip-shadow.lock'), '123');
  fs.writeFileSync(path.join(dir, 'DIP_STOP'), 'secret-sentinel');
  fs.writeFileSync(path.join(dir, 'competition-stats.json'), JSON.stringify({status: 'waiting_for_signal', updatedAt: at(0), positions: ['secret-sentinel'], owner: 'secret-sentinel'}));
  const before = fs.readdirSync(dir).map(f => [f, fs.readFileSync(path.join(dir, f), 'utf8')]);
  const r = await fleetHealth({runtimeDir: dir, workspaceDir: dir, collectorLockPath: path.join(dir, 'bot.lock'), now,
    probeProcesses: () => ({123: {alive: true, commandLine: 'node live-paper/dip-shadow.mjs --name dip secret-sentinel', startedAt: at(-60000)}})});
  assert.ok(!JSON.stringify(r).includes('secret-sentinel')); assert.equal(r.runtimeWrites, 0); assert.equal(r.automaticRestarts, 0);
  assert.deepEqual(fs.readdirSync(dir).map(f => [f, fs.readFileSync(path.join(dir, f), 'utf8')]), before);
  assert.equal(r.workers.find(w => w.name === 'dip').status, 'stop_pending');
});
test('an empty runtime directory is not an initialized running fleet', async t => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'fleet-health-')); t.after(() => fs.rmSync(dir, {recursive: true, force: true}));
  const r = await fleetHealth({runtimeDir: dir, workspaceDir: dir, collectorLockPath: path.join(dir, 'bot.lock'), now, probeProcesses: () => ({})});
  assert.equal(r.status, 'not_initialized'); assert.ok(r.workers.every(w => !w.configured && w.status === 'not_initialized'));
});

test('actual STOP names and realtime schema distinguish recovered books from stream evidence', async t => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'fleet-health-')); t.after(() => fs.rmSync(dir, {recursive: true, force: true}));
  const write = (name, value) => fs.writeFileSync(path.join(dir, name + '.json'), JSON.stringify(value));
  write('fee-free-bank', bank); write('fee-free-stats', base().report);
  fs.writeFileSync(path.join(dir, 'fee-free-pid.txt'), '123'); fs.writeFileSync(path.join(dir, 'fee-free-shadow.lock'), '123');
  fs.writeFileSync(path.join(dir, 'FEE_FREE_STOP'), 'private-stop-text');
  fs.writeFileSync(path.join(dir, 'EXECUTION_STOP'), 'private-stop-text');
  write('realtime-stats', {at: at(-1000), markets: [{marketId: '1', ready: true}, {marketId: '2', ready: false}], marketStreamObserved: false,
    transport: {eventsReceived: 0, diagnostics: {knownBroadcasts: {market_batch: 0, book_dirty: 0, market_settled: 0}}}});
  const options = {runtimeDir: dir, workspaceDir: dir, collectorLockPath: path.join(dir, 'bot.lock'), now,
    probeProcesses: () => ({123: {alive: true, commandLine: 'node live-paper/fee-free-shadow.mjs', startedAt: at(-60000)}})};
  let r = await fleetHealth(options); assert.equal(r.officialContext.execution.stopPresent, true);
  assert.equal(r.workers.find(w => w.name === 'fee-free').status, 'stop_pending');
  assert.equal(r.officialContext.realtime.readyMarkets, 1); assert.equal(r.officialContext.realtime.marketStreamObserved, false);
  assert.equal(r.officialContext.realtime.eventsReceived, 0); assert.equal(r.officialContext.realtime.knownBroadcasts.market_batch, 0);
  write('realtime-stats', {at: at(1000), markets: [{ready: true}]}); r = await fleetHealth(options);
  assert.equal(r.officialContext.realtime.status, 'invalid_heartbeat'); assert.equal(r.officialContext.realtime.readyMarkets, null);
  write('realtime-stats', {at: at(-1000), markets: [{booksReady: true}]}); r = await fleetHealth(options);
  assert.equal(r.officialContext.realtime.status, 'invalid_schema'); assert.equal(r.officialContext.realtime.readyMarkets, null);
  write('realtime-stats', null); assert.equal((await fleetHealth(options)).officialContext.realtime.status, 'invalid_schema');
});

test('actual passive report shape checks current session, capture presence and nonempty market lineage', async t => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'fleet-health-')); t.after(() => fs.rmSync(dir, {recursive: true, force: true}));
  const write = (name, value) => fs.writeFileSync(path.join(dir, name + '.json'), JSON.stringify(value));
  const passiveBank = {...bank, sessionId: 'fixture', sourceFile: 'realtime-fixture.jsonl', codeIdentity: {'passive-watch.mjs': 'b'.repeat(64)}, scope: {markets: [{marketId: '1'}]}};
  const report = {sessionId: passiveBank.sessionId, sourceFile: passiveBank.sourceFile, cutoff: passiveBank.cutoff, codeIdentity: passiveBank.codeIdentity,
    markets: [{sourceHash: passiveBank.sourceHash, ready: false}], reportedAt: at(-1000), runtime: {status: 'running', pid: 123}, sourceBytes: 3};
  write('passive-bank', passiveBank); write('passive-stats', report); write('realtime-session', {file: passiveBank.sourceFile});
  fs.writeFileSync(path.join(dir, 'passive-watch.lock'), '123'); fs.writeFileSync(path.join(dir, passiveBank.sourceFile), '{}\n');
  fs.utimesSync(path.join(dir, passiveBank.sourceFile), new Date(at(-1000)), new Date(at(-1000)));
  const options = {runtimeDir: dir, workspaceDir: dir, collectorLockPath: path.join(dir, 'bot.lock'), now,
    probeProcesses: () => ({123: {alive: true, commandLine: 'node live-paper/passive-watch.mjs', startedAt: at(-60000)}})};
  const passive = async () => (await fleetHealth(options)).workers.find(w => w.name === 'passive');
  let r = await passive(); assert.equal(r.status, 'running'); assert.equal(r.lineage.status, 'reported_session_matches'); assert.equal(r.source.kind, 'realtime_capture');
  write('realtime-session', {file: 'realtime-new.jsonl'}); assert.equal((await passive()).status, 'source_unavailable');
  write('realtime-session', {file: passiveBank.sourceFile}); fs.unlinkSync(path.join(dir, passiveBank.sourceFile));
  assert.equal((await passive()).status, 'source_unavailable');
  write('passive-stats', {...report, markets: []}); assert.equal((await passive()).lineage.status, 'mismatch');
});
