// Read-only reconciliation adapter. The injected dispatch is the collector's
// sole shared queue; this module has no credential, fetch, timer or retry loop.
import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
import {OrderTransport} from './transport.mjs';
import {reconcileProductionAccount,fillsReadAuthorized} from './production-reconciliation.mjs';
import {payloadHash} from './core.mjs';import {atomicJson} from './ledger.mjs';
const need=(ok,code)=>{if(!ok)throw Error('MEDIAN_PREFLIGHT_'+code);};
const TID='bda92870-621e-47b0-bc3c-3602c5c26f55',RUN=/^[a-z][a-z0-9-]{1,100}$/,HEX=/^[a-f0-9]{64}$/;
function requestOf(raw,now){const x=JSON.parse(raw);need(x&&Object.keys(x).sort().join('|')==='activationAuthorityHash|requestedAt|runId|schemaVersion'&&x.schemaVersion===1&&RUN.test(x.runId)&&HEX.test(x.activationAuthorityHash),'REQUEST_SCHEMA');const at=Date.parse(x.requestedAt);need(Number.isSafeInteger(at)&&new Date(at).toISOString()===x.requestedAt&&at<=now&&now-at<=90000,'REQUEST_CLOCK');return x;}
const byteHash=x=>createHash('sha256').update(x).digest('hex');
const safeCode=e=>/^MEDIAN_PREFLIGHT_[A-Z_]+$/.test(e?.message)?e.message:'MEDIAN_PREFLIGHT_RECONCILIATION_BLOCKED';

