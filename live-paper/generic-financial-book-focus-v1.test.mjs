import test from 'node:test';import assert from 'node:assert/strict';import {focusGenericFinancialBooks} from './generic-financial-book-focus-v1.mjs';
const universe={tournamentId:'T',exchanges:['305','377','381','354','356','195'].map((marketId,i)=>({marketId,exchangeId:String(i+1),option:'YES'}))},status={controller:{ownerId:'Codex',tournamentId:'T',inventory:Object.fromEntries(universe.exchanges.slice(0,3).map(m=>[m.exchangeId,{marketId:m.marketId,quantity:1,proofHash:'a'.repeat(64)}]))}},plan={reads:['305','377','354','195'].map(marketId=>({marketId,path:'sourceSnapshots'}))};
test('all three attributed positions receive existing four slots, including previously missing Nevada',()=>{
 const p=focusGenericFinancialBooks(plan,status,universe);assert.equal(p.reads.length,4);assert.equal(new Set(p.reads.map(r=>r.marketId)).size,4);assert.deepEqual(p.reads.slice(0,3).map(r=>r.marketId),['305','377','381']);assert.equal(p.reads[2].path,'supplementalDepth');assert.equal(p.genericFinancialFocus.executionAuthority,false);
 assert.equal(focusGenericFinancialBooks(plan,{},universe),plan);
});
test('scope errors fail instead of adopting foreign holdings; fairness rotates when obligations exceed capacity',()=>{
 const bad=structuredClone(status);bad.controller.inventory['1'].marketId='999';assert.throws(()=>focusGenericFinancialBooks(plan,bad,universe),/SCOPE/);
 const broad={controller:{ownerId:'Codex',tournamentId:'T',inventory:Object.fromEntries(universe.exchanges.map(m=>[m.exchangeId,{marketId:m.marketId,quantity:1,proofHash:'b'.repeat(64)}]))}};
 const p=focusGenericFinancialBooks(plan,broad,universe,{'305':4,'377':5,'381':6});assert.equal(p.reads.length,4);assert.ok(p.reads.some(r=>r.marketId==='356'));assert.ok(p.reads.some(r=>r.marketId==='195'));
});

