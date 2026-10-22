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
 let record=log.pre_network_records().get(&qid).ok_or("PRIOR_TYPED_NOT_SENT_RECORD_REQUIRED")?.clone();
 let(spec,permit,at,error)=log.pre_admission_originals(qid)?;
 let proof=moloch_live_runtime::ordinary_pre_network::restore(account,&scope,&spec,&permit,at,&error,&record["proof"])?;
 log.verify_pre_network_not_sent(qid,&proof)?;
 if !log.proved_no_network(qid)||fs::read(&args[2])?!=bytes{return Err("PRIOR_VERIFIED_SNAPSHOT_CHANGED".into())}
 println!("{}",json!({"qid":qid,"priorRawSha256":args[4],"kind":"positively_proved_pre_network_not_sent",
  "dispatchSnapshotSha256":format!("{:x}",Sha256::digest(&bytes)),"opaqueSdkProofVerified":true,
  "originalProof":proof.evidence(),"exchangeClosedClaimed":false,"retryAuthorized":false,
  "accountRequests":0,"dispatchSnapshotModified":false}));Ok(())
}
