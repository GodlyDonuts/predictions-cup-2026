import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
import {atomicJson,payloadHash} from '../execution/ledger.mjs';
import {validatePriceObservation} from './market-price-publication.mjs';
import {screenPartyWinnerNoPairs,partyWinnerQuoteHash} from './party-winner-no-screen-v1.mjs';
import {selectPartyWinnerPairCapture,createPartyPairSelectorState} from './party-winner-pair-selector-v1.mjs';
const need=(ok,code)=>{if(!ok)throw Error(code);};
// Schedules existing paired-candidate reads from current published bulk data.
// No client, secret, timer, order or additional ordinary book slot is created.
export function openPartyWinnerPairCollector({runtimeDir,universe,getAccount,getCapture,getBoundPlanHash,canOperate,now=Date.now}){
 const planFile=path.join(runtimeDir,'party-pair-source-plan.json');if(!fs.existsSync(planFile))return null;
 const plan=JSON.parse(fs.readFileSync(planFile)),planHash=payloadHash(plan),catalogFile=path.join(runtimeDir,'party-pair-root-catalog.json'),catalog=JSON.parse(fs.readFileSync(catalogFile));
 need(plan.universeHash===universe.hash&&catalog.universeHash===universe.hash&&Array.isArray(catalog.rootCaptures)&&payloadHash(catalog.rootCaptures)===plan.rootCatalogHash,'PARTY_PAIR_CATALOG');
 const stateFile=path.join(runtimeDir,'party-pair-selector-state.json'),requestFile=path.join(runtimeDir,'paired-candidate-request.json'),statusFile=path.join(runtimeDir,'party-pair-selector-status.json');
 let state=fs.existsSync(stateFile)?JSON.parse(fs.readFileSync(stateFile)):createPartyPairSelectorState(universe.hash),quotes=new Map(),sequence=null,previous=null,lastReceivedAt=null,lastKnownAt=null,failed=false,reason='awaiting_bulk_publication';
 const fixed=()=>!failed&&canOperate()===true&&payloadHash(JSON.parse(fs.readFileSync(planFile)))===planHash&&payloadHash(JSON.parse(fs.readFileSync(catalogFile)))===payloadHash(catalog);
 const persist=(file,value)=>{try{atomicJson(file,value);}catch(error){failed=true;reason='durability_failure';error.fatal=true;error.durableRecordingFailed=true;throw error;}};
 function schedule(){if(!fixed())return;const account=getAccount();if(!account)return;const capture=getCapture(),request=fs.existsSync(requestFile)?JSON.parse(fs.readFileSync(requestFile)):null;
  const accountView={complete:account.complete===true,tournamentId:universe.tournamentId,availableAt:account.availableAt,positionsAt:account.snapshot.positionsAt,ordersAt:account.snapshot.ordersAt,excludedExchangeIds:account.snapshot.excludedExchangeIds,unknownAcceptance:account.unknownAcceptance===true,sourceHash:payloadHash(account)};
  const at=new Date(now()).toISOString(),screen=screenPartyWinnerNoPairs({universe,rootCaptures:catalog.rootCaptures,quoteCaptures:[...quotes.values()],account:accountView,now:at});
  const terminalRecords=capture&&['complete','failed','interrupted'].includes(capture.phase)?[{sourceId:capture.name,name:capture.name,marketIds:capture.legs?.map(l=>l.marketId)||request?.marketIds,phase:capture.phase,updatedAt:capture.at,positiveConditionalScreen:capture.screen?.positiveConditionalScreen===true}]:[];
  const result=selectPartyWinnerPairCapture({universe,screen,account:accountView,state,sourcePlan:plan,currentRequest:request,capture,terminalRecords,boundPlanHash:getBoundPlanHash(),collectorCanRead:true,now:at});
  // Record the admission proposal before installing the existing collector's
  // request. A crash never turns the same source into a duplicate request.
  persist(stateFile,result.nextState);state=result.nextState;reason=result.reason;
  if(result.request)persist(requestFile,result.request);
  persist(statusFile,{schemaVersion:1,observedAt:at,reason,selected:result.selected,request:result.request,sourcePlanHash:planHash,priceSequence:sequence,priceHash:previous,rootPairs:screen.semanticPairCount,orders:0,additionalClients:0,ordinaryBookSlots:4,auxiliaryBookSlots:2});
 }
 return{onPublished(row){if(!fixed())return;const {hash,...body}=row;need(hash===createHash('sha256').update(JSON.stringify(body)).digest('hex')&&Number.isSafeInteger(row.sequence)&&row.sequence>0&&(row.previous===''||typeof row.previous==='string'&&/^[a-f0-9]{64}$/.test(row.previous))&&row.event?.type==='market_price_observation'&&row.state===null,'PARTY_PAIR_PRICE_SOURCE');if(sequence!==null)need(row.sequence===sequence+1&&row.previous===previous,'PARTY_PAIR_PRICE_CHAIN');const snapshot=validatePriceObservation(row.event.snapshot,universe),known=Date.parse(row.at),received=Date.parse(snapshot.receivedAt);need(Number.isSafeInteger(known)&&new Date(known).toISOString()===row.at&&received<=known&&known<=now()&&(lastReceivedAt===null||received>=lastReceivedAt)&&(lastKnownAt===null||known>=lastKnownAt),'PARTY_PAIR_PRICE_CLOCK');
   for(const id of snapshot.missingIds)quotes.delete(id);for(const q of snapshot.quotes){const value={...q,tournamentId:universe.tournamentId,requestStartedAt:snapshot.requestStartedAt,receivedAt:snapshot.receivedAt,knownAt:row.at,sourceHash:hash};value.quoteHash=partyWinnerQuoteHash(value);quotes.set(q.exchangeId,value);}sequence=row.sequence;previous=hash;lastReceivedAt=received;lastKnownAt=known;schedule();},afterAccountRefresh:schedule,status:()=>({reason,priceSequence:sequence,knownQuotes:quotes.size,failed,additionalClients:0,orders:0})};
}
