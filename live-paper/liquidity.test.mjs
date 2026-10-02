import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {roundTripLabel} from './roundtrip-target.mjs';
import {decisionAudit} from './liquidity.mjs';
const c=JSON.parse(fs.readFileSync(new URL('./config.json',import.meta.url))),base=Date.parse('2026-10-02T02:00:00Z'),at=n=>new Date(base+n*1000).toISOString();
const row=n=>({at:at(n),marketId:'377',instrument:'9:YES',segment:'0:0',historyLength:6,book:{marketId:'377',exchangeId:'9',instrument:'9:YES',sequence:String(n),sourceAt:at(n),receivedAt:at(n),requestStartedAt:at(n),versionKey:n+':'+at(n),bids:[{price:'0.400000',quantity:10000}],asks:[{price:'0.410000',quantity:10000}]}});
test('losing censored inventory remains visible rather than becoming a zero-return closed label',()=>{
 const rows=[row(0),row(60)];rows[1].book.bids[0].price='0.350000';const result=roundTripLabel(rows,0,c,600000,{includeOutcome:true,asOf:at(60)});
 assert.equal(result.reason,'exit_censored');assert.equal(result.label,undefined);assert.equal(result.outcome.openQuantity,10);assert.equal(result.outcome.realizedPnl,0);assert.ok(result.outcome.cashDelta< -4);assert.ok(result.outcome.fees>0);assert.ok(result.outcome.liquidationPnl<-.7);
 const invalid=roundTripLabel(rows,0,c,600000,{includeOutcome:true,asOf:at(90),terminalInvalidated:true});assert.equal(invalid.outcome.liquidationPnl,null);assert.equal(invalid.outcome.referencePnl,null);
 const stale=roundTripLabel(rows,0,c,600000,{includeOutcome:true,asOf:at(600)});assert.equal(stale.outcome.liquidationPnl,null);assert.equal(stale.outcome.openQuantity,10);
});
test('partial exits retain FIFO realized losses and depth-insufficient unresolved inventory',()=>{
 const rows=[0,60,120,180].map(row);rows.forEach(r=>r.book.bids[0].quantity=100);const result=roundTripLabel(rows,0,c,60000,{includeOutcome:true,asOf:at(180)});
 assert.equal(result.reason,'exit_censored');assert.equal(result.outcome.enteredQuantity,10);assert.equal(result.outcome.soldQuantity,1);assert.equal(result.outcome.openQuantity,9);assert.ok(result.outcome.realizedPnl<0);assert.equal(result.outcome.liquidationPnl,null);
});
test('all unique decisions include warmup, unfilled and pending outcomes without manufactured fills',()=>{
 const rows=[0,60,120,180].map(row);rows[0].historyLength=5;rows.forEach(r=>r.book.asks[0].quantity=50);const m=decisionAudit(rows,c,at(180),{'377':'0:0'},{horizons:[60]})['377'],h=m.horizons[60];assert.equal(h.decisions,4);assert.equal(Object.values(h.reasons).reduce((n,x)=>n+x,0),4);assert.equal(h.reasons.decision_warmup,1);assert.equal(h.enteredDecisions,0);assert.equal(h.fees.max,0);assert.equal(m.continuousPairs,3);assert.equal(m.quoteUnchanged,3);assert.equal(m.gapMs.median,60000);
 assert.throws(()=>decisionAudit(rows,c,at(180),{}, {horizons:[60,60]}),/horizons/);
});
test('diagnostic options preserve completed labels and segment breaks suppress executable marks',()=>{
 const rows=[0,60,120,180].map(row),legacy=roundTripLabel(rows,0,c,60000),audited=roundTripLabel(rows,0,c,60000,{includeOutcome:true,asOf:at(180)});assert.deepEqual(audited.label,legacy.label);assert.equal(audited.outcome.reason,'completed');assert.equal(audited.outcome.openQuantity,0);assert.ok(Math.abs(audited.outcome.realizedPnl-audited.label.y*audited.label.quantity)<1e-12);
 rows[2].segment='1:0';const broken=roundTripLabel(rows,0,c,60000,{includeOutcome:true,asOf:at(180)});assert.equal(broken.reason,'segment_boundary');assert.equal(broken.outcome.openQuantity,10);assert.equal(broken.outcome.liquidationPnl,null);assert.throws(()=>roundTripLabel(rows,0,c,60000,{includeOutcome:true,asOf:at(100)}),/cutoff/);
});
import {auditLiquidity} from './liquidity.mjs';
import {digest} from './model.mjs';
test('terminal account outage retains held probes with unavailable marks and unchanged source rows',()=>{
 let previous='';const events=Array.from({length:11},(_,n)=>{const r=row(n*30);return {type:'cycle',clockAt:r.at,accountExclusions:[],sourceSnapshots:[{endpoint:'/markets/377/orderbook?tournamentId='+c.tournamentId,requestStartedAt:r.at,receivedAt:r.at,data:{contexts:[{type:'tournament',tournament:{id:c.tournamentId},orderbook:{exchanges:[{exchangeId:'9',option:'YES',asOf:{at:r.at,sequence:n},bids:[{price:.4,quantity:10000}],asks:[{price:.41,quantity:10000}]}]}}]}}]};});events.push({type:'account_read_outage',clockAt:at(330),reason:'connection_unavailable'});
 const raw=events.map((event,i)=>{const body={sequence:i+1,previous,at:event.clockAt,state:{},event},hash=digest(body);previous=hash;return JSON.stringify({...body,hash});}).join('\n')+'\n';const report=auditLiquidity(raw,{...c,marketIds:['377']},{horizons:[600]});const h=report.markets['377'].horizons[600];assert.equal(report.decisionCount,11);assert.equal(report.captureHealth.accountReadOutages,1);assert.equal(h.decisions,11);assert.equal(h.reasons.decision_warmup,6);assert.equal(h.openDecisions,4);assert.equal(h.missingLiquidation,4);assert.equal(h.outcomesByReason.exit_censored.liquidationPnl.count,0);assert.equal(h.outcomesByReason.exit_censored.realizedPnl.max,0);
});
