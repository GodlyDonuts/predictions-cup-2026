import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {fixture,journal,run,base} from './native-quote-markouts-fixture.mjs';
import {sha} from './native-dip-proof-v1.mjs';

const minute=row=>(Date.parse(row[0])-base)/60000;
function bind(f){
 const raw=journal(f.rows),prefix=journal(f.rows.filter(row=>minute(row)<40));
 fs.writeFileSync(f.files.quotes,raw);
 f.declaration.development.quotes={bytes:Buffer.byteLength(prefix),hash:sha(prefix)};
 f.sources.quotes={path:f.files.quotes,bytes:Buffer.byteLength(raw),hash:sha(raw),checkpoints:[f.declaration.development.quotes]};
}
function trending(t){
 const f=fixture(t);
 for(const row of f.rows){const q=row[1].snapshot.quotes[0],i=minute(row);q.bestBid=i<40?.90:i<46?.93:.97;q.bestAsk=q.bestBid+.005;q.latestPrice=q.bestBid;q.spread=.005;}
 bind(f);return f;
}
const find=(r,side,family,horizon=5)=>r.markouts.sides[side].strict.cards.find(c=>c.strategy===`trend15m_${family}_h${horizon}m`);

test('real mechanism CLI preserves independent entry/horizon clocks and exact spread/slippage',t=>{
 const f=trending(t),result=run(f,4,'quote-mechanisms');assert.equal(result.child.status,0,result.child.stderr);
 const m=result.result.markouts,c=find(result.result,'yes','continuation');
 assert.equal(m.candidates,12);assert.equal(m.fees,0);assert.equal(m.networkRequests,0);assert.equal(m.competitionOrders,0);
 const first=c.episodes[0];assert.equal(first.signal.publishedAt,base+40*60000);
 assert.equal(first.entry.publishedAt,base+41*60000);
 assert.equal(first.outcome.publishedAt,base+47*60000);
 assert.equal(first.quotedNetMarkoutMicros,25000);
 assert.equal(first.midpointMoveTwiceMicros-first.spreadCostTwiceMicros-2*first.slippageCostMicros,2*first.quotedNetMarkoutMicros);
 for(const side of ['yes','no'])for(const c of m.sides[side].strict.cards){
  assert.equal(Object.values(c.summary.statusCounts).reduce((n,v)=>n+v,0),c.episodes.length);
  assert.equal(c.causalSignalFeatures.length,c.episodes.length);
  for(const e of c.episodes){if(e.entry){assert(e.entry.requestStartedAt>=e.signal.publishedAt+c.scenario.latencyMs);assert(e.entry.ask<=e.signal.ask);}if(e.outcome)assert(e.outcome.requestStartedAt>=e.targetAt);}
  for(const f of c.causalSignalFeatures){assert(f.signalPublishedAt-f.lagPublishedAt>=840000);assert(f.signalPublishedAt-f.lagPublishedAt<=900000);assert(f.windowDistinctMinutes>=10);assert(f.lagReceivedAt<=f.signalPublishedAt);}
 }
 assert.equal(m.profitabilityEstablished,false);
});
test('canonical side mirroring keeps mechanism evidence and all price endpoints exact',t=>{
 const f=trending(t),original=run(f,2,'quote-mechanisms');assert.equal(original.child.status,0,original.child.stderr);
 const mirrored=trending(t);for(const row of mirrored.rows){const q=row[1].snapshot.quotes[0],bid=1-q.bestAsk,ask=1-q.bestBid;q.bestBid=Number(bid.toFixed(6));q.bestAsk=Number(ask.toFixed(6));q.latestPrice=q.bestBid;}
 bind(mirrored);const result=run(mirrored,4,'quote-mechanisms');assert.equal(result.child.status,0,result.child.stderr);
 assert.deepEqual(result.result.markouts.sides.no,original.result.markouts.sides.yes);
 assert.deepEqual(result.result.markouts.sides.yes,original.result.markouts.sides.no);
});
test('gaps reset mechanism history, and future prices cannot change earlier causal signal features',t=>{
 const f=trending(t);f.rows=f.rows.filter(row=>minute(row)<19||minute(row)>=40);bind(f);
 const result=run(f,2,'quote-mechanisms');assert.equal(result.child.status,0,result.child.stderr);
 for(const side of ['yes','no'])for(const c of result.result.markouts.sides[side].strict.cards){assert(c.signalDiagnostics.history_gap_reset>0);assert(c.causalSignalFeatures.every(x=>x.signalPublishedAt>=base+54*60000));}
 const earlier=trending(t),extended=trending(t);
 for(const row of extended.rows){if(minute(row)>=65){const q=row[1].snapshot.quotes[0];q.bestBid=.02;q.bestAsk=.025;q.latestPrice=.02;}}
 bind(extended);const a=run(earlier,2,'quote-mechanisms'),b=run(extended,2,'quote-mechanisms');assert.equal(a.child.status,0,a.child.stderr);assert.equal(b.child.status,0,b.child.stderr);
 for(const side of ['yes','no'])for(let i=0;i<a.result.markouts.sides[side].strict.cards.length;i++){
  const before=card=>card.causalSignalFeatures.filter(x=>x.signalPublishedAt<base+65*60000);
  assert.deepEqual(before(a.result.markouts.sides[side].strict.cards[i]),before(b.result.markouts.sides[side].strict.cards[i]));
 }
});
test('one and eight workers produce identical mechanism cards; source changes fail closed',t=>{
 const f=trending(t),a=run(f,1,'quote-mechanisms'),b=run(f,8,'quote-mechanisms');assert.equal(a.child.status,0,a.child.stderr);assert.equal(b.child.status,0,b.child.stderr);assert.deepEqual(a.result.markouts.sides,b.result.markouts.sides);
 const changed=fs.readFileSync(f.files.quotes);changed[changed.length-2]=changed[changed.length-2]===32?33:32;fs.writeFileSync(f.files.quotes,changed);
 const corrupt=run(f,2,'quote-mechanisms');assert.notEqual(corrupt.child.status,0);assert.equal(fs.existsSync(corrupt.out),false);
});
test('spread changes alone cannot trigger corroborated directional mechanisms',t=>{
 const f=trending(t);for(const row of f.rows){const q=row[1].snapshot.quotes[0];q.bestBid=.9;q.bestAsk=minute(row)<40?.905:.955;q.latestPrice=.9;q.spread=minute(row)<40?.005:.055;}
 bind(f);const result=run(f,2,'quote-mechanisms');assert.equal(result.child.status,0,result.child.stderr);
 for(const side of ['yes','no'])for(const c of result.result.markouts.sides[side].strict.cards)assert.equal(c.episodes.length,0);
 assert(find(result.result,'yes','continuation').signalDiagnostics.one_sided_quote_move>0);
});
test('prospective cutoffs exclude early and late-publication pre-cutoff requests without inventing future signals',t=>{
 const f=trending(t),cutoff=base+40*60000;
 f.rows[41][1].snapshot.requestStartedAt=new Date(cutoff-1).toISOString();bind(f);
 const result=run(f,2,'quote-mechanisms',['--signal-after',String(cutoff)]);assert.equal(result.child.status,0,result.child.stderr);
 assert.equal(result.result.markouts.signalAfter,cutoff);
 const c=find(result.result,'yes','continuation');assert.equal(c.episodes[0].signal.publishedAt,base+42*60000);
 for(const side of ['yes','no'])for(const c of result.result.markouts.sides[side].strict.cards)for(const e of c.episodes){assert(e.signal.publishedAt>cutoff);assert(e.signal.requestStartedAt>cutoff);}
 const future=run(f,2,'quote-mechanisms',['--signal-after',String(base+100*60000)]);assert.equal(future.child.status,0,future.child.stderr);for(const side of ['yes','no'])for(const c of future.result.markouts.sides[side].strict.cards)assert.equal(c.episodes.length,0);
 const invalid=run(f,2,'quote-mechanisms',['--signal-after','-1']);assert.notEqual(invalid.child.status,0);assert.equal(fs.existsSync(invalid.out),false);
});
