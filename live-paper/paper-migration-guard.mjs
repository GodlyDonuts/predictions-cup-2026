import {payloadHash} from '../execution/ledger.mjs';
const need=(v,c)=>{if(!v)throw Error('PAPER_MIGRATION_'+c);};
// A comparison gate, not an importer, lease or activation. Native source and
// economic replay must independently establish every proposed state first.
export function validatePaperMigration({observedAt,maximumAgeMs=30000,current,proposed,now=Date.now()}){
 need(Number.isSafeInteger(now)&&Number.isSafeInteger(maximumAgeMs)&&maximumAgeMs>0&&Number.isFinite(Date.parse(observedAt))&&Date.parse(observedAt)<=now&&now-Date.parse(observedAt)<=maximumAgeMs,'STALE_OBSERVATION');
 need(Array.isArray(current)&&Array.isArray(proposed),'MANIFEST');const indexes=rows=>{const map=new Map();for(const r of rows){need(typeof r.cohort==='string'&&!map.has(r.cohort),'DUPLICATE_COHORT');map.set(r.cohort,r);}return map;};const owners=indexes(current),candidate=indexes(proposed);
 need(owners.size===candidate.size&&[...owners.keys()].every(k=>candidate.has(k)),'MISSING_OR_ADOPTED_COHORT');
 for(const [id,old]of owners){const next=candidate.get(id);need(next.originalBankHash===old.bankHash&&next.originalBankMode===old.bankMode,'BANK_FAMILY_OR_ID');
  need(payloadHash(next.originalAnchors)===payloadHash(old.anchors),'ORIGINAL_ANCHORS');
  need(next.status===old.status,'RETIREMENT_OR_DRAIN');
  if(old.authoritativeOwner==='operator_overlay')need(next.authoritativeOwner==='operator_overlay'&&next.overlayHash===old.overlayHash&&next.baseActorEnabled===false,'FENCED_BASE_RESURRECTION');
  else need(next.authoritativeOwner===old.authoritativeOwner,'OWNER');
  need(next.nativeReplayVerified===true&&next.originalSourceReplayVerified===true,'NATIVE_REPLAY_UNPROVEN');
  need(payloadHash(next.accounting)===payloadHash(old.accounting),'CASH_BASIS_OR_LIABILITY');
  if(old.status==='retired_flat')need(Object.values(old.accounting.wallets).every(w=>w.positions.length===0&&w.pending.length===0&&w.unresolvedQuotes===0&&w.reservedCost==='0.000000'),'RETIREMENT_NOT_FLAT');
 }
 return{compatible:true,cohorts:owners.size,authoritativeInventoryPreserved:true,activationAuthority:false,contestOrders:0};
}
