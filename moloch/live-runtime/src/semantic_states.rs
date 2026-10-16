//! Reviewed finite contractual state space. Titles and candidate caucus labels
//! never imply a payout. This is NOT a settlement-floor certificate: source
//! hashes bind an explicitly approved interpretation, not its completeness.
use crate::feedback::Result;
use baal_transport_service::{Receipt,Scope};
use serde::{Deserialize,Serialize};
use serde_json::Value;
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet};
pub const UNIT:i128=1_000_000;
fn need(b:bool,s:&'static str)->Result<()>{if b{Ok(())}else{Err(s.into())}}
pub(crate) fn digest<T:Serialize+?Sized>(v:&T)->Result<String>{Ok(format!("{:x}",Sha256::digest(serde_json::to_vec(v)?)))}
fn raw_digest(b:&[u8])->String{format!("{:x}",Sha256::digest(b))}
fn hex(s:&str)->bool{s.len()==64&&s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))}
fn atom(s:&str)->bool{!s.is_empty()&&s.len()<=128&&s.bytes().all(|b|b.is_ascii_alphanumeric()||b"-_:".contains(&b))}
fn numeric(s:&str)->bool{!s.is_empty()&&!s.starts_with('0')&&s.bytes().all(|b|b.is_ascii_digit())}
#[derive(Clone,Copy,Debug,Serialize,Deserialize,PartialEq,Eq,PartialOrd,Ord)]
#[serde(rename_all="UPPERCASE")]
pub enum Side{Yes,No}
#[derive(Clone,Copy,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(rename_all="camelCase")]
pub enum StateKind{PartyWinner,FusionWinner,IndependentWinner,Replacement,Runoff,NotApplicable,Cancelled}
#[derive(Clone,Copy,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(rename_all="camelCase")]
pub enum PayoutRule{ResolvedYes,ResolvedNo,RefundPrincipal}
impl PayoutRule{
 /// Refunds return ORIGINAL attributable principal, not a universal 0/1 or
 /// an inferred complement. Fees are deliberately separate and not refunded.
 pub fn payout(self,side:Side,quantity:u64,principal:i128)->Result<i128>{
  need(principal>=0&&principal<=(quantity as i128).checked_mul(UNIT).ok_or("SEMANTIC_RANGE")?,"SEMANTIC_PRINCIPAL")?;
  match self{Self::RefundPrincipal=>Ok(principal),Self::ResolvedYes=>Ok(if side==Side::Yes{quantity as i128*UNIT}else{0}),Self::ResolvedNo=>Ok(if side==Side::No{quantity as i128*UNIT}else{0})}
 }
}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Contract{
 pub market_id:String,pub exchange_id:String,pub contract_id:String,
 pub official_root_sha256:String,pub original_node_sha256:String,
 /// Source-bound interpretation is explicit even when official details omit
 /// fusion, affiliation, replacement, runoff or refund implementation rules.
 pub predicate:String,pub unresolved_exceptions:Vec<String>,
}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct State{
 pub id:String,pub kind:StateKind,pub outcomes:Vec<PayoutRule>,
 pub source_sha256:Vec<String>,pub assumption_label:String,
}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Source{pub sha256:String,pub observed_at_ms:u64,pub maximum_age_ms:u64}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Review{
 pub version:u32,pub review_id:String,pub run_id:String,pub bank_manifest_sha256:String,
 pub tournament_id:String,pub tournament_slug:String,
 pub scenario_source_sha256:String,pub reviewed_at_ms:u64,pub valid_until_ms:u64,
 pub contracts:Vec<Contract>,pub states:Vec<State>,pub sources:Vec<Source>,
 pub excluded_states:Vec<String>,pub probabilities_are_reviewed_assumptions:bool,
 /// Explicit mixture mass for exceptional states, never learned implicitly
 /// from ordinary DEM/REP Atlas draws. Ordinary mass is the exact remainder.
 pub exception_weights_ppm:BTreeMap<String,u32>,
}
pub struct OriginalSource<'a>{pub sha256:&'a str,pub bytes:&'a [u8]}
/// No Deserialize/public fields. Created only after exact original review,
/// approved review hash and every scoped original contract root are verified.
pub struct Compiled{review:Review,sha:String,source_until:u64}
impl Compiled{
 pub fn contracts(&self)->&[Contract]{&self.review.contracts}
 pub fn states(&self)->&[State]{&self.review.states}
 pub fn review_sha256(&self)->&str{&self.sha}
 pub fn scenario_source_sha256(&self)->&str{&self.review.scenario_source_sha256}
 pub fn run_id(&self)->&str{&self.review.run_id}
 pub fn bank_manifest_sha256(&self)->&str{&self.review.bank_manifest_sha256}
 pub fn tournament_id(&self)->&str{&self.review.tournament_id}
 pub fn tournament_slug(&self)->&str{&self.review.tournament_slug}
 pub fn valid_until_ms(&self)->u64{self.source_until}
 pub fn reviewed_at_ms(&self)->u64{self.review.reviewed_at_ms}
 pub fn excluded_states(&self)->&[String]{&self.review.excluded_states}
 pub fn exception_weights_ppm(&self)->&BTreeMap<String,u32>{&self.review.exception_weights_ppm}
 pub fn validate_time(&self,now:u64)->Result<()>{need(now>=self.review.reviewed_at_ms&&now<self.source_until,"SEMANTIC_REVIEW_STALE")}
 /// Fresh ORIGINAL node receipts are still mandatory for any future physical
 /// path; compilation of historical evidence never extends their clocks.
 pub fn validate_current_contracts(&self,scope:&Scope,receipts:&BTreeMap<String,Receipt>,now:u64)->Result<()>{
  self.validate_time(now)?;need(scope.tournament_id==self.review.tournament_id&&scope.tournament_slug==self.review.tournament_slug&&receipts.len()==self.review.contracts.len(),"SEMANTIC_CURRENT_SCOPE")?;
  for c in &self.review.contracts{
   need(scope.exchanges.get(&c.exchange_id)==Some(&c.market_id),"SEMANTIC_CURRENT_SCOPE")?;
   let r=receipts.get(&c.exchange_id).ok_or("SEMANTIC_CURRENT_MISSING")?;
   let start=crate::daily_windows::clock(&r.request_started_at)?;let end=crate::daily_windows::clock(&r.received_at)?;
   need(start<=end&&end<=now&&now-start<=10_000,"SEMANTIC_CURRENT_STALE")?;
   need(r.status==200&&r.endpoint==format!("/markets/{}/nodes?tournamentId={}",c.market_id,self.review.tournament_id),"SEMANTIC_CURRENT_ENDPOINT")?;
   validate_node(c,&r.data)?;
  }Ok(())
 }
}
fn validate_node(c:&Contract,data:&Value)->Result<()>{
 need(data["market_id"]==c.market_id&&data["root"]["node_type"]=="contract"&&data["root"]["node_id"]==c.contract_id&&data["root"]["contract_id"]==c.contract_id
  &&digest(&data["root"])?==c.official_root_sha256,"SEMANTIC_CONTRACT_CHANGED")
}
pub fn compile(bytes:&[u8],expected_sha:&str,approved:&BTreeSet<String>,originals:&[OriginalSource<'_>],now:u64)->Result<Compiled>{
 need(!bytes.is_empty()&&bytes.len()<=2*1024*1024&&hex(expected_sha)&&raw_digest(bytes)==expected_sha&&approved.contains(expected_sha),"SEMANTIC_REVIEW_NOT_APPROVED")?;
 let r:Review=serde_json::from_slice(bytes)?;
 need(r.version==1&&atom(&r.review_id)&&atom(&r.run_id)&&hex(&r.bank_manifest_sha256)&&!r.tournament_id.is_empty()&&!r.tournament_slug.is_empty()
  &&hex(&r.scenario_source_sha256)&&r.reviewed_at_ms<r.valid_until_ms&&r.valid_until_ms-r.reviewed_at_ms<=366*86_400_000
  &&r.probabilities_are_reviewed_assumptions&&(1..=6).contains(&r.contracts.len())&&(1..=4096).contains(&r.states.len())
  &&(1..=32).contains(&r.sources.len())&&originals.len()==r.sources.len()&&r.excluded_states.len()<=32,"SEMANTIC_REVIEW_SHAPE")?;
 let mut source_map=BTreeMap::new();let mut until=r.valid_until_ms;
 for s in &r.sources{
  need(hex(&s.sha256)&&s.observed_at_ms<=r.reviewed_at_ms&&s.maximum_age_ms>0&&s.maximum_age_ms<=366*86_400_000,"SEMANTIC_SOURCE_CLOCK")?;
  let matches:Vec<_>=originals.iter().filter(|o|o.sha256==s.sha256).collect();
  need(matches.len()==1&&!matches[0].bytes.is_empty()&&matches[0].bytes.len()<=8*1024*1024&&raw_digest(matches[0].bytes)==s.sha256&&source_map.insert(s.sha256.clone(),matches[0].bytes).is_none(),"SEMANTIC_ORIGINAL_SOURCE")?;
  until=until.min(s.observed_at_ms.checked_add(s.maximum_age_ms).ok_or("SEMANTIC_RANGE")?);
 }
 let mut exchanges=BTreeSet::new();
 for c in &r.contracts{
  need(numeric(&c.market_id)&&numeric(&c.exchange_id)&&numeric(&c.contract_id)&&hex(&c.official_root_sha256)&&hex(&c.original_node_sha256)
   &&exchanges.insert(&c.exchange_id)&&!c.predicate.trim().is_empty()&&c.predicate.len()<=4096&&c.unresolved_exceptions.len()<=32,"SEMANTIC_CONTRACT_SCOPE")?;
  let node:Value=serde_json::from_slice(source_map.get(&c.original_node_sha256).ok_or("SEMANTIC_ORIGINAL_NODE_MISSING")?)?;
  need(node["status"]==200&&node["endpoint"]==format!("/markets/{}/nodes?tournamentId={}",c.market_id,r.tournament_id),"SEMANTIC_ORIGINAL_NODE_ENDPOINT")?;
  validate_node(c,&node["data"])?;
  let start=crate::daily_windows::clock(node["requestStartedAt"].as_str().ok_or("SEMANTIC_NODE_CLOCK")?)?;
  let end=crate::daily_windows::clock(node["receivedAt"].as_str().ok_or("SEMANTIC_NODE_CLOCK")?)?;
  let observed=r.sources.iter().find(|s|s.sha256==c.original_node_sha256).ok_or("SEMANTIC_NODE_SOURCE")?.observed_at_ms;
  need(start<=end&&end<=observed,"SEMANTIC_NODE_CLOCK")?;
 }
 let mut states=BTreeSet::new();
 for s in &r.states{
  need(atom(&s.id)&&states.insert(s.id.clone())&&s.outcomes.len()==r.contracts.len()&&!s.assumption_label.trim().is_empty()&&s.assumption_label.len()<=4096
   &&!s.source_sha256.is_empty()&&s.source_sha256.len()<=32&&s.source_sha256.iter().all(|h|source_map.contains_key(h)),"SEMANTIC_STATE_BINDING")?;
  need(!matches!(s.kind,StateKind::NotApplicable|StateKind::Cancelled)||s.outcomes.contains(&PayoutRule::RefundPrincipal),"SEMANTIC_REFUND_STATE")?;
 }
 let mut exceptional=0u64;for(id,w)in &r.exception_weights_ppm{need(*w>0&&states.contains(id)&&r.states.iter().any(|s|&s.id==id&&s.kind!=StateKind::PartyWinner),"SEMANTIC_EXCEPTION_WEIGHT")?;exceptional=exceptional.checked_add(*w as u64).ok_or("SEMANTIC_RANGE")?;}need(exceptional<1_000_000,"SEMANTIC_EXCEPTION_WEIGHT")?;
 let c=Compiled{review:r,sha:expected_sha.into(),source_until:until};c.validate_time(now)?;Ok(c)
}
