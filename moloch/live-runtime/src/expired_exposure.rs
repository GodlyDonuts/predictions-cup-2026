//! Reconcile an expired uncertain attempt's economic exposure, without claiming
//! that the HTTP request was never sent or manufacturing a venue order ID.
use crate::{dispatch::DispatchLog, feedback::{self, Result}};
use baal_transport_service::{Receipt, Scope, decimal_units};
use moloch_replay::{Side, session::Session, money};
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::{collections::{BTreeMap,BTreeSet},fs,path::Path};
pub fn hash(v:&Value)->String{format!("{:x}",Sha256::digest(serde_json::to_vec(v).expect("JSON")))}
fn ms(v:&str)->Result<i64>{Ok(chrono::DateTime::parse_from_rfc3339(v)?.timestamp_millis())}
fn need(ok:bool,code:&str)->Result<()>{if ok{Ok(())}else{Err(code.to_owned().into())}}
fn value_units(v:&Value)->Result<i128>{Ok(decimal_units(&v.as_str().map(str::to_owned).unwrap_or_else(||v.to_string()))?)}
pub struct Proof{evidence:Value}
impl Proof{
 pub fn evidence(&self)->&Value{&self.evidence}
 pub fn event(&self)->Value{event(&self.evidence)}
 pub fn qid(&self)->u64{self.evidence["qid"].as_u64().expect("verified qid")}
 pub fn binding(&self)->&Value{&self.evidence["binding"]}
}
fn event(p:&Value)->Value{let q=&p["quote"];json!({"type":"expired_attempt_exposure_reconciled_v1","source":"moloch_live_dispatcher","orderId":p["qid"],"exchangeId":q["exchangeId"],"marketId":q["marketId"],"expiresAt":q["expiresAt"],"receiptHash":hash(p),"originalIntentSha256":p["originalIntentSha256"],"originalAttemptSha256":p["originalAttemptSha256"],"originalErrorSha256":p["originalErrorSha256"],"networkAttemptRetained":true,"sameIntentRetryAuthorized":false,"venueOrderIdentityKnown":false,"accountEffectsReconciled":true,"receivedAt":p["reconciledAt"],"processedAt":p["reconciledAt"]})}
/// Called only while the stopped writer's original global lease is held. These
/// receipts come from its shared SDK queue after original bank restoration.
pub fn capture(bank:&Session,seed:&Value,log:&DispatchLog,qid:u64,scope:&Scope,queue:&Value,account:Receipt,positions:Receipt,orders:Receipt,collateral:Receipt,at:&str)->Result<Proof>{
 let q=bank.engine().order_records().into_iter().find(|q|q["id"]==qid).ok_or("EXPOSURE_NATIVE_INTENT")?;
 need(q["retired"]==false&&q["filled"]==0&&!bank.engine().live_order_bindings().contains_key(&qid),"EXPOSURE_NATIVE_ALREADY_BOUND")?;
 need(q["action"]=="sell"&&q.get("reviewedInventoryExit").is_some(),"EXPOSURE_REVIEWED_INVENTORY_SALE_ONLY")?;
 let original=log.intents.get(&qid).ok_or("EXPOSURE_ORIGINAL_INTENT")?;
 for key in ["id","marketId","exchangeId","side","action","quantity","price","expiresAt"]{need(q[key]==original["quote"][key],"EXPOSURE_QUOTE_CHANGED")?;}
 let pending:Vec<_>=log.attempted.iter().filter(|id|!log.acknowledgments.contains_key(id)&&!log.proved_no_network(**id)&&!log.exposure_resolved(**id)).copied().collect();
 need(pending==vec![qid]&&log.acknowledgments.keys().all(|id|log.closed.contains(id)),"EXPOSURE_OTHER_PENDING_ATTEMPT")?;
 let report=bank.engine().report()?;
 need(report.get("collateralAccounting").is_none(),"EXPOSURE_COLLATERAL_LEDGER_NOT_SUPPORTED")?;
 let profile=report["ownerCapitalAllocation"]["bankScope"]["profileId"].as_str().filter(|s|!s.is_empty()).ok_or("EXPOSURE_OWNER_PROFILE")?;
 let expected=decimal_units(seed["availableCash"].as_str().ok_or("EXPOSURE_SEED_CASH")?)?+bank.engine().wallet.cash-bank.engine().wallet.initial+bank.engine().opening_inventory_cost();
 let mut signed=BTreeMap::new();
 let native_positions=bank.engine().wallet.positions();
 for ex in native_positions.as_object().ok_or("EXPOSURE_NATIVE_POSITIONS")?.keys(){let n=bank.engine().wallet.quantity(ex,Side::Yes) as i128-bank.engine().wallet.quantity(ex,Side::No) as i128;if n!=0{signed.insert(ex.clone(),n.to_string());}}
 let key=original["spec"]["body"]["idempotencyKey"].as_str().ok_or("EXPOSURE_KEY")?;
 let binding=queue["state"]["unknown"][key].clone();
 need(queue["state"]["active"].is_null()&&queue["state"]["manual_review"]==false&&queue["state"]["unknown"].as_object().is_some_and(|m|m.len()==1),"EXPOSURE_QUEUE_NOT_QUIET")?;
 let attempt=log.original_attempt(qid).ok_or("EXPOSURE_ATTEMPT")?;let error=log.original_error(qid).ok_or("EXPOSURE_ERROR")?;
 let p=json!({"kind":"expired_uncertain_exposure_v1","qid":qid,"quote":q,"scope":scope,"binding":binding,"originalIntentSha256":hash(original),"originalAttemptSha256":hash(attempt),"originalErrorSha256":hash(error),"manifestHash":bank.manifest_hash(),"nativeHeadBefore":bank.durable_head(),"expectedCash":money(expected),"expectedSignedPositions":signed,"expectedProfileId":profile,"reconciledAt":at,"account":account,"positions":positions,"orders":orders,"collateral":collateral});
 let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis() as i64;
 validate(&p,log)?;need(ms(at)?<=now+1000,"EXPOSURE_FUTURE_CAPTURE")?;Ok(Proof{evidence:p})
}
fn validate(p:&Value,log:&DispatchLog)->Result<()>{
 need(p["kind"]=="expired_uncertain_exposure_v1","EXPOSURE_KIND")?;
 let qid=p["qid"].as_u64().ok_or("EXPOSURE_QID")?;let original=log.intents.get(&qid).ok_or("EXPOSURE_INTENT")?;
 let attempt=log.original_attempt(qid).ok_or("EXPOSURE_ATTEMPT")?;let error=log.original_error(qid).ok_or("EXPOSURE_ERROR")?;
 need(p["originalIntentSha256"]==hash(original)&&p["originalAttemptSha256"]==hash(attempt)&&p["originalErrorSha256"]==hash(error)&&error["error"]["mayHaveExecuted"]==true&&error["error"]["orderId"].is_null()&&!log.acknowledgments.contains_key(&qid),"EXPOSURE_ORIGINAL_BINDING")?;
 let scope:Scope=serde_json::from_value(p["scope"].clone())?;let body=&original["spec"]["body"];
 need(body["tournamentId"]==scope.tournament_id&&p["binding"]["key"]==body["idempotencyKey"]&&p["binding"]["payload_hash"]==hash(&json!({"scope":scope,"spec":original["spec"]})),"EXPOSURE_PAYLOAD_BINDING")?;
 let key=body["idempotencyKey"].as_str().ok_or("EXPOSURE_KEY")?;need(p["binding"]["intent"]==key,"EXPOSURE_INTENT_KEY")?;
 let at=ms(p["reconciledAt"].as_str().ok_or("EXPOSURE_CLOCK")?)?;let expiry=ms(body["expirationDate"].as_str().ok_or("EXPOSURE_EXPIRY")?)?;
 need(p["quote"]["expiresAt"].as_i64()==Some(expiry),"EXPOSURE_NATIVE_EXPIRY")?;
 let mut receipts=Vec::new();
 for label in ["orders","account","positions","collateral"]{let r:Receipt=serde_json::from_value(p[label].clone())?;let start=ms(&r.request_started_at)?;let end=ms(&r.received_at)?;need(r.status==200&&start>=expiry+60_000&&start<=end&&end<=at&&at-start<=60_000,"EXPOSURE_POST_EXPIRY_CLOCK_OR_STATUS")?;receipts.push(r);}
 let orders=&receipts[0];let account=&receipts[1];let positions=&receipts[2];let collateral=&receipts[3];
 need(orders.endpoint==format!("/orders?status=all&limit=200&tournamentId={}",scope.tournament_id)&&account.endpoint==format!("/account?tournamentId={}",scope.tournament_id)&&positions.endpoint==format!("/tournaments/{}/portfolio/positions",scope.tournament_slug)&&collateral.endpoint==format!("/portfolio/collateral?tournamentId={}",scope.tournament_id),"EXPOSURE_ENDPOINT_SCOPE")?;
 need(orders.data["coverage"]["complete"]==true&&orders.data["pagination"]["hasMore"]==false&&orders.data["pagination"]["nextCursor"].is_null(),"EXPOSURE_HISTORY_INCOMPLETE")?;
 let mut ids=BTreeSet::new();for row in orders.data["data"].as_array().ok_or("EXPOSURE_ORDER_ROWS")?{
  need(row["tournamentId"]==scope.tournament_id&&row["open"]==false&&ids.insert(row["id"].to_string()),"EXPOSURE_OPEN_OR_FOREIGN_ORDER")?;
  // Broadly reject a possibly canonicalized/resized original request too.
  need(!(row["exchangeId"]==body["exchangeId"]&&row["expirationDate"]==body["expirationDate"]),"EXPOSURE_POSSIBLE_ACCEPTED_ORDER")?;
 }
 need(account.data["id"]==p["expectedProfileId"]&&value_units(&account.data["balance"])?==value_units(&p["expectedCash"] )?,"EXPOSURE_PROFILE_OR_CASH_CHANGED")?;
 let mut actual=BTreeMap::new();let mut seen=BTreeSet::new();for row in positions.data["positions"].as_array().ok_or("EXPOSURE_POSITION_ROWS")?{let ex=row["exchangeId"].as_str().ok_or("EXPOSURE_EXCHANGE")?;let n=row["quantity"].as_i64().ok_or("EXPOSURE_QUANTITY")?;need(seen.insert(ex),"EXPOSURE_DUPLICATE_POSITION")?;if n!=0{actual.insert(ex.to_owned(),n.to_string());}}
 need(serde_json::to_value(actual)?==p["expectedSignedPositions"],"EXPOSURE_POSITIONS_CHANGED")?;
 need(collateral.data["data"].as_array().is_some_and(Vec::is_empty)&&value_units(&collateral.data["availableAdvance"])?==0,"EXPOSURE_COLLATERAL_NOT_EMPTY")?;
 Ok(())
}
/// Startup trust comes from an original committed native source event plus the
/// exact retained private receipt bundle, not from a serialized boolean.
pub fn restore(root:&Path,bank:&Session,log:&mut DispatchLog)->Result<()>{
 let rows:Vec<_>=log.exposure_records().values().cloned().collect();
 for row in rows{let qid=row["qid"].as_u64().ok_or("EXPOSURE_QID")?;let path=root.join(format!("expired-exposure-{qid}.private.json"));let meta=fs::symlink_metadata(&path)?;need(meta.is_file()&&!meta.file_type().is_symlink()&&meta.len()<=16*1024*1024,"EXPOSURE_ORIGINAL_FILE")?;let raw=fs::read(&path)?;need(format!("{:x}",Sha256::digest(&raw))==row["proofFileSha256"],"EXPOSURE_FILE_CHANGED")?;let p:Value=serde_json::from_slice(&raw)?;validate(&p,log)?;need(p["manifestHash"]==*bank.manifest_hash(),"EXPOSURE_BANK_CHANGED")?;let e=event(&p);let seq=row["nativeSourceSequence"].as_u64().ok_or("EXPOSURE_SOURCE_SEQUENCE")?;need(bank.committed_source_hash("expired-exposure-recovery",seq)==Some(hash(&e).as_str()),"EXPOSURE_NATIVE_EVENT_NOT_COMMITTED")?;let proof=Proof{evidence:p};log.verify_exposure(&proof)?;}
 Ok(())
}
pub fn file_sha(bytes:&[u8])->String{format!("{:x}",Sha256::digest(bytes))}

