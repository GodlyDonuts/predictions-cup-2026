//! Physical network attempts are recorded independently of strategy projections.
use crate::feedback::Result;
use baal_transport_service::{OrdinaryPreAdmissionProof,RequestSpec,Permit,TransportError};
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::{fs::{self,File,OpenOptions},io::{BufRead,BufReader,Write},path::Path,collections::{BTreeMap,BTreeSet}};
fn hash(v:&Value)->String{format!("{:x}",Sha256::digest(serde_json::to_vec(v).unwrap()))}
fn clock(v:&Value)->Result<u64>{Ok(chrono::DateTime::parse_from_rfc3339(v.as_str().ok_or("DISPATCH_ABORT_CLOCK")?)?.timestamp_millis().try_into().map_err(|_|"DISPATCH_ABORT_CLOCK")?)}
fn stored_error(v:&Value)->Result<TransportError>{let e=TransportError{code:v["code"].as_str().ok_or("DISPATCH_ABORT_ERROR")?.into(),status:serde_json::from_value(v["status"].clone())?,may_have_executed:v["mayHaveExecuted"].as_bool().ok_or("DISPATCH_ABORT_ERROR")?,retry_after_ms:v["retryAfterMs"].as_u64().ok_or("DISPATCH_ABORT_ERROR")?,order_id:serde_json::from_value(v["orderId"].clone())?,api_code:serde_json::from_value(v["apiCode"].clone())?};if serde_json::to_value(&e)?!=*v{return Err("DISPATCH_ABORT_ERROR".into())}Ok(e)}
pub struct DispatchLog {file:File,sequence:u64,previous:String,pub intents:BTreeMap<u64,Value>,pub acknowledgments:BTreeMap<u64,Value>,pub recovered:BTreeMap<u64,Value>,pub attempted:BTreeSet<u64>,pub closed:BTreeSet<u64>,pub cancel_acknowledged:BTreeSet<u64>,attempts:BTreeMap<u64,Value>,errors:BTreeMap<u64,Value>,aborted_records:BTreeMap<u64,Value>,proved_no_network:BTreeSet<u64>}
impl DispatchLog {
 pub fn descriptor_identity(&self)->Result<Value>{
  #[cfg(unix)]{use std::os::unix::fs::MetadataExt;let m=self.file.metadata()?;Ok(json!({"schemaVersion":1,"device":m.dev(),"inode":m.ino(),"bytes":m.len(),"sequence":self.sequence,"previous":self.previous}))}
  #[cfg(not(unix))]{Err("DISPATCH_DESCRIPTOR_IDENTITY_UNSUPPORTED".into())}
 }
 pub fn open(path:&Path)->Result<Self>{
  let mut options=OpenOptions::new();options.read(true).append(true).create(true);
  #[cfg(unix)]{use std::os::unix::fs::OpenOptionsExt;options.mode(0o600);}
  let file=options.open(path)?;file.sync_all()?;File::open(path.parent().ok_or("DISPATCH_PARENT")?)?.sync_all()?;
  let bytes=fs::read(path)?;if !bytes.is_empty()&&bytes.last()!=Some(&b'\n'){return Err("DISPATCH_TORN_TAIL".into())}
  let mut log=Self{file,sequence:0,previous:"0".repeat(64),intents:Default::default(),acknowledgments:Default::default(),recovered:Default::default(),attempted:Default::default(),closed:Default::default(),cancel_acknowledged:Default::default(),attempts:Default::default(),errors:Default::default(),aborted_records:Default::default(),proved_no_network:Default::default()};
  for line in BufReader::new(File::open(path)?).lines(){let mut row:Value=serde_json::from_str(&line?)?;let claimed=row.as_object_mut().ok_or("DISPATCH_ROW")?.remove("hash").ok_or("DISPATCH_HASH")?;if row["sequence"]!=log.sequence+1||row["previous"]!=log.previous||claimed!=hash(&row){return Err("DISPATCH_CHAIN".into())}log.apply(&row["event"])?;log.sequence+=1;log.previous=claimed.as_str().ok_or("DISPATCH_HASH")?.into();}
  Ok(log)
 }
 fn apply(&mut self,e:&Value)->Result<()> {
  let id=e["qid"].as_u64().ok_or("DISPATCH_QID")?;
  match e["kind"].as_str(){
   Some("intent")=>{if self.intents.contains_key(&id){return Err("DISPATCH_DUPLICATE_INTENT".into())}self.intents.insert(id,e.clone());},
   Some("attempt")=>{if !self.intents.contains_key(&id)||!self.attempted.insert(id){return Err("DISPATCH_REPEAT_ATTEMPT".into())}self.attempts.insert(id,e.clone());},
   Some("ack")=>{if self.aborted_records.contains_key(&id)||!self.attempted.contains(&id)||self.acknowledgments.insert(id,e["receipt"].clone()).is_some(){return Err("DISPATCH_ACK_WITHOUT_ATTEMPT".into())}},
   Some("recovered_ack")=>{if self.aborted_records.contains_key(&id)||!self.attempted.contains(&id)||self.acknowledgments.insert(id,e["order"].clone()).is_some()||self.recovered.insert(id,e.clone()).is_some(){return Err("DISPATCH_RECOVERY_WITHOUT_ATTEMPT".into())}},
   Some("aborted_before_admission")=>{self.validate_abort_record(e)?;self.aborted_records.insert(id,e.clone());},
   Some("closed")=>{if !self.acknowledgments.contains_key(&id)||!self.closed.insert(id){return Err("DISPATCH_TERMINAL_WITHOUT_ACK".into())}},
   Some("cancel_ack")=>{if !self.acknowledgments.contains_key(&id)||!self.cancel_acknowledged.insert(id){return Err("DISPATCH_CANCEL_ACK_WITHOUT_ORDER_OR_DUPLICATE".into())}},
   Some("error")=>{if e["operation"].is_null()&&self.attempted.contains(&id)&&!self.acknowledgments.contains_key(&id){if self.aborted_records.contains_key(&id){return Err("DISPATCH_ERROR_AFTER_ABORT".into())}self.errors.insert(id,e.clone());}},
   Some("cancel_attempt")|Some("source")|Some("not_sent")=>{},
   _=>return Err("DISPATCH_EVENT".into())
  }Ok(())
 }
 pub fn append(&mut self,event:Value)->Result<()>{
  if event["kind"]=="aborted_before_admission"{return Err("DISPATCH_TYPED_ABORT_REQUIRED".into())}self.append_inner(event)
 }
 fn append_inner(&mut self,event:Value)->Result<()>{
  let mut next=json!({"sequence":self.sequence+1,"previous":self.previous,"event":event});let digest=hash(&next);next["hash"]=json!(digest);
  // An append failure ends the process; no network call follows uncertain durability.
  let bytes=serde_json::to_vec(&next)?;self.file.write_all(&bytes)?;self.file.write_all(b"\n")?;self.file.sync_all()?;
  self.apply(&event)?;self.sequence+=1;self.previous=digest;Ok(())
 }
 /// Only opaque SDK verification populates this process-local set. Replaying
 /// sealed JSON cannot resolve an attempt; startup must reverify SDK originals.
 pub fn proved_no_network(&self,qid:u64)->bool{self.proved_no_network.contains(&qid)}
 pub fn aborted_records(&self)->&BTreeMap<u64,Value>{&self.aborted_records}
 pub fn original_attempt(&self,qid:u64)->Option<&Value>{self.attempts.get(&qid)}
 pub fn original_error(&self,qid:u64)->Option<&Value>{self.errors.get(&qid)}
 pub fn pre_admission_originals(&self,qid:u64)->Result<(RequestSpec,Permit,u64,TransportError)>{self.abort_context(qid)}
 fn abort_context(&self,qid:u64)->Result<(RequestSpec,Permit,u64,TransportError)>{
  let intent=self.intents.get(&qid).ok_or("DISPATCH_ABORT_INTENT")?;let attempt=self.attempts.get(&qid).ok_or("DISPATCH_ABORT_ATTEMPT")?;let error=self.errors.get(&qid).ok_or("DISPATCH_ABORT_ORIGINAL_ERROR")?;
  let at=clock(&attempt["at"])?;if attempt.get("captureAtMs").is_some()&&attempt["captureAtMs"].as_u64()!=Some(at){return Err("DISPATCH_ABORT_ATTEMPT_CLOCK".into())}
  if clock(&error["at"])?<at||error["error"]["mayHaveExecuted"]!=false{return Err("DISPATCH_ABORT_ERROR_CLOCK_OR_UNKNOWN".into())}
  Ok((serde_json::from_value(intent["spec"].clone())?,serde_json::from_value(intent["permit"].clone())?,at,stored_error(&error["error"])?))
 }
 /// The SDK capability is checked against this held journal's original intent,
 /// attempt and error before writing a separate outcome. It grants no retry.
 pub fn append_aborted_before_admission(&mut self,qid:u64,proof:&OrdinaryPreAdmissionProof,at:&str)->Result<()>{
  let(spec,permit,attempt,error)=self.abort_context(qid)?;proof.validate_binding(&spec,&permit,attempt,&error)?;
  let event=json!({"kind":"aborted_before_admission","qid":qid,"at":at,"originalIntentSha256":hash(&self.intents[&qid]),"originalAttemptSha256":hash(&self.attempts[&qid]),"originalErrorSha256":hash(&self.errors[&qid]),"proof":proof.evidence()});
  self.validate_abort_record(&event)?;self.append_inner(event)?;self.proved_no_network.insert(qid);Ok(())
 }
 /// Called on restart only after the SDK reopens/verifies its original retained
 /// sources. A structurally valid serialized abort alone remains unresolved.
 pub fn verify_aborted_before_admission(&mut self,qid:u64,proof:&OrdinaryPreAdmissionProof)->Result<()>{
  let e=self.aborted_records.get(&qid).ok_or("DISPATCH_ABORT_RECORD_REQUIRED")?;
  if e["proof"]!=*proof.evidence(){return Err("DISPATCH_ABORT_ORIGINAL_PROOF_CHANGED".into())}
  let(spec,permit,attempt,error)=self.abort_context(qid)?;proof.validate_binding(&spec,&permit,attempt,&error)?;self.proved_no_network.insert(qid);Ok(())
 }
 fn validate_abort_record(&self,e:&Value)->Result<()>{
  let id=e["qid"].as_u64().ok_or("DISPATCH_QID")?;
  let intent=self.intents.get(&id).ok_or("DISPATCH_ABORT_INTENT")?;let attempt=self.attempts.get(&id).ok_or("DISPATCH_ABORT_ATTEMPT")?;
  let error=self.errors.get(&id).ok_or("DISPATCH_ABORT_ORIGINAL_ERROR")?;self.abort_context(id)?;
  if e["kind"]!="aborted_before_admission"||self.aborted_records.contains_key(&id)||self.acknowledgments.contains_key(&id)||self.closed.contains(&id)||e["originalIntentSha256"]!=hash(intent)||e["originalAttemptSha256"]!=hash(attempt)||e["originalErrorSha256"]!=hash(error)||!e["proof"].is_object()||clock(&e["at"])?<clock(&error["at"])?{return Err("DISPATCH_ABORT_BINDING".into())}Ok(())
 }
 pub fn require_resolved_attempts(&self)->Result<()>{if self.attempted.iter().any(|q|!self.acknowledgments.contains_key(q)&&!self.proved_no_network.contains(q)){return Err("DISPATCH_UNKNOWN_ACCEPTANCE_MANUAL_RECONCILIATION".into())}Ok(())}
 pub fn never_attempted(&self,qid:u64)->bool{!self.attempted.contains(&qid)&&!self.acknowledgments.contains_key(&qid)}
 pub fn proof(&self,qid:u64)->Value{json!({"qid":qid,"chain":self.previous,"sequence":self.sequence,"networkAttempts":if self.never_attempted(qid){0}else{1}})}
}
#[cfg(test)]mod tests{
 use super::*;
 fn path()->std::path::PathBuf{let p=std::env::temp_dir().join(format!("moloch-dispatch-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));fs::create_dir(&p).unwrap();p.join("dispatch.ndjson")}
 #[test]fn unknown_attempt_survives_restart_and_cannot_be_replaced(){let p=path();{let mut l=DispatchLog::open(&p).unwrap();l.append(json!({"kind":"intent","qid":1,"spec":{"price":0.5}})).unwrap();assert!(l.never_attempted(1));l.append(json!({"kind":"attempt","qid":1})).unwrap();assert!(!l.never_attempted(1));assert!(l.require_resolved_attempts().is_err());}let l=DispatchLog::open(&p).unwrap();assert!(l.require_resolved_attempts().is_err());assert!(!l.never_attempted(1));fs::remove_dir_all(p.parent().unwrap()).unwrap();}
 #[test]fn acknowledged_orders_and_terminal_proof_survive_restart(){let p=path();{let mut l=DispatchLog::open(&p).unwrap();for e in [json!({"kind":"intent","qid":1}),json!({"kind":"attempt","qid":1}),json!({"kind":"ack","qid":1,"receipt":{"orderId":77}}),json!({"kind":"closed","qid":1})]{l.append(e).unwrap();}}let l=DispatchLog::open(&p).unwrap();assert_eq!(l.acknowledgments[&1]["orderId"],77);assert!(l.closed.contains(&1));l.require_resolved_attempts().unwrap();fs::remove_dir_all(p.parent().unwrap()).unwrap();}
 #[test]fn truncation_or_mutation_is_not_a_clean_log(){let p=path();{let mut l=DispatchLog::open(&p).unwrap();l.append(json!({"kind":"intent","qid":1})).unwrap();}let text=fs::read_to_string(&p).unwrap();fs::write(&p,text.replace("\"qid\":1","\"qid\":2")).unwrap();assert!(DispatchLog::open(&p).is_err());fs::write(&p,"{").unwrap();assert!(DispatchLog::open(&p).is_err());fs::remove_dir_all(p.parent().unwrap()).unwrap();}
 #[test]fn confirmed_cancel_survives_restart_while_terminal_reconciliation_remains_pending(){let p=path();{let mut l=DispatchLog::open(&p).unwrap();for e in [json!({"kind":"intent","qid":1}),json!({"kind":"attempt","qid":1}),json!({"kind":"ack","qid":1,"receipt":{"orderId":77}}),json!({"kind":"cancel_attempt","qid":1}),json!({"kind":"cancel_ack","qid":1,"receipt":{"orderId":77}})]{l.append(e).unwrap();}}let l=DispatchLog::open(&p).unwrap();assert!(l.cancel_acknowledged.contains(&1));assert!(!l.closed.contains(&1));l.require_resolved_attempts().unwrap();fs::remove_dir_all(p.parent().unwrap()).unwrap();}
 fn pending(l:&mut DispatchLog,qid:u64){let body=json!({"tournamentId":"cup","exchangeId":"101","side":"yes","action":"buy","quantity":1,"price":0.5,"idempotencyKey":format!("run:q{qid}"),"expirationDate":"1970-01-01T00:00:10.000Z"});let spec=RequestSpec{method:"POST".into(),endpoint:"/orders".into(),body_json:Some(body.to_string()),body:Some(body)};let permit=Permit{run_id:"run".into(),intent_id:format!("q{qid}"),owner:"Codex".into(),expires_at_ms:1999,maximum_quantity:1,maximum_cost:"0.500000".into()};let error=TransportError{code:"PERMIT_EXPIRED_OR_INVALID".into(),status:None,may_have_executed:false,retry_after_ms:0,order_id:None,api_code:None};for e in [json!({"kind":"intent","qid":qid,"spec":spec,"permit":permit}),json!({"kind":"attempt","qid":qid,"at":"1970-01-01T00:00:02.000Z","captureAtMs":2000}),json!({"kind":"error","qid":qid,"at":"1970-01-01T00:00:02.001Z","error":error})]{l.append(e).unwrap();}}
 fn untrusted_abort(l:&DispatchLog,qid:u64)->Value{json!({"kind":"aborted_before_admission","qid":qid,"at":"1970-01-01T00:00:02.002Z","originalIntentSha256":hash(&l.intents[&qid]),"originalAttemptSha256":hash(&l.attempts[&qid]),"originalErrorSha256":hash(&l.errors[&qid]),"proof":{"kind":"caller_claims_no_network"}})}
 #[test]fn arbitrary_abort_json_is_not_an_append_capability(){let p=path();let mut l=DispatchLog::open(&p).unwrap();pending(&mut l,1);let original=fs::read(&p).unwrap();assert!(l.append(untrusted_abort(&l,1)).unwrap_err().to_string().contains("TYPED_ABORT"));assert_eq!(original,fs::read(&p).unwrap());assert!(!l.proved_no_network(1));assert!(l.require_resolved_attempts().is_err());drop(l);fs::remove_dir_all(p.parent().unwrap()).unwrap();}
 #[test]fn rehashed_abort_json_replays_as_untrusted_and_never_makes_retryable(){let p=path();let mut l=DispatchLog::open(&p).unwrap();pending(&mut l,1);let prefix=fs::read(&p).unwrap();l.append_inner(untrusted_abort(&l,1)).unwrap();assert!(fs::read(&p).unwrap().starts_with(&prefix));drop(l);let l=DispatchLog::open(&p).unwrap();assert_eq!(l.aborted_records().len(),1);assert!(!l.proved_no_network(1));assert!(!l.never_attempted(1));assert!(l.require_resolved_attempts().is_err());assert!(l.acknowledgments.is_empty()&&l.closed.is_empty());assert_eq!(l.proof(1)["networkAttempts"],1);drop(l);fs::remove_dir_all(p.parent().unwrap()).unwrap();}
 #[test]fn abort_record_requires_exact_prior_error_attempt_and_intent(){let p=path();let mut l=DispatchLog::open(&p).unwrap();pending(&mut l,1);for field in ["originalIntentSha256","originalAttemptSha256","originalErrorSha256"]{let mut e=untrusted_abort(&l,1);e[field]=json!("0".repeat(64));assert!(l.validate_abort_record(&e).is_err());}let mut e=untrusted_abort(&l,1);e["at"]=json!("1970-01-01T00:00:02.000Z");assert!(l.validate_abort_record(&e).is_err());let e=untrusted_abort(&l,1);l.errors.remove(&1);assert!(l.validate_abort_record(&e).is_err());drop(l);fs::remove_dir_all(p.parent().unwrap()).unwrap();}
 #[test]fn serialized_abort_never_authorizes_ack_terminal_or_second_attempt(){let p=path();let mut l=DispatchLog::open(&p).unwrap();pending(&mut l,1);l.append_inner(untrusted_abort(&l,1)).unwrap();for e in [json!({"kind":"attempt","qid":1}),json!({"kind":"ack","qid":1,"receipt":{"orderId":77}}),json!({"kind":"recovered_ack","qid":1,"order":{"id":77}}),json!({"kind":"closed","qid":1})]{assert!(l.apply(&e).is_err());}assert!(l.acknowledgments.is_empty()&&l.closed.is_empty());assert!(!l.never_attempted(1));drop(l);fs::remove_dir_all(p.parent().unwrap()).unwrap();}
}
