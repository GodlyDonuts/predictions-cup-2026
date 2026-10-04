import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {nativeExecutable,evaluateNativeCapture} from './rust-backend.mjs';
import {digest} from './model.mjs';

const root=fs.mkdtempSync(path.join(os.tmpdir(),'baal-native-clock-v2-'));
process.on('exit',()=>{
 assert.equal(path.dirname(path.resolve(root)),path.resolve(os.tmpdir()));
 fs.rmSync(root,{recursive:true,force:true});
});
const config={...JSON.parse(fs.readFileSync(new URL('./config.json',import.meta.url))),
 initialCash:'100.000000',cashReserve:'50.000000',maxExposure:'50.000000',
 feeBps:0,marketIds:['377'],orderShares:1,maxPositionShares:1,latencyMs:3000};
const strict={...config,fillClockPolicy:'independent_request_latency_v2'};
const epoch=Date.parse('2026-10-01T16:00:00Z'),at=n=>new Date(epoch+n).toISOString();
const sha=b=>createHash('sha256').update(b).digest('hex');let serial=0;
function file(value,suffix='.json'){
 const f=path.join(root,++serial+suffix);
 fs.writeFileSync(f,typeof value==='string'?value:JSON.stringify(value));return f;
}
function book(receipt,request=receipt,sequence=receipt,source=receipt,bid='.400',ask='.410'){
 return {marketId:'377',exchangeId:'9',instrument:'9:YES',sequence:String(sequence),
 sourceAt:at(source),receivedAt:at(receipt),requestStartedAt:at(request),
 bids:[{price:'0'+bid,quantity:10000}],asks:[{price:'0'+ask,quantity:10000}]};
}
const step=(b,net=.1)=>({kind:'book',book:b,netEdge:net,horizonMs:3600000,minimumEdge:.005});
const point=n=>({kind:'point',at:at(n)});
function trace(c,operations){
 const r=spawnSync(nativeExecutable,['trace','--fixture',file({config:c,startedAt:at(0),operations})],
  {windowsHide:true,encoding:'utf8'});
 assert.equal(r.status,0,r.stderr||r.error?.message);return JSON.parse(r.stdout);
}
function validClock(f,latency=3000){
 const c=f.sourceClock;assert.equal(c.policy,'independent_request_latency_v2');
 assert.equal(c.minimumLatencyMs,latency);
 assert(c.requestStartedAt>=c.decisionAt+latency);
 assert.equal(c.receivedAt,f.at);assert(c.receivedAt>=c.requestStartedAt);
 for(const key of ['decisionSequence','decisionSourceNs','fillSequence','fillSourceNs']){
  assert.equal(typeof c[key],'string');assert.match(c[key],/^[0-9]+$/);
 }
 assert(c.fillSequence!==c.decisionSequence||c.fillSourceNs!==c.decisionSourceNs);
}

test('a later receipt for an early request cannot fill V2; exact request-latency threshold can',()=>{
 const ops=[step(book(1000,900)),step(book(6000,2000)),point(6000),step(book(7000,4000)),point(7000)];
 const legacy=trace(config,ops),current=trace(strict,ops);
 assert.equal(legacy.points[0].fillExecutions,1,'regression reproduces the legacy timing problem');
 assert.equal(current.points[0].fillExecutions,0);assert.equal(current.points[0].pendingIntents,1);
 assert.equal(current.points[0].blockedPreLatencySnapshots,1);
 assert.equal(current.points[1].fillExecutions,1);validClock(current.fills[0]);
 assert.equal(current.fills[0].sourceClock.requestStartedAt-current.fills[0].sourceClock.decisionAt,3000);
 assert.equal(current.points[1].fees,'0.000000');
});

test('sell fills also require a post-latency request, while preserving the original profit limit',()=>{
 const dip=b=>({...step(b,null),takeProfit:'0.005000',exitPolicy:'profit_floor_stop_buffer_v1',stopBufferTicks:2});
 const ops=[step(book(1000,900)),dip(book(5000,4000)),dip(book(10000,9000,10000,10000,'.450','.460')),
  dip(book(15000,11000,15000,15000,'.440','.450')),point(15000),
  dip(book(16000,13000,16000,16000,'.440','.450')),point(16000)];
 const r=trace(strict,ops);assert.equal(r.points[0].completedTrades,0);
 assert.equal(r.points[0].pendingIntents,1);assert.equal(r.points[1].completedTrades,1);
 assert.equal(r.points[1].realizedPnl,'0.020000');
 assert.equal(r.points[1].cash,'100.020000');r.fills.forEach(f=>validClock(f));
 assert.equal(r.fills.at(-1).economics.reason,'TAKE_PROFIT');
});

test('liquidation freshness uses request age instead of a late response timestamp in V2',()=>{
 const ops=[step(book(1000,900)),step(book(5000,4000)),point(24001)];
 const old=trace(config,ops).points[0],current=trace(strict,ops).points[0];
 assert.notEqual(old.liquidationPnl,null);assert.equal(current.liquidationPnl,null);
 assert.equal(current.unsoldShares,1);assert.equal(current.conservativePnl,'-0.415000');
 assert.equal(current.realizedPnl,'0.000000');
});

