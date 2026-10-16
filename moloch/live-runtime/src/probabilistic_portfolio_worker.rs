//! Dedicated probabilistic lifecycle over the sole real bank and original journal.
//! No client, lease, retry loop, activation renewal or settlement assumption.
//! Stage is not Deserialize/Clone; restart NEVER resumes a send from old JSON.
//! Root retains cooperative activation waiting and STOP/stream/source cadence.
use crate::{authority::ProductionAuthority,dispatch::DispatchLog,feedback::{self,Result},probabilistic_portfolio::{Plan,Reservation,Policy},package_authority::PackageSnapshotSet,package_dispatch::{DurableIntent,PackageDispatchLog},package_portfolio_feedback as events,package_not_sent_evidence::VerifiedNotSent};
use baal_transport_service::{Transport,Scope,Permit,RequestSpec,Receipt,Endpoint,TransportError,multi_leg::{self,ResponseEvidence,RequestEvidence},multi_leg_portfolio::{self as signed,Basket,Outcome,Action,Leg},admission::{ImmutableBinding,BindingLookup}};
use moloch_replay::session::{Session,atomic_json};
use serde::Serialize;
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::{fs,path::Path,time::{SystemTime,UNIX_EPOCH}};
fn need(ok:bool,why:&'static str)->Result<()>{if ok{Ok(())}else{Err(why.into())}}
fn hash<T:Serialize>(v:&T)->Result<String>{Ok(format!("{:x}",Sha256::digest(serde_json::to_vec(v)?)))}
fn iso(at:u64)->Result<String>{crate::daily_windows::iso(at)}
fn clock(s:&str)->Result<u64>{crate::daily_windows::clock(s)}
fn accept(bank:&mut Session,e:&Value)->Result<()>{let next=bank.last_source_sequence("moloch-package").checked_add(1).ok_or("PROBABILISTIC_WORKER_SEQUENCE")?;need(bank.accept("moloch-package",next,e)?,"PROBABILISTIC_WORKER_DUPLICATE_EVENT")?;crate::execution_edge::observe_runtime_committed(bank,"moloch-package",next,e);Ok(())}
fn current(bank:&Session,id:&str)->Result<Value>{let p=bank.engine().package_records().as_array().ok_or("PROBABILISTIC_WORKER_NATIVE_RECORDS")?.iter().find(|p|p["packageId"]==id).cloned().ok_or("PROBABILISTIC_WORKER_NATIVE_MISSING")?;need(p["probabilistic"]["kind"]=="reviewed_probabilistic_portfolio"&&p["economics"]["kind"]=="probabilistic","PROBABILISTIC_WORKER_NATIVE_CLASS")?;Ok(p)}
fn envelope(kind:&str,p:&Value,at:u64)->Result<Value>{let at=iso(at)?;Ok(json!({"type":kind,"source":"moloch_live_dispatcher","packageId":p["packageId"],"proofHash":p["proofHash"],"bankScope":p["bankScope"],"requestHash":p["requestHash"],"receivedAt":at,"processedAt":at}))}
fn read_json(path:&Path)->Result<Value>{let m=fs::symlink_metadata(path)?;need(m.is_file()&&!m.file_type().is_symlink()&&m.len()<=8*1024*1024,"PROBABILISTIC_WORKER_EVIDENCE_FILE")?;let bytes=fs::read(path)?;need(bytes.len()<=8*1024*1024,"PROBABILISTIC_WORKER_EVIDENCE_FILE")?;Ok(serde_json::from_slice(&bytes)?)}
fn store(path:&Path,value:&Value)->Result<()>{if path.exists(){need(read_json(path)?==*value,"PROBABILISTIC_WORKER_EVIDENCE_CHANGED")}else{atomic_json(path,value)}}
fn retain(directory:&Path,r:&Receipt)->Result<()>{store(&directory.join(format!("receipt-{}.json",hash(r)?)),&serde_json::to_value(r)?)}
fn error_from(v:&Value)->Result<TransportError>{Ok(TransportError{code:v["code"].as_str().ok_or("PROBABILISTIC_WORKER_ERROR")?.into(),status:serde_json::from_value(v["status"].clone())?,may_have_executed:v["mayHaveExecuted"].as_bool().ok_or("PROBABILISTIC_WORKER_ERROR")?,retry_after_ms:v["retryAfterMs"].as_u64().ok_or("PROBABILISTIC_WORKER_ERROR")?,order_id:serde_json::from_value(v["orderId"].clone())?,api_code:serde_json::from_value(v["apiCode"].clone())?})}
fn response_from(v:&Value)->Result<ResponseEvidence>{Ok(ResponseEvidence{private_body_path:serde_json::from_value(v["privateBodyPath"].clone())?,private_metadata_path:serde_json::from_value(v["privateMetadataPath"].clone())?,body_sha256:v["bodySha256"].as_str().ok_or("PROBABILISTIC_WORKER_RESPONSE")?.into(),request_sha256:v["requestSha256"].as_str().ok_or("PROBABILISTIC_WORKER_RESPONSE")?.into(),status:u16::try_from(v["status"].as_u64().ok_or("PROBABILISTIC_WORKER_RESPONSE")?)?,request_started_at:v["requestStartedAt"].as_str().ok_or("PROBABILISTIC_WORKER_RESPONSE")?.into(),received_at:v["receivedAt"].as_str().ok_or("PROBABILISTIC_WORKER_RESPONSE")?.into(),complete_body:v["completeBody"].as_bool().ok_or("PROBABILISTIC_WORKER_RESPONSE")?})}
/// Production implementation below borrows the already serialized Transport.
/// A fixture implementation must still use real Session/journal/source proofs.
/// Extends the existing backend, never creates an account client or limiter.
pub trait Backend:crate::package_worker::Backend{
 fn prepare_probabilistic(&mut self,bank:&Session,plan:&Plan,reservation:&Reservation,basket:&Basket,permit:Permit,facts:PackageSnapshotSet)->Result<()>;
 fn ordinary_dispatch(&self)->&DispatchLog;
 fn send_portfolio(&mut self,basket:Basket,permit:Permit)->baal_transport_service::Result<Outcome>;
}
pub use crate::package_worker::Live;
impl Backend for Live<'_>{
 fn prepare_probabilistic(&mut self,bank:&Session,plan:&Plan,r:&Reservation,basket:&Basket,p:Permit,f:PackageSnapshotSet)->Result<()>{self.authority.prepare_probabilistic(bank,plan,r,basket,p,f,self.ordinary_dispatch,crate::package_worker::Backend::now_ms(self))}
 fn ordinary_dispatch(&self)->&DispatchLog{self.ordinary_dispatch}
 fn send_portfolio(&mut self,b:Basket,p:Permit)->baal_transport_service::Result<Outcome>{self.transport.place_portfolio_all_or_none(b,p)}
}
/// One in-process reviewed probability-bound plan, never reconstructed to send.
pub struct Staged{admission:Plan,reservation:Reservation,basket:Basket,permit:Permit,scope:Scope,queue_scope:String}
impl Staged{pub fn package_id(&self)->&str{self.reservation.event()["packageId"].as_str().expect("factory package ID")}pub fn plan(&self)->&Plan{&self.admission}pub fn reservation(&self)->&Reservation{&self.reservation}pub fn basket(&self)->&Basket{&self.basket}pub fn permit(&self)->&Permit{&self.permit}}
#[derive(Debug,PartialEq,Eq)]pub enum SendResult{Acknowledged,DefinitiveZero,LocalNotSent,NeverAttempted,Unknown}
#[derive(Debug,Default)]pub struct Recovery{pub acknowledged_packages:usize,pub fill_events:usize,pub completed_packages:usize,pub abandoned_unattempted:usize,pub blocks_new_submissions:bool}
/// Original intent is fsynced before native funding. Qualification is minted
/// only from the opaque approved model/semantics/policy and ACTUAL current bank.
pub fn stage<B:Backend>(bank:&mut Session,log:&mut PackageDispatchLog,b:&B,plan:Plan,surface:&crate::value_surface::BoundSurface,semantics:&crate::semantic_states::Compiled,policy:&Policy,activation:&crate::authority::Activation)->Result<Staged>{
 b.quiet()?;need(!log.unresolved()&&!bank.engine().package_blocks_submissions(),"PROBABILISTIC_WORKER_UNRESOLVED")?;
 need(bank.live_account_seed().is_some_and(|s|s["runId"]==activation.run_id)&&activation.tournament_id==b.scope().tournament_id&&activation.tournament_slug==b.scope().tournament_slug,"PROBABILISTIC_WORKER_ACTIVATION_SCOPE")?;
 let reservation=plan.qualified_reservation(bank,b.ordinary_dispatch(),b.scope(),surface,semantics,policy,b.now_ms())?;
 let reserve=reservation.event();let id=reserve["packageId"].as_str().ok_or("PROBABILISTIC_WORKER_ID")?;
 let p=Permit{run_id:activation.run_id.clone(),intent_id:id.into(),owner:"Codex".into(),expires_at_ms:plan.expires_at_ms(),maximum_quantity:activation.maximum_quantity,maximum_cost:moloch_replay::money(plan.metrics().gross_buy_funding)};
 let basket=plan.basket(&format!("{}:{id}",p.run_id),&b.scope().tournament_id)?;signed::request_spec(&basket,b.scope(),&p)?;
 let intent=DurableIntent::new_probabilistic(reserve["qualification"].clone(),reserve.clone(),basket.clone(),b.scope().clone(),p.clone(),b.account_scope())?;
 need(clock(reserve["receivedAt"].as_str().ok_or("PROBABILISTIC_WORKER_CLOCK")?)?<=b.now_ms()&&b.now_ms()<plan.expires_at_ms(),"PROBABILISTIC_WORKER_STAGE_CLOCK")?;
 log.intent(intent,&iso(b.now_ms())?)?;accept(bank,reserve)?;
 Ok(Staged{admission:plan,reservation,basket,permit:p,scope:b.scope().clone(),queue_scope:b.account_scope().into()})
}
fn request_evidence(e:&RequestEvidence,spec:&RequestSpec,b:&Basket,p:&Permit)->Result<()>{let value=read_json(&e.private_path)?;need(e.request_sha256==hash(spec)?&&e.logical_payload_sha256==hash(&value)?&&value==json!({"request":spec,"basket":b,"runId":p.run_id,"intentId":p.intent_id,"owner":p.owner}),"PROBABILISTIC_WORKER_REQUEST_EVIDENCE")}
fn response_evidence(e:&ResponseEvidence,spec:&RequestSpec)->Result<Value>{need(e.request_sha256==hash(spec)?,"PROBABILISTIC_WORKER_RESPONSE_REQUEST")?;let meta=read_json(&e.private_metadata_path)?;need(meta["evidence"]==serde_json::to_value(e)?&&meta["request"]==serde_json::to_value(spec)?,"PROBABILISTIC_WORKER_RESPONSE_METADATA")?;Ok(multi_leg::evidenced_response(e)?)}
fn bind<B:Backend>(bank:&Session,id:&str,b:&mut B)->Result<()>{for l in current(bank,id)?["legs"].as_array().ok_or("PROBABILISTIC_WORKER_LEGS")?{if let Some(id)=l["actualOrderId"].as_str(){b.bind_owned(id);}}Ok(())}
fn unknown<B:Backend>(bank:&mut Session,log:&mut PackageDispatchLog,b:&B,id:&str,error:TransportError)->Result<SendResult>{need(error.may_have_executed,"PROBABILISTIC_WORKER_UNKNOWN_FLAG")?;log.unknown(id,&error,&iso(b.now_ms())?)?;accept(bank,&envelope("probabilistic_portfolio_unknown_v1",&current(bank,id)?,b.now_ms())?)?;Ok(SendResult::Unknown)}
/// Consumes Staged. Exactly ONE physical method invocation; no replacement or
/// SDK retry on any outcome. Caller cooperatively waits for original activation.
pub fn submit_once<B:Backend>(bank:&mut Session,log:&mut PackageDispatchLog,b:&mut B,staged:Staged,facts:PackageSnapshotSet)->Result<SendResult>{
 let a=&staged.admission;let id=staged.package_id();let spec=signed::request_spec(&staged.basket,&staged.scope,&staged.permit)?;
 b.clear_ready();b.quiet()?;need(hash(b.scope())?==hash(&staged.scope)?&&b.account_scope()==staged.queue_scope&&!log.unresolved(),"PROBABILISTIC_WORKER_BACKEND_CHANGED")?;
 let row=log.verified_record(id)?;need(row["phase"]=="intent"&&row["admission"]["spec"]==serde_json::to_value(&spec)?,"PROBABILISTIC_WORKER_ATTEMPT_REPLAY")?;
 let p=current(bank,id)?;need(p["phase"]=="reserved","PROBABILISTIC_WORKER_NATIVE_PHASE")?;
 let mut event=envelope("probabilistic_portfolio_attempt_v1",&p,b.now_ms())?;event["requestHash"]=json!(hash(&spec)?);event["idempotencyKey"]=json!(&staged.basket.idempotency_key);event["bookHashes"]=bank.engine().live_package_books().iter().map(|raw|(raw["exchangeId"].as_str().unwrap_or("").to_owned(),json!(hash(raw).unwrap_or_default()))).filter(|(ex,_)|staged.basket.legs.iter().any(|l|l.exchange_id==*ex)).collect::<serde_json::Map<_,_>>().into();
 if let Err(error)=accept(bank,&event){store(&b.evidence_directory().join(format!("prepare-denied-{}.json",hash(&spec)?)),&json!({"stage":"native_attempt","code":error.to_string(),"at":iso(b.now_ms())?,"physicalAttempted":false}))?;abandon(bank,log,b,id)?;return Ok(SendResult::NeverAttempted)}
 if let Err(error)=b.prepare_probabilistic(bank,a,&staged.reservation,&staged.basket,staged.permit.clone(),facts){b.clear_ready();store(&b.evidence_directory().join(format!("prepare-denied-{}.json",hash(&spec)?)),&json!({"stage":"authority_prepare","code":error.to_string(),"at":iso(b.now_ms())?,"physicalAttempted":false}))?;abandon(bank,log,b,id)?;return Ok(SendResult::NeverAttempted)}
 let token=match log.attempt(id,&iso(b.now_ms())?){Ok(token)=>token,Err(error)=>{b.clear_ready();return Err(error)}};
 // This is deliberately the next queue-capable operation after the attempt.
 let outcome=b.send_portfolio(staged.basket.clone(),staged.permit.clone());
 let result=(||->Result<SendResult>{match outcome{
  Ok(Outcome::FullFillReported{receipt,evidence,request_evidence:e,..})=>{request_evidence(&e,&spec,&staged.basket,&staged.permit)?;let body=response_evidence(&evidence,&spec)?;need(receipt.data==body&&receipt.status==evidence.status&&receipt.request_started_at==evidence.request_started_at&&clock(&receipt.request_started_at)?<=clock(&evidence.received_at)?&&clock(&evidence.received_at)?<=clock(&receipt.received_at)?&&clock(&receipt.received_at)?<=b.now_ms(),"PROBABILISTIC_WORKER_ACK_SOURCE")?;retain(b.evidence_directory(),&receipt)?;log.acknowledged(id,&receipt,&iso(b.now_ms())?)?;let event=events::acknowledgment(bank,&current(bank,id)?,&staged.basket,&staged.scope,&staged.permit,&receipt,&iso(b.now_ms())?)?;accept(bank,&event)?;bind(bank,id,b)?;Ok(SendResult::Acknowledged)},
  Ok(Outcome::NoExecution{error,reason,evidence,request_evidence:e})=>{request_evidence(&e,&spec,&staged.basket,&staged.permit)?;response_evidence(&evidence,&spec)?;let event=events::no_execution(bank,&current(bank,id)?,&staged.basket,&staged.scope,&staged.permit,&error,&reason,&evidence,&iso(b.now_ms())?)?;log.definitive_zero(id,&evidence,&iso(b.now_ms())?)?;accept(bank,&event)?;Ok(SendResult::DefinitiveZero)},
  Ok(Outcome::Unknown{error,request_evidence:e,..})=>{request_evidence(&e,&spec,&staged.basket,&staged.permit)?;unknown(bank,log,b,id,error)},
  Err(error)=>{
   // Take BEFORE summary/status/read/clear-ready operations. A false SDK flag,
   // empty queue or missing capability alone NEVER releases native funding.
   let proof=if !error.may_have_executed&&error.status.is_none()&&error.order_id.is_none()&&error.api_code.is_none(){b.take_not_sent().ok()}else{None};
   if let Some(proof)=proof{let raw=proof.evidence();need(raw["tailRecord"]==raw["completionRecord"],"PROBABILISTIC_WORKER_INTERVENING_QUEUE_OPERATION")?;store(&b.evidence_directory().join(format!("not-sent-{}.json",hash(&spec)?)),raw)?;let attestation=proof.attestation()?;log.not_sent(token,&error,&attestation,&iso(b.now_ms())?)?;let event=events::local_not_sent(bank,&current(bank,id)?,&staged.basket,&staged.scope,&staged.permit,log,&proof,&error,&iso(b.now_ms())?)?;accept(bank,&event)?;Ok(SendResult::LocalNotSent)}else if error.may_have_executed{unknown(bank,log,b,id,error)}else if error.status.is_none()&&error.order_id.is_none()&&error.api_code.is_none(){
    // The queue capability miss is read-only. Take the SDK first-call proof
    // immediately; no read/status/prepare/queue mutation may intervene.
    let proof=b.take_first_call_not_sent().map_err(|_|"PROBABILISTIC_WORKER_NO_POSITIVE_NOT_SENT_PROOF")?;
    store(&b.evidence_directory().join(format!("first-call-not-sent-{}.json",hash(&spec)?)),proof.evidence())?;
    log.first_call_not_sent(token,&error,&proof,&iso(b.now_ms())?)?;
    let event=events::first_call_not_sent(bank,&current(bank,id)?,&staged.basket,&staged.scope,&staged.permit,log,&proof,&error,&iso(b.now_ms())?)?;accept(bank,&event)?;Ok(SendResult::LocalNotSent)
   }else{Err("PROBABILISTIC_WORKER_NO_POSITIVE_NOT_SENT_PROOF".into())}
  }
 }})();b.clear_ready();result
}
struct Frozen{row:Value,basket:Basket,scope:Scope,permit:Permit}
fn abandon<B:Backend>(bank:&mut Session,log:&mut PackageDispatchLog,b:&B,id:&str)->Result<()>{
 b.quiet()?;let record=log.verified_record(id)?;need(matches!(record["phase"].as_str(),Some("intent"|"never_attempted")),"PROBABILISTIC_WORKER_ABANDON_AFTER_ATTEMPT")?;
 let binding:ImmutableBinding=serde_json::from_value(record["admission"]["queue_binding"].clone())?;let lookup=b.lookup(&binding.intent)?;
 need(lookup.binding.is_none()&&!lookup.has_unknown&&lookup.global_unknown_count==0,"PROBABILISTIC_WORKER_UNATTEMPTED_QUEUE_BINDING")?;
 if record["phase"]=="intent"{log.abandon_before_attempt(id,&iso(b.now_ms())?)?;}
 let p=bank.engine().package_records();if let Some(p)=p.as_array().ok_or("PROBABILISTIC_WORKER_NATIVE_RECORDS")?.iter().find(|p|p["packageId"]==id){
  let event=crate::package_never_attempted::release(bank,p,log,&iso(b.now_ms())?)?;accept(bank,&event)?;
 }Ok(())
}
fn frozen<B:Backend>(log:&PackageDispatchLog,id:&str,b:&B,bank:&Session)->Result<Frozen>{let row=log.verified_record(id)?;let i=&row["admission"];let scope:Scope=serde_json::from_value(i["scope"].clone())?;need(hash(&scope)?==hash(b.scope())?&&i["queue_scope"]==b.account_scope()&&i["reserve"]["bankScope"]["bankManifestHash"]==*bank.manifest_hash()&&bank.live_account_seed().is_some_and(|seed|i["reserve"]["bankScope"]["runId"]==seed["runId"]),"PROBABILISTIC_WORKER_RESTART_SCOPE")?;Ok(Frozen{basket:serde_json::from_value(i["basket"].clone())?,scope,permit:serde_json::from_value(i["permit"].clone())?,row})}
/// Restore already durable outcomes. No admission object is recreated and no
/// physical write is possible here. Unknown/attempted stay blocking; reads and
/// actual fill reconciliation for every verified known binding still proceeds,
/// including a partially mapped package whose remaining acceptance is unknown.
pub fn recover<B:Backend>(bank:&mut Session,log:&mut PackageDispatchLog,b:&mut B)->Result<Recovery>{
 let mut report=Recovery::default();
 for record in log.records(){if record["admission"]["reserve"]["type"]!="probabilistic_portfolio_reserve_v1"{continue}let id=record["admission"]["permit"]["intentId"].as_str().ok_or("PROBABILISTIC_WORKER_ID")?.to_owned();let f=frozen(log,&id,b,bank)?;
  let native=bank.engine().package_records();let p=native.as_array().ok_or("PROBABILISTIC_WORKER_RECORDS")?.iter().find(|p|p["packageId"]==id).cloned();
  if matches!(f.row["phase"].as_str(),Some("intent"|"never_attempted")){if p.as_ref().is_none_or(|p|matches!(p["phase"].as_str(),Some("reserved"|"attempted"))){abandon(bank,log,b,&id)?;report.abandoned_unattempted+=1;}else{need(f.row["phase"]=="never_attempted"&&p.as_ref().is_some_and(|p|p["phase"]=="no_execution"),"PROBABILISTIC_WORKER_UNATTEMPTED_NATIVE_CONTRADICTION")?;}continue}
  let p=p.ok_or("PROBABILISTIC_WORKER_MISSING_ATTEMPTED_NATIVE")?;
  need(p["proofHash"]==f.row["admission"]["reserve"]["proofHash"]&&p["bankScope"]==f.row["admission"]["reserve"]["bankScope"],"PROBABILISTIC_WORKER_NATIVE_RESTART_BINDING")?;
  bind(bank,&id,b)?;
  if p["phase"]=="complete"{need(f.row["phase"]=="ack","PROBABILISTIC_WORKER_COMPLETE_DISPATCH_CONTRADICTION")?;continue}if p["phase"]=="no_execution"{need(matches!(f.row["phase"].as_str(),Some("not_sent"|"first_call_not_sent"|"definitive_zero")),"PROBABILISTIC_WORKER_ZERO_DISPATCH_CONTRADICTION")?;continue}
  let unresolved_acceptance=matches!(f.row["phase"].as_str(),Some("attempt"|"unknown"));
  match f.row["phase"].as_str(){
   Some("ack")=>{let receipt:Receipt=serde_json::from_value(f.row["outcome"]["receipt"].clone())?;let event=events::acknowledgment(bank,&current(bank,&id)?,&f.basket,&f.scope,&f.permit,&receipt,&iso(b.now_ms())?)?;accept(bank,&event)?;bind(bank,&id,b)?;report.acknowledged_packages+=1;},
   Some("definitive_zero")=>{let e=response_from(&f.row["outcome"]["evidence"])?;let response=multi_leg::evidenced_response(&e)?;let reason=multi_leg::zero_execution_reason(e.status,&response,f.basket.legs.len()).ok_or("PROBABILISTIC_WORKER_ZERO_UNVERIFIED")?;let error=TransportError{code:"OFFICIAL_REQUEST_REJECTED".into(),status:Some(e.status),may_have_executed:false,retry_after_ms:0,order_id:None,api_code:response["error"]["code"].as_str().map(str::to_owned)};let event=events::no_execution(bank,&current(bank,&id)?,&f.basket,&f.scope,&f.permit,&error,&reason,&e,&iso(b.now_ms())?)?;accept(bank,&event)?;continue},
   Some("first_call_not_sent")=>{
    // Original durable outcome AND saved original SDK evidence are required.
    // Attempt/unknown plus an on-disk terminal file alone never enters here.
    let binding:ImmutableBinding=serde_json::from_value(f.row["admission"]["queue_binding"].clone())?;
    let request=f.row["admission"]["request_hash"].as_str().ok_or("PROBABILISTIC_WORKER_REQUEST_HASH")?;
    let original=read_json(&b.evidence_directory().join(format!("first-call-not-sent-{request}.json")))?;
    need(original==f.row["outcome"]["firstCallEvidence"],"PROBABILISTIC_WORKER_ORIGINAL_FIRST_CALL_PROOF_CHANGED")?;
    let proof=b.retained_first_call_not_sent(&binding,request)?;
    need(proof.evidence()==&original,"PROBABILISTIC_WORKER_ORIGINAL_FIRST_CALL_PROOF_CHANGED")?;
    let error=error_from(&f.row["outcome"]["error"])?;
    let event=events::first_call_not_sent(bank,&current(bank,&id)?,&f.basket,&f.scope,&f.permit,log,&proof,&error,&iso(b.now_ms())?)?;accept(bank,&event)?;continue
   },
   Some("not_sent")=>{let binding:ImmutableBinding=serde_json::from_value(f.row["admission"]["queue_binding"].clone())?;let request=f.row["outcome"]["queueEvidence"]["request"].as_str().ok_or("PROBABILISTIC_WORKER_NOT_SENT_REQUEST")?;let minimum=clock(f.row["attemptedAt"].as_str().ok_or("PROBABILISTIC_WORKER_ATTEMPT_CLOCK")?)?;
    let proof=b.retained_not_sent(&binding,request,minimum)?;let original=read_json(&b.evidence_directory().join(format!("not-sent-{}.json",f.row["admission"]["request_hash"].as_str().ok_or("PROBABILISTIC_WORKER_REQUEST_HASH")?)))?;let mut checked=proof.evidence().clone();checked.as_object_mut().ok_or("PROBABILISTIC_WORKER_NOT_SENT_PROOF")?.remove("currentTailRecord");checked.as_object_mut().unwrap().remove("currentTailStateJson");need(checked==original,"PROBABILISTIC_WORKER_ORIGINAL_NOT_SENT_PROOF_CHANGED")?;let error=error_from(&f.row["outcome"]["error"])?;let event=events::local_not_sent(bank,&current(bank,&id)?,&f.basket,&f.scope,&f.permit,log,&proof,&error,&iso(b.now_ms())?)?;accept(bank,&event)?;continue},
   Some("attempt"|"unknown")=>{report.blocks_new_submissions=true;},
   _=>return Err("PROBABILISTIC_WORKER_DISPATCH_PHASE".into()),
  }
  // Unknown acceptance stays blocking, but verified owned IDs still get fresh
  // complete tapes. Never infer an ID, an overall ACK or a final release.
  for index in 0..f.basket.legs.len(){let p=current(bank,&id)?;let l=&p["legs"][index];if l["terminalReconciled"]==true{continue}let Some(actual)=l["actualOrderId"].as_str()else{need(unresolved_acceptance,"PROBABILISTIC_WORKER_ACTUAL_ID")?;continue};let actual=actual.to_owned();let order=b.read(Endpoint::OwnedOrder{order_id:actual.clone()})?;retain(b.evidence_directory(),&order)?;let tape=b.read(Endpoint::OwnedFills{order_id:actual})?;retain(b.evidence_directory(),&tape)?;need(order.status==200&&tape.status==200,"PROBABILISTIC_WORKER_READ_STATUS")?;
   // Prevalidate the COMPLETE tape before any credit. Persist genuine fills even
   // if the separate later terminal-state validation fails and stops this cycle.
   let fills=events::fills(bank,&current(bank,&id)?,&f.scope,index,&tape,&iso(b.now_ms())?)?;for event in fills{let before=bank.engine().fills().len();accept(bank,&event)?;report.fill_events+=bank.engine().fills().len().saturating_sub(before);}
   if let Some(event)=events::terminal(bank,&current(bank,&id)?,&f.scope,index,&order,&tape,&iso(b.now_ms())?)?{accept(bank,&event)?;}
  }
  let p=current(bank,&id)?;if !unresolved_acceptance&&p["legs"].as_array().ok_or("PROBABILISTIC_WORKER_LEGS")?.iter().all(|l|l["terminalReconciled"]==true&&l["filled"]==l["quantity"]){let event=events::complete(bank,&p,&f.scope,&iso(b.now_ms())?)?;accept(bank,&event)?;report.completed_packages+=1;}
 }
 report.blocks_new_submissions|=log.unresolved()||bank.engine().package_blocks_submissions();Ok(report)
}
