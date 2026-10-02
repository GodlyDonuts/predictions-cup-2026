import test from 'node:test';
import assert from 'node:assert/strict';
import {createRealtimeHost} from './realtime-host.mjs';
import {ReadClient} from './runtime.mjs';

const tournamentId='bda92870-621e-47b0-bc3c-3602c5c26f55';
const scope={tournamentId,markets:[{marketId:'377',exchangeId:'1066'},{marketId:'354',exchangeId:'1040'}]};
function fixture({failStart=false,stopOnAdmission=false}={}) {
  let now=Date.parse('2026-10-02T10:00:00Z'),active=true,transportState='idle',options,starts=0,host;
  const rows=[],calls=[],clientMarkets=scope.markets.map(m=>({...m,subscribed:false,connectionId:null}));
  const api=new ReadClient('SYNTHETIC', {now:()=>now,sleepFn:async ms=>{now+=ms;},authorizeDispatch:s=>host.authorizes(s),
    onAdmission:()=>{if(stopOnAdmission)active=false;},fetchFn:async(url,init)=>{
      calls.push({url,method:init.method});const marketId=/\/markets\/(\d+)\//.exec(url)[1];const m=scope.markets.find(m=>m.marketId===marketId);
      return {ok:true,status:200,json:async()=>({contexts:[{type:'tournament',tournament:{id:tournamentId},orderbook:{exchanges:[{exchangeId:m.exchangeId,option:'YES',asOf:{sequence:1,at:new Date(now).toISOString()},bids:[{price:.4,quantity:100}],asks:[{price:.44,quantity:100}]}]}}]})};
    }});
  const start=async()=>{starts++;if(failStart){transportState='failed';throw Error('DO NOT LEAK PROVIDER BODY');}
    transportState='subscribed';for(const m of clientMarkets){m.subscribed=true;m.connectionId='test-'+starts+'-'+m.marketId;options.onSubscribed({...m,reason:'INITIAL',receivedAt:new Date(now).toISOString()});}};
  host=createRealtimeHost({dispatch:s=>api.dispatch(s),scope,canOperate:()=>active,capture:{append:r=>rows.push(r)},now:()=>now,
    clientFactory:o=>{options=o;return {start,reconnect:start,authorizes:()=>false,status:()=>({state:transportState,markets:clientMarkets}),stop:async()=>{transportState='stopped';}};}});
  return {host,api,rows,calls,getStarts:()=>starts,advance:ms=>now+=ms,stop:()=>active=false};
}
test('host shares serialized dispatcher and recovers only one scoped market per cycle',async()=>{
  const f=fixture();assert.equal(await f.host.service(),true);assert.equal(f.calls.length,1);
  assert.equal(f.host.status().markets.filter(m=>m.ready).length,1);
  assert.equal(await f.host.service(),true);assert.equal(f.calls.length,2);
  assert(f.calls.every(c=>c.method==='GET'&&c.url.endsWith('&depth=200')));
  assert.equal(f.host.status().recoverySuccesses,2);assert.equal(f.host.status().executionAuthorized,false);
  assert.equal(f.host.status().markets.filter(m=>m.ready).length,2);
  assert(f.rows.some(r=>r.kind==='snapshot'));assert(!JSON.stringify(f.host.status()).includes('bids'));
  assert.throws(()=>f.api.dispatch({method:'POST',endpoint:'/orders'}),/not authorized/);
  assert.throws(()=>f.api.dispatch({method:'POST',endpoint:'/realtime/token',beforeDispatch:()=>{}}),/not authorized/);
  const fake=()=>{};assert.throws(()=>f.api.dispatch({method:'GET',endpoint:'/markets/377/orderbook?tournamentId='+tournamentId+'&depth=200',beforeDispatch:fake,beforeNetwork:fake}),/not authorized/);
  await f.host.stop();
});
test('capture connection failure retries at most once per minute and suppresses provider content',async()=>{
  const f=fixture({failStart:true});await f.host.service();await f.host.service();assert.equal(f.getStarts(),1);
  assert(!JSON.stringify(f.host.status()).includes('PROVIDER BODY'));f.advance(60000);await f.host.service();assert.equal(f.getStarts(),2);await f.host.stop();
});
test('STOP after persistent admission blocks recovery network and records no snapshot',async()=>{
  const f=fixture({stopOnAdmission:true});await f.host.service();assert.equal(f.calls.length,0);
  assert.equal(f.rows.filter(r=>r.kind==='snapshot').length,0);assert.equal(f.host.status().recoverySuccesses,0);
  assert.equal(f.host.status().lastCode,'REALTIME_RECOVERY_UNAVAILABLE');await f.host.stop();
});
test('stale ready books rotate through bounded recovery instead of stale capture readiness',async()=>{
  const f=fixture();await f.host.service();await f.host.service();f.advance(20000);
  assert.equal(f.host.status().markets.filter(m=>m.ready).length,0);await f.host.service();
  assert.equal(f.calls.length,3);assert.equal(f.host.status().markets.filter(m=>m.ready).length,1);await f.host.stop();
});
