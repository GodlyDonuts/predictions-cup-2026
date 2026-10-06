import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
import {digest} from '../live-paper/model.mjs';
import {validateUniverse} from '../live-paper/breadth.mjs';
import {Journal} from '../live-paper/runtime.mjs';
import {atomicJson,canonicalJson} from '../execution/ledger.mjs';
import {payloadHash} from '../execution/core.mjs';
import {validateMedianBank} from '../execution/median-controller.mjs';
import {MEDIAN_EXECUTION_POLICY_V2,medianControllerV2Identity,validateMedianRevision} from '../execution/median-controller-v2.mjs';

const runtime=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),workspace=process.cwd();
const read=file=>JSON.parse(fs.readFileSync(file,'utf8'));
const hash=b=>createHash('sha256').update(b).digest('hex');
const activationFile=path.join(runtime,'competition-activation.json'),activationBytes=fs.readFileSync(activationFile),activation=JSON.parse(activationBytes);
const universe=validateUniverse(read(path.join(workspace,'live-paper/price-universe.json')),activation.tournamentId);
const priorBank=read(path.join(runtime,'production-banks',activation.strategyBankHash+'.json'));
validateMedianBank(priorBank,activation,universe,workspace);
const archiveDir=path.join(runtime,'production-code',activation.strategyBankHash);
for(const [relative,pin]of Object.entries(priorBank.codeIdentity)){
 const source=path.resolve(workspace,relative),dest=path.resolve(archiveDir,relative);
 assert(source.toLowerCase().startsWith((workspace+path.sep).toLowerCase()));assert(dest.toLowerCase().startsWith((archiveDir+path.sep).toLowerCase()));
 const bytes=fs.readFileSync(source);assert.equal(hash(bytes),pin);fs.mkdirSync(path.dirname(dest),{recursive:true});
 if(!fs.existsSync(dest))fs.writeFileSync(dest,bytes,{flag:'wx',mode:0o600});assert.equal(hash(fs.readFileSync(dest)),pin);
}
const preparedFile=path.join(runtime,'median-cap-revision-prepared-20261006.json');
if(!process.argv.includes('--install')){
 const next=JSON.parse(canonicalJson({schemaVersion:2,mode:'baal_historical_median_production_v2',tournamentId:priorBank.tournamentId,universeHash:priorBank.universeHash,cutoff:priorBank.cutoff,paperBinding:priorBank.paperBinding,sourcePrefix:priorBank.sourcePrefix,policy:MEDIAN_EXECUTION_POLICY_V2,codeIdentity:medianControllerV2Identity(workspace),legacyCompatibility:{priorBankHash:activation.strategyBankHash,priorPolicyHash:payloadHash(priorBank.policy),priorCodeIdentityHash:payloadHash(priorBank.codeIdentity)}}));
 const bankHash=digest(next),bankFile=path.join(runtime,'production-banks',bankHash+'.json');if(!fs.existsSync(bankFile))atomicJson(bankFile,next);assert.equal(digest(read(bankFile)),bankHash);
 atomicJson(preparedFile,{schemaVersion:1,activationHash:payloadHash(activation),priorBankHash:activation.strategyBankHash,nextBankHash:bankHash,at:new Date().toISOString()});
 console.log(JSON.stringify({prepared:true,nextBankHash:bankHash,originalActivationUnchanged:true,ordersSubmitted:0}));
}else{
 const stopFile=path.join(runtime,'COLLECTOR_STOP');assert.equal(read(stopFile).source,'median-cap-v2-controlled-install-20261006');
 assert(!fs.existsSync(path.join(process.env.LOCALAPPDATA,'SigTradingCup/bot.lock')));const dir=path.join(runtime,'competition-runs',activation.runId);assert(!fs.existsSync(path.join(dir,'execution-owner.lock')));
 const previousPid=read(path.join(workspace,'data/current-runtime-pids.json')).collector;let alive=false;try{process.kill(previousPid,0);alive=true;}catch(e){assert.equal(e.code,'ESRCH');}assert(!alive,'Prior collector must be stopped, not inferred stopped from saved reports');
 const core=new Journal(path.join(dir,'core-journal.jsonl')),proposals=new Journal(path.join(dir,'median-proposals.jsonl'));assert(core.last&&proposals.last);
 assert(Object.values(core.last.state.intents).every(i=>i.status==='closed'));
 assert(Object.values(core.last.state.control.cancellations).every(x=>x.phase==='reconciled'));
 assert(Object.values(proposals.last.state.proposals).every(p=>p.status!=='proposed'||Date.parse(p.expiresAt)<=Date.now()));
 const prepared=read(preparedFile);assert.equal(prepared.activationHash,payloadHash(activation));assert.equal(prepared.priorBankHash,activation.strategyBankHash);
 const proposalBytes=fs.readFileSync(proposals.file),revision={schemaVersion:1,kind:'same_run_median_entry_cap_revision',revisionId:'median-entry-cap-20261006-v2',ownerId:'Codex',runId:activation.runId,tournamentId:activation.tournamentId,activationHash:payloadHash(activation),priorBankHash:prepared.priorBankHash,nextBankHash:prepared.nextBankHash,recordedAt:new Date().toISOString(),coreCheckpoint:{sequence:core.sequence,hash:core.previous,stateHash:payloadHash(core.last.state)},proposalCheckpoint:{sequence:proposals.sequence,hash:proposals.previous,stateHash:payloadHash(proposals.last.state),bytes:proposalBytes.length,sha256:hash(proposalBytes)}};
 validateMedianRevision({revision,bank:read(path.join(runtime,'production-banks',prepared.nextBankHash+'.json')),priorBank,activation,universe,workspace,archiveDir});
 const revisionFile=path.join(dir,'median-entry-cap-revision.json');assert(!fs.existsSync(revisionFile));atomicJson(revisionFile,revision);assert.deepEqual(fs.readFileSync(activationFile),activationBytes);
 console.log(JSON.stringify({revisionInstalled:true,nextBankHash:prepared.nextBankHash,runId:activation.runId,entryExpiresAt:activation.entryExpiresAt,ownedLotsPreserved:Object.keys(core.last.state.owned).length,ordersSubmitted:0}));
}
