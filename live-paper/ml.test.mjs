import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {fitRidge,fitStumps,predict,features,trainBank} from './ml-bank.mjs';
import {createHash} from 'node:crypto';
import {initialState,stepForecast,assertState,report,digest} from './model.mjs';
import {advanceShadow,verifySource,combineReports} from './ml-shadow.mjs';
const c=JSON.parse(fs.readFileSync(new URL('./config.json',import.meta.url)));
const start=Date.parse('2026-10-01T23:00:00Z'),at=n=>new Date(start+n*1000).toISOString();
function book(n){return {marketId:'377',exchangeId:'9',instrument:'9:YES',sequence:String(n),sourceAt:at(n),receivedAt:at(n),requestStartedAt:at(n),versionKey:n+':'+at(n),bids:[{price:'0.400000',quantity:10000}],asks:[{price:'0.410000',quantity:10000}]};}
test('ridge normalization uses training only and fits a known relationship; degenerate data regularized',()=>{
 const rows=Array.from({length:40},(_,i)=>({x:[1,i/100],y:.01+i*.002}));const model=fitRidge(rows,2,.01);assert.ok(Math.abs(predict(model,[1,.5])-.11)<.001);assert.equal(model.means[0],0);assert.ok(Math.abs(model.means[1]-.195)<1e-12);assert.equal(model.weights.length,2);
 const constant=fitRidge(rows.map(r=>({x:[1,0],y:r.y})),2,1);assert.ok(Number.isFinite(predict(constant,[1,0])));assert.throws(()=>fitRidge(rows.slice(0,5),2,1),/Insufficient/);
});
test('ML features contain only current book and prior observations',()=>{
 const b=book(30),h=[{at:at(0),mid:.395}];const x=features(b,h);assert.equal(x.length,12);assert.ok(Math.abs(x[2]-.01)<1e-10);assert.equal(x[10],30);assert.equal(features(b,[])[2],0);
});
test('regularized stumps learn nonlinear training structure with finite constant-data fallback',()=>{
 const rows=Array.from({length:80},(_,i)=>({x:[1,i/80],y:i<40?-.03:.04})),m=fitStumps(rows,2,64,.1,5);
 assert.ok(predict(m,[1,.1])<-.015);assert.ok(predict(m,[1,.9])>.02);assert.ok(m.stumps.every(s=>s.threshold>=0&&s.threshold<=1));
 const flat=fitStumps(rows.map(r=>({x:[1,0],y:.02})),2,16,.05,5);assert.equal(flat.stumps.length,0);assert.ok(Math.abs(predict(flat,[1,9])-.02)<1e-12);
});
test('cohort aggregation keeps wallets distinct and reports a failed cohort as degraded',()=>{
 const r={status:'running',modelsAttempted:128,modelsTrained:96,candidateWallets:1,candidates:[{id:'same',cash:'100000'}]};const all=combineReports(r,[{name:'nonlinear',report:{...r,status:'failed'}}]);assert.equal(all.modelsTrained,192);assert.equal(all.candidateWallets,2);assert.notEqual(all.candidates[0].id,all.candidates[1].id);assert.equal(all.status,'degraded');assert.equal(r.candidates[0].id,'same');
});
test('forecast wallets charge entry/exit costs, fill only later, survive FIFO and reject duplicate fills',()=>{
 const policy={exitValue:.5,horizonMs:30000,minimumEdge:.005};let s=initialState(c,'ml',at(0));s=stepForecast(s,book(1),c,policy);assert.equal(s.strategies.mean_reversion.fills.length,0);s=stepForecast(s,book(31),c,policy);assert.equal(s.strategies.mean_reversion.fills.length,1);assert.equal(s.strategies.mean_reversion.cash,'99995.845850');s=stepForecast(s,book(61),c,policy);s=stepForecast(s,book(91),c,policy);assertState(s,c);assert.equal(s.strategies.mean_reversion.fills.length,2);assert.equal(s.strategies.mean_reversion.realized,'-0.208100');assert.equal(report(s,c,at(91)).strategies.mean_reversion.closedTrades,1);
 const count=s.strategies.mean_reversion.fills.length;const repeated={...book(92),sourceAt:at(91),sequence:'91',versionKey:'91:'+at(91)};s=stepForecast(s,repeated,c,policy);assert.equal(s.strategies.mean_reversion.fills.length,count);
});
test('no future-only signal or insufficient cost edge manufactures an entry',()=>{
 for(const exitValue of [null,NaN,.415,-1,2]){const s=stepForecast(initialState(c,'ml',at(0)),book(1),c,{exitValue,horizonMs:30000,minimumEdge:.005});assert.equal(s.strategies.mean_reversion.intents,0);}
});
test('shadow replay detects frozen-bank changes and source-prefix substitution on resume',()=>{
 const bank={config:c,selected:[],trainingCaptureThrough:at(0)},s={bankHash:digest(bank),sourceSequence:1,sourceHash:'original',wallets:{},histories:{},predictions:{}};
 assert.throws(()=>advanceShadow(s,[{sequence:1,hash:'replacement'}],bank),/prefix/);assert.throws(()=>advanceShadow({...s,bankHash:'other'},[],bank),/bank/);
});
test('partial capture waits without processing; complete corruption and backward clocks fail',()=>{
 const body={sequence:1,previous:'',at:at(0),event:{type:'started'},state:{}},hash=digest(body),raw=JSON.stringify({...body,hash})+'\n';
 assert.equal(verifySource(raw.slice(0,-1)),null);assert.equal(verifySource(''),null);assert.equal(verifySource(raw).length,1);
 assert.throws(()=>verifySource(raw.replace('started','changed')),/integrity/);
 const next={sequence:2,previous:hash,at:at(-1),event:{type:'stopped'},state:{}};assert.throws(()=>verifySource(raw+JSON.stringify({...next,hash:digest(next)})+'\n'),/clock rollback/);
});
test('candidate cost telemetry counts unique decisions and resumed source batches exactly once',()=>{
 const bank={config:c,trainingCaptureThrough:at(0),selected:[{id:'constant',horizon:60,weights:[.1],means:[0],scales:[1],rmse:.02}]};
 const records=Array.from({length:7},(_,i)=>{const n=i+1,b=book(n*30),data={contexts:[{type:'tournament',tournament:{id:c.tournamentId},orderbook:{exchanges:[{exchangeId:'9',option:'YES',asOf:{at:b.sourceAt,sequence:n*30},bids:[{price:.4,quantity:10000}],asks:[{price:.41,quantity:10000}]}]}}]};return {sequence:n,hash:'record'+n,at:b.receivedAt,event:{type:'cycle',clockAt:b.receivedAt,accountExclusions:[],sourceSnapshots:[{endpoint:'/markets/377/orderbook?tournamentId='+c.tournamentId,requestStartedAt:b.requestStartedAt,receivedAt:b.receivedAt,data}]}};});
 const initial={bankHash:digest(bank),sourceSequence:0,sourceHash:null,wallets:{},histories:{},predictions:{}},s=advanceShadow(initial,records,bank);
 for(const m of Object.values(s.signalMetrics)){assert.equal(m.observations,7);assert.equal(m.warmup,6);assert.equal(m.edgePass,1);}
 assert.deepEqual(advanceShadow(s,records,bank),s);
 for(const p of Object.values(s.predictions)){assert.ok(p.netEdge>.08);assert.ok(p.entryCost>.415);}
});
test('future development observations cannot change fitted weights or training normalization',()=>{
 function capture(changed){let previous='';return Array.from({length:180},(_,n)=>{
  const time=at(n*30),bid=changed&&n>=117?.7:.4+(n%3)*.005;
  const data={contexts:[{type:'tournament',tournament:{id:c.tournamentId},orderbook:{exchanges:[{exchangeId:'9',option:'YES',asOf:{at:time,sequence:n},bids:[{price:Number(bid.toFixed(3)),quantity:10000}],asks:[{price:Number((bid+.01).toFixed(3)),quantity:10000}]}]}}]};
  const body={sequence:n+1,previous,at:time,state:{},event:{type:'cycle',clockAt:time,accountExclusions:[],sourceSnapshots:[{endpoint:'/markets/377/orderbook?tournamentId='+c.tournamentId,receivedAt:time,requestStartedAt:time,data}]}};const hash=createHash('sha256').update(JSON.stringify(body)).digest('hex');previous=hash;return JSON.stringify({...body,hash});
 }).join('\n')+'\n';}
 for(const family of ['ridge','boosted_stumps']){
  const first=trainBank(capture(false),c,{family}),changed=trainBank(capture(true),c,{family});assert.equal(first.modelsTrained,128);
  for(const m of first.models){const other=changed.models.find(x=>x.id===m.id);assert.deepEqual(other.weights,m.weights);assert.deepEqual(other.means,m.means);assert.deepEqual(other.scales,m.scales);assert.deepEqual(other.stumps,m.stumps);assert.equal(other.base,m.base);assert.ok(Date.parse(m.maxTrainingLabelAt)<Date.parse(first.developmentSplitAt));}
 }
});


