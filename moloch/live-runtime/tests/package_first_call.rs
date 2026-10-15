use moloch_live_runtime::package_dispatch::{DurableIntent,PackageDispatchLog};
use baal_transport_service::{Scope,Permit,Receipt,TransportError,multi_leg::{Basket,Leg,Side,ResponseEvidence}};
use serde_json::{Value,json};use sha2::{Digest,Sha256};
use std::{path::PathBuf,fs};
const T:&str="11111111-1111-4111-8111-111111111111";
fn iso(n:i64)->String{chrono::DateTime::from_timestamp_millis(1_792_022_400_000i64+n).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis,true)}
fn hash<T:serde::Serialize>(v:&T)->String{format!("{:x}",Sha256::digest(serde_json::to_vec(v).unwrap()))}
fn path()->PathBuf{let p=std::env::temp_dir().join(format!("package-first-call-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));fs::create_dir(&p).unwrap();p}
fn intent(n:u64)->DurableIntent{
 let legs=json!([{"exchangeId":"101","marketId":"101","side":"NO","quantity":5,"limitPrice":"0.600000","bookHash":"a".repeat(64)},{"exchangeId":"102","marketId":"102","side":"YES","quantity":5,"limitPrice":"0.450000","bookHash":"b".repeat(64)}]);
 let proof=json!({"nonce":n,"runId":"run-1","bankManifestHash":"c".repeat(64),"tournamentId":T,"atMs":1_792_022_400_001i64,"expiresAtMs":1_792_022_409_000i64,"maximumCost":"5.250000","usesFutureCollateral":false,"wireLegs":legs});let digest=hash(&proof);let id=format!("pkg-{digest}");
 let reserve=json!({"type":"live_package_reserve","source":"moloch_live_dispatcher","packageId":id,"proofHash":digest,"bankScope":{"runId":"run-1","bankManifestHash":"c".repeat(64),"tournamentId":T},"receivedAt":iso(1),"expiresAt":iso(9000),"economics":{"maximumCost":"5.250000"},"legs":legs});
 let scope=Scope{tournament_id:T.into(),tournament_slug:"cup".into(),exchanges:[("101".into(),"101".into()),("102".into(),"102".into())].into()};
 let basket=Basket{idempotency_key:format!("run-1:{id}"),tournament_id:T.into(),maximum_cost:"5.250000".into(),relationship_constraint:None,legs:vec![Leg{exchange_id:"101".into(),side:Side::No,quantity:5,limit_price:"0.600000".into()},Leg{exchange_id:"102".into(),side:Side::Yes,quantity:5,limit_price:"0.450000".into()}]};
 let permit=Permit{run_id:"run-1".into(),intent_id:id,owner:"Codex".into(),expires_at_ms:1_792_022_409_000u64,maximum_quantity:10,maximum_cost:"5.250000".into()};DurableIntent::new(proof,reserve,basket,scope,permit,"account-scope").unwrap()
}

use baal_transport_service::{Transport,Config,Clock,DisabledCredentials,ReadOnlyAuthority,admission::ImmutableBinding,multi_leg_first_call::Proof};
struct Fixed(u64);impl Clock for Fixed{fn now_ms(&self)->u64{self.0}fn sleep_ms(&mut self,_:u64)->baal_transport_service::Result<()>{panic!("no admission wait expected")}}
fn sdk(root:&std::path::Path,i:&DurableIntent)->Transport{let v=serde_json::to_value(i).unwrap();Transport::new(Config{queue_dir:root.join("actual-sdk-queue"),account_scope:"account-scope".into(),scope:serde_json::from_value(v["scope"].clone()).unwrap(),maximum_pages:2,writes_enabled:true},Box::new(DisabledCredentials),Box::new(Fixed(1_792_022_403_010u64)),Box::new(ReadOnlyAuthority)).unwrap()}
fn original(t:&mut Transport,i:&DurableIntent)->(TransportError,Proof){let v=serde_json::to_value(i).unwrap();let e=t.place_all_or_none(i.basket().unwrap().clone(),serde_json::from_value(v["permit"].clone()).unwrap()).err().unwrap();assert_eq!(e.code,"WRITES_DISABLED");let proof=t.take_first_call_no_http_proof().unwrap();assert_eq!(t.admission_summary().unwrap().sequence,0);(e,proof)}
#[test]fn original_sdk_opaque_proof_and_token_create_distinct_durable_outcome(){let p=path();let i=intent(1);let id=i.package_id().to_owned();let expected;{
 let mut l=PackageDispatchLog::open(&p.join("dispatch")).unwrap();l.intent(i.clone(),&iso(1)).unwrap();let token=l.attempt(&id,&iso(3001)).unwrap();let mut t=sdk(&p,&i);let(e,proof)=original(&mut t,&i);l.first_call_not_sent(token,&e,&proof,&iso(3020)).unwrap();assert!(!l.unresolved());let row=l.verified_record(&id).unwrap();assert_eq!(row["phase"],"first_call_not_sent");assert_eq!(row["outcome"]["firstCallEvidence"],*proof.evidence());assert_eq!(row["outcome"]["error"],serde_json::to_value(e).unwrap());assert!(row["outcome"].get("queueEvidence").is_none());expected=row;
 }{let l=PackageDispatchLog::open(&p.join("dispatch")).unwrap();assert_eq!(l.verified_record(&id).unwrap(),expected);assert!(!l.unresolved());}fs::remove_dir_all(p).unwrap();}
#[test]fn changed_error_wrong_package_or_premature_clock_cannot_use_genuine_proof(){for mode in 0..3{let p=path();let i=intent(1);let id=i.package_id().to_owned();{
 let mut l=PackageDispatchLog::open(&p.join("dispatch")).unwrap();l.intent(i.clone(),&iso(1)).unwrap();let token=l.attempt(&id,&iso(3001)).unwrap();let mut t=sdk(&p,&i);let(mut e,proof)=original(&mut t,&i);let at=if mode==2{iso(3005)}else{iso(3020)};if mode==0{e.code="different-original-error".into();}if mode==1{e.may_have_executed=true;}assert!(l.first_call_not_sent(token,&e,&proof,&at).is_err());assert!(l.unresolved());assert_eq!(l.verified_record(&id).unwrap()["phase"],"attempt");
 }fs::remove_dir_all(p).unwrap();}}
