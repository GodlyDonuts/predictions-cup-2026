import {instant} from '../paper/fixed.mjs';

const need=(ok,code)=>{if(!ok)throw Object.assign(Error(code),{code});};
// Sequential orchestration inside the existing collector, not another worker.
// Reads finish before admission; the controller still owns both write fences.
export function createPairedRestCycle({controller,bridge,entryAllowed,canOperate,recoveryAllowed=()=>true,now=Date.now,recoveryMinimumMs=90000,onStatus=()=>{}}){
 need(controller&&bridge&&[entryAllowed,canOperate,recoveryAllowed,now,onStatus].every(f=>typeof f==='function'),'PAIRED_CYCLE_DEPENDENCIES');
 need(Number.isSafeInteger(recoveryMinimumMs)&&recoveryMinimumMs>=90000,'PAIRED_CYCLE_RECOVERY_INTERVAL');
 let busy=false,closed=false,lastAction=null;
 function report(action){lastAction=action;const state=controller.status();onStatus({action,state,observedAt:new Date(now()).toISOString()});return{action,state};}
 async function step(){
  need(!busy&&!closed,'PAIRED_CYCLE_BUSY_OR_CLOSED');busy=true;
  try{
   if(!canOperate())return report('operational_stop');
   const state=controller.status();if(state.failed||state.closed)return report('controller_failed_or_closed');
   const current=state.orders.find(o=>o.phase!=='reconciled');
   if(!current){
    if(['paired','orphaned','unfilled'].includes(state.phase))return report('known_terminal_pair_state');
    if(!entryAllowed()||now()>=instant(state.reviewExpiresAt))return report(state.owned.length?'hedge_window_closed_inventory_retained':'entry_window_closed');
    const leg=state.planLegs[state.orders.length];need(leg,'PAIRED_CYCLE_LEG');
    await bridge.refreshPreparation(leg);
    // These calls occur in one cycle; no future timer can submit this intent.
    if(!canOperate()||!entryAllowed()||now()>=instant(state.reviewExpiresAt))return report('entry_stopped_after_reads');
    await controller.prepareNext();
    if(!entryAllowed())return report('prepared_entry_window_closed');
    await controller.dispatchPrepared();return report('placement_attempt_completed');
   }
   const leg=state.planLegs.find(l=>l.marketId===current.marketId&&l.exchangeId===current.exchangeId);need(leg,'PAIRED_CYCLE_ORDER_LEG');
   if(current.semanticMismatch)return report('semantic_uncertainty_blocks_replacement');
   if(current.phase==='prepared'){
    if(!entryAllowed()||now()>=instant(current.body.expirationDate)||now()>=instant(state.reviewExpiresAt))return report('prepared_expired_reserve_retained');
    await bridge.refreshPreparation(leg);
    if(!canOperate()||!entryAllowed()||now()>=instant(current.body.expirationDate)||now()>=instant(state.reviewExpiresAt))return report('prepared_entry_stopped');
    await controller.dispatchPrepared();return report('original_prepared_intent_dispatched');
   }
   if(!current.orderId){
    const started=current.lastActualFetchStartAt;
    if(!started)return report('missing_original_fetch_proof_blocks_recovery');
    if(now()<instant(current.body.expirationDate)||now()-instant(started)<recoveryMinimumMs)return report('exact_recovery_backoff');
    if(recoveryAllowed()!==true)return report('shared_api_recovery_cooldown');
    await controller.recoverExact();return report('identical_expired_key_recovery_attempted');
   }
   try{const evidence=await bridge.refreshReconciliation(current.orderId,leg);await controller.reconcile(evidence);return report('official_scoped_reconciliation');}
   catch(error){
    // An open order is not a missing fill or a terminal outcome. Expiry alone
    // neither releases funding nor permits another order or another key.
    if(error.code!=='PAIRED_EVIDENCE_ORDER_SCOPE_OR_PHASE')throw error;
    if(now()<instant(current.body.expirationDate))return report('known_order_pending');
    if(current.cancelPending)return report('cancel_pending_reconciliation');
    await bridge.refreshCancel(current.orderId,leg);await controller.cancelExpired();return report('owned_expired_order_cancel_attempted');
   }
  }finally{busy=false;}
 }
 return{step,status:()=>({busy,closed,lastAction,additionalPersistentProcesses:0}),close(){need(!busy,'PAIRED_CYCLE_BUSY');closed=true;}};
}
