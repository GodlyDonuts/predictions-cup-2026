import test from 'node:test';import assert from 'node:assert/strict';import {readRecoveryEvents} from './live.mjs';import {StreamSource} from './stream-source.mjs';
const scopes=[{marketId:'1',exchangeId:'2'},{marketId:'3',exchangeId:'4'}],at='2026-10-14T00:00:00Z',pending={phase:'waiting_backoff',retryCount:1,sameEndpoint:true,failure:{marketId:'1',exchangeId:'2',method:'Get',status:0,transport:{category:'transport_dns'}}};
test('bounded DNS recovery invalidates only the failed scope and requires later ordinary reconciliation',()=>{
 const mux=new StreamSource(scopes,'cup',at);mux.status.get('1').blocked=false;mux.status.get('3').blocked=false;
 const events=readRecoveryEvents(mux,scopes,pending,at);assert.equal(events.length,1);assert.equal(events[0].exchangeId,'2');assert.equal(mux.status.get('1').blocked,true);assert.equal(mux.status.get('3').blocked,false);
 assert.deepEqual(readRecoveryEvents(mux,scopes,{phase:'recovered'},at),[]);assert.equal(mux.status.get('1').blocked,true);
});
test('POSTs, HTTP response failures and foreign scopes cannot enter DNS recovery',()=>{
 for(const change of [{method:'Post'},{status:429},{marketId:'9'}, {transport:{category:'transport_timeout'}}])assert.throws(()=>readRecoveryEvents(null,scopes,{...pending,failure:{...pending.failure,...change}},at),/RECOVERY_SCOPE/);
 assert.throws(()=>readRecoveryEvents(null,scopes,{...pending,retryCount:2},at),/RECOVERY_SCOPE/);
});
