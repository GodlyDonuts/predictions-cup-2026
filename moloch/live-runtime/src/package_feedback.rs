//! Source-bound package feedback. Pure event construction only: the caller must
//! durably admit returned events; ACKs never stand in for fills or cash changes.
use baal_transport_service::{Scope,Permit,Receipt,TransportError,multi_leg::{self,Basket,Side,ResponseEvidence,ZeroExecutionReason}};
use moloch_replay::session::Session;
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use crate::feedback;
pub type Result<T> = feedback::Result<T>;
fn hash<T:serde::Serialize>(v:&T)->Result<String>{Ok(format!("{:x}",Sha256::digest(serde_json::to_vec(v)?)))}
fn clock(s:&str)->Result<i64>{Ok(chrono::DateTime::parse_from_rfc3339(s)?.timestamp_millis())}
fn need(ok:bool)->Result<()>{if ok{Ok(())}else{Err("PACKAGE_FEEDBACK_BINDING".into())}}
fn current(bank:&Session,p:&Value,scope:&Scope)->Result<()>{
 let records=bank.engine().package_records();
 need(records.as_array().ok_or("PACKAGE_RECORDS")?.iter().any(|r|r==p))?;
 let seed=bank.live_account_seed().ok_or("PACKAGE_NOT_LIVE")?;
 need(p["bankScope"]["runId"]==seed["runId"]&&p["bankScope"]["bankManifestHash"]==*bank.manifest_hash()&&p["bankScope"]["tournamentId"]==scope.tournament_id)?;
 need(matches!(p["phase"].as_str(),Some("attempted"|"acknowledged"|"unknown")))
}
fn bound(bank:&Session,p:&Value,b:&Basket,s:&Scope,permit:&Permit)->Result<baal_transport_service::RequestSpec>{
 current(bank,p,s)?;
 need(p["bankScope"]["runId"]==permit.run_id&&p["packageId"]==permit.intent_id&&p["idempotencyKey"]==b.idempotency_key&&p["economics"]["maximumCost"]==b.maximum_cost)?;
 let legs=p["legs"].as_array().ok_or("PACKAGE_LEGS")?;need(legs.len()==b.legs.len())?;
 for (l,w) in legs.iter().zip(&b.legs){
  need(l["exchangeId"]==w.exchange_id&&l["side"]==(if w.side==Side::Yes{"YES"}else{"NO"})&&l["quantity"]==w.quantity&&l["limitPrice"]==w.limit_price&&s.exchanges.get(&w.exchange_id).map(|m|l["marketId"]==*m)==Some(true))?;
 }
 let spec=multi_leg::request_spec(b,s,permit)?;need(p["requestHash"]==hash(&spec)?)?;Ok(spec)
}
fn envelope(kind:&str,p:&Value,received:&str,processed:&str)->Result<Value>{
 need(clock(processed)? >= clock(received)?)?;
 Ok(json!({"type":kind,"source":"moloch_live_dispatcher","packageId":p["packageId"],"proofHash":p["proofHash"],"bankScope":p["bankScope"],"receivedAt":received,"processedAt":processed}))
}
/// Map all component ACKs in one native transaction, retaining the full original
/// response digest on every component. No synthetic single-order receipt exists.
pub fn acknowledgment(bank:&Session,p:&Value,b:&Basket,s:&Scope,permit:&Permit,r:&Receipt,processed:&str)->Result<Value>{
 bound(bank,p,b,s,permit)?;need(clock(&r.request_started_at)? >= p["attemptedAt"].as_i64().ok_or("PACKAGE_ATTEMPT_CLOCK")?)?;
 let ack=multi_leg::reported_fill(r,b)?;let receipt_hash=feedback::digest(r)?;
 let mut event=envelope("live_package_ack",p,&r.received_at,processed)?;let mut rows=Vec::new();
 for a in ack.legs{
  let l=&p["legs"][a.index];
  rows.push(json!({"type":"actual_order_ack","source":"official_sig_api_v1","receiptHash":receipt_hash,"orderId":l["qid"],"actualOrderId":a.order_id,"exchangeId":l["exchangeId"],"marketId":l["marketId"],"side":l["side"],"action":"buy","quantity":l["quantity"],"price":l["limitPrice"],"receivedAt":r.received_at,"processedAt":processed}));
 }
 event["acknowledgments"]=json!(rows);Ok(event)
}
/// Definitive zero execution only; unknown outcomes remain liabilities. Verify
/// both retained files and the exact frozen physical request before releasing.
pub fn no_execution(bank:&Session,p:&Value,b:&Basket,s:&Scope,permit:&Permit,error:&TransportError,reason:&ZeroExecutionReason,e:&ResponseEvidence,processed:&str)->Result<Value>{
 let spec=bound(bank,p,b,s,permit)?;
 need(matches!(p["phase"].as_str(),Some("attempted"|"unknown"))&&p["legs"].as_array().ok_or("PACKAGE_LEGS")?.iter().all(|l|l["actualOrderId"].is_null()&&l["filled"]==0))?;
 need(!error.may_have_executed&&error.status==Some(e.status)&&e.request_sha256==hash(&spec)?)?;
 need(std::fs::metadata(&e.private_metadata_path)?.len()<=8*1024*1024)?;
 let metadata=std::fs::read(&e.private_metadata_path)?;need(metadata.len()<=8*1024*1024)?;
 let m:Value=serde_json::from_slice(&metadata)?;
 need(m["evidence"]==serde_json::to_value(e)?&&m["request"]==serde_json::to_value(&spec)?)?;
 let data=multi_leg::evidenced_response(e)?;
 need(multi_leg::zero_execution_reason(e.status,&data,b.legs.len()).as_ref()==Some(reason)&&error.api_code.as_deref()==data["error"]["code"].as_str())?;
 need(p["attemptedAt"].as_i64().ok_or("PACKAGE_ATTEMPT_CLOCK")?<=clock(&e.request_started_at)?&&clock(&e.request_started_at)?<=clock(&e.received_at)?)?;
 let receipt=Receipt{endpoint:spec.endpoint,request_started_at:e.request_started_at.clone(),received_at:e.received_at.clone(),status:e.status,data};
 let mut event=envelope("live_package_no_execution",p,&e.received_at,processed)?;
 event["evidence"]=json!({"kind":"official_multi_leg_zero_execution","completeBody":true,"requestHash":p["requestHash"],"receiptHash":feedback::digest(&receipt)?,"responseHash":hash(&receipt.data)?,"response":receipt.data,"status":e.status,"requestStartedAt":e.request_started_at,"receivedAt":e.received_at});Ok(event)
}
fn component(bank:&Session,p:&Value,s:&Scope,index:usize)->Result<(Value,String)>{
 current(bank,p,s)?;let l=p["legs"].get(index).ok_or("PACKAGE_LEG_INDEX")?;
 let id=l["qid"].as_u64().ok_or("PACKAGE_QID")?;let actual=l["actualOrderId"].as_str().ok_or("PACKAGE_NOT_ACKNOWLEDGED")?;
 need(l["exchangeId"].as_str().and_then(|ex|s.exchanges.get(ex)).map(|market|l["marketId"]==*market)==Some(true))?;
 need(bank.engine().live_order_bindings().get(&id).map(String::as_str)==Some(actual))?;
 let q=bank.engine().order_records().into_iter().find(|q|q["id"]==id).ok_or("PACKAGE_QUOTE_MISSING")?;
 need(q["exchangeId"]==l["exchangeId"]&&q["marketId"]==l["marketId"]&&q["side"]==l["side"]&&q["action"]==l.get("action").cloned().unwrap_or_else(||json!("buy"))&&q["quantity"]==l["quantity"]&&q["price"]==l["limitPrice"])?;Ok((q,actual.to_owned()))
}
fn tape_endpoint(actual:&str,_s:&Scope,r:&Receipt)->Result<()>{need(r.endpoint==format!("/orders/{actual}/fills?limit=200"))}
/// Only independently received, complete official component fill tapes move cash.
pub fn fills(bank:&Session,p:&Value,s:&Scope,index:usize,r:&Receipt,processed:&str)->Result<Vec<Value>>{
 let(q,actual)=component(bank,p,s,index)?;tape_endpoint(&actual,s,r)?;
 need(clock(&r.request_started_at)?<=clock(&r.received_at)?&&clock(&r.request_started_at)? >= p["attemptedAt"].as_i64().ok_or("PACKAGE_ATTEMPT_CLOCK")?)?;
 feedback::fills(&q,&actual,&s.tournament_id,r,processed)
}
/// AON has no wire expiry: use genuine closed-order and subsequent complete tape
/// evidence, never recovered_acknowledgment's expiration-date reconstruction.
pub fn terminal(bank:&Session,p:&Value,s:&Scope,index:usize,order:&Receipt,tape:&Receipt,processed:&str)->Result<Option<Value>>{
 let(q,actual)=component(bank,p,s,index)?;tape_endpoint(&actual,s,tape)?;
 need(order.endpoint==format!("/orders/{actual}")&&clock(&order.request_started_at)?<=clock(&order.received_at)?&&clock(&tape.request_started_at)?<=clock(&tape.received_at)?&&clock(&order.request_started_at)? >= p["attemptedAt"].as_i64().ok_or("PACKAGE_ATTEMPT_CLOCK")?)?;
 feedback::terminal(&q,&actual,&s.tournament_id,order,tape,processed)
}
/// Completion requires already durably admitted real fills and terminal proofs.
pub fn complete(bank:&Session,p:&Value,s:&Scope,processed:&str)->Result<Value>{
 current(bank,p,s)?;
 for (i,l) in p["legs"].as_array().ok_or("PACKAGE_LEGS")?.iter().enumerate(){let(q,_)=component(bank,p,s,i)?;need(l["filled"]==l["quantity"]&&l["terminalReconciled"]==true&&q["filled"]==q["quantity"]&&q["retired"]==true)?;}
 envelope("live_package_complete",p,processed,processed)
}

