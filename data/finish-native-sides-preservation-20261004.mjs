import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {nativeExecutable} from '../live-paper/rust-backend.mjs';
import {sha} from '../live-paper/native-dip-proof-v1.mjs';

const runtime=path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime');
const read=f=>JSON.parse(fs.readFileSync(f));
const activeFile=path.join(runtime,'native-quote-active.json');
const activeHash=sha(fs.readFileSync(activeFile));
const active=read(activeFile),dir=path.join(runtime,'research-cohorts',active.cohort);
const binding=read(path.join(dir,'binding.json'));
const originalExe=path.join(runtime,'native-builds',binding.binaryHash,'baal-backend.exe');
assert.equal(sha(fs.readFileSync(originalExe)),binding.binaryHash);
const sourcesFile=path.join(dir,'sides-preservation.sources.json');
const sources=read(sourcesFile),firstOutput=path.join(dir,'sides-preservation.json');
const first=read(firstOutput);
// Reuse the exact immutable prefix already replayed, not a moving live pointer.
for(const [name,source] of Object.entries(sources)){
  assert.equal(source.bytes,first.capture.sources[name].bytes);
  assert.equal(source.hash,first.capture.sources[name].hash);
}
const runs={};
for(const [label,exe] of [['original',originalExe],['recheck',nativeExecutable]]){
  const out=path.join(dir,'sides-preservation-'+label+'.json');
  assert(!fs.existsSync(out));
  const started=performance.now();
  const child=spawnSync(exe,['quote-replay','--declaration',path.join(dir,'declaration.json'),'--sources',sourcesFile,'--workers','8','--out',out],{encoding:'utf8',windowsHide:true,maxBuffer:65536});
  assert.equal(child.status,0,child.stderr);
  runs[label]={result:read(out),outputHash:sha(fs.readFileSync(out)),wallMs:performance.now()-started};
}
const prior=runs.original.result,next=runs.recheck.result;
assert.deepEqual(next.evaluation.scorecards,prior.evaluation.scorecards);
assert.deepEqual(next.evaluation.scorecards,first.evaluation.scorecards);
assert.equal(prior.capture.engineHash,binding.engineHash);
assert.equal(sha(fs.readFileSync(activeFile)),activeHash);
const experiment=read('data/native-quote-endpoints-20261004.json');
assert.equal(next.capture.engineHash,experiment.engineHash);
for(const [file,hash] of Object.entries(experiment.activeHashesUnchanged))assert.equal(sha(fs.readFileSync(path.join(runtime,file))),hash);

const groups=new Map(),fills=new Map();
for(const card of prior.evaluation.scorecards){
  const point=card.point;
  const key=JSON.stringify({cash:point.cash,realizedPnl:point.realizedPnl,liquidationPnl:point.liquidationPnl,completedTrades:point.completedTrades,positions:point.positions,fills:card.fills.length});
  if(!groups.has(key))groups.set(key,{wallets:0,cash:point.cash,realizedPnl:point.realizedPnl,liquidationPnl:point.liquidationPnl,completedTrades:point.completedTrades,fillCount:card.fills.length});
  groups.get(key).wallets++;
  for(const fill of card.fills)fills.set(sha(JSON.stringify(fill)),fill);
}
const stats=read(path.join(runtime,'stats.json')),view=read(path.join(runtime,'native-quote-status.json'));
const pids=read('data/current-runtime-pids.json');
for(const key of ['collector','paper','dashboard'])process.kill(pids[key],0);
assert.equal(stats.runtime.status,'running');assert.equal(view.status,'running');assert.equal(view.lastError,null);
// Error history is retained. Require current recovery, rather than erasing a past
// transient 503 or incorrectly treating a nonempty history as a current failure.
const errors=stats.runtime.errors.map(e=>({at:e.at,reasonHash:sha(String(e.reason)),httpStatus:/^http_[0-9]{3}$/.test(e.reason)?Number(e.reason.slice(5)):null}));
if(errors.length)assert(Date.parse(stats.runtime.lastCycleAt)>Math.max(...errors.map(e=>Date.parse(e.at))));
assert(Date.now()-Date.parse(stats.runtime.lastCycleAt)<180000);
assert(Date.now()-Date.parse(view.verifiedAt)<180000);
assert.equal(read(path.join(runtime,'competition-activation.json')).enabled,false);
assert.equal(read(path.join(runtime,'coordinated-network-budget.json')).executionOwner,'Daenerys');
const evidence={schemaVersion:1,at:new Date().toISOString(),status:'native_sides_preservation_and_current_live_recovery_verified',claimComment:5982841708,through:prior.evaluation.evaluationThrough,activeEngineHash:prior.capture.engineHash,newEngineHash:next.capture.engineHash,activeHash,originalOutputHash:runs.original.outputHash,newOutputHash:runs.recheck.outputHash,firstNewOutputHash:sha(fs.readFileSync(firstOutput)),sourcePrefixHash:sha(fs.readFileSync(sourcesFile)),exactOriginalCards:prior.evaluation.scorecards.length,allOriginalFillsExact:true,originalWalletsReset:false,activeCohortReplaced:false,originalWallMs:runs.original.wallMs,newWallMs:runs.recheck.wallMs,paperGroups:[...groups.values()],uniqueCounterfactualFillVariants:fills.size,paperFillVariants:[...fills.values()],collector:{pid:pids.collector,status:stats.runtime.status,cycles:stats.runtime.cycles,lastCycleAt:stats.runtime.lastCycleAt,errorCount:errors.length,errors,currentRecoveryVerified:true},native:{status:view.status,verifiedAt:view.verifiedAt,lastError:view.lastError,heldMarketIds:view.heldMarketIds},pids,persistentProcesses:3,additionalPersistentProcesses:0,fees:0,networkRequests:0,competitionOrders:0,profitabilityEstablished:false};
fs.writeFileSync('data/native-sides-preservation-20261004.json',JSON.stringify(evidence,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({exactOriginalCards:evidence.exactOriginalCards,allOriginalFillsExact:true,through:evidence.through,paperGroups:evidence.paperGroups,collector:evidence.collector,persistentProcesses:3,profitabilityEstablished:false}));
