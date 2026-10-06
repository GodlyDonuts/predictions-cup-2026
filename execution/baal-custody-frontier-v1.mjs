// Read-only import of the complete existing economic frontier for a successor.
// Returns private receipts in memory; callers must never publish the state.
// No writer, account client, activation, order, lease or file mutation.
import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
import {payloadHash} from './core.mjs';
import {Journal} from '../live-paper/transport-runtime.mjs';
import {baalCombinedCustodyV6Identity} from './baal-combined-custody-host-v6.mjs';
import {replayBaalScaledControllerV1,baalScaledNoControllerIdentity} from './baal-scaled-controller-legacy-replay-v1.mjs';
import {replayLiveHedgeJournal,liveCoordinatorIdentity,projectRisk,pilotUnknown} from './baal-neutral-owned-counterpart-pilot-v2.mjs';
import {amount,money} from '../paper/fixed.mjs';
const need=(v,c)=>{if(!v)throw Error('CUSTODY_FRONTIER_'+c);},sha=b=>createHash('sha256').update(b).digest('hex');
function stable(file){const before=fs.statSync(file),bytes=fs.readFileSync(file),after=fs.statSync(file);need(before.ino===after.ino&&before.birthtimeMs===after.birthtimeMs&&before.size===after.size&&bytes.length===after.size,'SOURCE_CHANGED');return{bytes,identity:{ino:before.ino,birthtimeMs:before.birthtimeMs},hash:sha(bytes)};}
function summaryOf(state,risk){const owned=Object.values(state.currentInventory);return{ownedQuantity:owned.reduce((n,p)=>n+p.proof.remainingOwnedQuantity,0),ownedPrincipal:money(owned.reduce((n,p)=>n+amount(p.proof.remainingOwnedCost),0n)),realizedPnl:money(owned.reduce((n,p)=>n+amount(p.proof.cumulativeRealizedPnl),0n)),legacyOrders:state.originalOrders.length,currentCoordinatorOrders:state.orders.length,inheritedOrders:state.originalOrders.length+state.orders.length,abandonedOrders:[...state.originalOrders,...state.orders].filter(o=>o.phase==='abandoned').length,pairAssignments:state.assignments.length,pairRetirements:state.pairRetirements.length,unknownAcceptance:pilotUnknown(state),unresolvedIntentIds:[...risk.unresolvedIntentIds]};}
export function readBaalCustodyFrontierV1({runtimeDir,workspace}){
 need(path.isAbsolute(runtimeDir)&&path.isAbsolute(workspace),'PATHS');
 const descriptorFile=path.join(runtimeDir,'baal-combined-custody-request-v6.json'),descriptor=stable(descriptorFile),d=JSON.parse(descriptor.bytes);
 need(d.kind==='baal_combined_custody_host_v6'&&d.ownerId==='Codex'&&payloadHash(d.codeIdentity)===payloadHash(baalCombinedCustodyV6Identity(workspace)),'DESCRIPTOR_GRAPH');
 need(/^[a-zA-Z0-9_-]{1,100}$/.test(d.migration.implementationEpoch)&&/^baal-scaled-no-[a-z0-9-]{1,80}$/.test(d.runId),'SOURCE_PATH_COMPONENTS');
 const legacyFile=path.join(runtimeDir,'baal-scaled-live-runs',d.runId,'controller','baal-scaled-no-journal.jsonl'),legacy=stable(legacyFile),seed=replayBaalScaledControllerV1({bodyText:legacy.bytes.toString(),expectedCodeIdentity:baalScaledNoControllerIdentity()});
 need(payloadHash(seed)===d.seedHash&&payloadHash(seed.anchor)===payloadHash(d.legacyAnchor),'ORIGINAL_SEED');
 const dir=path.join(runtimeDir,'baal-combined-custody-runs',d.runId,d.migration.implementationEpoch),actorFile=path.join(dir,'coordinator','neutral-owned-counterpart-pilot-v2.jsonl'),actor=stable(actorFile),code=liveCoordinatorIdentity(workspace);
 const current=replayLiveHedgeJournal({bodyText:actor.bytes.toString(),seed,activation:d.activation,migration:d.migration,expectedCodeIdentity:code});
 const budgetFile=path.join(dir,'budget','owned-hedge-shared-budget-v2.jsonl'),budget=stable(budgetFile),journal=new Journal(budgetFile),last=journal.last;
 need(last?.event.kind==='installed_source_verified_hedge_reserve'&&last.state.seedHash===d.seedHash&&last.state.coordinatorCodeIdentityHash===payloadHash(code),'BUDGET_BINDING');
 const bound=last.state.actorAnchor;need(Number.isSafeInteger(bound?.bytes)&&bound.bytes>0&&bound.bytes<=actor.bytes.length&&sha(actor.bytes.subarray(0,bound.bytes))===bound.sha256,'BUDGET_ACTOR_PREFIX');
 const budgetActor=replayLiveHedgeJournal({bodyText:actor.bytes.subarray(0,bound.bytes).toString(),seed,activation:d.activation,migration:d.migration,expectedCodeIdentity:code});
 need(payloadHash(budgetActor.anchor)===payloadHash(bound)&&payloadHash(projectRisk(budgetActor.state))===payloadHash(last.state.projection.record),'BUDGET_ECONOMICS');
 const state=current.state,risk=projectRisk(state),summary=summaryOf(state,risk);
 const sourceRecords=[{file:descriptorFile,source:descriptor},{file:legacyFile,source:legacy},{file:actorFile,source:actor},{file:budgetFile,source:budget}];
 for(const {file,source}of sourceRecords){const fresh=stable(file);need(fresh.hash===source.hash&&payloadHash(fresh.identity)===payloadHash(source.identity),'FRONTIER_ADVANCED_DURING_IMPORT');}
 return{schemaVersion:1,kind:'baal_complete_custody_frontier_v1',descriptorHash:payloadHash(d),originalSeedHash:d.seedHash,originalActivation:structuredClone(d.activation),migration:structuredClone(d.migration),legacyAnchor:seed.anchor,currentActorAnchor:current.anchor,budgetAnchor:{bytes:budget.bytes.length,sha256:budget.hash,sequence:journal.sequence,hash:journal.previous},currentStateHash:payloadHash(state),currentState:state,risk,summary,sourceHashes:sourceRecords.map(({file,source})=>({relative:path.relative(runtimeDir,file),bytes:source.bytes.length,sha256:source.hash})),executionAuthority:false,networkRequests:0,originalJournalsChanged:false};
}
export function assertBaalCustodyFrontierCanContinue(frontier){
 need(frontier?.kind==='baal_complete_custody_frontier_v1'&&frontier.executionAuthority===false&&payloadHash(frontier.currentState)===frontier.currentStateHash,'EXACT_FRONTIER');
 need(payloadHash(summaryOf(frontier.currentState,frontier.risk))===payloadHash(frontier.summary),'SUMMARY_ECONOMICS');
 need(frontier.summary.unknownAcceptance===false&&frontier.summary.unresolvedIntentIds.length===0&&frontier.risk.unresolvedIntentIds.length===0,'UNRESOLVED_ACCEPTANCE_OR_INTENT');
 need(payloadHash(projectRisk(frontier.currentState))===payloadHash(frontier.risk),'CURRENT_FULL_RESERVATION');
 return{continuationStateHash:frontier.currentStateHash,sourceActorAnchor:frontier.currentActorAnchor,summary:structuredClone(frontier.summary),executionAuthority:false,freshAccountStillRequired:true,newActivationStillRequired:true};
}
