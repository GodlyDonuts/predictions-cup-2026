import{openBaalCombinedCustodyHostV7,baalCombinedCustodyV7Identity}from'../execution/baal-combined-custody-host-v7.mjs';
// OFFLINE ONLY: actual sealed history, hypothetical fresh responses, zero network.
// Raw copies stay in a new child of the already restricted paper runtime.
import fs from 'node:fs';
import path from 'node:path';
import {createHash,randomUUID} from 'node:crypto';
import {performance} from 'node:perf_hooks';
import {fileURLToPath} from 'node:url';
import {ReadClient} from '../live-paper/transport-runtime.mjs';
import {validateUniverse} from '../live-paper/breadth.mjs';
import {createScopedRestReceiptsV5} from '../execution/scoped-rest-receipts-v5.mjs';
import {createBaalCombinedCustodyV6Descriptor,baalCombinedCustodyV6Identity} from '../execution/baal-combined-custody-host-v6.mjs';
import {replayBaalScaledControllerV1,baalScaledNoControllerIdentity} from '../execution/baal-scaled-controller-legacy-replay-v1.mjs';
import {payloadHash,createExecutionState} from '../execution/core.mjs';
import {acquireOwnerLease} from '../execution/ledger.mjs';
import {amount,money,price} from '../paper/fixed.mjs';

