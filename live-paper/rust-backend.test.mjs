import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {nativeExecutable,replayNativeBank,sweepNativeCapture} from './rust-backend.mjs';
import {initialState,stepForecast,invalidate,digest,normalizeBook} from './model.mjs';
import {portfolioPoint,replayFrozenPortfolioBank,chronologicalPartitions,SELECTION_POLICY} from './portfolio-bank.mjs';
import {verifySource} from './ml-shadow-v2.mjs';
import {amount,fee,TICK,money} from '../paper/fixed.mjs';

// Native execution is mandatory for these checks. Build the backend first.
const c={...JSON.parse(fs.readFileSync(new URL('./config.json',import.meta.url))),feeBps:0,marketIds:['377'],orderShares:6,maxPositionShares:12};
const root=fs.mkdtempSync(path.join(os.tmpdir(),'baal-rust-parity-'));
process.on('exit',()=>fs.rmSync(root,{recursive:true,force:true}));
const epoch=Date.parse('2026-10-01T16:00:00Z'),at=ms=>new Date(epoch+ms).toISOString();
let serial=0;
const file=(value,suffix='.json')=>{const p=path.join(root,++serial+suffix);fs.writeFileSync(p,typeof value==='string'?value:JSON.stringify(value));return p;};
function book(t,{sequence=t,bid='.400',ask='.410',bids,asks,request=t}={}){
 return {marketId:'377',exchangeId:'9',instrument:'9:YES',sequence:String(sequence),sourceAt:at(t),receivedAt:at(t),requestStartedAt:at(request),versionKey:sequence+':'+at(t),bids:bids??[{price:'0'+bid,quantity:10000}],asks:asks??[{price:'0'+ask,quantity:10000}]};
}
const step=(b,netEdge=.1,horizonMs=60000)=>({kind:'book',book:b,netEdge,horizonMs,minimumEdge:.005});
const point=t=>({kind:'point',at:at(t)});
function compare(config,operations){
 let w=initialState(config,'native-fixture',at(0));const points=[];
 for(const op of operations){
  if(op.kind==='book'){
   const held=w.strategies.mean_reversion.positions[op.book.instrument];let horizon=op.horizonMs,policyConfig=config;
   if(op.takeProfit){const bid=amount(op.book.bids[0].price)-TICK*BigInt(config.slippageTicks);policyConfig={...config,stopLoss:money(amount(config.stopLoss)-TICK*BigInt(config.slippageTicks))};
    if(held){const n=bid*BigInt(held.quantity);if(n-fee(n,held.quantity,{feeBps:config.feeBps,feePerShare:'0.000000'})-amount(held.cost)>=amount(op.takeProfit)*BigInt(held.quantity))horizon=1000;}
   }
   // Keep the wallet binding to the original config while adjusting the forecast
   // stop to compare the dip policy's executable bid, rather than raw bid.
   if(policyConfig!==config)w.configHash=digest(policyConfig);
   w=stepForecast(w,op.book,policyConfig,{netEdge:op.netEdge,horizonMs:horizon,minimumEdge:op.minimumEdge});w.configHash=digest(config);
  }
  else if(op.kind==='invalidate'){invalidate(w,op.marketId,op.at,'fixture');for(const [id,p]of Object.entries(w.strategies.mean_reversion.pending))if(p.action==='buy'&&(op.marketId===null||w.books[id]?.marketId===op.marketId)){delete w.strategies.mean_reversion.pending[id];w.strategies.mean_reversion.expiredIntents++;}}
  else points.push(portfolioPoint(w,config,op.at));
 }
 const result=spawnSync(nativeExecutable,['trace','--fixture',file({config,startedAt:at(0),operations})],{windowsHide:true,encoding:'utf8'});
 assert.equal(result.status,0,result.stderr||result.error?.message);const native=JSON.parse(result.stdout);
 assert.deepEqual(native.points,points);
 const fills=w.strategies.mean_reversion.fills.map(({intentId,instrument,action,quantity,price,fee,at})=>({intentId,instrument,action,quantity,price,fee,at:Date.parse(at)}));
 assert.deepEqual(native.fills,fills);return native;
}
test('native accounting matches latency, ladder fills, FIFO partial sales and zero-fee closes',()=>{
 const a=compare(c,[step(book(1000)),step(book(2000)),point(2000),step(book(6000,{bid:'.390',asks:[{price:'0.400',quantity:200},{price:'0.405',quantity:1000}]})),point(6000),step(book(67000,{bid:'.495',ask:'.520'})),step(book(71000,{bids:[{price:'0.510',quantity:200},{price:'0.500',quantity:100}],ask:'.530'})),point(71000),step(book(75000,{bid:'.510',ask:'.530'}),null),point(75000)]);
 assert.ok(a.fills.length>=4);assert.equal(a.points.at(-1).completedTrades,1);assert.equal(a.points.at(-1).fees,'0.000000');
});
test('fee rounding and reserve-limited affordability match exact JS accounting',()=>{
 const a=compare({...c,feeBps:17,initialCash:'2.000000',cashReserve:'1.000000',maxExposure:'1.000000'},[step(book(1000)),step(book(6000)),point(6000),step(book(67000,{bid:'.500',ask:'.510'}),null),step(book(71000,{bid:'.505',ask:'.515'}),null),point(71000)]);
 assert.equal(a.fills[0].quantity,2);assert.notEqual(a.points.at(-1).fees,'0.000000');
});
test('dip take-profit exits are delayed and retain exact accounting',()=>{
 const dip=b=>({...step(b,null,3600000),takeProfit:'0.005000'});
 const a=compare(c,[{...dip(book(1000)),netEdge:.1},dip(book(6000)),dip(book(11000,{bid:'.450',ask:'.460'})),point(11000),dip(book(15000,{bid:'.455',ask:'.465'})),point(15000)]);
 assert.equal(a.points[0].pendingIntents,1);assert.equal(a.points[0].completedTrades,0);assert.equal(a.points[1].completedTrades,1);assert.equal(a.points[1].realizedPnl,'0.210000');
});
test('duplicate versions cannot fill again; missing liquidation charges unsold inventory',()=>{
 const b=book(6000),duplicate={...b,receivedAt:at(8000),requestStartedAt:at(8000)};
 const a=compare(c,[step(book(1000)),step(b),step(duplicate),point(8000),point(31000)]);
 assert.equal(a.fills.length,1);assert.equal(a.points.at(-1).liquidationPnl,null);assert.equal(a.points.at(-1).conservativePnl,'-2.490000');
 compare(c,[step(book(1000)),step(book(6000)),step(book(10000,{bids:[{price:'0.400',quantity:100}]})),point(10000)]);
});
test('invalidation revokes entry, survives repeated outage and only fresh request recovers',()=>{
 const a=compare(c,[step(book(1000)),{kind:'invalidate',marketId:'377',at:at(2000)},{kind:'invalidate',marketId:null,at:at(3000)},point(3000),step(book(6000),null),point(6000)]);
 assert.equal(a.fills.length,0);assert.equal(a.points[0].expiredIntents,1);
 const f={config:c,startedAt:at(0),operations:[step(book(1000)),{kind:'invalidate',marketId:'377',at:at(2000)},step(book(6000,{request:1500}))]};
 const r=spawnSync(nativeExecutable,['trace','--fixture',file(f)],{encoding:'utf8',windowsHide:true});assert.equal(r.status,1);assert.equal(JSON.parse(r.stderr).error,'PRE_INVALIDATION_SNAPSHOT');
});
test('native rejects changed depth under same version, rollback and unknown command flags',()=>{
 for(const b of [{...book(6000),sequence:'999',sourceAt:at(1000)},{...book(1000),receivedAt:at(6000),requestStartedAt:at(6000),asks:[{price:'0.420',quantity:10000}]}]){
  const r=spawnSync(nativeExecutable,['trace','--fixture',file({config:c,startedAt:at(0),operations:[step(book(1000)),step(b)]})],{encoding:'utf8',windowsHide:true});assert.equal(r.status,1);
 }
 const r=spawnSync(nativeExecutable,['trace','--secret','unsupported'],{encoding:'utf8',windowsHide:true});assert.equal(r.status,1);assert.equal(JSON.parse(r.stderr).error,'INVALID_ARGUMENTS');
});
function fixtureCapture({futureReceipt=-1,gapAfter=Infinity,gapMs=0}={}){let previous='';const time=i=>at(i*30000+(i>=gapAfter?gapMs:0));return Array.from({length:120},(_,i)=>{
 const t=time(i),bid=i%12<3?.35:.60;
 const receipt=i===futureReceipt?time(i+1):t;
 const body={sequence:i+1,previous,at:t,state:{},event:{type:'cycle',clockAt:t,accountExclusions:[],sourceSnapshots:[{endpoint:'/markets/377/orderbook?tournamentId='+c.tournamentId,receivedAt:receipt,requestStartedAt:receipt,data:{contexts:[{type:'tournament',tournament:{id:c.tournamentId},orderbook:{exchanges:[{exchangeId:'9',option:'YES',asOf:{at:receipt,sequence:i},bids:[{price:bid,quantity:10000}],asks:[{price:bid+.01,quantity:10000}]}]}}]}}]}};
 const hash=digest(body);previous=hash;return JSON.stringify({...body,hash});
 }).join('\n')+'\n';}
