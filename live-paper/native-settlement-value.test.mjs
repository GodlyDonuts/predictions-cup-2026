import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import {spawnSync} from 'node:child_process';
import {nativeExecutable} from './rust-backend.mjs';
import {sha} from './native-dip-proof-v1.mjs';

const now=Date.parse('2026-10-04T20:00:00.000Z');
function fixture(){
 const input={schemaVersion:1,mode:'settlement_value_policy_v1',asOfMs:now,
  policy:{minimumEdge:'0.020000',maximumSpread:'0.050000',maximumBookAgeMs:20000,maximumResearchAgeMs:21600000,fractionalKellyBps:2500,slippageTicks:1,depthFractionBps:100,maxOrderCost:'2.000000',maxExposure:'5.000000',cashReserve:'90.000000',cumulativeBuyBudget:'10.000000',groupCaps:{national:'3.000000',race1:'2.000000'}},
  wallet:{capital:'100.000000',cash:'100.000000',cumulativeBuys:'0.000000',positions:[],reservations:[]},candidates:[]};
 const c={id:'1:YES',marketId:'1',exchangeId:'101',side:'YES',predicateHash:'a'.repeat(64),riskGroups:['national','race1'],settlesAtMs:now+30*86400000,
  research:{id:'research1',predicateHash:'a'.repeat(64),side:'YES',lower:'0.940000',upper:'0.990000',recordedAtMs:now-10000,sourceAtMs:now-12000,expiresAtMs:now+21600000,method:'subjective_range',settlementReviewed:true,flags:[],sources:[{url:'https://example.org/research',contentHash:'b'.repeat(64)}]},
  book:{requestStartedAtMs:now-1000,receivedAtMs:now-500,availableAtMs:now-100,sequence:10,sourceHash:'c'.repeat(64),representation:'direct_yes',bids:[{price:'0.845000',quantity:1000}],asks:[{price:'0.850000',quantity:1000}]},
  account:{positionsAtMs:now-5000,ordersAtMs:now-4000,knownAtMs:now-3000,complete:true,positionExcluded:false,openOrderExcluded:false}};
 input.candidates.push(c);return input;
}
let serial=0;
function run(t,input,{badHash=false,privateField=false}={}){
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'baal-settlement-'));
 t.after(()=>{assert.equal(path.dirname(path.resolve(dir)),path.resolve(os.tmpdir()));fs.rmSync(dir,{recursive:true,force:true});});
 const file=path.join(dir,'input.json'),out=path.join(dir,'out-'+(++serial)+'.json');
 if(privateField)input.candidates[0].credential='PRIVATE_TEST_SENTINEL';
 const bytes=JSON.stringify(input)+'\n';fs.writeFileSync(file,bytes);
 const child=spawnSync(nativeExecutable,['settlement-policy','--input',file,'--expected-sha',badHash?'0'.repeat(64):sha(bytes),'--out',out],{encoding:'utf8',windowsHide:true});
 return{child,out,result:child.status===0?JSON.parse(fs.readFileSync(out)):null};
}
function ok(r){assert.equal(r.child.status,0,r.child.stderr);return r.result;}

