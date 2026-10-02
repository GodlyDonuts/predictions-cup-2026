import test from 'node:test';import assert from 'node:assert/strict';
import {basketDefinitions,basketCandidates} from './basket-inputs.mjs';
const at=Date.parse('2026-10-02T10:00:00.000Z');
function fixture(){
 const universe={hash:'u',tournamentId:'t',exchanges:[{marketId:'1',exchangeId:'101',option:'YES'},{marketId:'2',exchangeId:'102',option:'YES'}]};
 const markets=universe.exchanges.map(m=>({id:m.marketId,status:'open',isComposite:false,exchanges:[{id:m.exchangeId}]}));
 const nodes=Object.fromEntries(markets.map((m,i)=>[m.id,{root:{node_type:'contract',contract_type:'Election Outcome',settlement_date:'2026-11-04T00:00:00Z',settled_with:null,contract_details:{raceId:'race',stageId:'stage',electionDate:'2026-11-03',resolutionType:'Party Winner',raceStage:'General',winnerName:i?'Republican Party':'Democratic Party'}}}]));
 const coverage={universeHash:'u',tournamentId:'t',quotes:Object.fromEntries(universe.exchanges.map((m,i)=>[m.exchangeId,{...m,receivedAt:new Date(at-1000).toISOString(),bestBid:i?.835:.19,bestAsk:i?.845:.2,crossed:false}]))};
 return {universe,markets,nodes,coverage};
}
test('metadata creates fingerprinted research pairs; exact dual-NO margin includes both adverse ticks',()=>{
 const f=fixture(),defs=basketDefinitions(f.markets,f.nodes,f.universe);assert.equal(defs.length,1);assert.equal(defs[0].settlementVerified,false);assert.equal(defs[0].legs[0].predicateHash.length,64);
 const c=basketCandidates(defs,f.coverage,f.universe,at);assert.equal(c.length,1);assert.equal(c[0].priceGapMicros,15000);
 f.nodes['2'].root.contract_details.stageId='different';assert.deepEqual(basketDefinitions(f.markets,f.nodes,f.universe),[]);
});
test('asynchronous, stale, future, crossed, foreign and fractional-tick bulk leads are rejected',()=>{
 for(const change of [f=>f.coverage.quotes['102'].receivedAt=new Date(at-2000).toISOString(),f=>Object.values(f.coverage.quotes).forEach(q=>q.receivedAt=new Date(at-120001).toISOString()),f=>Object.values(f.coverage.quotes).forEach(q=>q.receivedAt=new Date(at+1).toISOString()),f=>f.coverage.quotes['101'].crossed=true,f=>f.coverage.universeHash='foreign',f=>f.coverage.quotes['101'].bestBid=.190001,f=>f.coverage.quotes['101'].bestBid=.175]){
  const f=fixture(),defs=basketDefinitions(f.markets,f.nodes,f.universe);change(f);assert.deepEqual(basketCandidates(defs,f.coverage,f.universe,at),[]);
 }
});
test('candidate scope and predicate fingerprints cannot silently follow changed market identity',()=>{
 const f=fixture(),defs=basketDefinitions(f.markets,f.nodes,f.universe);defs[0].legs[0].exchangeId='999';assert.throws(()=>basketCandidates(defs,f.coverage,f.universe,at),/scope/);
 f.markets[0].exchanges[0].id='999';assert.throws(()=>basketDefinitions(f.markets,f.nodes,f.universe),/scope/);
});
