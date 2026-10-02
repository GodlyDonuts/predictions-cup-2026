import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {resetHistory,ISOLATED,LEGACY,historyPolicy} from './history-policy.mjs';
import {captureRows} from './ml-bank.mjs';
import {advanceShadow} from './ml-shadow.mjs';
import {digest} from './model.mjs';
const c=JSON.parse(fs.readFileSync(new URL('./config.json',import.meta.url)));
const at=n=>new Date(Date.parse('2026-10-02T01:00:00Z')+n*30000).toISOString();
function records(){let previous='';return Array.from({length:8},(_,i)=>{
 const time=at(i+1),snap={endpoint:'/markets/377/orderbook?tournamentId='+c.tournamentId,requestStartedAt:time,receivedAt:time,data:{contexts:[{type:'tournament',tournament:{id:c.tournamentId},orderbook:{exchanges:[{exchangeId:'1066',option:'YES',asOf:{at:time,sequence:i+1},bids:[{price:Number((.4+i*.005).toFixed(3)),quantity:10000}],asks:[{price:Number((.41+i*.005).toFixed(3)),quantity:10000}]}]}}]}};
 const body={sequence:i+1,previous,at:time,event:{type:'cycle',clockAt:time,accountExclusions:[],sourceSnapshots:[snap,{marketId:'354',skipped:'missing_depth',at:time}]}};const r={...body,hash:digest(body)};previous=r.hash;return r;
 });}
test('isolated policy keeps unrelated market features and labels continuous',()=>{
 const raw=records().map(r=>JSON.stringify(r)).join('\n')+'\n';
 const a=captureRows(raw,c,{historyPolicy:ISOLATED}).rows,b=captureRows(raw,c).rows;
 assert.equal(new Set(a.map(r=>r.segment)).size,1);assert.equal(new Set(b.map(r=>r.segment)).size,8);
 assert.ok(a[7].x[2]>.004);assert.equal(b[7].x[2],0);
 const h={'1:YES':[{marketId:'377'}],'2:YES':[{marketId:'354'}]};resetHistory(h,'354',ISOLATED);assert.ok(h['1:YES']);assert.equal(h['2:YES'],undefined);resetHistory(h,null,ISOLATED);assert.deepEqual(h,{});assert.throws(()=>historyPolicy('unknown'));
});
test('new-policy shadow warms unaffected market; legacy remains reset; account outage clears all',()=>{
 function run(policy){const bank={config:c,historyPolicy:policy,trainingCaptureThrough:at(0),selected:[{id:'flat',horizon:60,weights:[0],means:[0],scales:[1],rmse:.01}]};const initial={bankHash:digest(bank),sourceSequence:0,sourceHash:null,wallets:{},histories:{},predictions:{}};return {bank,state:advanceShadow(initial,records(),bank)};}
 const a=run(ISOLATED),b=run(LEGACY);assert.equal(Object.values(a.state.signalMetrics)[0].warmup,6);assert.equal(Object.values(b.state.signalMetrics)[0].warmup,8);
 const outage={sequence:9,hash:'outage',at:at(9),event:{type:'account_read_outage',clockAt:at(9)}};assert.deepEqual(advanceShadow(a.state,[...records(),outage],a.bank).histories,{});
});