test('native settlement decisions reserve hard-limit cost and never turn scenario surplus into profit',t=>{
 const result=ok(run(t,fixture())),d=result.decisions[0];
 assert.equal(d.action,'candidate_buy');assert.equal(d.quantity,2);assert.equal(d.maximumCost,'1.710000');
 assert.deepEqual(d.scenarioSettlementSurplusRange,['0.170000','0.270000']);
 assert.deepEqual(d.conditionalOrdinaryOutcomePnl,{win:'0.290000',lose:'-1.710000'});
 assert.equal(result.proposedMaximumCost,'1.710000');assert.equal(result.realizedPnl,null);
 assert.equal(result.fills,0);assert.equal(result.fees,0);assert.equal(d.orderEligible,false);assert.equal(result.executionAuthority,false);
});
test('multiple price levels honor fractional depth and reserve the worst limit, including unused spread',t=>{
 const input=fixture(),c=input.candidates[0];c.book.bids[0].price='0.795000';c.book.asks=[{price:'0.800000',quantity:100},{price:'0.850000',quantity:200}];
 const d=ok(run(t,input)).decisions[0];assert.equal(d.quantity,2);assert.equal(d.depthWeightedCostWithCushion,'1.660000');assert.equal(d.maximumCost,'1.710000');
 input.policy.maxOrderCost='1.680000';const capped=ok(run(t,input)).decisions[0];assert.equal(capped.quantity,1);assert.equal(capped.maximumCost,'0.805000');
});
test('group, total, cash and cumulative budgets include prior held cost and pending reservations',t=>{
 const input=fixture(),second=structuredClone(input.candidates[0]);second.id='2:YES';second.marketId='2';second.exchangeId='102';input.candidates.push(second);
 input.policy.groupCaps.race1='2.000000';const result=ok(run(t,input));assert.equal(result.decisions.filter(x=>x.action==='candidate_buy').length,1);
 assert.equal(result.decisions[1].quantity,0);assert.equal(result.groupExposureAfterReservations.race1,'1.710000');
 input.wallet.reservations=[{candidateId:'1:YES',maximumCost:'1.500000'}];const reserved=ok(run(t,input));assert.equal(reserved.existingReservedCost,'1.500000');assert.equal(reserved.proposedMaximumCost,'0.000000');
 input.wallet.reservations=[];input.wallet.cumulativeBuys='9.500000';assert.equal(ok(run(t,input)).proposedMaximumCost,'0.000000');
 input.wallet.cumulativeBuys='0.000000';input.wallet.cash='90.500000';assert.equal(ok(run(t,input)).proposedMaximumCost,'0.000000');
 input.wallet.cash='100.000000';input.wallet.cumulativeBuys='1.800000';input.wallet.positions=[{candidateId:'1:YES',quantity:2,cost:'1.800000',enteredAtMs:now-10000}];assert.equal(ok(run(t,input)).proposedMaximumCost,'0.000000');
});
test('holding beyond two days follows current future value; basis does not prevent a rational loss exit',t=>{
 const input=fixture(),c=input.candidates[0];input.wallet.positions=[{candidateId:c.id,quantity:1,cost:'0.900000',enteredAtMs:now-3*86400000}];
 input.wallet.cumulativeBuys='0.900000';
 c.account.positionExcluded=true;c.book.bids=[{price:'0.700000',quantity:1000}];c.book.asks=[{price:'0.710000',quantity:1000}];
 let d=ok(run(t,input)).decisions[0];assert.equal(d.action,'hold');assert.equal(d.holdingAgeMs,3*86400000);assert.equal(d.liquidation.estimatedPnl,'-0.205000');
 assert.deepEqual(d.scenarioSettlementPnlRange,['0.040000','0.090000']);
 c.research.lower='0.500000';c.research.upper='0.600000';d=ok(run(t,input)).decisions[0];assert.equal(d.action,'reduce');assert.equal(d.quantity,1);assert.equal(d.liquidation.estimatedPnl,'-0.205000');
});
test('expired, conflicting, unknown and settled research preserves risk and refuses invented payout or entries',async t=>{
 for(const kind of ['expired','conflict','unknown','settled'])await t.test(kind,t=>{
  const input=fixture(),c=input.candidates[0];input.wallet.positions=[{candidateId:c.id,quantity:1,cost:'0.900000',enteredAtMs:now-10000}];
  input.wallet.cumulativeBuys='0.900000';
  if(kind==='expired')c.research.expiresAtMs=now;
  if(kind==='conflict')c.research.flags=['source_conflict'];
  if(kind==='unknown')c.research.recordedAtMs=now-900;
  if(kind==='settled')c.settlesAtMs=now;
  const result=ok(run(t,input)),d=result.decisions[0];assert.equal(d.action,'review_required');assert.equal(d.worstCaseHeldLoss,'0.900000');assert.equal(result.proposedMaximumCost,'0.000000');assert.equal(result.realizedPnl,null);assert.equal(d.liquidation.complete,true);
 });
});
test('fresh complete account observations must precede requests; open orders block even reductions',async t=>{
 for(const kind of ['futureAccount','oldAccount','incomplete','ownOrder','ownPosition','futureBook','slowRequest'])await t.test(kind,t=>{
  const input=fixture(),c=input.candidates[0];
  if(kind==='futureAccount')c.account.knownAtMs=now-900;
  if(kind==='oldAccount')c.account.positionsAtMs=now-20001;
  if(kind==='incomplete')c.account.complete=false;
  if(kind==='ownOrder'){c.account.openOrderExcluded=true;input.wallet.cumulativeBuys='0.900000';input.wallet.positions=[{candidateId:c.id,quantity:1,cost:'0.900000',enteredAtMs:now-10000}];}
  if(kind==='ownPosition')c.account.positionExcluded=true;
  if(kind==='futureBook')c.book.availableAtMs=now+1;
  if(kind==='slowRequest'){c.book.requestStartedAtMs=now-30000;c.research.recordedAtMs=now-40000;c.research.sourceAtMs=now-45000;}
  assert.equal(ok(run(t,input)).proposedMaximumCost,'0.000000');
 });
});
test('fractional liquidation across all levels refuses a full-position value when depth is missing',t=>{
 const input=fixture(),c=input.candidates[0];input.wallet.positions=[{candidateId:c.id,quantity:2,cost:'1.800000',enteredAtMs:now-10000}];
 input.wallet.cumulativeBuys='1.800000';
 c.book.bids=[{price:'0.845000',quantity:100}];let d=ok(run(t,input)).decisions[0];assert.equal(d.liquidation.complete,false);assert.equal(d.liquidation.estimatedPnl,null);
 c.book.bids.push({price:'0.840000',quantity:100});d=ok(run(t,input)).decisions[0];assert.equal(d.liquidation.complete,true);assert.equal(d.liquidation.estimatedPnl,'-0.125000');
});
test('declared NO diagnostics retain exact economics without implying a REST price basis',t=>{
 const input=fixture(),c=input.candidates[0];c.side='NO';c.id='1:NO';c.research.side='NO';c.book.representation='canonical_yes_complement';
 const result=ok(run(t,input));assert.equal(result.decisions[0].maximumCost,'1.710000');assert.equal(result.decisions[0].side,'NO');assert.equal(result.decisions[0].orderEligible,false);
 c.book.representation='direct_yes';assert.notEqual(run(t,input).child.status,0);
});
test('fractional Kelly reduces weak-edge sizing and allocation is deterministic under input reordering',t=>{
 const input=fixture();input.policy.maxOrderCost='5.000000';input.policy.groupCaps.race1='5.000000';input.policy.groupCaps.national='5.000000';input.policy.fractionalKellyBps=100;
 input.candidates[0].research.lower='0.900000';assert.equal(ok(run(t,input)).proposedMaximumCost,'0.000000');
 input.policy.fractionalKellyBps=2500;const second=structuredClone(input.candidates[0]);second.id='2:YES';second.marketId='2';second.exchangeId='102';input.candidates.push(second);
 const original=ok(run(t,input));input.candidates.reverse();assert.deepEqual(ok(run(t,input)).decisions,original.decisions);
});
test('malformed identities, unsafe depth, changed hashes and extra fields fail before output',async t=>{
 for(const kind of ['hash','private','duplicate','negativeDepth','tick','group','binding','accounting'])await t.test(kind,t=>{
  const input=fixture(),c=input.candidates[0];
  if(kind==='duplicate')input.candidates.push(structuredClone(c));
  if(kind==='negativeDepth')c.book.asks[0].quantity=-1;
  if(kind==='tick')c.book.asks[0].price='0.851000';
  if(kind==='group')c.riskGroups=['unknown'];
  if(kind==='binding')input.wallet.positions=[{candidateId:'missing',quantity:1,cost:'0.900000',enteredAtMs:now-10000}];
  if(kind==='accounting')input.wallet.positions=[{candidateId:c.id,quantity:1,cost:'0.900000',enteredAtMs:now-10000}];
  const r=run(t,input,{badHash:kind==='hash',privateField:kind==='private'});assert.notEqual(r.child.status,0);assert.equal(fs.existsSync(r.out),false);assert(!r.child.stderr.includes('PRIVATE_TEST_SENTINEL'));
 });
});
test('inherited group risk breaches are visible and block new exposure without erasing inventory',t=>{
 const input=fixture(),c=input.candidates[0];input.wallet.cumulativeBuys='3.600000';input.wallet.positions=[{candidateId:c.id,quantity:4,cost:'3.600000',enteredAtMs:now-10000}];
 const result=ok(run(t,input));assert.equal(result.riskBreaches.length,2);assert.equal(result.heldAcquisitionRisk,'3.600000');assert.equal(result.proposedMaximumCost,'0.000000');
});
