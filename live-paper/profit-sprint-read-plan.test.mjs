import test from 'node:test';import assert from 'node:assert/strict';import fs from 'node:fs';import os from 'node:os';import path from 'node:path';
import {readProfitSprint,passiveHostsCompletelyFlat,scopedRiskMarketIds,focusProfitSprintReads} from './profit-sprint-read-plan.mjs';

test('bounded operator setting expires and rejects malformed or unlimited schedules',()=>{
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'baal-sprint-')),file=path.join(dir,'COLLECTION_PROFIT_SPRINT');
 try{assert.equal(readProfitSprint(dir,1000),null);const valid={schemaVersion:1,owner:'Codex',mode:'source_bound_and_owned_risk',createdAt:new Date(1000).toISOString(),expiresAt:new Date(3000).toISOString()};fs.writeFileSync(file,JSON.stringify(valid));assert.deepEqual(readProfitSprint(dir,2000),valid);assert.equal(readProfitSprint(dir,3000),null);
  for(const change of [{owner:'agent'},{createdAt:new Date(2500).toISOString()},{expiresAt:new Date(4000000).toISOString()},{writesEnabled:true},{expiresAt:'unknown'}]){fs.writeFileSync(file,JSON.stringify({...valid,...change}));assert.throws(()=>readProfitSprint(dir,2000),/INVALID_PROFIT_SPRINT/);}
 }finally{fs.rmSync(dir,{recursive:true,force:true});}
});
test('unknown quote liability, inventory, missing exact reserve or failed host retains original reads',()=>{
 const flat={failed:false,riskMarketIds:[],wallets:{A:{positions:[],unresolvedQuotes:[],reserved:'0.000000'}}};assert.equal(passiveHostsCompletelyFlat([flat]),true);
 for(const s of [{...flat,failed:true},{...flat,riskMarketIds:['195']},{...flat,wallets:{A:{positions:[{}],unresolvedQuotes:[],reserved:'0'}}},{...flat,wallets:{A:{positions:[],unresolvedQuotes:[{}],reserved:'0'}}},{...flat,wallets:{A:{positions:[],unresolvedQuotes:[],reserved:'0.001'}}},{...flat,wallets:{A:{positions:[],unresolvedQuotes:[]}}}])assert.equal(passiveHostsCompletelyFlat([s]),false);
});
test('source-bound/owned reads keep their original order and paths; unrelated reads alone removed',()=>{
 const plan={reads:['195','161','356','209'].map(marketId=>({marketId,path:'supplementalDepth'})),managedMarketIds:['356'],entryMarketIds:['195','161']};assert.equal(focusProfitSprintReads(plan),plan);
 const focused=focusProfitSprintReads(plan,{enabled:true,sourceMarketIds:['195']});assert.deepEqual(focused.reads,[plan.reads[0],plan.reads[2]]);assert.deepEqual(focused.entryMarketIds,['195']);assert.deepEqual(focused.profitSprint.removedBookMarketIds,['161','209']);assert.equal(focused.profitSprint.executionAuthority,false);
});
test('scoped paper pending risk joins official risk while unowned research does not fast-cycle',()=>{
 const focus={ownedRiskReads:true,markets:[{marketId:'195',phase:'held'},{marketId:'356',phase:'pending_sell'},{marketId:'161',phase:'depth_required'}]};assert.deepEqual(scopedRiskMarketIds(['195'],focus),['195','356']);assert.deepEqual(scopedRiskMarketIds([], {...focus,ownedRiskReads:false}),[]);assert.throws(()=>scopedRiskMarketIds([195]),/INVALID/);
});
