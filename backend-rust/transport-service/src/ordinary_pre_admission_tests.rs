#![cfg(unix)]
use super::*;
use std::{fs,path::PathBuf,sync::{Arc,atomic::{AtomicU64,Ordering}}};
fn temp()->PathBuf{let p=std::env::temp_dir().join(format!("ordinary-unadmitted-{}-{}",std::process::id(),SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));fs::create_dir(&p).unwrap();p}
fn scope()->Scope{Scope{tournament_id:"11111111-1111-4111-8111-111111111111".into(),tournament_slug:"cup".into(),exchanges:[("1009".into(),"319".into())].into()}}
fn config(dir:&std::path::Path)->Config{Config{queue_dir:dir.into(),account_scope:"synthetic-original-account".into(),scope:scope(),maximum_pages:3,writes_enabled:true}}
fn permit()->Permit{Permit{run_id:"fixture".into(),intent_id:"3411".into(),owner:"Codex".into(),expires_at_ms:1999,maximum_quantity:5196,maximum_cost:"3221.520000".into()}}
fn spec()->RequestSpec{RequestSpec{method:"POST".into(),endpoint:"/orders".into(),body:Some(json!({"idempotencyKey":"fixture:3411","exchangeId":"1009","side":"no","action":"buy","quantity":5196,"price":0.62,"expirationDate":"2030-01-01T00:00:00.000Z","tournamentId":scope().tournament_id})),body_json:None}}
struct TestClock(Arc<AtomicU64>);impl Clock for TestClock{fn now_ms(&self)->u64{self.0.load(Ordering::SeqCst)}fn sleep_ms(&mut self,n:u64)->Result<()>{self.0.fetch_add(n,Ordering::SeqCst);Ok(())}}
fn original_error(dir:&std::path::Path)->TransportError{
 let clock=Arc::new(AtomicU64::new(1000));let mut t=Transport::new_for_mock(config(dir),Box::new(DisabledCredentials),Box::new(TestClock(clock.clone())),Box::new(ReadOnlyAuthority),"http://127.0.0.1:9/api/v1".into()).unwrap();t.admission_state().unwrap();clock.store(2001,Ordering::SeqCst);
 let body=spec().body.unwrap();let e=t.place(Order{idempotency_key:"fixture:3411".into(),exchange_id:"1009".into(),side:"no".into(),action:"buy".into(),quantity:5196,price:"0.620000".into(),expiration_date:body["expirationDate"].as_str().unwrap().into(),tournament_id:scope().tournament_id},permit()).err().unwrap();assert_eq!(e.code,"PERMIT_EXPIRED_OR_INVALID");assert!(!e.may_have_executed);assert_eq!(t.admission_summary().unwrap().state.last_clock,1000);assert!(t.admission_lookup_intent("fixture:3411").unwrap().binding.is_none());e
}
#[test]fn real_sdk_expired_guard_and_positive_frozen_core_watermark_mint_bound_retained_proof(){let p=temp();let error=original_error(&p);let c=config(&p);let proof=verify_frozen_ordinary_pre_admission(&c,&spec(),&permit(),2000,&error).unwrap();proof.validate_binding(&spec(),&permit(),2000,&error).unwrap();assert_eq!(proof.evidence()["originalHead"]["lastClock"],1000);assert_eq!(proof.evidence()["binding"]["intent"],"fixture:3411");assert!(!p.join("offline-account-queue.lock").exists());
 let original=proof.evidence().clone();{let mut q=admission::AccountAdmission::open(&p,&c.account_scope,"later-controller").unwrap();q.command(json!({"op":"status","scope":c.account_scope,"now":3000})).unwrap();q.command(json!({"op":"compact","scope":c.account_scope,"now":3000})).unwrap();}
 let retained=verify_retained_ordinary_pre_admission(&c,&spec(),&permit(),2000,&error,&original).unwrap();assert_eq!(retained.evidence(),&original);assert!(verify_frozen_ordinary_pre_admission(&c,&spec(),&permit(),2000,&error).is_err());fs::remove_dir_all(p).unwrap();}
