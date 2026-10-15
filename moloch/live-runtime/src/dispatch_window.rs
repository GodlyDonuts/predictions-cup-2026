//! Read-only preflight scheduling; never dispatch authority or an expiry change.
use serde::Serialize;
use serde_json::Value;

pub const MAXIMUM_AGE_PREFETCH_LEAD_MS:u64=3000;
pub const INITIAL_EXPIRY_HEADROOM_MS:u64=5000;
pub const FINAL_EXPIRY_HEADROOM_MS:u64=3000;

#[derive(Debug,Clone,Copy,PartialEq,Eq,Serialize)]
#[serde(rename_all="snake_case")]
pub enum PreflightStage {
 Activated,
 MaximumAgeSellPrefetch,
 DeferredUntilActivation,
 NotNeverAttempted,
 ExistingDurableIntent,
 RetiredOrCanceled,
 InvalidClock,
 InsufficientExpiryHeadroom,
}
impl PreflightStage {
 pub fn may_read_snapshots(self)->bool{matches!(self,Self::Activated|Self::MaximumAgeSellPrefetch)}
 pub fn may_start_pre_activation_snapshots(self)->bool{self==Self::MaximumAgeSellPrefetch}
}

/// Call only for native ledger quotes, with current dispatch journal facts.
/// An activated quote keeps the ordinary preflight behavior. The only future
/// quote eligible for reads is an unattempted maximum-age inventoryAON SELL.
pub fn preflight_stage(q:&Value,never_attempted:bool,has_durable_intent:bool,now_ms:u64)->PreflightStage{
 if !never_attempted{return PreflightStage::NotNeverAttempted}
 if has_durable_intent{return PreflightStage::ExistingDurableIntent}
 if q["retired"]!=false||q.get("cancelEffectiveAt").is_some_and(|v|!v.is_null()){return PreflightStage::RetiredOrCanceled}
 let (Some(created),Some(activation),Some(expiry))=(q["createdAt"].as_u64(),q["activationAt"].as_u64(),q["expiresAt"].as_u64())else{return PreflightStage::InvalidClock};
 if created>now_ms||activation<created||expiry<=activation{return PreflightStage::InvalidClock}
 if expiry.saturating_sub(now_ms)<=INITIAL_EXPIRY_HEADROOM_MS{return PreflightStage::InsufficientExpiryHeadroom}
 if activation<=now_ms{return PreflightStage::Activated}
 if q["action"]=="sell"&&q["executionStyle"]=="inventoryAON"&&q["inventoryExitReason"]=="maximumAge"&&q["filled"].as_u64()==Some(0)&&activation-now_ms<=MAXIMUM_AGE_PREFETCH_LEAD_MS {
  PreflightStage::MaximumAgeSellPrefetch
 }else{PreflightStage::DeferredUntilActivation}
}

/// Necessary final timing/intent gate on the REREAD current native quote after
/// fresh-book acceptance. ProductionAuthority still checks every live condition
/// again. `true` does not authorize a write, create an intent or bypass funding.
pub fn final_native_window_ready(q:&Value,never_attempted:bool,has_durable_intent:bool,now_ms:u64)->bool{
 if !never_attempted||has_durable_intent||q["retired"]!=false||q.get("cancelEffectiveAt").is_some_and(|v|!v.is_null()){return false}
 let (Some(created),Some(activation),Some(expiry))=(q["createdAt"].as_u64(),q["activationAt"].as_u64(),q["expiresAt"].as_u64())else{return false};
 created<=now_ms&&created<=activation&&activation<=now_ms&&expiry>activation&&expiry.saturating_sub(now_ms)>FINAL_EXPIRY_HEADROOM_MS
}
