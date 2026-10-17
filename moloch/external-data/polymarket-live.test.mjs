import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import {EventEmitter} from 'node:events';
import {run,args} from './polymarket-live.mjs';
const conditionId='0x'+'a'.repeat(64),outcomes=[{assetId:'1',conditionId,pricingAuthority:false,resolutionEquivalenceVerified:false}];
const turn=()=>new Promise(resolve=>setTimeout(resolve,10));
async function harness(action,{recordFailure=false,scope=outcomes}={}){
 const cache=await fs.mkdtemp(path.join(os.tmpdir(),'poly-live-'));let at=1000,socket,stopped=false,journalClosed=false;const recorded=[];
 class Socket extends EventEmitter{send(){}terminate(){}close(){}}
 const rest=async(_options,hooks)=>{let resolve;const stoppedPromise=new Promise(r=>resolve=r);try{await hooks.onReady({outcomes:scope,stop:()=>{stopped=true;resolve();},recordEvent:async row=>{assert.equal(journalClosed,false);if(recordFailure)throw Error('DISK_FULL');recorded.push(row);},status:()=>({})});await stoppedPromise;}finally{await hooks.onStopping();journalClosed=true;}return{stopped:true};};
 try{return await run({cache,'ws-port':0},{runRest:rest,now:()=>at,streamOptions:{now:()=>at,webSocketFactory:()=>{socket=new Socket();return socket;},reconnectBaseMs:60000},onReady:async ready=>{await action({ready,cache,recorded,socket,setTime:n=>at=n,get stopped(){return stopped;}});}});}finally{await fs.rm(cache,{recursive:true,force:true});}
}
const raw=()=>JSON.stringify({event_type:'book',asset_id:'1',market:conditionId,timestamp:'1000',hash:'venue',bids:[{price:'0.4',size:'8'}],asks:[{price:'0.6',size:'9'}]});
const get=async(port,route)=>{const r=await fetch(`http://127.0.0.1:${port}${route}`);return{status:r.status,value:await r.json()};};
test('actual WS frame is original-journaled, cached separately, and HTTP view expires without clock renewal',async()=>{
 await harness(async({ready,socket,recorded,cache,setTime})=>{socket.emit('open');const body=raw();socket.emit('message',Buffer.from(body));await turn();const first=await get(ready.port,'/book?assetId=1');assert.equal(first.value.book.valid,true);assert.equal(first.value.book.bestBid,'0.4');assert.equal(recorded[0].bodyText,body);assert.equal(recorded[0].source,'polymarket_clob_ws_public');const stored=JSON.parse(await fs.readFile(path.join(cache,'ws-latest/1.json')));assert.equal(stored.receivedAt,'1970-01-01T00:00:01.000Z');setTime(31001);const stale=await get(ready.port,'/book?assetId=1');assert.equal(stale.value.book.valid,false);assert.equal(stale.value.book.receivedAt,first.value.book.receivedAt);ready.stop();});
});
test('network disconnect invalidates disk and export, records boundary, but keeps REST alive',async()=>{
 await harness(async ctx=>{const{ready,socket,recorded,cache}=ctx;socket.emit('open');socket.emit('message',Buffer.from(raw()));await turn();socket.emit('close');await turn();assert.equal(ctx.stopped,false);const q=await get(ready.port,'/book?assetId=1');assert.equal(q.value.book.valid,false);const stored=JSON.parse(await fs.readFile(path.join(cache,'ws-latest/1.json')));assert.equal(stored.normalized.valid,false);assert.equal(stored.streamEpoch,1);assert.equal(recorded.at(-1).eventType,'stream_invalidated');assert.equal(recorded.at(-1).bodyText,null);ready.stop();});
});
test('REST stop drains WS invalidations into journal before closure',async()=>{
 const result=await harness(async({ready,socket})=>{socket.emit('open');socket.emit('message',Buffer.from(raw()));await turn();ready.stop();});assert.equal(result.rest.stopped,true);assert.equal(result.ws.stream.stopped,true);
});
test('local capture failure stops both loops and rejects, rather than public reconnect',async()=>{
 await assert.rejects(harness(async({socket})=>{socket.emit('open');socket.emit('message',Buffer.from(raw()));await turn();},{recordFailure:true}),/DISK_FULL/);
});
test('deterministic bounded WS subset discloses REST-only assets',async()=>{
 const scope=Array.from({length:1026},(_,i)=>({assetId:String(i+1),conditionId}));await harness(async({ready})=>{const s=ready.status();assert.equal(s.wsOutcomes,1024);assert.equal(s.restOnlyOutcomes,2);assert.equal(s.restOutcomes,1026);ready.stop();},{scope});
});
test('startup withdraws a previous process book, unknown assets are404 and no auth methods exposed',async()=>{
 await harness(async({ready,cache})=>{const row=JSON.parse(await fs.readFile(path.join(cache,'ws-latest/1.json')));assert.equal(row.normalized.valid,false);assert.equal((await fetch('http://127.0.0.1:'+ready.port+'/book?assetId=2')).status,404);const r=await fetch(`http://127.0.0.1:${ready.port}/book`,{method:'POST'});assert.equal(r.status,405);ready.stop();});
});
test('CLI forwards shared REST budget and optional WS port, rejects unknown flags',()=>{assert.deepEqual(args(['--rps','30','--concurrency','12','--ws-port','17843']),{rps:'30',concurrency:'12','ws-port':'17843'});assert.throws(()=>args(['--secret','x']),/ARGUMENT/);});


test('real REST collector lifecycle shares bounded original journal and awaits final WS withdrawal',async()=>{
 const {gunzipSync}=await import('node:zlib');
 const root=await fs.mkdtemp(path.join(os.tmpdir(),'poly-live-real-'));const cache=path.join(root,'cache'),universe=path.join(root,'universe.json');let socket;
 class Socket extends EventEmitter{send(){}terminate(){}close(){}}
 try{
  await fs.writeFile(universe,JSON.stringify({schemaVersion:1,source:'polymarket_gamma_public',outcomes:[{...outcomes[0],marketId:'7',eventSlug:'actual-race',question:'Party winner?',outcome:'Yes',sigReferences:[]}]}));
  await run({cache,universe,port:0,'ws-port':0},{streamOptions:{now:()=>1000,webSocketFactory:()=>{socket=new Socket();return socket;}},restDependencies:{client:{stats:{admissions:0,completions:0},book:()=>{throw Error('SHOULD_NOT_REQUEST');}}},onReady:async ready=>{socket.emit('open');socket.emit('message',Buffer.from(raw()));await turn();ready.stop();}});
  const files=await fs.readdir(path.join(cache,'journal'));const rows=[];for(const f of files){const bytes=gunzipSync(await fs.readFile(path.join(cache,'journal',f)));rows.push(...bytes.toString().trim().split('\n').map(JSON.parse));}
  assert.equal(rows[0].bodyText,raw());assert.equal(rows.at(-1).eventType,'stream_invalidated');assert.equal(rows.at(-1).normalized.valid,false);assert.equal(JSON.parse(await fs.readFile(path.join(cache,'ws-latest/1.json'))).normalized.valid,false);await assert.rejects(fs.access(path.join(cache,'collector.lock')),/ENOENT/);
 }finally{await fs.rm(root,{recursive:true,force:true});}
});

