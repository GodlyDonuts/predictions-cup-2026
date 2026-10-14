import test from 'node:test';import assert from 'node:assert/strict';import {classifyQuotePrint} from './fill-audit.mjs';
const base=Date.parse('2026-10-14T00:00:00Z'),q={marketId:'1',exchangeId:'2',tournamentId:'cup',side:'YES',action:'buy',price:'0.450000',activationAt:base+3000,expiresAt:base+10000,retired:false};
const t=(n,p)=>({marketId:'1',exchangeId:'2',tournamentId:'cup',executedAt:new Date(base+n*1000).toISOString(),yesPrice:p});
test('audit uses execution clock and exact side-relative tick thresholds',()=>{
 assert.equal(classifyQuotePrint(q,t(2,'0.440000')),'beforeActivation');assert.equal(classifyQuotePrint(q,t(10,'0.440000')),'afterWindow');assert.equal(classifyQuotePrint(q,t(4,'0.450000')),'touch');assert.equal(classifyQuotePrint(q,t(4,'0.445000')),'through');assert.equal(classifyQuotePrint(q,t(4,'0.455000')),'away');assert.equal(classifyQuotePrint({...q,side:'NO',price:'0.545000'},t(4,'0.460000')),'through');assert.equal(classifyQuotePrint({...q,cancelEffectiveAt:base+5000},t(5,'0.440000')),'afterWindow');assert.equal(classifyQuotePrint({...q,retired:true},t(4,'0.440000')),null);
});
