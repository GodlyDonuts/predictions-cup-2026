import {openEntryComparison as openStreaming} from './entry-comparison-watch-v2.mjs';
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {digest} from './model.mjs';
import {validateUniverse} from './breadth.mjs';
import {verifySource} from './ml-shadow.mjs';
import {historyEndpoint,normalizeHistory} from './history.mjs';
import {openEntryComparison,runEntryComparison} from './entry-comparison-watch.mjs';

const base=Date.parse('2026-10-02T10:00:00.000Z'),stamp=ms=>new Date(base+ms).toISOString();
const config=JSON.parse(fs.readFileSync(new URL('./config.json',import.meta.url))),tid=config.tournamentId;
const rawUniverse=JSON.parse(fs.readFileSync(new URL('./price-universe.json',import.meta.url)));
const universe=validateUniverse({tournamentId:tid,exchanges:rawUniverse.exchanges.filter(x=>config.marketIds.includes(x.marketId))},tid);
const exchange=universe.exchanges.find(x=>x.marketId==='377').exchangeId;
function record(sequence,previous,event,ms){const body={sequence,previous,at:stamp(ms),event,state:{fixture:true}};return {...body,hash:digest(body)};}
function book(ms,extra={}){return {endpoint:'/markets/377/orderbook?tournamentId='+tid+'&depth=20',requestStartedAt:stamp(ms-100),receivedAt:stamp(ms),data:{contexts:[{type:'tournament',tournament:{id:tid},orderbook:{exchanges:[{exchangeId:exchange,option:'YES',asOf:{at:stamp(ms-200),sequence:ms},bids:[{price:.495,quantity:10000}],asks:[{price:.5,quantity:10000}]}]}}]},...extra};}
function cycle(ms,extra={}){return {type:'cycle',clockAt:stamp(ms),sourceSnapshots:[book(ms-100)],supplementalDepth:[],accountExclusions:[],exclusionObservedAt:stamp(ms-500),...extra};}
function fixture(t){
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'sig-entry-watch-test-')),workspace=path.join(dir,'workspace');fs.mkdirSync(workspace);
 t.after(()=>{const target=path.resolve(dir);assert.equal(path.dirname(target),path.resolve(os.tmpdir()));assert.ok(path.basename(target).startsWith('sig-entry-watch-test-'));fs.rmSync(target,{recursive:true,force:true});});
 const policy={...JSON.parse(fs.readFileSync(new URL('./dip-policy.json',import.meta.url))),executionPolicy:'responsive_v2',requireScheduledEntry:true,maxOpenMarkets:3};
 fs.writeFileSync(path.join(dir,'dip-responsive-bank.json'),JSON.stringify({config:{...config,feeBps:0},policy,universe,identities:Object.fromEntries(config.marketIds.map(id=>[id,{state:'FL',office:'Senate'}]))}));
 const source=path.join(dir,'journal.jsonl'),records=[];let now=10000;
 function append(event,ms){const r=record(records.length+1,records.at(-1)?.hash||'',event,ms);fs.appendFileSync(source,JSON.stringify(r)+'\n');records.push(r);return r;}
 const history=normalizeHistory({endpoint:historyEndpoint(exchange,stamp(-100)),requestStartedAt:stamp(-100),receivedAt:stamp(0),data:{marketId:'377',exchangeId:exchange,resolution:'1m',from:stamp(-3600000),to:stamp(-60000),coverage:{complete:true,projectedThroughSequence:1},candles:Array.from({length:8},(_,i)=>({time:stamp(-(45-i*5)*60000),open:.55,high:.55,low:.55,close:.55,vwap:.55,volume:10,tradeCount:1}))}},{marketId:'377',exchangeId:exchange});
 append({type:'market_history',clockAt:stamp(0),history},0);
 append(cycle(1000),1000);append(cycle(3000),3000);
 return {dir,workspace,source,records,append,setNow(ms){now=ms;},open(cohort='first',initialize=false){return openEntryComparison({runtimeDir:dir,cohort,initialize,now:()=>base+now});},run(cohort='first',initialize=false){return runEntryComparison({runtimeDir:dir,cohort,initialize,once:true,now:()=>base+now,workspaceDir:workspace});},cohortDir(cohort='first'){return path.join(dir,'entry-comparisons',cohort);}};
}
const wallet=(watch,name='historical_dip')=>watch.sourceState().strategies[name].wallet.strategies.mean_reversion;

