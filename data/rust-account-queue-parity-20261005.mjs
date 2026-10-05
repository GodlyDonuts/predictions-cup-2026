import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {ReadClient} from '../live-paper/transport-runtime.mjs';

const executable=path.resolve('backend-rust/account-queue/target/release/baal-account-queue.exe');
const budget={schemaVersion:2,ownerId:'Codex',enabled:true,intervalMs:1000,writeIntervalMs:2500,soleAccountClient:true,formerWriterStopCommentId:5947535773};
const scope='offline-fixture-only';
const scenarios=[
 {name:'mixed_completion_pacing',statuses:[200,200,200,200,200,200],delays:[359,7000,0,11,25,100],writes:[false,true,false,true,false,false]},
 {name:'slow_durable_network_fence',statuses:[200,200],delays:[400,0],writes:[true,false],guardDelay:5000},
 {name:'429_retry_after_90s',statuses:[429,200],delays:[300,50],writes:[false,false],retry:'90'},
 {name:'503_retry_after_15s',statuses:[503,200],delays:[0,50],writes:[false,false],retry:'15'},
];
const evidence=[];
for(const scenario of scenarios){
 let now=0, index=0, persisted=0;
 const recorded=[], commands=[];
 const client=new ReadClient('OFFLINE_PLACEHOLDER',{networkBudget:budget,now:()=>now,sleepFn:async ms=>{now+=ms;},authorizeDispatch:()=>true,onAdmission:n=>{persisted=n;},fetchFn:async(_url,options)=>{
  const i=index++,at=now,status=scenario.statuses[i],request='fixture-'+i,write=options.method!=='GET';
  const binding=write?{intent:'intent-'+i,key:'key-'+i,payload_hash:'a'.repeat(64)}:undefined;
  // Node reserves before the durable pre-network guard. Match that reservation clock.
  commands.push({op:'admit',scope,now:at-(scenario.guardDelay&&i===0?scenario.guardDelay:0),request,write,...(binding?{binding}:{}),authorized:true,permit_valid:true,lease_valid:true});
  commands.push({op:'network',scope,now:at,request,write,...(binding?{binding}:{}),authorized:true,permit_valid:true,lease_valid:true});
  now+=scenario.delays[i];
  commands.push({op:'complete',scope,now,request,write,status,outcome:status===200?'success':'rejected',retry_after:status===200?'':scenario.retry});
  recorded.push({networkAt:at,finishedAt:now,status,write});
  return {ok:status===200,status,headers:new Headers({'Retry-After':status===200?'':scenario.retry}),json:async()=>({})};
 }});
 try{for(let i=0;i<scenario.statuses.length;i++){
  try{if(scenario.writes[i])await client.dispatch({method:'POST',endpoint:'/orders',beforeDispatch:async()=>{},beforeNetwork:async()=>{if(i===0)now+=scenario.guardDelay||0;}});else await client.request('/tournaments/midterm-elections');}
  catch(e){assert.equal(e.status,scenario.statuses[i]);}
 }}finally{client.close();}
 commands.push({op:'status',scope,now});
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'baal-offline-rust-queue-'));
 try{
  const r=spawnSync(executable,['--offline-jsonl',dir,scope,'offline-fixture-lock'],{input:commands.map(c=>JSON.stringify(c)).join('\n')+'\n',encoding:'utf8',windowsHide:true});
  assert.equal(r.status,0,r.stderr);const responses=r.stdout.trim().split('\n').map(JSON.parse);assert.equal(responses.length,commands.length);
  for(let i=0;i<responses.length;i++){assert.equal(responses[i].ok,true,`${scenario.name} command ${i}: ${JSON.stringify(responses[i])}`);if(commands[i].op==='admit')assert.equal(responses[i].result.admitted,true);}
  const state=responses.at(-1).result.state;
  assert.equal(state.next,persisted);assert.equal(state.cooldown,client.cooldown);assert.equal(state.active,null);assert.equal(state.reads.length,recorded.filter(r=>!r.write&&now-(r.networkAt-(scenario.guardDelay&&recorded.indexOf(r)===0?scenario.guardDelay:0))<=60000).length);
  evidence.push({scenario:scenario.name,requests:recorded.length,nodeRustFinalReservationParity:true,next:state.next,cooldown:state.cooldown,networkSequence:recorded});
 }finally{fs.rmSync(dir,{recursive:true,force:true});}
}
const result={schemaVersion:1,recordedAt:new Date().toISOString(),mode:'offline_actual_Node_ReadClient_Rust_core_parity',binarySha256:createHash('sha256').update(fs.readFileSync(executable)).digest('hex'),scenarios:evidence,limitations:['No API, credentials, runtime bank or production client was accessed.','Rust explicitly enforces 60 reads/24 writes in an inclusive rolling minute; Node pacing alone permits boundary counts of 61/25 in existing tests. That conservative extra boundary differs intentionally.','This measures correctness, not profitability or faster account dispatch. Existing production HTTP/authority validators remain Node.']};
fs.writeFileSync('data/rust-account-queue-parity-20261005.json',JSON.stringify(result,null,2)+'\n');
console.log(JSON.stringify({passedScenarios:evidence.length,binarySha256:result.binarySha256}));
