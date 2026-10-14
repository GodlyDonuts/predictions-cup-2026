import test from 'node:test';import assert from 'node:assert/strict';
import {evaluateDirectional} from './directional-forward.mjs';
const registration={registeredAt:'2026-10-14T19:00:00.000Z',marketId:'342',exchangeId:'1031',side:'NO',minimumPriorObservableLeadMs:3000,markoutMs:30000,fundedWallet:false,contestOrders:0};
const sample=(at,delta,quantity=1,side='NO')=>({executedAt:`2026-10-14T19:00:${at}.000Z`,exchangeId:'1031',side,quantity,liquidationDeltaPerShare:delta,generation:0});
test('forward evaluation excludes development, activation window and opposite side; preserves losses and exact weighting',()=>{
 const study={sequence:8,previous:'abc',markets:new Map([['342',{generation:0,markouts:[sample('00','0.900000'),sample('02','0.900000'),sample('04','0.005000',1),sample('05','-0.010000',3),sample('06','0.900000',1,'YES')],pending:[]}]])};
 const r=evaluateDirectional(registration,study);assert.equal(r.completedSamples,2);assert.equal(r.meanConditionalDelta,'-0.002500');assert.equal(r.volumeWeightedConditionalDelta,'-0.006250');assert.equal(r.profitabilityEstablished,false);
});
test('unmarked future entries are censored rather than zero-return wins',()=>{
 const study={sequence:1,previous:'abc',markets:new Map([['342',{generation:1,markouts:[],pending:[sample('10','0.000000')]}]])};
 const r=evaluateDirectional(registration,study);assert.equal(r.completedSamples,0);assert.equal(r.meanConditionalDelta,null);assert.equal(r.censoredSamples,1);assert.match(r.censored[0].reason,/gap/);
});
