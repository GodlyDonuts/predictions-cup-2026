import fs from 'node:fs';import path from 'node:path';import os from 'node:os';
import test from 'node:test';import assert from 'node:assert/strict';
import {ReadClient} from './transport-runtime.mjs';
import {createBaalReadAttention} from './baal-read-attention.mjs';
import {openPairedCandidateCollector} from './paired-candidate-collector.mjs';
const base=Date.parse('2026-10-05T21:00:00.000Z'),at=n=>new Date(base+n).toISOString();
const universe={tournamentId:'bda92870-621e-47b0-bc3c-3602c5c26f55',exchanges:[{marketId:'353',exchangeId:'1042',option:'YES'},{marketId:'386',exchangeId:'1075',option:'YES'}]};
function fixture(){
 const runtimeDir=fs.mkdtempSync(path.join(os.tmpdir(),'baal-paired-collector-'));let clock=1000,operating=true,room=true,host;const calls=[];
 const now=()=>base+clock,requestFile=path.join(runtimeDir,'paired-candidate-request.json');
 fs.writeFileSync(requestFile,JSON.stringify({schemaVersion:1,owner:'Codex',name:'delaware-offline-check',marketIds:['353','386'],createdAt:at(0),expiresAt:at(900000)}));
 const attention=createBaalReadAttention({runtimeDir,tournamentId:universe.tournamentId,canOperate:()=>operating});
 let accountClock=clock;
 const account=()=>({complete:true,availableAt:at(accountClock),snapshot:{schemaVersion:1,positionsAt:at(accountClock),ordersAt:at(accountClock),excludedExchangeIds:[]}});
 const api=new ReadClient('OFFLINE_PLACEHOLDER',{now,monitorEventLoop:false,sleepFn:async ms=>{clock+=ms;},networkBudget:{schemaVersion:2,ownerId:'Codex',enabled:true,intervalMs:1000,writeIntervalMs:2500,soleAccountClient:true,formerWriterStopCommentId:5947535773},authorizeDispatch:s=>host.authorizes(s),fetchFn:async url=>{
  const endpoint=url.split('/api/v1')[1],marketId=/markets\/(\d+)/.exec(endpoint)[1],i=marketId==='353'?0:1;calls.push(endpoint);clock+=100;
  const contexts=[{type:'tournament',tournament:{id:universe.tournamentId}}];
  let data;
  if(endpoint.includes('/nodes?'))data={market_id:marketId,contexts,root:{node_id:String(354+i*33),contract_id:String(354+i*33),node_type:'contract',contract_type:'Election Outcome',settled_with:null,settlement_date:'2026-11-04T17:00:00.000Z',contract_details:{raceId:'62957',stageId:'98087',usState:'DE',raceName:'U.S. Senate Delaware',raceStage:'General',winnerName:i?'Republican Party':'Democratic Party',officeLevel:'Federal',contractType:'Election Outcome',electionDate:'2026-11-03',resolutionType:'Party Winner'}}};
  else{contexts[0].orderbook={exchanges:[{exchangeId:universe.exchanges[i].exchangeId,option:'YES',asOf:{sequence:calls.length,at:at(clock)},bids:[{price:i?.12:.91,quantity:1000}],asks:[{price:i?.125:.915,quantity:1000}]}]};data={marketId,contexts};}
  return{ok:true,status:200,json:async()=>data};
 }});
 const options={runtimeDir,universe,dispatch:s=>api.dispatch(s),reserve:n=>attention.reserve(n),headroom:()=>room,getAccountEligibility:account,canOperate:()=>operating,now};host=openPairedCandidateCollector(options);
 return{runtimeDir,host,calls,options,warm:()=>clock+=60000,refreshAccount:()=>accountClock=clock,noRoom:()=>room=false,stop:()=>operating=false,api,clean:()=>{api.close();fs.rmSync(runtimeDir,{recursive:true,force:true});}};
}
test('actual serialized client and durable auxiliary reservations capture roots then two cofresh books without ordinary publication',async()=>{const f=fixture();try{
 assert.equal(await f.host.afterAccountRefresh(),false);assert.equal(f.calls.length,0);f.warm();await f.host.afterAccountRefresh();assert.equal(f.host.status().phase,'books');
 f.refreshAccount();await f.host.afterAccountRefresh();const status=f.host.status();
 assert.equal(status.phase,'complete');assert.equal(status.roots.length,2);assert.equal(status.books.length,2);assert.equal(status.screen.cushionedCost,'0.980000');assert.equal(status.ordinaryPublications,0);assert.equal(status.screen.fills,0);assert.equal(f.calls.length,4);assert.equal(f.host.pending(),null);
 const grants=JSON.parse(fs.readFileSync(path.join(f.runtimeDir,'baal-auxiliary-reservations.json')));assert.equal(grants.cyclesReserved,2);
 const restarted=openPairedCandidateCollector(f.options);assert.equal(restarted.pending(),null);assert.equal(await restarted.afterAccountRefresh(),false);assert.equal(f.calls.length,4);
}finally{f.clean()}});
test('read headroom and STOP refuse auxiliary reservations and dispatch',async()=>{const f=fixture();try{f.warm();f.noRoom();assert.equal(await f.host.afterAccountRefresh(),false);assert.equal(f.calls.length,0);assert.equal(fs.existsSync(path.join(f.runtimeDir,'baal-auxiliary-reservations.json')),false);f.stop();assert.equal(f.host.pending(),null);}finally{f.clean()}});
test('partial recorded job is interrupted on restart, never re-read under same request name',async()=>{const f=fixture();try{f.warm();await f.host.afterAccountRefresh();const restarted=openPairedCandidateCollector(f.options);assert.equal(restarted.status().phase,'interrupted');assert.equal(restarted.pending(),null);assert.equal(await restarted.afterAccountRefresh(),false);assert.equal(f.calls.length,2);}finally{f.clean()}});
test('durable reservation failure propagates as fatal before network admission',async()=>{const f=fixture();try{const h=openPairedCandidateCollector({...f.options,reserve:()=>{throw Error('filesystem failure');}});f.warm();await assert.rejects(()=>h.afterAccountRefresh(),e=>e.durableRecordingFailed&&e.fatal);assert.equal(f.calls.length,0);}finally{f.clean()}});
test('a second named job keeps the first terminal evidence and does not stop the client or repeat a handled name',async()=>{const f=fixture();try{
 f.warm();await f.host.afterAccountRefresh();f.refreshAccount();await f.host.afterAccountRefresh();assert.equal(f.host.status().phase,'complete');
 const requestFile=path.join(f.runtimeDir,'paired-candidate-request.json'),r=JSON.parse(fs.readFileSync(requestFile));fs.writeFileSync(requestFile,JSON.stringify({...r,name:'delaware-second-offline-check'}));
 assert.equal(f.host.pending().name,'delaware-second-offline-check');f.refreshAccount();await f.host.afterAccountRefresh();f.refreshAccount();await f.host.afterAccountRefresh();assert.equal(f.host.status().phase,'complete');assert.equal(f.calls.length,8);
 const rows=fs.readFileSync(path.join(f.runtimeDir,'paired-candidate-history.jsonl'),'utf8').trim().split('\n').map(JSON.parse);assert.equal(rows.length,2);assert.deepEqual(rows.map(x=>x.event.name),[r.name,'delaware-second-offline-check']);
 fs.writeFileSync(requestFile,JSON.stringify(r));assert.equal(f.host.pending(),null);assert.equal(f.calls.length,8);
}finally{f.clean()}});
