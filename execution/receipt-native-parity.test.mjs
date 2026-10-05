import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import {spawnSync} from 'node:child_process';
import {fixtureBank,fixtureBook} from '../live-paper/probability-receipt.test.mjs';
import {createProbabilityReceiptState,advanceReceiptProbability} from '../live-paper/probability-receipt.mjs';
import {digest} from '../live-paper/model.mjs';
import {amount,money} from '../paper/fixed.mjs';
import {sha} from '../live-paper/native-dip-proof-v1.mjs';

const executable=path.resolve('backend-rust/target/receipt-strategy-20261005/release/baal-backend.exe');
function paperStep(s,b,bank){const at=b.receivedAt,clock=Date.parse(at);return advanceReceiptProbability(s,{kind:'book',at,sequence:Number(b.sequence),hash:digest(b),payload:{book:b}},bank,at,{schemaVersion:1,positionsAt:new Date(clock-1000).toISOString(),ordersAt:new Date(clock-1000).toISOString(),excludedExchangeIds:[]});}
function nativeInput(s,b,bank){
 const w=s.wallets['1000'],st=w.state.strategies.mean_reversion,r=bank.research[0],id=r.marketId+':YES',now=Date.parse(b.receivedAt),pending=st.pending[b.instrument];
 return{schemaVersion:2,mode:'probability_receipt_policy_v1',asOfMs:now,
  receiptPolicy:{takeProfit:bank.policy.takeProfit,stopLoss:bank.config.stopLoss,maximumHoldMs:bank.policy.maxHoldMs,cooldownMs:900000,orderShares:1},
  receiptState:{riskExits:pending?.exitPolicy==='risk_reprice_v2'?{[id]:pending.reason==='dip_stop'?'stop_loss':'holding_timeout'}:{},lastExitedAtMs:w.cooldowns[r.marketId]?{[id]:w.cooldowns[r.marketId]-900000}:{}},
  policy:{minimumEdge:'0.020000',maximumSpread:'0.020000',maximumBookAgeMs:20000,maximumResearchAgeMs:21600000,fractionalKellyBps:2500,slippageTicks:1,depthFractionBps:100,maxOrderCost:'1.000000',maxExposure:'10.000000',cashReserve:'90.000000',cumulativeBuyBudget:'100.000000',groupCaps:{all:'10.000000'}},
  wallet:{capital:'100.000000',cash:st.cash,cumulativeBuys:money(st.fills.filter(f=>f.action==='buy').reduce((n,f)=>n+amount(f.price)*BigInt(f.quantity),0n)),positions:Object.entries(st.positions).map(([,p])=>({candidateId:id,quantity:p.quantity,cost:p.cost,enteredAtMs:Date.parse(p.enteredAt)})),reservations:[]},
  candidates:[{id,marketId:r.marketId,exchangeId:r.exchangeId,side:r.side,predicateHash:r.predicateHash,riskGroups:['all'],settlesAtMs:Date.parse('2026-11-04T17:00:00Z'),research:{id:r.id,predicateHash:r.predicateHash,side:r.side,lower:r.lower,upper:r.upper,recordedAtMs:Date.parse(r.recordedAt),sourceAtMs:Date.parse(r.sourceAt),expiresAtMs:Date.parse(r.expiresAt),method:r.method,settlementReviewed:r.settlementReviewed,flags:r.flags,sources:r.sources},book:{requestStartedAtMs:Date.parse(b.requestStartedAt),receivedAtMs:now,availableAtMs:now,sequence:Number(b.sequence),sourceHash:digest(b),representation:'direct_yes',bids:b.bids,asks:b.asks},account:{positionsAtMs:now-1000,ordersAtMs:now-1000,knownAtMs:now-1000,complete:true,positionExcluded:false,openOrderExcluded:false}}]};
}
function run(t,input){const dir=fs.mkdtempSync(path.join(os.tmpdir(),'baal-receipt-parity-'));t.after(()=>{assert.equal(path.dirname(path.resolve(dir)),path.resolve(os.tmpdir()));fs.rmSync(dir,{recursive:true,force:true});});const file=path.join(dir,'input.json'),out=path.join(dir,'output.json'),bytes=JSON.stringify(input)+'\n';fs.writeFileSync(file,bytes);const c=spawnSync(executable,['settlement-policy','--input',file,'--expected-sha',sha(bytes),'--out',out],{encoding:'utf8',windowsHide:true});assert.equal(c.status,0,c.stderr);return JSON.parse(fs.readFileSync(out)).decisions[0];}

test('native receipt entry agrees with the frozen paper engine price, share count and zero added fee',t=>{const bank=fixtureBank(),s=createProbabilityReceiptState(bank),b=fixtureBook(1000),d=run(t,nativeInput(s,b,bank));paperStep(s,b,bank);const p=s.wallets[1000].state.strategies.mean_reversion.pending[b.instrument];assert.equal(d.action,'candidate_buy');assert.equal(d.quantity,p.quantity);assert.equal(d.hardLimitWithCushion,p.limit);assert.equal(d.maximumCost,'0.905000');});

for(const [name,delta,quotes,expected]of [
 ['below_profit',6000,{bid:.925,ask:.930},'hold'],
 ['take_profit',6000,{bid:.930,ask:.935},'reduce'],
 ['stop_loss',6000,{bid:.860,ask:.865},'reduce'],
 ['expired_research_profit',3601000,{bid:.930,ask:.935},'reduce'],
 ['holding_timeout',172810000,{bid:.895,ask:.900},'reduce']
])test('native and paper exit policy agree: '+name,t=>{const bank=fixtureBank(),s=createProbabilityReceiptState(bank);for(const n of [1000,5000])paperStep(s,fixtureBook(n),bank);const b=fixtureBook(delta,quotes),d=run(t,nativeInput(s,b,bank));paperStep(s,b,bank);const p=s.wallets[1000].state.strategies.mean_reversion.pending[b.instrument];assert.equal(d.action,expected);if(expected==='reduce'){assert.equal(p.action,'sell');assert.equal(d.hardLimitWithCushion,p.limit);}else assert.equal(p,undefined);});
