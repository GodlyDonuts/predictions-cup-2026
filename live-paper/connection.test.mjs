import test from 'node:test';
import assert from 'node:assert/strict';
import {connectTournament} from './connection.mjs';
const c={tournamentSlug:'midterm-elections',tournamentId:'expected'},good={id:'expected',currencyName:'SUSQies',status:'active',isPendingEnrolment:false};
test('startup retries only recoverable reads with capped exponential pacing and Retry-After headroom',async()=>{
 let calls=0,clock=0;const waits=[],retries=[],api={cooldown:0,async request(path){assert.equal(path,'/tournaments/midterm-elections');calls++;if(calls<=5){if(calls===2)this.cooldown=clock+90000;const e=Error('unavailable');e.recoverable=true;throw e;}return {data:good};}};
 assert.equal(await connectTournament(api,c,{stopped:()=>false,now:()=>clock,onRetry:x=>retries.push(x),pause:async delay=>{waits.push(delay);clock+=delay;}}),good);
 assert.deepEqual(waits,[10000,90000,40000,60000,60000]);assert.equal(calls,6);assert.deepEqual(retries.map(x=>x.attempts),[1,2,3,4,5]);
});
test('STOP during startup recovery sends no additional read',async()=>{
 let stop=false,calls=0;const api={cooldown:0,async request(){calls++;const e=Error('connection');e.recoverable=true;throw e;}};
 assert.equal(await connectTournament(api,c,{stopped:()=>stop,onRetry:()=>{},pause:async()=>{stop=true;}}),null);assert.equal(calls,1);
});
test('startup authentication, manual-review and invalid identity errors stay fatal',async()=>{
 for(const variant of ['auth','manual','identity','json']){let calls=0;const api={cooldown:0,manualReview:variant==='manual',async request(){calls++;if(variant==='identity')return {data:{...good,id:'wrong'}};const e=Error(variant);e.recoverable=variant==='manual';throw e;}};await assert.rejects(connectTournament(api,c,{stopped:()=>false,onRetry:()=>assert.fail('Unexpected retry'),pause:()=>assert.fail('Unexpected pause')}));assert.equal(calls,1);}
});
