import test from 'node:test';import assert from 'node:assert/strict';
import {diagnoseQuotedDip,diagnosePriceCapture} from './bulk-dip-diagnostic.mjs';
import {digest} from './model.mjs';import {validateUniverse} from './breadth.mjs';
const at=n=>new Date(Date.parse('2026-10-03T00:00:00Z')+n).toISOString(),
 points=[1000,61000,121000,181000,241000].map(n=>({at:at(n),bid:'0.450000'})),quote={bestBid:.425,bestAsk:.43,crossed:false,marketId:'1',exchangeId:'2'};
test('diagnostic gates preserve causal reference and distinguish economic rejection from missing history',()=>{
 assert.equal(diagnoseQuotedDip(points,quote,at(300000)).reason,'depth_candidate');
 assert.equal(diagnoseQuotedDip(points,{...quote,bestBid:.45,bestAsk:.455},at(300000)).reason,'insufficient_concession');
 assert.equal(diagnoseQuotedDip(points.slice(0,4),quote,at(300000)).reason,'history_minutes');
 assert.equal(diagnoseQuotedDip(points,quote,at(400000)).reason,'history_gap');
 const future=[...points,{at:at(300001),bid:'0.100000'}];assert.deepEqual(diagnoseQuotedDip(future,quote,at(300000)),diagnoseQuotedDip(points,quote,at(300000)));
});
test('price capture diagnostics verify full original chain, partial tail and chronological cutoff counts',()=>{
 const universe={tournamentId:'fixture',exchanges:[{marketId:'1',exchangeId:'2',option:'YES'}]},canonical=validateUniverse(universe,'fixture');let sequence=0,previous='';const records=[];
 for(const n of [1000,61000,121000,181000,241000,300000]){const q={...quote,option:'YES',latestPrice:.45,spread:.005,...(n<300000?{bestBid:.45,bestAsk:.455}: {})},snapshot={tournamentId:'fixture',universeHash:canonical.hash,requestStartedAt:at(n-100),receivedAt:at(n),sourceTimestamp:null,depthAvailable:false,executable:false,quotes:[q],missingIds:[]},body={sequence:++sequence,previous,at:at(n),event:{type:'market_price_observation',snapshot},state:null},hash=digest(body);records.push({...body,hash});previous=hash;}
 const source=Buffer.from(records.map(r=>JSON.stringify(r)+'\n').join('')),report=diagnosePriceCapture(Buffer.concat([source,Buffer.from('{partial')]),universe,{cutoff:at(250000)});
 assert.equal(report.source.bytes,source.length);assert.equal(report.source.excludedPartialTailBytes,8);assert.equal(report.all.primaryGates.depth_candidate,1);assert.equal(report.segments.beforeCutoff.quotes,5);assert.equal(report.segments.afterCutoff.quotes,1);assert.equal(report.competitionOrders,0);assert.equal(report.profitabilityEstablished,false);
 assert.equal(report.all.observedFirstAt,at(1000));assert.equal(report.all.priceBatchReceipts,6);assert.equal(report.all.priceChanges.askDownAtLeastTwoCents,1);
 const gapRows=structuredClone(records),last=gapRows.at(-1);last.at=at(900000);last.event.snapshot.requestStartedAt=at(899900);last.event.snapshot.receivedAt=at(900000);const {hash,...gapBody}=last;last.hash=digest(gapBody);
 const gapReport=diagnosePriceCapture(Buffer.from(gapRows.map(r=>JSON.stringify(r)+'\n').join('')),universe);
 assert.equal(gapReport.all.priceChanges.unobservedGapComparisons,1);assert.equal(gapReport.all.priceChanges.askDownAtLeastTwoCents,0,'a change during an unobserved gap is not an observed dip');
 const changed=Buffer.from(source);changed[changed.indexOf(Buffer.from('0.45'))+3]=54;assert.throws(()=>diagnosePriceCapture(changed,universe),/DIAGNOSTIC_CHAIN/);
});