const streaming=f=>openStreaming({runtimeDir:f.dir,cohort:'first',now:()=>base+30000});
test('streaming successor replays every original checkpoint and preserves exact wallet state and bank',t=>{
 const f=fixture(t),old=f.open('first',true),bank=fs.readFileSync(path.join(f.cohortDir(),'bank.json'));
 f.append(cycle(13000),13000);old.consume();f.append(cycle(16000),16000);old.consume();
 const journal=fs.readFileSync(path.join(f.cohortDir(),'journal.jsonl')),next=streaming(f);
 assert.deepEqual(next.sourceState(),old.sourceState());assert.equal(next.status().verifiedOriginalCheckpoints,3);
 assert.equal(next.status().hostVersion,'entry-comparison-streaming-v2');assert.equal(next.consume(),0);
 assert.deepEqual(fs.readFileSync(path.join(f.cohortDir(),'bank.json')),bank);
 assert.deepEqual(fs.readFileSync(path.join(f.cohortDir(),'journal.jsonl')),journal);
});
test('full source read is forbidden while large records crossing read chunks replay identically',t=>{
 const f=fixture(t);let previous='';
 const padded=f.records.map(r=>{const {hash,...body}=r;body.previous=previous;body.state={fixture:true,padding:'x'.repeat(2*1024*1024)};const row={...body,hash:digest(body)};previous=row.hash;return row;});
 fs.writeFileSync(f.source,padded.map(r=>JSON.stringify(r)+'\n').join(''));
 const old=f.open('first',true),expected=old.sourceState(),original=fs.readFileSync;
 fs.readFileSync=function(p,...args){if(path.resolve(String(p))===path.resolve(f.source))throw Error('Full source read forbidden');return original.call(this,p,...args);};
 try{assert.deepEqual(streaming(f).sourceState(),expected);}finally{fs.readFileSync=original;}
});
test('incomplete startup and incremental tails commit only after the whole record arrives',t=>{
 const f=fixture(t);f.open('first',true);const nextRecord=record(4,f.records.at(-1).hash,cycle(21000),21000),bytes=Buffer.from(JSON.stringify(nextRecord)+'\n'),half=Math.floor(bytes.length/2);
 fs.appendFileSync(f.source,bytes.subarray(0,half));const w=streaming(f);assert.equal(w.sourceState().sourceSequence,3);
 assert.equal(w.consume(),0);assert.ok(w.status().incompleteTailBytes>0);
 fs.appendFileSync(f.source,bytes.subarray(half));assert.equal(w.consume(),1);assert.equal(w.sourceState().sourceSequence,4);
 assert.equal(w.consume(),0);assert.equal(streaming(f).sourceState().sourceSequence,4);
});
test('derived checkpoint corruption and live frozen-bank edits fail closed',async t=>{
 await t.test('derived state',t=>{const f=fixture(t);f.open('first',true);const file=path.join(f.cohortDir(),'journal.jsonl'),rows=verifySource(fs.readFileSync(file,'utf8'));
  rows[0].state.counters.developmentBooks++;const {hash,...body}=rows[0];fs.writeFileSync(file,JSON.stringify({...body,hash:digest(body)})+'\n');assert.throws(()=>streaming(f),/REPLAY_MISMATCH/);});
 await t.test('bank mutation',t=>{const f=fixture(t);f.open('first',true);const w=streaming(f),file=path.join(f.cohortDir(),'bank.json'),bank=JSON.parse(fs.readFileSync(file));bank.cutoff=stamp(11000);fs.writeFileSync(file,JSON.stringify(bank));assert.throws(()=>w.consume(),/BANK_CHANGED/);});
});
test('STOP at post-replay commit fence preserves the original journal',t=>{
 const f=fixture(t);f.open('first',true);f.append(cycle(13000),13000);const file=path.join(f.cohortDir(),'journal.jsonl'),before=fs.readFileSync(file);let checks=0;
 assert.throws(()=>openStreaming({runtimeDir:f.dir,cohort:'first',now:()=>base+30000,canOperate:()=>++checks===1}),/STOPPED/);
 assert.deepEqual(fs.readFileSync(file),before);
});
test('streaming successor cannot initialize or replace the frozen experiment',t=>{
 const f=fixture(t);f.open('first',true);assert.throws(()=>openStreaming({runtimeDir:f.dir,cohort:'first',initialize:true}),/EXISTING_FROZEN_BANK/);
});