export function createMedianPreflight({runtimeDir,universe,dispatch,canOperate,now=Date.now}){
 need(path.isAbsolute(runtimeDir)&&fs.statSync(runtimeDir).isDirectory()&&[dispatch,canOperate,now].every(x=>typeof x==='function')&&universe?.tournamentId===TID,'DEPENDENCIES');
 const requestFile=path.join(runtimeDir,'median-production-preflight-request.json'),latestFile=path.join(runtimeDir,'median-production-preflight-latest.json');let active=null;const issued=new WeakMap();
 const transport=new OrderTransport({scope:{tournamentId:TID,tournamentSlug:'midterm-elections',exchanges:universe.exchanges},dispatch:guardedDispatch,canWrite:()=>false,ownsOrder:()=>false,validatePermit:()=>false,validateLease:()=>false,validateNetworkPermit:()=>false,now,maxPages:20});
 function allowedRoute(spec){if(!spec||spec.method!=='GET'||spec.body!==undefined||spec.bodyJson!==undefined)return false;if(fillsReadAuthorized(spec))return true;const e=spec.endpoint;if(e==='/tournaments/midterm-elections'||e==='/tournaments/midterm-elections/portfolio/positions')return true;if(!new RegExp('^/orders\\?status=(?:all|open)&limit=200&tournamentId='+TID+'(?:&cursor=[A-Za-z0-9%._~-]+)?$').test(e))return false;try{return transport.authorizeRequest(spec)===true;}catch{return false;}}
 function fence(){need(active,'NO_ACTIVE_REQUEST');need(canOperate()===true&&!['STOP','COLLECTOR_STOP','EXECUTION_STOP','MEDIAN_PRODUCTION_STOP','MEDIAN_PREFLIGHT_STOP'].some(x=>fs.existsSync(path.join(runtimeDir,x))),'STOPPED');need(now()>=Date.parse(active.request.requestedAt)&&now()-Date.parse(active.request.requestedAt)<=90000,'REQUEST_EXPIRED');need(byteHash(fs.readFileSync(requestFile))===active.byteHash,'REQUEST_CHANGED');return true;}
 function authorizes(spec){try{const permit=issued.get(spec?.beforeDispatch);return active!==null&&permit?.endpoint===spec.endpoint&&permit.beforeNetwork===spec.beforeNetwork&&allowedRoute(spec)&&fence()===true;}catch{return false;}}
 async function guardedDispatch(spec){fence();need(allowedRoute(spec),'READ_SCOPE');const permitted={...spec,beforeDispatch:async()=>fence(),beforeNetwork:async()=>fence()};issued.set(permitted.beforeDispatch,{endpoint:permitted.endpoint,beforeNetwork:permitted.beforeNetwork});try{const receipt=await dispatch(permitted);fence();need(receipt?.endpoint===permitted.endpoint&&Number.isFinite(Date.parse(receipt.requestStartedAt))&&Number.isFinite(Date.parse(receipt.receivedAt))&&Date.parse(receipt.requestStartedAt)>=Date.parse(active.request.requestedAt)&&Date.parse(receipt.requestStartedAt)<=Date.parse(receipt.receivedAt)&&Date.parse(receipt.receivedAt)<=now(),'RESPONSE_CLOCK_OR_SCOPE');return receipt;}finally{issued.delete(permitted.beforeDispatch);}}
 function load(){if(!fs.existsSync(requestFile))return null;const raw=fs.readFileSync(requestFile);need(raw.length<=8192,'REQUEST_BOUND');const request=requestOf(raw,now()),requestHash=payloadHash(request),statusFile=path.join(runtimeDir,'median-production-preflight-'+request.runId+'.status.json');return{request,requestHash,byteHash:byteHash(raw),statusFile};}
 function pending(){try{if(active)return false;const x=load();if(!x)return false;if(!fs.existsSync(x.statusFile))return true;const st=JSON.parse(fs.readFileSync(x.statusFile));return st.requestHash!==x.requestHash;}catch{return false;}}
 async function consume(){need(active===null,'BUSY');const x=load();if(!x)return{status:'no_request',executionAuthority:false};if(fs.existsSync(x.statusFile)){const st=JSON.parse(fs.readFileSync(x.statusFile));need(st.requestHash===x.requestHash,'RUN_ID_REUSED');return{status:st.status,runId:x.request.runId,requestHash:x.requestHash,...(st.receiptHash?{receiptHash:st.receiptHash}:{}),deduplicated:true,executionAuthority:false};}
  active=x;const statusBase={schemaVersion:1,runId:x.request.runId,requestHash:x.requestHash,activationAuthorityHash:x.request.activationAuthorityHash};try{
   fence();atomicJson(x.statusFile,{...statusBase,status:'started',at:new Date(now()).toISOString(),executionAuthority:false});
   const result=await reconcileProductionAccount({transport,dispatch:guardedDispatch,universe,now,maxPages:20});fence();
   const file='production-human-reconciliation-'+x.request.runId+'.json',receiptFile=path.join(runtimeDir,file);need(!fs.existsSync(receiptFile),'RECEIPT_ALREADY_EXISTS');
   // Raw data stays in the pre-existing restricted runtime directory. No raw
   // financial rows or upstream errors enter the derived pointer/status/logs.
   atomicJson(receiptFile,{schemaVersion:1,runId:x.request.runId,requestHash:x.requestHash,activationAuthorityHash:x.request.activationAuthorityHash,receiptHash:result.receiptHash,receipt:result.receipt});fence();
   const pointer={schemaVersion:1,runId:x.request.runId,activationAuthorityHash:x.request.activationAuthorityHash,requestHash:x.requestHash,receiptHash:result.receiptHash,at:result.receipt.at,file,executionAuthority:false};
   atomicJson(x.statusFile,{...statusBase,status:'complete',receiptHash:result.receiptHash,at:result.receipt.at,executionAuthority:false});atomicJson(latestFile,pointer);return{status:'complete',...pointer};
  }catch(e){try{atomicJson(x.statusFile,{...statusBase,status:'blocked',reason:safeCode(e),at:new Date(now()).toISOString(),executionAuthority:false});}catch{throw Error('MEDIAN_PREFLIGHT_DURABILITY_FAILURE');}throw Error(safeCode(e));}finally{active=null;}
 }
 return{authorizes,pending,consume};
}
