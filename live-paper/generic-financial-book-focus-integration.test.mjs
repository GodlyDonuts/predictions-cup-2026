import fs from 'node:fs';import vm from 'node:vm';import test from 'node:test';import assert from 'node:assert/strict';import {focusGenericFinancialBooks} from './generic-financial-book-focus-v1.mjs';
test('actual runner applies generic attributed inventory after optional read overlays',()=>{
 const source=fs.readFileSync(new URL('./runner.mjs',import.meta.url),'utf8'),start=source.indexOf('      planned=focusGenericFinancialBooks('),end=source.indexOf('\n',start);assert.ok(start>source.indexOf('planned=focusProfitSprintReads'));
 const universe={tournamentId:'T',exchanges:['305','377','381','354','195'].map((marketId,i)=>({marketId,exchangeId:String(i+1),option:'YES'}))},inventory=Object.fromEntries(universe.exchanges.slice(0,3).map(m=>[m.exchangeId,{marketId:m.marketId,quantity:1,proofHash:'a'.repeat(64)}])),ctx={universe,focusGenericFinancialBooks,pairedLiveHost:{status:()=>({controller:{ownerId:'Codex',tournamentId:'T',inventory}})},scheduler:{bookLastAttempt:{}},runtime:{},planned:{reads:['305','377','354','195'].map(marketId=>({marketId,path:'supplementalDepth'}))}};
 vm.runInNewContext(source.slice(start,end),ctx);assert.equal(ctx.planned.reads.length,4);assert.ok(ctx.planned.reads.some(r=>r.marketId==='381'));assert.equal(ctx.runtime.genericFinancialReadFocus.executionAuthority,false);
});