#[test]fn live_controller_same_or_newer_clock_prior_intent_or_key_never_proves_unadmitted(){for mode in 0..4{let p=temp();let error=original_error(&p);let c=config(&p);let mut q=admission::AccountAdmission::open(&p,&c.account_scope,"guard-controller").unwrap();if mode==0{assert!(verify_frozen_ordinary_pre_admission(&c,&spec(),&permit(),2000,&error).is_err());}else{
 if mode==1{q.command(json!({"op":"status","scope":c.account_scope,"now":2000})).unwrap();}else{q.command(json!({"op":"admit","scope":c.account_scope,"now":1500,"request":"original-admitted","write":true,"authorized":true,"lease_valid":true,"permit_valid":true,"binding":{"intent":if mode==2{"fixture:3411"}else{"different-intent"},"key":"fixture:3411","payload_hash":"a".repeat(64)}})).unwrap();q.command(json!({"op":"complete","scope":c.account_scope,"now":1501,"request":"original-admitted","outcome":"not_sent"})).unwrap();}drop(q);assert!(verify_frozen_ordinary_pre_admission(&c,&spec(),&permit(),2000,&error).is_err());}fs::remove_dir_all(p).unwrap();}}
#[test]fn original_spec_permit_error_clock_and_archive_mutation_cannot_rebind_proof(){let p=temp();let error=original_error(&p);let c=config(&p);let proof=verify_frozen_ordinary_pre_admission(&c,&spec(),&permit(),2000,&error).unwrap();for mode in 0..5{let mut s=spec();let mut per=permit();let mut e=error.clone();let mut at=2000;match mode{0=>s.body.as_mut().unwrap()["quantity"]=json!(5195),1=>per.intent_id="3412".into(),2=>e.may_have_executed=true,3=>at=2001,_=>e.status=Some(200)}assert!(proof.validate_binding(&s,&per,at,&e).is_err());assert!(verify_retained_ordinary_pre_admission(&c,&s,&per,at,&e,proof.evidence()).is_err());}
 let name=proof.evidence()["sourceArchive"].as_str().unwrap();let path=p.join(name);let mut bytes=fs::read(&path).unwrap();bytes.push(b' ');fs::write(path,bytes).unwrap();assert!(verify_retained_ordinary_pre_admission(&c,&spec(),&permit(),2000,&error,proof.evidence()).is_err());fs::remove_dir_all(p).unwrap();}
#[test]fn torn_or_replaced_or_missing_positive_source_is_not_a_zero_network_receipt(){for mode in 0..3{let p=temp();let error=original_error(&p);match mode{0=>{let file=p.join("account-queue.jsonl");let mut bytes=fs::read(&file).unwrap();bytes.pop();fs::write(file,bytes).unwrap();},1=>{fs::write(p.join("account-queue.jsonl"),b"").unwrap();},_=>{let bytes=fs::read_to_string(p.join("account-queue.jsonl")).unwrap();fs::write(p.join("account-queue.jsonl"),bytes.replace("last_clock\":1000","last_clock\":999")).unwrap();}}assert!(verify_frozen_ordinary_pre_admission(&config(&p),&spec(),&permit(),2000,&error).is_err());fs::remove_dir_all(p).unwrap();}}

#[test]fn original_held_controller_can_seal_only_pre_admission_and_later_operations_invalidate_capture(){let p=temp();let c=config(&p);let clock=Arc::new(AtomicU64::new(1000));let mut t=Transport::new_for_mock(c.clone(),Box::new(DisabledCredentials),Box::new(TestClock(clock.clone())),Box::new(ReadOnlyAuthority),"http://127.0.0.1:9/api/v1".into()).unwrap();t.admission_state().unwrap();clock.store(2001,Ordering::SeqCst);let b=spec().body.unwrap();let error=t.place(Order{idempotency_key:"fixture:3411".into(),exchange_id:"1009".into(),side:"no".into(),action:"buy".into(),quantity:5196,price:"0.620000".into(),expiration_date:b["expirationDate"].as_str().unwrap().into(),tournament_id:scope().tournament_id},permit()).err().unwrap();let proof=t.verify_current_ordinary_pre_admission(&spec(),&permit(),2000,&error).unwrap();assert_eq!(proof.evidence()["originalHead"]["lastClock"],1000);assert!(p.join("offline-account-queue.lock").exists());t.admission_state().unwrap();assert!(t.verify_current_ordinary_pre_admission(&spec(),&permit(),2000,&error).is_err());let historical=verify_retained_ordinary_pre_admission(&c,&spec(),&permit(),2000,&error,proof.evidence()).unwrap();assert_eq!(historical.evidence(),proof.evidence());t.poisoned=true;assert!(t.verify_current_ordinary_pre_admission(&spec(),&permit(),2000,&error).is_err());drop(t);fs::remove_dir_all(p).unwrap();}
#[test]fn other_global_unknown_or_active_request_is_not_bypassed_by_exact_intent_absence(){for unknown in [false,true]{let p=temp();let error=original_error(&p);let c=config(&p);{let mut q=admission::AccountAdmission::open(&p,&c.account_scope,"other-global").unwrap();let binding=json!({"intent":"different-intent","key":"different-key","payload_hash":"a".repeat(64)});q.command(json!({"op":"admit","scope":c.account_scope,"now":1500,"request":"other","write":true,"authorized":true,"lease_valid":true,"permit_valid":true,"binding":binding})).unwrap();if unknown{q.command(json!({"op":"network","scope":c.account_scope,"now":1501,"request":"other","write":true,"authorized":true,"lease_valid":true,"permit_valid":true,"binding":binding})).unwrap();q.command(json!({"op":"complete","scope":c.account_scope,"now":1502,"request":"other","outcome":"unknown"})).unwrap();}}assert!(verify_frozen_ordinary_pre_admission(&c,&spec(),&permit(),2000,&error).is_err());fs::remove_dir_all(p).unwrap();}}

