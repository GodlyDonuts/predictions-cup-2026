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

function activeFixture(t, name = 'limit-entry') {
  const dir=fs.mkdtempSync(path.join(os.tmpdir(),'fleet-active-'));t.after(()=>fs.rmSync(dir,{recursive:true,force:true}));
  const cohort='prospective-fixture',subdir=name==='limit-entry'?'limit-entry-cohorts':'entry-comparisons',root=path.join(dir,subdir,cohort);fs.mkdirSync(root,{recursive:true});
  const manifest={schemaVersion:1,cohort,bankHash},write=(file,value)=>fs.writeFileSync(file,JSON.stringify(value));
  write(path.join(dir,name+'-active.json'),manifest);write(path.join(root,'bank.json'),bank);
  write(path.join(root,'stats.json'),{...base().report,cohort,pid:123,sourceHash:'d'.repeat(64)});
  fs.writeFileSync(path.join(root,'pid.txt'),'123');fs.writeFileSync(path.join(root,'worker.lock'),'123');
  fs.writeFileSync(path.join(dir,'journal.jsonl'),JSON.stringify({sequence:20,previous:'',at:at(-500),event:{type:'cycle',clockAt:at(-500)},hash:'d'.repeat(64)})+'\n');
  const processes={123:{alive:true,commandLine:'node live-paper/'+name+'-watch.mjs --name '+cohort,startedAt:at(-100000)}};
  const options={runtimeDir:dir,workspaceDir:dir,collectorLockPath:path.join(dir,'bot.lock'),now,probeProcesses:()=>processes};
  return {dir,root,manifest,write,processes,options,name};
}

test('active comparison manifests add scoped workers with verified bank, source, script and cohort identities',async t=>{
  for(const name of ['entry-comparison','limit-entry']){
    const f=activeFixture(t,name),before=fs.readFileSync(path.join(f.root,'stats.json'));const result=await fleetHealth(f.options),worker=result.workers.find(x=>x.name===name);
    assert.equal(worker.status,'running');assert.equal(worker.cohort,f.manifest.cohort);assert.equal(worker.manifestBankHash,bankHash);assert.equal(worker.source.identity,'matches_source_record');
    assert.equal(result.runtimeWrites,0);assert.deepEqual(fs.readFileSync(path.join(f.root,'stats.json')),before);
    f.processes[123].commandLine='node live-paper/'+name+'-watch.mjs --name wrong';assert.equal((await fleetHealth(f.options)).workers.find(x=>x.name===name).status,'pid_conflict');
    f.processes[123].commandLine='node live-paper/'+name+'-watch.mjs --name wrong --name '+f.manifest.cohort;assert.equal((await fleetHealth(f.options)).workers.find(x=>x.name===name).status,'pid_conflict');
  }
});

test('active cohort STOP variants apply and a failed exited report remains failed after its heartbeat ages',async t=>{
  const f=activeFixture(t);
  for(const marker of [path.join(f.dir,'STOP'),path.join(f.dir,'LIMIT_ENTRY_STOP'),path.join(f.root,'STOP')]){
    fs.writeFileSync(marker,'private-stop-sentinel');const result=await fleetHealth(f.options),worker=result.workers.find(x=>x.name==='limit-entry');assert.equal(worker.status,'stop_pending');assert.equal(JSON.stringify(result).includes('private-stop-sentinel'),false);fs.unlinkSync(marker);
  }
  const report=JSON.parse(fs.readFileSync(path.join(f.root,'stats.json')));report.status='failed';report.updatedAt=at(-3600000);f.write(path.join(f.root,'stats.json'),report);f.processes[123].alive=false;
  const result=await fleetHealth(f.options);assert.equal(result.workers.find(x=>x.name==='limit-entry').status,'failed');assert.equal(result.status,'attention_required');
});

test('malformed and escaping active manifests fail closed without exporting their content',async t=>{
  const f=activeFixture(t);
  for(const manifest of [{...f.manifest,cohort:'../private-sentinel'},{...f.manifest,cohort:'con'},{...f.manifest,bankHash:'private-sentinel'},{...f.manifest,extra:'private-sentinel'},[]]){
    f.write(path.join(f.dir,'limit-entry-active.json'),manifest);const result=await fleetHealth(f.options),worker=result.workers.find(x=>x.name==='limit-entry');assert.equal(worker.status,'manifest_invalid');assert.equal(result.status,'attention_required');assert.equal(JSON.stringify(result).includes('private-sentinel'),false);
  }
  f.write(path.join(f.dir,'limit-entry-active.json'),{...f.manifest,cohort:'missing-cohort'});assert.equal((await fleetHealth(f.options)).workers.find(x=>x.name==='limit-entry').status,'manifest_invalid');
  fs.unlinkSync(path.join(f.dir,'limit-entry-active.json'));assert.equal((await fleetHealth(f.options)).workers.some(x=>x.name==='limit-entry'),false);
});

