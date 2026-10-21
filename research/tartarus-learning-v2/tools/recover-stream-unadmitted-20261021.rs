//! Exact stopped R65 incident: append only the SDK's typed proof for q20155.
//! No account client, credentials, native replay, order retry or STOP mutation.
use std::{env,fs,path::Path};
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use baal_transport_service as api;
use moloch_live_runtime::dispatch::DispatchLog;
fn hash(b:&[u8])->String{format!("{:x}",Sha256::digest(b))}
fn main()->Result<(),Box<dyn std::error::Error>>{
 let a:Vec<_>=env::args().collect();assert_eq!(a.len(),4);
 let control=fs::read(&a[1])?;let c:Value=serde_json::from_slice(&control)?;
 let root=Path::new(c["root"].as_str().ok_or("root")?);
 assert_eq!(root,Path::new("/home/archon/apps/moloch/shared/live-production-v1"));
 let stopped=fs::read(root.join("EXECUTION_STOP"))?;assert_eq!(hash(&stopped),a[2]);
 let incident:Value=serde_json::from_slice(&stopped)?;
 assert_eq!(incident["reason"],"LIVE_WRITE_NOT_CONFIRMED_NO_RETRY");
 assert_eq!(incident["at"],"2026-10-21T14:10:10.545Z");
 let path=root.join("dispatch.ndjson");let original=fs::read(&path)?;assert_eq!(hash(&original),a[3]);
 let mut log=DispatchLog::open(&path)?;let qid=20155;
 assert!(!log.acknowledgments.contains_key(&qid)&&!log.aborted_records().contains_key(&qid));
 let(spec,permit,at,error)=log.pre_admission_originals(qid)?;
 assert_eq!(at,1792591806956);assert_eq!(permit.intent_id,"20155");
 assert_eq!(permit.run_id,"moloch-live-20261015-v1");assert_eq!(error.code,"LIVE_VERIFIED_STREAM_REQUIRED");
 assert!(!error.may_have_executed);let body=spec.body.as_ref().ok_or("body")?;
 assert_eq!(body["exchangeId"],"1077");assert_eq!(body["quantity"],9);
 assert_eq!(body["action"],"sell");assert_eq!(body["side"],"no");
 let config=api::Config{queue_dir:root.join("account-queue"),account_scope:c["accountScope"].as_str().ok_or("scope")?.into(),scope:serde_json::from_value(c["scope"].clone())?,maximum_pages:20,writes_enabled:false};
 let proof=api::verify_frozen_ordinary_pre_admission(&config,&spec,&permit,at,&error)?;
 assert_eq!(fs::read(&a[1])?,control);assert_eq!(fs::read(root.join("EXECUTION_STOP"))?,stopped);assert_eq!(fs::read(&path)?,original);
 let now:i64=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis().try_into()?;
 let stamp=chrono::DateTime::<chrono::Utc>::from_timestamp_millis(now).ok_or("clock")?.to_rfc3339_opts(chrono::SecondsFormat::Millis,true);
 log.append_aborted_before_admission(qid,&proof,&stamp)?;
 assert!(log.proved_no_network(qid));assert!(!log.never_attempted(qid));
 assert_eq!(fs::read(root.join("EXECUTION_STOP"))?,stopped);
 println!("{}",json!({"qid":qid,"typedSdkProof":proof.evidence(),"originalDispatchFileSha256":a[3],"stopSha256":a[2],"stopPreserved":true,"sameIntentRetryAuthorized":false,"networkRequests":0,"nativeStateChanged":false,"otherHistoricalRecordsReclassified":false}));Ok(())
}
