//! File-only SDK verification for replacing a completed review, never a retry.
use std::{fs,path::Path};
use serde_json::{Value,json};
use sha2::{Digest,Sha256};

fn main()->Result<(),Box<dyn std::error::Error>>{
 let args:Vec<String>=std::env::args().collect();
 if args.len()!=5{return Err("usage: verifier CONTROL DISPATCH_COPY QID PRIOR_REVIEW_SHA".into())}
 let control:Value=serde_json::from_slice(&fs::read(&args[1])?)?;
 let scope:baal_transport_service::Scope=serde_json::from_value(control["scope"].clone())?;
 let account=control["accountScope"].as_str().ok_or("PRIOR_ACCOUNT_SCOPE")?;
 let qid:u64=args[3].parse()?;let bytes=fs::read(&args[2])?;
 let mut log=moloch_live_runtime::dispatch::DispatchLog::open(Path::new(&args[2]))?;
 if log.intents.get(&qid).ok_or("PRIOR_INTENT_REQUIRED")?["rankCampaign"]["rawSha256"]!=args[4]
  ||log.acknowledgments.contains_key(&qid)||log.recovered.contains_key(&qid)||log.closed.contains(&qid)||log.cancel_attempted(qid){return Err("PRIOR_REVIEW_NOT_A_MATCHING_UNSENT_INTENT".into())}
 let(kind,evidence)=if let Some(record)=log.pre_network_records().get(&qid).cloned(){
  let(spec,permit,at,error)=log.pre_admission_originals(qid)?;
  let proof=moloch_live_runtime::ordinary_pre_network::restore(account,&scope,&spec,&permit,at,&error,&record["proof"])?;
  log.verify_pre_network_not_sent(qid,&proof)?;
  ("positively_proved_pre_network_not_sent",proof.evidence().clone())
 }else if let Some(record)=log.aborted_records().get(&qid).cloned(){
  let(spec,permit,at,error)=log.pre_admission_originals(qid)?;
  let config=baal_transport_service::Config{queue_dir:Path::new(control["root"].as_str().ok_or("PRIOR_RUNTIME_ROOT")?).join("account-queue"),account_scope:account.into(),scope:scope.clone(),maximum_pages:20,writes_enabled:false};
  let proof=baal_transport_service::verify_retained_ordinary_pre_admission(&config,&spec,&permit,at,&error,&record["proof"])?;
  log.verify_aborted_before_admission(qid,&proof)?;
  let original=proof.evidence();
  ("positively_proved_aborted_before_admission",json!({"kind":original["kind"],"contextSha256":original["contextSha256"],"sdkSourceSha256":original["sdkSourceSha256"]}))
 }else{
  let record=log.local_final_guard_records().get(&qid).ok_or("PRIOR_TYPED_NOT_SENT_RECORD_REQUIRED")?.clone();
  if record["proof"]["proofMode"]!="durable_local_guard"{return Err("PRIOR_DURABLE_SDK_CERTIFICATE_REQUIRED".into())}
  let(spec,permit,at,error)=log.local_final_guard_originals(qid)?;
  let config=baal_transport_service::Config{queue_dir:Path::new(control["root"].as_str().ok_or("PRIOR_RUNTIME_ROOT")?).join("account-queue"),account_scope:account.into(),scope:scope.clone(),maximum_pages:20,writes_enabled:false};
  let proof=baal_transport_service::verify_retained_ordinary_local_guard_no_http(&config,&spec,&permit,at,&error,&record["proof"])?;
  log.verify_local_final_guard_no_http(qid,&proof)?;
  let original=proof.evidence()?;
  ("positively_proved_local_final_guard_no_http",json!({"kind":original["kind"],"contextSha256":original["contextSha256"],"sdkSourceSha256":original["sdkSourceSha256"],"proofMode":original["proofMode"]}))
 };
 if !log.proved_no_network(qid)||fs::read(&args[2])?!=bytes{return Err("PRIOR_VERIFIED_SNAPSHOT_CHANGED".into())}
 println!("{}",json!({"qid":qid,"priorRawSha256":args[4],"kind":kind,
  "dispatchSnapshotSha256":format!("{:x}",Sha256::digest(&bytes)),"opaqueSdkProofVerified":true,
  "originalProof":evidence,"exchangeClosedClaimed":false,"retryAuthorized":false,
  "accountRequests":0,"dispatchSnapshotModified":false}));Ok(())
}
