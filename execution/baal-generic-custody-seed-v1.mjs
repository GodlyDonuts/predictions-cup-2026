// Pure private-state conversion after complete source replay. Preserves all
// attributable lots and closed/abandoned history; never grants write authority.
import {payloadHash} from './core.mjs';
import {assertBaalCustodyFrontierCanContinue} from './baal-custody-frontier-v1.mjs';
const need=(v,c)=>{if(!v)throw Error('GENERIC_CUSTODY_SEED_'+c);};
export function buildBaalGenericCustodySeedV1({frontier,scope,allocation}){
 assertBaalCustodyFrontierCanContinue(frontier);const s=frontier.currentState;
 for(const k of ['ownerId','runId','tournamentId'])need(scope[k]===s.scope[k]&&allocation[k]===s.allocation[k],'ATTRIBUTION_SCOPE');
 need(payloadHash(allocation)===payloadHash(s.allocation),'ORIGINAL_ALLOCATION');
 need(Array.isArray(scope.exchanges)&&new Set(scope.exchanges.map(e=>e.exchangeId)).size===scope.exchanges.length,'UNIQUE_SCOPE');
 for(const [ex,p]of Object.entries(s.currentInventory))need(scope.exchanges.some(e=>e.exchangeId===ex&&e.marketId===p.proof.marketId),'ALL_INHERITED_POSITIONS_IN_SCOPE');
 need([...s.originalOrders,...s.orders].every(o=>o.proof||o.phase==='abandoned'),'NO_INHERITED_PENDING');
 const seed={schemaVersion:1,kind:'baal_generic_custody_seed_v1',ownerId:scope.ownerId,runId:scope.runId,tournamentId:scope.tournamentId,scopeHash:payloadHash(scope),allocationHash:payloadHash(allocation),sourceStateHash:frontier.currentStateHash,sourceDescriptorHash:frontier.descriptorHash,sourceActorAnchor:structuredClone(frontier.currentActorAnchor),sourceHashes:structuredClone(frontier.sourceHashes),inventory:structuredClone(s.currentInventory),inheritedClosedOrders:structuredClone([...s.originalOrders,...s.orders]),inheritedPairAssignments:structuredClone(s.assignments),inheritedPairRetirements:structuredClone(s.pairRetirements),originalActivation:structuredClone(frontier.originalActivation),summary:structuredClone(frontier.summary),executionAuthority:false};
 return {...seed,seedHash:payloadHash(seed)};
}
export function validateBaalGenericCustodySeedV1(seed,scope,allocation){
 const {seedHash,...body}=seed??{};need(seed?.kind==='baal_generic_custody_seed_v1'&&seed.executionAuthority===false&&payloadHash(body)===seedHash,'HASH');
 need(seed.scopeHash===payloadHash(scope)&&seed.allocationHash===payloadHash(allocation),'CONFIGURATION');
 for(const k of ['ownerId','runId','tournamentId'])need(seed[k]===scope[k],'ATTRIBUTION');
 need(seed.inheritedClosedOrders.every(o=>o.proof||o.phase==='abandoned'),'PENDING');
 return seedHash;
}