test('native features and ridge/stump inference match JS through trading fixtures',async()=>{
 const raw=fixtureCapture(),records=verifySource(raw),windows=chronologicalPartitions(records);
 const base={id:'ridge-fixture',horizon:60,width:1,weights:[.08],means:[0],scales:[1],rmse:.1,coverage:[{marketId:'377',eligible:true,trainingInstruments:['9:YES']}]};
 const bank={config:c,sourceRecords:records.length,sourceHash:records.prefixHashes.at(-1),trainingCaptureThrough:records.at(-1).at,snapshotOrderPolicy:'actual_receipt_order_v1',targetPolicy:'delayed_fifo_roundtrip_net_v1',historyPolicy:'market-isolated-v2',forecastLifecycle:'revoke-on-invalidation-v2',requireInstrumentCoverage:true,trainingScope:'all-markets-v1',breadthContext:false,models:[base,{...base,id:'ridge-12-fixture',width:12,weights:[.03,-.08,-.3,.2,.1,.01,.01,.001,-.001,0,0,-.01],means:Array(12).fill(0),scales:Array(12).fill(1)},{...base,id:'stump-fixture',family:'boosted_stumps',base:.01,stumps:[{feature:11,threshold:.5,leftValue:.07,rightValue:-.05}]}],selected:[],portfolioSelection:{policy:SELECTION_POLICY,windows,fitSourceHash:records.prefixHashes[windows.fitRecords-1],developmentSourceHash:records.prefixHashes[windows.selectionRecords-1]}};
 const cards=await replayFrozenPortfolioBank(raw,bank,{workers:2});
 const journal=file(raw,'.jsonl'),bankFile=file(bank),sha=createHash('sha256').update(raw).digest('hex'),out=path.join(root,++serial+'.json');
 const n=await replayNativeBank({journal,bank:bankFile,endByte:Buffer.byteLength(raw),expectedSha:sha,out,workers:2});
 assert.deepEqual(n.replay.scorecards,cards);assert.ok(cards.some(x=>x.points.at(-1).fillExecutions>0),'test must exercise trading');
 await assert.rejects(replayNativeBank({journal,bank:bankFile,endByte:Buffer.byteLength(raw),expectedSha:sha,out,workers:2}),/overwrite/);
 const corrupt=raw.replace('"quantity":10000','"quantity":9999');
 await assert.rejects(replayNativeBank({journal:file(corrupt,'.jsonl'),bank:bankFile,endByte:Buffer.byteLength(corrupt),expectedSha:sha,out:path.join(root,++serial+'.json'),workers:2}),/SOURCE_HASH_INVALID/);
 await assert.rejects(replayFrozenPortfolioBank(raw,{...bank,sourceHash:'0'.repeat(64)}),/source mismatch/);
 for(const [changed,shaOverride,end,expected]of [[raw,'0'.repeat(64),Buffer.byteLength(raw),'SOURCE_PREFIX_MISMATCH'],[raw,sha,Buffer.byteLength(raw)-1,'INCOMPLETE_SOURCE_RECORD'],[fixtureCapture({futureReceipt:10}),sha,null,'OBSERVATION_EXCEEDS_EVENT_CLOCK']]){
  await assert.rejects(replayNativeBank({journal:file(changed,'.jsonl'),bank:bankFile,endByte:end??Buffer.byteLength(changed),expectedSha:shaOverride,out:path.join(root,++serial+'.json'),workers:2}),new RegExp(expected));
 }
 for(const [b,expected]of [[{...bank,sourceHash:'0'.repeat(64)},'FROZEN_SOURCE_MISMATCH'],[{...bank,snapshotOrderPolicy:'undefined_policy'},'UNKNOWN_SNAPSHOT_POLICY'],[{...bank,portfolioSelection:{...bank.portfolioSelection,developmentSourceHash:'0'.repeat(64)}},'INVALID_FIT_PREFIX']]){
  await assert.rejects(replayNativeBank({journal,bank:file(b),endByte:Buffer.byteLength(raw),expectedSha:sha,out:path.join(root,++serial+'.json'),workers:2}),new RegExp(expected));
 }
});
test('full native dip grid preserves causal history gaps and sequential accounting',async()=>{
 const raw=fixtureCapture({gapAfter:65,gapMs:1800000}),records=verifySource(raw),sha=createHash('sha256').update(raw).digest('hex');
 const out=path.join(root,++serial+'.json');const n=await sweepNativeCapture({journal:file(raw,'.jsonl'),config:file(c),endByte:Buffer.byteLength(raw),expectedSha:sha,out,workers:4});
 assert.equal(n.sweep.candidates,2304);assert.equal(n.sweep.profitabilityEstablished,false);assert.ok(fs.existsSync(out+'.receipt.json'));
 for(const gap of [120,900,3600,21600]){
  const operations=[],h=[];for(const [i,r]of records.entries()){
   const snap=r.event.sourceSnapshots[0],b=normalizeBook(snap.data,'377',c.tournamentId,snap.receivedAt,snap.requestStartedAt,c.maxDataAgeMs);
   if(h.length&&Date.parse(b.receivedAt)-h.at(-1).at>gap*1000)h.length=0;
   const mid=(Number(amount(b.bids[0].price))+Number(amount(b.asks[0].price)))/2e6;
   let netEdge=null;if(h.length>=6){const mean=h.reduce((n,x)=>n+x.mid,0)/h.length,sd=Math.sqrt(h.reduce((n,x)=>n+(x.mid-mean)**2,0)/h.length);if(sd>0&&(mean-mid)/sd>=0)netEdge=mean-(Number(amount(b.asks[0].price))+2*Number(TICK)*c.slippageTicks)/1e6;}
   operations.push({...step(b,netEdge,60000),takeProfit:'0.005000'});h.push({at:Date.parse(b.receivedAt),mid});if(h.length>6)h.shift();
   if([71,95,119].includes(i))operations.push({kind:'point',at:r.at});
  }
  const expected=compare(c,operations).points;
  const card=n.sweep.scorecards.find(x=>x.parameters.gapResetSeconds===gap&&x.parameters.lookback===6&&x.parameters.z===0&&x.parameters.minimumEdge===.005&&x.parameters.horizonSeconds===60);
  assert.deepEqual(card.points.map(({signalMetrics,unsupportedInstruments,...p})=>p),expected);
 }
});
