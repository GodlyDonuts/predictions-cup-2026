//! Original V1 sole-worker queue evidence only. No HTTP client, queue command,
//! lease acquisition, retry, recovery or authority is created here.
use baal_transport_service::admission::{self,ImmutableBinding,VerifiedLocalNotSentV1};
use crate::{feedback::Result,package_dispatch::QueueNotSentAttestation};
use serde_json::{Value,json};
use std::path::Path;
pub struct VerifiedNotSent{original:VerifiedLocalNotSentV1}
impl VerifiedNotSent{
 pub fn read(directory:&Path,scope:&str,binding:&ImmutableBinding,request:&str,minimum_clock:u64)->Result<Self>{Ok(Self{original:admission::verify_local_not_sent_v1(directory,scope,binding,request,minimum_clock).map_err(|_|"PACKAGE_LOCAL_QUEUE_PROOF_UNAVAILABLE")?})}
 pub fn evidence(&self)->&Value{self.original.evidence()}
 /// Construct the existing durable dispatch attestation ONLY from the positive
 /// original source transition. It is not inferred from an empty current queue.
 pub fn attestation(&self)->Result<QueueNotSentAttestation>{
  let p=self.evidence();let at=p["completionRecord"]["state"]["last_clock"].as_u64().ok_or("PACKAGE_QUEUE_CLOCK")?;
  let observed=chrono::DateTime::from_timestamp_millis(i64::try_from(at)?).ok_or("PACKAGE_QUEUE_CLOCK")?.to_rfc3339_opts(chrono::SecondsFormat::Millis,true);
  Ok(QueueNotSentAttestation{queue_scope:p["queueScope"].as_str().ok_or("PACKAGE_QUEUE_SCOPE")?.into(),observed_at:observed,request:p["request"].as_str().ok_or("PACKAGE_QUEUE_REQUEST")?.into(),binding:p["binding"].clone(),state:p["tailRecord"]["state"].clone(),completion:json!({"request":p["request"],"binding":p["binding"],"outcome":"not_sent","networkStarted":false})})
 }
}
