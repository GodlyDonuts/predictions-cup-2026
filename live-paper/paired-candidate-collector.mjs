import fs from 'node:fs';
import path from 'node:path';
import {atomicJson,payloadHash} from '../execution/ledger.mjs';
import {createPairedCandidateCapture} from './paired-candidate-capture.mjs';
import {Journal} from './transport-runtime.mjs';

const need=(v,c)=>{if(!v)throw Error('PAIRED_COLLECTOR_'+c);};
// A single requested public pair screen replaces both auxiliary reads. It
// never publishes an ordinary book, supplies a wallet or owns an API client.
export function openPairedCandidateCollector({runtimeDir,universe,dispatch,reserve,headroom,getAccountEligibility,canOperate=()=>true,signal,now=Date.now}){
 const requestFile=path.join(runtimeDir,'paired-candidate-request.json'),statusFile=path.join(runtimeDir,'paired-candidate-status.json');
 let capture=null,binding=null,prior=fs.existsSync(statusFile)?JSON.parse(fs.readFileSync(statusFile)):null;
 const history=new Journal(path.join(runtimeDir,'paired-candidate-history.jsonl')),handled=new Set();
 if(fs.existsSync(history.file))for(const line of fs.readFileSync(history.file,'utf8').trim().split('\n').filter(Boolean)){const row=JSON.parse(line);need(row.event?.kind==='terminal_candidate'&&typeof row.event.name==='string','HISTORY');handled.add(row.event.name);}
 function archiveTerminal(){if(prior&&['complete','failed','interrupted'].includes(prior.phase)&&!handled.has(prior.name)){try{history.commit(null,{kind:'terminal_candidate',name:prior.name,requestHash:prior.requestHash||null,result:prior});handled.add(prior.name);}catch{const e=Error('Paired candidate history recording failed');e.durableRecordingFailed=true;e.fatal=true;throw e;}}}
 const startup=now();
 if(prior&&!['complete','failed','interrupted'].includes(prior.phase)){
  prior={...prior,phase:'interrupted',failureCode:'RESTART_REQUIRES_NEW_REQUEST_NAME',at:new Date(now()).toISOString()};atomicJson(statusFile,prior);
 }
 archiveTerminal();
 const allowed=()=>canOperate()&&!signal?.aborted;
 function pending(){
  if(!allowed()||!fs.existsSync(requestFile))return null;
  const r=JSON.parse(fs.readFileSync(requestFile));
  need(r.schemaVersion===1&&r.owner==='Codex'&&/^[a-z][a-z0-9-]{1,63}$/.test(r.name)&&Array.isArray(r.marketIds)&&r.marketIds.length===2&&new Set(r.marketIds).size===2&&r.marketIds.every(id=>universe.exchanges.some(x=>x.marketId===id&&x.option==='YES')),'REQUEST');
  const start=Date.parse(r.createdAt),end=Date.parse(r.expiresAt);
  need(Number.isSafeInteger(start)&&Number.isSafeInteger(end)&&end>start&&end-start<=1800000&&start<=now(),'REQUEST_CLOCK');
  if(now()>=end||handled.has(r.name)||prior?.name===r.name&&['complete','failed','interrupted'].includes(prior.phase))return null;
  if(prior&&['complete','failed','interrupted'].includes(prior.phase)&&prior.name!==r.name){archiveTerminal();capture=null;binding=null;}
  if(binding)need(payloadHash(r)===binding,'REQUEST_CHANGED');
  return r;
 }
 const persist=()=>{try{atomicJson(statusFile,prior);}catch{const e=Error('Paired candidate evidence recording failed');e.durableRecordingFailed=true;e.fatal=true;throw e;}};
 return{
  pending,
  authorizes:spec=>allowed()&&capture?.authorizes(spec)===true,
  status:()=>prior||{phase:'idle',executionAuthority:false},
  async afterAccountRefresh(){
   const request=pending();if(!request)return false;
   // No incomplete timing history on startup can justify additional reads.
   if(now()-startup<60000||!headroom({reads:2,at:new Date(now()).toISOString()}))return false;
   if(!capture){binding=payloadHash(request);capture=createPairedCandidateCapture({universe,dispatch,sharedReadHeadroom:headroom,getAccountEligibility,canOperate:()=>allowed()&&now()<Date.parse(request.expiresAt),signal,now});capture.begin(request);}
   let allocation;
   try{allocation=reserve(2);}catch{const e=Error('Paired candidate reservation recording failed');e.durableRecordingFailed=true;e.fatal=true;throw e;}
   need(allocation?.totalReads===2&&Number.isSafeInteger(allocation.reservedOrdinal),'DURABLE_TWO_READ_RESERVATION');
   const issuedAt=new Date(now()).toISOString(),grant={kind:'auxiliary_candidate_reads',sharedAccountBudget:true,reservedReads:2,reservationId:request.name+':'+allocation.reservedOrdinal,issuedAt,expiresAt:new Date(now()+10000).toISOString()};
   prior={...capture.status(),name:request.name,requestHash:binding,phase: 'reserved',reservation:grant,at:issuedAt};persist();
   let failure;
   try{await capture.advance({reservation:grant});}catch(e){failure=e;}
   prior={...capture.status(),name:request.name,requestHash:binding,reservation:grant,at:new Date(now()).toISOString(),ordinaryPublications:0,competitionOrders:0};persist();archiveTerminal();
   if(failure&&(failure.durableRecordingFailed||failure.fatal||failure.status===401||failure.status===403))throw failure;
   return true;
  }
 };
}
