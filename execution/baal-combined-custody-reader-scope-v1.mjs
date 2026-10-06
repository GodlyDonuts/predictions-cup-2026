import fs from 'node:fs';
import path from 'node:path';
import {payloadHash} from './core.mjs';
import {replayBaalScaledControllerV1,baalScaledNoControllerIdentity} from './baal-scaled-controller-legacy-replay-v1.mjs';

// Decoder scope only: financial authority remains with the combined host.
// Startup replay binds graph reads to current owned packages, including their
// counterpart. This creates no account client and makes no request.
export function readBaalCombinedCustodyReaderScopeV1({runtimeDir,universe}) {
 const need=(ok,code)=>{if(!ok)throw Error('CUSTODY_READER_SCOPE_'+code);};
 need(path.isAbsolute(runtimeDir),'RUNTIME');
 const descriptor=JSON.parse(fs.readFileSync(path.join(runtimeDir,'baal-combined-custody-request-v6.json')));
 need(descriptor.kind==='baal_combined_custody_host_v6'&&descriptor.ownerId==='Codex'
  &&descriptor.tournamentId===universe.tournamentId&&descriptor.universeHash===universe.hash
  &&/^baal-scaled-no-[a-z0-9-]{1,80}$/.test(descriptor.runId),'DESCRIPTOR');
 const file=path.join(runtimeDir,'baal-scaled-live-runs',descriptor.runId,'controller','baal-scaled-no-journal.jsonl');
 const seed=replayBaalScaledControllerV1({bodyText:fs.readFileSync(file,'utf8'),expectedCodeIdentity:baalScaledNoControllerIdentity()});
 need(payloadHash(seed)===descriptor.seedHash&&payloadHash(seed.anchor)===payloadHash(descriptor.legacyAnchor)
  &&seed.legacyCodeIdentityHash===descriptor.legacyCodeIdentityHash
  &&seed.state.scope.runId===descriptor.runId&&seed.state.scope.tournamentId===descriptor.tournamentId,'EXACT_SEAL');
 const members=new Map(universe.exchanges.map(l=>[l.exchangeId,l.marketId]));
 const ids=new Set();
 for(const [exchangeId,slot] of Object.entries(seed.state.inventory)) {
  if(slot.proof.remainingOwnedQuantity===0)continue;
  need(slot.proof.remainingOwnedQuantity>0&&slot.proof.exchangeId===exchangeId
   &&slot.package.legs.length===2,'OWNED_PACKAGE');
  for(const leg of slot.package.legs) {
   need(members.get(leg.exchangeId)===leg.marketId,'UNIVERSE_LEG');
   ids.add(leg.marketId);
  }
 }
 need(ids.size>0&&ids.size<=20,'GRAPH_SCOPE_BOUND');
 return {allowedOwnedGraphMarketIds:[...ids].sort((a,b)=>Number(a)-Number(b)),
  seedHash:descriptor.seedHash,executionAuthority:false,networkRequests:0};
}
