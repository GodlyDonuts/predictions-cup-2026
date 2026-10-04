import test from 'node:test';import assert from 'node:assert/strict';import fs from 'node:fs';import os from 'node:os';import path from 'node:path';
import {normalizeTradeTape,tradeTapeEndpoint,createTradeTape,selectAuxiliaryReads} from './trade-tape.mjs';
import {ReadClient} from './transport-runtime.mjs';
import {Journal} from './transport-runtime.mjs';import {digest} from './model.mjs';
const tid='bda92870-621e-47b0-bc3c-3602c5c26f55',time=Date.parse('2026-10-03T23:00:00.000Z');
const universe={tournamentId:tid,exchanges:[{marketId:'363',exchangeId:'1052',option:'YES'},{marketId:'297',exchangeId:'986',option:'YES'}]};
const at=n=>new Date(time+n).toISOString();
test('an incomplete page keeps the original window across restart until every page is complete',async t=>{
 const f=fixture(t);let h=f.host(),calls=[];
 const dispatch=async s=>{calls.push(s.endpoint);const r=response(s.endpoint,[],calls.length===1,calls.length===1?'A':null);if(calls.length===1)r.data.coverage.complete=false;return r;};
 await h.collect(dispatch,['363']);h=f.host();f.setTime(4000);await h.collect(dispatch,['363']);
 h=f.host();f.setTime(8000);await h.collect(async s=>{calls.push(s.endpoint);return response(s.endpoint,[],false);},['363']);
 for(const endpoint of calls){const u=new URL('https://example.test'+endpoint);assert.equal(u.searchParams.get('from'),at(-1800000));assert.equal(u.searchParams.get('to'),at(0));}
 assert.equal(new URL('https://example.test'+calls[2]).searchParams.get('cursor'),null,'retry starts at the first page');
 h=f.host();f.setTime(12000);await h.collect(async s=>{assert.equal(new URL('https://example.test'+s.endpoint).searchParams.get('from'),at(-60000));return response(s.endpoint,[],false);},['363']);
 assert.equal(h.status().pages,4);
});
const trade=(id='1',offset=-1000,side='NO')=>({id,createdAt:at(offset),price:.9,size:2,side,volume:.2});
function response(endpoint,trades=[trade()],more=false,next=null){const u=new URL('https://example.test'+endpoint);return {endpoint,status:200,requestStartedAt:new Date(Math.max(time,Date.parse(u.searchParams.get('to')))).toISOString(),receivedAt:new Date(Math.max(time,Date.parse(u.searchParams.get('to')))+100).toISOString(),headers:{Authorization:'NEVER_RECORD'},data:{exchangeId:'1052',marketId:'363',from:u.searchParams.get('from'),to:u.searchParams.get('to'),data:trades,pagination:{limit:200,hasMore:more,nextCursor:next},coverage:{complete:true,projectedThroughSequence:123},rawAccount:'NEVER_RECORD'}};}
function fixture(t){const runtimeDir=fs.mkdtempSync(path.join(os.tmpdir(),'sig-tape-'));t.after(()=>fs.rmSync(runtimeDir,{recursive:true,force:true}));let clock=time,allowed=true;return {runtimeDir,host:()=>createTradeTape({runtimeDir,universe,now:()=>clock,canOperate:()=>allowed}),setTime:x=>clock=time+x,stop:()=>allowed=false};}
test('YES-normalized price stays distinct from executed space and aggressor; legacy nulls/IDs retain source clock',()=>{
 const spec={...universe.exchanges[0],tournamentId:tid,from:at(-1800000),to:at(0),cursor:null},r=response(tradeTapeEndpoint(spec),[trade(),{...trade('1',-2000,'YES'),price:null}]);
 const p=normalizeTradeTape(r,spec);assert.equal(p.trades[0].yesPrice,'0.9');assert.equal(p.trades[0].executedSpace,'NO');assert.equal(p.trades[0].aggressorSide,null);assert.notEqual(p.trades[0].key,p.trades[1].key);assert.equal(p.trades[1].yesPrice,null);assert.equal(p.firstAvailableAt,r.receivedAt);assert.equal(p.completeTapeProven,false);assert.equal(p.counterfactualFillsProven,false);assert.ok(!JSON.stringify(p).includes('NEVER_RECORD'));
});
test('foreign scopes, future/end-exclusive clocks, malformed prices/depth/cursors and unsorted pages rejected',()=>{
 const spec={...universe.exchanges[0],tournamentId:tid,from:at(-1800000),to:at(0),cursor:null};
 for(const mutate of [r=>r.data.exchangeId='986',r=>r.data.marketId='297',r=>r.endpoint+='&extra=true',r=>r.requestStartedAt=at(-1),r=>r.data.data[0].createdAt=at(0),r=>r.data.data[0].size=-1,r=>r.data.data[0].price=1.1,r=>r.data.data[0].side='BUY',r=>r.data.pagination.hasMore=true,r=>r.data.coverage.projectedThroughSequence=-1,r=>r.data.data.push(trade('2',-500))]){const r=response(tradeTapeEndpoint(spec));mutate(r);assert.throws(()=>normalizeTradeTape(r,spec));}
});
test('one slot resumes fixed-window pagination after restart, deduplicates overlap, and records only public novel trades',async t=>{
 const f=fixture(t);let h=f.host();let calls=[];
 const dispatch=async spec=>{assert.equal(h.authorizes(spec),true);assert.equal(h.authorizes({...spec,method:'POST'}),false);calls.push(spec.endpoint);return calls.length===1?response(spec.endpoint,[trade()],true,'opaque/next+1'):calls.length===2?response(spec.endpoint,[trade('2',-2000)],false):response(spec.endpoint,[trade(),trade('2',-2000)],false);};
 await h.collect(dispatch,['363']);assert.equal(h.status().markets[0].paginationPending,true);h=f.host();f.setTime(2000);
 await h.collect(dispatch,['363']);assert.equal(new URL('https://example.test'+calls[1]).searchParams.get('cursor'),'opaque/next+1');assert.equal(new URL('https://example.test'+calls[1]).searchParams.get('to'),at(0));
 await h.collect(dispatch,['363']);assert.equal(h.status().pages,3);assert.equal(h.status().tradesCaptured,2);assert.equal(h.status().duplicates,2);assert.equal(h.authorizes({method:'GET',endpoint:calls[0]}),false);
 const raw=fs.readFileSync(path.join(f.runtimeDir,'anonymous-trades.jsonl'),'utf8'),rows=raw.trim().split('\n').map(JSON.parse);assert.equal(rows[2].event.trades.length,0);assert.ok(!raw.includes('NEVER_RECORD'));assert.equal(f.host().status().tradesCaptured,2);
});
test('duplicate identity changes and cursor cycles leave durable accounting/cursor unchanged',async t=>{
 const f=fixture(t);const h=f.host();await h.collect(async s=>response(s.endpoint,[trade()],true,'A'),['363']);const before=h.status();
 await assert.rejects(h.collect(async s=>response(s.endpoint,[{...trade(),size:3}],true,'B'),['363']),/IDENTITY_CONFLICT/);assert.deepEqual(h.status(),before);
 await h.collect(async s=>response(s.endpoint,[trade('2',-2000)],true,'B'),['363']);const second=h.status();
 await assert.rejects(h.collect(async s=>response(s.endpoint,[trade('3',-3000)],true,'A'),['363']),/CURSOR_CYCLE/);assert.deepEqual(h.status(),second);
});
test('STOP during receipt commits nothing and journal corruption rejects restart',async t=>{
 const f=fixture(t),h=f.host();await h.collect(async s=>{f.stop();return response(s.endpoint);},['363']);assert.equal(h.status().pages,0);assert.equal(fs.existsSync(path.join(f.runtimeDir,'anonymous-trades.jsonl')),false);
 fs.writeFileSync(path.join(f.runtimeDir,'anonymous-trades.jsonl'),'{}\n');assert.throws(()=>f.host(),/integrity/);
});
test('auxiliary trade pages replace history reads within every original cycle budget',()=>{
 for(const budget of [0,1,2])for(let cycle=0;cycle<100;cycle++){const s=selectAuxiliaryReads(budget,cycle);assert.equal(s.totalReads,budget);assert.equal(s.historyReads+s.tapeReads,budget);assert.ok(s.tapeReads<=1);assert.ok(s.historyReads>=0);}
 assert.deepEqual(selectAuxiliaryReads(1,0),{historyReads:0,tapeReads:1,totalReads:1});assert.deepEqual(selectAuxiliaryReads(1,1),{historyReads:1,tapeReads:0,totalReads:1});
});
test('trade pages use the existing read-only admission queue and preserve completion spacing and HTTP backoff',async t=>{
 const f=fixture(t),h=f.host();let now=time,fail=false;const starts=[];const budget={schemaVersion:3,ownerId:'Codex',enabled:true,intervalMs:3000,soleAccountClient:false,executionOwner:'Daenerys',ackCommentId:5966761902,maxReadsPerMinute:20,writesEnabled:false};
 const client=new ReadClient('unit-fixture',{networkBudget:budget,now:()=>now,sleepFn:async ms=>now+=ms,authorizeDispatch:s=>h.authorizes(s),fetchFn:async url=>{starts.push(now);now+=100;return fail?{ok:false,status:503,headers:{get:()=> '20'},json:async()=>({})}:{ok:true,status:200,json:async()=>{const r=response(url.slice('https://sig.thesuper.market/api/v1'.length));return r.data;}};}});
 await h.collect(s=>client.dispatch(s),['363']);await h.collect(s=>client.dispatch(s),['363']);assert.ok(starts[1]-starts[0]>=3100);assert.equal(client.requests,2);
 assert.throws(()=>client.dispatch({method:'POST',endpoint:'/orders',body:{}}),/forbids/);assert.throws(()=>client.dispatch({method:'GET',endpoint:'/exchanges/1052/trades'}),/not authorized/);
 fail=true;await assert.rejects(h.collect(s=>client.dispatch(s),['363']),/HTTP 503/);assert.ok(client.cooldown>=now+20000);assert.equal(h.status().pages,2);
});
test('failed attempts rotate across all markets and survive restart without altering successful receipts',async t=>{
 const f=fixture(t);let h=f.host(),attempts=[];
 for(let n=0;n<4;n++){f.setTime(n*4000);await assert.rejects(h.collect(async s=>{attempts.push(new URL('https://example.test'+s.endpoint).pathname);throw Error('HTTP 503');},['297','363']),/503/);h=f.host();}
 assert.deepEqual(attempts,['/exchanges/986/trades','/exchanges/1052/trades','/exchanges/986/trades','/exchanges/1052/trades']);assert.equal(h.status().pages,0);assert.equal(h.status().markets.length,0);
});
test('equivalent source UTC formatting deduplicates while preserving raw source clock and nanoseconds',async t=>{
 const f=fixture(t),h=f.host(),times=['2026-10-03T22:59:59Z','2026-10-03T22:59:59.000Z','2026-10-03T22:59:59.000000000Z'];
 for(const createdAt of times)await h.collect(async s=>response(s.endpoint,[{...trade(),createdAt}]),['363']);
 assert.equal(h.status().tradesCaptured,1);assert.equal(h.status().duplicates,2);assert.equal(JSON.parse(fs.readFileSync(path.join(f.runtimeDir,'anonymous-trades.jsonl'),'utf8').split('\n')[0]).event.trades[0].createdAt,times[0]);
 await h.collect(async s=>response(s.endpoint,[{...trade(),createdAt:'2026-10-03T22:59:59.000000001Z'}]),['363']);assert.equal(h.status().tradesCaptured,2,'nanosecond differences remain distinct');
 await assert.rejects(h.collect(async s=>response(s.endpoint,[{...trade(),createdAt:times[2],size:3}]),['363']),/IDENTITY_CONFLICT/);
});
test('legacy journal dedup migration preserves original prefix and does not redeliver retained prints',async t=>{
 const f=fixture(t),spec={...universe.exchanges[0],tournamentId:tid,from:at(-1800000),to:at(0),cursor:null},page=normalizeTradeTape(response(tradeTapeEndpoint(spec)),spec),tr=page.trades[0];
 tr.key=digest([spec.exchangeId,tr.id,tr.createdAt,tr.executedSpace]);
 const scope={tournamentId:tid,exchanges:universe.exchanges.map(({marketId,exchangeId})=>({marketId,exchangeId}))},state={schemaVersion:1,scopeHash:digest(scope),pages:1,tradesCaptured:1,duplicates:0,lastAt:page.receivedAt,
 markets:{1052:{marketId:'363',exchangeId:'1052',pages:1,tradesCaptured:1,seen:{[tr.key]:{at:tr.createdAt,hash:digest(tr)}},latestTradeAt:tr.createdAt,lastReceiptAt:page.receivedAt,completedTo:spec.to,window:null}}};
 const file=path.join(f.runtimeDir,'anonymous-trades.jsonl');new Journal(file).commit(state,{type:'anonymous_trade_page',...page,trades:[tr]});const prefix=fs.readFileSync(file);
 const h=f.host();await h.collect(async s=>response(s.endpoint,[{...trade(),createdAt:'2026-10-03T22:59:59Z'}]),['363']);assert.equal(h.status().tradesCaptured,1);assert.equal(h.status().duplicates,1);
 assert.deepEqual(fs.readFileSync(file).subarray(0,prefix.length),prefix);assert.equal(f.host().status().tradesCaptured,1);
});
