//! Bid/ask-interval common-factor diagnostics and small fully funded hedge ideas.
//! The fitted factor is a descriptive venue displacement, not a calibrated
//! revision to Atlas. No short sale, synthetic queue position or midpoint fill.
use crate::{feedback::Result,value_surface::{BoundSurface,UNIT}};
use baal_transport_service::{decimal_units,Scope};
use moloch_relationship_arbitrage::{depth_cost,Level};
use moloch_replay::{session::Session,Side};
use serde::Serialize;
use std::collections::BTreeMap;
use sha2::{Digest,Sha256};
fn need(b:bool,e:&'static str)->Result<()>{if b{Ok(())}else{Err(e.into())}}
fn add(a:i128,b:i128)->Result<i128>{a.checked_add(b).ok_or("CURVE_RANGE".into())}
fn mul(a:i128,b:u64)->Result<i128>{a.checked_mul(b as i128).ok_or("CURVE_RANGE".into())}
#[derive(Clone,Debug,Serialize)]
pub struct FitPolicy{pub factor_id:String,pub minimum_races:usize,pub maximum_depth_weight:u64,pub maximum_factor_abs:f64,pub minimum_residual_micros:i128}
impl FitPolicy{fn validate(&self)->Result<()>{need(!self.factor_id.is_empty()&&(3..=128).contains(&self.minimum_races)&&(1..=10000).contains(&self.maximum_depth_weight)&&self.maximum_factor_abs.is_finite()&&self.maximum_factor_abs>0.&&self.maximum_factor_abs<=10.&&(0..=UNIT).contains(&self.minimum_residual_micros),"CURVE_POLICY")}}
#[derive(Clone,Debug,Serialize)]
pub struct IntervalObservation{pub exchange_id:String,pub race_id:String,pub lower:f64,pub upper:f64,pub weight:u64}
#[derive(Clone,Debug,Serialize)]
pub struct Residual{pub exchange_id:String,pub lower_micros:f64,pub upper_micros:f64,pub buy_residual_upper_micros:f64,pub sell_residual_lower_micros:f64}
#[derive(Clone,Debug,Serialize)]
pub struct Fit{factor_id:String,pub factor_lower:f64,pub factor_upper:f64,pub weighted_interval_loss:f64,pub distinct_races:usize,pub observations:Vec<IntervalObservation>,pub residuals:Vec<Residual>,source_sha:String,head:serde_json::Value,valid_until:u64,minimum_residual:i128,#[serde(skip)]seal_sha:String}
fn fit_sha(f:&Fit)->Result<String>{Ok(format!("{:x}",Sha256::digest(serde_json::to_vec(f)?)))}
impl Fit{pub fn factor_id(&self)->&str{&self.factor_id}pub fn source_sha256(&self)->&str{&self.source_sha}pub fn valid_until_ms(&self)->u64{self.valid_until}}
/// Weighted L1 distance to intervals: every interval contributes two endpoints
/// with the SAME capped depth weight. The weighted median endpoint interval is
/// the complete minimizer set. No quoted midpoint is treated as fair or filled.
pub fn interval_factor(rows:&[(f64,f64,u64)],bound:f64)->Result<(f64,f64,f64)>{
 need(!rows.is_empty()&&bound.is_finite()&&bound>0.,"CURVE_INTERVALS")?;let mut ends=Vec::new();let mut total=0u64;
 for &(lo,hi,w)in rows{need(lo.is_finite()&&hi.is_finite()&&lo<=hi&&w>0,"CURVE_INTERVALS")?;total=total.checked_add(w).ok_or("CURVE_RANGE")?;ends.push((lo,w));ends.push((hi,w));}
 ends.sort_by(|a,b|a.0.total_cmp(&b.0));let mut cumulative=0u64;let mut low=None;let mut high=None;
 for i in 0..ends.len(){cumulative=cumulative.checked_add(ends[i].1).ok_or("CURVE_RANGE")?;if cumulative>=total{low=Some(ends[i].0);high=Some(if cumulative==total{ends.get(i+1).map_or(ends[i].0,|e|e.0)}else{ends[i].0});break}}
 let lo=low.ok_or("CURVE_INTERVALS")?.clamp(-bound,bound);let hi=high.ok_or("CURVE_INTERVALS")?.clamp(-bound,bound);let center=(lo+hi)/2.;let loss=rows.iter().map(|(l,h,w)|if center<*l{(l-center)*(*w as f64)}else if center>*h{(center-h)*(*w as f64)}else{0.}).sum();Ok((lo,hi,loss))
}
pub fn fit(surface:&BoundSurface,p:&FitPolicy)->Result<Fit>{
 p.validate()?;let mut by_race=BTreeMap::<String,IntervalObservation>::new();
 for c in surface.contracts(){let Some(l)=c.loadings.iter().find(|l|l.factor_id==p.factor_id)else{continue};let b=&surface.books()[&c.exchange_id];let(bids,asks)=if c.side==Side::Yes{(&b.yes_bids,&b.yes_asks)}else{(&b.no_bids,&b.no_asks)};
  let bid=bids.first().ok_or("CURVE_DEPTH")?;let ask=asks.first().ok_or("CURVE_DEPTH")?;let weight=bid.quantity.min(ask.quantity).min(p.maximum_depth_weight);let x=(bid.price-c.ordinary_payout_micros)as f64/l.loading_micros as f64;let y=(ask.price-c.ordinary_payout_micros)as f64/l.loading_micros as f64;
  let row=IntervalObservation{exchange_id:c.exchange_id.clone(),race_id:c.race_id.clone(),lower:x.min(y),upper:x.max(y),weight};
  // Two party/outcome contracts on one race do not become two independent races.
  if by_race.get(&c.race_id).is_none_or(|old|row.weight>old.weight||(row.weight==old.weight&&row.exchange_id<old.exchange_id)){by_race.insert(c.race_id.clone(),row);}
 }
 need(by_race.len()>=p.minimum_races,"CURVE_INSUFFICIENT_DISTINCT_RACES")?;
 let observations:Vec<_>=by_race.into_values().collect();let rows:Vec<_>=observations.iter().map(|r|(r.lower,r.upper,r.weight)).collect();let(lo,hi,loss)=interval_factor(&rows,p.maximum_factor_abs)?;let mut residuals=Vec::new();
 for c in surface.contracts(){let Some(l)=c.loadings.iter().find(|l|l.factor_id==p.factor_id)else{continue};let b=&surface.books()[&c.exchange_id];let(bid,ask)=if c.side==Side::Yes{(b.yes_bids[0].price,b.yes_asks[0].price)}else{(b.no_bids[0].price,b.no_asks[0].price)};
  let a=c.ordinary_payout_micros as f64+l.loading_micros as f64*lo;let z=c.ordinary_payout_micros as f64+l.loading_micros as f64*hi;let fair_low=a.min(z).clamp(0.,UNIT as f64);let fair_high=a.max(z).clamp(0.,UNIT as f64);
  residuals.push(Residual{exchange_id:c.exchange_id.clone(),lower_micros:bid as f64-fair_high,upper_micros:ask as f64-fair_low,buy_residual_upper_micros:ask as f64-fair_low,sell_residual_lower_micros:bid as f64-fair_high});
 }
 let mut fit=Fit{factor_id:p.factor_id.clone(),factor_lower:lo,factor_upper:hi,weighted_interval_loss:loss,distinct_races:observations.len(),observations,residuals,source_sha:surface.source_sha256().into(),head:surface.bound_head().clone(),valid_until:surface.valid_until_ms(),minimum_residual:p.minimum_residual_micros,seal_sha:String::new()};fit.seal_sha=fit_sha(&fit)?;Ok(fit)
}
#[derive(Clone,Debug,Serialize)]
pub struct HedgeLimits{pub maximum_quantity_per_leg:u64,pub maximum_gross_funding:i128,pub maximum_absolute_factor_exposure:i128,pub minimum_model_gain:i128,pub tail_probability_ppm:u32,pub maximum_tail_loss:i128}
#[derive(Clone,Debug,Serialize)]
pub struct HedgeLeg{pub exchange_id:String,pub market_id:String,pub side:String,pub quantity:u64,pub limit_price:i128,pub visible_cost:i128}
#[derive(Clone,Debug,Serialize)]
pub struct HedgeIdea{pub legs:Vec<HedgeLeg>,pub gross_funding:i128,pub expected_ordinary_gain:i128,pub conservative_review_gain:i128,pub worst_ordinary_gain:i128,pub ordinary_tail_mean_gain:i128,pub exceptional_zero_payout_loss:i128,pub residual_factor_exposure:i128,pub source_sha256:String,pub factor_id:String,pub execution_authority:bool}
fn marginal(levels:&[Level],n:u64)->Result<i128>{let mut left=n;for l in levels{left=left.saturating_sub(l.quantity);if left==0{return Ok(l.price)}}Err("CURVE_DEPTH".into())}
/// Candidate pairs only, not a replacement portfolio optimizer. Both are BUYs
/// with opposite reviewed factor loadings; no uncovered SELL is proposed. Full
/// limit funding and existing native reservations/caps are charged before ideas.
pub fn hedge_candidates(surface:&BoundSurface,fit:&Fit,bank:&Session,scope:&Scope,p:&HedgeLimits,now:u64)->Result<Vec<HedgeIdea>>{
 surface.validate_current(bank,scope,now)?;need(fit.source_sha==surface.source_sha256()&&fit.head==*surface.bound_head()&&now<fit.valid_until&&fit.seal_sha==fit_sha(fit)?,"CURVE_FIT_CHANGED")?;
 need((1..=10000).contains(&p.maximum_quantity_per_leg)&&p.maximum_gross_funding>0&&p.maximum_absolute_factor_exposure>=0&&p.minimum_model_gain>=0&&(1..=1_000_000).contains(&p.tail_probability_ppm)&&p.maximum_tail_loss>=0,"CURVE_HEDGE_POLICY")?;
 let report=bank.engine().report()?;let reserved=decimal_units(report["reservedCash"].as_str().ok_or("CURVE_BANK")?)?;let policy=bank.engine().policy();let committed=decimal_units(policy["maximumCommitted"].as_str().ok_or("CURVE_BANK")?)?;
 let available=(bank.engine().wallet.cash-reserved).min(committed-bank.engine().wallet.owned_cost()-reserved).min(p.maximum_gross_funding);if available<=0{return Ok(vec![])}
 let cap=decimal_units(policy["maximumMarketCost"].as_str().ok_or("CURVE_BANK")?)?;let native_n=policy["quantity"].as_u64().ok_or("CURVE_BANK")?;let mut used=BTreeMap::<String,i128>::new();for c in surface.contracts(){used.insert(c.exchange_id.clone(),bank.engine().wallet.market_cost(&c.exchange_id));}for q in bank.engine().order_records().iter().filter(|q|q["retired"]==false&&q["action"]=="buy"){if let Some(v)=q["exchangeId"].as_str().and_then(|e|used.get_mut(e)){let n=q["quantity"].as_u64().ok_or("CURVE_BANK")?.checked_sub(q["filled"].as_u64().ok_or("CURVE_BANK")?).ok_or("CURVE_BANK")?;*v=add(*v,mul(decimal_units(q["price"].as_str().ok_or("CURVE_BANK")?)?,n)?)?;}}
 let mut out=Vec::new();let mut evaluated_pairs=0usize;let cs=surface.contracts();for i in 0..cs.len(){for j in i+1..cs.len(){let(a,b)=(&cs[i],&cs[j]);if a.race_id==b.race_id{continue}let Some(la)=a.loadings.iter().find(|l|l.factor_id==fit.factor_id)else{continue};let Some(lb)=b.loadings.iter().find(|l|l.factor_id==fit.factor_id)else{continue};if la.loading_micros.signum()==lb.loading_micros.signum(){continue}
  if [a,b].iter().any(|c|fit.residuals.iter().find(|r|r.exchange_id==c.exchange_id).is_none_or(|r|r.buy_residual_upper_micros>-(fit.minimum_residual as f64))){continue}
  if evaluated_pairs>=64{continue}evaluated_pairs+=1;
  let ba=&surface.books()[&a.exchange_id];let bb=&surface.books()[&b.exchange_id];let aa=if a.side==Side::Yes{&ba.yes_asks}else{&ba.no_asks};let ab=if b.side==Side::Yes{&bb.yes_asks}else{&bb.no_asks};let da=aa.iter().try_fold(0u64,|s,l|s.checked_add(l.quantity).ok_or("CURVE_RANGE"))?;let db=ab.iter().try_fold(0u64,|s,l|s.checked_add(l.quantity).ok_or("CURVE_RANGE"))?;
  let maxa=p.maximum_quantity_per_leg.min(native_n).min(a.maximum_quantity).min(da);let maxb=p.maximum_quantity_per_leg.min(native_n).min(b.maximum_quantity).min(db);let mut best=None;
  // A bounded power-of-two grid plus the full eligible endpoint. This is a
  // candidate generator; it does not claim an exact portfolio optimum.
  let mut quantities=vec![];let mut size=1u64;while size<=maxa{quantities.push(size);size=size.checked_mul(2).ok_or("CURVE_RANGE")?;}if maxa>0&&quantities.last()!=Some(&maxa){quantities.push(maxa)}
  for na in quantities{let numerator=mul(la.loading_micros.abs(),na)?;let rounded=(numerator+lb.loading_micros.abs()/2)/lb.loading_micros.abs();let Ok(nb)=u64::try_from(rounded)else{continue};if nb==0||nb>maxb{continue}let exposure=add(mul(la.loading_micros,na)?,mul(lb.loading_micros,nb)?)?;if exposure.abs()>p.maximum_absolute_factor_exposure{continue}
   let pa=marginal(aa,na)?;let pb=marginal(ab,nb)?;if pa>a.expected_payout_micros-a.margin_per_share||pb>b.expected_payout_micros-b.margin_per_share{continue}let fa=mul(pa,na)?;let fb=mul(pb,nb)?;let funding=add(fa,fb)?;if funding>available||fa>a.maximum_principal||fb>b.maximum_principal||fa>cap-used[&a.exchange_id]||fb>cap-used[&b.exchange_id]{continue}
   let visible_a=depth_cost(aa,na,true)?;let visible_b=depth_cost(ab,nb,true)?;let mut gains=Vec::new();let mut expected=0i128;for s in surface.scenarios(){let gain=add(mul(s.payout_micros[i],na)?,mul(s.payout_micros[j],nb)?)?-funding;expected=add(expected,gain.checked_mul(s.weight_ppm as i128).ok_or("CURVE_RANGE")?)?;gains.push((gain,s.weight_ppm));}let expected=expected/UNIT;let conservative=add(mul(a.expected_payout_micros,na)?,mul(b.expected_payout_micros,nb)?)?-funding;if expected<p.minimum_model_gain||conservative<p.minimum_model_gain{continue}
   gains.sort_by_key(|g|g.0);let worst=gains[0].0;let mut remaining=p.tail_probability_ppm;let mut tail=0i128;for(g,w)in &gains{let take=remaining.min(*w);tail=add(tail,g.checked_mul(take as i128).ok_or("CURVE_RANGE")?)?;remaining-=take;if remaining==0{break}}let tail=tail/p.tail_probability_ppm as i128;if tail < -p.maximum_tail_loss{continue}
   let idea=HedgeIdea{legs:vec![HedgeLeg{exchange_id:a.exchange_id.clone(),market_id:a.market_id.clone(),side:if a.side==Side::Yes{"YES"}else{"NO"}.into(),quantity:na,limit_price:pa,visible_cost:visible_a},HedgeLeg{exchange_id:b.exchange_id.clone(),market_id:b.market_id.clone(),side:if b.side==Side::Yes{"YES"}else{"NO"}.into(),quantity:nb,limit_price:pb,visible_cost:visible_b}],gross_funding:funding,expected_ordinary_gain:expected,conservative_review_gain:conservative,worst_ordinary_gain:worst,ordinary_tail_mean_gain:tail,exceptional_zero_payout_loss:funding,residual_factor_exposure:exposure,source_sha256:surface.source_sha256().into(),factor_id:fit.factor_id.clone(),execution_authority:false};
   if best.as_ref().is_none_or(|b:&HedgeIdea|idea.conservative_review_gain>b.conservative_review_gain){best=Some(idea)}
  }if let Some(b)=best{out.push(b)}
 }}out.sort_by(|a,b|b.conservative_review_gain.cmp(&a.conservative_review_gain));Ok(out)
}
