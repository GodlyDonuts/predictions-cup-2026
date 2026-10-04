import test from 'node:test';
import assert from 'node:assert/strict';
import {realtimeCaptureScope, realtimeScopes} from './realtime-scope.mjs';
import {RealtimeBooks} from './realtime-books.mjs';
import {ReadClient} from './transport-runtime.mjs';
import {createRealtimeHost} from './realtime-host.mjs';

const tournamentId='bda92870-621e-47b0-bc3c-3602c5c26f55';
const markets=Array.from({length:237},(_,i)=>({marketId:String(i+1),exchangeId:String(i+1000)}));
const universe={tournamentId,exchanges:markets},baseline={tournamentId,marketIds:['1','2']};

test('explicit activation version selects whole universe; legacy activation preserves its original scope',()=>{
  const legacy=realtimeCaptureScope({schemaVersion:1,enabled:true,mode:'market_capture_only'},baseline,universe);
  assert.equal(legacy.markets.length,2);assert.equal(legacy.channelMode,'per_market');
  const full=realtimeCaptureScope({schemaVersion:2,enabled:true,mode:'market_capture_only',channelMode:'tournament'},baseline,universe);
  assert.equal(full.markets.length,237);assert.equal(realtimeScopes(full).topic,`tournament:${tournamentId}:markets`);
  for(const changed of [{schemaVersion:1,channelMode:'tournament'},{schemaVersion:2,channelMode:'per_market'},{schemaVersion:2},{schemaVersion:2,channelMode:'tournament',enabled:false}])
    assert.throws(()=>realtimeCaptureScope({enabled:true,mode:'market_capture_only',...changed},baseline,universe),/ACTIVATION/);
  assert.throws(()=>realtimeCaptureScope({schemaVersion:2,enabled:true,mode:'market_capture_only',channelMode:'tournament'},baseline,{...universe,tournamentId:'foreign'}),/UNIVERSE/);
});

test('tournament books keep all 237 instruments unavailable until individual subscription and REST recovery',()=>{
  const books=new RealtimeBooks({tournamentId,markets,channelMode:'tournament'});
  assert.equal(books.status().markets.length,237);assert(books.status().markets.every(m=>!m.ready));
  const at=new Date().toISOString();books.subscribed({marketId:'237',connectionId:'one-socket-237',receivedAt:at});
  const status=books.status();assert(status.markets.every(m=>!m.ready));
  assert.deepEqual(books.needsRecovery().map(m=>m.marketId),['237']);
  assert.throws(()=>new RealtimeBooks({tournamentId,markets}),/SCOPE/);
  assert.throws(()=>new RealtimeBooks({tournamentId,markets:[...markets,markets[0]],channelMode:'tournament'}),/DUPLICATE/);
  books.stop();
});

test('actual budget v3 remains immutable zero-write even when a caller claims token authority',async()=>{
  const budget={schemaVersion:3,ownerId:'Codex',enabled:true,intervalMs:3000,soleAccountClient:false,executionOwner:'Daenerys',ackCommentId:5966761902,maxReadsPerMinute:20,writesEnabled:false};
  let requests=0,admissions=0;const rows=[];
  const api=new ReadClient('SYNTHETIC',{networkBudget:budget,fetchFn:async()=>{requests++;throw Error('Unexpected request');},authorizeDispatch:()=>true,onAdmission:()=>admissions++});
  const host=createRealtimeHost({scope:{tournamentId,markets,channelMode:'tournament'},dispatch:s=>api.dispatch(s),createClient:()=>{throw Error('Unexpected SDK');},canOperate:()=>true,canMintToken:()=>api.readOnly!==true,capture:{append:r=>rows.push(r)}});
  for(let i=0;i<3;i++)assert.equal(await host.service(),false);
  assert.equal(host.status().transport.tokenRequests,0);assert.equal(host.status().transport.desiredChannels,1);
  assert.equal(host.status().markets.length,237);assert.equal(requests,0);assert.equal(admissions,0);
  assert.equal(rows.length,237);assert(rows.every(r=>r.kind==='config'));
  assert.throws(()=>{api.readOnly=false;},TypeError);
  assert.throws(()=>api.dispatch({method:'POST',endpoint:'/realtime/token'}),/forbids/);
  assert.throws(()=>api.dispatch({method:'POST',endpoint:'/orders'}),/forbids/);
  await host.stop();api.close();
});