#[cfg(test)]pub(crate) mod tests{
 use super::*;
 fn fixture()->(std::path::PathBuf,DispatchLog,Value){
  let root=std::env::temp_dir().join(format!("expired-exposure-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));fs::create_dir(&root).unwrap();let mut log=DispatchLog::open(&root.join("dispatch.ndjson")).unwrap();
  let scope=json!({"tournamentId":"cup","tournamentSlug":"midterm-elections","exchanges":{"101":"201"}});
  let body=json!({"idempotencyKey":"run:1","exchangeId":"101","side":"yes","action":"sell","quantity":2,"price":0.985,"expirationDate":"2026-10-18T05:00:00Z","tournamentId":"cup"});
  let q=json!({"id":1,"exchangeId":"101","marketId":"201","side":"YES","action":"sell","quantity":2,"price":"0.985000","expiresAt":1792299600000i64,"filled":0,"retired":false});
  let intent=json!({"kind":"intent","qid":1,"quote":q,"spec":{"method":"POST","endpoint":"/orders","body":body,"bodyJson":null}});
  let attempt=json!({"kind":"attempt","qid":1,"at":"2026-10-18T04:59:30Z"});let error=json!({"kind":"error","qid":1,"at":"2026-10-18T04:59:31Z","error":{"mayHaveExecuted":true,"orderId":null}});
  for e in [&intent,&attempt,&error]{log.append(e.clone()).unwrap();}
  let r=|endpoint:String,data:Value|json!({"endpoint":endpoint,"status":200,"requestStartedAt":"2026-10-18T05:02:00Z","receivedAt":"2026-10-18T05:02:01Z","data":data});
  let p=json!({"kind":"expired_uncertain_exposure_v1","qid":1,"quote":q,"scope":scope,"binding":{"key":"run:1","intent":"run:1","payload_hash":hash(&json!({"scope":scope,"spec":intent["spec"]}))},"originalIntentSha256":hash(&intent),"originalAttemptSha256":hash(&attempt),"originalErrorSha256":hash(&error),"reconciledAt":"2026-10-18T05:02:02Z","expectedProfileId":"profile","expectedCash":"100.000000","expectedSignedPositions":{"101":"2"},"orders":r("/orders?status=all&limit=200&tournamentId=cup".into(),json!({"data":[],"coverage":{"complete":true},"pagination":{"hasMore":false,"nextCursor":null}})),"account":r("/account?tournamentId=cup".into(),json!({"id":"profile","balance":100})),"positions":r("/tournaments/midterm-elections/portfolio/positions".into(),json!({"positions":[{"exchangeId":"101","quantity":2}]})),"collateral":r("/portfolio/collateral?tournamentId=cup".into(),json!({"data":[],"availableAdvance":0}))});
  (root,log,p)
 }
 pub(crate) fn resolved_fixture()->(std::path::PathBuf,DispatchLog){let(root,mut log,p)=fixture();validate(&p,&log).unwrap();log.append_exposure(&Proof{evidence:p},&"a".repeat(64),1).unwrap();(root,log)}
 #[test]fn complete_expired_unchanged_account_preserves_attempt_without_fake_ack(){let(root,mut log,p)=fixture();validate(&p,&log).unwrap();let proof=Proof{evidence:p};assert!(log.append(json!({"kind":"expired_exposure_reconciled","qid":1})).is_err());log.append_exposure(&proof,&"a".repeat(64),1).unwrap();log.require_resolved_attempts().unwrap();assert!(!log.never_attempted(1)&&!log.proved_no_network(1)&&log.acknowledgments.is_empty()&&log.closed.is_empty());drop(log);let log=DispatchLog::open(&root.join("dispatch.ndjson")).unwrap();assert!(log.require_resolved_attempts().is_err(),"serialized resolution must be reverified against native source and original receipt bundle");drop(log);fs::remove_dir_all(root).unwrap();}
 #[test]fn incomplete_stale_changed_foreign_or_possible_execution_is_rejected(){let(root,log,p)=fixture();validate(&p,&log).unwrap();for case in 0..13{let mut x=p.clone();match case{0=>x["orders"]["data"]["coverage"]["complete"]=json!(false),1=>x["orders"]["data"]["pagination"]["hasMore"]=json!(true),2=>x["account"]["data"]["balance"]=json!(101),3=>x["positions"]["data"]["positions"][0]["quantity"]=json!(1),4=>x["account"]["data"]["id"]=json!("other"),5=>x["orders"]["requestStartedAt"]=json!("2026-10-18T04:59:59Z"),6=>x["reconciledAt"]=json!("2026-10-18T05:04:00Z"),7=>x["collateral"]["data"]["data"]=json!([{}]),8=>x["orders"]["endpoint"]=json!("/orders?status=open"),9=>x["binding"]["payload_hash"]=json!("0".repeat(64)),10=>x["orders"]["data"]["data"]=json!([{"id":9,"tournamentId":"cup","open":true}]),11=>x["orders"]["data"]["data"]=json!([{"id":9,"tournamentId":"cup","open":false,"exchangeId":"101","expirationDate":"2026-10-18T05:00:00Z"}]),_=>x["positions"]["data"]["positions"]=json!([])}assert!(validate(&x,&log).is_err(),"case {case}");}drop(log);fs::remove_dir_all(root).unwrap();}
}
