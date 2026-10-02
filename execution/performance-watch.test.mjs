import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {digest} from '../live-paper/model.mjs';
import {createPerformanceCapture,openPerformanceTail} from './performance-watch.mjs';

const tournamentId='bda92870-621e-47b0-bc3c-3602c5c26f55';
const base=Date.parse('2026-10-02T12:00:00.000Z'),at=ms=>new Date(base+ms).toISOString();
const universe={tournamentId,exchanges:[{marketId:'1',exchangeId:'101',option:'YES'},{marketId:'2',exchangeId:'102',option:'YES'}]};
const exclusionReason='Exchange has real account holdings or open orders';
function row(sequence,previous,event,ms){const body={sequence,previous,at:at(ms),event,state:{fixture:true}};return {...body,hash:digest(body)};}
function snapshot(marketId,ms,sequence=1){return {
  endpoint:`/markets/${marketId}/orderbook?tournamentId=${tournamentId}&depth=20`,requestStartedAt:at(ms-100),receivedAt:at(ms),
  data:{contexts:[{type:'tournament',tournament:{id:tournamentId},orderbook:{exchanges:[{
    exchangeId:marketId==='1'?'101':'102',option:'YES',asOf:{sequence,at:at(ms-200)},
    bids:[{price:.4,quantity:100}],asks:[{price:.42,quantity:100}],
  }]}}]},
};}
function fixture(){
  const capture=createPerformanceCapture(universe);let sequence=0,previous='';
  const consume=(event,ms)=>{const record=row(++sequence,previous,event,ms);capture.consume(record);previous=record.hash;return record;};
  const cycle=(rows,ms,extra={})=>consume({type:'cycle',clockAt:at(ms),sourceSnapshots:rows,supplementalDepth:[],accountExclusions:[],...extra},ms);
  return {capture,consume,cycle};
}
function temporary(t){
  const dir=fs.mkdtempSync(path.join(os.tmpdir(),'sig-performance-test-'));
  t.after(()=>{const target=path.resolve(dir);if(path.dirname(target)!==path.resolve(os.tmpdir())||!path.basename(target).startsWith('sig-performance-test-'))throw Error('Unsafe test cleanup');fs.rmSync(target,{recursive:true,force:true});});
  return dir;
}

test('source hash, sequence and predecessor must all match before projection advances',()=>{
  const f=fixture(),first=f.cycle([snapshot('1',1000)],2000),before=structuredClone(f.capture.status());
  const next=row(2,first.hash,{type:'market_history',clockAt:at(3000)},3000);
  for(const bad of [{...next,hash:'0'.repeat(64)},row(3,first.hash,next.event,3000),row(2,'f'.repeat(64),next.event,3000),{...next,event:{...next.event,type:'changed'}}]){
    assert.throws(()=>f.capture.consume(bad),/SOURCE_INTEGRITY_FAILURE/);assert.deepEqual(f.capture.status(),before);
  }
  f.capture.consume(next);assert.equal(f.capture.status().sourceSequence,2);assert.equal(f.capture.status().sourceHash,next.hash);
});

test('incremental tail waits for complete UTF-8 lines, consumes appended records once, and replays identically',t=>{
  const file=path.join(temporary(t),'journal.jsonl'),first=row(1,'',{type:'started',clockAt:at(0),label:'résumé'},0),second=row(2,first.hash,{type:'cycle',clockAt:at(2000),sourceSnapshots:[snapshot('1',1000)]},2000);
  const bytes=Buffer.from(JSON.stringify(first)+'\n'),split=bytes.indexOf(Buffer.from('é'))+1;
  fs.writeFileSync(file,bytes.subarray(0,split));const capture=createPerformanceCapture(universe),poll=openPerformanceTail(file,capture);
  assert.equal(poll(),false);assert.equal(capture.status().sourceSequence,0);
  fs.appendFileSync(file,bytes.subarray(split));assert.equal(poll(),false);assert.equal(capture.status().sourceSequence,1);
  fs.appendFileSync(file,JSON.stringify(second));poll();assert.equal(capture.status().sourceSequence,1);
  fs.appendFileSync(file,'\n');poll();assert.equal(capture.status().sourceSequence,2);const expected=structuredClone(capture.status());
  poll();assert.deepEqual(capture.status(),expected);
  const restarted=createPerformanceCapture(universe),replay=openPerformanceTail(file,restarted);while(replay()){}assert.deepEqual(restarted.status(),expected);
});

test('truncated or replaced source fails before it can silently restart the projection',t=>{
  for(const action of ['truncate','replace']){
    const dir=temporary(t),file=path.join(dir,'journal.jsonl'),first=row(1,'',{type:'started',clockAt:at(0)},0);
    fs.writeFileSync(file,JSON.stringify(first)+'\n');const capture=createPerformanceCapture(universe),poll=openPerformanceTail(file,capture);poll();
    if(action==='truncate')fs.truncateSync(file,0);
    else{fs.renameSync(file,path.join(dir,'original.jsonl'));fs.writeFileSync(file,JSON.stringify(first)+'\n');}
    assert.throws(poll,/SOURCE_REPLACED_OR_TRUNCATED/);assert.equal(capture.status().sourceHash,first.hash);
  }
});