#[test]fn archived_proof_without_dispatch_abort_is_reused_under_distinct_genuine_capture_leases(){
 let p=temp();let error=original_error(&p);let c=config(&p);let first=verify_frozen_ordinary_pre_admission(&c,&spec(),&permit(),2000,&error).unwrap();let archive=p.join(first.evidence()["sourceArchive"].as_str().unwrap());let original=fs::read(&archive).unwrap();let inode={use std::os::unix::fs::MetadataExt;fs::metadata(&archive).unwrap().ino()};
 // No dispatch-abort row was written. A later actual SDK owner has a different
 // token and leaseKind, but performs NO queue command or account operation.
 assert!(!p.join("dispatch.ndjson").exists());let clock=Arc::new(AtomicU64::new(2500));let t=Transport::new_for_mock(c.clone(),Box::new(DisabledCredentials),Box::new(TestClock(clock)),Box::new(ReadOnlyAuthority),"http://127.0.0.1:9/api/v1".into()).unwrap();let held=t.verify_current_ordinary_pre_admission(&spec(),&permit(),2000,&error).unwrap();assert_eq!(held.evidence(),first.evidence());drop(t);
 let next=verify_frozen_ordinary_pre_admission(&c,&spec(),&permit(),2000,&error).unwrap();assert_eq!(next.evidence(),first.evidence());assert_eq!(fs::read(&archive).unwrap(),original);{use std::os::unix::fs::MetadataExt;assert_eq!(fs::metadata(&archive).unwrap().ino(),inode);}assert!(!p.join("offline-account-queue.lock").exists());fs::remove_dir_all(p).unwrap();
}

#[test]fn original_held_sdk_archive_survives_repeat_then_offline_capture_without_rewriting_old_lease(){
 let p=temp();let c=config(&p);let clock=Arc::new(AtomicU64::new(1000));let mut t=Transport::new_for_mock(c.clone(),Box::new(DisabledCredentials),Box::new(TestClock(clock.clone())),Box::new(ReadOnlyAuthority),"http://127.0.0.1:9/api/v1".into()).unwrap();t.admission_state().unwrap();clock.store(2001,Ordering::SeqCst);
 let b=spec().body.unwrap();let error=t.place(Order{idempotency_key:"fixture:3411".into(),exchange_id:"1009".into(),side:"no".into(),action:"buy".into(),quantity:5196,price:"0.620000".into(),expiration_date:b["expirationDate"].as_str().unwrap().into(),tournament_id:scope().tournament_id},permit()).err().unwrap();let first=t.verify_current_ordinary_pre_admission(&spec(),&permit(),2000,&error).unwrap();let path=p.join(first.evidence()["sourceArchive"].as_str().unwrap());let original=fs::read(&path).unwrap();let v:Value=serde_json::from_slice(&original).unwrap();assert_eq!(v["source"]["leaseKind"],"heldOriginalSdkController");
 assert_eq!(t.verify_current_ordinary_pre_admission(&spec(),&permit(),2000,&error).unwrap().evidence(),first.evidence());drop(t);let recovered=verify_frozen_ordinary_pre_admission(&c,&spec(),&permit(),2000,&error).unwrap();assert_eq!(recovered.evidence(),first.evidence());assert_eq!(fs::read(&path).unwrap(),original);let v:Value=serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();assert_eq!(v["source"]["leaseKind"],"heldOriginalSdkController");fs::remove_dir_all(p).unwrap();
}

