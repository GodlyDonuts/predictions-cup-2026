import fs from 'node:fs';
import path from 'node:path';
import {payloadHash,atomicJson} from '../execution/ledger.mjs';
const need=(v,c)=>{if(!v)throw Error('PRODUCTION_METADATA_'+c);};
// One requested public-contract read replaces an auxiliary history/tape read.
// This reader owns no credentials, queue, timer or order permission.
export function openProductionMetadata({runtimeDir,universe,dispatch,canOperate=()=>true,now=Date.now}){
 const requestFile=path.join(runtimeDir,'production-metadata-request.json'),permits=new WeakMap();
 const authorizes=spec=>canOperate()&&spec?.method==='GET'&&spec.body===undefined&&spec.bodyJson===undefined&&typeof spec.beforeNetwork==='function'&&spec.beforeDispatch===spec.beforeNetwork&&permits.get(spec.beforeNetwork)===spec.endpoint;
 function pending(){
  if(!fs.existsSync(requestFile))return null;
  const request=JSON.parse(fs.readFileSync(requestFile));need(request.schemaVersion===1&&/^[a-z][a-z0-9-]{1,63}$/.test(request.name)&&Array.isArray(request.marketIds)&&request.marketIds.length>0&&request.marketIds.length<=3&&new Set(request.marketIds).size===request.marketIds.length&&request.marketIds.every(id=>universe.exchanges.some(x=>x.marketId===id)),'REQUEST');
  const dir=path.join(runtimeDir,'production-metadata',request.name);const marketId=request.marketIds.find(id=>!fs.existsSync(path.join(dir,id+'.json')));return marketId?{request,dir,marketId}:null;
 }
 async function consumeOne(){
  const p=pending();if(!p||!canOperate())return false;
  const endpoint='/markets/'+p.marketId+'/nodes?tournamentId='+universe.tournamentId;
  const fence=()=>need(canOperate()&&permits.get(fence)===endpoint,'STOPPED');permits.set(fence,endpoint);
  try{const r=await dispatch({method:'GET',endpoint,beforeDispatch:fence,beforeNetwork:fence});fence();
   need(r.status===200&&r.endpoint===endpoint&&Number.isFinite(Date.parse(r.requestStartedAt))&&Date.parse(r.requestStartedAt)<=Date.parse(r.receivedAt)&&Date.parse(r.receivedAt)<=now()&&now()-Date.parse(r.receivedAt)<=20000,'RECEIPT');
   const d=r.data;need(d?.market_id===p.marketId&&d.contexts?.length===1&&d.contexts[0].type==='tournament'&&d.contexts[0].tournament?.id===universe.tournamentId&&d.root?.node_type==='contract'&&d.root.contract_type==='Election Outcome','SCOPE');
   const record={schemaVersion:1,marketId:p.marketId,requestStartedAt:r.requestStartedAt,receivedAt:r.receivedAt,root:d.root,rootHash:payloadHash(d.root),responseHash:payloadHash(d),competitionOrders:0};
   fs.mkdirSync(p.dir,{recursive:true,mode:0o700});const f=path.join(p.dir,p.marketId+'.json');fs.writeFileSync(f,JSON.stringify(record)+'\n',{flag:'wx',mode:0o600});const fd=fs.openSync(f,'r+');try{fs.fsyncSync(fd);}finally{fs.closeSync(fd);}atomicJson(path.join(runtimeDir,'production-metadata-status.json'),{at:new Date(now()).toISOString(),name:p.request.name,completedMarketId:p.marketId,remaining:pending()?.request.marketIds.filter(id=>!fs.existsSync(path.join(p.dir,id+'.json')))||[],executionAuthority:false});return true;
  }finally{permits.delete(fence);}
 }
 return{authorizes,pending,consumeOne};
}
