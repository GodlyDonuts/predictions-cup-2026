import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {digest} from './model.mjs';
import {diagnoseJournal} from './diagnostics.mjs';
import {evaluateJournal} from './evaluate.mjs';
import {replayJournal} from './replay.mjs';

const baseConfig=JSON.parse(fs.readFileSync(new URL('./config.json',import.meta.url)));
const config={...baseConfig,marketIds:['377','342']};
const base=Date.parse('2026-10-02T03:00:00Z'),at=s=>new Date(base+s*1000).toISOString();
function book(marketId,n,{requested=n,bid=.4,ask=.41}={}){
  return {endpoint:'/markets/'+marketId+'/orderbook?tournamentId='+config.tournamentId+'&depth=20',requestStartedAt:at(requested),receivedAt:at(n),data:{contexts:[{type:'tournament',tournament:{id:config.tournamentId},orderbook:{exchanges:[{exchangeId:marketId==='377'?'1066':'1031',option:'YES',asOf:{at:at(n),sequence:n},bids:[{price:bid,quantity:10000}],asks:[{price:ask,quantity:10000}]}]}}]}};
}
function cycle(core,extra,clock,reconciled=.5){return {type:'cycle',clockAt:at(clock),exclusionObservedAt:at(reconciled),accountExclusions:[],sourceSnapshots:core,supplementalDepth:extra};}
function capture(events){let previous='';return events.map((event,i)=>{const body={sequence:i+1,previous,at:event.clockAt,event};const hash=digest(body);previous=hash;return JSON.stringify({...body,hash});}).join('\n')+'\n';}
function plan(c=config){return {schemaVersion:1,createdAt:at(-60),startAt:at(0),endAt:at(60),baseConfig:c,baseConfigHash:digest(c),minimumClosedTrades:100,variants:[{name:'base',overrides:{},configHash:digest(c)}]};}
function evaluate(events,c=config){return evaluateJournal(capture(events),plan(c));}

test('diagnostic per-market accepted counts include selected supplemental books',()=>{
  const r=diagnoseJournal(capture([cycle([book('377',1)],[book('342',2)],3)]),config);
  assert.equal(r.observations,2);assert.equal(Object.values(r.markets).reduce((n,m)=>n+m.accepted,0),2);assert.equal(r.markets['342'].accepted,1);
});
test('supplemental-only diagnostics pairs malformed response/skip once and ignores foreign skips',()=>{
  const reason='Missing, locked or crossed market';
  const raw=capture([cycle([book('377',1)],[book('342',2)],3),cycle([{marketId:'377',skipped:'http_503',at:at(10)}],[book('342',11,{ask:.4}),{marketId:'342',skipped:reason,at:at(12)}],13,9)]);
  const r=diagnoseJournal(raw,{...config,marketIds:['342']});
  assert.equal(r.observations,1);assert.deepEqual(Object.keys(r.markets),['342']);assert.equal(r.skipReasons[reason],1);assert.equal(r.skipReasons.http_503,undefined);
});
test('original scope ignores supplemental prices and failures',()=>{
  const a=[cycle([book('377',1)],[book('342',2)],3),cycle([book('377',59)],[{marketId:'342',skipped:'http_503',at:at(60)}],62,58)],b=a.map(e=>({...e,supplementalDepth:[]}));
  const c={...baseConfig,marketIds:['377']};
  assert.deepEqual(replayJournal(capture(a),c).strategies,replayJournal(capture(b),c).strategies);
  const d=diagnoseJournal(capture(a),c);assert.equal(d.observations,2);assert.deepEqual(d.skipReasons,{});
});
test('end-exclusive filtering prevents an after-window supplemental fill',()=>{
  const r=evaluate([cycle([book('377',1)],[book('342',2)],3),cycle([book('377',59)],[book('342',61)],62,58)]);
  assert.equal(r.variants[0].observations,3);assert.equal(r.variants[0].strategies.buy_hold_benchmark.fills,1);assert.equal(r.variants[0].strategies.buy_hold_benchmark.openShares,10);assert.equal(r.excludedSnapshots,1);
});
test('a supplemental observation exactly at the end is excluded',()=>{
  const r=evaluate([cycle([book('377',1)],[book('342',2)],3),cycle([book('377',59)],[book('342',60)],61,58)]);
  assert.equal(r.variants[0].observations,3);assert.equal(r.excludedSnapshots,1);
});
test('supplemental requests before the start or before reconciliation are excluded',()=>{
  for(const requested of [-.1,.1]){
    const r=evaluate([cycle([book('377',1)],[book('342',2,{requested})],3)]);
    assert.equal(r.variants[0].observations,1);assert.equal(r.excludedSnapshots,1);
  }
});
test('start-inclusive supplemental-only cycle is retained',()=>{
  const r=evaluate([cycle([], [book('342',0)],1,0)],{...config,marketIds:['342']});
  assert.equal(r.variants[0].observations,1);assert.equal(r.excludedSnapshots,0);
});
test('eligible supplemental-only cycles survive even when core array is empty',()=>{
  const r=evaluate([cycle([book('377',1)],[book('342',2)],3),cycle([],[book('342',59)],62,58)]);
  assert.equal(r.variants[0].observations,3);assert.equal(r.variants[0].strategies.buy_hold_benchmark.fills,1);assert.equal(r.excludedSnapshots,0);
});
test('an invalid reconciliation excludes both source arrays',()=>{
  const r=evaluate([cycle([book('377',1)],[book('342',2)],3,-1)]);
  assert.equal(r.status,'awaiting_in_window_capture');assert.equal(r.excludedSnapshots,2);
});
test('a supplemental book cannot occur after its recorded cycle clock',()=>{
  const r=evaluate([cycle([book('377',1)],[book('342',4)],3)]);
  assert.equal(r.variants[0].observations,1);assert.equal(r.excludedSnapshots,1);
});