#[test]fn existing_archive_cannot_mask_new_quiet_clock_or_compacted_generation_below_original_attempt(){
 for mode in 0..3{let p=temp();let error=original_error(&p);let c=config(&p);let proof=verify_frozen_ordinary_pre_admission(&c,&spec(),&permit(),2000,&error).unwrap();let path=p.join(proof.evidence()["sourceArchive"].as_str().unwrap());let original=fs::read(&path).unwrap();
  {let mut q=admission::AccountAdmission::open(&p,&c.account_scope,"later-generation").unwrap();if mode==1{q.command(json!({"op":"compact","scope":c.account_scope,"now":1000})).unwrap();}else if mode==0{q.command(json!({"op":"status","scope":c.account_scope,"now":1500})).unwrap();}else{let binding=json!({"intent":"other-unknown","key":"other-unknown-key","payload_hash":"a".repeat(64)});for op in ["admit","network"]{q.command(json!({"op":op,"scope":c.account_scope,"now":1500,"request":"other-write","write":true,"authorized":true,"lease_valid":true,"permit_valid":true,"binding":binding})).unwrap();}q.command(json!({"op":"complete","scope":c.account_scope,"now":1501,"request":"other-write","outcome":"unknown"})).unwrap();}}
  let denied=verify_frozen_ordinary_pre_admission(&c,&spec(),&permit(),2000,&error).err().unwrap();if mode<2{assert_eq!(denied.code,"UNADMITTED_ARCHIVE_FRESH_SOURCE_CHANGED");}assert_eq!(fs::read(&path).unwrap(),original);
  // Historical proof verification remains a separate, explicitly bound API;
  // it does not make a new current capture valid after generation advancement.
  assert_eq!(verify_retained_ordinary_pre_admission(&c,&spec(),&permit(),2000,&error,proof.evidence()).unwrap().evidence(),proof.evidence());fs::remove_dir_all(p).unwrap();
 }
}

#[test]fn archive_reuse_rejects_torn_noncanonical_changed_or_symlink_sources_without_repair(){
 for mode in 0..5{let p=temp();let error=original_error(&p);let c=config(&p);let proof=verify_frozen_ordinary_pre_admission(&c,&spec(),&permit(),2000,&error).unwrap();let path=p.join(proof.evidence()["sourceArchive"].as_str().unwrap());let original=fs::read(&path).unwrap();
  match mode{0=>{fs::write(&path,&original[..original.len()-1]).unwrap();},1=>{let mut b=original.clone();b.push(b' ');fs::write(&path,b).unwrap();},2=>{let mut v:Value=serde_json::from_slice(&original).unwrap();v["source"]["verifiedHead"]["lastClock"]=json!(999);fs::write(&path,serde_json::to_vec(&v).unwrap()).unwrap();},3=>{fs::write(&path,b"").unwrap();},_=>{let retained=p.join("retained-exact-original");fs::rename(&path,&retained).unwrap();std::os::unix::fs::symlink(&retained,&path).unwrap();}}
  let damaged=fs::read(&path).unwrap();assert!(verify_frozen_ordinary_pre_admission(&c,&spec(),&permit(),2000,&error).is_err(),"mode{mode}");assert_eq!(fs::read(&path).unwrap(),damaged);if mode==4{assert!(fs::symlink_metadata(&path).unwrap().file_type().is_symlink());}fs::remove_dir_all(p).unwrap();
 }
}

