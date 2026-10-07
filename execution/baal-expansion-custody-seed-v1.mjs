import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
import {payloadHash} from './core.mjs';
import {buildBaalCohortRecoveredCustodySeedV4} from './baal-cohort-recovered-custody-seed-v4.mjs';
import {replayBaalGenericNoJournalV7,baalGenericNoControllerV7Identity} from './baal-generic-no-controller-v7.mjs';
const need=(v,c)=>{if(!v)throw Error('EXPANSION_SEED_'+c);},sha=b=>createHash('sha256').update(b).digest('hex');
// New flat run, with complete verified old custody preserved as an immutable archive.
// Historical gains are not imported into the new run's spending/profit buffer.
export function buildBaalExpansionCustodySeedV1({frontier,scope,allocation,runtimeDir}){
 const reqFile=path.join(runtimeDir,'cohort-profit-lock-request-v2.json'),r=JSON.parse(fs.readFileSync(reqFile)),base=buildBaalCohortRecoveredCustodySeedV4({frontier,scope:r.scope,allocation:r.allocation,runtimeDir});
 const actorFile=path.join(runtimeDir,'baal-generic-live-runs',r.runId,r.epoch,'controller','baal-generic-no-v7-journal.jsonl'),bytes=fs.readFileSync(actorFile),replay=replayBaalGenericNoJournalV7({bodyText:bytes.toString(),expectedCodeIdentity:baalGenericNoControllerV7Identity(),custodySeed:base}),s=replay.state;
 need(Object.values(s.inventory).every(x=>x.proof.remainingOwnedQuantity===0)&&s.orders.every(o=>o.proof&&o.phase==='reconciled'||o.phase==='abandoned'),'PRIOR_NOT_FLAT_OR_PENDING');
 const authorityFile=path.join(runtimeDir,'baal-expansion-authority-v1.json'),authority=JSON.parse(fs.readFileSync(authorityFile));
 need(payloadHash(authority.scope)===payloadHash(scope)&&payloadHash(authority.allocation)===payloadHash(allocation)&&authority.priorActor.sha256===sha(bytes)&&authority.priorActor.bytes===bytes.length&&authority.priorActor.hash===replay.anchor.hash,'EXACT_AUTHORITY_AND_SOURCE');
 const {seedHash,...old}=base,orders=[...base.inheritedClosedOrders,...s.orders],sources=[reqFile,actorFile,authorityFile].map(file=>{const b=fs.readFileSync(file);return{relative:path.relative(runtimeDir,file),bytes:b.length,sha256:sha(b)};});
 const seed={...old,kind:'baal_expansion_flat_custody_seed_v1',ownerId:scope.ownerId,runId:scope.runId,tournamentId:scope.tournamentId,scopeHash:payloadHash(scope),allocationHash:payloadHash(allocation),inventory:{},ownedRuleHashes:{},inheritedClosedOrders:orders,inheritedPairAssignments:[],inheritedPairRetirements:[],originalActivation:authority.activationBounds,expansionSources:sources,archivedPrior:{runId:r.runId,actorAnchor:replay.anchor,inventory:s.inventory,closedIntentCount:orders.length},summary:{ownedQuantity:0,ownedPrincipal:'0.000000',realizedPnl:'0.000000',inheritedOrders:orders.length,abandonedOrders:orders.filter(o=>o.phase==='abandoned').length},executionAuthority:false};
 return{...seed,seedHash:payloadHash(seed)};
}
export function validateBaalExpansionCustodySeedV1(seed,scope,allocation){const{seedHash,...body}=seed??{};need(seed?.kind==='baal_expansion_flat_custody_seed_v1'&&seed.executionAuthority===false&&payloadHash(body)===seedHash&&seed.scopeHash===payloadHash(scope)&&seed.allocationHash===payloadHash(allocation),'HASH_SCOPE');for(const k of['ownerId','runId','tournamentId'])need(seed[k]===scope[k],'ATTRIBUTION');need(Object.keys(seed.inventory).length===0&&seed.inheritedClosedOrders.every(o=>o.proof||o.phase==='abandoned'),'FLAT_CLOSED');return seedHash;}
