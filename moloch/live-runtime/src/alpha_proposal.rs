//! Read-only forecast execution comparison. No reservation, authority or fill
//! probability is minted here. ReviewBounds must come from the caller's pinned
//! review; its hashes bind inputs but do not authenticate their economic claims.
//! Candidates still require native reservation, fresh venue/account/contract
//! checks, latency and deadline validation, and opaque physical authorization.
//! Full funding uses the entire frozen limit, not weighted visible ask cost.
use crate::{feedback::Result, package_admission};
use baal_transport_service::{decimal_units, Scope};
use moloch_replay::{session::Session, Side};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
const UNIT:i128=1_000_000;
const TICK:i128=5_000;
fn need(b:bool,s:&'static str)->Result<()>{if b{Ok(())}else{Err(s.into())}}
fn hash<T:Serialize>(v:&T)->Result<String>{Ok(format!("{:x}",Sha256::digest(serde_json::to_vec(v)?)))}
fn hex(s:&str)->bool{s.len()==64&&s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))}
fn times(a:i128,n:u64)->Result<i128>{a.checked_mul(n as i128).ok_or("ALPHA_RANGE".into())}
#[derive(Clone,Copy,Debug,PartialEq,Eq,Serialize)]
pub enum Mode{JoinBid,ImproveOneTick,BoundedLimitMayRest}
/// Explicit operator/model assumptions, not a calibrated probability estimate.
pub struct ReviewBounds{
 pub exchange_id:String,pub market_id:String,pub side:Side,
 pub review_sha256:String,pub model_sha256:String,pub source_sha256:String,
 pub reviewed_at_ms:u64,pub valid_until_ms:u64,pub horizon_ms:u64,
 pub expected_payout:i128,pub acquisition_ceiling:i128,
 pub maximum_principal:i128,pub maximum_quantity:u64,
 pub modes:Vec<Mode>,
}
#[derive(Clone,Debug,Serialize)]
pub struct Candidate{
 mode:Mode,action:Value,quantity:u64,limit_price:i128,gross_funding:i128,
 immediate_acquisition_quantity:u64,visible_acquisition_notional:i128,
 liquidation_capacity_quantity:u64,visible_liquidation_notional:i128,
 conditional_forecast_surplus:i128,horizon_ms:u64,valid_until_ms:u64,
 provenance:Value,
}
impl Candidate{
 pub fn mode(&self)->Mode{self.mode}
 pub fn action(&self)->&Value{&self.action}
 pub fn quantity(&self)->u64{self.quantity}
 pub fn limit_price(&self)->i128{self.limit_price}
 pub fn gross_funding(&self)->i128{self.gross_funding}
 pub fn immediate_acquisition_quantity(&self)->u64{self.immediate_acquisition_quantity}
 pub fn visible_acquisition_notional(&self)->i128{self.visible_acquisition_notional}
 pub fn visible_liquidation_notional(&self)->i128{self.visible_liquidation_notional}
 pub fn conditional_forecast_surplus(&self)->i128{self.conditional_forecast_surplus}
 pub fn provenance(&self)->&Value{&self.provenance}
 pub fn valid_until_ms(&self)->u64{self.valid_until_ms}
}
#[derive(Debug,Serialize)]
pub struct Deferred{pub mode:Mode,pub reason:&'static str}
#[derive(Debug,Serialize)]
pub struct Comparison{pub candidates:Vec<Candidate>,pub deferred:Vec<Deferred>}
fn defer(modes:&[Mode],why:&'static str)->Comparison{Comparison{candidates:vec![],deferred:modes.iter().map(|m|Deferred{mode:*m,reason:why}).collect()}}
/// Inputs are fixed-point micro-SUSQ per share. Mode ordering is the reviewed
/// caller order, not an EV ranking: passive fill probability is UNKNOWN.
pub fn compare(bank:&Session,scope:&Scope,r:&ReviewBounds,now:u64)->Result<Comparison>{
 need(bank.engine().is_live()&&scope.exchanges.get(&r.exchange_id)==Some(&r.market_id),"ALPHA_SCOPE")?;
 need(hex(&r.review_sha256)&&hex(&r.model_sha256)&&hex(&r.source_sha256)
  &&r.reviewed_at_ms<r.valid_until_ms&&r.horizon_ms>0&&r.maximum_quantity>0
  &&r.maximum_principal>0&&(0..=UNIT).contains(&r.expected_payout)
  &&(TICK..=UNIT-TICK).contains(&r.acquisition_ceiling)
  &&!r.modes.is_empty()&&r.modes.len()<=3,"ALPHA_REVIEW")?;
 for(i,m)in r.modes.iter().enumerate(){need(!r.modes[..i].contains(m),"ALPHA_DUPLICATE_MODE")?;}
 if now<r.reviewed_at_ms||now>=r.valid_until_ms{return Ok(defer(&r.modes,"review_not_current"))}
 let policy=bank.engine().policy();
 if policy["entriesEnabled"]!=true||policy["entryStopAt"].as_str().map(crate::daily_windows::clock).transpose()?.is_some_and(|end|now>=end){return Ok(defer(&r.modes,"native_entries_disabled_or_expired"))}
 if policy["feeBps"]!=0||decimal_units(policy["feePerShare"].as_str().ok_or("ALPHA_POLICY")?)?!=0{return Ok(defer(&r.modes,"unsupported_nonzero_fees"))}
 let age=policy["bookAgeMs"].as_u64().ok_or("ALPHA_POLICY")?.min(10_000);
 let(books,_)=package_admission::native_books(bank,scope,now,age)?;
 let Some(b)=books.get(&r.exchange_id)else{return Ok(defer(&r.modes,"fresh_admitted_book_required"))};
 let(bids,asks)=if r.side==Side::Yes{(&b.yes_bids,&b.yes_asks)}else{(&b.no_bids,&b.no_asks)};
 let bid=bids.first().ok_or("ALPHA_DEPTH")?.price;
 let ask=asks.first().ok_or("ALPHA_DEPTH")?.price;
 let report=bank.engine().report()?;
 let reserved=decimal_units(report["reservedCash"].as_str().ok_or("ALPHA_REPORT")?)?;
 let committed=decimal_units(policy["maximumCommitted"].as_str().ok_or("ALPHA_POLICY")?)?;
 let market_cap=decimal_units(policy["maximumMarketCost"].as_str().ok_or("ALPHA_POLICY")?)?;
 let mut market_used=bank.engine().wallet.market_cost(&r.exchange_id);
 for q in bank.engine().order_records().iter().filter(|q|q["exchangeId"]==r.exchange_id&&q["retired"]==false&&q["action"]=="buy"){
  let quantity=q["quantity"].as_u64().ok_or("ALPHA_ORDER")?.checked_sub(q["filled"].as_u64().ok_or("ALPHA_ORDER")?).ok_or("ALPHA_ORDER")?;
  market_used=market_used.checked_add(times(decimal_units(q["price"].as_str().ok_or("ALPHA_ORDER")?)?,quantity)?).ok_or("ALPHA_RANGE")?;
 }
 let available=r.maximum_principal.min(bank.engine().wallet.cash.checked_sub(reserved).ok_or("ALPHA_RANGE")?)
  .min(committed.checked_sub(bank.engine().wallet.owned_cost()).and_then(|x|x.checked_sub(reserved)).ok_or("ALPHA_RANGE")?)
  .min(market_cap.checked_sub(market_used).ok_or("ALPHA_RANGE")?);
 let maximum=r.maximum_quantity.min(policy["quantity"].as_u64().ok_or("ALPHA_POLICY")?);
 let liquidation_capacity=bids.iter().try_fold(0u64,|n,l|n.checked_add(l.quantity).ok_or("ALPHA_RANGE"))?;
 let validity=r.valid_until_ms.min(b.source.received_ms.checked_add(age).ok_or("ALPHA_RANGE")?).min(b.observed_ms.checked_add(age).ok_or("ALPHA_RANGE")?);
 let review_bounds=json!({"marketId":r.market_id,"exchangeId":r.exchange_id,"side":if r.side==Side::Yes{"YES"}else{"NO"},"expectedPayout":r.expected_payout,"acquisitionCeiling":r.acquisition_ceiling,"maximumPrincipal":r.maximum_principal,"maximumQuantity":r.maximum_quantity,"modes":r.modes,"horizonMs":r.horizon_ms,"validUntilMs":r.valid_until_ms});
 let provenance=json!({"bankHead":bank.durable_head(),"reportSha256":hash(&report)?,"scopeSha256":hash(scope)?,"bookSha256":b.source.sha256,"reviewSha256":r.review_sha256,"modelSha256":r.model_sha256,"sourceSha256":r.source_sha256,"reviewBounds":review_bounds,"reviewedAtMs":r.reviewed_at_ms,"decisionAtMs":now,"bookObservedMs":b.observed_ms,"bookReceivedMs":b.source.received_ms,"executionProbability":null,"valuationBasis":"operator_reviewed_conditional_forecast","venuePostOnly":false,"venueAllOrNone":false});
 let mut out=Comparison{candidates:vec![],deferred:vec![]};
 for mode in &r.modes{
  let fail=|out:&mut Comparison,reason|out.deferred.push(Deferred{mode:*mode,reason});
  let mut limit=match mode{Mode::JoinBid=>bid,Mode::ImproveOneTick=>bid.checked_add(TICK).ok_or("ALPHA_RANGE")?,Mode::BoundedLimitMayRest=>r.acquisition_ceiling/TICK*TICK};
  if *mode==Mode::ImproveOneTick&&limit>=ask{fail(&mut out,"no_inside_tick");continue}
  if limit>r.acquisition_ceiling||limit>r.expected_payout{fail(&mut out,"reviewed_value_ceiling");continue}
  if available<limit{fail(&mut out,"insufficient_fully_funded_capital");continue}
  let mut n=maximum.min(liquidation_capacity).min(u64::try_from(available/limit)?);
  if *mode==Mode::BoundedLimitMayRest{
   let capacity=asks.iter().filter(|l|l.price<=limit).try_fold(0u64,|a,l|a.checked_add(l.quantity).ok_or("ALPHA_RANGE"))?;
   n=n.min(capacity);if n==0{fail(&mut out,"no_ask_depth_within_ceiling");continue}
   let mut left=n;for l in asks.iter().filter(|l|l.price<=limit){left=left.saturating_sub(l.quantity);if left==0{limit=l.price;break}}
  }
  if n==0{fail(&mut out,"no_liquidation_capacity");continue}
  let mut left=n;let mut acquisition=0i128;for l in asks.iter().filter(|l|l.price<=limit){let take=left.min(l.quantity);acquisition=acquisition.checked_add(times(l.price,take)?).ok_or("ALPHA_RANGE")?;left-=take;if left==0{break}}
  let immediate=n-left;left=n;let mut liquidation=0i128;for l in bids{let take=left.min(l.quantity);liquidation=liquidation.checked_add(times(l.price,take)?).ok_or("ALPHA_RANGE")?;left-=take;if left==0{break}}
  let funding=times(limit,n)?;let surplus=times(r.expected_payout-limit,n)?;
  if surplus<=0{fail(&mut out,"nonpositive_conditional_forecast_surplus");continue}
  out.candidates.push(Candidate{mode:*mode,action:json!({"exchangeId":r.exchange_id,"marketId":r.market_id,"tournamentId":scope.tournament_id,"side":if r.side==Side::Yes{"yes"}else{"no"},"action":"buy","quantity":n,"limitPrice":moloch_replay::money(limit),"execution":"bounded_limit_may_rest"}),quantity:n,limit_price:limit,gross_funding:funding,immediate_acquisition_quantity:immediate,visible_acquisition_notional:acquisition,liquidation_capacity_quantity:n,visible_liquidation_notional:liquidation,conditional_forecast_surplus:surplus,horizon_ms:r.horizon_ms,valid_until_ms:validity,provenance:provenance.clone()});
 }Ok(out)
}
