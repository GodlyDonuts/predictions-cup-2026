//! Dedicated probabilistic reservation. These reviewed probabilities are NOT
//! authenticated forecasts or a guaranteed payout certificate. Runtime must
//! independently verify opaque reviewed source/config/model/physical evidence.
//! Native recomputes their arithmetic using actual FIFO inventory and funding.
//! Genuine actual ACK/fill/terminal events retain their existing sole meaning.
use super::*;
fn add(a:i128,b:i128)->Result<i128>{a.checked_add(b).ok_or("PROBABILISTIC_RANGE")}
fn mul(a:i128,b:i128)->Result<i128>{a.checked_mul(b).ok_or("PROBABILISTIC_RANGE")}
fn integer(v:&Value)->Result<i128>{let s=v.as_str().ok_or("PROBABILISTIC_INTEGER")?;let n=s.parse::<i128>().map_err(|_|"PROBABILISTIC_INTEGER")?;if n.to_string()!=s{return Err("PROBABILISTIC_INTEGER")};Ok(n)}
fn nonnegative(v:&Value)->Result<i128>{let n=dec(v)?;if n<0{return Err("PROBABILISTIC_NEGATIVE")};Ok(n)}
fn ceil(a:i128,b:i128)->Result<i128>{if a<0||b<=0{return Err("PROBABILISTIC_RANGE")};Ok(add(a,b-1)?/b)}
fn payout(rule:&Value,s:Side,q:u64,principal:i128)->Result<i128>{match rule.as_str(){Some("resolvedYes")=>Ok(if s==Side::Yes{mul(q as i128,SCALE)?}else{0}),Some("resolvedNo")=>Ok(if s==Side::No{mul(q as i128,SCALE)?}else{0}),Some("refundPrincipal")=>Ok(principal),_=>Err("PROBABILISTIC_PAYOUT_RULE")}}
fn cvar(values:&[(i128,u64)],tail:u64)->Result<i128>{let mut rows=values.to_vec();rows.sort_by_key(|r|r.0);let mut left=tail;let mut sum=0;for(g,w)in rows{let n=left.min(w);let loss=if g<0{g.checked_neg().ok_or("PROBABILISTIC_RANGE")?}else{0};sum=add(sum,mul(loss,n as i128)?)?;left-=n;if left==0{break}}if left!=0{return Err("PROBABILISTIC_CVAR_MASS")};ceil(sum,tail as i128)}
fn fifo(engine:&Engine,ex:&str,s:Side,q:u64)->Result<i128>{let mut left=q;let mut basis=0;if let Some(lots)=engine.wallet.lots.get(&(ex.into(),s)){for lot in lots{let n=left.min(lot.quantity);basis=add(basis,mul(lot.price,n as i128)?)?;left-=n;if left==0{break}}}if left!=0{return Err("PROBABILISTIC_UNCOVERED_SELL")};Ok(basis)}
impl Engine{
 pub(crate) fn apply_probabilistic_package(&mut self,e:&Value,at:i64)->Result<()>{
  if !self.live_mode||e["source"]!="moloch_live_dispatcher"{return Err("PROBABILISTIC_LIVE_SOURCE")}
  if e["type"]=="probabilistic_portfolio_reserve_v1"{return self.reserve_probabilistic_package(e,at)}
  let mapped=match e["type"].as_str(){Some("probabilistic_portfolio_attempt_v1")=>"live_package_attempt",Some("probabilistic_portfolio_unknown_v1")=>"live_package_unknown",Some("probabilistic_portfolio_no_execution_v1")=>"live_package_no_execution",Some("probabilistic_portfolio_ack_v1")=>"live_package_ack",Some("probabilistic_portfolio_complete_v1")=>"live_package_complete",_=>return Err("PROBABILISTIC_EVENT")};
  let mut internal=e.clone();internal["type"]=json!(mapped);self.apply_package_kind(&internal,at,true)
 }
 fn reserve_probabilistic_package(&mut self,e:&Value,at:i64)->Result<()>{
  let name=package_id(&e["packageId"])?;if self.live_packages.contains_key(&name)||self.package_blocks_new_submissions(){return Err("PROBABILISTIC_DUPLICATE_OR_UNRESOLVED")}
  if self.policy.fee_bps!=0||self.policy.fee_share!=0{return Err("PROBABILISTIC_ZERO_FEES_ONLY")}
  if e.get("portfolio").is_some()||e.get("economics").is_some(){return Err("PROBABILISTIC_NOT_GUARANTEED_ECONOMICS")}
  let scope=e["bankScope"].clone();exact_keys(&scope,&["runId","bankManifestHash","tournamentId"])?;
  if scope["runId"].as_str().is_none_or(|s|s.is_empty()||s.len()>128)||scope["tournamentId"].as_str().is_none_or(|s|s.is_empty())||digest(&scope["bankManifestHash"]).is_err()||self.live_packages.values().any(|p|p.scope!=scope){return Err("PROBABILISTIC_SCOPE")}
  let qualification=e["qualification"].clone();exact_keys(&qualification,&["version","kind","planSha256","semanticReviewSha256","modelSha256","policySha256","dispatchSha256","positionsSha256","validUntilMs","contracts","states","risk","metrics","wireLegs"])?;
  if qualification["version"]!=1||qualification["kind"]!="reviewed_probabilistic_portfolio"{return Err("PROBABILISTIC_CLASS")}
  for k in ["planSha256","semanticReviewSha256","modelSha256","policySha256","dispatchSha256","positionsSha256"]{digest(&qualification[k])?;}
  let proof=digest(&e["proofHash"])?;if proof!=hash(&qualification)||qualification["positionsSha256"]!=hash(&self.wallet.positions())||qualification["wireLegs"]!=e["legs"]{return Err("PROBABILISTIC_PROOF_OR_POSITIONS")}
  let valid=qualification["validUntilMs"].as_i64().ok_or("PROBABILISTIC_CLOCK")?;let expires=ms(&e["expiresAt"])?;
  if expires<=at.checked_add(self.policy.latency).ok_or("PROBABILISTIC_RANGE")?||expires>valid||expires.checked_sub(at).is_none_or(|n|n>self.policy.lifetime){return Err("PROBABILISTIC_EXPIRY")}
  let rows=qualification["contracts"].as_array().filter(|x|!x.is_empty()&&x.len()<=6).ok_or("PROBABILISTIC_CONTRACT_COUNT")?;
  let mut contracts=Vec::new();let mut components=BTreeSet::new();let mut basis=0;
  for r in rows{exact_keys(r,&["marketId","exchangeId","contractId","side","officialRootSha256"])?;let ex=id(&r["exchangeId"])?;let market=id(&r["marketId"])?;id(&r["contractId"])?;digest(&r["officialRootSha256"])?;let s=side(&r["side"])?;
   if !components.insert(ex.clone())||self.wallet.quantity(&ex,s.other())!=0||self.package_protects_exchange(&ex)||self.orders.iter().any(|q|q.exchange==ex&&!q.retired){return Err("PROBABILISTIC_COMPONENT_LIABILITY")}
   let b=self.books.get(&ex).ok_or("PROBABILISTIC_BOOK")?;if !b.fresh(&self.policy,at)||at-b.source>self.policy.book_age||b.market!=market||b.tournament!=scope["tournamentId"].as_str().unwrap(){return Err("PROBABILISTIC_CONTRACT_SCOPE")}
   let q=self.wallet.quantity(&ex,s);let principal=fifo(self,&ex,s,q)?;basis=add(basis,principal)?;contracts.push((ex,market,s,q,principal));
  }
  let wire=e["legs"].as_array().filter(|x|!x.is_empty()&&x.len()<=6).ok_or("PROBABILISTIC_LEG_COUNT")?;let mut legs=Vec::new();let mut seen=BTreeSet::new();let mut funding=0;let mut proceeds=0;let mut changes=BTreeMap::new();let mut aggregate=0u64;
  let mut markets:BTreeSet<_>=self.wallet.lots.iter().filter(|(_,lots)|!lots.is_empty()).map(|((ex,_),_)|ex.clone()).collect();markets.extend(self.orders.iter().filter(|q|!q.retired).map(|q|q.exchange.clone()));
  for w in wire{exact_keys(w,&["marketId","exchangeId","side","action","quantity","limitPrice","bookHash"])?;let ex=id(&w["exchangeId"])?;let market=id(&w["marketId"])?;let s=side(&w["side"])?;let buy=match w["action"].as_str(){Some("buy")=>true,Some("sell")=>false,_=>return Err("PROBABILISTIC_ACTION")};
   let index=contracts.iter().position(|c|c.0==ex&&c.1==market&&c.2==s).ok_or("PROBABILISTIC_WIRE_SCOPE")?;if !seen.insert(ex.clone()){return Err("PROBABILISTIC_DUPLICATE_EXCHANGE")}
   let q=w["quantity"].as_u64().filter(|q|*q>0&&*q<=self.policy.quantity&&*q<=u32::MAX as u64).ok_or("PROBABILISTIC_QUANTITY")?;aggregate=aggregate.checked_add(q).ok_or("PROBABILISTIC_RANGE")?;let px=price(dec(&w["limitPrice"])?)?;let value=mul(px,q as i128)?;
   let sold=if buy{if !self.policy.entry(&ex,at)||self.policy.stop.is_none_or(|stop|expires>stop)||self.wallet.market_cost(&ex).checked_add(value).is_none_or(|v|v>self.policy.market){return Err("PROBABILISTIC_ENTRY_OR_MARKET_CAP")};funding=add(funding,value)?;0}else{let cost=fifo(self,&ex,s,q)?;proceeds=add(proceeds,value)?;cost};
   let qid=self.prior_orders.checked_add(self.orders.len()as u64).and_then(|n|n.checked_add(legs.len()as u64+1)).ok_or("PROBABILISTIC_RANGE")?;let l=Leg{qid,market,exchange:ex.clone(),side:s,buy,quantity:q,limit:px,book_hash:digest(&w["bookHash"])?,terminal:false};self.package_book(&l,at,&scope)?;changes.insert(index,(buy,q,value,sold));markets.insert(ex);legs.push(l);
  }
  if aggregate>self.policy.quantity{return Err("PROBABILISTIC_AGGREGATE_CAP")}
  let inherited=|ex:&String|self.inherited.as_ref().is_some_and(|i|i.remaining.iter().any(|((e,_),lots)|e==ex&&!lots.is_empty()));let fresh=self.inherited.as_ref().map_or(self.policy.markets,|i|i.fresh_market_limit).min(3);if markets.len()>self.policy.markets||markets.iter().filter(|ex|!inherited(ex)).count()>fresh{return Err("PROBABILISTIC_MARKET_COUNT")}
  // Full gross funding is checked BEFORE any covered SELL can produce cash.
  if funding>self.wallet.cash.checked_sub(self.wallet.reserved()?).ok_or("PROBABILISTIC_RANGE")?||add(add(self.wallet.owned_cost(),self.wallet.reserved()?)?,funding)?>self.policy.committed{return Err("PROBABILISTIC_GROSS_FUNDING")}
  let risk=&qualification["risk"];exact_keys(risk,&["minimumExpectedGain","maximumIncrementalCvarLoss","maximumTerminalCvarLoss","tailPpm","maximumGrossBuyFunding","capitalRatePpmPerDay","horizonMs","factors"])?;
  let minimum=nonnegative(&risk["minimumExpectedGain"])?;let max_incremental=nonnegative(&risk["maximumIncrementalCvarLoss"])?;let max_terminal=nonnegative(&risk["maximumTerminalCvarLoss"])?;if funding>nonnegative(&risk["maximumGrossBuyFunding"])?{return Err("PROBABILISTIC_REVIEW_FUNDING_CAP")}
  let tail=risk["tailPpm"].as_u64().filter(|n|*n>0&&*n<=1_000_000).ok_or("PROBABILISTIC_TAIL")?;let rate=risk["capitalRatePpmPerDay"].as_u64().filter(|n|*n<=u32::MAX as u64).ok_or("PROBABILISTIC_RATE")?;let horizon=risk["horizonMs"].as_u64().filter(|n|*n>0&&*n<=366*86_400_000).ok_or("PROBABILISTIC_HORIZON")?;
  let opportunity=ceil(mul(mul(funding,rate as i128)?,horizon as i128)?,SCALE*86_400_000)?;
  let factors=risk["factors"].as_object().filter(|x|x.len()<=32).ok_or("PROBABILISTIC_FACTORS")?;for f in factors.values(){exact_keys(f,&["maximumAbsoluteSharesMicros","loadingPpm"])?;let cap=integer(&f["maximumAbsoluteSharesMicros"])?;if cap<0{return Err("PROBABILISTIC_FACTORS")};let loads=f["loadingPpm"].as_array().filter(|a|a.len()==contracts.len()).ok_or("PROBABILISTIC_FACTORS")?;let mut total=0;for(i,(_,_,_,q,_))in contracts.iter().enumerate(){let loading=integer(&loads[i])?;if loading.checked_abs().is_none_or(|v|v>SCALE){return Err("PROBABILISTIC_FACTORS")};let after=changes.get(&i).map_or(*q as i128,|(buy,n,_,_)|if *buy{*q as i128+*n as i128}else{*q as i128-*n as i128});total=add(total,mul(loading,after)?)?;}if total.checked_abs().is_none_or(|n|n>cap){return Err("PROBABILISTIC_FACTOR_CAP")}}
  let states=qualification["states"].as_array().filter(|s|!s.is_empty()&&s.len()<=4096).ok_or("PROBABILISTIC_STATES")?;let mut ids=BTreeSet::new();let mut mass=0u64;let mut expected=0;let mut increments=Vec::new();let mut terminal=Vec::new();let mut modeled_min=i128::MAX;
  for state in states{exact_keys(state,&["id","weightPpm","outcomes"])?;let id=state["id"].as_str().filter(|s|!s.is_empty()&&s.len()<=128).ok_or("PROBABILISTIC_STATE_ID")?;if !ids.insert(id){return Err("PROBABILISTIC_STATE_ID")};let weight=state["weightPpm"].as_u64().filter(|w|*w<=1_000_000).ok_or("PROBABILISTIC_STATE_WEIGHT")?;mass=mass.checked_add(weight).ok_or("PROBABILISTIC_RANGE")?;let outcomes=state["outcomes"].as_array().filter(|a|a.len()==contracts.len()).ok_or("PROBABILISTIC_STATE_DIMENSION")?;
   let mut before=0;let mut after=0;for(i,(_,_,s,q,principal))in contracts.iter().enumerate(){before=add(before,payout(&outcomes[i],*s,*q,*principal)?)?;let(n,basis)=if let Some((buy,n,value,sold))=changes.get(&i){if *buy{(q.checked_add(*n).ok_or("PROBABILISTIC_RANGE")?,add(*principal,*value)?)}else{(q.checked_sub(*n).ok_or("PROBABILISTIC_UNCOVERED_SELL")?,principal.checked_sub(*sold).ok_or("PROBABILISTIC_RANGE")?)}}else{(*q,*principal)};after=add(after,payout(&outcomes[i],*s,n,basis)?)?;}
   let gain=add(add(after-before,proceeds-funding)?,-opportunity)?;expected=add(expected,mul(gain,weight as i128)?)?;modeled_min=modeled_min.min(gain);increments.push((gain,weight));terminal.push((add(after,proceeds-funding)?.checked_sub(basis).ok_or("PROBABILISTIC_RANGE")?,weight));
  }
  if mass!=1_000_000{return Err("PROBABILISTIC_STATE_MASS")};let incremental=cvar(&increments,tail)?;let terminal=cvar(&terminal,tail)?;if expected<mul(minimum,SCALE)?||incremental>max_incremental||terminal>max_terminal{return Err("PROBABILISTIC_EXPECTATION_OR_TAIL_LIMIT")}
  let metrics=&qualification["metrics"];exact_keys(metrics,&["expectedIncrementNumerator","incrementalCvarLoss","terminalCvarLoss","minimumModeledIncrement","grossBuyFunding","minimumSellProceeds","capitalOpportunityCost"])?;
  if integer(&metrics["expectedIncrementNumerator"])?!=expected||dec(&metrics["incrementalCvarLoss"])?!=incremental||dec(&metrics["terminalCvarLoss"])?!=terminal||dec(&metrics["minimumModeledIncrement"])?!=modeled_min||dec(&metrics["grossBuyFunding"])?!=funding||dec(&metrics["minimumSellProceeds"])?!=proceeds||dec(&metrics["capitalOpportunityCost"])?!=opportunity{return Err("PROBABILISTIC_RECOMPUTED_METRICS")}
  for l in &legs{if l.buy{self.wallet.reserve(&l.qid.to_string(),Reservation{exchange:l.exchange.clone(),side:l.side,quantity:l.quantity,limit:l.limit})?;}self.orders.push(Order{id:l.qid,market:l.market.clone(),exchange:l.exchange.clone(),tournament:scope["tournamentId"].as_str().unwrap().into(),side:l.side,buy:l.buy,quantity:l.quantity,filled:0,limit:l.limit,created:at,activation:at+self.policy.latency,expires,cancel:None,retired:false,credit:0,barrier:None,same:0,better:0,visible_same:0,visible_better:0,touch_remainder:0,aon:false,age_reason:false,through:false,participation:10000,conditional:false,touch_bps:0});self.package_orders.insert(l.qid,name.clone());}
  self.live_packages.insert(name.clone(),LivePackage{id:name,proof,scope,economics:json!({"kind":"probabilistic","grossBuyFunding":money(funding),"minimumSellProceeds":money(proceeds),"expectedIncrementNumerator":expected.to_string(),"guaranteedArbitrage":false}),created:at,expires,phase:Phase::Reserved,legs,portfolio:None,probabilistic:Some(qualification),protection_released:false,request:None,key:None,attempted_at:None,zero_evidence:None});Ok(())
 }
}