/// Release an attempted package ONLY after its original in-process not-sent
/// result is durable AND the genuine V1 queue shows the positive pre-network
/// completion transition. No empty-queue or caller boolean inference is used.
/// Original private files must remain retained; this returns an event only.
pub fn local_not_sent(bank:&Session,p:&Value,b:&Basket,s:&Scope,permit:&Permit,dispatch:&crate::package_dispatch::PackageDispatchLog,original:&crate::package_not_sent_evidence::VerifiedNotSent,error:&TransportError,processed:&str)->Result<Value>{
 let spec=bound(bank,p,b,s,permit)?;need(p["phase"]=="attempted"&&!dispatch.unresolved()&&!error.may_have_executed&&error.status.is_none()&&error.order_id.is_none()&&error.api_code.is_none())?;
 let id=p["packageId"].as_str().ok_or("PACKAGE_ID")?;let record=dispatch.verified_record(id)?;
 let q=original.evidence();let attestation=serde_json::to_value(original.attestation()?)?;
 need(record["phase"]=="not_sent"&&record["outcome"]["error"]==serde_json::to_value(error)?&&record["outcome"]["queueEvidence"]==attestation&&record["admission"]["spec"]==serde_json::to_value(&spec)?&&record["admission"]["queue_binding"]==q["binding"]&&record["admission"]["queue_scope"]==q["queueScope"]&&record["admission"]["reserve"]["proofHash"]==p["proofHash"]&&record["admission"]["reserve"]["bankScope"]==p["bankScope"])?;
 let start=q["beforeRecord"]["state"]["last_clock"].as_i64().ok_or("PACKAGE_QUEUE_CLOCK")?;let received=q["tailRecord"]["state"]["last_clock"].as_i64().ok_or("PACKAGE_QUEUE_CLOCK")?;
 need(start>=p["attemptedAt"].as_i64().ok_or("PACKAGE_ATTEMPT_CLOCK")?&&clock(record["attemptedAt"].as_str().ok_or("PACKAGE_DISPATCH_CLOCK")?)?<=start&&clock(record["outcome"]["at"].as_str().ok_or("PACKAGE_DISPATCH_CLOCK")?)?>=received&&clock(processed)? >= received)?;
 let stamp=chrono::DateTime::from_timestamp_millis(received).ok_or("PACKAGE_QUEUE_CLOCK")?.to_rfc3339_opts(chrono::SecondsFormat::Millis,true);
 let mut event=envelope("live_package_no_execution",p,&stamp,processed)?;
 event["evidence"]=json!({"kind":"local_queue_not_sent_v1","requestHash":p["requestHash"],"sdkError":error,"requestJson":serde_json::to_string(&spec)?,"dispatchRecordHash":hash(&record)?,"dispatchRecord":record,"dispatchHead":dispatch.head(),"queueProof":q});Ok(event)
}

/// SDK-local pre-first-admission proof, distinct from queue completion and an
/// official response. No ACK/unknown/fill can be released through this path.
pub fn first_call_not_sent(bank:&Session,p:&Value,b:&Basket,s:&Scope,permit:&Permit,dispatch:&crate::package_dispatch::PackageDispatchLog,proof:&baal_transport_service::multi_leg_first_call::Proof,error:&TransportError,processed:&str)->Result<Value>{
 let spec=bound(bank,p,b,s,permit)?;
 crate::package_first_call::release(bank,p,&spec,dispatch,proof,error,processed)
}
