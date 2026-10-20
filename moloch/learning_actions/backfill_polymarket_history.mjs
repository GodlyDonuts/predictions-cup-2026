// Public price samples, never reconstructed books or original delivery clocks.
import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { Admission, assetId, sha256 } from '../external-data/polymarket-client.mjs';

const json=x=>JSON.stringify(x,null,2)+'\n';
export function historyUrl(asset,start,end){
  assetId(asset);
  if(!Number.isSafeInteger(start)||!Number.isSafeInteger(end)||start<0||end<=start||end-start>7*86400)throw Error('HISTORY_RANGE');
  const u=new URL('https://clob.polymarket.com/prices-history');
  u.search=new URLSearchParams({market:asset,startTs:String(start),endTs:String(end),fidelity:'1'});return u;
}
export function validate(receipt,asset,start,end){
  if(receipt.status!==200||receipt.url!==String(historyUrl(asset,start,end))||sha256(receipt.bodyText)!==receipt.bodySha256)throw Error('EXACT_PUBLIC_HISTORY_RECEIPT');
  const a=Date.parse(receipt.requestStartedAt),b=Date.parse(receipt.receivedAt);
  if(!Number.isFinite(a)||!Number.isFinite(b)||a>b)throw Error('HISTORY_RECEIPT_CLOCK');
  const rows=JSON.parse(receipt.bodyText).history;if(!Array.isArray(rows))throw Error('HISTORY_POINTS_SCHEMA');
  for(const r of rows)if(!Number.isSafeInteger(r.t)||r.t<start||r.t>end||r.t*1000>b||typeof r.p!=='number'||!Number.isFinite(r.p)||r.p<0||r.p>1)throw Error('HISTORY_POINT_OUTSIDE_SCOPE');
  return {records:rows.length,insideHalfOpenRange:rows.filter(r=>r.t<end).length};
}
async function atomic(file,value){const tmp=file+'.pending';await fs.writeFile(tmp,json(value));await fs.rename(tmp,file);}
export async function recover(root,plan,{fetcher=fetch,maximumRequests=0,admission=new Admission({rps:5})}={}){
  await fs.mkdir(root,{recursive:true});let old;
  try{old=await fs.readFile(path.join(root,'plan.json'),'utf8');}catch(e){if(e.code!=='ENOENT')throw e;}
  if(old!==undefined&&old!==json(plan))throw Error('IMMUTABLE_HISTORY_PLAN_CHANGED');
  if(old===undefined)await fs.writeFile(path.join(root,'plan.json'),json(plan),{flag:'wx'});
  const pages=path.join(root,'pages');await fs.mkdir(pages,{recursive:true});let completed=0,records=0,requests=0;
  async function status(state,extra={}){await atomic(path.join(root,'status.json'),{state,at:new Date().toISOString(),completedAssets:completed,totalAssets:plan.assets.length,records,requestsThisInvocation:requests,planSha256:sha256(json(plan)),trainingAdmitted:false,orderBooksReconstructed:false,originalHistoricalDeliveryVerified:false,productionChanged:false,...extra});}
  await status('recovering');
  for(let i=0;i<plan.assets.length;i++){
    const asset=plan.assets[i];let chunk=0;
    for(let start=plan.startTs;start<plan.endTs;start+=7*86400,chunk++){
      const end=Math.min(plan.endTs,start+7*86400),file=path.join(pages,`${String(i).padStart(4,'0')}-${chunk}.json`);let receipt;
      try{receipt=JSON.parse(await fs.readFile(file,'utf8'));}catch(e){if(e.code!=='ENOENT')throw e;}
      if(receipt===undefined){
        if(maximumRequests&&requests>=maximumRequests){await status('bounded_incomplete');return;}
        try{await fs.access(path.join(root,'STOP'));await status('stopped');return;}catch(e){if(e.code!=='ENOENT')throw e;}
        const u=historyUrl(asset,start,end);
        for(let attempt=0;;attempt++){
          if(maximumRequests&&requests>=maximumRequests){await status('bounded_incomplete');return;}
          await admission.reserve();const requestStartedAt=new Date().toISOString();
          const response=await fetcher(u,{redirect:'error',signal:AbortSignal.timeout(30000),headers:{Accept:'application/json'}});
          const bodyText=await response.text();requests++;
          if(Buffer.byteLength(bodyText)>8*1024**2)throw Error('PUBLIC_HISTORY_BODY_LIMIT');
          receipt={url:String(u),requestStartedAt,receivedAt:new Date().toISOString(),status:response.status,bodyText,bodySha256:sha256(bodyText)};
          if(response.status===200)break;
          // No identity/IP/header changes on denial. Preserve error evidence.
          await fs.writeFile(file+`.error-${requests}.json`,json(receipt),{flag:'wx'});
          if((response.status!==429&&response.status<500)||attempt>=7)throw Error(`PUBLIC_HISTORY_HTTP_${response.status}`);
          const retry=response.headers.get('retry-after');const delay=/^\d+(\.\d+)?$/.test(retry??'')?Number(retry)*1000:Date.parse(retry)-Date.now();
          admission.backoff(Math.max(1000,Number.isFinite(delay)?delay:Math.min(60000,1000*2**attempt)));
          await status('provider_backoff',{assetIndex:i,status:response.status});
        }
        validate(receipt,asset,start,end);await fs.writeFile(file+'.pending',json(receipt),{flag:'wx'});await fs.rename(file+'.pending',file);
      }
      records+=validate(receipt,asset,start,end).insideHalfOpenRange;await status('recovering',{assetIndex:i,chunk});
    }
    completed++;await status('recovering');
  }
  await status('complete_receipt_recovery_not_replay_qualification');
}
async function main(){
  const args={};for(let i=2;i<process.argv.length;i+=2){if(!['--universe','--output','--start','--end','--maximum-requests'].includes(process.argv[i])||!process.argv[i+1])throw Error('HISTORY_ARGUMENT');args[process.argv[i].slice(2)]=process.argv[i+1];}
  const raw=await fs.readFile(args.universe);const assets=[...new Set(JSON.parse(raw).outcomes.map(x=>assetId(x.assetId)))].sort();
  const startTs=Date.parse(args.start)/1000,endTs=Date.parse(args.end)/1000;
  if(!Number.isSafeInteger(startTs)||!Number.isSafeInteger(endTs)||endTs<=startTs||endTs*1000>Date.now())throw Error('EXPLICIT_PAST_HISTORY_RANGE');
  const maximumRequests=Number(args['maximum-requests']??0);if(!Number.isSafeInteger(maximumRequests)||maximumRequests<0)throw Error('HISTORY_REQUEST_BOUND');
  const plan={classification:'historical_price_samples_not_executable_depth',assets,startTs,endTs,fidelityMinutes:1,universeSha256:sha256(raw),runnerSha256:sha256(await fs.readFile(fileURLToPath(import.meta.url))),role:'training_history_recovery_only',competitionStartVerified:false,independentTestUsed:false,requestsPerSecond:5};
  await recover(args.output,plan,{maximumRequests});
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url))main().catch(e=>{console.error(e.message);process.exitCode=1;});
