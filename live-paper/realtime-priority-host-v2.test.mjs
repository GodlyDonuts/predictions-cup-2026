import test from 'node:test';import assert from 'node:assert/strict';
import {createRealtimePriorityHostV2} from './realtime-priority-host-v2.mjs';import {scope as original,start,at} from '../feed/test/helpers.mjs';
const markets=[{marketId:'277',exchangeId:'701'},{marketId:'278',exchangeId:'702'},{marketId:'279',exchangeId:'703'}];
function fixture(){let clock=start,active=true,clientState='idle',callbacks,report={at:at(0),marketIds:['278']},stopAtNetwork=false;const revisions=new Map();
  const calls=[];let host;
  host=createRealtimePriorityHostV2({scope:{tournamentId:original.tournamentId,markets},canOperate:()=>active,now:()=>clock,
    capture:{append:()=>{}},getRecoveryPriority:()=>report,
    clientFactory:opts=>{callbacks=opts;return{status:()=>({state:clientState,eventsReceived:1,markets:markets.map(m=>({...m,subscribed:clientState==='subscribed',connectionId:'c1'}))}),
      start:async()=>{clientState='subscribed';for(const m of markets)opts.onSubscribed({...m,connectionId:'c1',receivedAt:at(clock-start)});},
      reconnect:async()=>{},stop:async()=>{clientState='stopped';},authorizes:()=>false};},
    dispatch:async spec=>{assert.equal(host.authorizes(spec),true);spec.beforeDispatch();if(stopAtNetwork)active=false;spec.beforeNetwork();
      const marketId=/\/markets\/(\d+)\//.exec(spec.endpoint)[1],m=markets.find(m=>m.marketId===marketId),time=at(clock-start);calls.push(marketId);
      return{status:200,endpoint:spec.endpoint,requestStartedAt:time,receivedAt:time,data:{contexts:[{type:'tournament',tournament:{id:original.tournamentId},
        orderbook:{exchanges:[{exchangeId:m.exchangeId,option:'YES',asOf:{sequence:100,at:time},bids:[{price:.4,quantity:100}],asks:[{price:.6,quantity:100}]}]}}]}};}});
  return{host,calls,gap:()=>{for(const m of markets)callbacks.onResyncRequired({...m,connectionId:'c1',reason:'GAP'});},
    emitActivity:(marketId,count=1,includeTrades=true)=>{const market=markets.find(m=>m.marketId===marketId);assert(market);let revision=revisions.get(marketId)||0;
      for(let i=0;i<count;i++){revision++;const time=at(clock-start);callbacks.onEvent({marketId,connectionId:'c1',event:'market_batch',receivedAt:time,payload:{
        delivery:{model:'best-effort-authoritative-resync',correlationId:'fixture',revision,previousRevision:revision-1,sourceSequenceFrom:revision,sourceSequenceThrough:revision},
        books:[{exchangeId:market.exchangeId,marketId,tournamentId:original.tournamentId,asOf:{sequence:revision,at:time},nextExpiryAt:null,bids:[{price:.4,quantity:100}],asks:[{price:.6,quantity:100}]}],
        trades:includeTrades?[{id:`${marketId}-${revision}`,sequence:revision,exchangeId:market.exchangeId,marketId,tournamentId:original.tournamentId,price:.5,quantity:10,executedAt:time}]:[],bookDirty:[],marketSettled:[]}});}
      revisions.set(marketId,revision);},
    emitNoopActivity:(marketId,count=1)=>{assert(markets.some(m=>m.marketId===marketId));let revision=revisions.get(marketId)||0;
      for(let i=0;i<count;i++){revision++;const time=at(clock-start);callbacks.onEvent({marketId,connectionId:'c1',event:'market_batch',receivedAt:time,payload:{
        delivery:{model:'best-effort-authoritative-resync',correlationId:'fixture-noop',revision,previousRevision:revision-1,sourceSequenceFrom:revision,sourceSequenceThrough:revision},
        books:[],trades:[],bookDirty:[],marketSettled:[]}});}
      revisions.set(marketId,revision);},
    setReport:r=>report=r,setClock:ms=>clock=start+ms,stopAtNetwork:()=>stopAtNetwork=true};}