test('tampered completed tail and empty records cannot become successful observations',t=>{
  for(const payload of ['\n','{"sequence":1,"hash":"invalid"}\n']){
    const file=path.join(temporary(t),'journal.jsonl');fs.writeFileSync(file,payload);
    const capture=createPerformanceCapture(universe),poll=openPerformanceTail(file,capture);assert.throws(poll);assert.equal(capture.status().sourceSequence,0);
  }
});

test('receipt order merges supplemental and core snapshots before version validation',()=>{
  const f=fixture();f.cycle([snapshot('1',8000,2)],10000,{supplementalDepth:[snapshot('1',6000,1)]});
  const [book]=f.capture.status().books;assert.equal(book.sequence,'2');assert.equal(book.receivedAt,at(8000));
});

test('malformed and future snapshot clocks invalidate after valid rows without affecting other markets',()=>{
  for(const clock of ['bad',at(11000)]){
    const f=fixture();f.cycle([snapshot('1',8000,2),{marketId:'1',skipped:'http_503',at:clock},snapshot('2',9000)],10000,{supplementalDepth:[snapshot('1',6000,1)]});
    assert.deepEqual(f.capture.status().books.map(b=>b.exchangeId),['102']);
  }
});

test('genuine skips and account outages clear marks and retain version rollback protection',()=>{
  const f=fixture();f.cycle([snapshot('1',1000,3),snapshot('2',1200,3)],2000);
  f.cycle([{marketId:'1',skipped:'http_503',at:at(2500)}],3000);assert.deepEqual(f.capture.status().books.map(b=>b.exchangeId),['102']);
  f.consume({type:'account_read_outage',clockAt:at(4000)},4000);assert.deepEqual(f.capture.status().books,[]);
  f.cycle([snapshot('1',4500,2)],5000);assert.deepEqual(f.capture.status().books,[]);
  f.cycle([snapshot('1',5500,4)],6000);assert.equal(f.capture.status().books.length,1);
});

test('quiet unchanged book versions refresh authoritative receipt without inventing a new version',()=>{
  const f=fixture(),first=snapshot('1',1000),quiet=snapshot('1',30000);
  quiet.data.contexts[0].orderbook.exchanges[0].asOf=structuredClone(first.data.contexts[0].orderbook.exchanges[0].asOf);
  f.cycle([first],2000);const oldKey=f.capture.status().books[0].versionKey;
  f.cycle([quiet],31000);const [book]=f.capture.status().books;
  assert.equal(book.receivedAt,at(30000));assert.equal(book.versionKey,oldKey);assert.equal(book.freshnessPolicy,'authoritative_receipt_v1');
  const changed=structuredClone(quiet);changed.receivedAt=at(32000);changed.requestStartedAt=at(31900);changed.data.contexts[0].orderbook.exchanges[0].bids[0].quantity=99;
  f.cycle([changed],33000);assert.deepEqual(f.capture.status().books,[]);
});

test('wrong tournament, endpoint, or exchange invalidates known-market books without admitting foreign depth',()=>{
  for(const mutate of [b=>{b.endpoint=b.endpoint.replace(tournamentId,'foreign');},b=>{b.data.contexts[0].tournament.id='foreign';},b=>{b.data.contexts[0].orderbook.exchanges[0].exchangeId='999';}]){
    const f=fixture();f.cycle([snapshot('1',1000),snapshot('2',1200)],2000);
    const wrong=snapshot('1',3000,2);mutate(wrong);f.cycle([wrong],4000);
    assert.deepEqual(f.capture.status().books.map(b=>b.exchangeId),['102']);
  }
});

test('own-position book survives the exact adjacent paper-exclusion marker but not a genuine read failure',()=>{
  const f=fixture(),valid=snapshot('1',1000);
  f.cycle([valid,{marketId:'1',skipped:exclusionReason,at:at(1001)}],2000,{accountExclusions:['101']});
  assert.equal(f.capture.status().books.length,1);assert.equal(f.capture.status().books[0].exchangeId,'101');
  f.cycle([{marketId:'1',skipped:'http_503',at:at(2500)}],3000,{accountExclusions:['101']});assert.deepEqual(f.capture.status().books,[]);
});

test('paper-exclusion marker alone cannot rehabilitate a cached book or bypass invalid same-cycle data',()=>{
  const f=fixture();f.cycle([snapshot('1',1000)],2000);
  f.cycle([{marketId:'1',skipped:exclusionReason,at:at(2500)}],3000,{accountExclusions:['101']});assert.deepEqual(f.capture.status().books,[]);
  const wrong=snapshot('1',4000,2);wrong.data.contexts[0].tournament.id='foreign';
  f.cycle([wrong,{marketId:'1',skipped:exclusionReason,at:at(4001)}],5000,{accountExclusions:['101']});assert.deepEqual(f.capture.status().books,[]);
});