test('active manifest must pin the actual bank and report cohort, independent of a fresh heartbeat',async t=>{
  const f=activeFixture(t);f.write(path.join(f.dir,'limit-entry-active.json'),{...f.manifest,bankHash:'f'.repeat(64)});
  assert.equal((await fleetHealth(f.options)).workers.find(x=>x.name==='limit-entry').status,'lineage_mismatch');
  f.write(path.join(f.dir,'limit-entry-active.json'),f.manifest);const report=JSON.parse(fs.readFileSync(path.join(f.root,'stats.json')));report.cohort='other-cohort';f.write(path.join(f.root,'stats.json'),report);
  assert.equal((await fleetHealth(f.options)).workers.find(x=>x.name==='limit-entry').status,'lineage_mismatch');
});

test('cohort directory junction cannot redirect manifest reads outside the permitted subtree',async t=>{
  const f=activeFixture(t),outside=fs.mkdtempSync(path.join(os.tmpdir(),'fleet-outside-'));t.after(()=>fs.rmSync(outside,{recursive:true,force:true}));
  const alias=path.join(f.dir,'limit-entry-cohorts','redirected');fs.symlinkSync(outside,alias,'junction');f.write(path.join(f.dir,'limit-entry-active.json'),{...f.manifest,cohort:'redirected'});
  const result=await fleetHealth(f.options);assert.equal(result.workers.find(x=>x.name==='limit-entry').status,'manifest_invalid');assert.equal(result.runtimeWrites,0);
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

test('stream gateway health uses its capture and global lock, not the unrelated REST journal',async t=>{
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'fleet-stream-'));t.after(()=>fs.rmSync(dir,{recursive:true,force:true}));
 const cohort='stream-fixture',root=path.join(dir,'stream-book-cohorts',cohort);fs.mkdirSync(root,{recursive:true});
 const b={...bank,sourceFile:'realtime-stream.jsonl'},h=createHash('sha256').update(JSON.stringify(b)).digest('hex'),write=(p,v)=>fs.writeFileSync(p,JSON.stringify(v));
 write(path.join(dir,'stream-book-active.json'),{schemaVersion:1,cohort,bankHash:h});write(path.join(root,'bank.json'),b);
 write(path.join(root,'stats.json'),{status:'running',updatedAt:at(-1000),pid:123,cohort,bankHash:h,sourceBytes:3});
 write(path.join(dir,'realtime-session.json'),{file:b.sourceFile});fs.writeFileSync(path.join(dir,b.sourceFile),'{}\n');fs.utimesSync(path.join(dir,b.sourceFile),new Date(at(-1000)),new Date(at(-1000)));
 fs.writeFileSync(path.join(dir,'stream-book-gateway.lock'),'123');fs.writeFileSync(path.join(root,'pid.txt'),'123');
 const options={runtimeDir:dir,workspaceDir:dir,now,probeProcesses:()=>({123:{alive:true,commandLine:'node live-paper/stream-book-watch.mjs --name '+cohort,startedAt:at(-60000)}})};
 const get=async()=>(await fleetHealth(options)).workers.find(w=>w.name==='stream-book');
 assert.equal((await get()).status,'running');assert.equal((await get()).source.kind,'realtime_capture');
 write(path.join(dir,'realtime-session.json'),{file:'realtime-changed.jsonl'});assert.equal((await get()).status,'source_unavailable');
});
test('stream dip health pins its own cohort and singleton while observing the shared capture',async t=>{
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'fleet-stream-dip-'));t.after(()=>fs.rmSync(dir,{recursive:true,force:true}));
 const cohort='stream-dip-fixture',root=path.join(dir,'stream-dip-cohorts',cohort);fs.mkdirSync(root,{recursive:true});
 const b={...bank,sourceFile:'realtime-stream.jsonl'},h=createHash('sha256').update(JSON.stringify(b)).digest('hex'),write=(p,v)=>fs.writeFileSync(p,JSON.stringify(v));
 write(path.join(dir,'stream-dip-active.json'),{schemaVersion:1,cohort,bankHash:h});write(path.join(root,'bank.json'),b);
 write(path.join(root,'stats.json'),{status:'running',updatedAt:at(-1000),pid:123,cohort,bankHash:h,sourceBytes:3});
 write(path.join(dir,'realtime-session.json'),{file:b.sourceFile});fs.writeFileSync(path.join(dir,b.sourceFile),'{}\n');fs.utimesSync(path.join(dir,b.sourceFile),new Date(at(-1000)),new Date(at(-1000)));
 fs.writeFileSync(path.join(dir,'stream-dip.lock'),'123');fs.writeFileSync(path.join(root,'pid.txt'),'123');
 const options={runtimeDir:dir,workspaceDir:dir,now,probeProcesses:()=>({123:{alive:true,commandLine:'node live-paper/stream-dip-watch.mjs --name '+cohort,startedAt:at(-60000)}})};
 const get=async()=>(await fleetHealth(options)).workers.find(w=>w.name==='stream-dip');
 assert.equal((await get()).status,'running');assert.equal((await get()).source.kind,'realtime_capture');
 write(path.join(root,'stats.json'),{status:'awaiting_verified_input',updatedAt:at(-1000),pid:123,cohort,bankHash:h,sourceBytes:3});assert.equal((await get()).status,'awaiting_verified_input');
 fs.writeFileSync(path.join(dir,'STREAM_DIP_STOP'),'private-stop-provenance');assert.equal((await get()).status,'stop_pending');
});
test('corrected source and paper manifests resolve versioned paths and shared singleton locks',async t=>{
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'fleet-stream-v2-'));t.after(()=>fs.rmSync(dir,{recursive:true,force:true}));
 const write=(p,v)=>fs.writeFileSync(p,JSON.stringify(v)),b={...bank,sourceFile:'realtime-stream.jsonl'},h=createHash('sha256').update(JSON.stringify(b)).digest('hex');
 const specs=[['stream-book-v2','stream-book-v2-cohorts','stream-book-watch-v2.mjs','stream-book-gateway.lock',123],['stream-dip-v2','stream-dip-v2-cohorts','stream-dip-watch-v2.mjs','stream-dip.lock',124],['stream-recovery','stream-recovery-cohorts','stream-recovery-watch.mjs','stream-recovery.lock',125]],processes={};
 for(const [name,directory,script,lock,pid]of specs){const cohort=name+'-fixture',root=path.join(dir,directory,cohort);fs.mkdirSync(root,{recursive:true});
  write(path.join(dir,name+'-active.json'),{schemaVersion:1,cohort,bankHash:h});write(path.join(root,'bank.json'),b);write(path.join(root,'stats.json'),{status:'running',updatedAt:at(-1000),pid,cohort,bankHash:h,sourceBytes:3});
  fs.writeFileSync(path.join(dir,lock),String(pid));fs.writeFileSync(path.join(root,'pid.txt'),String(pid));processes[pid]={alive:true,commandLine:'node live-paper/'+script+' --name '+cohort,startedAt:at(-60000)};}
 write(path.join(dir,'realtime-session.json'),{file:b.sourceFile});fs.writeFileSync(path.join(dir,b.sourceFile),'{}\n');fs.utimesSync(path.join(dir,b.sourceFile),new Date(at(-1000)),new Date(at(-1000)));
 const r=await fleetHealth({runtimeDir:dir,workspaceDir:dir,now,probeProcesses:()=>processes});for(const [name]of specs){const w=r.workers.find(x=>x.name===name);assert.equal(w.status,'running');assert.equal(w.source.kind,'realtime_capture');assert.equal(w.process.identity,'matches_expected_script');}
});

