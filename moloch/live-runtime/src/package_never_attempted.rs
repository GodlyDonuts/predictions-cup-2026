//! Release only behind an irreversible fence in the EXISTING sole-worker log.
//! The coordinator must durably log a physical attempt before any SDK call and
//! borrow its existing configured log, never open an evidence-selected copy.
//! Queue quiet/exact-binding absence is supplemental, not not-sent evidence.
//! Hashes bind admitted evidence; they do not authenticate a hostile rewriter.
use crate::{feedback::Result,package_dispatch::PackageDispatchLog};
use moloch_replay::session::Session;
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
fn need(ok:bool,why:&'static str)->Result<()>{if ok{Ok(())}else{Err(why.into())}}
fn hash(v:&Value)->Result<String>{Ok(format!("{:x}",Sha256::digest(serde_json::to_vec(v)?)))}
fn clock(v:&str)->Result<i64>{Ok(chrono::DateTime::parse_from_rfc3339(v)?.timestamp_millis())}
/// `abandon_before_attempt` must have succeeded first. This re-reads the actual
/// original retained journal and mints an opaque proof internally, not from JSON.
/// No mutation, network request, queue recovery, retry or automatic bank accept.
pub fn release(bank:&Session,p:&Value,dispatch:&PackageDispatchLog,processed:&str)->Result<Value>{
 let packages=bank.engine().package_records();
 need(packages.as_array().is_some_and(|rows|rows.iter().any(|row|row==p)),"PACKAGE_NEVER_NATIVE_RECORD")?;
 let phase=p["phase"].as_str().ok_or("PACKAGE_NEVER_NATIVE_PHASE")?;
 need(matches!(phase,"reserved"|"attempted")&&!dispatch.unresolved(),"PACKAGE_NEVER_NATIVE_PHASE")?;
 let seed=bank.live_account_seed().ok_or("PACKAGE_NEVER_NOT_LIVE")?;
 need(p["bankScope"]["runId"]==seed["runId"]&&p["bankScope"]["bankManifestHash"]==*bank.manifest_hash(),"PACKAGE_NEVER_BANK_SCOPE")?;
 let id=p["packageId"].as_str().ok_or("PACKAGE_NEVER_ID")?;
 let original=dispatch.verified_never_attempted(id)?;let proof=original.evidence();let record=&proof["dispatchRecord"];let admission=&record["admission"];
 need(admission["reserve"]["packageId"]==p["packageId"]&&admission["reserve"]["proofHash"]==p["proofHash"]&&hash(&admission["proof"])?==p["proofHash"]&&admission["reserve"]["bankScope"]==p["bankScope"]&&admission["reserve"]["economics"]==p["economics"]&&admission["reserve"]["expiresAt"].as_str().map(clock).transpose()?==p["expiresAt"].as_i64(),"PACKAGE_NEVER_ADMISSION_CHANGED")?;
 need(admission["proof"]["wireLegs"]==admission["reserve"]["legs"]&&admission["spec"]["method"]=="POST"&&admission["spec"]["endpoint"]=="/orders/multi-leg"&&admission["spec"]["body"]["executionMode"]=="allOrNone","PACKAGE_NEVER_REQUEST")?;
 let legs=p["legs"].as_array().ok_or("PACKAGE_NEVER_LEGS")?;let reserved=admission["reserve"]["legs"].as_array().ok_or("PACKAGE_NEVER_LEGS")?;
 need(legs.len()==reserved.len()&&(2..=3).contains(&legs.len()),"PACKAGE_NEVER_LEGS")?;
 let orders=bank.engine().order_records();
 for(l,r)in legs.iter().zip(reserved){
  let qid=l["qid"].as_u64().ok_or("PACKAGE_NEVER_QID")?;let q=orders.iter().find(|q|q["id"]==qid).ok_or("PACKAGE_NEVER_ORDER")?;
  for key in ["marketId","exchangeId","side","quantity","limitPrice","bookHash"]{need(l[key]==r[key],"PACKAGE_NEVER_LEG_CHANGED")?;}
  need(l.get("actualOrderId")==Some(&Value::Null)&&l["filled"]==0&&l["terminalReconciled"]==false&&!bank.engine().live_order_bindings().contains_key(&qid)&&q["retired"]==false&&q["filled"]==0&&q["action"]=="buy"&&q["marketId"]==l["marketId"]&&q["exchangeId"]==l["exchangeId"]&&q["side"]==l["side"]&&q["quantity"]==l["quantity"]&&q["price"]==l["limitPrice"],"PACKAGE_NEVER_AFTER_ACK_OR_FILL")?;
 }
 let at=clock(processed)?;let fence_at=clock(record["outcome"]["at"].as_str().ok_or("PACKAGE_NEVER_CLOCK")?)?;
 need(at>=proof["journalLastClockMs"].as_i64().ok_or("PACKAGE_NEVER_CLOCK")?&&at>=fence_at,"PACKAGE_NEVER_CLOCK")?;
 // Preserve the actual RequestSpec struct's serialization order. Re-encoding
 // its Value object would change the frozen request hash despite equal JSON.
 let spec:baal_transport_service::RequestSpec=serde_json::from_value(admission["spec"].clone())?;
 let request_json=serde_json::to_string(&spec)?;let request_hash=format!("{:x}",Sha256::digest(request_json.as_bytes()));
 let mut evidence=proof.clone();evidence["kind"]=json!("dispatch_never_attempted_v1");evidence["requestHash"]=admission["request_hash"].clone();evidence["requestJson"]=json!(request_json);
 let mut event=json!({"type":"live_package_no_execution","source":"moloch_live_dispatcher","packageId":p["packageId"],"proofHash":p["proofHash"],"bankScope":p["bankScope"],"receivedAt":record["outcome"]["at"],"processedAt":processed});
 if phase=="attempted"{
  need(p["requestHash"]==admission["request_hash"]&&p["requestHash"]==request_hash&&p["idempotencyKey"]==admission["basket"]["idempotencyKey"]&&p["attemptedAt"].as_i64().is_some_and(|t|t<=fence_at),"PACKAGE_NEVER_NATIVE_ATTEMPT_CHANGED")?;
  event["evidence"]=evidence;
 }else{
  need(p.get("requestHash")==Some(&Value::Null)&&p.get("idempotencyKey")==Some(&Value::Null)&&p.get("attemptedAt")==Some(&Value::Null),"PACKAGE_NEVER_RESERVED_CHANGED")?;
  event["evidence"]=json!({"kind":"never_attempted","confirmedNetworkAttempts":0});
  // Existing Reserved-only native transition; source evidence is retained for
  // audit, while the new Attempted-only evidence can never use this branch.
  event["neverAttemptedDispatchEvidence"]=evidence;
 }
 Ok(event)
}
