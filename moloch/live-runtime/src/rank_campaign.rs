//! Tartarus v1: one owner-reviewed physical BUY or SELL, not positive alpha.
//!
//! Frozen campaign JSON schema: schemaVersion=1, objective=
//! declared_first_place_rank_risk; campaignStage=direct|rotation_prerequisite|acquisition_prerequisite;
//! reviewedAtMs/validUntilMs; sources[{sha256,observedAtMs,maximumAgeMs,
//! assumptionLabel}]; account{positionsSha256,cash,reservedCash,
//! actualFillsSha256,modeledTerminalDebt,liabilityAssumption}; complete
//! contracts[{contractId,exchangeId,marketId,semanticSourceSha256,
//! officialContractRootSha256}]; scenarios[{id,kind,assumptionLabel,payouts}];
//! probabilityModels[{id,weightsPpm,assumptionLabel}]; fieldModels[{id,
//! thresholds,assumptionLabel,completeField:false}]; modelPairs[{probabilityId,
//! fieldId}]; action{contractId,exchangeId,marketId,side,action:BUY|SELL,
//! quantity,limitPrice,nominationBookSha256}; risk{maximumGrossCost,
//! maximumQuantity,maximumModeledLoss,minimumTerminalWealth,
//! minimumTargetGainPpm}; unmodeledStateStatement. Prerequisite-only
//! contingentPlan{assumptionLabel,executionAuthority:false,
//! actualFillAndFreshReapprovalRequired:true,minimumTargetGainPpm>0,
//! executionAssumption:{minimumCompletionMs,availableAtMs,validUntilMs,
//! conditionalFutureLiquidityAssumption},
//! buyLegs:[{contractId,exchangeId,marketId,side,quantity,limitPrice,
//! sourceSha256,availableAtMs,validUntilMs}]} is conditional research ONLY.
//! Acquisition prerequisites fund the current BUY from actual cash; later BUYs
//! remain conditional. At most 128 future BUY tranches, each bounded by the physical order cap;
//! repeated same-side tranches share cumulative market/funding limits. Native
//! evaluation includes the minimum post-leg pacing/rolling-rate horizon,
//! incomplete-prefix and partial-fill outcomes. Future liquidity/completion are
//! explicitly uncertified; every next tranche needs actual fills and reapproval.
//! Direct gain must be positive; a prerequisite may declare exactly zero
//! immediate gain. Monetary expectations remain explicit, including negative.
//! No source clock is derived from capture time. Current physical book evidence
//! is separate from the immutable nomination and checked afresh at each stage.
use crate::{authority::{SnapshotSet,reconciled_expected_cash},collateral_feedback::{self,Boundary,RetainedReceipt,ReceiptStore},feedback::Result,daily_windows};
use baal_transport_service::{Receipt,Scope,RequestSpec,Permit,decimal_units,relationships};
use moloch_replay::{session::Session,Side};
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::{collections::{BTreeSet,BTreeMap},fs::File,io::{Read,Seek,SeekFrom},path::PathBuf};
const MAX_CAMPAIGN:usize=2*1024*1024;
fn need(ok:bool,code:&'static str)->Result<()>{if ok{Ok(())}else{Err(code.into())}}
fn hash<T:serde::Serialize>(v:&T)->Result<String>{Ok(format!("{:x}",Sha256::digest(serde_json::to_vec(v)?)))}
fn rawhash(b:&[u8])->String{format!("{:x}",Sha256::digest(b))}
fn clock(s:&str)->Result<u64>{daily_windows::clock(s)}
fn amount(v:&Value)->Result<i128>{Ok(decimal_units(v.as_str().ok_or("RANK_DECIMAL")?)?)}
/// Original source bytes come from the owner's retained bounded roots, not paths
/// supplied by the campaign. Digests bind content, not economic truth/consent.
pub struct Original<'a>{pub sha256:&'a str,pub bytes:&'a [u8]}
#[derive(Clone)]pub struct Reviewed{campaign:Value,canonical_sha:String,raw_sha:String,until:u64}
impl Reviewed{
 pub fn load(bytes:&[u8],expected_raw_sha:&str,approved:&BTreeSet<String>,originals:&[Original<'_>],at:u64)->Result<Self>{
  need(bytes.len()<=MAX_CAMPAIGN&&rawhash(bytes)==expected_raw_sha&&approved.contains(expected_raw_sha),"RANK_REVIEW_NOT_APPROVED")?;
  let c:Value=serde_json::from_slice(bytes)?;need(c["schemaVersion"]==1&&c["objective"]=="declared_first_place_rank_risk","RANK_REVIEW_SCHEMA")?;
  let sources=c["sources"].as_array().filter(|v|!v.is_empty()&&v.len()<=64).ok_or("RANK_SOURCES")?;
  need(originals.len()==sources.len()&&originals.iter().map(|v|v.bytes.len()).sum::<usize>()<=16*1024*1024,"RANK_ORIGINAL_SOURCE_BOUND")?;
  let mut until=c["validUntilMs"].as_u64().ok_or("RANK_REVIEW_CLOCK")?;let mut seen=BTreeSet::new();
  for s in sources{let sha=s["sha256"].as_str().ok_or("RANK_SOURCE_HASH")?;need(seen.insert(sha)&&originals.iter().filter(|o|o.sha256==sha&&rawhash(o.bytes)==sha).count()==1,"RANK_ORIGINAL_SOURCE_MISSING_OR_CHANGED")?;let observed=s["observedAtMs"].as_u64().ok_or("RANK_SOURCE_CLOCK")?;let age=s["maximumAgeMs"].as_u64().filter(|a|*a>0).ok_or("RANK_SOURCE_CLOCK")?;need(observed<=at,"RANK_FUTURE_SOURCE")?;until=until.min(observed.checked_add(age).ok_or("RANK_CLOCK")?);}
  let reviewed=c["reviewedAtMs"].as_u64().ok_or("RANK_REVIEW_CLOCK")?;need(reviewed<=at&&at<until,"RANK_REVIEW_EXPIRED")?;
  Ok(Self{canonical_sha:hash(&c)?,raw_sha:expected_raw_sha.into(),campaign:c,until})
 }
 pub fn raw_sha256(&self)->&str{&self.raw_sha}
 pub fn campaign_sha256(&self)->&str{&self.canonical_sha}
 pub fn exchange_id(&self)->&str{self.campaign["action"]["exchangeId"].as_str().unwrap_or("")}
 pub fn action(&self)->&Value{&self.campaign["action"]}
 pub fn valid_until_ms(&self)->u64{self.until}
 pub fn managed_inventory(&self)->Option<&Value>{self.campaign.get("managedInventory").filter(|v|!v.is_null())}
 fn current(&self,at:u64)->Result<()>{need(at>=self.campaign["reviewedAtMs"].as_u64().ok_or("RANK_CLOCK")?&&at<self.until,"RANK_REVIEW_EXPIRED")}
}
struct DispatchPin{file:File,path:PathBuf,descriptor:Value,sha:String}
impl DispatchPin{
 fn new(b:&Boundary<'_>)->Result<Self>{let descriptor=b.ordinary.descriptor_identity()?;let path=b.root.join("dispatch.ndjson");let file=File::open(&path)?;let mut p=Self{file,path,descriptor,sha:String::new()};p.sha=p.read_prefix()?;Ok(p)}
 fn read_prefix(&self)->Result<String>{
  #[cfg(unix)]{use std::os::unix::fs::MetadataExt;for m in [self.file.metadata()?,std::fs::symlink_metadata(&self.path)?]{need(m.is_file()&&m.nlink()==1&&m.dev()==self.descriptor["device"].as_u64().ok_or("RANK_DISPATCH_IDENTITY")?&&m.ino()==self.descriptor["inode"].as_u64().ok_or("RANK_DISPATCH_IDENTITY")?&&m.len()>=self.descriptor["bytes"].as_u64().ok_or("RANK_DISPATCH_IDENTITY")?,"RANK_DISPATCH_CHANGED")?;}}
  #[cfg(not(unix))]{return Err("RANK_HELD_IDENTITY_UNSUPPORTED".into())}
  #[allow(unreachable_code)]{let n=self.descriptor["bytes"].as_u64().filter(|n|*n<=128*1024*1024).ok_or("RANK_DISPATCH_BOUND")?;let mut f=self.file.try_clone()?;f.seek(SeekFrom::Start(0))?;let mut take=f.take(n);let mut digest=Sha256::new();let mut got=0u64;let mut buf=[0u8;65536];loop{let k=take.read(&mut buf)?;if k==0{break}got+=k as u64;digest.update(&buf[..k]);}need(got==n,"RANK_DISPATCH_TRUNCATED")?;Ok(format!("{:x}",digest.finalize()))}
 }
 fn validate(&self)->Result<()>{need(self.read_prefix()?==self.sha,"RANK_DISPATCH_PREFIX_CHANGED")}
}
fn contract<'a>(r:&'a Reviewed)->Result<&'a Value>{r.campaign["contracts"].as_array().and_then(|rows|rows.iter().find(|v|v["contractId"]==r.action()["contractId"])).ok_or("RANK_TARGET_CONTRACT".into())}
fn validate_contract(r:&Reviewed,scope:&Scope,receipt:&Receipt)->Result<()>{let c=contract(r)?;need(receipt.status==200&&receipt.endpoint==format!("/markets/{}/nodes?tournamentId={}",c["marketId"].as_str().ok_or("RANK_MARKET")?,scope.tournament_id)&&receipt.data["market_id"]==c["marketId"]&&receipt.data["root"]["node_type"]=="contract"&&receipt.data["root"]["node_id"]==c["contractId"]&&receipt.data["root"]["contract_id"]==c["contractId"]&&hash(&receipt.data["root"])?==c["officialContractRootSha256"],"RANK_CONTRACT_CHANGED")}
fn facts(bank:&Session,b:&Boundary<'_>,r:&Reviewed,f:&SnapshotSet,collateral:&RetainedReceipt,at:u64)->Result<(Vec<RetainedReceipt>,u64)>{
 r.current(at)?;let scope=b.transport.scope();let a=r.action();let ex=a["exchangeId"].as_str().ok_or("RANK_EXCHANGE")?;let market=a["marketId"].as_str().ok_or("RANK_MARKET")?;
 need(scope.exchanges.get(ex).is_some_and(|m|m==market),"RANK_SCOPE")?;for c in r.campaign["contracts"].as_array().ok_or("RANK_REGISTRY")?{need(scope.exchanges.get(c["exchangeId"].as_str().ok_or("RANK_REGISTRY")?).is_some_and(|m|c["marketId"]==*m),"RANK_COMPLETE_REGISTRY_SCOPE")?;}
 let tid=&scope.tournament_id;let endpoints=[format!("/tournaments/{}",scope.tournament_slug),format!("/tournaments/{}/portfolio/positions",scope.tournament_slug),format!("/orders?status=open&limit=200&tournamentId={tid}"),format!("/account?tournamentId={tid}"),format!("/markets/{market}?tournamentId={tid}"),format!("/markets/{market}/nodes?tournamentId={tid}"),format!("/markets/{market}/orderbook?tournamentId={tid}&depth=20")];
 let rows=[&f.account,&f.positions,&f.open_orders,&f.cash_balance,&f.market,&f.contract,&f.book];let mut retained=vec![];let mut until=r.until;let mut last=0;
 for (i,(row,endpoint))in rows.iter().zip(&endpoints).enumerate(){let start=clock(&row.request_started_at)?;let end=clock(&row.received_at)?;let age=if i<4{20000}else{10000};need(row.status==200&&row.endpoint==*endpoint&&start<=end&&end<=at&&at-start<=age&&start>=last,"RANK_ORIGINAL_FACT_CLOCK_OR_SCOPE")?;last=end;until=until.min(start.checked_add(age).ok_or("RANK_CLOCK")?);retained.push(RetainedReceipt::open(b.root,row,ReceiptStore::Ordinary)?);}
 need(f.account.data["id"]==*tid&&f.account.data["slug"]==scope.tournament_slug&&f.account.data["status"]=="active"&&f.market.data["id"]==market&&f.market.data["status"]=="open"&&f.market.data["isComposite"]==false&&f.market.data["isMultiOutcome"]==false&&f.market.data["contexts"].as_array().is_some_and(|rows|rows.iter().any(|c|c["status"]=="open"&&c["type"]=="tournament"&&c["tournament"]["id"]==*tid)),"RANK_OFFICIAL_MARKET")?;
 validate_contract(r,scope,&f.contract)?;
 let power=relationships::parse_buying_power(&f.cash_balance.data,scope)?;let physical_cash=reconciled_expected_cash(bank)?;
 need(power.gross_cash_units==Some(physical_cash)&&physical_cash==bank.engine().wallet.cash,"RANK_WHOLE_ACCOUNT_CASH_REQUIRES_FULL_ALLOCATION")?;
 let p=&f.positions.data;need(p.get("pagination").is_none(),"RANK_INCOMPLETE_POSITIONS")?;let mut actual=BTreeMap::new();for row in p["positions"].as_array().filter(|a|a.len()<=4096).ok_or("RANK_POSITIONS")?{let ex=row["exchangeId"].as_str().ok_or("RANK_POSITION_SCOPE")?;need(scope.exchanges.get(ex).is_some_and(|m|row["marketId"]==*m)&&actual.insert(ex.to_owned(),row["quantity"].as_i64().ok_or("RANK_POSITION_QUANTITY")? as i128).is_none(),"RANK_POSITION_SCOPE")?;}
 let held=bank.engine().wallet.positions();for ex in actual.keys().chain(held.as_object().ok_or("RANK_POSITIONS")?.keys()){let yes=bank.engine().wallet.quantity(ex,Side::Yes);let no=bank.engine().wallet.quantity(ex,Side::No);need(yes.min(no)==0&&actual.get(ex).copied().unwrap_or(0)==yes as i128-no as i128,"RANK_WHOLE_ACCOUNT_POSITION_MISMATCH")?;}
 let o=&f.open_orders.data;need(o["coverage"]["complete"]==true&&o["pagination"]["hasMore"]==false&&o["pagination"].get("nextCursor")==Some(&Value::Null)&&o["data"].as_array().is_some_and(Vec::is_empty),"RANK_PHYSICAL_OPEN_ORDERS")?;
 collateral.verify_root(b.root)?;let cr=collateral.receipt();let start=clock(&cr.request_started_at)?;let end=clock(&cr.received_at)?;need(cr.endpoint==format!("/portfolio/collateral?tournamentId={tid}")&&cr.status==200&&start<=end&&end<=at&&at-start<=20000&&cr.data.get("pagination").is_none()&&cr.data.get("coverage").is_none_or(|v|v["complete"]==true),"RANK_COLLATERAL_CLOCK_OR_COVERAGE")?;until=until.min(start.checked_add(20000).ok_or("RANK_CLOCK")?);
 let debt=relationships::parse_collateral(&cr.data,scope)?;need(debt.available_advance_units==power.available_advance_units&&debt.components.iter().all(|v|f.cash_balance.data["id"]==v.profile_id)&&amount(&r.campaign["account"]["modeledTerminalDebt"])? >=debt.outstanding_liability_units,"RANK_LIABILITY_MODEL_OR_PROFILE")?;
 let wrapper=json!({"source":"official_sig_api_v1","kind":"book","tournamentId":tid,"marketId":market,"exchangeId":ex,"endpoint":f.book.endpoint,"status":f.book.status,"requestStartedAt":f.book.request_started_at,"receivedAt":f.book.received_at,"data":f.book.data});let events=moloch_replay::source::receipt_events(&wrapper)?;let expected=events.iter().find(|e|e["type"]=="book").ok_or("RANK_ORIGINAL_BOOK")?;let mut current=bank.engine().live_package_books().into_iter().find(|b|b["exchangeId"]==ex).ok_or("RANK_ACCEPTED_BOOK")?;if current["invalid"]==false{current.as_object_mut().ok_or("RANK_BOOK")?.remove("invalid");}need(current==expected["book"],"RANK_BOOK_NOT_ADMITTED")?;
 let age=bank.engine().policy()["bookAgeMs"].as_u64().ok_or("RANK_BOOK_POLICY")?;let mut target_scope=scope.clone();target_scope.exchanges.retain(|key,_|key==ex);let(books,_)=crate::package_admission::native_books(bank,&target_scope,at,age)?;let book=books.get(ex).ok_or("RANK_BOOK_NOT_FRESH")?;until=until.min(book.observed_ms.checked_add(age).ok_or("RANK_CLOCK")?).min(book.source.received_ms.checked_add(age).ok_or("RANK_CLOCK")?);
 need(at<until,"RANK_FACTS_EXPIRED")?;Ok((retained,until))
}
pub struct Reservation{event:Value,boundary:Value,review:Reviewed,until:u64,receipts:Vec<RetainedReceipt>,collateral:RetainedReceipt}
impl Reservation{
 pub fn event(&self)->&Value{&self.event}
 pub fn valid_until_ms(&self)->u64{self.until}
 pub fn validate_current(&self,bank:&Session,b:&Boundary<'_>,at:u64)->Result<()>{self.review.current(at)?;need(at<self.until&&collateral_feedback::allocation_boundary(bank,b,i64::try_from(at)?)?==self.boundary,"RANK_RESERVATION_BOUNDARY_CHANGED")?;for r in &self.receipts{r.verify()?;}self.collateral.verify()?;Ok(())}
}
/// No writes. The root worker may accept event() ONLY after validate_current,
/// then native admission and activation/final seven ORIGINAL facts. Native
/// admission recomputes the evaluation; this plan is not an SDK capability.
pub fn prepare(bank:&Session,b:&Boundary<'_>,review:&Reviewed,f:&SnapshotSet,collateral:&RetainedReceipt,at:u64)->Result<Reservation>{
 let before=collateral_feedback::allocation_boundary(bank,b,i64::try_from(at)?)?;let(receipts,until)=facts(bank,b,review,f,collateral,at)?;
 let evaluation=bank.engine().evaluate_rank_campaign(&review.campaign,i64::try_from(at)?)?;
 let policy=bank.engine().policy();let expires=at.checked_add(policy["lifetimeMs"].as_u64().ok_or("RANK_POLICY")?).ok_or("RANK_CLOCK")?.min(review.until).min(clock(policy["entryStopAt"].as_str().ok_or("RANK_ENTRY_DEADLINE")?)?);
 let event=json!({"type":"reviewed_rank_campaign_reserved_v1","source":"moloch_live_dispatcher","receivedAt":daily_windows::iso(at)?,"processedAt":daily_windows::iso(at)?,"bankScope":{"runId":bank.live_account_seed().ok_or("RANK_LIVE_BANK")?["runId"],"bankManifestHash":bank.manifest_hash(),"tournamentId":b.transport.scope().tournament_id},"bankHead":bank.durable_head(),"expectedReportSha256":hash(&bank.engine().report()?)?,"campaignSha256":review.canonical_sha,"campaign":review.campaign,"currentBookSha256":evaluation["currentBookSha256"],"originSha256":hash(&json!({"reviewRawSha256":review.raw_sha,"boundary":before,"originalFacts":receipts.iter().map(|r|r.evidence()).collect::<Result<Vec<_>>>()?,"collateral":collateral.evidence()?}))?,"evaluation":evaluation,"expiresAt":daily_windows::iso(expires)?});
 let cr=RetainedReceipt::open(b.root,collateral.receipt(),ReceiptStore::Ordinary)?;
 let result=Reservation{event,boundary:before,review:review.clone(),until,receipts,collateral:cr};result.validate_current(bank,b,at)?;Ok(result)
}
/// Opaque process-local physical proof. JSON research records cannot construct it.
pub struct PhysicalDecision{head:Value,quote_sha:String,ledger_sha:String,scope_sha:String,spec_sha:String,permit_sha:String,qid:u64,minted:u64,until:u64,review:Reviewed,receipts:Vec<RetainedReceipt>,collateral:RetainedReceipt,dispatch:DispatchPin,evaluation:Value,ownership:Option<std::sync::Arc<crate::fundamental_policy::RankInventoryOwnership>>}
/// Exact full JSON is checked once, then held by immutable shared owners.
/// No deserialization, mutation or caller-supplied digest can create this seal.
/// Physical checks still bind the same decision, quote and ledger allocations
/// and recheck every original clock; external files are checked by authorize.
pub struct FrozenState{decision:std::sync::Arc<PhysicalDecision>,quote:std::sync::Arc<Value>,ledger:std::sync::Arc<Value>,qid:u64}
impl FrozenState{
 pub fn validate(&self,decision:&std::sync::Arc<PhysicalDecision>,quote:&std::sync::Arc<Value>,ledger:&std::sync::Arc<Value>,qid:u64,at:u64)->Result<()>{
  need(std::sync::Arc::ptr_eq(decision,&self.decision)&&same_frozen_values(&self.quote,&self.ledger,self.qid,quote,ledger,qid),"RANK_PHYSICAL_FROZEN_CHANGED")?;
  decision.current(at)
 }
}
fn same_frozen_values(q:&std::sync::Arc<Value>,l:&std::sync::Arc<Value>,id:u64,other_q:&std::sync::Arc<Value>,other_l:&std::sync::Arc<Value>,other_id:u64)->bool{
 id==other_id&&std::sync::Arc::ptr_eq(q,other_q)&&std::sync::Arc::ptr_eq(l,other_l)
}
fn physical_clock(minted:u64,until:u64,at:u64)->Result<()>{need(at>=minted,"RANK_PHYSICAL_CLOCK_REGRESSED")?;need(at<until,"RANK_PHYSICAL_FACTS_EXPIRED")}
impl PhysicalDecision{
 pub fn native_head(&self)->&Value{&self.head}
 pub fn allows_managed_inventory(&self)->bool{self.ownership.is_some()&&self.review.managed_inventory().is_some()}
 pub fn cohort_ownership_tag(&self)->Option<&Value>{self.ownership.as_ref().map(|o|o.durable_tag())}
 /// Full cohort/financial checks precede minting this opaque decision. Between
 /// mint and dispatch the sole worker does not mutate that bank or sidecar.
 /// Bind the new durable intent to the same owner; SDK guards still validate
 /// the frozen quote/ledger, current clocks, source pins and owner lease.
 pub fn validate_intent_ownership_tag(&self,sidecar:Option<&Value>,intent:&Value)->Result<()>{
  need(self.allows_managed_inventory(),"RANK_MANAGED_INTENT_PROOF_REQUIRED")?;
  self.ownership.as_ref().ok_or("RANK_MANAGED_INTENT_PROOF_REQUIRED")?.validate_intent_tag(sidecar,intent)
 }
 pub fn expires_at_ms(&self)->u64{self.until}
 pub fn evaluation(&self)->&Value{&self.evaluation}
 pub fn validate_contract(&self,scope:&Scope,r:&Receipt)->Result<()>{validate_contract(&self.review,scope,r)}
 fn current(&self,at:u64)->Result<()>{self.review.current(at)?;physical_clock(self.minted,self.until,at)}
 pub fn freeze(self:&std::sync::Arc<Self>,quote:std::sync::Arc<Value>,ledger:std::sync::Arc<Value>,qid:u64,at:u64)->Result<FrozenState>{
  self.validate_frozen(&quote,&ledger,qid,at)?;Ok(FrozenState{decision:self.clone(),quote,ledger,qid})
 }
 pub fn validate_frozen(&self,q:&Value,ledger:&Value,qid:u64,at:u64)->Result<()>{self.current(at)?;need(qid==self.qid&&hash(q)?==self.quote_sha&&hash(ledger)?==self.ledger_sha,"RANK_PHYSICAL_FROZEN_CHANGED")?;if let Some(o)=&self.ownership{o.validate_frozen(q,ledger,qid,at)?;}Ok(())}
 pub fn authorize(&self,scope:&Scope,spec:&RequestSpec,permit:&Permit,at:u64)->Result<()>{self.current(at)?;need(hash(scope)?==self.scope_sha&&hash(spec)?==self.spec_sha&&hash(permit)?==self.permit_sha,"RANK_PHYSICAL_REQUEST_CHANGED")?;self.dispatch.validate()?;if let Some(o)=&self.ownership{o.authorize(scope,spec,permit,at)?;}for r in &self.receipts{r.verify()?;}self.collateral.verify()?;Ok(())}
}
/// Mint ONLY after activation and new seven facts. No intent/attempt may exist.
/// ProductionAuthority must additionally enforce its normal account stream,
/// STOP/control/generation/lease/binary/freshness and SDK physical guards.
pub fn mint(bank:&Session,b:&Boundary<'_>,review:&Reviewed,qid:u64,f:&SnapshotSet,collateral:&RetainedReceipt,spec:&RequestSpec,permit:&Permit,at:u64)->Result<PhysicalDecision>{
 mint_inner(bank,b,review,qid,f,collateral,spec,permit,at,None)
}
/// Ownership is supplemental attribution, not a positive-alpha decision.
pub fn mint_managed(bank:&Session,b:&Boundary<'_>,review:&Reviewed,qid:u64,f:&SnapshotSet,collateral:&RetainedReceipt,spec:&RequestSpec,permit:&Permit,at:u64,ownership:crate::fundamental_policy::RankInventoryOwnership)->Result<PhysicalDecision>{
 mint_inner(bank,b,review,qid,f,collateral,spec,permit,at,Some(std::sync::Arc::new(ownership)))
}
fn mint_inner(bank:&Session,b:&Boundary<'_>,review:&Reviewed,qid:u64,f:&SnapshotSet,collateral:&RetainedReceipt,spec:&RequestSpec,permit:&Permit,at:u64,ownership:Option<std::sync::Arc<crate::fundamental_policy::RankInventoryOwnership>>)->Result<PhysicalDecision>{
 need(review.managed_inventory().is_some()==ownership.is_some(),"RANK_COHORT_OPAQUE_OWNERSHIP_REQUIRED")?;

 let before=collateral_feedback::allocation_boundary(bank,b,i64::try_from(at)?)?;need(b.ordinary.never_attempted(qid)&&!b.ordinary.intents.contains_key(&qid),"RANK_ORIGINAL_INTENT_EXISTS")?;
 let(receipts,mut until)=facts(bank,b,review,f,collateral,at)?;let q=crate::order_snapshot::snapshot(bank).records().iter().find(|q|q["id"]==qid).cloned().ok_or("RANK_NATIVE_QUOTE")?;let a=review.action();let scope=b.transport.scope();
 need(q["reviewedRankCampaign"]["campaignSha256"]==review.canonical_sha&&q["marketId"]==a["marketId"]&&q["exchangeId"]==a["exchangeId"]&&q["side"]==a["side"]&&q["action"]==(if a["action"]=="BUY"{"buy"}else{"sell"})&&q["quantity"]==a["quantity"]&&q["price"]==a["limitPrice"]&&q["filled"]==0&&q["retired"]==false,"RANK_NATIVE_QUOTE_CHANGED")?;
 let evaluation=bank.engine().evaluate_reserved_rank_campaign(qid,&review.campaign,i64::try_from(at)?)?;
 let expected_body=json!({"idempotencyKey":format!("{}:{qid}",permit.run_id),"exchangeId":q["exchangeId"],"side":if q["side"]=="YES"{"yes"}else{"no"},"action":q["action"],"quantity":q["quantity"],"price":serde_json::from_str::<Value>(q["price"].as_str().ok_or("RANK_QUOTE_PRICE")?)?,"expirationDate":daily_windows::iso(q["expiresAt"].as_u64().ok_or("RANK_QUOTE_CLOCK")?)?,"tournamentId":scope.tournament_id});
 need(spec.method=="POST"&&spec.endpoint=="/orders"&&spec.body.as_ref()==Some(&expected_body)&&spec.body_json.as_ref().is_some_and(|v|serde_json::from_str::<Value>(v).ok().as_ref()==Some(&expected_body))&&permit.owner=="Codex"&&bank.live_account_seed().is_some_and(|s|s["runId"]==permit.run_id)&&permit.intent_id==qid.to_string()&&u64::from(permit.maximum_quantity)==q["quantity"].as_u64().ok_or("RANK_QUANTITY")?&&decimal_units(&permit.maximum_cost)?==amount(&q["price"])?*i128::from(permit.maximum_quantity),"RANK_PHYSICAL_SPEC_OR_PERMIT")?;
 if let Some(o)=&ownership{o.validate_frozen(&q,&bank.engine().report()?,qid,at)?;o.authorize(scope,spec,permit,at)?;need(o.native_head()==&bank.durable_head(),"RANK_COHORT_NATIVE_HEAD_CHANGED")?;until=until.min(o.expires_at_ms());}
 until=until.min(q["expiresAt"].as_u64().ok_or("RANK_QUOTE_CLOCK")?).min(permit.expires_at_ms);let dispatch=DispatchPin::new(b)?;need(collateral_feedback::allocation_boundary(bank,b,i64::try_from(at)?)?==before,"RANK_BOUNDARY_CHANGED")?;
 let cr=RetainedReceipt::open(b.root,collateral.receipt(),ReceiptStore::Ordinary)?;
 Ok(PhysicalDecision{head:bank.durable_head(),quote_sha:hash(&q)?,ledger_sha:hash(&bank.engine().report()?)?,scope_sha:hash(scope)?,spec_sha:hash(spec)?,permit_sha:hash(permit)?,qid,minted:at,until,review:review.clone(),receipts,collateral:cr,dispatch,evaluation,ownership})
}

#[cfg(test)]mod tests{
 use super::*;
 #[test]fn frozen_allocations_cannot_be_changed_or_replaced_with_equal_json(){
  use std::sync::Arc;
  let mut q=Arc::new(json!({"id":7,"review":{"large":"original"}}));let l=Arc::new(json!({"cash":"10.000000"}));let pinned_q=q.clone();let pinned_l=l.clone();
  assert!(Arc::get_mut(&mut q).is_none());
  assert!(same_frozen_values(&pinned_q,&pinned_l,7,&q,&l,7));
  assert!(!same_frozen_values(&pinned_q,&pinned_l,7,&Arc::new((*q).clone()),&l,7));
  assert!(!same_frozen_values(&pinned_q,&pinned_l,7,&q,&Arc::new((*l).clone()),7));
  assert!(!same_frozen_values(&pinned_q,&pinned_l,7,&q,&l,8));
  Arc::make_mut(&mut q)["id"]=json!(8);
  assert_eq!(pinned_q["id"],7);assert!(!same_frozen_values(&pinned_q,&pinned_l,7,&q,&l,7));
 }
 #[test]fn final_clock_is_exclusive_and_distinguishes_regression_from_expiry(){
  assert!(physical_clock(100,110,100).is_ok());assert!(physical_clock(100,110,109).is_ok());
  assert_eq!(physical_clock(100,110,110).unwrap_err().to_string(),"RANK_PHYSICAL_FACTS_EXPIRED");
  assert_eq!(physical_clock(100,110,99).unwrap_err().to_string(),"RANK_PHYSICAL_CLOCK_REGRESSED");
 }
 fn fixture()->(Vec<u8>,String,BTreeSet<String>,Vec<u8>){
  let original=b"Synthetic retained research source, not an official account receipt".to_vec();
  let c=json!({"schemaVersion":1,"objective":"declared_first_place_rank_risk","reviewedAtMs":1000,"validUntilMs":10000,"action":{"exchangeId":"101"},"sources":[{"sha256":rawhash(&original),"observedAtMs":500,"maximumAgeMs":2000,"assumptionLabel":"Synthetic source clock"}]});
  let raw=serde_json::to_vec(&c).unwrap();let sha=rawhash(&raw);let approved=BTreeSet::from([sha.clone()]);(raw,sha,approved,original)
 }
 #[test]fn retained_source_bytes_and_explicit_raw_approval_are_both_required(){
  let(raw,sha,approved,original)=fixture();let source_sha=rawhash(&original);
  let originals=[Original{sha256:&source_sha,bytes:&original}];
  let r=Reviewed::load(&raw,&sha,&approved,&originals,1000).unwrap();assert_eq!(r.raw_sha256(),sha);assert_eq!(r.exchange_id(),"101");
  assert!(Reviewed::load(&raw,&sha,&BTreeSet::new(),&originals,1000).is_err());
  assert!(Reviewed::load(&raw,&sha,&approved,&[],1000).is_err());
  let changed=[Original{sha256:&source_sha,bytes:b"changed source"}];assert!(Reviewed::load(&raw,&sha,&approved,&changed,1000).is_err());
  let mut changed=raw.clone();changed.push(b' ');assert!(Reviewed::load(&changed,&sha,&approved,&originals,1000).is_err());
 }
 #[test]fn original_source_expiry_is_not_reset_to_review_or_capture_clock(){
  let(raw,sha,approved,original)=fixture();let source_sha=rawhash(&original);let originals=[Original{sha256:&source_sha,bytes:&original}];
  let r=Reviewed::load(&raw,&sha,&approved,&originals,2499).unwrap();assert!(r.current(2499).is_ok());assert!(r.current(2500).is_err());assert!(r.current(999).is_err());
  assert!(Reviewed::load(&raw,&sha,&approved,&originals,2500).is_err());
  let mut c:Value=serde_json::from_slice(&raw).unwrap();c["sources"][0]["observedAtMs"]=json!(1001);let future=serde_json::to_vec(&c).unwrap();let future_sha=rawhash(&future);
  assert!(Reviewed::load(&future,&future_sha,&BTreeSet::from([future_sha.clone()]),&originals,1000).is_err());
 }
 #[test]fn duplicate_source_membership_cannot_stand_in_for_a_complete_original_set(){
  let(raw,_,_,original)=fixture();let mut c:Value=serde_json::from_slice(&raw).unwrap();let duplicate=c["sources"][0].clone();c["sources"].as_array_mut().unwrap().push(duplicate);
  let raw=serde_json::to_vec(&c).unwrap();let sha=rawhash(&raw);let source_sha=rawhash(&original);let originals=[Original{sha256:&source_sha,bytes:&original},Original{sha256:&source_sha,bytes:&original}];
  assert!(Reviewed::load(&raw,&sha,&BTreeSet::from([sha.clone()]),&originals,1000).is_err());
 }
}
