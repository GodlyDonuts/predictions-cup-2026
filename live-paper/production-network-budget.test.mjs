import test from 'node:test';
import assert from 'node:assert/strict';
import {ReadClient} from './transport-runtime.mjs';
import {validateNetworkBudget,networkWriteInterval} from './transport-network-budget.mjs';
const budget={schemaVersion:4,ownerId:'Codex',enabled:true,intervalMs:1000,writeIntervalMs:2500,soleAccountClient:true,executionOwner:'Codex',handoffId:'daenerys-to-baal-fixture',releaseCommentId:123,ackCommentId:124,writesEnabled:true};
test('current production budget requires the exact named-release fields and leaves platform headroom',()=>{
 assert.deepEqual(validateNetworkBudget(budget),budget);assert.equal(networkWriteInterval(6000,budget),2500);
 for(const c of [{executionOwner:'Daenerys'},{releaseCommentId:0},{ackCommentId:0},{soleAccountClient:false},{writesEnabled:false},{intervalMs:500},{writeIntervalMs:1000},{handoffId:'old'}])assert.throws(()=>validateNetworkBudget({...budget,...c}));
});
test('new handoff budget serializes mixed traffic without bursts under every rolling account window',async()=>{
 let now=0,inflight=0;const requests=[];
 const client=new ReadClient('FIXTURE',{networkBudget:budget,now:()=>now,sleepFn:async ms=>now+=ms,authorizeDispatch:()=>true,fetchFn:async(_url,o)=>{assert.equal(inflight++,0);requests.push({at:now,write:o.method!=='GET'});now+=5;inflight--;return{ok:true,status:200,json:async()=>({})};}});
 try{await Promise.all(Array.from({length:200},(_,i)=>i%3?client.request('/tournaments/midterm-elections'):client.dispatch({method:'POST',endpoint:'/orders',beforeDispatch:async()=>{},beforeNetwork:async()=>{}})));
  for(let n=0;n<requests.length;n++){const r=requests[n],window=requests.filter(x=>x.at>=r.at&&x.at<r.at+60000);assert.ok(window.filter(x=>x.write).length<=24);assert.ok(window.filter(x=>!x.write).length<=60);if(n)assert.ok(r.at-requests[n-1].at>=(requests[n-1].write?2500:1000));}
 }finally{client.close();}
});
