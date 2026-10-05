import fs from 'node:fs';import path from 'node:path';import {randomUUID} from 'node:crypto';
import {loadStableMakerSources,stableMakerEvents} from './stable-maker-replay-v2.mjs';
import {createCoupledMakerState,advanceCoupledMaker,coupledMakerReport} from './coupled-maker-paper-v1.mjs';
export function replayCoupledMakerEvents(events,bank){
 let state=createCoupledMakerState(bank),accounts=[],processed=0;const cutoff=Date.parse(bank.cutoff);
 for(const e of events){if(e.kind==='eligibility'){accounts.push(e);continue;}if(Date.parse(e.availableAt)<cutoff)continue;
  if(e.kind==='book'){const request=Date.parse(e.book.requestStartedAt);let causal=null;
   for(let i=accounts.length-1;i>=0;i--){const a=accounts[i];if(Date.parse(a.snapshot.positionsAt)<=request&&Date.parse(a.snapshot.ordersAt)<=request&&Date.parse(a.availableAt)<=Date.parse(e.availableAt)){causal=a;break;}}
   state=advanceCoupledMaker(state,{...e,accountEligibility:causal?.snapshot||null},bank);
  }else state=advanceCoupledMaker(state,e,bank);processed++;
 }
 return{state,processed,report:coupledMakerReport(state,bank,state.lastAt)};
}
export async function runCoupledMakerReplay({runtimeDir,bank,executable,binaryHash,out,workingDir=path.join(runtimeDir,'coupled-maker-diagnostics',randomUUID())}){
 const started=performance.now(),sources=await loadStableMakerSources({runtimeDir,workingDir,executable,binaryHash}),events=stableMakerEvents(sources,bank),loaded=performance.now(),replay=replayCoupledMakerEvents(events,bank),finished=performance.now();
 const record={schemaVersion:1,at:new Date().toISOString(),mode:'coupled_maker_actual_capture_development_replay_v1',bank,bindings:sources.bindings,nativeVerification:sources.nativeVerification,
  firstAt:events[0]?.availableAt,through:events.at(-1)?.availableAt,processed:replay.processed,timing:{sourceMs:loaded-started,replayMs:finished-loaded,totalMs:finished-started},report:replay.report,
  fills:replay.state.wallets.COUPLED.fills,redemptions:replay.state.wallets.COUPLED.redemptions,networkRequests:0,competitionOrders:0,fees:0,profitabilityEstablished:false,
  limitation:'Historical development diagnostic. The model was implemented after this capture; hypothetical fills are not actual orders or prospective proof.'};
 fs.writeFileSync(out,JSON.stringify(record,null,2)+'\n',{flag:'wx',mode:0o600});return record;
}
