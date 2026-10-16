//! Original owned DELETE no-HTTP evidence. Never retry or terminal authority.
use super::*;

/// Opaque, not Deserialize. Minted only inside the original owned cancel call.
/// An external dispatch timestamp is not observed by this SDK: validation binds
/// its causal relation to the internal call clock, not a fabricated exact clock.
pub struct Proof { evidence:Value }
impl Proof {
 pub fn evidence(&self)->&Value{&self.evidence}
 pub fn validate_binding(&self,spec:&RequestSpec,permit:&Permit,attempt_at:u64,error:&TransportError)->Result<()>{
  need(attempt_at>0&&self.evidence["callStartedAtMs"].as_u64().is_some_and(|start|attempt_at<=start)
   &&self.evidence["requestSha256"]==digest(&serde_json::to_vec(spec).map_err(|_|fail("CANCEL_NO_HTTP_SERIALIZE"))?)
   &&self.evidence["permitSha256"]==digest(&serde_json::to_vec(permit).map_err(|_|fail("CANCEL_NO_HTTP_SERIALIZE"))?)
   &&self.evidence["originalError"]==serde_json::to_value(error).map_err(|_|fail("CANCEL_NO_HTTP_SERIALIZE"))?,"CANCEL_NO_HTTP_BINDING_CHANGED")
 }
 pub(crate) fn mint(scope:&Scope,account_scope:&str,call:&Call,error:&TransportError,ended:u64,source:&Value)->Result<Self>{
  need(call.started>0&&call.started<=ended&&!error.may_have_executed&&error.status.is_none()&&error.order_id.is_none()&&error.api_code.is_none()&&error.retry_after_ms==0,"CANCEL_NO_HTTP_RESULT")?;
  let kind=match &call.phase{Phase::BeforeAdmitFailure=>"owned_delete_before_admit_no_http_v1",Phase::DurableNotSent{..}=>"owned_delete_durable_not_sent_v1",_=>return Err(fail("CANCEL_NO_HTTP_PHASE"))};
  Ok(Self{evidence:json!({"schemaVersion":1,"kind":kind,"orderId":call.order_id,"accountScope":account_scope,"scopeSha256":digest(&serde_json::to_vec(scope).map_err(|_|fail("CANCEL_NO_HTTP_SERIALIZE"))?),"requestSha256":digest(&serde_json::to_vec(&call.spec).map_err(|_|fail("CANCEL_NO_HTTP_SERIALIZE"))?),"permitSha256":digest(&serde_json::to_vec(&call.permit).map_err(|_|fail("CANCEL_NO_HTTP_SERIALIZE"))?),"callStartedAtMs":call.started,"callEndedAtMs":ended,"externalAttemptClockObserved":false,"originalError":error,"source":source,"retryPermitted":false,"orderTerminal":false})})
 }
}
pub(crate) enum Phase {
 Started,BeforeAdmitFailure,
 Admitted{request:String,binding:admission::ImmutableBinding,minimum_clock:u64},
 DurableNotSent{request:String,binding:admission::ImmutableBinding,minimum_clock:u64},
 NetworkAttempted,
}
pub(crate) struct Call {pub order_id:String,pub spec:RequestSpec,pub permit:Permit,pub started:u64,pub phase:Phase}
pub(crate) enum Source {
 BeforeAdmit{binding:admission::ImmutableBinding,ended:u64,source_sha256:String},
 Durable{binding:admission::ImmutableBinding,request:String,minimum_clock:u64,source_sha256:String},
}
pub(crate) struct Pending {pub proof:Proof,pub source:Source,pub sequence:u64,pub previous:String,pub order_id:String,pub operation_epoch:u64}

pub(crate) fn binding(scope:&Scope,spec:&RequestSpec,p:&Permit)->Result<admission::ImmutableBinding>{
 let id=spec.endpoint.strip_prefix("/orders/").ok_or_else(||fail("CANCEL_NO_HTTP_DELETE_ONLY"))?;
 need(spec.method=="DELETE"&&numeric(id)&&spec.body.is_none()&&spec.body_json.is_none()
  &&p.owner=="Codex"&&!p.run_id.is_empty()&&!p.intent_id.is_empty(),"CANCEL_NO_HTTP_DELETE_ONLY")?;
 Ok(admission::ImmutableBinding{intent:format!("{}:{}",p.run_id,p.intent_id),key:format!("{}:{}:cancel",p.run_id,p.intent_id),payload_hash:digest(serde_json::to_string(&json!({"scope":scope,"spec":spec})).map_err(|_|fail("CANCEL_NO_HTTP_SERIALIZE"))?.as_bytes())})
}
/// Raw original-source hashes, not an invented receipt or an archived replay
/// capability. A later controller must not reconstruct this one-shot token.
pub(crate) fn before_source(capture:&admission::FrozenUnadmittedSource,ended:u64)->Result<Value>{
 let v=capture.snapshot();let cutoff=ended.checked_add(1).ok_or_else(||fail("CANCEL_NO_HTTP_CLOCK"))?;
 Ok(json!({"kind":"held_original_controller_current_end_capture","callEndedAtMs":ended,"captureCutoffExclusiveMs":cutoff,"verifiedHead":v["verifiedHead"],"snapshotSha256":digest(&serde_json::to_vec(v).map_err(|_|fail("CANCEL_NO_HTTP_SERIALIZE"))?),"journalSha256":digest(v["journal"].as_str().ok_or_else(||fail("CANCEL_NO_HTTP_SOURCE"))?.as_bytes())}))
}
