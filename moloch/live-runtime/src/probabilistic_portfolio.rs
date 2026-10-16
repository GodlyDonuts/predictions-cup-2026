//! Finite, exact portfolio search over a reviewed probabilistic state space.
//! Neither a positive expectation nor a finite state minimum is an arbitrage
//! certificate. This module never reserves, credits, submits or borrows funds.
use crate::{feedback::Result,semantic_states::{self as semantics,Compiled,Side,UNIT},value_surface::BoundSurface,dispatch::DispatchLog,package_admission};
use baal_transport_service::{Scope,decimal_units,multi_leg,multi_leg_portfolio as wire};
use moloch_replay::{session::Session,Side as NativeSide,money};
use serde::{Serialize,Deserialize};
use serde_json::{Value,json};
use std::collections::{BTreeMap,BTreeSet};
use sha2::{Sha256,Digest};
fn need(v:bool,s:&'static str)->Result<()>{if v{Ok(())}else{Err(s.into())}}
fn add(a:i128,b:i128)->Result<i128>{a.checked_add(b).ok_or_else(||"PROB_RANGE".into())}
fn mul(a:i128,b:i128)->Result<i128>{a.checked_mul(b).ok_or_else(||"PROB_RANGE".into())}
fn ceil(a:i128,b:i128)->Result<i128>{need(a>=0&&b>0,"PROB_RANGE")?;Ok(add(a,b-1)?/b)}
fn hash<T:Serialize+?Sized>(v:&T)->Result<String>{semantics::digest(v)}
fn ns(s:Side)->NativeSide{if s==Side::Yes{NativeSide::Yes}else{NativeSide::No}}
fn label(s:Side)->&'static str{if s==Side::Yes{"YES"}else{"NO"}}
#[derive(Clone,Serialize,Deserialize)]#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct FactorLimit{pub maximum_absolute_shares_micros:i128,pub loading_ppm:Vec<i128>}
#[derive(Clone,Serialize,Deserialize)]#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct PolicyInput{
 pub version:u32,pub run_id:String,pub bank_manifest_sha256:String,pub valid_until_ms:u64,
 pub quantities:Vec<u64>,pub maximum_enumerations:u64,pub maximum_gross_buy_cost:i128,
 pub minimum_expected_gain:i128,pub maximum_incremental_cvar_loss:i128,
 pub maximum_terminal_cvar_loss:i128,pub tail_ppm:u32,pub capital_rate_ppm_per_day:u32,
 pub fee_per_share:i128,pub maximum_book_age_ms:u64,pub maximum_plan_age_ms:u64,
 pub factors:BTreeMap<String,FactorLimit>,
}
/// Operator-reviewed immutable limits, not dynamically caller-adjustable scores.
pub struct Policy{input:PolicyInput,sha:String}
impl Policy{
 pub fn load(bytes:&[u8],expected:&str,approved:&BTreeSet<String>)->Result<Self>{
  need(bytes.len()<=64*1024&&format!("{:x}",Sha256::digest(bytes))==expected&&approved.contains(expected),"PROB_POLICY_NOT_APPROVED")?;
  let p:PolicyInput=serde_json::from_slice(bytes)?;let quantities:BTreeSet<_>=p.quantities.iter().copied().collect();
  need(p.version==1&&!p.run_id.is_empty()&&p.bank_manifest_sha256.len()==64&&!quantities.is_empty()&&quantities.len()==p.quantities.len()&&quantities.len()<=16&&quantities.iter().all(|q|*q>0&&*q<=u32::MAX as u64)
   &&(1..=100_000).contains(&p.maximum_enumerations)&&p.maximum_gross_buy_cost>=0&&p.minimum_expected_gain>=0&&p.maximum_incremental_cvar_loss>=0&&p.maximum_terminal_cvar_loss>=0
   &&p.tail_ppm>0&&p.tail_ppm<=1_000_000&&p.fee_per_share>=0&&p.maximum_book_age_ms>0&&p.maximum_book_age_ms<=10_000&&p.maximum_plan_age_ms>0&&p.maximum_plan_age_ms<=60_000&&p.factors.len()<=32,"PROB_POLICY_SHAPE")?;
  need(p.factors.values().all(|f|f.maximum_absolute_shares_micros>=0&&f.loading_ppm.iter().all(|l|l.checked_abs().is_some_and(|x|x<=UNIT))),"PROB_FACTOR_SHAPE")?;
  Ok(Self{input:p,sha:expected.into()})
 }
 pub fn sha256(&self)->&str{&self.sha}
}
#[derive(Clone,Serialize,Debug)]#[serde(rename_all="camelCase")]
pub struct Leg{pub market_id:String,pub exchange_id:String,pub contract_id:String,pub side:Side,pub action:String,pub quantity:u64,pub limit_price:i128,pub book_sha256:String}
#[derive(Clone,Serialize,Debug)]#[serde(rename_all="camelCase")]
pub struct Metrics{
 /// Exact expectation numerators; denominator is always 1,000,000.
 pub expected_wealth_numerator:i128,pub expected_increment_numerator:i128,
 pub capital_opportunity_cost:i128,pub incremental_cvar_loss:i128,
 pub terminal_cvar_loss:i128,pub minimum_modeled_increment:i128,
 pub gross_buy_funding:i128,pub minimum_sell_proceeds:i128,pub fees:i128,
 pub modeled_sleeve_basis:i128,
}
pub struct Plan{head:Value,report_sha:String,dispatch_sha:String,policy_sha:String,model_sha:String,semantic_sha:String,legs:Vec<Leg>,metrics:Metrics,expires:u64,proof:Value}
/// Genuine original qualification capability. A JSON clone is an audit value,
/// never a replacement for this non-deserializable immutable factory product.
pub struct Reservation{event:Value,plan_sha:String}
impl Reservation{
 pub fn event(&self)->&Value{&self.event}pub fn plan_sha(&self)->&str{&self.plan_sha}
 pub fn validate_plan(&self,plan:&Plan)->Result<()>{need(hash(plan.proof())?==self.plan_sha&&self.event["qualification"]["planSha256"]==self.plan_sha&&hash(&self.event["qualification"])?==self.event["proofHash"],"PROB_RESERVATION_PLAN_CHANGED")}
}
impl Plan{
 pub fn legs(&self)->&[Leg]{&self.legs}pub fn metrics(&self)->&Metrics{&self.metrics}pub fn proof(&self)->&Value{&self.proof}pub fn expires_at_ms(&self)->u64{self.expires}
 pub fn model_sha256(&self)->&str{&self.model_sha}pub fn semantic_review_sha256(&self)->&str{&self.semantic_sha}
 pub fn qualified_reservation(&self,bank:&Session,log:&DispatchLog,scope:&Scope,surface:&BoundSurface,c:&Compiled,p:&Policy,now:u64)->Result<Reservation>{let event=self.reserve_event(bank,log,scope,surface,c,p,now)?;let r=Reservation{event,plan_sha:hash(&self.proof)?};r.validate_plan(self)?;Ok(r)}
 /// Native capability is a DIFFERENT event/class from guaranteed-floor
 /// packages. Research surfaces can be optimized offline but cannot mint it.
 /// The immutable model artifact, review and risk allowlists remain runtime
 /// authority; native verifies arithmetic, funding and durable prefix only.
 pub fn reserve_event(&self,bank:&Session,log:&DispatchLog,scope:&Scope,surface:&BoundSurface,c:&Compiled,p:&Policy,now:u64)->Result<Value>{
  self.validate_current(bank,log,scope,p,now)?;surface.validate_current(bank,scope,now)?;c.validate_time(now)?;
  need(!surface.surface().is_research()&&surface.source_sha256()==self.model_sha&&c.review_sha256()==self.semantic_sha&&p.input.fee_per_share==0,"PROB_MODEL_NOT_EXECUTION_QUALIFIED")?;
  let latency=bank.engine().policy()["latencyMs"].as_u64().ok_or("PROB_POLICY")?;need(now.checked_add(latency).is_some_and(|at|at<self.expires),"PROB_RESERVATION_WINDOW")?;
  let sides:Vec<_>=surface.contracts().iter().map(|c|if c.side==NativeSide::Yes{Side::Yes}else{Side::No}).collect();let states=scenarios(surface,c,&sides)?;
  let wire_legs:Vec<_>=self.legs.iter().map(|l|json!({"marketId":l.market_id,"exchangeId":l.exchange_id,"side":label(l.side),"action":l.action,"quantity":l.quantity,"limitPrice":money(l.limit_price),"bookHash":l.book_sha256})).collect();
  let factors=p.input.factors.iter().map(|(id,f)|(id.clone(),json!({"maximumAbsoluteSharesMicros":f.maximum_absolute_shares_micros.to_string(),"loadingPpm":f.loading_ppm.iter().map(i128::to_string).collect::<Vec<_>>()}))).collect::<BTreeMap<_,_>>();
  let metrics=&self.metrics;let cfg=&p.input;
  let qualification=json!({"version":1,"kind":"reviewed_probabilistic_portfolio","planSha256":hash(&self.proof)?,"semanticReviewSha256":self.semantic_sha,"modelSha256":self.model_sha,"policySha256":self.policy_sha,"dispatchSha256":self.dispatch_sha,"positionsSha256":hash(&bank.engine().wallet.positions())?,"validUntilMs":self.expires,
   "contracts":c.contracts().iter().zip(&sides).map(|(r,s)|json!({"marketId":r.market_id,"exchangeId":r.exchange_id,"contractId":r.contract_id,"side":label(*s),"officialRootSha256":r.official_root_sha256})).collect::<Vec<_>>(),
   "states":states.iter().map(|s|json!({"id":s.id,"weightPpm":s.weight,"outcomes":s.outcomes})).collect::<Vec<_>>(),
   "risk":{"minimumExpectedGain":money(cfg.minimum_expected_gain),"maximumIncrementalCvarLoss":money(cfg.maximum_incremental_cvar_loss),"maximumTerminalCvarLoss":money(cfg.maximum_terminal_cvar_loss),"tailPpm":cfg.tail_ppm,"maximumGrossBuyFunding":money(cfg.maximum_gross_buy_cost),"capitalRatePpmPerDay":cfg.capital_rate_ppm_per_day,"horizonMs":surface.horizon_ms(),"factors":factors},
   "metrics":{"expectedIncrementNumerator":metrics.expected_increment_numerator.to_string(),"incrementalCvarLoss":money(metrics.incremental_cvar_loss),"terminalCvarLoss":money(metrics.terminal_cvar_loss),"minimumModeledIncrement":money(metrics.minimum_modeled_increment),"grossBuyFunding":money(metrics.gross_buy_funding),"minimumSellProceeds":money(metrics.minimum_sell_proceeds),"capitalOpportunityCost":money(metrics.capital_opportunity_cost)},"wireLegs":wire_legs});
  let proof=hash(&qualification)?;Ok(json!({"type":"probabilistic_portfolio_reserve_v1","source":"moloch_live_dispatcher","receivedAt":crate::daily_windows::iso(now)?,"processedAt":crate::daily_windows::iso(now)?,"bankScope":{"runId":cfg.run_id,"bankManifestHash":bank.manifest_hash(),"tournamentId":scope.tournament_id},"bankHead":self.head,"expectedReportSha256":self.report_sha,"packageId":format!("prob-{proof}"),"proofHash":proof,"expiresAt":crate::daily_windows::iso(self.expires)?,"qualification":qualification,"legs":wire_legs}))
 }
 pub fn validate_current(&self,bank:&Session,log:&DispatchLog,scope:&Scope,p:&Policy,now:u64)->Result<()>{
  need(self.proof["atMs"].as_u64().is_some_and(|at|now>=at)&&now<self.expires&&bank.durable_head()==self.head&&hash(&bank.engine().report()?)?==self.report_sha&&hash(&log.descriptor_identity()?)?==self.dispatch_sha&&p.sha==self.policy_sha,"PROB_PLAN_CHANGED_OR_EXPIRED")?;
  log.require_resolved_attempts()?;let(_,books)=package_admission::native_books(bank,scope,now,p.input.maximum_book_age_ms)?;
  for l in &self.legs{need(books.get(&l.exchange_id)==Some(&l.book_sha256),"PROB_BOOK_CHANGED")?;}Ok(())
 }
 /// Transport-independent instruction only. Existing physical authority must
 /// independently approve this probabilistic plan; no guaranteed-floor event
 /// is manufactured. SELL receipts cannot fund these gross BUY obligations.
 pub fn basket(&self,key:&str,tournament:&str)->Result<wire::Basket>{
  need(!key.is_empty()&&key.len()<=128&&self.proof["tournamentId"]==tournament,"PROB_WIRE_SCOPE")?;
  Ok(wire::Basket{idempotency_key:key.into(),tournament_id:tournament.into(),legs:self.legs.iter().map(|l|wire::Leg{exchange_id:l.exchange_id.clone(),side:if l.side==Side::Yes{multi_leg::Side::Yes}else{multi_leg::Side::No},action:if l.action=="buy"{wire::Action::Buy}else{wire::Action::Sell},quantity:l.quantity as u32,limit_price:money(l.limit_price)}).collect(),maximum_gross_buy_cost:money(self.metrics.gross_buy_funding-self.metrics.fees),minimum_gross_sell_proceeds:money(self.metrics.minimum_sell_proceeds),relationship_constraint:None})
 }
}
#[derive(Clone)]struct Instrument{side:Side,quantity:u64,principal:i128,lots:Vec<(u64,i128)>}
#[derive(Clone)]struct OptionLeg{leg:Leg,cost:i128,proceeds:i128,fee:i128,sold_principal:i128}
struct Scenario{id:String,weight:u32,outcomes:Vec<semantics::PayoutRule>}
/// Exact lower-tail CVaR of LOSS. Negative gains are losses; profits do not
/// cancel another scenario's loss. Fractional boundary mass is retained.
pub fn cvar_loss(gains:&[(i128,u32)],tail_ppm:u32)->Result<i128>{
 need(tail_ppm>0&&tail_ppm<=1_000_000&&gains.iter().map(|x|x.1 as u64).sum::<u64>()==1_000_000,"PROB_CVAR_WEIGHTS")?;
 let mut sorted=gains.to_vec();sorted.sort_by_key(|x|x.0);let mut left=tail_ppm as u64;let mut total=0;
 for(g,w)in sorted{let n=left.min(w as u64);let loss=if g<0{g.checked_neg().ok_or("PROB_RANGE")?}else{0};total=add(total,mul(loss,n as i128)?)?;left-=n;if left==0{break}}
 ceil(total,tail_ppm as i128)
}
/// Largest-remainder allocation preserves EXACT total mass and stable IDs.
pub fn scaled_ordinary_weights(weights:&[(String,u32)],mass:u32)->Result<BTreeMap<String,u32>>{
 need(mass<=1_000_000&&!weights.is_empty()&&weights.iter().map(|x|x.1 as u64).sum::<u64>()==1_000_000,"PROB_SCENARIO_WEIGHTS")?;
 let mut out=BTreeMap::new();let mut remainders=vec![];let mut sum=0u32;
 for(id,w)in weights{let n=*w as u64*mass as u64;let floor=(n/1_000_000)as u32;need(out.insert(id.clone(),floor).is_none(),"PROB_DUPLICATE_SCENARIO")?;sum+=floor;remainders.push((n%1_000_000,id.clone()));}
 remainders.sort_by(|a,b|b.0.cmp(&a.0).then(a.1.cmp(&b.1)));for(_,id)in remainders.into_iter().take((mass-sum)as usize){*out.get_mut(&id).unwrap()+=1;}Ok(out)
}
fn scenarios(surface:&BoundSurface,c:&Compiled,sides:&[Side])->Result<Vec<Scenario>>{
 need(surface.source_sha256()==c.scenario_source_sha256()&&surface.contracts().len()==c.contracts().len(),"PROB_MODEL_BINDING")?;
 for(a,b)in surface.contracts().iter().zip(c.contracts()){need(a.market_id==b.market_id&&a.exchange_id==b.exchange_id&&a.contract_id==b.contract_id&&a.official_root_sha256==b.official_root_sha256,"PROB_MODEL_CONTRACT")?;}
 let exceptions=c.exception_weights_ppm();let mass=1_000_000-exceptions.values().sum::<u32>();
 let weights=scaled_ordinary_weights(&surface.scenarios().iter().map(|s|(s.id.clone(),s.weight_ppm)).collect::<Vec<_>>(),mass)?;
 let mut used=BTreeSet::new();let mut out=vec![];
 for s in surface.scenarios(){let t=c.states().iter().find(|t|t.id==s.id).ok_or("PROB_STATE_MAPPING_MISSING")?;
  need(t.kind==semantics::StateKind::PartyWinner&&s.payout_micros.len()==sides.len(),"PROB_ORDINARY_STATE")?;
  for(i,side)in sides.iter().enumerate(){need(surface.contracts()[i].side==ns(*side)&&t.outcomes[i].payout(*side,1,0)?==s.payout_micros[i],"PROB_ORDINARY_PAYOUT_MISMATCH")?;}
  need(used.insert(t.id.clone())&&!exceptions.contains_key(&t.id),"PROB_DUPLICATE_SCENARIO")?;out.push(Scenario{id:t.id.clone(),weight:weights[&s.id],outcomes:t.outcomes.clone()});
 }
 for(id,w)in exceptions{let t=c.states().iter().find(|t|&t.id==id).ok_or("PROB_STATE_MAPPING_MISSING")?;need(used.insert(id.clone()),"PROB_DUPLICATE_SCENARIO")?;out.push(Scenario{id:t.id.clone(),weight:*w,outcomes:t.outcomes.clone()});}
 need(used.len()==c.states().len(),"PROB_UNWEIGHTED_STATE")?;Ok(out)
}
fn fifo(lots:&[(u64,i128)],q:u64)->Result<i128>{let mut left=q;let mut cost=0;for(n,p)in lots{let take=left.min(*n);cost=add(cost,mul(*p,take as i128)?)?;left-=take;if left==0{return Ok(cost)}}need(left==0,"PROB_UNCOVERED_SELL")?;Ok(cost)}
fn fixed(levels:&[moloch_relationship_arbitrage::Level],q:u64)->Option<i128>{let mut left=q;for l in levels{left=left.saturating_sub(l.quantity);if left==0{return Some(l.price)}}None}
/// Finite joint optimizer, not independent per-leg ranking. Every lattice
/// point is evaluated against actual held FIFO lots and one shared cash pool.
pub fn optimize(bank:&Session,log:&DispatchLog,scope:&Scope,surface:&BoundSurface,c:&Compiled,p:&Policy,now:u64)->Result<Option<Plan>>{
 c.validate_time(now)?;surface.validate_current(bank,scope,now)?;let cfg=&p.input;let native=bank.engine().policy();let head=bank.durable_head();
 need(bank.engine().is_live()&&bank.live_account_seed().is_some_and(|s|s["runId"]==cfg.run_id)&&c.run_id()==cfg.run_id&&bank.manifest_hash().as_str()==Some(&cfg.bank_manifest_sha256)&&c.bank_manifest_sha256()==cfg.bank_manifest_sha256
  &&scope.tournament_id==c.tournament_id()&&scope.tournament_slug==c.tournament_slug()&&head==*surface.bound_head()&&now<cfg.valid_until_ms&&now<surface.valid_until_ms()&&!bank.engine().package_blocks_submissions(),"PROB_CONTEXT")?;
 log.require_resolved_attempts()?;let exchanges:BTreeSet<_>=c.contracts().iter().map(|x|x.exchange_id.as_str()).collect();
 let mut occupied_markets=BTreeSet::new();for q in bank.engine().order_records(){if q["retired"]==false&&q["action"]=="buy"{occupied_markets.insert(q["exchangeId"].as_str().ok_or("PROB_ORDER_SCOPE")?.to_owned());}if q["exchangeId"].as_str().is_some_and(|e|exchanges.contains(e)){let id=q["id"].as_u64().ok_or("PROB_ORDER")?;need(q["retired"]==true&&(!bank.engine().live_order_bindings().contains_key(&id)||log.closed.contains(&id)),"PROB_PENDING_COMPONENT")?;}}
 for pkg in bank.engine().package_records().as_array().ok_or("PROB_PACKAGES")?{if pkg["phase"]=="no_execution"||pkg["protectionReleased"]==true{continue}need(!pkg["legs"].as_array().ok_or("PROB_PACKAGES")?.iter().any(|l|l["exchangeId"].as_str().is_some_and(|e|exchanges.contains(e))),"PROB_PROTECTED_COMPONENT")?;}
 need(native["feeBps"]==0&&decimal_units(native["feePerShare"].as_str().ok_or("PROB_FEE")?)?==cfg.fee_per_share,"PROB_FEE_POLICY")?;
 let positions=bank.engine().wallet.positions();for(ex,s)in positions.as_object().ok_or("PROB_POSITIONS")?{if ["YES","NO"].iter().any(|k|s[*k].as_array().is_some_and(|lots|!lots.is_empty())){occupied_markets.insert(ex.clone());}}let mut held=vec![];let mut options=vec![];let mut baseline_basis=0;
 let(books,book_hashes)=package_admission::native_books(bank,scope,now,cfg.maximum_book_age_ms)?;
 for(i,contract)in c.contracts().iter().enumerate(){need(scope.exchanges.get(&contract.exchange_id)==Some(&contract.market_id),"PROB_SCOPE")?;let side=if surface.contracts().get(i).ok_or("PROB_MODEL_CONTRACT")?.side==NativeSide::Yes{Side::Yes}else{Side::No};
  need(bank.engine().wallet.quantity(&contract.exchange_id,ns(if side==Side::Yes{Side::No}else{Side::Yes}))==0,"PROB_OPPOSITE_HELD_NETTING_UNSUPPORTED")?;
  let mut lots=vec![];if let Some(rows)=positions[&contract.exchange_id][label(side)].as_array(){for l in rows{let n=l["quantity"].as_u64().ok_or("PROB_LOT")?;let price=decimal_units(l["price"].as_str().ok_or("PROB_LOT")?)?;need(n>0&&price>=0&&price<=UNIT,"PROB_LOT")?;lots.push((n,price));}}
  let quantity=bank.engine().wallet.quantity(&contract.exchange_id,ns(side));need(lots.iter().try_fold(0u64,|a,l|a.checked_add(l.0)).ok_or("PROB_RANGE")?==quantity,"PROB_LOT_QUANTITY")?;let principal=fifo(&lots,quantity)?;baseline_basis=add(baseline_basis,principal)?;held.push(Instrument{side,quantity,principal,lots});
  let book=books.get(&contract.exchange_id).ok_or("PROB_FRESH_BOOK_REQUIRED")?;let asks=if side==Side::Yes{&book.yes_asks}else{&book.no_asks};let bids=if side==Side::Yes{&book.yes_bids}else{&book.no_bids};let mut choices=vec![None];
  for qty in &cfg.quantities{for buy in [true,false]{if !buy&&*qty>quantity{continue}let Some(price)=fixed(if buy{asks}else{bids},*qty)else{continue};let value=mul(price,*qty as i128)?;let fee=mul(cfg.fee_per_share,*qty as i128)?;
   let review=&surface.contracts()[i];if *qty>review.maximum_quantity||add(value,fee)?>review.maximum_principal{continue}if buy&&(!review.allowed_modes.contains(&crate::alpha_proposal::Mode::BoundedLimitMayRest)||add(add(price,cfg.fee_per_share)?,review.margin_per_share)?>review.expected_payout_micros||add(add(principal,value)?,fee)?>review.maximum_principal||quantity.checked_add(*qty).is_none_or(|q|q>review.maximum_quantity)){continue}
   choices.push(Some(OptionLeg{leg:Leg{market_id:contract.market_id.clone(),exchange_id:contract.exchange_id.clone(),contract_id:contract.contract_id.clone(),side,action:if buy{"buy"}else{"sell"}.into(),quantity:*qty,limit_price:price,book_sha256:book_hashes[&contract.exchange_id].clone()},cost:if buy{value}else{0},proceeds:if buy{0}else{value},fee,sold_principal:if buy{0}else{fifo(&held[i].lots,*qty)?}}));
  }}options.push(choices);
 }
 let states=scenarios(surface,c,&held.iter().map(|h|h.side).collect::<Vec<_>>())?;need(cfg.factors.values().all(|f|f.loading_ppm.len()==held.len()),"PROB_FACTOR_SHAPE")?;
 let count=options.iter().try_fold(1u64,|n,o|n.checked_mul(o.len()as u64)).ok_or("PROB_SEARCH_LIMIT")?;need(count<=cfg.maximum_enumerations,"PROB_SEARCH_LIMIT")?;
 let reserved=bank.engine().wallet.reserved()?;let available=bank.engine().wallet.cash.checked_sub(reserved).ok_or("PROB_RANGE")?;let committed=add(bank.engine().wallet.owned_cost(),reserved)?;let total_cap=decimal_units(native["maximumCommitted"].as_str().ok_or("PROB_CAP")?)?;let market_cap=decimal_units(native["maximumMarketCost"].as_str().ok_or("PROB_CAP")?)?;let maxq=native["quantity"].as_u64().ok_or("PROB_CAP")?;
 let mut best:Option<(Vec<Leg>,Metrics,i128)>=None;
 for code in 1..count{let mut n=code;let mut selected=vec![];for choices in &options{selected.push(choices[(n%choices.len()as u64)as usize].as_ref());n/=choices.len()as u64;}let cost=selected.iter().flatten().try_fold(0,|s,o|add(s,o.cost))?;let proceeds=selected.iter().flatten().try_fold(0,|s,o|add(s,o.proceeds))?;let fees=selected.iter().flatten().try_fold(0,|s,o|add(s,o.fee))?;let funding=add(cost,fees)?;
  if funding>available||funding>cfg.maximum_gross_buy_cost||add(committed,funding)?>total_cap||selected.iter().flatten().map(|o|o.leg.quantity).sum::<u64>()>maxq{continue}
  let buys=selected.iter().flatten().any(|o|o.cost>0);if buys&&(native["entriesEnabled"]!=true||now>=crate::daily_windows::clock(native["entryStopAt"].as_str().ok_or("PROB_ENTRY_CLOCK")?)?){continue}
  let new_markets:BTreeSet<_>=selected.iter().flatten().filter(|o|o.cost>0&&bank.engine().wallet.market_cost(&o.leg.exchange_id)==0).map(|o|o.leg.exchange_id.as_str()).collect();
  let all_markets=occupied_markets.iter().map(|e|e.as_str()).chain(new_markets.iter().copied()).collect::<BTreeSet<_>>().len();if new_markets.len()>3||all_markets>native["maximumMarkets"].as_u64().ok_or("PROB_CAP")? as usize{continue}
  if selected.iter().flatten().any(|o|o.cost>0&&bank.engine().wallet.market_cost(&o.leg.exchange_id).checked_add(o.cost).and_then(|v|v.checked_add(o.fee)).is_none_or(|v|v>market_cap)){continue}
  let mut factor_ok=true;for f in cfg.factors.values(){let mut exposure=0;for(i,h)in held.iter().enumerate(){let delta=selected[i].map(|o|if o.cost>0{o.leg.quantity as i128}else{-(o.leg.quantity as i128)}).unwrap_or(0);exposure=add(exposure,mul(h.quantity as i128+delta,f.loading_ppm[i])?)?;}if exposure.checked_abs().ok_or("PROB_RANGE")?>f.maximum_absolute_shares_micros{factor_ok=false}}if !factor_ok{continue}
  let opportunity=ceil(mul(mul(funding,cfg.capital_rate_ppm_per_day as i128)?,surface.horizon_ms()as i128)?,UNIT*86_400_000)?;
  let mut expected=0;let mut wealth=0;let mut increments=vec![];let mut terminal=vec![];let mut minimum=i128::MAX;
  for s in &states{let mut before=0;let mut after=0;for(i,h)in held.iter().enumerate(){before=add(before,s.outcomes[i].payout(h.side,h.quantity,h.principal)?)?;let(mut q,mut basis)=(h.quantity,h.principal);if let Some(o)=selected[i]{if o.cost>0{q=q.checked_add(o.leg.quantity).ok_or("PROB_RANGE")?;basis=add(basis,o.cost)?;}else{q-=o.leg.quantity;basis-=o.sold_principal;}}after=add(after,s.outcomes[i].payout(h.side,q,basis)?)?;}
   let delta=add(add(after-before,proceeds)?, -funding)?;let gain=delta.checked_sub(opportunity).ok_or("PROB_RANGE")?;minimum=minimum.min(gain);expected=add(expected,mul(gain,s.weight as i128)?)?;let w=add(add(bank.engine().wallet.cash,after)?,proceeds-funding)?;wealth=add(wealth,mul(w,s.weight as i128)?)?;increments.push((gain,s.weight));terminal.push((add(add(after,proceeds-funding)?, -baseline_basis)?,s.weight));
  }
  if expected<mul(cfg.minimum_expected_gain,UNIT)?{continue}let incremental_cvar=cvar_loss(&increments,cfg.tail_ppm)?;let terminal_cvar=cvar_loss(&terminal,cfg.tail_ppm)?;if incremental_cvar>cfg.maximum_incremental_cvar_loss||terminal_cvar>cfg.maximum_terminal_cvar_loss{continue}
  let metrics=Metrics{expected_wealth_numerator:wealth,expected_increment_numerator:expected,capital_opportunity_cost:opportunity,incremental_cvar_loss:incremental_cvar,terminal_cvar_loss:terminal_cvar,minimum_modeled_increment:minimum,gross_buy_funding:funding,minimum_sell_proceeds:proceeds,fees,modeled_sleeve_basis:baseline_basis};let legs=selected.iter().flatten().map(|o|o.leg.clone()).collect::<Vec<_>>();if best.as_ref().is_none_or(|(_,_,score)|expected>*score){best=Some((legs,metrics,expected));}
 }
 let Some((legs,metrics,_))=best else{return Ok(None)};let expires=now.checked_add(cfg.maximum_plan_age_ms).ok_or("PROB_RANGE")?.min(cfg.valid_until_ms).min(c.valid_until_ms()).min(surface.valid_until_ms());let report_sha=hash(&bank.engine().report()?)?;
 let dispatch_sha=hash(&log.descriptor_identity()?)?;let proof=json!({"version":1,"classification":"reviewed_probabilistic_portfolio","guaranteedArbitrage":false,"realizedProfit":false,"usesFutureCollateral":false,"tournamentId":scope.tournament_id,"bankHead":head,"reportSha256":report_sha,"dispatchSha256":dispatch_sha,"policySha256":p.sha,"modelSha256":surface.source_sha256(),"modelQualification":surface.surface().qualification(),"semanticReviewSha256":c.review_sha256(),"atMs":now,"expiresAtMs":expires,"horizonMs":surface.horizon_ms(),"legs":legs,"metrics":metrics,"excludedStates":c.excluded_states(),"exceptionWeightsPpm":c.exception_weights_ppm(),"wealthScope":"reviewed sleeve plus actual cash; unrelated holdings and debt excluded","executionProbability":null});
 Ok(Some(Plan{head,report_sha,dispatch_sha,policy_sha:p.sha.clone(),model_sha:surface.source_sha256().into(),semantic_sha:c.review_sha256().into(),legs,metrics,expires,proof}))
}
