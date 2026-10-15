#![cfg(unix)]
// Compile the production modules here to exercise their private durability fault
// hook. No SDK account queue/client or network is created by these fixtures.
#[path="../src/package_dispatch.rs"]mod package_dispatch;
#[path="../src/package_never_attempted.rs"]mod package_never_attempted;
use moloch_live_runtime::feedback;
use package_dispatch::{DurableIntent,PackageDispatchLog};
use baal_transport_service::{Scope,Permit,TransportError,multi_leg::{self,Basket,Leg,Side}};
use moloch_replay::session::Session;
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::{fs,path::PathBuf,io::Write};
const T:&str="11111111-1111-4111-8111-111111111111";
fn iso(n:i64)->String{chrono::DateTime::from_timestamp_millis(1_792_022_400_000+n).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis,true)}
fn hash<V:serde::Serialize>(v:&V)->String{format!("{:x}",Sha256::digest(serde_json::to_vec(v).unwrap()))}
fn book(ex:&str)->Value{json!({"type":"book","receivedAt":iso(0),"book":{"tournamentId":T,"marketId":ex,"exchangeId":ex,"sequence":"1","sourceAt":iso(0),"requestStartedAt":iso(0),"receivedAt":iso(0),"bids":[{"price":"0.400000","quantity":20}],"asks":[{"price":"0.450000","quantity":20}]}})}
struct Fixture{bank:Session,root:PathBuf,policy:Value,seed:Value,proof:Value,reserve:Value,scope:Scope,basket:Basket,permit:Permit,n:u64}
impl Fixture{
 fn new(attempted:bool)->Self{
  let root=std::env::temp_dir().join(format!("package-never-attempted-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
  let policy=json!({"initialCash":"100.000000","maximumCommitted":"40.000000","maximumMarketCost":"10.000000","quantity":15,"maximumMarkets":3,"minimumSpreadTicks":1000,"entryStopAt":iso(100000)});
  let seed=json!({"runId":"run-1","availableCash":"100.000000","accountSnapshotHash":"a".repeat(64),"observedAt":iso(0)});
  let mut bank=Session::open_live(&root.join("bank"),&policy,&seed).unwrap();for(i,ex)in ["101","102"].into_iter().enumerate(){bank.accept("books",i as u64+1,&book(ex)).unwrap();}
  let scope=Scope{tournament_id:T.into(),tournament_slug:"cup".into(),exchanges:[("101".into(),"101".into()),("102".into(),"102".into())].into()};
  let legs=json!([{"marketId":"101","exchangeId":"101","side":"NO","quantity":5,"limitPrice":"0.600000","bookHash":hash(&book("101")["book"])},{"marketId":"102","exchangeId":"102","side":"YES","quantity":5,"limitPrice":"0.450000","bookHash":hash(&book("102")["book"])}]);
  let proof=json!({"runId":"run-1","bankManifestHash":bank.manifest_hash(),"tournamentId":T,"atMs":1_792_022_400_001i64,"expiresAtMs":1_792_022_409_000i64,"maximumCost":"5.250000","wireLegs":legs,"usesFutureCollateral":false});
  let digest=hash(&proof);let id=format!("pkg-{digest}");
  let basket=Basket{idempotency_key:format!("run-1:{id}"),tournament_id:T.into(),maximum_cost:"5.250000".into(),relationship_constraint:None,legs:vec![Leg{exchange_id:"101".into(),side:Side::No,quantity:5,limit_price:"0.600000".into()},Leg{exchange_id:"102".into(),side:Side::Yes,quantity:5,limit_price:"0.450000".into()}]};
  let permit=Permit{run_id:"run-1".into(),intent_id:id.clone(),owner:"Codex".into(),expires_at_ms:1_792_022_409_000u64,maximum_quantity:10,maximum_cost:"5.250000".into()};
  let reserve=json!({"type":"live_package_reserve","source":"moloch_live_dispatcher","receivedAt":iso(1),"processedAt":iso(1),"packageId":id,"proofHash":digest,"bankScope":{"runId":"run-1","bankManifestHash":bank.manifest_hash(),"tournamentId":T},"expiresAt":iso(9000),"economics":{"maximumCost":"5.250000","minimumPayout":"6.000000","minimumSurplus":"0.750000","feeTotal":"0.000000"},"legs":legs});
  bank.accept("dispatch",1,&reserve).unwrap();let mut f=Self{bank,root,policy,seed,proof,reserve,scope,basket,permit,n:1};
  if attempted{let mut a=f.envelope("live_package_attempt",3001);a["requestHash"]=json!(hash(&multi_leg::request_spec(&f.basket,&f.scope,&f.permit).unwrap()));a["idempotencyKey"]=json!(f.basket.idempotency_key);a["bookHashes"]=json!({"101":hash(&book("101")["book"]),"102":hash(&book("102")["book"])});f.accept(&a);}f
 }
 fn p(&self)->Value{self.bank.engine().package_records()[0].clone()}
 fn envelope(&self,kind:&str,at:i64)->Value{let mut e=self.reserve.clone();e["type"]=json!(kind);e["receivedAt"]=json!(iso(at));e["processedAt"]=json!(iso(at));e}
 fn intent(&self)->DurableIntent{DurableIntent::new(self.proof.clone(),self.reserve.clone(),self.basket.clone(),self.scope.clone(),self.permit.clone(),"original-account").unwrap()}
 fn log(&self)->PackageDispatchLog{PackageDispatchLog::open(&self.root.join("dispatch")).unwrap()}
 fn accept(&mut self,e:&Value){self.n+=1;self.bank.accept("dispatch",self.n,e).unwrap();}
 fn reopen(self)->Self{let Self{bank,root,policy,seed,proof,reserve,scope,basket,permit,n}=self;drop(bank);let bank=Session::open_live(&root.join("bank"),&policy,&seed).unwrap();Self{bank,root,policy,seed,proof,reserve,scope,basket,permit,n}}
 fn finish(self){let root=self.root.clone();drop(self);fs::remove_dir_all(root).unwrap();}
}
#[test]fn durable_intent_fence_precedes_attempted_native_release_and_prevents_all_future_tokens(){
 let mut f=Fixture::new(true);let mut log=f.log();let id=f.permit.intent_id.clone();log.intent(f.intent(),&iso(1)).unwrap();
 assert!(package_never_attempted::release(&f.bank,&f.p(),&log,&iso(3100)).is_err());
 log.abandon_before_attempt(&id,&iso(3100)).unwrap();assert_eq!(log.record(&id).unwrap()["phase"],"never_attempted");assert!(log.attempt(&id,&iso(3101)).is_err());
 let e=package_never_attempted::release(&f.bank,&f.p(),&log,&iso(3101)).unwrap();assert_eq!(e["evidence"]["kind"],"dispatch_never_attempted_v1");
 let original=f.bank.engine().report().unwrap();f.accept(&e);let after=f.bank.engine().report().unwrap();assert_eq!(after["reservedCash"],"0.000000");for k in ["cash","realized","ownedCost","fees"]{assert_eq!(after[k],original[k]);}assert!(f.bank.engine().fills().is_empty()&&f.bank.engine().live_order_bindings().is_empty());assert!(package_never_attempted::release(&f.bank,&f.p(),&log,&iso(3200)).is_err());drop(log);f.finish();
}
#[test]fn restart_after_durable_fence_before_bank_release_is_idempotently_recoverable(){
 let mut f=Fixture::new(true);let id=f.permit.intent_id.clone();{let mut log=f.log();log.intent(f.intent(),&iso(1)).unwrap();log.abandon_before_attempt(&id,&iso(3100)).unwrap();}
 f=f.reopen();let mut log=f.log();let head=log.head();log.abandon_before_attempt(&id,&iso(3200)).unwrap();assert_eq!(log.head(),head);
 let event=package_never_attempted::release(&f.bank,&f.p(),&log,&iso(3201)).unwrap();f.accept(&event);let expected=f.bank.engine().report().unwrap();drop(log);f=f.reopen();assert_eq!(f.bank.engine().report().unwrap(),expected);assert_eq!(f.p()["phase"],"no_execution");f.finish();
}
#[test]fn reserved_native_uses_existing_route_only_after_same_irrevocable_source_fence(){
 let mut f=Fixture::new(false);let mut log=f.log();let id=f.permit.intent_id.clone();log.intent(f.intent(),&iso(1)).unwrap();assert!(package_never_attempted::release(&f.bank,&f.p(),&log,&iso(10)).is_err());log.abandon_before_attempt(&id,&iso(10)).unwrap();let e=package_never_attempted::release(&f.bank,&f.p(),&log,&iso(11)).unwrap();assert_eq!(e["evidence"],json!({"kind":"never_attempted","confirmedNetworkAttempts":0}));assert_eq!(e["neverAttemptedDispatchEvidence"]["dispatchRecord"]["phase"],"never_attempted");f.accept(&e);assert_eq!(f.p()["phase"],"no_execution");assert!(log.attempt(&id,&iso(12)).is_err());drop(log);f.finish();
}
#[test]fn any_physical_attempt_or_unknown_remains_blocked_even_without_queue_activity(){
 let f=Fixture::new(true);let mut log=f.log();let id=f.permit.intent_id.clone();log.intent(f.intent(),&iso(1)).unwrap();let _token=log.attempt(&id,&iso(3001)).unwrap();assert!(log.abandon_before_attempt(&id,&iso(3100)).is_err());assert!(package_never_attempted::release(&f.bank,&f.p(),&log,&iso(3101)).is_err());let error=TransportError{code:"TRANSPORT_ERROR".into(),status:None,may_have_executed:true,retry_after_ms:0,order_id:None,api_code:None};log.unknown(&id,&error,&iso(3200)).unwrap();assert!(log.abandon_before_attempt(&id,&iso(3300)).is_err());assert!(package_never_attempted::release(&f.bank,&f.p(),&log,&iso(3301)).is_err());assert_eq!(f.bank.engine().report().unwrap()["reservedCash"],"5.250000");drop(log);f.finish();
}
#[test]fn native_unknown_ack_and_changed_caller_record_cannot_use_a_fenced_dispatch(){
 for case in ["unknown","ack","record"]{let mut f=Fixture::new(true);let mut log=f.log();let id=f.permit.intent_id.clone();log.intent(f.intent(),&iso(1)).unwrap();log.abandon_before_attempt(&id,&iso(3100)).unwrap();
  if case=="unknown"{let mut e=f.envelope("live_package_unknown",3002);e["requestHash"]=f.p()["requestHash"].clone();f.accept(&e);}else if case=="ack"{let q=f.bank.engine().order_records()[0].clone();let e=json!({"type":"actual_order_ack","source":"official_sig_api_v1","receiptHash":"e".repeat(64),"orderId":q["id"],"actualOrderId":"901","marketId":q["marketId"],"exchangeId":q["exchangeId"],"side":q["side"],"action":"buy","quantity":q["quantity"],"price":q["price"],"receivedAt":iso(3002)});f.accept(&e);}
  let mut p=f.p();if case=="record"{p["bankScope"]["bankManifestHash"]=json!("f".repeat(64));}assert!(package_never_attempted::release(&f.bank,&p,&log,&iso(3200)).is_err());assert_eq!(f.bank.engine().report().unwrap()["reservedCash"],"5.250000");drop(log);f.finish();
 }
}
#[test]fn replaced_same_bytes_path_and_torn_original_source_never_mint_release_proof(){
 for case in ["replacement","torn","changed"]{let f=Fixture::new(true);let mut log=f.log();let id=f.permit.intent_id.clone();log.intent(f.intent(),&iso(1)).unwrap();log.abandon_before_attempt(&id,&iso(3100)).unwrap();let path=f.root.join("dispatch/package-dispatch.ndjson");
  match case{"replacement"=>{let original=fs::read(&path).unwrap();fs::rename(&path,path.with_extension("retained-original")).unwrap();fs::write(&path,original).unwrap();},"torn"=>{fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(b"{").unwrap();},_=>{let s=fs::read_to_string(&path).unwrap();fs::write(&path,s.replace("original-account","changed-account")).unwrap();}}
  assert!(package_never_attempted::release(&f.bank,&f.p(),&log,&iso(3200)).is_err());assert!(log.abandon_before_attempt(&id,&iso(3200)).is_err());assert_eq!(f.bank.engine().report().unwrap()["reservedCash"],"5.250000");drop(log);f.finish();
 }
}
#[test]fn failure_after_fence_write_never_releases_in_poisoned_process_but_checked_restart_can_recover(){
 let f=Fixture::new(true);let id=f.permit.intent_id.clone();{let mut log=f.log();log.intent(f.intent(),&iso(1)).unwrap();log.inject_sync_failure();assert!(log.abandon_before_attempt(&id,&iso(3100)).is_err());assert_eq!(log.head()["poisoned"],true);assert!(package_never_attempted::release(&f.bank,&f.p(),&log,&iso(3200)).is_err());}
 let log=f.log();let e=package_never_attempted::release(&f.bank,&f.p(),&log,&iso(3300)).unwrap();assert_eq!(e["evidence"]["dispatchRecord"]["phase"],"never_attempted");assert_eq!(f.bank.engine().report().unwrap()["reservedCash"],"5.250000");drop(log);f.finish();
}
#[test]fn absent_original_intent_and_release_clock_regression_cannot_free_funding(){
 let f=Fixture::new(true);let mut log=f.log();let id=f.permit.intent_id.clone();assert!(package_never_attempted::release(&f.bank,&f.p(),&log,&iso(3100)).is_err());log.intent(f.intent(),&iso(1)).unwrap();log.abandon_before_attempt(&id,&iso(3100)).unwrap();assert!(package_never_attempted::release(&f.bank,&f.p(),&log,&iso(3099)).is_err());assert!(package_never_attempted::release(&f.bank,&f.p(),&log,&iso(11000)).is_ok());drop(log);f.finish();
}