test('hash-valid source clock rollback cannot move the projection clock backward',()=>{
  const f=fixture(),first=f.cycle([snapshot('1',1000)],2000),before=structuredClone(f.capture.status());
  for(const clock of [at(1999),'bad']){
    const record=row(2,first.hash,{type:'market_history',clockAt:clock},3000);
    assert.throws(()=>f.capture.consume(record),/SOURCE_CLOCK/);assert.deepEqual(f.capture.status(),before);
  }
});

test('an in-flight request from before invalidation cannot restore a mark',()=>{
  const f=fixture();f.cycle([snapshot('1',1000)],2000);f.consume({type:'account_read_outage',clockAt:at(3000)},3000);
  const oldRequest=snapshot('1',3500,2);oldRequest.requestStartedAt=at(2900);
  f.cycle([oldRequest],4000);assert.deepEqual(f.capture.status().books,[]);
  f.cycle([snapshot('1',4500,3)],5000);assert.equal(f.capture.status().books.length,1);
});

test('isolated worker requires a fresh owned execution projection and reports zero network or order activity',t=>{
  for(const age of [0,61000]){
    const dir=temporary(t),now=Date.now(),source={sequence:1,previous:'',at:new Date(now-100).toISOString(),event:{type:'started',clockAt:new Date(now-100).toISOString()},state:{fixture:true}};
    fs.writeFileSync(path.join(dir,'journal.jsonl'),JSON.stringify({...source,hash:digest(source)})+'\n');
    fs.writeFileSync(path.join(dir,'competition-stats.json'),JSON.stringify({mode:'official_quant_execution',owner:'Codex',runId:'test-only',addedFees:'0.000000',updatedAt:new Date(now-age).toISOString(),positions:{},fills:[]}));
    const run=spawnSync(process.execPath,[fileURLToPath(new URL('./performance-watch.mjs',import.meta.url)),'--once'],{env:{...process.env,SIG_PAPER_DIR:dir},encoding:'utf8',timeout:10000});
    assert.equal(run.status,0,run.stderr);const stats=JSON.parse(fs.readFileSync(path.join(dir,'performance-stats.json')));
    assert.equal(stats.status,age?'unavailable':'running');assert.equal(stats.networkRequests,0);assert.equal(stats.ordersSubmitted,0);assert.equal(stats.sourceHash,digest(source));
    assert.equal(fs.existsSync(path.join(dir,'performance-watch.lock')),false);
  }
});

test('isolated own-fill worker values the valid depth retained through its paper-only exclusion',t=>{
  const dir=temporary(t),now=Date.now(),stamp=ms=>new Date(now+ms).toISOString();
  const book=snapshot('1',1000);
  book.endpoint=`/markets/369/orderbook?tournamentId=${tournamentId}&depth=20`;
  book.requestStartedAt=stamp(-2000);book.receivedAt=stamp(-1000);
  Object.assign(book.data.contexts[0].orderbook.exchanges[0],{exchangeId:'1058',asOf:{sequence:5,at:stamp(-1500)},bids:[{price:.235,quantity:100}],asks:[{price:.240,quantity:100}]});
  const source={sequence:1,previous:'',at:stamp(-500),event:{type:'cycle',clockAt:stamp(-500),accountExclusions:['1058'],sourceSnapshots:[],supplementalDepth:[book,{marketId:'369',skipped:exclusionReason,at:stamp(-999)}]},state:{fixture:true}};
  const execution={mode:'official_quant_execution',owner:'Codex',runId:'test-owned-fill',addedFees:'0.000000',updatedAt:stamp(0),positions:{'1058':{marketId:'369',quantity:16,cost:'3.920000',lots:[{quantity:16,cost:'3.920000'}]}},fills:[{id:1,orderId:100,marketId:'369',exchangeId:'1058',action:'buy',quantity:16,price:'0.245000',filledAt:stamp(-5000)}]};
  fs.writeFileSync(path.join(dir,'journal.jsonl'),JSON.stringify({...source,hash:digest(source)})+'\n');fs.writeFileSync(path.join(dir,'competition-stats.json'),JSON.stringify(execution));
  const run=spawnSync(process.execPath,[fileURLToPath(new URL('./performance-watch.mjs',import.meta.url)),'--once'],{env:{...process.env,SIG_PAPER_DIR:dir},encoding:'utf8',timeout:10000});
  assert.equal(run.status,0,run.stderr);const stats=JSON.parse(fs.readFileSync(path.join(dir,'performance-stats.json')));
  assert.equal(stats.status,'running');assert.equal(stats.observedLiquidationTotalPnl,'-0.160000');assert.equal(stats.midpointTotalPnl,'-0.120000');assert.equal(stats.realizedPnl,'0.000000');
  assert.equal(stats.coverage.validBooks,1);assert.equal(stats.positions[0].book.receivedAt,book.receivedAt);assert.equal(stats.executionHash,digest(execution));
  assert.equal(stats.networkRequests,0);assert.equal(stats.ordersSubmitted,0);assert.equal(stats.executionGuaranteed,false);
});
