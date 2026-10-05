import fs from 'node:fs';import path from 'node:path';import {createHash,randomUUID} from 'node:crypto';
import {sealedNativePrefix} from './native-dip-host-v1.mjs';import {loadStableMakerSources,stableMakerEvents,verifyChainedRows} from './stable-maker-replay-v2.mjs';
import {createCoupledBulkState,advanceCoupledBulk,coupledBulkReport} from './coupled-maker-paper-v2.mjs';
const sha=b=>createHash('sha256').update(b).digest('hex');
export function replayCoupledBulkEvents(events,bank){let state=createCoupledBulkState(bank),accounts=[],processed=0;const cutoff=Date.parse(bank.cutoff);
 for(const event of events){if(event.kind==='eligibility'){accounts.push(event);continue;}if(Date.parse(event.availableAt)<cutoff)continue;
  if(event.kind==='book'){const request=Date.parse(event.book.requestStartedAt);let causal=null;
   for(let i=accounts.length-1;i>=0;i--){const a=accounts[i];if(Date.parse(a.snapshot.positionsAt)<=request&&Date.parse(a.snapshot.ordersAt)<=request&&Date.parse(a.availableAt)<=request){causal=a;break;}}
   state=advanceCoupledBulk(state,{...event,accountEligibility:causal?.snapshot||null},bank);
  }else state=advanceCoupledBulk(state,event,bank);processed++;
 }return{state,processed,report:coupledBulkReport(state,bank,state.lastAt)};
}
export async function runCoupledBulkReplay({runtimeDir,bank,executable,binaryHash,out,workingDir=path.join(runtimeDir,'coupled-bulk-diagnostics',randomUUID())}){
 const started=performance.now(),sources=await loadStableMakerSources({runtimeDir,workingDir,executable,binaryHash}),priceFile=path.join(runtimeDir,'market-prices.jsonl'),prefix=sealedNativePrefix(priceFile),bytes=fs.readFileSync(priceFile).subarray(0,prefix.bytes),priceRows=verifyChainedRows(bytes);
 sources.bindings.prices={bytes:prefix.bytes,hash:sha(bytes),dev:prefix.dev,ino:prefix.ino};const stat=fs.statSync(priceFile);if(stat.dev!==prefix.dev||stat.ino!==prefix.ino||stat.size<prefix.bytes)throw Error('COUPLED_BULK_REPLAY_PRICE_REPLACED');
 const events=stableMakerEvents(sources,bank);for(const row of priceRows){if(row.event?.type!=='market_price_observation')throw Error('COUPLED_BULK_REPLAY_PRICE_TYPE');events.push({kind:'prices',availableAt:row.at,snapshot:row.event.snapshot});}
 events.sort((a,b)=>Date.parse(a.availableAt)-Date.parse(b.availableAt)||(a.kind==='eligibility'?-1:b.kind==='eligibility'?1:0));
 const loaded=performance.now(),replay=replayCoupledBulkEvents(events,bank),finished=performance.now(),record={schemaVersion:1,at:new Date().toISOString(),mode:'coupled_bulk_actual_capture_development_replay_v2',bank,bindings:sources.bindings,nativeVerification:sources.nativeVerification,
  firstAt:events[0]?.availableAt,through:events.at(-1)?.availableAt,processed:replay.processed,timing:{sourceMs:loaded-started,replayMs:finished-loaded,totalMs:finished-started},report:replay.report,fills:replay.state.wallets.COUPLED.fills,redemptions:replay.state.wallets.COUPLED.redemptions,
  networkRequests:0,competitionOrders:0,fees:0,profitabilityEstablished:false,limitation:'Historical development diagnostic on original sequential receipts. The strategy was implemented after these captures; strict-through fills are counterfactual and cannot prove production profitability.'};
 fs.writeFileSync(out,JSON.stringify(record,null,2)+'\n',{flag:'wx',mode:0o600});return record;
}
