//! Financial bridge for the original SDK first-logical-invocation trace.
//! No API, account client, retry, queue absence inference or proof constructor.
//! Authenticity comes from the SDK opaque proof and retained original sources;
//! embedded JSON/digests allow deterministic native replay, not authority.
use baal_transport_service::{RequestSpec,TransportError,multi_leg_first_call::Proof};
use moloch_replay::session::Session;
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use crate::{feedback::Result,package_dispatch::PackageDispatchLog};
fn need(ok:bool)->Result<()>{if ok{Ok(())}else{Err("PACKAGE_FIRST_CALL_FEEDBACK_BINDING".into())}}
fn hash<T:serde::Serialize>(v:&T)->Result<String>{Ok(format!("{:x}",Sha256::digest(serde_json::to_vec(v)?)))}
fn clock(v:&str)->Result<i64>{Ok(chrono::DateTime::parse_from_rfc3339(v)?.timestamp_millis())}
pub(crate) fn release(bank:&Session,p:&Value,spec:&RequestSpec,log:&PackageDispatchLog,proof:&Proof,error:&TransportError,processed:&str)->Result<Value>{
 need(p["phase"]=="attempted"&&!log.unresolved()&&bank.engine().package_records().as_array().is_some_and(|rows|rows.iter().any(|row|row==p)))?;
 let id=p["packageId"].as_str().ok_or("PACKAGE_FIRST_CALL_ID")?;let record=log.verified_first_call_record(id)?;
 let outcome=&record["outcome"];let admission=&record["admission"];
 need(record["phase"]=="first_call_not_sent"&&outcome["kind"]=="first_call_not_sent"&&outcome["firstCallEvidence"]==*proof.evidence()&&outcome["error"]==serde_json::to_value(error)?&&admission["request_hash"]==hash(spec)?&&p["requestHash"]==admission["request_hash"]&&admission["spec"]==serde_json::to_value(spec)?&&admission["reserve"]["proofHash"]==p["proofHash"]&&admission["reserve"]["bankScope"]==p["bankScope"])?;
 let attempted=clock(record["attemptedAt"].as_str().ok_or("PACKAGE_FIRST_CALL_ATTEMPT")?)?;
 let at=clock(outcome["at"].as_str().ok_or("PACKAGE_FIRST_CALL_OUTCOME")?)?;
 crate::package_dispatch::validate_first_call(admission,&serde_json::to_value(error)?,proof.evidence(),attempted,at)?;
 let start=proof.evidence()["ticket"]["createdAtMs"].as_i64().ok_or("PACKAGE_FIRST_CALL_CLOCK")?;
 let end=proof.evidence()["terminal"]["completedAtMs"].as_i64().ok_or("PACKAGE_FIRST_CALL_CLOCK")?;
 need(start>=p["attemptedAt"].as_i64().ok_or("PACKAGE_FIRST_CALL_NATIVE_CLOCK")?&&clock(processed)? >=at)?;
 let received=chrono::DateTime::from_timestamp_millis(end).ok_or("PACKAGE_FIRST_CALL_CLOCK")?.to_rfc3339_opts(chrono::SecondsFormat::Millis,true);
 Ok(json!({"type":if p.get("probabilistic").is_some(){"probabilistic_portfolio_no_execution_v1"}else{"live_package_no_execution"},"source":"moloch_live_dispatcher","packageId":p["packageId"],"proofHash":p["proofHash"],"bankScope":p["bankScope"],"receivedAt":received,"processedAt":processed,"evidence":{"kind":"sdk_before_first_admit_no_http","requestHash":p["requestHash"],"requestJson":serde_json::to_string(spec)?,"sdkError":error,"dispatchRecordHash":hash(&record)?,"dispatchRecord":record,"dispatchHead":log.head(),"firstCallProof":proof.evidence()}}))
}
