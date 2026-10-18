//! Finite owner-leased recovery. Never submits or retries a contest order.
use baal_transport_service::{Authority,Scope,RequestSpec,Permit,TransportError,ReadOnlyAuthority,Config,Transport,SystemClock,Endpoint,Receipt,credentials::UnixFileCredentials};
use moloch_live_runtime::{dispatch::DispatchLog,expired_exposure as E,feedback};
use moloch_replay::session::{ExclusiveLock,Session};
use serde_json::{Value,json};
use std::{fs,io::Write,path::PathBuf,process::Command,sync::{Arc,Mutex},os::unix::fs::OpenOptionsExt};
fn iso()->String{chrono::DateTime::from(std::time::SystemTime::now()).to_rfc3339_opts(chrono::SecondsFormat::Millis,true)}
fn private_new(path:&std::path::Path,bytes:&[u8])->Result<(),Box<dyn std::error::Error>>{let mut f=fs::OpenOptions::new().create_new(true).write(true).mode(0o600).open(path)?;f.write_all(bytes)?;f.sync_all()?;fs::File::open(path.parent().ok_or("EXPOSURE_PARENT")?)?.sync_all()?;Ok(())}
// Endpoint contains typed observations only. Keep the restored bank and original
// shared SDK alive across temporary GET failures; never retry a write or change
// an unknown binding. SDK admission still enforces durable pacing/Retry-After.
trait RecoveryReads{
 fn state(&mut self)->Result<Value,TransportError>;
 fn observe(&mut self,endpoint:Endpoint)->Result<Receipt,TransportError>;
}
impl RecoveryReads for Transport{
 fn state(&mut self)->Result<Value,TransportError>{self.admission_state()}
 fn observe(&mut self,endpoint:Endpoint)->Result<Receipt,TransportError>{self.request(endpoint)}
}
fn read_guard(state:&Value,unknown:&Value)->Result<(),Box<dyn std::error::Error>>{
 let s=&state["state"];
 if s["manual_review"]!=false||s.get("active")!=Some(&Value::Null)||!s["unknown"].is_object()||&s["unknown"]!=unknown{return Err("EXPOSURE_READ_QUEUE_CHANGED".into())}Ok(())
}
fn recovery_read<T:RecoveryReads>(t:&mut T,endpoint:Endpoint,mut pause:impl FnMut(u64))->Result<Receipt,Box<dyn std::error::Error>>{
 let unknown=t.state()?["state"]["unknown"].clone();
 for attempt in 0..12u32{
  read_guard(&t.state()?,&unknown)?;
  let result=t.observe(endpoint.clone());
  read_guard(&t.state()?,&unknown)?;
  match result{
   Ok(receipt)=>return Ok(receipt),
   Err(error) if attempt<11&&moloch_live_runtime::read_recovery::retryable_read(&error)=>{
    eprintln!("{} recovery GET retry {}/12: {} HTTP {:?}; original unknown retained",iso(),attempt+1,error.code,error.status);
    pause((1000u64.saturating_mul(1u64<<attempt.min(5))).max(error.retry_after_ms).min(30_000));
   },
   Err(error)=>return Err(error.into()),
  }
 }unreachable!()
}
fn read(t:&mut Transport,endpoint:Endpoint)->Result<Receipt,Box<dyn std::error::Error>>{recovery_read(t,endpoint,|ms|std::thread::sleep(std::time::Duration::from_millis(ms)))}
struct RecoveryAuthority{scope:Scope,owned:std::collections::BTreeSet<String>,resolved:Arc<Mutex<Option<Value>>>}
impl Authority for RecoveryAuthority{
 fn can_read(&self,s:&Scope)->bool{s.tournament_id==self.scope.tournament_id&&s.exchanges==self.scope.exchanges}
 fn owns_order(&self,id:&str)->bool{self.owned.contains(id)}
 fn authorize(&self,s:&Scope,r:&RequestSpec,p:&Permit,b:bool,n:u64)->Result<(),TransportError>{ReadOnlyAuthority.authorize(s,r,p,b,n)}
 fn reconcile_unknown(&self,s:&Scope,b:&Value,_:u64)->Result<(),TransportError>{if self.can_read(s)&&self.resolved.lock().unwrap().as_ref()==Some(b){Ok(())}else{Err(TransportError{code:"EXPOSURE_EXACT_PROOF_REQUIRED".into(),status:None,may_have_executed:false,retry_after_ms:0,order_id:None,api_code:None})}}
}
fn main()->Result<(),Box<dyn std::error::Error>>{
 let a:Vec<_>=std::env::args().collect();if a.len()!=5||a[4]!="--apply"{return Err("usage: moloch-reconcile-expired-exposure CONTROL QID EXPECTED_CONTROL_SHA256 --apply".into())}
 let raw=fs::read(&a[1])?;if E::file_sha(&raw)!=a[3]{return Err("EXPOSURE_CONTROL_CHANGED".into())}let c:Value=serde_json::from_slice(&raw)?;let root=PathBuf::from(c["root"].as_str().ok_or("EXPOSURE_ROOT")?);let _lease=ExclusiveLock::acquire(&PathBuf::from(c["ownerLock"].as_str().ok_or("EXPOSURE_LOCK")?))?;
 let pid=Command::new("systemctl").args(["--user","show","moloch-live.service","-p","MainPID","--value"]).output()?;if !pid.status.success()||String::from_utf8(pid.stdout)?.trim()!="0"||!root.join("EXECUTION_STOP").is_file(){return Err("EXPOSURE_STOPPED_WRITER_REQUIRED".into())}
 let qid:u64=a[2].parse()?;let mut log=DispatchLog::open(&root.join("dispatch.ndjson"))?;
 let bank_name=c["bankName"].as_str().ok_or("EXPOSURE_BANK")?;if !bank_name.starts_with("bank")||bank_name.contains(['/', '\\']){return Err("EXPOSURE_BANK_PATH".into())}
 let seed:Value=serde_json::from_slice(&fs::read(root.join(format!("{bank_name}-account-seed.json")))?)?;
 eprintln!("{} restoring original financial bank; no account requests or orders yet",iso());
 // Original financial replay precedes the fresh physical observations.
 let mut bank=Session::open_live(&root.join(bank_name),&c["policy"],&seed)?;
 let original_scope:Scope=serde_json::from_value(c["scope"].clone())?;
 let original_config=Config{queue_dir:root.join("account-queue"),account_scope:c["accountScope"].as_str().ok_or("EXPOSURE_ACCOUNT_SCOPE")?.into(),scope:original_scope,maximum_pages:20,writes_enabled:false};
 let aborted:Vec<_>=log.aborted_records().iter().map(|(q,r)|(*q,r["proof"].clone())).collect();
 for(id,evidence)in aborted{let(spec,permit,at,error)=log.pre_admission_originals(id)?;let proof=baal_transport_service::verify_retained_ordinary_pre_admission(&original_config,&spec,&permit,at,&error,&evidence)?;log.verify_aborted_before_admission(id,&proof)?;}
 let guarded:Vec<_>=log.local_final_guard_records().iter().map(|(q,r)|(*q,r["proof"].clone())).collect();
 for(id,evidence)in guarded{let(spec,permit,at,error)=log.local_final_guard_originals(id)?;let proof=baal_transport_service::verify_retained_ordinary_local_guard_no_http(&original_config,&spec,&permit,at,&error,&evidence)?;log.verify_local_final_guard_no_http(id,&proof)?;}
 E::restore(&root,&bank,&mut log)?;
 eprintln!("{} original bank restored; observing exact account through shared SDK",iso());
 let view:Value=serde_json::from_slice(&fs::read(root.join("tartarus-current-native-state.json"))?)?;let scope:Scope=serde_json::from_value(view["scope"].clone())?;
 if scope.tournament_id!=c["scope"]["tournamentId"].as_str().ok_or("EXPOSURE_SCOPE")?{return Err("EXPOSURE_SCOPE_CHANGED".into())}
 let owned=log.acknowledgments.values().map(|r|{let v=&r["data"];v.get("orderId").or_else(||v.get("id")).map(|v|v.as_str().map(str::to_owned).unwrap_or_else(||v.to_string())).ok_or("EXPOSURE_KNOWN_ORDER_ID")}).collect::<Result<std::collections::BTreeSet<_>,_>>()?;
 let resolved=Arc::new(Mutex::new(None));let mut t=Transport::new_with_read_budget(Config{queue_dir:root.join("account-queue"),account_scope:c["accountScope"].as_str().ok_or("EXPOSURE_ACCOUNT_SCOPE")?.into(),scope:scope.clone(),maximum_pages:20,writes_enabled:false},Box::new(UnixFileCredentials::approved_file(PathBuf::from(c["credentials"].as_str().ok_or("EXPOSURE_CREDENTIALS")?))?),Box::new(SystemClock),Box::new(RecoveryAuthority{scope:scope.clone(),owned,resolved:resolved.clone()}),100,600)?.with_full_account_observation();
 let known:Vec<_>=log.acknowledgments.iter().filter(|(id,_)|!log.closed.contains(id)).map(|(id,r)|(*id,r.clone())).collect();
 for(id,r)in known{
  let v=&r["data"];let actual=v.get("orderId").or_else(||v.get("id")).ok_or("EXPOSURE_KNOWN_ID")?;let actual=actual.as_str().map(str::to_owned).unwrap_or_else(||actual.to_string());
  let q=bank.engine().order_records().into_iter().find(|q|q["id"]==id).ok_or("EXPOSURE_KNOWN_NATIVE")?;
  let order=read(&mut t,Endpoint::OwnedOrder{order_id:actual.clone()})?;let fills=read(&mut t,Endpoint::OwnedFills{order_id:actual.clone()})?;
  let terminal=feedback::terminal(&q,&actual,&scope.tournament_id,&order,&fills,&iso())?.ok_or("EXPOSURE_KNOWN_STILL_OPEN")?;
  let path=root.join(format!("expired-exposure-{qid}-known-{id}.private.json"));private_new(&path,&serde_json::to_vec(&json!({"order":order,"fills":fills}))?)?;
  for mut e in feedback::fills(&q,&actual,&scope.tournament_id,&fills,&iso())?.into_iter().chain(std::iter::once(terminal)){e["processedAt"]=json!(iso());bank.accept("expired-exposure-known-orders",bank.last_source_sequence("expired-exposure-known-orders")+1,&e)?;}
  log.append(json!({"kind":"closed","qid":id,"orderReceiptHash":feedback::digest(&order)?,"fillReceiptHash":feedback::digest(&fills)?}))?;
 }
 let before=t.admission_state()?;
 let orders=read(&mut t,Endpoint::Orders{status:"all".into()})?;let account=read(&mut t,Endpoint::Account)?;let positions=read(&mut t,Endpoint::Positions)?;let collateral=read(&mut t,Endpoint::Collateral)?;
 let after=t.admission_state()?;if before["state"]["unknown"]!=after["state"]["unknown"]{return Err("EXPOSURE_UNKNOWN_CHANGED_DURING_READS".into())}
 let proof=E::capture(&bank,&seed,&log,qid,&scope,&after,account,positions,orders,collateral,&iso())?;
 let bytes=serde_json::to_vec(proof.evidence())?;let path=root.join(format!("expired-exposure-{qid}.private.json"));private_new(&path,&bytes)?;
 let sequence=bank.last_source_sequence("expired-exposure-recovery")+1;bank.accept("expired-exposure-recovery",sequence,&proof.event())?;
 log.append_exposure(&proof,&E::file_sha(&bytes),sequence)?;
 *resolved.lock().unwrap()=Some(proof.binding().clone());t.reconcile_unknown(proof.binding().clone())?;
 E::restore(&root,&bank,&mut log)?;log.require_resolved_attempts()?;
 let final_queue=t.admission_state()?;
 private_new(&root.join(format!("expired-exposure-{qid}-completed.private.json")),&serde_json::to_vec(&json!({"qid":qid,"at":iso(),"proofFileSha256":E::file_sha(&bytes),"nativeHead":bank.durable_head(),"queue":final_queue,"ordersSubmitted":0,"stopPreserved":root.join("EXECUTION_STOP").exists()}))?)?;
 println!("{}",json!({"qid":qid,"economicExposureReconciled":true,"originalNetworkAttemptRetained":true,"venueOrderIdentityKnown":false,"sameIntentRetryAuthorized":false,"ordersSubmitted":0,"sdkUnknownCount":final_queue["state"]["unknown"].as_object().map(|m|m.len()),"stopPreserved":true}));Ok(())
}
#[cfg(test)]mod tests{
 use super::*;
 struct Fake{calls:usize,failures:usize,status:u16,unknown_write:bool,change:Option<&'static str>}
 impl RecoveryReads for Fake{
  fn state(&mut self)->Result<Value,TransportError>{let mut v=json!({"state":{"active":null,"manual_review":false,"unknown":{"original":{"key":"original","payload_hash":"retained"}}}});if self.calls>0{match self.change{Some("unknown")=>v["state"]["unknown"]=json!({}),Some("active")=>v["state"]["active"]=json!({"request":"other"}),Some("manual_review")=>v["state"]["manual_review"]=json!(true),_=>()}}Ok(v)}
  fn observe(&mut self,e:Endpoint)->Result<Receipt,TransportError>{assert!(matches!(e,Endpoint::OwnedOrder{order_id}if order_id=="original"));self.calls+=1;if self.calls<=self.failures{Err(TransportError{code:"OFFICIAL_REQUEST_REJECTED".into(),status:Some(self.status),may_have_executed:self.unknown_write,retry_after_ms:524,order_id:None,api_code:None})}else{Ok(Receipt{endpoint:"/orders/original".into(),request_started_at:"original-start".into(),received_at:"original-end".into(),status:200,data:json!({"quantityFilled":0})})}}
 }
 fn fake()->Fake{Fake{calls:0,failures:1,status:500,unknown_write:false,change:None}}
 fn endpoint()->Endpoint{Endpoint::OwnedOrder{order_id:"original".into()}}
 #[test]fn transient_read_preserves_real_receipt_and_original_binding(){let mut f=fake();let mut waits=vec![];let r=recovery_read(&mut f,endpoint(),|ms|waits.push(ms)).unwrap();assert_eq!(f.calls,2);assert_eq!(waits,vec![1000]);assert_eq!(r.request_started_at,"original-start");assert_eq!(r.data,json!({"quantityFilled":0}));}
 #[test]fn persistent_failure_is_bounded(){let mut f=fake();f.failures=100;let mut waits=vec![];assert!(recovery_read(&mut f,endpoint(),|ms|waits.push(ms)).is_err());assert_eq!(f.calls,12);assert_eq!(waits.len(),11);assert!(waits.iter().all(|ms|*ms<=30_000));}
 #[test]fn account_uncertainty_change_blocks_even_a_success(){for change in ["unknown","active","manual_review"]{for failures in [0,1]{let mut f=fake();f.change=Some(change);f.failures=failures;assert!(recovery_read(&mut f,endpoint(),|_|panic!("must not retry")).is_err());assert_eq!(f.calls,1);}}}
 #[test]fn authorization_errors_and_possible_writes_are_never_retried(){for(status,unknown)in [(401,false),(403,false),(409,false),(500,true)]{let mut f=fake();f.status=status;f.unknown_write=unknown;assert!(recovery_read(&mut f,endpoint(),|_|panic!("must not retry")).is_err());assert_eq!(f.calls,1);}}
 #[test]fn missing_guard_fields_fail_closed(){let expected=json!({});for state in [json!({}),json!({"state":{"unknown":{}}}),json!({"state":{"unknown":{},"manual_review":false}})]{assert!(read_guard(&state,&expected).is_err());}}
}
