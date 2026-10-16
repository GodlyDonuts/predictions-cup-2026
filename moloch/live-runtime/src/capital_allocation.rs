//! Sole-worker owner-cash allocation. No network/client/fitting operation.
//! Caller supplies ORIGINAL retained receipts and held owner/dispatch/SDK objects.
//! Mandate SHA is pinned by the execution owner's separately validated Control.
use crate::{collateral_feedback::{self,Boundary,RetainedReceipt},feedback};
use moloch_replay::{amount,money,session::Session};
use serde::{Serialize,Deserialize};
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::time::{Instant,Duration};
pub type Result<T>=feedback::Result<T>;
const SOURCE:&str="moloch-owner-capital";
fn need(ok:bool,e:&'static str)->Result<()>{if ok{Ok(())}else{Err(e.into())}}
fn hash<T:Serialize>(v:&T)->Result<String>{Ok(format!("{:x}",Sha256::digest(serde_json::to_vec(v)?)))}
fn clock(s:&str)->Result<i64>{Ok(chrono::DateTime::parse_from_rfc3339(s)?.timestamp_millis())}
fn sha(s:&str)->bool{s.len()==64&&s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))}
#[derive(Serialize,Deserialize)]#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct MandateInput{
 pub authorization_id:String,pub allocation_id:String,pub control_sha256:String,
 pub bank_manifest_hash:String,pub run_id:String,pub account_scope:String,
 pub tournament_id:String,pub tournament_slug:String,pub profile_id:String,
 pub scope_sha256:String,pub delta:String,pub maximum_extra:String,
 pub maximum_effective_initial:String,pub maximum_committed:String,
 pub approved_at:String,pub valid_until:String,
}
/// No deserialization or mutable access. Expected SHA must come from separately
/// pinned owner Control; hashes alone do not create human authorization.
pub struct ApprovedMandate{input:MandateInput,sha256:String}
impl ApprovedMandate{
 pub fn verify(input:MandateInput,approved_sha256:&str)->Result<Self>{
  let digest=hash(&serde_json::to_value(&input)?)?;need(sha(approved_sha256)&&digest==approved_sha256,"CAPITAL_MANDATE_NOT_APPROVED")?;
  need(input.authorization_id=="human_moloch_additional_cash_20261016"&&!input.allocation_id.is_empty()&&input.allocation_id.len()<=128&&sha(&input.control_sha256)&&sha(&input.bank_manifest_hash)&&sha(&input.scope_sha256)&&!input.run_id.is_empty()&&!input.profile_id.is_empty()&&!input.account_scope.is_empty(),"CAPITAL_MANDATE")?;
  need(amount(&input.delta)?>0&&amount(&input.delta)?<=amount(&input.maximum_extra)?&&amount(&input.maximum_extra)?<=55_579_915_000&&amount(&input.maximum_effective_initial)?<=95_579_915_000&&amount(&input.maximum_committed)?<=95_579_915_000,"CAPITAL_MANDATE_LIMIT")?;
  need(clock(&input.approved_at)?<clock(&input.valid_until)?,"CAPITAL_MANDATE_WINDOW")?;Ok(Self{input,sha256:digest})
 }
 pub fn sha256(&self)->&str{&self.sha256}
}
#[derive(Clone,Copy)]pub struct Sources<'a>{pub account:&'a RetainedReceipt,pub collateral:&'a RetainedReceipt,pub positions:&'a RetainedReceipt,pub orders:&'a RetainedReceipt}
impl<'a>Sources<'a>{fn rows(&self)->[&'a RetainedReceipt;4]{[self.account,self.collateral,self.positions,self.orders]}}
/// Consumed immediately. Final commit rechecks actual held file descriptors,
/// raw receipt bytes, original dispatch heads, quiet queue and native head.
pub struct Prepared<'a>{event:Value,captured:Value,sources:Sources<'a>,minted:Instant,processed:String,valid_until:i64}
pub struct Committed{pub source_sequence:u64,pub event_sha256:String,pub durable_head:Value,pub report:Value}
pub fn prepare<'a>(bank:&Session,b:&Boundary<'_>,mandate:&ApprovedMandate,sources:Sources<'a>,processed:&str)->Result<Prepared<'a>>{
 let at=clock(processed)?;let m=&mandate.input;need(clock(&m.approved_at)?<=at&&at<clock(&m.valid_until)?,"CAPITAL_MANDATE_EXPIRED")?;
 let seed=bank.live_account_seed().ok_or("CAPITAL_LIVE_BANK")?;let scope=b.transport.scope();
 need(bank.manifest_hash().as_str()==Some(m.bank_manifest_hash.as_str())&&seed["runId"]==m.run_id&&m.account_scope==b.transport.account_scope()&&scope.tournament_id==m.tournament_id&&scope.tournament_slug==m.tournament_slug&&hash(&scope)?==m.scope_sha256,"CAPITAL_MANDATE_SCOPE")?;
 let delta=amount(&m.delta)?;let effective=bank.engine().wallet.initial.checked_add(delta).ok_or("CAPITAL_RANGE")?;let maximum=amount(bank.engine().policy()["maximumCommitted"].as_str().ok_or("CAPITAL_POLICY")?)?.checked_add(delta).ok_or("CAPITAL_RANGE")?;
 let previous=bank.engine().owner_capital_allocation_record();let prior_net=previous.as_ref().map_or(Ok(0),|r|amount(r["netContributions"].as_str().ok_or("CAPITAL_PREVIOUS")?))?;
 need(prior_net.checked_add(delta).is_some_and(|n|n<=amount(&m.maximum_extra).unwrap_or(-1))&&effective<=amount(&m.maximum_effective_initial)?&&maximum<=amount(&m.maximum_committed)?,"CAPITAL_MANDATE_LIMIT")?;
 let captured=collateral_feedback::allocation_boundary(bank,b,at)?;
 for r in sources.rows(){r.verify_root(b.root)?;let receipt=r.receipt();let start=clock(&receipt.request_started_at)?;let end=clock(&receipt.received_at)?;need(receipt.status==200&&start>=0&&start<=end&&end<=at&&at-start<=20_000,"CAPITAL_ORIGINAL_SOURCE_CLOCK")?;}
 need(sources.account.receipt().data["id"]==m.profile_id,"CAPITAL_PROFILE")?;
 let mut event=json!({"type":"live_capital_allocation_v1","source":"moloch_live_dispatcher","authorizationId":m.authorization_id,"allocationId":m.allocation_id,"controlSha256":m.control_sha256,"mandateSha256":mandate.sha256,"receivedAt":processed,"processedAt":processed,"bankScope":{"runId":m.run_id,"bankManifestHash":bank.manifest_hash(),"tournamentId":scope.tournament_id,"tournamentSlug":scope.tournament_slug,"profileId":m.profile_id},"expectedReportHash":captured["reportHash"],"expectedPolicySha256":hash(bank.engine().policy())?,"expectedPreviousAllocationSha256":previous.map_or(Value::Null,|r|r["sha256"].clone()),"expectedInitialCash":money(bank.engine().wallet.initial),"expectedMaximumCommitted":bank.engine().policy()["maximumCommitted"],"delta":m.delta,"effectiveInitialCash":money(effective),"effectiveMaximumCommitted":money(maximum),"scopeExchanges":scope.exchanges,"boundary":{"nativeHead":captured["nativeHead"],"ordinaryDispatchHash":captured["ordinarySha256"],"packageDispatchHash":captured["packageSha256"],"queueHeadHash":hash(&captured["queue"])?,"ownerLeaseHash":captured["ownerLeaseSha256"],"unknownCount":0,"activeRequest":null,"unsubmittedOrderIds":captured["unsubmitted"],"provedNoNetworkOrderIds":captured["provedNoNetworkOrderIds"]},"receipts":{"account":sources.account.receipt(),"collateral":sources.collateral.receipt(),"positions":sources.positions.receipt(),"orders":sources.orders.receipt()}});
 event["originalSourceEvidence"]=json!(sources.rows().iter().map(|r|r.evidence()).collect::<Result<Vec<_>>>()?);
 // Native Session independently verifies seed cash/prior unallocated capital at
 // accept; preview here enforces all event economics without durable changes.
 let mut preview=bank.engine().clone();preview.advance(&event).map_err(|s|->Box<dyn std::error::Error>{s.into()})?;
 let official=baal_transport_service::decimal_units(&sources.account.receipt().data["balance"].as_str().map(str::to_owned).unwrap_or_else(||sources.account.receipt().data["balance"].to_string()))?;
 need(official==crate::authority::reconciled_expected_cash(bank)?,"CAPITAL_ORIGINAL_CASH_MISMATCH")?;
 need(collateral_feedback::allocation_boundary(bank,b,at)?==captured,"CAPITAL_BOUNDARY_CHANGED")?;for r in sources.rows(){r.verify_root(b.root)?;}
 Ok(Prepared{event,captured,sources,minted:Instant::now(),processed:processed.into(),valid_until:clock(&m.valid_until)?})
}
impl Prepared<'_>{
 pub fn commit(self,bank:&mut Session,b:&Boundary<'_>,now:&str)->Result<Committed>{
  let at=clock(now)?;need(self.minted.elapsed()<=Duration::from_secs(2)&&at>=clock(&self.processed)?&&at<self.valid_until&&at-clock(&self.processed)?<=2000,"CAPITAL_COMMIT_EXPIRED")?;
  need(collateral_feedback::allocation_boundary(bank,b,at)?==self.captured,"CAPITAL_BOUNDARY_CHANGED")?;
  for r in self.sources.rows(){r.verify_root(b.root)?;need(at-clock(&r.receipt().request_started_at)?<=20_000,"CAPITAL_ORIGINAL_SOURCE_CLOCK")?;}
  let sequence=bank.last_source_sequence(SOURCE).checked_add(1).ok_or("CAPITAL_RANGE")?;let digest=hash(&self.event)?;
  need(bank.accept(SOURCE,sequence,&self.event)?,"CAPITAL_ALREADY_COMMITTED")?;need(bank.committed_source_hash(SOURCE,sequence)==Some(digest.as_str()),"CAPITAL_COMMIT_REFERENCE")?;
  Ok(Committed{source_sequence:sequence,event_sha256:digest,durable_head:bank.durable_head(),report:bank.engine().report().map_err(|s|->Box<dyn std::error::Error>{s.into()})?})
 }
}
