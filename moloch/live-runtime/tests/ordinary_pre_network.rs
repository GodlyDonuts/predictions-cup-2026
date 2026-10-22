use baal_transport_service::{admission::AccountAdmission,Scope,RequestSpec,Permit,TransportError};
use moloch_live_runtime::{ordinary_pre_network as proof,dispatch::DispatchLog};
use serde_json::{json,Value};use sha2::{Digest,Sha256};use std::{fs,path::PathBuf};
fn fixture(network:bool)->(PathBuf,Scope,RequestSpec,Permit,TransportError){
 let dir=std::env::temp_dir().join(format!("ordinary-pre-network-test-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));fs::create_dir(&dir).unwrap();
 let scope=Scope{tournament_id:"cup".into(),tournament_slug:"cup".into(),exchanges:[("101".into(),"201".into())].into()};
 let mut body=json!({"tournamentId":"cup","exchangeId":"101","side":"yes","action":"buy","quantity":1,"price":"0.500000","idempotencyKey":"stable-key","expirationDate":"1970-01-01T00:00:30.000Z"});
 let spec=RequestSpec{method:"POST".into(),endpoint:"/orders".into(),body_json:Some(body.to_string()),body:Some(body.clone())};
 body["price"]=json!(serde_json::Number::from_string_unchecked("0.500000".into()));let actual=RequestSpec{method:"POST".into(),endpoint:"/orders".into(),body_json:Some(body.to_string()),body:Some(body)};
 let binding=json!({"intent":"run:1","key":"stable-key","payload_hash":format!("{:x}",Sha256::digest(serde_json::to_vec(&json!({"scope":scope,"spec":actual})).unwrap()))});
 {let mut q=AccountAdmission::open(&dir,"fixture-account","owner").unwrap();q.command(json!({"op":"compact","scope":"fixture-account","now":1})).unwrap();
 q.command(json!({"op":"admit","scope":"fixture-account","now":10000,"request":"actual-request","write":true,"binding":binding,"authorized":true,"lease_valid":true,"permit_valid":true})).unwrap();
 if network{q.command(json!({"op":"network","scope":"fixture-account","now":10001,"request":"actual-request","write":true,"binding":binding,"authorized":true,"lease_valid":true,"permit_valid":true})).unwrap();}
 q.command(json!({"op":"complete","scope":"fixture-account","now":10002,"request":"actual-request","outcome":if network{"rejected"}else{"not_sent"}})).unwrap();}
 let p=Permit{run_id:"run".into(),intent_id:"1".into(),owner:"Codex".into(),expires_at_ms:30000,maximum_quantity:1,maximum_cost:"0.500000".into()};let e=TransportError{code:"FUNDAMENTAL_PHYSICAL_TOKEN_EXPIRED_OR_REGRESSED".into(),status:None,may_have_executed:false,retry_after_ms:0,order_id:None,api_code:None};(dir,scope,spec,p,e)
}
#[test]fn actual_admitted_completion_survives_compaction_without_same_key_retry(){let(dir,scope,spec,p,e)=fixture(false);
 let positive=proof::capture(&dir,"fixture-account",&scope,&spec,&p,9999,&e).unwrap();let retained=positive.evidence().clone();
 let mut log=DispatchLog::open(&dir.join("dispatch.ndjson")).unwrap();for row in [json!({"kind":"intent","qid":1,"spec":spec,"permit":p}),json!({"kind":"attempt","qid":1,"at":"1970-01-01T00:00:09.999Z","captureAtMs":9999}),json!({"kind":"error","qid":1,"at":"1970-01-01T00:00:10.003Z","error":e})]{log.append(row).unwrap();}
 assert!(log.append(json!({"kind":"admitted_pre_network_not_sent","qid":1})).is_err());log.append_pre_network_not_sent(1,&positive,"1970-01-01T00:00:10.004Z").unwrap();log.require_resolved_attempts().unwrap();assert!(!log.never_attempted(1));drop(log);
 {let mut q=AccountAdmission::open(&dir,"fixture-account","new-owner").unwrap();q.command(json!({"op":"compact","scope":"fixture-account","now":20000})).unwrap();}
 let mut log=DispatchLog::open(&dir.join("dispatch.ndjson")).unwrap();assert!(log.require_resolved_attempts().is_err());let again=proof::restore("fixture-account",&scope,&spec,&p,9999,&e,&retained).unwrap();log.verify_pre_network_not_sent(1,&again).unwrap();log.require_resolved_attempts().unwrap();assert!(!log.never_attempted(1));
 let mut changed=spec.clone();changed.body.as_mut().unwrap()["quantity"]=json!(2);assert!(proof::restore("fixture-account",&scope,&changed,&p,9999,&e,&retained).is_err());
 let snapshot=PathBuf::from(retained["snapshot"].as_str().unwrap());let m:Value=serde_json::from_slice(&fs::read(snapshot.join("account-queue-manifest.json")).unwrap()).unwrap();fs::write(snapshot.join(m["journal"].as_str().unwrap()),b"corrupt").unwrap();assert!(proof::restore("fixture-account",&scope,&spec,&p,9999,&e,&retained).is_err());drop(log);fs::remove_dir_all(dir).unwrap();}
#[test]fn actual_network_marker_or_wrong_error_cannot_be_declared_unsent(){let(dir,scope,spec,p,e)=fixture(true);assert!(proof::capture(&dir,"fixture-account",&scope,&spec,&p,9999,&e).is_err());fs::remove_dir_all(dir).unwrap();let(dir,scope,spec,p,mut e)=fixture(false);e.may_have_executed=true;assert!(proof::capture(&dir,"fixture-account",&scope,&spec,&p,9999,&e).is_err());fs::remove_dir_all(dir).unwrap();}