test('priority priming uses one existing fenced full-depth GET and no second recovery',async()=>{
  const f=fixture();await f.host.service();assert.deepEqual(f.calls,['277']);await f.host.service();
  assert.deepEqual(f.calls,['277','278']);assert.equal(f.host.view('278').ready,true);assert.equal(f.host.status().priorityRecovery.successes,1);
  assert.equal(f.host.status().priorityRecovery.additionalApiClients,0);await f.host.stop();
});
test('persistent priority pressure cannot reset the independent background rotation',async()=>{
  const f=fixture();await f.host.service();for(let i=0;i<12;i++){f.gap();const n=f.calls.length;await f.host.service();assert.equal(f.calls.length,n+1);}
  const background=[f.calls[4],f.calls[8],f.calls[12]];assert.deepEqual(background,['278','279','277']);
  assert.ok(f.host.status().priorityRecovery.priorityStreak<=3);await f.host.stop();
});
test('stale priority reports fall back; unknown markets fail closed without dispatch',async()=>{
  const f=fixture();await f.host.service();f.setClock(121000);await f.host.service();assert.equal(f.host.status().priorityRecovery.staleReports,1);
  f.setReport({at:at(121000),marketIds:['999']});const before=f.calls.length;await assert.rejects(f.host.service(),/PRIORITY_SCOPE/);assert.equal(f.calls.length,before);await f.host.stop();
});
test('STOP at the guarded network boundary prevents priority fetch and hides readiness',async()=>{
  const f=fixture();await f.host.service();f.stopAtNetwork();await f.host.service();assert.deepEqual(f.calls,['277']);
  assert.equal(f.host.view('278'),null);assert.equal(f.host.status().priorityRecovery.lastCode,'PRIORITY_RECOVERY_UNAVAILABLE');await f.host.stop();
});

test('missing or empty session priority uses recent live activity in the same recovery slot',async()=>{
  const f=fixture();f.setReport(null);await f.host.service();assert.deepEqual(f.calls,['277']);
  f.emitActivity('278',30,false);f.emitActivity('279',3);await f.host.service();assert.deepEqual(f.calls,['277','279']);
  const status=f.host.status().priorityRecovery;assert.equal(status.attempts,1);assert.equal(status.successes,1);
  assert.equal(status.lastPrioritySource,'live_activity');assert.deepEqual(status.lastPriorityMarketIds,['279','278']);
  assert.equal(status.additionalApiClients,0);assert.equal(status.additionalRecoverySlots,0);await f.host.stop();
});

test('live activity falls through when every session-priority market is already ready',async()=>{
  const f=fixture();await f.host.service();await f.host.service();assert.equal(f.host.view('278').ready,true);
  f.emitActivity('279',2);await f.host.service();assert.deepEqual(f.calls,['277','278','279']);
  assert.equal(f.host.status().priorityRecovery.lastPrioritySource,'live_activity');await f.host.stop();
});

test('live activity expires from priority and returns the existing slot to background rotation',async()=>{
  const f=fixture();f.setReport({at:at(0),marketIds:[]});await f.host.service();
  f.emitActivity('279',2);await f.host.service();assert.equal(f.host.status().priorityRecovery.attempts,1);
  f.setClock(121000);const prior=f.calls.length;await f.host.service();assert.equal(f.calls.length,prior+1);
  const status=f.host.status().priorityRecovery;assert.equal(status.backgroundCalls,2);assert.equal(status.lastPrioritySource,'background_rotation');
  assert.equal(status.additionalApiClients,0);assert.equal(status.additionalRecoverySlots,0);await f.host.stop();
});

test('activity accumulated during an unsampled gap is not treated as recent',async()=>{
  const f=fixture();f.setReport(null);await f.host.service();
  f.emitActivity('279',2);f.setClock(121000);await f.host.service();
  const status=f.host.status().priorityRecovery;
  assert.equal(status.attempts,0);assert.equal(status.backgroundCalls,2);
  assert.equal(status.lastPrioritySource,'background_rotation');await f.host.stop();
});

test('empty accepted batches do not steal live-activity recovery priority',async()=>{
  const f=fixture();f.setReport(null);await f.host.service();f.emitNoopActivity('278',30);await f.host.service();
  const status=f.host.status().priorityRecovery;assert.equal(status.attempts,0);
  assert.equal(status.lastPrioritySource,'background_rotation');await f.host.stop();
});

test('activity telemetry separates WebSocket book changes from REST snapshots',async()=>{
  const f=fixture();f.setReport(null);await f.host.service();
  assert.equal(f.host.status().markets.find(m=>m.marketId==='277').metrics.streamBookUpdates,0);
  f.emitActivity('279',2,false);await f.host.service();
  const market=f.host.status().markets.find(m=>m.marketId==='279');
  assert.equal(market.metrics.streamBookUpdates,2);
  assert.equal(market.metrics.recoveries,1);
  assert.equal(f.host.status().priorityRecovery.lastPrioritySource,'live_activity');await f.host.stop();
});