const exitOnly=process.argv.includes('--exit');
const workspace=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const sourceRuntime=path.resolve(process.env.SIG_PAPER_DIR??path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime'));
const sha=b=>createHash('sha256').update(b).digest('hex'),copy=structuredClone;
const need=(v,c)=>{if(!v)throw Error('OFFLINE_CURRENT_PREFIX_'+c);};
const request=JSON.parse(fs.readFileSync(path.join(sourceRuntime,'baal-scaled-live-request.json')));
const relative=path.join('baal-scaled-live-runs',request.runId,'controller','baal-scaled-no-journal.jsonl');
const originalFile=path.join(sourceRuntime,relative),originalBytes=fs.readFileSync(originalFile);
const seed=replayBaalScaledControllerV1({bodyText:originalBytes.toString(),expectedCodeIdentity:baalScaledNoControllerIdentity()});
need(sha(fs.readFileSync(originalFile))===seed.anchor.sha256,'SEAL_CHANGED');
const firstEx='1070',owned=seed.state.inventory[firstEx];
need(owned?.proof.remainingOwnedQuantity>0,'FIRST_LOT_MUST_STILL_EXIST');
const first=owned.package.legs.find(x=>x.exchangeId===firstEx),other=owned.package.legs.find(x=>x.exchangeId!==firstEx);
need(!seed.state.inventory[other.exchangeId]?.proof.remainingOwnedQuantity,'COUNTERPART_NOT_FLAT');
let roots=null;
for(const line of originalBytes.toString().trimEnd().split('\n')){
 const r=JSON.parse(line),e=r.event.input;
 if(e.kind==='prepared'&&e.evidence?.rootReceipts?.some(x=>x.exchangeId===firstEx))roots=e.evidence.rootReceipts;
}
need(roots?.length===2&&roots.every(x=>owned.package.legs.some(l=>l.exchangeId===x.exchangeId&&l.marketId===x.marketId)),'ROOT_SOURCES');
const latest=Object.values(seed.state.inventory).sort((a,b)=>Date.parse(b.proof.readAt)-Date.parse(a.proof.readAt))[0];
const cash=JSON.parse(latest.after.cash.bodyText),positions=JSON.parse(latest.after.positions.bodyText);
need(positions.positions.some(p=>p.exchangeId===firstEx&&p.quantity===-owned.proof.remainingOwnedQuantity),'ACTUAL_LATEST_POSITION');
const scoped=JSON.parse(owned.after.scopedOrdersPages[0].bodyText);
const privateDir=path.join(sourceRuntime,'offline-custody-v6-'+randomUUID());
need(path.dirname(privateDir)===sourceRuntime,'PRIVATE_PATH');fs.mkdirSync(privateDir,{mode:0o700});
const files=['baal-scaled-live-request.json','baal-scaled-host-upgrade.json','baal-scaled-host-upgrade-v3.json','baal-scaled-host-upgrade-v5.json','baal-capital-allocation.json','paired-live-authority.json','baal-scaled-rootcatalog.json'];
for(const f of files)fs.copyFileSync(path.join(sourceRuntime,f),path.join(privateDir,f));
fs.mkdirSync(path.dirname(path.join(privateDir,relative)),{recursive:true,mode:0o700});fs.writeFileSync(path.join(privateDir,relative),originalBytes,{mode:0o600});
const universe=validateUniverse(JSON.parse(fs.readFileSync(path.join(workspace,'live-paper','price-universe.json'))),request.tournamentId);
const base=Date.now()+1000,start=performance.now();let virtualDelay=0,host=null;
const now=()=>base+virtualDelay+Math.floor(performance.now()-start),iso=()=>new Date(now()).toISOString();
const descriptor=createBaalCombinedCustodyV6Descriptor({workspace,universe,originalRequest:request,priorV5Upgrade:JSON.parse(fs.readFileSync(path.join(privateDir,'baal-scaled-host-upgrade-v5.json'))),seed,firstExchangeId:firstEx,implementationEpoch:'offline-current-prefix-'+randomUUID(),startedAt:iso(),hedgeExpiresAt:new Date(now()+3600000).toISOString(),exitExpiresAt:new Date(now()+86400000).toISOString()});
fs.writeFileSync(path.join(privateDir,'baal-combined-custody-request-v6.json'),JSON.stringify(descriptor)+'\n',{mode:0o600});
fs.writeFileSync(path.join(privateDir,'baal-combined-read-upgrade-v7.json'),JSON.stringify({schemaVersion:1,kind:'baal_combined_read_upgrade_v7',ownerId:descriptor.ownerId,runId:descriptor.runId,tournamentId:descriptor.tournamentId,originalDescriptorHash:payloadHash(descriptor),codeIdentity:baalCombinedCustodyV7Identity(workspace),recordedAt:iso()})+'\n',{mode:0o600});
const coreLease=acquireOwnerLease({dir:path.join(privateDir,'offline-actual-core-lease'),ownerId:'Codex',runId:'offline-core',scope:request.tournamentId,now});
const core=createExecutionState({ownerId:'Codex',runId:'offline-core',tournamentId:request.tournamentId,accountCapital:'1000.000000'});
const page=data=>({data,pagination:{limit:200,hasMore:false,nextCursor:null},coverage:{complete:true,projectedThroughSequence:99999999}});
const calls=[];
const producer=createScopedRestReceiptsV5({scope:{tournamentId:request.tournamentId,tournamentSlug:request.tournamentSlug,marketIds:owned.package.legs.map(x=>x.marketId),allowedOwnedGraphMarketIds:owned.package.legs.map(x=>x.marketId)},now,onBeforeFetch:e=>host?.onBeforeFetch(e),onFetchStart:e=>host?.onFetchStart(e),fetchFn:async(url,options)=>{
 const u=new URL(url),endpoint=u.pathname.slice('/api/v1'.length)+u.search;calls.push({method:options.method,endpoint,at:iso()});let d;
 if(options.method==='POST'){
  const b=JSON.parse(options.body);need(exitOnly?b.action==='sell'&&b.quantity>0&&b.quantity<=100&&b.exchangeId===firstEx:b.action==='buy'&&b.quantity===1&&b.exchangeId===other.exchangeId,'ONLY_HYPOTHETICAL_BOUND_ORDER');
  const unit=price(String(b.price));d={orderId:990000001,exchangeId:b.exchangeId,side:'no',action:b.action,quantity:b.quantity,quantityTraded:b.quantity,remainingQuantity:0,open:false,price:b.price,fillPrice:b.price,totalCost:Number(money(unit*BigInt(b.quantity))),all:null};
 }else if(endpoint.includes('/nodes?'))d=JSON.parse(roots.find(r=>endpoint.includes('/'+r.marketId+'/')).receipt.bodyText);
 else if(endpoint==='/tournaments/'+request.tournamentSlug)d=copy(cash);
 else if(endpoint.endsWith('/portfolio/positions'))d=copy(positions);
 else if(endpoint.startsWith('/orders?status=open'))d=page([]);
 else if(endpoint.startsWith('/orders?status=all'))d=u.searchParams.get('exchangeId')===firstEx?copy(scoped):page([]);
 else if(endpoint.includes('/orderbook?')){
  const limit=990000n-amount(owned.proof.remainingLots[0].entryPrice),bid=exitOnly?995000n-(amount(owned.proof.remainingLots[0].entryPrice)+10000n):1000000n-(limit-5000n);
  d={contexts:[{type:'tournament',tournament:{id:request.tournamentId},orderbook:{exchanges:[{exchangeId:exitOnly?firstEx:other.exchangeId,option:'YES',asOf:{sequence:999999999,at:iso()},bids:[{price:Number(money(bid)),quantity:300}],asks:[{price:Number(money(bid+5000n)),quantity:300}]}]}}]};
 }else if(endpoint.startsWith('/relationships/graph?'))d={nodes:[],relationships:[]};
 else if(endpoint.startsWith('/portfolio/collateral?'))d={data:[]};else throw Error('OFFLINE_UNEXPECTED_ROUTE');
 virtualDelay+=100;return new Response(JSON.stringify(d),{status:200});
}});
const api=new ReadClient('OFFLINE-NONCREDENTIAL',{fetchFn:producer.fetch,now,sleepFn:async ms=>{virtualDelay+=ms;},networkBudget:{schemaVersion:2,ownerId:'Codex',enabled:true,intervalMs:1000,writeIntervalMs:2500,soleAccountClient:true,formerWriterStopCommentId:5947535773},authorizeDispatch:s=>host?.authorizes(s)===true,monitorEventLoop:false});
let output;
try{
 host=openBaalCombinedCustodyHostV7({runtimeDir:privateDir,workspace,universe,api,receiptOf:producer.receiptOf,getAccountLease:()=>({ownerId:'Codex',tournamentId:request.tournamentId,leaseId:payloadHash(coreLease.identity)}),getCoreOwnerRecord:()=>{coreLease.assertOwnership();return coreLease.identity;},getOtherState:()=>core,getLegacyRisk:()=>({complete:true,sourceHash:'a'.repeat(64),currentOwnedQuantity:0,currentReservedCash:'0.000000',unresolvedIntentIds:[]}),canOperate:()=>true,hasReadHeadroom:()=>true,getPairReadHints:()=>exitOnly?{tournamentId:request.tournamentId,universeHash:universe.hash,quoteCaptures:[{marketId:first.marketId,exchangeId:firstEx,option:'YES',tournamentId:request.tournamentId,bestBid:Number(money(985000n-amount(owned.proof.remainingLots[0].entryPrice))),bestAsk:Number(money(990000n-amount(owned.proof.remainingLots[0].entryPrice))),crossed:false,sourceHash:'e'.repeat(64),requestStartedAt:new Date(now()-200).toISOString(),receivedAt:new Date(now()-100).toISOString()}]}:null,now});
 const s=await host.step();output={schemaVersion:1,mode:'offline_current_sealed_prefix_hypothetical_packet_cpu_and_queue',observedAt:new Date().toISOString(),scenario:exitOnly?'priority_owned_sell':'one_share_hedge',sourceAnchor:seed.anchor,sourceCodeHash:payloadHash(baalCombinedCustodyV6Identity(workspace)),originalRealizedPnl:money(Object.values(seed.state.inventory).reduce((n,x)=>n+amount(x.proof.cumulativeRealizedPnl),0n)),actualCurrentSeedShares:Object.values(seed.state.inventory).reduce((n,x)=>n+x.proof.remainingOwnedQuantity,0),action:s.lastAction,reason:s.lastFailure,dispatch:s.lastDispatchTiming,rejection:s.lastRejectedTiming,stages:s.stageTimings,riskTimings:s.recentLocalTimings,coordinatorTimings:s.controller.recentLocalTimings,lastGuardFailure:s.controller.lastGuardFailure??null,simulatedGetCalls:calls.filter(c=>c.method==='GET').length,simulatedPostCalls:calls.filter(c=>c.method==='POST').length,actualNetworkCalls:0,actualAccountEvidence:false,actualExecutionAuthority:false,actualFillOrProfitClaim:false,originalSourceUnchanged:sha(fs.readFileSync(originalFile))===seed.anchor.sha256};
}finally{try{host?.close();}finally{api.close();producer.close();coreLease.release();}}
fs.writeFileSync(path.join(privateDir,'curated-offline-timing.json'),JSON.stringify(output,null,2)+'\n',{mode:0o600});
process.stdout.write(JSON.stringify(output)+'\n');