test('a newly requested duplicate version remains non-executable in V2',()=>{
 const r=trace(strict,[step(book(1000,900,1,1000)),step(book(6000,5000,1,1000)),point(6000),
  step(book(7000,6000,2,7000)),point(7000)]);
 assert.equal(r.points[0].fillExecutions,0);assert.equal(r.points[1].fillExecutions,1);
 validClock(r.fills[0]);
});

test('exact nanosecond versions and large sequence IDs survive JSON without Number rounding',()=>{
 const seq='9007199254740993',a=book(1000,900,seq,1000),b=book(5000,4000,seq,1000);
 a.sourceAt='2026-10-01T16:00:01.000000001Z';b.sourceAt='2026-10-01T16:00:01.000000002Z';
 const r=trace(strict,[step(a),step(b),point(5000)]);assert.equal(r.fills.length,1);
 const c=r.fills[0].sourceClock;validClock(r.fills[0]);
 assert.equal(c.decisionSequence,seq);assert.equal(c.fillSequence,seq);
 const base=BigInt(epoch+1000)*1000000n;
 assert.equal(c.decisionSourceNs,String(base+1n));assert.equal(c.fillSourceNs,String(base+2n));
 assert.equal(BigInt(c.fillSourceNs)-BigInt(c.decisionSourceNs),1n);
});

function capture({length=120,earlyLastRequest=false}={}){
 let previous='';const rows=[];
 for(let i=0;i<length;i++){
  const receipt=i*30000,request=earlyLastRequest&&i===length-1?receipt-11000:receipt;
  const bid=earlyLastRequest&&i===length-1?.35:i%12<3?.35:.60;
  const body={sequence:i+1,previous,at:at(receipt),state:{},event:{type:'cycle',clockAt:at(receipt),
   accountExclusions:[],sourceSnapshots:[{endpoint:'/markets/377/orderbook?tournamentId='+config.tournamentId,
    receivedAt:at(receipt),requestStartedAt:at(request),data:{contexts:[{type:'tournament',
     tournament:{id:config.tournamentId},orderbook:{exchanges:[{exchangeId:'9',option:'YES',
      asOf:{at:at(receipt),sequence:i},bids:[{price:bid,quantity:10000}],asks:[{price:bid+.01,quantity:10000}]}]}}]}}]}};
  previous=digest(body);rows.push(JSON.stringify({...body,hash:previous}));
 }
 return rows.join('\n')+'\n';
}
function declaration(raw,declared=90*30000){
 const rows=raw.trimEnd().split('\n'),prefix=rows.slice(0,60).join('\n')+'\n';
 return {schemaVersion:1,mode:'causal_dip_subsequent_v2',cohort:'test-request-clock-v2',declaredAt:at(declared),config:strict,
  development:{records:60,bytes:Buffer.byteLength(prefix),hash:sha(prefix),through:JSON.parse(rows[59]).at},
  strategies:[{id:'dip',parameters:{lookback:6,gapResetSeconds:900,z:0,minimumEdge:.005,horizonSeconds:60,takeProfit:'0.005000'}}],
  scenarios:[{id:'base',latencyMs:3000,slippageTicks:1,depthFractionBps:100}],stopBufferTicks:2};
}
function evaluate(raw,d){return evaluateNativeCapture({journal:file(raw,'.jsonl'),declaration:file(d),
 endByte:Buffer.byteLength(raw),expectedSha:sha(raw),out:path.join(root,++serial+'.json'),workers:2});}

test('V2 subsequent wallets begin at actual declaration time; warmup never contributes earlier fills or P&L',async()=>{
 const raw=capture(),d=declaration(raw),r=await evaluate(raw,d),e=r.evaluation;
 assert.equal(e.evaluationStart,d.declaredAt);assert.equal(e.subsequentRecords,29);
 assert.equal(e.evaluationClassification,'post_declaration_data_frozen_strategies_no_pre_declaration_wallet_activity');
 assert(e.scorecards.some(c=>c.fills.length>0),'fixture must exercise actual native fills');
 for(const card of e.scorecards){
  assert.equal(card.point.fillClockPolicy,'independent_request_latency_v2');
  for(const f of card.fills){validClock(f);assert(f.sourceClock.decisionAt>Date.parse(d.declaredAt));}
  assert.equal(card.point.realizedPnl,card.economics.netPnl);assert.equal(card.point.fees,'0.000000');
 }
});

test('a post-declaration response requested before the freeze cannot create even an entry intent',async()=>{
 const raw=capture({length:80,earlyLastRequest:true}),d=declaration(raw,79*30000-10000),r=await evaluate(raw,d);
 assert.equal(r.evaluation.subsequentRecords,1);
 for(const card of r.evaluation.scorecards){assert.equal(card.point.intents,0);assert.equal(card.fills.length,0);}
});

test('clock policy, declaration version and new-data availability fail closed on mismatches',async()=>{
 const raw=capture(),d=declaration(raw);
 await assert.rejects(evaluate(raw,{...d,config}),/FILL_CLOCK_POLICY_MISMATCH/);
 await assert.rejects(evaluate(raw,{...d,mode:'causal_dip_subsequent_v1'}),/FILL_CLOCK_POLICY_MISMATCH/);
 await assert.rejects(evaluate(raw,{...d,declaredAt:at(120*30000)}),/NO_POST_DECLARATION_DATA/);
 await assert.rejects(evaluate(raw,{...d,config:{...strict,fillClockPolicy:'unrecognized'}}),/DECLARATION_JSON_INVALID/);
});
