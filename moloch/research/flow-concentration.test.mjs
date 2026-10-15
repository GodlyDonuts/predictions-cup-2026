import test from 'node:test';import assert from 'node:assert/strict';import {flowConcentration} from './flow-concentration.mjs';
const at=n=>new Date(Date.UTC(2026,9,15)+n*1000).toISOString(),pairs=[{marketId:'1',exchangeId:'2'}];
const row=(id,executed,received,quantity=100)=>({event:{kind:'broadcast',event:'market_batch',source:'official_sig_realtime',marketId:'1',receivedAt:at(received),payload:{trades:[{id,exchangeId:'2',sequence:Number(id),executedAt:at(executed),quantity}]}}});
test('duplicate prints do not inflate volume; independent seconds reduce concentration',()=>{
 const a=row('1',1,2),b=row('2',31,32);const one=flowConcentration([a,a],at(40),pairs)[0];assert.equal(one.observedTrades,1);assert.equal(one.largestSecondFraction,1);
 const two=flowConcentration([a,a,b],at(40),pairs)[0];assert.equal(two.observedShares,'200');assert.equal(two.largestSecondFraction,.5);assert.equal(two.effectiveActiveSeconds,2);assert.equal(two.activeThirtySecondBins,2);
});
test('late receipt, old execution and future execution cannot help the decision',()=>{
 const rows=[row('1',1,2),row('2',10,300),row('3',190,200)];const r=flowConcentration(rows,at(200),pairs)[0];assert.equal(r.observedTrades,1);assert.equal(r.observedShares,'100');
 assert.throws(()=>flowConcentration([row('4',20,10)],at(40),pairs),/TRADE/);
});
test('many trades in a single execution second remain one burst, and changed facts reject',()=>{
 const a=row('1',1,2,300),b=row('2',1,2,100);const r=flowConcentration([a,b],at(40),pairs)[0];assert.equal(r.observedTrades,2);assert.equal(r.largestSecondFraction,1);assert.equal(r.effectiveActiveSeconds,1);
 assert.throws(()=>flowConcentration([a,row('1',1,2,301)],at(40),pairs),/CONFLICT/);
});
