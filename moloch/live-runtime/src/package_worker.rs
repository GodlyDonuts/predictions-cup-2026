//! Sole-worker package lifecycle over the real native bank and durable journal.
//! No client, lease, retry loop, activation renewal or settlement assumption.
//! Stage is not Deserialize/Clone; restart NEVER resumes a send from old JSON.
//! Root retains cooperative activation waiting and STOP/stream/source cadence.
use crate::{authority::ProductionAuthority,dispatch::DispatchLog,feedback::{self,Result},package_admission::Admission,package_authority::PackageSnapshotSet,package_dispatch::{DurableIntent,PackageDispatchLog},package_feedback as events,package_not_sent_evidence::VerifiedNotSent};
use baal_transport_service::{Transport,Scope,Permit,RequestSpec,Receipt,Endpoint,TransportError,multi_leg::{self,Basket,Outcome,ResponseEvidence,RequestEvidence},admission::{ImmutableBinding,BindingLookup}};
use moloch_replay::session::{Session,atomic_json};
use serde::Serialize;
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::{fs,path::Path,time::{SystemTime,UNIX_EPOCH}};
fn need(ok:bool,why:&'static str)->Result<()>{if ok{Ok(())}else{Err(why.into())}}
fn hash<T:Serialize>(v:&T)->Result<String>{Ok(format!("{:x}",Sha256::digest(serde_json::to_vec(v)?)))}
fn iso(at:u64)->Result<String>{crate::daily_windows::iso(at)}
fn clock(s:&str)->Result<u64>{crate::daily_windows::clock(s)}
fn accept(bank:&mut Session,e:&Value)->Result<()>{let next=bank.last_source_sequence("moloch-package").checked_add(1).ok_or("PACKAGE_WORKER_SEQUENCE")?;need(bank.accept("moloch-package",next,e)?,"PACKAGE_WORKER_DUPLICATE_EVENT")?;crate::execution_edge::observe_runtime_committed(bank,"moloch-package",next,e);Ok(())}
fn current(bank:&Session,id:&str)->Result<Value>{bank.engine().package_records().as_array().ok_or("PACKAGE_WORKER_NATIVE_RECORDS")?.iter().find(|p|p["packageId"]==id).cloned().ok_or_else(||"PACKAGE_WORKER_NATIVE_MISSING".into())}
fn envelope(kind:&str,p:&Value,at:u64)->Result<Value>{let at=iso(at)?;Ok(json!({"type":kind,"source":"moloch_live_dispatcher","packageId":p["packageId"],"proofHash":p["proofHash"],"bankScope":p["bankScope"],"requestHash":p["requestHash"],"receivedAt":at,"processedAt":at}))}
fn read_json(path:&Path)->Result<Value>{let m=fs::symlink_metadata(path)?;need(m.is_file()&&!m.file_type().is_symlink()&&m.len()<=8*1024*1024,"PACKAGE_WORKER_EVIDENCE_FILE")?;let bytes=fs::read(path)?;need(bytes.len()<=8*1024*1024,"PACKAGE_WORKER_EVIDENCE_FILE")?;Ok(serde_json::from_slice(&bytes)?)}
fn store(path:&Path,value:&Value)->Result<()>{if path.exists(){need(read_json(path)?==*value,"PACKAGE_WORKER_EVIDENCE_CHANGED")}else{atomic_json(path,value)}}
fn retain(directory:&Path,r:&Receipt)->Result<()>{store(&directory.join(format!("receipt-{}.json",hash(r)?)),&serde_json::to_value(r)?)}
fn error_from(v:&Value)->Result<TransportError>{Ok(TransportError{code:v["code"].as_str().ok_or("PACKAGE_WORKER_ERROR")?.into(),status:serde_json::from_value(v["status"].clone())?,may_have_executed:v["mayHaveExecuted"].as_bool().ok_or("PACKAGE_WORKER_ERROR")?,retry_after_ms:v["retryAfterMs"].as_u64().ok_or("PACKAGE_WORKER_ERROR")?,order_id:serde_json::from_value(v["orderId"].clone())?,api_code:serde_json::from_value(v["apiCode"].clone())?})}
fn response_from(v:&Value)->Result<ResponseEvidence>{Ok(ResponseEvidence{private_body_path:serde_json::from_value(v["privateBodyPath"].clone())?,private_metadata_path:serde_json::from_value(v["privateMetadataPath"].clone())?,body_sha256:v["bodySha256"].as_str().ok_or("PACKAGE_WORKER_RESPONSE")?.into(),request_sha256:v["requestSha256"].as_str().ok_or("PACKAGE_WORKER_RESPONSE")?.into(),status:u16::try_from(v["status"].as_u64().ok_or("PACKAGE_WORKER_RESPONSE")?)?,request_started_at:v["requestStartedAt"].as_str().ok_or("PACKAGE_WORKER_RESPONSE")?.into(),received_at:v["receivedAt"].as_str().ok_or("PACKAGE_WORKER_RESPONSE")?.into(),complete_body:v["completeBody"].as_bool().ok_or("PACKAGE_WORKER_RESPONSE")?})}
/// Production implementation below borrows the already serialized Transport.
/// A fixture implementation must still use real Session/journal/source proofs.
pub trait Backend{
 fn now_ms(&self)->u64;
 fn scope(&self)->&Scope;
 fn account_scope(&self)->&str;
 fn evidence_directory(&self)->&Path;
 fn quiet(&self)->Result<()>;
 fn lookup(&self,intent:&str)->Result<BindingLookup>;
 fn prepare(&mut self,bank:&Session,a:&Admission,p:Permit,f:PackageSnapshotSet)->Result<()>;
 fn clear_ready(&mut self);
 fn send(&mut self,b:Basket,p:Permit)->baal_transport_service::Result<Outcome>;
 fn take_not_sent(&mut self)->Result<VerifiedNotSent>;
 fn retained_not_sent(&mut self,binding:&ImmutableBinding,request:&str,minimum:u64)->Result<VerifiedNotSent>;
 fn take_first_call_not_sent(&mut self)->Result<baal_transport_service::multi_leg_first_call::Proof>{Err("PACKAGE_FIRST_CALL_PROOF_UNAVAILABLE".into())}
 fn retained_first_call_not_sent(&mut self,_binding:&ImmutableBinding,_request_sha256:&str)->Result<baal_transport_service::multi_leg_first_call::Proof>{Err("PACKAGE_FIRST_CALL_PROOF_UNAVAILABLE".into())}
 fn bind_owned(&mut self,actual:&str);
 fn read(&mut self,ep:Endpoint)->Result<Receipt>;
}
pub struct Live<'a>{pub transport:&'a mut Transport,pub authority:&'a ProductionAuthority,pub ordinary_dispatch:&'a DispatchLog,pub receipts:&'a Path}
impl Backend for Live<'_>{
 fn now_ms(&self)->u64{SystemTime::now().duration_since(UNIX_EPOCH).map(|d|d.as_millis() as u64).unwrap_or(0)}
 fn scope(&self)->&Scope{self.transport.scope()}
 fn account_scope(&self)->&str{self.transport.account_scope()}
 fn evidence_directory(&self)->&Path{self.receipts}
 fn quiet(&self)->Result<()>{self.ordinary_dispatch.require_resolved_attempts()?;self.transport.package_queue_quiet()?;Ok(())}
 fn lookup(&self,intent:&str)->Result<BindingLookup>{Ok(self.transport.admission_lookup_intent(intent)?)}
 fn prepare(&mut self,bank:&Session,a:&Admission,p:Permit,f:PackageSnapshotSet)->Result<()>{self.authority.prepare_package(bank,a,p,f,self.ordinary_dispatch,self.now_ms())}
 fn clear_ready(&mut self){self.authority.clear_ready();}
 fn send(&mut self,b:Basket,p:Permit)->baal_transport_service::Result<Outcome>{self.transport.place_all_or_none(b,p)}
 fn take_not_sent(&mut self)->Result<VerifiedNotSent>{Ok(VerifiedNotSent::from_sdk(self.transport.take_local_not_sent_proof()?))}
 fn retained_not_sent(&mut self,b:&ImmutableBinding,r:&str,min:u64)->Result<VerifiedNotSent>{Ok(VerifiedNotSent::from_sdk(self.transport.verify_retained_local_not_sent(b,r,min)?))}
 fn take_first_call_not_sent(&mut self)->Result<baal_transport_service::multi_leg_first_call::Proof>{Ok(self.transport.take_first_call_no_http_proof()?)}
 fn retained_first_call_not_sent(&mut self,b:&ImmutableBinding,request_sha256:&str)->Result<baal_transport_service::multi_leg_first_call::Proof>{Ok(self.transport.verify_retained_first_call_no_http(b,request_sha256)?)}
 fn bind_owned(&mut self,id:&str){self.authority.bind_owned(id.into());}
 fn read(&mut self,ep:Endpoint)->Result<Receipt>{Ok(self.transport.request(ep)?)}
}
/// Capability for this in-process, never-yet-sent package only.
pub struct Staged{admission:Admission,permit:Permit,scope:Scope,queue_scope:String}
impl Staged{pub fn package_id(&self)->&str{self.admission.package_id()}}
#[derive(Debug,PartialEq,Eq)]pub enum SendResult{Acknowledged,DefinitiveZero,LocalNotSent,NeverAttempted,Unknown}
#[derive(Debug,Default)]pub struct Recovery{pub acknowledged_packages:usize,pub fill_events:usize,pub completed_packages:usize,pub abandoned_unattempted:usize,pub blocks_new_submissions:bool}
/// Journal intent BEFORE native reservation. No network or activation wait.
pub fn stage<B:Backend>(bank:&mut Session,log:&mut PackageDispatchLog,b:&B,a:&Admission,p:Permit)->Result<Staged>{
 b.quiet()?;need(!log.unresolved()&&!bank.engine().package_blocks_submissions(),"PACKAGE_WORKER_UNRESOLVED")?;
 need(b.scope().tournament_id==a.basket().tournament_id,"PACKAGE_WORKER_SCOPE")?;
 let intent=DurableIntent::new(a.proof().clone(),a.reserve_event().clone(),a.basket().clone(),b.scope().clone(),p.clone(),b.account_scope())?;
 need(clock(a.reserve_event()["receivedAt"].as_str().ok_or("PACKAGE_WORKER_CLOCK")?)?<=b.now_ms()&&b.now_ms()<a.expires_at_ms(),"PACKAGE_WORKER_STAGE_CLOCK")?;
 log.intent(intent,&iso(b.now_ms())?)?;accept(bank,a.reserve_event())?;
 Ok(Staged{admission:a.clone(),permit:p,scope:b.scope().clone(),queue_scope:b.account_scope().into()})
}
fn request_evidence(e:&RequestEvidence,spec:&RequestSpec,b:&Basket,p:&Permit)->Result<()>{let value=read_json(&e.private_path)?;need(e.request_sha256==hash(spec)?&&e.logical_payload_sha256==hash(&value)?&&value==json!({"request":spec,"basket":b,"runId":p.run_id,"intentId":p.intent_id,"owner":p.owner}),"PACKAGE_WORKER_REQUEST_EVIDENCE")}
fn response_evidence(e:&ResponseEvidence,spec:&RequestSpec)->Result<Value>{need(e.request_sha256==hash(spec)?,"PACKAGE_WORKER_RESPONSE_REQUEST")?;let meta=read_json(&e.private_metadata_path)?;need(meta["evidence"]==serde_json::to_value(e)?&&meta["request"]==serde_json::to_value(spec)?,"PACKAGE_WORKER_RESPONSE_METADATA")?;Ok(multi_leg::evidenced_response(e)?)}
fn bind<B:Backend>(bank:&Session,id:&str,b:&mut B)->Result<()>{for l in current(bank,id)?["legs"].as_array().ok_or("PACKAGE_WORKER_LEGS")?{if let Some(id)=l["actualOrderId"].as_str(){b.bind_owned(id);}}Ok(())}
fn unknown<B:Backend>(bank:&mut Session,log:&mut PackageDispatchLog,b:&B,id:&str,error:TransportError)->Result<SendResult>{need(error.may_have_executed,"PACKAGE_WORKER_UNKNOWN_FLAG")?;log.unknown(id,&error,&iso(b.now_ms())?)?;accept(bank,&envelope("live_package_unknown",&current(bank,id)?,b.now_ms())?)?;Ok(SendResult::Unknown)}
/// Consumes Staged. Exactly ONE physical method invocation; no replacement or
/// SDK retry on any outcome. Caller cooperatively waits for original activation.
pub fn submit_once<B:Backend>(bank:&mut Session,log:&mut PackageDispatchLog,b:&mut B,staged:Staged,facts:PackageSnapshotSet)->Result<SendResult>{
 let a=&staged.admission;let id=a.package_id();let spec=multi_leg::request_spec(a.basket(),&staged.scope,&staged.permit)?;
 b.clear_ready();b.quiet()?;need(hash(b.scope())?==hash(&staged.scope)?&&b.account_scope()==staged.queue_scope&&!log.unresolved(),"PACKAGE_WORKER_BACKEND_CHANGED")?;
 let row=log.verified_record(id)?;need(row["phase"]=="intent"&&row["admission"]["spec"]==serde_json::to_value(&spec)?,"PACKAGE_WORKER_ATTEMPT_REPLAY")?;
 let p=current(bank,id)?;need(p["phase"]=="reserved","PACKAGE_WORKER_NATIVE_PHASE")?;
 let mut event=envelope("live_package_attempt",&p,b.now_ms())?;event["requestHash"]=json!(hash(&spec)?);event["idempotencyKey"]=json!(a.basket().idempotency_key);event["bookHashes"]=bank.engine().live_package_books().iter().map(|raw|(raw["exchangeId"].as_str().unwrap_or("").to_owned(),json!(hash(raw).unwrap_or_default()))).filter(|(ex,_)|a.basket().legs.iter().any(|l|l.exchange_id==*ex)).collect::<serde_json::Map<_,_>>().into();
 if let Err(error)=accept(bank,&event){store(&b.evidence_directory().join(format!("prepare-denied-{}.json",hash(&spec)?)),&json!({"stage":"native_attempt","code":error.to_string(),"at":iso(b.now_ms())?,"physicalAttempted":false}))?;abandon(bank,log,b,id)?;return Ok(SendResult::NeverAttempted)}
 if let Err(error)=b.prepare(bank,a,staged.permit.clone(),facts){b.clear_ready();store(&b.evidence_directory().join(format!("prepare-denied-{}.json",hash(&spec)?)),&json!({"stage":"authority_prepare","code":error.to_string(),"at":iso(b.now_ms())?,"physicalAttempted":false}))?;abandon(bank,log,b,id)?;return Ok(SendResult::NeverAttempted)}
 let token=match log.attempt(id,&iso(b.now_ms())?){Ok(token)=>token,Err(error)=>{b.clear_ready();return Err(error)}};
 // This is deliberately the next queue-capable operation after the attempt.
 let outcome=b.send(a.basket().clone(),staged.permit.clone());
 let result=(||->Result<SendResult>{match outcome{
  Ok(Outcome::FullFillReported{receipt,evidence,request_evidence:e,..})=>{request_evidence(&e,&spec,a.basket(),&staged.permit)?;let body=response_evidence(&evidence,&spec)?;need(receipt.data==body&&receipt.status==evidence.status&&receipt.request_started_at==evidence.request_started_at&&clock(&receipt.request_started_at)?<=clock(&evidence.received_at)?&&clock(&evidence.received_at)?<=clock(&receipt.received_at)?&&clock(&receipt.received_at)?<=b.now_ms(),"PACKAGE_WORKER_ACK_SOURCE")?;retain(b.evidence_directory(),&receipt)?;log.acknowledged(id,&receipt,&iso(b.now_ms())?)?;let event=events::acknowledgment(bank,&current(bank,id)?,a.basket(),&staged.scope,&staged.permit,&receipt,&iso(b.now_ms())?)?;accept(bank,&event)?;bind(bank,id,b)?;Ok(SendResult::Acknowledged)},
  Ok(Outcome::NoExecution{error,reason,evidence,request_evidence:e})=>{request_evidence(&e,&spec,a.basket(),&staged.permit)?;response_evidence(&evidence,&spec)?;let event=events::no_execution(bank,&current(bank,id)?,a.basket(),&staged.scope,&staged.permit,&error,&reason,&evidence,&iso(b.now_ms())?)?;log.definitive_zero(id,&evidence,&iso(b.now_ms())?)?;accept(bank,&event)?;Ok(SendResult::DefinitiveZero)},
  Ok(Outcome::Unknown{error,request_evidence:e,..})=>{request_evidence(&e,&spec,a.basket(),&staged.permit)?;unknown(bank,log,b,id,error)},
  Err(error)=>{
   // Take BEFORE summary/status/read/clear-ready operations. A false SDK flag,
   // empty queue or missing capability alone NEVER releases native funding.
   let proof=if !error.may_have_executed&&error.status.is_none()&&error.order_id.is_none()&&error.api_code.is_none(){b.take_not_sent().ok()}else{None};
   if let Some(proof)=proof{let raw=proof.evidence();need(raw["tailRecord"]==raw["completionRecord"],"PACKAGE_WORKER_INTERVENING_QUEUE_OPERATION")?;store(&b.evidence_directory().join(format!("not-sent-{}.json",hash(&spec)?)),raw)?;let attestation=proof.attestation()?;log.not_sent(token,&error,&attestation,&iso(b.now_ms())?)?;let event=events::local_not_sent(bank,&current(bank,id)?,a.basket(),&staged.scope,&staged.permit,log,&proof,&error,&iso(b.now_ms())?)?;accept(bank,&event)?;Ok(SendResult::LocalNotSent)}else if error.may_have_executed{unknown(bank,log,b,id,error)}else if error.status.is_none()&&error.order_id.is_none()&&error.api_code.is_none(){
    // The queue capability miss is read-only. Take the SDK first-call proof
    // immediately; no read/status/prepare/queue mutation may intervene.
    let proof=b.take_first_call_not_sent().map_err(|_|"PACKAGE_WORKER_NO_POSITIVE_NOT_SENT_PROOF")?;
    store(&b.evidence_directory().join(format!("first-call-not-sent-{}.json",hash(&spec)?)),proof.evidence())?;
    log.first_call_not_sent(token,&error,&proof,&iso(b.now_ms())?)?;
    let event=events::first_call_not_sent(bank,&current(bank,id)?,a.basket(),&staged.scope,&staged.permit,log,&proof,&error,&iso(b.now_ms())?)?;accept(bank,&event)?;Ok(SendResult::LocalNotSent)
   }else{Err("PACKAGE_WORKER_NO_POSITIVE_NOT_SENT_PROOF".into())}
  }
 }})();b.clear_ready();result
}
struct Frozen{row:Value,basket:Basket,scope:Scope,permit:Permit}
fn abandon<B:Backend>(bank:&mut Session,log:&mut PackageDispatchLog,b:&B,id:&str)->Result<()>{
 b.quiet()?;let record=log.verified_record(id)?;need(matches!(record["phase"].as_str(),Some("intent"|"never_attempted")),"PACKAGE_WORKER_ABANDON_AFTER_ATTEMPT")?;
 let binding:ImmutableBinding=serde_json::from_value(record["admission"]["queue_binding"].clone())?;let lookup=b.lookup(&binding.intent)?;
 need(lookup.binding.is_none()&&!lookup.has_unknown&&lookup.global_unknown_count==0,"PACKAGE_WORKER_UNATTEMPTED_QUEUE_BINDING")?;
 if record["phase"]=="intent"{log.abandon_before_attempt(id,&iso(b.now_ms())?)?;}
 let p=bank.engine().package_records();if let Some(p)=p.as_array().ok_or("PACKAGE_WORKER_NATIVE_RECORDS")?.iter().find(|p|p["packageId"]==id){
  let event=crate::package_never_attempted::release(bank,p,log,&iso(b.now_ms())?)?;accept(bank,&event)?;
 }Ok(())
}
fn frozen<B:Backend>(log:&PackageDispatchLog,id:&str,b:&B,bank:&Session)->Result<Frozen>{let row=log.verified_record(id)?;let i=&row["admission"];let scope:Scope=serde_json::from_value(i["scope"].clone())?;need(hash(&scope)?==hash(b.scope())?&&i["queue_scope"]==b.account_scope()&&i["reserve"]["bankScope"]["bankManifestHash"]==*bank.manifest_hash()&&bank.live_account_seed().is_some_and(|seed|i["reserve"]["bankScope"]["runId"]==seed["runId"]),"PACKAGE_WORKER_RESTART_SCOPE")?;Ok(Frozen{basket:serde_json::from_value(i["basket"].clone())?,scope,permit:serde_json::from_value(i["permit"].clone())?,row})}
/// Restore already durable outcomes. No admission object is recreated and no
/// physical write is possible here. Unknown/attempted stay blocking; reads and
/// actual fill reconciliation for every verified known binding still proceeds,
/// including a partially mapped package whose remaining acceptance is unknown.
pub fn recover<B:Backend>(bank:&mut Session,log:&mut PackageDispatchLog,b:&mut B)->Result<Recovery>{
 let mut report=Recovery::default();
 for record in log.records(){if record["admission"]["reserve"].get("portfolio").is_some(){continue}let id=record["admission"]["permit"]["intentId"].as_str().ok_or("PACKAGE_WORKER_ID")?.to_owned();let f=frozen(log,&id,b,bank)?;
  let native=bank.engine().package_records();let p=native.as_array().ok_or("PACKAGE_WORKER_RECORDS")?.iter().find(|p|p["packageId"]==id).cloned();
  if matches!(f.row["phase"].as_str(),Some("intent"|"never_attempted")){if p.as_ref().is_none_or(|p|matches!(p["phase"].as_str(),Some("reserved"|"attempted"))){abandon(bank,log,b,&id)?;report.abandoned_unattempted+=1;}else{need(f.row["phase"]=="never_attempted"&&p.as_ref().is_some_and(|p|p["phase"]=="no_execution"),"PACKAGE_WORKER_UNATTEMPTED_NATIVE_CONTRADICTION")?;}continue}
  let p=p.ok_or("PACKAGE_WORKER_MISSING_ATTEMPTED_NATIVE")?;
  need(p["proofHash"]==f.row["admission"]["reserve"]["proofHash"]&&p["bankScope"]==f.row["admission"]["reserve"]["bankScope"],"PACKAGE_WORKER_NATIVE_RESTART_BINDING")?;
  bind(bank,&id,b)?;
  if p["phase"]=="complete"{need(f.row["phase"]=="ack","PACKAGE_WORKER_COMPLETE_DISPATCH_CONTRADICTION")?;continue}if p["phase"]=="no_execution"{need(matches!(f.row["phase"].as_str(),Some("not_sent"|"first_call_not_sent"|"definitive_zero")),"PACKAGE_WORKER_ZERO_DISPATCH_CONTRADICTION")?;continue}
  let unresolved_acceptance=matches!(f.row["phase"].as_str(),Some("attempt"|"unknown"));
  match f.row["phase"].as_str(){
   Some("ack")=>{let receipt:Receipt=serde_json::from_value(f.row["outcome"]["receipt"].clone())?;let event=events::acknowledgment(bank,&current(bank,&id)?,&f.basket,&f.scope,&f.permit,&receipt,&iso(b.now_ms())?)?;accept(bank,&event)?;bind(bank,&id,b)?;report.acknowledged_packages+=1;},
   Some("definitive_zero")=>{let e=response_from(&f.row["outcome"]["evidence"])?;let response=multi_leg::evidenced_response(&e)?;let reason=multi_leg::zero_execution_reason(e.status,&response,f.basket.legs.len()).ok_or("PACKAGE_WORKER_ZERO_UNVERIFIED")?;let error=TransportError{code:"OFFICIAL_REQUEST_REJECTED".into(),status:Some(e.status),may_have_executed:false,retry_after_ms:0,order_id:None,api_code:response["error"]["code"].as_str().map(str::to_owned)};let event=events::no_execution(bank,&current(bank,&id)?,&f.basket,&f.scope,&f.permit,&error,&reason,&e,&iso(b.now_ms())?)?;accept(bank,&event)?;continue},
   Some("first_call_not_sent")=>{
    // Original durable outcome AND saved original SDK evidence are required.
    // Attempt/unknown plus an on-disk terminal file alone never enters here.
    let binding:ImmutableBinding=serde_json::from_value(f.row["admission"]["queue_binding"].clone())?;
    let request=f.row["admission"]["request_hash"].as_str().ok_or("PACKAGE_WORKER_REQUEST_HASH")?;
    let original=read_json(&b.evidence_directory().join(format!("first-call-not-sent-{request}.json")))?;
    need(original==f.row["outcome"]["firstCallEvidence"],"PACKAGE_WORKER_ORIGINAL_FIRST_CALL_PROOF_CHANGED")?;
    let proof=b.retained_first_call_not_sent(&binding,request)?;
    need(proof.evidence()==&original,"PACKAGE_WORKER_ORIGINAL_FIRST_CALL_PROOF_CHANGED")?;
    let error=error_from(&f.row["outcome"]["error"])?;
    let event=events::first_call_not_sent(bank,&current(bank,&id)?,&f.basket,&f.scope,&f.permit,log,&proof,&error,&iso(b.now_ms())?)?;accept(bank,&event)?;continue
   },
   Some("not_sent")=>{let binding:ImmutableBinding=serde_json::from_value(f.row["admission"]["queue_binding"].clone())?;let request=f.row["outcome"]["queueEvidence"]["request"].as_str().ok_or("PACKAGE_WORKER_NOT_SENT_REQUEST")?;let minimum=clock(f.row["attemptedAt"].as_str().ok_or("PACKAGE_WORKER_ATTEMPT_CLOCK")?)?;
    let proof=b.retained_not_sent(&binding,request,minimum)?;let original=read_json(&b.evidence_directory().join(format!("not-sent-{}.json",f.row["admission"]["request_hash"].as_str().ok_or("PACKAGE_WORKER_REQUEST_HASH")?)))?;let mut checked=proof.evidence().clone();checked.as_object_mut().ok_or("PACKAGE_WORKER_NOT_SENT_PROOF")?.remove("currentTailRecord");checked.as_object_mut().unwrap().remove("currentTailStateJson");need(checked==original,"PACKAGE_WORKER_ORIGINAL_NOT_SENT_PROOF_CHANGED")?;let error=error_from(&f.row["outcome"]["error"])?;let event=events::local_not_sent(bank,&current(bank,&id)?,&f.basket,&f.scope,&f.permit,log,&proof,&error,&iso(b.now_ms())?)?;accept(bank,&event)?;continue},
   Some("attempt"|"unknown")=>{report.blocks_new_submissions=true;},
   _=>return Err("PACKAGE_WORKER_DISPATCH_PHASE".into()),
  }
  // Unknown acceptance stays blocking, but verified owned IDs still get fresh
  // complete tapes. Never infer an ID, an overall ACK or a final release.
  for index in 0..f.basket.legs.len(){let p=current(bank,&id)?;let l=&p["legs"][index];if l["terminalReconciled"]==true{continue}let Some(actual)=l["actualOrderId"].as_str()else{need(unresolved_acceptance,"PACKAGE_WORKER_ACTUAL_ID")?;continue};let actual=actual.to_owned();let order=b.read(Endpoint::OwnedOrder{order_id:actual.clone()})?;retain(b.evidence_directory(),&order)?;let tape=b.read(Endpoint::OwnedFills{order_id:actual})?;retain(b.evidence_directory(),&tape)?;need(order.status==200&&tape.status==200,"PACKAGE_WORKER_READ_STATUS")?;
   // Prevalidate the COMPLETE tape before any credit. Persist genuine fills even
   // if the separate later terminal-state validation fails and stops this cycle.
   let fills=events::fills(bank,&current(bank,&id)?,&f.scope,index,&tape,&iso(b.now_ms())?)?;for event in fills{let before=bank.engine().fills().len();accept(bank,&event)?;report.fill_events+=bank.engine().fills().len().saturating_sub(before);}
   if let Some(event)=events::terminal(bank,&current(bank,&id)?,&f.scope,index,&order,&tape,&iso(b.now_ms())?)?{accept(bank,&event)?;}
  }
  let p=current(bank,&id)?;if !unresolved_acceptance&&p["legs"].as_array().ok_or("PACKAGE_WORKER_LEGS")?.iter().all(|l|l["terminalReconciled"]==true&&l["filled"]==l["quantity"]){let event=events::complete(bank,&p,&f.scope,&iso(b.now_ms())?)?;accept(bank,&event)?;report.completed_packages+=1;}
 }
 report.blocks_new_submissions|=log.unresolved()||bank.engine().package_blocks_submissions();Ok(report)
}