struct StaleAuthority;
impl Authority for StaleAuthority{ fn owns_order(&self,_:&str)->bool{false}
 fn can_read(&self,_:&Scope)->bool{true}
 fn authorize(&self,_:&Scope,_:&RequestSpec,_:&Permit,_:bool,_:u64)->Result<()>{Err(fail("LIVE_CONTRACT_OR_BOOK_STALE"))}
}
struct NoCredentialLoad(Arc<AtomicU64>);
impl CredentialProvider for NoCredentialLoad{fn load(&self)->Result<zeroize::Zeroizing<String>>{self.0.fetch_add(1,Ordering::SeqCst);Err(fail("UNEXPECTED_CREDENTIAL_LOAD"))}}
#[test]fn genuine_stale_authority_before_admit_seals_original_no_http_proof_and_survives_restart(){
 let dir=temp();let c=config(&dir);let clock=Arc::new(AtomicU64::new(1000));let loads=Arc::new(AtomicU64::new(0));
 let mut t=Transport::new_for_mock(c.clone(),Box::new(NoCredentialLoad(loads.clone())),Box::new(TestClock(clock.clone())),Box::new(StaleAuthority),"http://127.0.0.1:9/api/v1".into()).unwrap();
 t.admission_state().unwrap();clock.store(2001,Ordering::SeqCst);let mut p=permit();p.expires_at_ms=10000;
 let e=t.place(Order{idempotency_key:"fixture:3411".into(),exchange_id:"1009".into(),side:"no".into(),action:"buy".into(),quantity:5196,price:"0.620000".into(),expiration_date:"2030-01-01T00:00:00.000Z".into(),tournament_id:scope().tournament_id},p.clone()).unwrap_err();
 assert_eq!(e.code,"LIVE_CONTRACT_OR_BOOK_STALE");assert!(!e.may_have_executed);assert_eq!(loads.load(Ordering::SeqCst),0);
 let proof=t.verify_current_ordinary_pre_admission(&spec(),&p,2000,&e).unwrap();proof.validate_binding(&spec(),&p,2000,&e).unwrap();
 assert_eq!(proof.evidence()["originalHead"]["lastClock"],1000);let retained=proof.evidence().clone();drop(t);
 let rebuilt=verify_retained_ordinary_pre_admission(&c,&spec(),&p,2000,&e,&retained).unwrap();assert_eq!(rebuilt.evidence(),&retained);
 let mut changed=e.clone();changed.code="PERMIT_EXPIRED_OR_INVALID".into();assert!(verify_retained_ordinary_pre_admission(&c,&spec(),&p,2000,&changed,&retained).is_err());
 for mode in 0..5{let mut bad=e.clone();match mode{0=>bad.code="ARBITRARY_LOCAL_GUARD".into(),1=>bad.may_have_executed=true,2=>bad.status=Some(503),3=>bad.api_code=Some("LOCAL".into()),_=>bad.order_id=Some("999".into())}assert!(verify_frozen_ordinary_pre_admission(&c,&spec(),&p,2000,&bad).is_err());}
 fs::remove_dir_all(dir).unwrap();
}
#[test]fn stale_error_cannot_turn_admission_or_global_uncertainty_into_no_network(){
 for mode in 0..5{let dir=temp();let _=original_error(&dir);let c=config(&dir);let mut p=permit();p.expires_at_ms=10000;
  let error=TransportError{code:"LIVE_CONTRACT_OR_BOOK_STALE".into(),status:None,may_have_executed:false,retry_after_ms:0,order_id:None,api_code:None};
  {let mut q=admission::AccountAdmission::open(&dir,&c.account_scope,"stale-negative").unwrap();
   if mode==0{q.command(json!({"op":"status","scope":c.account_scope,"now":2000})).unwrap();}
   else{let binding=json!({"intent":if mode==1{"fixture:3411"}else{"other"},"key":if mode==2{"fixture:3411"}else{"other-key"},"payload_hash":"a".repeat(64)});
    q.command(json!({"op":"admit","scope":c.account_scope,"now":1500,"request":"admitted","write":true,"authorized":true,"lease_valid":true,"permit_valid":true,"binding":binding})).unwrap();
    if mode<3{q.command(json!({"op":"complete","scope":c.account_scope,"now":1501,"request":"admitted","outcome":"not_sent"})).unwrap();}
    else if mode==4{q.command(json!({"op":"network","scope":c.account_scope,"now":1501,"request":"admitted","write":true,"authorized":true,"lease_valid":true,"permit_valid":true,"binding":binding})).unwrap();q.command(json!({"op":"complete","scope":c.account_scope,"now":1502,"request":"admitted","outcome":"unknown"})).unwrap();}
   }
  }
  assert!(verify_frozen_ordinary_pre_admission(&c,&spec(),&p,2000,&error).is_err(),"mode{mode}");fs::remove_dir_all(dir).unwrap();
 }
}
