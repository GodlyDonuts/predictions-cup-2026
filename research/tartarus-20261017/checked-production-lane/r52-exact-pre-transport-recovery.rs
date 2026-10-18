//! File-only recovery for the original 2026-10-18 q19504 incident.
//! No Session, account client, credentials, dispatch retry or STOP removal.
use std::{fs,path::PathBuf};
use serde_json::{json,Value};
use baal_transport_service::{self as api,Config,Permit,RequestSpec,TransportError};
use moloch_live_runtime::{dispatch::DispatchLog,daily_windows::{clock,iso}};
use moloch_replay::session::ExclusiveLock;
use sha2::{Sha256,Digest};
fn hash(b:&[u8])->String{format!("{:x}",Sha256::digest(b))}
fn main()->Result<(),Box<dyn std::error::Error>>{
 let args:Vec<_>=std::env::args().collect();assert_eq!(args.len(),4);
 let cfg:Value=serde_json::from_slice(&fs::read(&args[1])?)?;
 let root=PathBuf::from(cfg["root"].as_str().unwrap());let stop=root.join("EXECUTION_STOP");let stopped=fs::read(&stop)?;
 assert_eq!(hash(&stopped),args[2]);let incident:Value=serde_json::from_slice(&stopped)?;
 assert_eq!(incident["reason"],"FUNDAMENTAL_ABANDON_DISPATCH_ROW_BOUND");
 let _owner=ExclusiveLock::acquire(&PathBuf::from(cfg["ownerLock"].as_str().unwrap()))?;
 let config=Config{queue_dir:root.join("account-queue"),account_scope:cfg["accountScope"].as_str().unwrap().into(),scope:serde_json::from_value(cfg["scope"].clone())?,maximum_pages:8,writes_enabled:false};
 let mut log=DispatchLog::open(&root.join("dispatch.ndjson"))?;
 // Do not restore unrelated historical trust in this scoped helper. Their
 // serialized records are classification only; normal startup independently
 // revalidates every original proof before it can dispatch anything.
 let unresolved:Vec<_>=log.attempted.iter().filter(|id|!log.acknowledgments.contains_key(id)&&!log.aborted_records().contains_key(id)&&!log.local_final_guard_records().contains_key(id)).copied().collect();
 assert_eq!(unresolved,vec![18533,19504]);
 // Historical18533 stays untrusted here. Normal startup must independently
 // restore it against its original committed native event and receipt file.
 assert_eq!(log.exposure_records().len(),1);assert_eq!(log.exposure_records()[&18533]["proofFileSha256"],"16e11d840f0d9709a9b9a1feec8defd30e6b7bbf44d38dd6004ff9421478fb99");
 let original=&log.intents[&19504];let spec:RequestSpec=serde_json::from_value(original["spec"].clone())?;
 let permit:Permit=serde_json::from_value(original["permit"].clone())?;
 let attempt=log.original_attempt(19504).unwrap();let at=clock(attempt["at"].as_str().unwrap())?;
 assert_eq!(at,1792360800665);assert_eq!(attempt["captureAtMs"],at);assert_eq!(clock(incident["at"].as_str().unwrap())?,1792360802549);
 assert!(log.original_error(19504).is_none());assert_eq!(permit.run_id,cfg["activation"]["runId"]);
 let error=TransportError{code:"FUNDAMENTAL_ABANDON_DISPATCH_ROW_BOUND".into(),status:None,may_have_executed:false,retry_after_ms:0,order_id:None,api_code:None};
 let proof=api::verify_frozen_ordinary_pre_admission(&config,&spec,&permit,at,&error)?;
 assert_eq!(fs::read(&stop)?,stopped);let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis()as u64;
 log.append(json!({"kind":"error","qid":19504,"at":iso(now)?,"error":error,"recoveredFromStoppedPreTransportException":true,"originalStop":incident,"originalStopSha256":args[2],"historicalSdkErrorReceipt":false}))?;
 log.append_aborted_before_admission(19504,&proof,&iso(now)?)?;
 assert!(log.proved_no_network(19504)&&!log.never_attempted(19504));assert!(!log.proved_no_network(18533));assert!(log.require_resolved_attempts().is_err());assert_eq!(fs::read(&stop)?,stopped);
 let result=json!({"at":iso(now)?,"recoveredOriginalQids":[19504],"networkRequests":0,"stopPreserved":true,"sdkArchiveReverified":true,"proof":proof.evidence(),"dispatchHead":log.descriptor_identity()?,"sameIntentRetryAuthorized":false,"financialStateChanged":false,"allOtherOriginalProofsMustBeReverifiedByNormalStartup":true,"historical18533NativeRestorationStillRequired":true,"allDispatchAttemptsResolvedClaimed":false});
 fs::write(&args[3],serde_json::to_vec_pretty(&result)?)?;fs::File::open(&args[3])?.sync_all()?;println!("{}",result);Ok(())
}
