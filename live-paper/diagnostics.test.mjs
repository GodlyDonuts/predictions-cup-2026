import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {createHash} from 'node:crypto';
import {diagnoseJournal} from './diagnostics.mjs';
const c=JSON.parse(fs.readFileSync(new URL('./config.json',import.meta.url)));
function capture(rows){
  let previous='';return rows.map((x,n)=>{
    const at=new Date(Date.parse('2026-10-01T23:00:00Z')+n*30000).toISOString();
    const sourceAt=new Date(Date.parse('2026-10-01T23:00:00Z')+(x.version??n)*30000).toISOString();
    const book={endpoint:'/markets/377/orderbook?tournamentId='+c.tournamentId,requestStartedAt:at,receivedAt:at,data:{contexts:[{type:'tournament',tournament:{id:c.tournamentId},orderbook:{exchanges:[{exchangeId:'9',option:'YES',asOf:{at:sourceAt,sequence:x.version??n},bids:[{price:x.bid??.4,quantity:10000}],asks:[{price:x.ask??.41,quantity:10000}]}]}}]}};
    if(x.malformed)book.data.contexts[0].orderbook.exchanges[0].asks=[];
    const event={type:'cycle',clockAt:at,accountExclusions:[],sourceSnapshots:[x.skip?{marketId:'377',skipped:x.skip,at}:book,...(x.extraSkip?[{marketId:x.skipMarket||'377',skipped:x.extraSkip,at}]:[])]};
    const body={sequence:n+1,previous,at,event};const hash=createHash('sha256').update(JSON.stringify(body)).digest('hex');previous=hash;return JSON.stringify({...body,hash});
  }).join('\n')+'\n';
}
test('constant prices expose zero variance and cost hurdle with no invented opportunities',()=>{
  const raw=capture(Array.from({length:8},()=>({}))),r=diagnoseJournal(raw,c),m=r.markets['377'];
  assert.equal(m.accepted,8);assert.equal(m.gateCounts.warmup,5);assert.equal(m.gateCounts.postWarmup,3);assert.equal(m.gateCounts.zeroVariance,3);assert.equal(m.gateCounts.edgeBelowThreshold,3);assert.equal(m.gateCounts.actualNewIntents,undefined);assert.equal(m.latencyMs.max,0);assert.equal(m.samplingGapMs.median,30000);assert.equal(r.interpretation.networkRequests,0);
  assert.throws(()=>diagnoseJournal(raw.replace('10000','9999'),c),/integrity/);assert.throws(()=>diagnoseJournal(raw.slice(0,-1),c),/Incomplete/);
});
test('duplicates do not count as new signals; skipped observations retain explicit reasons',()=>{
  const r=diagnoseJournal(capture([{},{version:0},{skip:'http_503'},{}]),{...c,maxDataAgeMs:60000});
  assert.equal(r.markets['377'].accepted,3);assert.equal(r.markets['377'].duplicates,1);assert.equal(r.markets['377'].uniqueVersions,2);assert.equal(r.markets['377'].gateCounts.warmup,2);assert.equal(r.skipReasons.http_503,1);
});
test('large dislocation shows both gates and actual intent without claiming a fill or profitability',()=>{
  const rows=[.5,.51,.49,.51,.5,.44].map(bid=>({bid,ask:bid+.01}));
  const r=diagnoseJournal(capture(rows),c),m=r.markets['377'];
  assert.equal(m.gateCounts.jointSignalPass,1);assert.equal(m.gateCounts.actualNewIntents,1);assert.ok(m.entryEdge.max>.04);assert.equal(r.interpretation.parametersChanged,false);
});
test('malformed response plus its validation skip counts once; independent errors still count',()=>{
 const reason='Missing, locked or crossed market';
 const paired=diagnoseJournal(capture([{},{malformed:true,extraSkip:reason}]),c);assert.equal(paired.skipReasons[reason],1);assert.equal(paired.observations,1);
 const unpaired=diagnoseJournal(capture([{},{malformed:true}]),c);assert.equal(unpaired.skipReasons[reason],1);
 const distinct=diagnoseJournal(capture([{},{malformed:true,extraSkip:'http_503',skipMarket:'354'}]),c);assert.equal(distinct.skipReasons[reason],1);assert.equal(distinct.skipReasons.http_503,1);
});
