import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {digest} from '../live-paper/model.mjs';
import {payloadHash} from '../execution/core.mjs';
import {atomicJson} from '../execution/ledger.mjs';
import {Journal} from '../live-paper/runtime.mjs';
import {MEDIAN_PAPER_POLICY,MEDIAN_EXECUTION_POLICY,medianControllerIdentity} from '../execution/median-controller.mjs';
import {validateProductionActivation,validateProductionHandoff} from '../execution/production-activation.mjs';

// Local preparation only. Reconciliation is performed by the existing collector.
const runtime=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime');
const read=name=>JSON.parse(fs.readFileSync(path.join(runtime,name),'utf8'));
const authority=read('median-human-activation-authority-20261006.json');
const paper=read('median-paper-cohorts/baal-median-prospective-20261006-v1/bank.json');
const paperHash='fe61bfcee3e0fbc2bda5170de3fff0f3841c82d451529df2a20403a027098331';
assert.equal(digest(paper),paperHash);
assert.deepEqual(paper.policy,MEDIAN_PAPER_POLICY);
assert.equal(authority.runId,'sig-pilot-median-20261006-human');
const preparedName='median-production-prepared-20261006.json';
if(!process.argv.includes('--activate')){
 const bank={schemaVersion:1,mode:'baal_historical_median_production_v1',cutoff:new Date().toISOString(),tournamentId:paper.universe.tournamentId,universeHash:paper.universe.hash,paperBinding:{bankHash:paperHash,policyHash:digest(MEDIAN_PAPER_POLICY)},sourcePrefix:paper.featurePrefix,policy:MEDIAN_EXECUTION_POLICY,codeIdentity:medianControllerIdentity(process.cwd())};
 const bankHash=digest(bank),dir=path.join(runtime,'production-banks');fs.mkdirSync(dir,{recursive:true});
 const file=path.join(dir,bankHash+'.json');assert(!fs.existsSync(file));atomicJson(file,bank);
 atomicJson(path.join(runtime,preparedName),{schemaVersion:1,bankHash,runId:authority.runId,authorityHash:payloadHash(authority),preparedAt:bank.cutoff});
 console.log(JSON.stringify({prepared:true,bankHash,executionEnabled:false}));
}else{
 const prepared=read(preparedName),pointer=read('median-production-preflight-latest.json');
 assert.equal(prepared.authorityHash,payloadHash(authority));assert.equal(pointer.activationAuthorityHash,prepared.authorityHash);assert.equal(pointer.runId,authority.runId);
 const request=read('median-production-preflight-request.json');assert.equal(pointer.requestHash,payloadHash(request));assert.equal(request.runId,authority.runId);assert.equal(request.activationAuthorityHash,prepared.authorityHash);
 const result=read(pointer.file),r=result.receipt;assert.equal(result.runId,authority.runId);assert.equal(result.activationAuthorityHash,prepared.authorityHash);assert.equal(result.requestHash,pointer.requestHash);assert.equal(result.receiptHash,pointer.receiptHash);assert.equal(payloadHash(r),pointer.receiptHash);assert.equal(r.complete,true);
 const old=read('competition-activation.json'),oldHandoff=read('production-handoff.json');validateProductionHandoff(oldHandoff,old);
 assert.notEqual(old.runId,authority.runId);
 // No unacknowledged order or owned inventory may be left in the outgoing run.
 const stateFile=path.join(runtime,'competition-stats.json');
 assert(fs.existsSync(stateFile),'Outgoing run status must be available');
 const stats=JSON.parse(fs.readFileSync(stateFile));assert.equal(stats.runId,old.runId);assert.equal(stats.openIntent,null);assert.equal(stats.unconfirmedIntents,0);assert.equal(Object.keys(stats.positions).length,0);
 const journal=new Journal(path.join(runtime,'competition-runs',old.runId,'core-journal.jsonl'));
 assert(journal.last,'Outgoing durable account state must exist');assert(Object.values(journal.last.state.intents).every(i=>i.status==='closed'));assert.equal(Object.keys(journal.last.state.owned).length,0);
 const at=new Date().toISOString();assert(Date.parse(at)-Date.parse(r.oldestReceivedAt)<=120000);
 const h={schemaVersion:3,kind:'same_owner_explicit_human_activation',executionOwner:'Codex',ownershipTransferred:false,handoffId:old.handoffId,authorityKind:old.authorityKind,assignmentCommentId:old.assignmentCommentId,ackCommentId:old.ackCommentId,inheritedOwnershipRecord:oldHandoff,inheritedActivation:old,humanActivation:authority,reconciliation:{complete:true,tournamentId:r.tournamentId,receiptHash:pointer.receiptHash,oldestReceivedAt:r.oldestReceivedAt,newestReceivedAt:r.newestReceivedAt},reconciledAt:r.at,acknowledgedAt:at};
 const a={...old,runId:authority.runId,strategyId:'historical-median-receipt-v1',strategyBankHash:prepared.bankHash,activationAuthority:authority,handoffHash:payloadHash(h),startedAt:at,entryExpiresAt:new Date(Date.parse(at)+6*3600000).toISOString()};
 validateProductionActivation(a);validateProductionHandoff(h,a);
 const bank=read('production-banks/'+prepared.bankHash+'.json');assert.deepEqual(bank.codeIdentity,medianControllerIdentity(process.cwd()));
 for(const [name,value]of [['competition-activation.json',old],['production-handoff.json',oldHandoff]]){const archive=path.join(runtime,name+'.before-median-20261006');assert(!fs.existsSync(archive));atomicJson(archive,value);}
 atomicJson(path.join(runtime,'production-handoff.json'),h);atomicJson(path.join(runtime,'competition-activation.json'),a);
 console.log(JSON.stringify({activationConfigured:true,runId:a.runId,entryExpiresAt:a.entryExpiresAt,bankHash:prepared.bankHash,ordersSubmitted:0}));
}
