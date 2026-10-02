import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {updateContext,contextFeatures} from './breadth-context.mjs';
import {captureRows} from './ml-bank.mjs';
import {advanceShadow as productionAdvanceShadow} from './ml-shadow.mjs';
import {digest} from './model.mjs';
const c=JSON.parse(fs.readFileSync(new URL('./config.json',import.meta.url))),zero=Date.parse('2026-10-02T01:00:00Z'),at=n=>new Date(zero+n*1000).toISOString();
const bulk=n=>({tournamentId:c.tournamentId,receivedAt:at(n),executable:false,depthAvailable:false,quotes:[{exchangeId:'1066',marketId:'377',bestBid:.5,bestAsk:.51}],missingIds:[]});
const book=n=>({exchangeId:'1066',marketId:'377',requestStartedAt:at(n),bids:[{price:'0.400000'}],asks:[{price:'0.410000'}]});
test('context rejects future receipts, scope changes and invalid prices; stale and missing data stay explicit',()=>{
 const s=updateContext(null,bulk(10),at(10));assert.equal(contextFeatures(s,book(9),c.tournamentId)[6],1);assert.equal(contextFeatures(s,book(311),c.tournamentId)[6],1);assert.ok(Math.abs(contextFeatures(s,book(20),c.tournamentId)[0]-.1)<1e-10);
 assert.throws(()=>updateContext(null,bulk(10),at(9)),/Future/);assert.throws(()=>contextFeatures(s,book(20),'wrong'),/tournament/);assert.throws(()=>updateContext(s,{...bulk(20),tournamentId:'wrong'}),/scope/);
 const bad=bulk(20);bad.quotes[0].bestBid=NaN;assert.throws(()=>updateContext(s,bad),/price/);
 const gone=updateContext(s,{...bulk(20),quotes:[],missingIds:['1066']});assert.equal(contextFeatures(gone,book(21),c.tournamentId)[6],1);
});
test('training and shadow use previous-cycle breadth only; future bulk perturbation cannot affect earlier features',()=>{
 let previous='';const records=Array.from({length:8},(_,i)=>{const n=(i+1)*30,time=at(n),body={sequence:i+1,previous,at:at(n+1),event:{type:'cycle',clockAt:at(n+1),accountExclusions:[],bulkPrices:bulk(n+1),sourceSnapshots:[{endpoint:'/markets/377/orderbook?tournamentId='+c.tournamentId,requestStartedAt:time,receivedAt:time,data:{contexts:[{type:'tournament',tournament:{id:c.tournamentId},orderbook:{exchanges:[{exchangeId:'1066',option:'YES',asOf:{sequence:i+1,at:time},bids:[{price:.4,quantity:10000}],asks:[{price:.41,quantity:10000}]}]}}]}}]}};const r={...body,hash:digest(body)};previous=r.hash;return r;});
 const raw=records.map(r=>JSON.stringify(r)).join('\n')+'\n',rows=captureRows(raw,c,{breadthContext:true}).rows;assert.equal(rows[0].x.length,19);assert.equal(rows[0].x[18],1);assert.ok(Math.abs(rows[1].x[12]-.1)<1e-10);
 const model={id:'context',horizon:60,weights:Array.from({length:19},(_,i)=>i===12?1:0),means:Array(19).fill(0),scales:Array(19).fill(1),rmse:0};const bank={config:c,breadthContext:true,trainingCaptureThrough:at(0),selected:[model]},state={bankHash:digest(bank),sourceSequence:0,sourceHash:null,wallets:{},histories:{},predictions:{}};
 const shadow=advanceShadow(state,records,bank);assert.ok(Math.abs(Object.values(shadow.predictions)[0].exitValue-.505)<1e-10);
 const changed=structuredClone(records);changed.at(-1).event.bulkPrices.quotes[0].bestBid=.8;changed.at(-1).event.bulkPrices.quotes[0].bestAsk=.9;let hash='';for(const r of changed){r.previous=hash;const {hash:old,...body}=r;r.hash=digest(body);hash=r.hash;}
 assert.deepEqual(captureRows(changed.map(r=>JSON.stringify(r)).join('\n')+'\n',c,{breadthContext:true}).rows.map(r=>r.x),rows.map(r=>r.x));
});

// Explicitly unbound synthetic fixtures; production never passes this option.
const advanceShadow=(...args)=>productionAdvanceShadow(...args,{allowUnboundFixture:true});
