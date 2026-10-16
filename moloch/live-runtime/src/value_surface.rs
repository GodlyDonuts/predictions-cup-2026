//! Ordinary election-outcome posterior, explicit reviewed contract mapping and
//! current official depth. This is a read-only model input, never order authority.
//! Atlas does not estimate affiliation changes, refunds, or venue void behavior.
use crate::{daily_windows, feedback::Result, fundamental_policy::{self, ReviewInput, Reviewed, SourceBytes, ValuationObjective}, package_admission};
use baal_transport_service::{decimal_units, Receipt, Scope};
use moloch_relationship_arbitrage::Book;
use moloch_replay::{session::Session, Side};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const UNIT:i128=1_000_000;
fn need(b:bool,e:&'static str)->Result<()>{if b{Ok(())}else{Err(e.into())}}
pub fn sha256(bytes:&[u8])->String{format!("{:x}",Sha256::digest(bytes))}
fn digest<T:Serialize>(v:&T)->Result<String>{Ok(sha256(&serde_json::to_vec(v)?))}
fn hex(s:&str)->bool{s.len()==64&&s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))}
fn atom(s:&str)->bool{!s.is_empty()&&s.len()<=256&&!s.chars().any(char::is_control)}
fn integer_only(v:&Value)->bool{match v{Value::Number(n)=>n.as_i64().is_some()||n.as_u64().is_some(),Value::Array(a)=>a.iter().all(integer_only),Value::Object(o)=>o.values().all(integer_only),_=>true}}
#[derive(Clone,Copy,Debug,PartialEq,Eq,Serialize,Deserialize)]
pub enum RaceOutcome{#[serde(rename="DEM")]Dem,#[serde(rename="REP")]Rep}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Race{pub race_id:String,pub state:String,pub office:String}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Scenario{pub id:String,pub weight_ppm:u32,pub race_outcomes:Vec<RaceOutcome>}
#[derive(Clone,Debug)]
pub struct Surface{source_sha:String,model_sha:String,observed:u64,expires:u64,training_cutoff:u64,qualification:Value,races:Vec<Race>,scenarios:Vec<Scenario>}
impl Surface{
 /// `trusted_artifacts` is a separately pinned execution-owner allowlist of
 /// ORIGINAL FILE hashes. Neither self-hashing nor `qualification` self-approves.
 pub fn load(bytes:&[u8],expected_sha:&str,trusted_artifacts:&BTreeSet<String>,now:u64)->Result<Self>{
  need(!bytes.is_empty()&&bytes.len()<=16*1024*1024&&hex(expected_sha)&&sha256(bytes)==expected_sha&&trusted_artifacts.contains(expected_sha),"SURFACE_ORIGINAL_ARTIFACT")?;
  let mut v:Value=serde_json::from_slice(bytes)?;need(integer_only(&v),"SURFACE_INTEGER_CANONICALIZATION")?;
  need(v["schemaVersion"]==1&&v["objective"]=="ordinary_terminal_race_outcome","SURFACE_SCHEMA_OBJECTIVE")?;
  let model=v["modelSha256"].as_str().filter(|s|hex(s)).ok_or("SURFACE_MODEL_HASH")?.to_owned();
  v.as_object_mut().ok_or("SURFACE_SCHEMA")?.remove("modelSha256");need(digest(&v)?==model,"SURFACE_MODEL_HASH")?;
  let observed=v["observedAtMs"].as_u64().ok_or("SURFACE_CLOCK")?;let expires=v["expiresAtMs"].as_u64().ok_or("SURFACE_CLOCK")?;let training=v["trainingCutoffMs"].as_u64().ok_or("SURFACE_CLOCK")?;
  need(training<=observed&&observed<=now&&now<expires,"SURFACE_NOT_CURRENT")?;
  let q=v["qualification"].clone();need(matches!(q["kind"].as_str(),Some("research"|"qualified")),"SURFACE_QUALIFICATION")?;
  let races:Vec<Race>=serde_json::from_value(v["races"].clone())?;let scenarios:Vec<Scenario>=serde_json::from_value(v["scenarios"].clone())?;
  need((1..=256).contains(&races.len())&&(2..=4096).contains(&scenarios.len()),"SURFACE_SIZE")?;
  let mut ids=BTreeSet::new();for r in &races{need(atom(&r.race_id)&&atom(&r.state)&&atom(&r.office)&&ids.insert(&r.race_id),"SURFACE_RACE")?;}
  let mut ids=BTreeSet::new();let mut outcomes=BTreeSet::new();let mut total=0u32;
  for s in &scenarios{need(atom(&s.id)&&ids.insert(&s.id)&&s.weight_ppm>0&&s.race_outcomes.len()==races.len(),"SURFACE_SCENARIO")?;need(outcomes.insert(serde_json::to_string(&s.race_outcomes)?),"SURFACE_DUPLICATE_OUTCOME")?;total=total.checked_add(s.weight_ppm).ok_or("SURFACE_WEIGHT")?;}
  need(total==1_000_000,"SURFACE_WEIGHT")?;
  let sources=v["sources"].as_array().filter(|a|!a.is_empty()&&a.len()<=1024).ok_or("SURFACE_SOURCE")?;
  need(sources.iter().all(|s|s["sha256"].as_str().is_some_and(hex)),"SURFACE_SOURCE")?;
  Ok(Self{source_sha:expected_sha.into(),model_sha:model,observed,expires,training_cutoff:training,qualification:q,races,scenarios})
 }
 pub fn scenarios(&self)->&[Scenario]{&self.scenarios}
 pub fn races(&self)->&[Race]{&self.races}
 pub fn source_sha256(&self)->&str{&self.source_sha}
 pub fn model_sha256(&self)->&str{&self.model_sha}
 pub fn observed_at_ms(&self)->u64{self.observed}
 pub fn training_cutoff_ms(&self)->u64{self.training_cutoff}
 pub fn valid_until_ms(&self)->u64{self.expires}
 pub fn qualification(&self)->&Value{&self.qualification}
 /// A recorded qualification is descriptive; it cannot mint trading authority.
 pub fn is_research(&self)->bool{self.qualification["kind"]!="qualified"}
}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct FactorLoading{pub factor_id:String,/// Change in micro-payout per unit factor move.
 pub loading_micros:i128}
/// Independent approval is needed because a contract review alone does not
/// certify a race ID, Atlas version or factor loading. Hash this full object.
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct MappingReview{pub version:u32,pub model_source_sha256:String,pub review_sha256:String,pub race_id:String,pub party:RaceOutcome,pub maximum_model_deviation_micros:u32,pub loadings:Vec<FactorLoading>,pub assumption_label:String}
pub fn mapping_sha256(m:&MappingReview)->Result<String>{digest(m)}
pub struct ContractReview<'a>{pub input:ReviewInput,pub originals:&'a [SourceBytes<'a>],pub current_receipt:&'a Receipt,pub mapping:MappingReview}
#[derive(Clone,Debug)]
pub struct BoundContract{
 pub market_id:String,pub exchange_id:String,pub contract_id:String,pub side:Side,
 pub official_root_sha256:String,pub race_id:String,pub party:RaceOutcome,
 pub expected_payout_micros:i128,pub ordinary_payout_micros:i128,
 pub exceptional_allowance_micros:i128,pub minimum_exceptional_payout_micros:i128,
 pub review_sha256:String,pub mapping_sha256:String,pub loadings:Vec<FactorLoading>,
 pub maximum_principal:i128,pub maximum_quantity:u64,pub margin_per_share:i128,
 pub scheduled_settlement_at_ms:u64,pub reviewed_at_ms:u64,pub valid_until_ms:u64,
 pub allowed_modes:Vec<crate::alpha_proposal::Mode>,
}
#[derive(Clone,Debug,Serialize)]
pub struct BoundScenario{pub id:String,pub weight_ppm:u32,pub payout_micros:Vec<i128>}
/// Matrix moments are exact rationals. E[X_i] is mean_numerators[i]/1e6;
/// Cov(X_i,X_j) is covariance_numerators[i][j]/1e12, in micro-payout².
#[derive(Clone,Debug,Serialize)]
pub struct Moments{pub mean_numerators:Vec<i128>,pub covariance_numerators:Vec<Vec<i128>>}
#[derive(Clone,Debug)]
pub struct BoundSurface{surface:Surface,contracts:Vec<BoundContract>,scenarios:Vec<BoundScenario>,moments:Moments,books:BTreeMap<String,Book>,head:Value,report_sha:String,scope_sha:String,at:u64,until:u64,horizon:u64}
fn mul(a:i128,b:i128)->Result<i128>{a.checked_mul(b).ok_or("SURFACE_RANGE".into())}
fn plus(a:i128,b:i128)->Result<i128>{a.checked_add(b).ok_or("SURFACE_RANGE".into())}
fn modes(r:&ReviewInput)->Vec<crate::alpha_proposal::Mode>{use fundamental_policy::ExecutionMode as E;use crate::alpha_proposal::Mode as A;r.execution_policy.as_ref().map(|p|p.allowed_modes.iter().map(|m|match m{E::JoinBid=>A::JoinBid,E::ImproveOneTick=>A::ImproveOneTick,E::BoundedLimitMayRest=>A::BoundedLimitMayRest}).collect()).unwrap_or_else(||vec![A::JoinBid])}
impl BoundSurface{
 pub fn bind(surface:Surface,bank:&Session,scope:&Scope,reviews:Vec<ContractReview<'_>>,trusted_reviews:&BTreeSet<String>,trusted_mappings:&BTreeSet<String>,now:u64)->Result<Self>{
  need(bank.engine().is_live()&&now>=surface.observed&&now<surface.expires&&(1..=128).contains(&reviews.len()),"SURFACE_BANK_OR_CLOCK")?;
  let age=bank.engine().policy()["bookAgeMs"].as_u64().ok_or("SURFACE_POLICY")?.min(10_000);let(available,_)=package_admission::native_books(bank,scope,now,age)?;
  let mut contracts=Vec::new();let mut books=BTreeMap::new();let mut seen=BTreeSet::new();let mut until=surface.expires;let mut horizon=0;
  for r in reviews{
   let input=r.input;let review=fundamental_policy::review_sha256(&input)?;let map_hash=mapping_sha256(&r.mapping)?;
   need(r.mapping.version==1&&r.mapping.model_source_sha256==surface.source_sha&&r.mapping.review_sha256==review&&trusted_mappings.contains(&map_hash)&&atom(&r.mapping.assumption_label)&&r.mapping.maximum_model_deviation_micros<=1_000_000&&r.mapping.loadings.len()<=16,"SURFACE_MAPPING_NOT_APPROVED")?;
   let mut factors=BTreeSet::new();for f in &r.mapping.loadings{need(atom(&f.factor_id)&&factors.insert(&f.factor_id)&&f.loading_micros!=0&&(-10*UNIT..=10*UNIT).contains(&f.loading_micros),"SURFACE_FACTOR_LOADING")?;}
   need(input.tournament_id==scope.tournament_id&&scope.exchanges.get(&input.exchange_id)==Some(&input.market_id)&&seen.insert(input.exchange_id.clone())&&input.bank_manifest_sha256==bank.manifest_hash().as_str().ok_or("SURFACE_BANK")?&&bank.live_account_seed().is_some_and(|s|s["runId"]==input.run_id),"SURFACE_CONTRACT_SCOPE")?;
   need(input.sources.iter().any(|s|s.kind=="forecast"&&s.sha256==surface.source_sha),"SURFACE_MODEL_NOT_REVIEWED_SOURCE")?;
   let verified=Reviewed::verify(input.clone(),r.originals,trusted_reviews,now)?;
   let settlement=match verified.valuation_objective(){Some(ValuationObjective::TerminalSettlement{scheduled_settlement_at_ms,..})=>*scheduled_settlement_at_ms,_=>return Err("SURFACE_TERMINAL_OBJECTIVE_REQUIRED".into())};
   need(settlement>now&&!input.revoke_existing,"SURFACE_TERMINAL_EXPIRED_OR_REVOKED")?;
   let receipt=r.current_receipt;let start=daily_windows::clock(&receipt.request_started_at)?;let received=daily_windows::clock(&receipt.received_at)?;
   need(receipt.status==200&&receipt.endpoint==format!("/markets/{}/nodes?tournamentId={}",input.market_id,scope.tournament_id)&&start<=received&&received<=now&&now-start<=age,"SURFACE_CURRENT_CONTRACT")?;
   need(receipt.data["market_id"]==input.market_id&&receipt.data["root"]["node_type"]=="contract"&&receipt.data["root"]["node_id"]==input.contract_id&&receipt.data["root"]["contract_id"]==input.contract_id&&fundamental_policy::official_contract_root_sha256(&receipt.data)?==input.official_contract_root_sha256,"SURFACE_CURRENT_CONTRACT_CHANGED")?;
   // An operator mapping cannot override contradictory structured primary
   // party metadata. Opaque/other resolver semantics remain labeled assumptions
   // and need the separate semantic-state review; titles are never parsed.
   let details=&receipt.data["root"]["contract_details"];
   if details["resolutionType"]=="Party Winner"{if let Some(party)=match details["winnerName"].as_str(){Some("Democratic Party")=>Some(RaceOutcome::Dem),Some("Republican Party")=>Some(RaceOutcome::Rep),_=>None}{need(party==r.mapping.party,"SURFACE_PRIMARY_PARTY_CONTRADICTION")?;}}
   let race=surface.races.iter().position(|x|x.race_id==r.mapping.race_id).ok_or("SURFACE_RACE_MAPPING")?;
   let side=match input.side.as_str(){"YES"=>Side::Yes,"NO"=>Side::No,_=>return Err("SURFACE_SIDE".into())};
   let mean=surface.scenarios.iter().filter(|s|(s.race_outcomes[race]==r.mapping.party)==(side==Side::Yes)).try_fold(0i128,|n,s|plus(n,s.weight_ppm as i128))?;
   need((mean-decimal_units(&input.ordinary_probability)?).abs()<=r.mapping.maximum_model_deviation_micros as i128,"SURFACE_MODEL_REVIEW_PROBABILITY_DRIFT")?;
   let exceptional=decimal_units(&input.exceptional_probability_allowance)?;let floor=decimal_units(&input.minimum_exceptional_payout)?;let expected=plus(mul(UNIT-exceptional,mean)?,mul(exceptional,floor)?)?/UNIT;
   let b=available.get(&input.exchange_id).ok_or("SURFACE_CURRENT_NATIVE_BOOK_REQUIRED")?.clone();
   until=until.min(input.expires_at_ms).min(start.checked_add(age).ok_or("SURFACE_RANGE")?).min(b.source.received_ms.checked_add(age).ok_or("SURFACE_RANGE")?).min(b.observed_ms.checked_add(age).ok_or("SURFACE_RANGE")?);horizon=horizon.max(settlement-now);
   books.insert(input.exchange_id.clone(),b);
   contracts.push(BoundContract{market_id:input.market_id.clone(),exchange_id:input.exchange_id.clone(),contract_id:input.contract_id.clone(),side,official_root_sha256:input.official_contract_root_sha256.clone(),race_id:r.mapping.race_id,party:r.mapping.party,expected_payout_micros:expected,ordinary_payout_micros:mean,exceptional_allowance_micros:exceptional,minimum_exceptional_payout_micros:floor,review_sha256:review,mapping_sha256:map_hash,loadings:r.mapping.loadings,maximum_principal:decimal_units(&input.maximum_principal)?,maximum_quantity:input.maximum_quantity,margin_per_share:decimal_units(&input.margin_per_share)?,scheduled_settlement_at_ms:settlement,reviewed_at_ms:input.reviewed_at_ms,valid_until_ms:input.expires_at_ms,allowed_modes:modes(&input)});
  }
  need(now<until,"SURFACE_NOT_CURRENT")?;
  let scenarios:Vec<_>=surface.scenarios.iter().map(|s|BoundScenario{id:s.id.clone(),weight_ppm:s.weight_ppm,payout_micros:contracts.iter().map(|c|{let i=surface.races.iter().position(|r|r.race_id==c.race_id).expect("validated race");if(s.race_outcomes[i]==c.party)==(c.side==Side::Yes){UNIT}else{0}}).collect()}).collect();
  let n=contracts.len();let mut means=vec![0i128;n];let mut second=vec![vec![0i128;n];n];
  for s in &scenarios{for i in 0..n{means[i]=plus(means[i],mul(s.weight_ppm as i128,s.payout_micros[i])?)?;for j in 0..n{second[i][j]=plus(second[i][j],mul(s.weight_ppm as i128,mul(s.payout_micros[i],s.payout_micros[j])?)?)?;}}}
  for i in 0..n{for j in 0..n{second[i][j]=mul(second[i][j],UNIT)?.checked_sub(mul(means[i],means[j])?).ok_or("SURFACE_RANGE")?;}}
  Ok(Self{surface,contracts,scenarios,moments:Moments{mean_numerators:means,covariance_numerators:second},books,head:bank.durable_head(),report_sha:digest(&bank.engine().report()?)?,scope_sha:digest(scope)?,at:now,until,horizon})
 }
 pub fn contracts(&self)->&[BoundContract]{&self.contracts}
 pub fn scenarios(&self)->&[BoundScenario]{&self.scenarios}
 pub fn moments(&self)->&Moments{&self.moments}
 pub fn books(&self)->&BTreeMap<String,Book>{&self.books}
 pub fn surface(&self)->&Surface{&self.surface}
 pub fn source_sha256(&self)->&str{self.surface.source_sha256()}
 pub fn model_sha256(&self)->&str{self.surface.model_sha256()}
 pub fn horizon_ms(&self)->u64{self.horizon}
 pub fn valid_until_ms(&self)->u64{self.until}
 pub fn bound_head(&self)->&Value{&self.head}
 pub fn bound_at_ms(&self)->u64{self.at}
 pub fn validate_current(&self,bank:&Session,scope:&Scope,now:u64)->Result<()>{need(self.at<=now&&now<self.until&&self.head==bank.durable_head()&&self.report_sha==digest(&bank.engine().report()?)?&&self.scope_sha==digest(scope)?,"SURFACE_FROZEN_BOUNDARY_CHANGED")}
}
