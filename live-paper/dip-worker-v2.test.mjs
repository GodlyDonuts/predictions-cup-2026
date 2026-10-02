import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {createDip,advanceDip} from './dip-shadow.mjs';
import {verifySource} from './ml-shadow.mjs';
import {digest} from './model.mjs';
import {atomicPaperJson,PaperPersistenceError} from './paper-persistence.mjs';
import {openDipWorker} from './dip-worker-v2.mjs';
const config=JSON.parse(fs.readFileSync(new URL('./fee-free-config.json',import.meta.url))),policy=JSON.parse(fs.readFileSync(new URL('./dip-policy.json',import.meta.url)));
const at=ms=>new Date(Date.parse('2026-10-02T08:00:00.000Z')+ms).toISOString();
function fixture(t){
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'dip-worker-'));t.after(()=>fs.rmSync(dir,{recursive:true,force:true}));const rows=[];
 const append=event=>{const body={sequence:rows.length+1,previous:rows.at(-1)?.hash||'',at:event.clockAt,event,state:{}},row={...body,hash:digest(body)};rows.push(row);fs.appendFileSync(path.join(dir,'journal.jsonl'),JSON.stringify(row)+'\n');return row;};
 append({type:'market_history',clockAt:at(0),history:{marketId:'377',exchangeId:'1066',tournamentId:config.tournamentId,receivedAt:at(0),coverage:{complete:true},candles:Array.from({length:8},(_,i)=>({time:at(-2400000+i*300000),vwap:.6}))}});
 const cfg={...config,marketIds:['377'],strategyUniverse:'diversified-dip-v1'},universe={tournamentId:config.tournamentId,exchanges:[{marketId:'377',exchangeId:'1066',option:'YES'}]};
 const {bank,state}=createDip(fs.readFileSync(path.join(dir,'journal.jsonl'),'utf8'),cfg,universe,policy,{377:{state:'AK',office:'Federal'}});
 for(const [name,value]of [['bank',bank],['state',state]])fs.writeFileSync(path.join(dir,'dip-scale-'+name+'.json'),JSON.stringify(value,null,2));
 const cycle=ms=>append({type:'cycle',clockAt:at(ms),accountExclusions:[],sourceSnapshots:[{endpoint:'/markets/377/orderbook?tournamentId='+config.tournamentId+'&depth=20',requestStartedAt:at(ms-100),receivedAt:at(ms),data:{contexts:[{type:'tournament',tournament:{id:config.tournamentId},orderbook:{exchanges:[{exchangeId:'1066',option:'YES',asOf:{at:at(ms),sequence:ms},bids:[{price:.545,quantity:10000}],asks:[{price:.550,quantity:10000}]}]}}]}}]});
 const read=name=>JSON.parse(fs.readFileSync(path.join(dir,'dip-scale-'+name+'.json'))),expected=()=>advanceDip(state,verifySource(fs.readFileSync(path.join(dir,'journal.jsonl'),'utf8')),bank);
 const options={runtimeDir:dir,workspaceDir:dir,cohort:'dip-scale',now:()=>Date.parse(at(10000)),onFault:()=>{}};
 return {dir,bank,state,cycle,read,expected,options};
}

test('STOP aborts a guarded state replacement cleanly and retains the old durable checkpoint',async t=>{const f=fixture(t),before=fs.readFileSync(path.join(f.dir,'dip-scale-state.json'));f.cycle(1000);const w=openDipWorker({...f.options,writeJson:(file,value,options)=>{if(file.endsWith('-state.json'))fs.writeFileSync(path.join(f.dir,'DIP_STOP'),'attributed stop');return atomicPaperJson(file,value,options);}});t.after(w.close);const result=await w.poll();assert.equal(result.status,'stopped');assert.deepEqual(fs.readFileSync(path.join(f.dir,'dip-scale-state.json')),before);assert.equal(f.read('stats').status,'stopped');assert.equal(f.read('stats').sourceSequence,1);});
test('a commit guard failure without STOP, or an I/O fault concurrent with STOP, still fails',async t=>{for(const kind of ['no_stop','io_stop'])await t.test(kind,async sub=>{const f=fixture(sub);f.cycle(1000);const w=openDipWorker({...f.options,writeJson:(file,value,options)=>{if(file.endsWith('-state.json')){if(kind==='io_stop')fs.writeFileSync(path.join(f.dir,'DIP_STOP'),'stop');throw new PaperPersistenceError({phase:kind==='io_stop'?'write':'guard',osCode:kind==='io_stop'?'EIO':'COMMIT_BLOCKED'});}return atomicPaperJson(file,value,options);}});sub.after(w.close);assert.equal((await w.poll()).status,'failed');assert.equal(f.read('stats').status,'failed');});});
