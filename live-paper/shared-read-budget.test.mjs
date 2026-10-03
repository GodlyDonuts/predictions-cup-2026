import test from 'node:test';import assert from 'node:assert/strict';
import {validateNetworkBudget,networkInterval} from './transport-network-budget.mjs';import {ReadClient} from './transport-runtime.mjs';
const budget={schemaVersion:3,ownerId:'Codex',enabled:true,intervalMs:3000,soleAccountClient:false,executionOwner:'Daenerys',ackCommentId:5966761902,maxReadsPerMinute:20,writesEnabled:false};
test('shared budget cannot assert sole ownership, enable writes or exceed agreed local ceiling',()=>{
 assert.equal(networkInterval(6000,budget),3000);for(const change of [{soleAccountClient:true},{writesEnabled:true},{intervalMs:2999},{maxReadsPerMinute:21},{executionOwner:'Codex'},{ackCommentId:1},{extra:true}])assert.throws(()=>validateNetworkBudget({...budget,...change}));
});
test('concurrent queues have at most twenty admissions per rolling minute and reject writes before network',async()=>{
 let now=0;const starts=[];const client=new ReadClient('test-key',{networkBudget:budget,now:()=>now,sleepFn:async ms=>{now+=ms;},authorizeDispatch:()=>true,fetchFn:async()=>{starts.push(now);now+=50;return {ok:true,status:200,headers:new Headers(),json:async()=>({})};}});
 await Promise.all(Array.from({length:45},()=>client.request('/tournaments/midterm-elections')));
 for(const at of starts)assert.ok(starts.filter(n=>n>=at&&n<at+60000).length<=20);
 for(let i=1;i<starts.length;i++)assert.ok(starts[i]-starts[i-1]>=3050);
 assert.throws(()=>client.dispatch({method:'POST',endpoint:'/orders',body:{}}),/forbids Codex writes/);
 await assert.rejects(client.perform('/orders',now,{method:'DELETE'}),/forbids Codex writes/);assert.equal(starts.length,45);
});