#[test]fn unknown_and_foreign_attempt_token_never_release_with_first_call_proof(){for mode in 0..2{let p=path();let i=intent(1);let id=i.package_id().to_owned();{
 let mut l=PackageDispatchLog::open(&p.join("dispatch")).unwrap();l.intent(i.clone(),&iso(1)).unwrap();let mut token=l.attempt(&id,&iso(3001)).unwrap();let mut t=sdk(&p,&i);let(e,proof)=original(&mut t,&i);
 if mode==0{let mut uncertain=e.clone();uncertain.may_have_executed=true;l.unknown(&id,&uncertain,&iso(3015)).unwrap();}else{let mut other=PackageDispatchLog::open(&p.join("foreign-dispatch")).unwrap();other.intent(i.clone(),&iso(1)).unwrap();token=other.attempt(&id,&iso(3001)).unwrap();}
 assert!(l.first_call_not_sent(token,&e,&proof,&iso(3020)).is_err());assert!(l.unresolved());
 }fs::remove_dir_all(p).unwrap();}}
#[test]fn genuine_proof_for_another_package_cannot_close_this_attempt(){let p=path();let i=intent(1);let other=intent(2);let id=i.package_id().to_owned();{
 let mut l=PackageDispatchLog::open(&p.join("dispatch")).unwrap();l.intent(i.clone(),&iso(1)).unwrap();let token=l.attempt(&id,&iso(3001)).unwrap();let mut t=sdk(&p,&other);let(e,proof)=original(&mut t,&other);assert!(l.first_call_not_sent(token,&e,&proof,&iso(3020)).is_err());assert!(l.unresolved());
 }fs::remove_dir_all(p).unwrap();}
#[test]fn durable_attempt_plus_sdk_terminal_alone_remains_unresolved_after_restart(){let p=path();let i=intent(1);let id=i.package_id().to_owned();{
 let mut l=PackageDispatchLog::open(&p.join("dispatch")).unwrap();l.intent(i.clone(),&iso(1)).unwrap();let _lost=l.attempt(&id,&iso(3001)).unwrap();let mut t=sdk(&p,&i);let _original=original(&mut t,&i);
 }{let l=PackageDispatchLog::open(&p.join("dispatch")).unwrap();assert!(l.unresolved());assert_eq!(l.verified_record(&id).unwrap()["phase"],"attempt");let t=sdk(&p,&i);let b:ImmutableBinding=serde_json::from_value(i.queue_binding().clone()).unwrap();assert!(t.verify_retained_first_call_no_http(&b,i.request_hash()).is_ok());/* This capability still supplies no missing durable dispatch outcome. */}fs::remove_dir_all(p).unwrap();}
