//! Source-bound bridge from the certified payoff scanner to native funding.
//! Construction re-runs the scanner. A deserialized candidate is never authority.
use crate::{authority::Activation,daily_windows,feedback::Result,relationship_census::Census,relationship_strategy::{self as strategy,Capital,Certificates,Policy}};
use baal_transport_service::{Receipt,multi_leg,decimal_units};
use moloch_relationship_arbitrage::{Book,Level,Side,Source,UNIT};
use moloch_replay::{money,session::Session};
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet};
fn hash(v:&Value)->Result<String>{Ok(format!("{:x}",Sha256::digest(serde_json::to_vec(v)?)))}
fn need(ok:bool,why:&'static str)->Result<()>{if ok{Ok(())}else{Err(why.into())}}
fn levels(raw:&Value,key:&str,ascending:bool)->Result<Vec<Level>>{
 let rows=raw[key].as_array().filter(|a|!a.is_empty()&&a.len()<=200).ok_or("PACKAGE_BOOK_DEPTH")?;
 let mut previous=if ascending{0}else{UNIT};let mut out=vec![];
 for row in rows{let p=decimal_units(row["price"].as_str().ok_or("PACKAGE_BOOK_DECIMAL")?)?;let n=row["quantity"].as_u64().filter(|n|*n>0).ok_or("PACKAGE_BOOK_QUANTITY")?;
  need((5000..=995000).contains(&p)&&p%5000==0&&(if ascending{p>=previous}else{p<=previous}),"PACKAGE_BOOK_LEVEL")?;previous=p;out.push(Level{price:p,quantity:n});
 }Ok(out)
}
/// Only books already admitted by this bank's scoped official source adapter.
/// A source gap is checked again by the native reservation transition.
pub fn native_books(bank:&Session,scope:&baal_transport_service::Scope,at:u64,age:u64)->Result<(BTreeMap<String,Book>,BTreeMap<String,String>)>{
 let mut books=BTreeMap::new();let mut hashes=BTreeMap::new();
 for raw in bank.engine().live_package_books(){let b=&raw;let ex=b["exchangeId"].as_str().ok_or("PACKAGE_BOOK_SCOPE")?;
  let Some(market)=scope.exchanges.get(ex)else{continue};need(b["marketId"]==*market&&b["tournamentId"]==scope.tournament_id,"PACKAGE_BOOK_SCOPE")?;
  let received=daily_windows::clock(b["receivedAt"].as_str().ok_or("PACKAGE_BOOK_CLOCK")?)?;
  let source=daily_windows::clock(b["sourceAt"].as_str().ok_or("PACKAGE_BOOK_CLOCK")?)?;
  let request=daily_windows::clock(b["requestStartedAt"].as_str().ok_or("PACKAGE_BOOK_CLOCK")?)?;
  let observed=if b["transport"]=="official_sig_realtime"{source}else{source.min(request)};
  if received>at||observed>at||at-received>age||at-observed>age{continue}
  let bids=levels(b,"bids",false)?;let asks=levels(b,"asks",true)?;need(bids[0].price<asks[0].price,"PACKAGE_CROSSED_BOOK")?;
  let digest=hash(b)?;let no_asks=bids.iter().map(|l|Level{price:UNIT-l.price,quantity:l.quantity}).collect();let no_bids=asks.iter().map(|l|Level{price:UNIT-l.price,quantity:l.quantity}).collect();
  need(books.insert(ex.to_owned(),Book{exchange_id:ex.into(),source:Source{sha256:digest.clone(),received_ms:received},observed_ms:observed,yes_asks:asks,no_asks,yes_bids:bids,no_bids}).is_none(),"PACKAGE_DUPLICATE_BOOK")?;hashes.insert(ex.into(),digest);
 }Ok((books,hashes))
}
fn worst_limit(levels:&[Level],quantity:u64)->Result<i128>{let mut left=quantity;for level in levels{left=left.saturating_sub(level.quantity);if left==0{return Ok(level.price)}}Err("PACKAGE_DEPTH_MISSING".into())}
#[derive(Clone)]
pub struct Admission{proof:Value,proof_hash:String,package_id:String,basket:multi_leg::Basket,reserve:Value,expires:u64,certificate:String,rule_bindings:BTreeMap<String,Value>}
impl Admission{
 pub fn proof(&self)->&Value{&self.proof}
 pub fn proof_hash(&self)->&str{&self.proof_hash}
 pub fn package_id(&self)->&str{&self.package_id}
 pub fn basket(&self)->&multi_leg::Basket{&self.basket}
 pub fn reserve_event(&self)->&Value{&self.reserve}
 pub fn expires_at_ms(&self)->u64{self.expires}
 pub fn certificate_sha256(&self)->&str{&self.certificate}
 /// V1 keeps its exact original receipt contract. V2 may refresh transport
 /// clocks, but every reviewed rule-body field and exact endpoint stay pinned.
 pub(crate) fn validate_physical_rule(&self,exchange:&str,r:&Receipt)->Result<()>{
  let b=self.rule_bindings.get(exchange).ok_or("PACKAGE_RULE_BINDING")?;
  need(r.status==200&&r.endpoint==b["endpoint"]&&self.proof["ruleBindings"][exchange]==*b,"PACKAGE_RULE_BINDING")?;
  match b["kind"].as_str(){Some("exact_receipt_v1")=>need(hash(r)?==b["sha256"],"PACKAGE_RULE_RECEIPT_CHANGED"),Some("complete_rule_body_v2")=>need(hash(&r.data)?==b["sha256"],"PACKAGE_RULE_BODY_CHANGED"),_=>Err("PACKAGE_RULE_BINDING".into())}
 }
}
/// Select the best *recomputed* certified package fitting the unchanged bank.
/// No new API calls, ledger changes, collateral borrowing or physical dispatch.
pub fn admit(bank:&Session,a:&Activation,census:&Census,canonical:&Receipt,certificates:&Certificates<'_>,gross_cash:i128,p:&Policy,entry_until:u64)->Result<Option<Admission>>{
 need(bank.engine().is_live()&&bank.live_account_seed().is_some_and(|s|s["runId"]==a.run_id)&&census.cache.scope.tournament_id==a.tournament_id&&census.cache.scope.tournament_slug==a.tournament_slug,"PACKAGE_BANK_SCOPE")?;
 let policy=bank.engine().policy();need(policy["maximumCommitted"]==a.maximum_committed&&policy["maximumMarketCost"]==a.maximum_market_cost&&policy["quantity"].as_u64()==Some(a.maximum_quantity as u64)&&policy["entriesEnabled"]==true,"PACKAGE_NATIVE_POLICY")?;
 need(daily_windows::clock(policy["entryStopAt"].as_str().ok_or("PACKAGE_ENTRY_CLOCK")?)?==entry_until&&p.now_ms<entry_until,"PACKAGE_ENTRY_EXPIRED")?;
 need(p.entry_fee_per_share_per_leg==0&&policy["feeBps"].as_u64()==Some(0)&&decimal_units(policy["feePerShare"].as_str().ok_or("PACKAGE_FEES_UNKNOWN")?)?==0&&p.buffer_per_share>=0&&p.minimum_surplus>0&&p.maximum_quantity>0,"PACKAGE_FEES_OR_MARGIN")?;
 let report=bank.engine().report()?;let reserved=decimal_units(report["reservedCash"].as_str().ok_or("PACKAGE_BANK_REPORT")?)?;
 let remaining=decimal_units(&a.maximum_committed)?.checked_sub(bank.engine().wallet.owned_cost()).and_then(|n|n.checked_sub(reserved)).ok_or("PACKAGE_CAPITAL_RANGE")?;
 need(remaining>=0&&gross_cash>=0,"PACKAGE_CAPITAL_RANGE")?;
 let available=bank.engine().wallet.cash.min(gross_cash).checked_sub(reserved).ok_or("PACKAGE_CAPITAL_RANGE")?;need(available>=0,"PACKAGE_RESERVATIONS_EXCEED_CASH")?;
 let capital=Capital{gross_account_cash:gross_cash,attributed_bank_cash:bank.engine().wallet.cash,reserved_cash:reserved,maximum_committed_remaining:remaining,package_cost_cap:remaining.min(available),orphan_cash_loss_cap:remaining.min(available)};
 let(book_map,hashes)=native_books(bank,&census.cache.scope,p.now_ms,p.max_book_age_ms.min(policy["bookAgeMs"].as_u64().ok_or("PACKAGE_NATIVE_POLICY")?))?;
 let analysis=strategy::analyze(census,canonical,certificates,&book_map,&[],&capital,p);
 let mut selected:Option<(i128,Admission)>=None;
 for candidate in analysis.candidates{
  let plan=&candidate.plan;if !(2..=3).contains(&plan.instruments.len()){continue}
  let quantity=plan.quantity;let total_quantity=quantity.checked_mul(plan.instruments.len() as u64).ok_or("PACKAGE_QUANTITY_RANGE")?;if total_quantity>a.maximum_quantity as u64||quantity>u32::MAX as u64{continue}
  let mut seen=BTreeSet::new();let mut legs=vec![];let mut native=vec![];let mut wire_cost=0i128;let mut occupied=false;
  for instrument in &plan.instruments{
   let(ex,market,_)=&candidate.contracts[instrument.contract];if !seen.insert(ex.clone()){occupied=true;break}
   if bank.engine().wallet.quantity(ex,moloch_replay::Side::Yes)>0||bank.engine().wallet.quantity(ex,moloch_replay::Side::No)>0||bank.engine().order_records().iter().any(|q|q["exchangeId"]==*ex&&q["retired"]==false){occupied=true;break}
   let book=book_map.get(ex).ok_or("PACKAGE_BOOK_MISSING")?;let asks=if instrument.side==Side::Yes{&book.yes_asks}else{&book.no_asks};let price=worst_limit(asks,quantity)?;let leg_cost=price.checked_mul(quantity as i128).ok_or("PACKAGE_COST_RANGE")?;
   if leg_cost>decimal_units(&a.maximum_market_cost)?{occupied=true;break}wire_cost=wire_cost.checked_add(leg_cost).ok_or("PACKAGE_COST_RANGE")?;
   legs.push(multi_leg::Leg{exchange_id:ex.clone(),side:if instrument.side==Side::Yes{multi_leg::Side::Yes}else{multi_leg::Side::No},quantity:quantity as u32,limit_price:money(price)});
   native.push(json!({"marketId":market,"exchangeId":ex,"side":if instrument.side==Side::Yes{"YES"}else{"NO"},"quantity":quantity,"limitPrice":money(price),"bookHash":hashes[ex]}));
  }
  if occupied{continue}
  let surplus=plan.terminal_floor.checked_sub(wire_cost).and_then(|n|n.checked_sub(plan.buffer)).ok_or("PACKAGE_COST_RANGE")?;
  if wire_cost<=0||wire_cost>available||wire_cost>remaining||surplus<p.minimum_surplus{continue}
  let relationship_end=daily_windows::clock(&canonical.received_at)?.checked_add(p.max_relationship_age_ms).ok_or("PACKAGE_CLOCK_RANGE")?;
  let certificate=certificates.values.iter().find(|c|strategy::certificate_sha256(c).ok().as_deref()==Some(&candidate.certificate_sha256)).ok_or("PACKAGE_CERTIFICATE_MISSING")?;
  // The intent retains frozen limits, not a promise that its original book is
  // still executable. Reserve, attempt and physical authorization independently
  // recheck CURRENT books. A ten-second original-book expiry made paced fresh
  // preflight plus the native activation delay infeasible.
  let mut expires=p.now_ms.checked_add(policy["lifetimeMs"].as_u64().filter(|n|*n>0).ok_or("PACKAGE_NATIVE_POLICY")?).ok_or("PACKAGE_CLOCK_RANGE")?.min(entry_until).min(relationship_end).min(certificate.expires_at_ms);
  let mut rule_bindings=BTreeMap::new();
  for c in &certificate.contracts{let r=strategy::validated_rule_receipt(certificate,c,certificates,&census.cache.scope,p)?;expires=expires.min(daily_windows::clock(&r.received_at)?.checked_add(p.max_rule_age_ms).ok_or("PACKAGE_CLOCK_RANGE")?);let(kind,digest)=if certificate.version==1{("exact_receipt_v1",hash(r)?)}else{("complete_rule_body_v2",strategy::rule_data_sha256(r)?)};need(rule_bindings.insert(c.exchange_id.clone(),json!({"kind":kind,"endpoint":r.endpoint,"sha256":digest})).is_none(),"PACKAGE_RULE_BINDING")?;}
  let _required_surplus=p.minimum_surplus.checked_add(plan.buffer).ok_or("PACKAGE_COST_RANGE")?;
  if expires<=p.now_ms.checked_add(policy["latencyMs"].as_u64().ok_or("PACKAGE_NATIVE_POLICY")?).ok_or("PACKAGE_CLOCK_RANGE")?{continue}
  let proof=json!({"version":1,"runId":a.run_id,"bankManifestHash":bank.manifest_hash(),"tournamentId":a.tournament_id,"atMs":p.now_ms,"entryUntilMs":entry_until,"expiresAtMs":expires,"candidate":candidate.private_json(),"wireLegs":native,"maximumCost":money(wire_cost),"minimumPayout":money(plan.terminal_floor),"buffer":money(plan.buffer),"minimumSurplus":money(p.minimum_surplus),"maximumCommitted":a.maximum_committed,"maximumMarketCost":a.maximum_market_cost,"maximumQuantity":a.maximum_quantity,"usesFutureCollateral":false});
  let mut proof=proof;proof["ruleBindings"]=json!(rule_bindings);
  let digest=hash(&proof)?;let id=format!("pkg-{digest}");let wire_key=format!("{}:{id}",a.run_id);need(wire_key.len()<=255,"PACKAGE_ID_LENGTH")?;
  let basket=multi_leg::Basket{idempotency_key:wire_key,tournament_id:a.tournament_id.clone(),legs,maximum_cost:money(wire_cost),relationship_constraint:Some(candidate.relationship_id.clone())};
  let stamp=daily_windows::iso(p.now_ms)?;let reserve=json!({"type":"live_package_reserve","source":"moloch_live_dispatcher","receivedAt":stamp,"processedAt":stamp,"packageId":id,"proofHash":digest,"bankScope":{"runId":a.run_id,"bankManifestHash":bank.manifest_hash(),"tournamentId":a.tournament_id},"expiresAt":daily_windows::iso(expires)?,"economics":{"maximumCost":money(wire_cost),"minimumPayout":money(plan.terminal_floor),"minimumSurplus":money(plan.terminal_floor-wire_cost),"feeTotal":"0.000000"},"legs":native});
  let admitted=Admission{proof,proof_hash:digest,package_id:id,basket,reserve,expires,certificate:candidate.certificate_sha256,rule_bindings};
  if selected.as_ref().is_none_or(|(best,_)|surplus>*best){selected=Some((surplus,admitted));}
 }
 Ok(selected.map(|(_,v)|v))
}