test('public recovery health uses its publication hash and source clock, not REST journal sequence',async t=>{
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'fleet-public-recovery-'));t.after(()=>fs.rmSync(dir,{recursive:true,force:true}));
 const cohort='public-test',root=path.join(dir,'public-recovery-cohorts',cohort);fs.mkdirSync(root,{recursive:true});
 const write=(p,v)=>fs.writeFileSync(p,JSON.stringify(v)),b={...bank,sourceFile:'public-books.jsonl'},h=createHash('sha256').update(JSON.stringify(b)).digest('hex'),sourceHash='f'.repeat(64);
 write(path.join(dir,'public-recovery-active.json'),{schemaVersion:1,cohort,bankHash:h});write(path.join(root,'bank.json'),b);
 write(path.join(root,'stats.json'),{status:'running',updatedAt:at(-1000),pid:123,cohort,bankHash:h,sourceSequence:1,sourceHash});
 const row={schemaVersion:1,kind:'book',sequence:1,previousHash:'',sessionId:'fixture',tournamentId:'bda92870-621e-47b0-bc3c-3602c5c26f55',scopeHash:'a'.repeat(64),at:at(-1000),payload:{privateSentinel:'DO_NOT_EXPORT'},hash:sourceHash};fs.writeFileSync(path.join(dir,'public-books.jsonl'),JSON.stringify(row)+'\n');
 fs.writeFileSync(path.join(dir,'public-recovery.lock'),'123');fs.writeFileSync(path.join(root,'pid.txt'),'123');
 const options={runtimeDir:dir,workspaceDir:dir,now,probeProcesses:()=>({123:{alive:true,commandLine:'node live-paper/public-recovery-watch.mjs --name '+cohort,startedAt:at(-60000)}})};
 const get=async()=>{const r=await fleetHealth(options);assert.equal(JSON.stringify(r).includes('DO_NOT_EXPORT'),false);return r.workers.find(x=>x.name==='public-recovery');};
 let w=await get();assert.equal(w.status,'running');assert.equal(w.source.kind,'public_book_publication');assert.equal(w.source.identity,'matches_source_record');
 write(path.join(root,'stats.json'),{status:'running',updatedAt:at(-1000),pid:123,cohort,bankHash:h,sourceSequence:1,sourceHash:'b'.repeat(64)});assert.equal((await get()).status,'lineage_mismatch');
});
