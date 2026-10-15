//! Opaque, pure authorization facts for one certified atomic package request.
//! Only Admission::admit can construct the borrowed admission; deserializing a
//! proof/candidate never creates Prepared. No clients, filesystem or bank writes.
//!
//! Enclosing ProductionAuthority MUST additionally enforce unchanged control and
//! executable identity, STOP, healthy stream/deadline, authorized daily window,
//! strict queue unknown/active/manual-review and durable attempt lifecycle. The
//! sole serialized worker must clear Prepared before any subsequent bank event;
//! these immutable facts are not permission to reuse or retry a physical request.
//! Before staging, require every same-exchange attempted/ACKed order to have
//! authoritative terminal+complete-fill closure, even if absent from open-list
//! or already retired natively. An owned-order ID set alone cannot prove closure.
//! ACK/fill reconciliation and unknown-acceptance handling remain unchanged.
use crate::{authority::Activation,feedback::Result,package_admission::Admission};
use baal_transport_service::{Receipt,Scope,Permit,RequestSpec,decimal_units,multi_leg,prices,relationships};
use moloch_replay::session::Session;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet};
fn need(ok:bool,why:&'static str)->Result<()>{if ok{Ok(())}else{Err(why.into())}}
fn hash<T:Serialize>(v:&T)->Result<String>{Ok(format!("{:x}",Sha256::digest(serde_json::to_vec(v)?)))}
fn clock(v:&str)->Result<u64>{crate::daily_windows::clock(v)}
fn dec(v:&Value)->Result<i128>{Ok(decimal_units(v.as_str().ok_or("PACKAGE_AUTH_DECIMAL")?)?)}
fn same<T:Serialize>(a:&T,b:&T)->Result<bool>{Ok(serde_json::to_value(a)?==serde_json::to_value(b)?)}
fn fresh(r:&Receipt,now:u64,age:u64)->Result<()>{let start=clock(&r.request_started_at)?;let end=clock(&r.received_at)?;need(r.status==200&&start<=end&&end<=now&&now-start<=age,"PACKAGE_AUTH_STALE_RECEIPT")}
fn context<'a>(v:&'a Value,t:&str)->Result<&'a Value>{let c=v["contexts"].as_array().filter(|c|c.len()==1).ok_or("PACKAGE_AUTH_CONTEXT")?;need(c[0]["type"]=="tournament"&&c[0]["tournament"]["id"]==t,"PACKAGE_AUTH_CONTEXT")?;Ok(&c[0])}
fn id(v:&Value)->Result<String>{v.as_str().map(str::to_owned).or_else(||v.as_u64().map(|n|n.to_string())).filter(|s|!s.is_empty()).ok_or_else(||"PACKAGE_AUTH_ID".into())}
#[derive(Clone)]pub struct ComponentSnapshots{pub market:Receipt,pub contract:Receipt,pub book:Receipt}
#[derive(Clone)]pub struct PackageSnapshotSet{pub account:Receipt,pub cash_balance:Receipt,pub positions:Receipt,pub open_orders:Receipt,pub components:BTreeMap<String,ComponentSnapshots>}
struct Leg{exchange:String,market:String,contract:String,side:multi_leg::Side,quantity:u32,limit:i128}
/// No Deserialize, no public fields and no construction from serialized proof.
pub struct Prepared{spec:RequestSpec,permit:Permit,scope:Scope,legs:Vec<Leg>,facts:PackageSnapshotSet,prepared:u64,expires:u64,entry_cutoff:u64,expected_cash:i128,maximum_cost:i128,reserved_total:i128,rule_sources:BTreeSet<String>}
impl Prepared{
 pub fn new(bank:&Session,a:&Activation,scope:&Scope,admission:&Admission,permit:Permit,facts:PackageSnapshotSet,now:u64,effective_entry_cutoff:u64)->Result<Self>{
  let e=bank.engine();let proof=admission.proof();let reserve=admission.reserve_event();let basket=admission.basket();let policy=e.policy();
  need(e.is_live()&&bank.live_account_seed().is_some_and(|s|s["runId"]==a.run_id)&&a.tournament_id==scope.tournament_id&&a.tournament_slug==scope.tournament_slug,"PACKAGE_AUTH_BANK_SCOPE")?;
  need(hash(proof)?==admission.proof_hash()&&proof["runId"]==a.run_id&&proof["bankManifestHash"]==*bank.manifest_hash()&&proof["tournamentId"]==scope.tournament_id&&proof["candidate"]["certificateSha256"]==admission.certificate_sha256(),"PACKAGE_AUTH_PROOF_BINDING")?;
  need(proof["usesFutureCollateral"]==false&&proof["expiresAtMs"].as_u64()==Some(admission.expires_at_ms())&&proof["entryUntilMs"].as_u64()==Some(effective_entry_cutoff)&&proof["atMs"].as_u64().is_some_and(|at|at<=now)&&now<admission.expires_at_ms(),"PACKAGE_AUTH_PROOF_CLOCK")?;
  need(policy["maximumCommitted"]==a.maximum_committed&&policy["maximumMarketCost"]==a.maximum_market_cost&&policy["quantity"].as_u64()==Some(a.maximum_quantity as u64)&&policy["entriesEnabled"]==true&&policy["feeBps"]==0&&dec(&policy["feePerShare"])?==0&&clock(policy["entryStopAt"].as_str().ok_or("PACKAGE_AUTH_ENTRY")?)?==effective_entry_cutoff,"PACKAGE_AUTH_POLICY")?;
  need(proof["maximumCommitted"]==a.maximum_committed&&proof["maximumMarketCost"]==a.maximum_market_cost&&proof["maximumQuantity"].as_u64()==Some(a.maximum_quantity as u64),"PACKAGE_AUTH_PROOF_CAPS")?;
  need(permit.run_id==a.run_id&&permit.owner=="Codex"&&permit.intent_id==admission.package_id()&&permit.expires_at_ms>now&&permit.expires_at_ms<=admission.expires_at_ms()&&permit.maximum_quantity<=a.maximum_quantity,"PACKAGE_AUTH_PERMIT")?;
  let spec=multi_leg::request_spec(basket,scope,&permit)?;
  let records=e.package_records();let records=records.as_array().ok_or("PACKAGE_AUTH_NATIVE_RECORDS")?;let packages:Vec<_>=records.iter().filter(|r|r["packageId"]==admission.package_id()).collect();need(packages.len()==1,"PACKAGE_AUTH_NATIVE_RECORDS")?;let package=packages[0];
  need(package["phase"]=="attempted"&&package["createdAt"]==proof["atMs"]&&package["proofHash"]==admission.proof_hash()&&package["bankScope"]==reserve["bankScope"]&&package["economics"]==reserve["economics"]&&package["expiresAt"].as_u64()==Some(admission.expires_at_ms())&&package["requestHash"]==hash(&spec)?&&package["idempotencyKey"]==basket.idempotency_key,"PACKAGE_AUTH_NATIVE_ATTEMPT")?;
  need(reserve["bankScope"]["runId"]==a.run_id&&reserve["bankScope"]["bankManifestHash"]==*bank.manifest_hash()&&reserve["bankScope"]["tournamentId"]==scope.tournament_id,"PACKAGE_AUTH_RESERVE_SCOPE")?;
  let rows=package["legs"].as_array().ok_or("PACKAGE_AUTH_NATIVE_LEGS")?;let reserved=reserve["legs"].as_array().ok_or("PACKAGE_AUTH_NATIVE_LEGS")?;let candidate=proof["candidate"]["legs"].as_array().ok_or("PACKAGE_AUTH_PROOF_LEGS")?;
  need((2..=3).contains(&rows.len())&&rows.len()==basket.legs.len()&&rows.len()==reserved.len()&&rows.len()==candidate.len()&&facts.components.len()==rows.len(),"PACKAGE_AUTH_NATIVE_LEGS")?;
  let mut legs=vec![];let mut qids=BTreeSet::new();let mut exchanges=BTreeSet::new();let mut total=0i128;let mut quantity=0u64;
  let orders=e.order_records();
  for(i,b)in basket.legs.iter().enumerate(){let row=&rows[i];let r=&reserved[i];let c=&candidate[i];let ex=&b.exchange_id;let market=scope.exchanges.get(ex).ok_or("PACKAGE_AUTH_LEG_SCOPE")?;let side=if b.side==multi_leg::Side::Yes{"YES"}else{"NO"};let limit=decimal_units(&b.limit_price)?;let qid=row["qid"].as_u64().ok_or("PACKAGE_AUTH_QID")?;
   need(qids.insert(qid)&&exchanges.insert(ex.clone())&&e.is_package_order(qid),"PACKAGE_AUTH_DUPLICATE_LEG")?;
   for v in [row,r,c]{need(v["exchangeId"]==*ex&&v["marketId"]==*market&&v["side"]==side&&v["quantity"].as_u64()==Some(b.quantity as u64),"PACKAGE_AUTH_ORDERED_LEG")?;}
   need(row["limitPrice"]==b.limit_price&&r["limitPrice"]==b.limit_price&&row["bookHash"]==r["bookHash"]&&row["filled"].as_u64()==Some(0)&&row["actualOrderId"].is_null()&&row["terminalReconciled"]==false,"PACKAGE_AUTH_NATIVE_LEG_CHANGED")?;
   need(e.wallet.quantity(ex,moloch_replay::Side::Yes)==0&&e.wallet.quantity(ex,moloch_replay::Side::No)==0,"PACKAGE_AUTH_COMPONENT_NOT_FLAT")?;
   let q=orders.iter().find(|q|q["id"].as_u64()==Some(qid)).ok_or("PACKAGE_AUTH_ORDER_MISSING")?;
   need(q["exchangeId"]==*ex&&q["marketId"]==*market&&q["side"]==side&&q["action"]=="buy"&&q["quantity"].as_u64()==Some(b.quantity as u64)&&q["filled"].as_u64()==Some(0)&&q["price"]==b.limit_price&&q["retired"]==false&&q["cancelEffectiveAt"].is_null()&&q["activationAt"].as_u64().is_some_and(|t|t<=now)&&q["expiresAt"].as_u64()==Some(admission.expires_at_ms()),"PACKAGE_AUTH_NATIVE_ORDER_CHANGED")?;
   need(!orders.iter().any(|q|q["exchangeId"]==*ex&&q["retired"]==false&&!qids.contains(&q["id"].as_u64().unwrap_or(0))&&q["id"]!=row["qid"]),"PACKAGE_AUTH_COMPETING_ORDER")?;
   let cost=limit.checked_mul(b.quantity as i128).ok_or("PACKAGE_AUTH_RANGE")?;need(cost>0&&cost<=decimal_units(&a.maximum_market_cost)?,"PACKAGE_AUTH_MARKET_CAP")?;total=total.checked_add(cost).ok_or("PACKAGE_AUTH_RANGE")?;quantity=quantity.checked_add(b.quantity as u64).ok_or("PACKAGE_AUTH_RANGE")?;
   legs.push(Leg{exchange:ex.clone(),market:market.clone(),contract:c["contractId"].as_str().filter(|s|!s.is_empty()).ok_or("PACKAGE_AUTH_CONTRACT_ID")?.into(),side:b.side,quantity:b.quantity,limit});
  }
  need(quantity<=a.maximum_quantity as u64&&quantity<=permit.maximum_quantity as u64&&total==decimal_units(&basket.maximum_cost)?&&total==dec(&reserve["economics"]["maximumCost"])?&&total==dec(&proof["maximumCost"])?&&total<=decimal_units(&permit.maximum_cost)?,"PACKAGE_AUTH_COST_OR_QUANTITY")?;
  need(dec(&proof["minimumPayout"])?==dec(&reserve["economics"]["minimumPayout"])?&&dec(&proof["minimumPayout"])?-total>=dec(&proof["buffer"])?+dec(&proof["minimumSurplus"])?&&dec(&proof["minimumSurplus"])? >0&&dec(&proof["buffer"])? >=0&&dec(&reserve["economics"]["feeTotal"])?==0,"PACKAGE_AUTH_SURPLUS")?;
  let ledger=e.report()?;let reserve_total=dec(&ledger["reservedCash"])?;let owned=dec(&ledger["ownedCost"])?;let cash=dec(&ledger["cash"])?;
  need(reserve_total>=total&&reserve_total<=cash&&owned.checked_add(reserve_total).is_some_and(|n|n<=decimal_units(&a.maximum_committed).unwrap_or(-1)),"PACKAGE_AUTH_FULL_FUNDING")?;
  let seed=dec(&bank.live_account_seed().ok_or("PACKAGE_AUTH_SEED")?["availableCash"])?;
  let expected_cash=seed.checked_add(e.wallet.cash).and_then(|n|n.checked_sub(e.wallet.initial)).and_then(|n|n.checked_add(e.opening_inventory_cost())).ok_or("PACKAGE_AUTH_RANGE")?;
  // Admission owns the independently reviewed rule bindings. Fresh transport
  // timestamps may differ only for a V2 full-body certificate; V1 stays exact.
  let mut rule_sources=BTreeSet::new();
  for l in &legs{let f=facts.components.get(&l.exchange).ok_or("PACKAGE_AUTH_COMPONENT_FACTS")?;admission.validate_physical_rule(&l.exchange,&f.contract)?;rule_sources.insert(hash(&f.contract)?);}
  let prepared=Self{spec,permit,scope:scope.clone(),legs,facts,prepared:now,expires:admission.expires_at_ms(),entry_cutoff:effective_entry_cutoff,expected_cash,maximum_cost:total,reserved_total:reserve_total,rule_sources};
  // Owned IDs are supplied at physical authorization; validate all other facts
  // here while retaining immutable official snapshots for the queue boundary.
  prepared.validate_facts(now,None)?;Ok(prepared)
 }
 pub fn authorize(&self,spec:&RequestSpec,permit:&Permit,now:u64,owned_order_ids:&BTreeSet<String>,effective_entry_cutoff:u64)->Result<()>{
  need(same(spec,&self.spec)?&&same(permit,&self.permit)?,"PACKAGE_AUTH_REQUEST_CHANGED")?;
  need(now>=self.prepared&&now<self.expires&&now<self.permit.expires_at_ms&&effective_entry_cutoff==self.entry_cutoff&&now<effective_entry_cutoff,"PACKAGE_AUTH_EXPIRED_OR_WINDOW_CHANGED")?;
  self.validate_facts(now,Some(owned_order_ids))
 }
 fn validate_facts(&self,now:u64,owned:Option<&BTreeSet<String>>)->Result<()>{
  need(now>=self.prepared&&now<self.expires&&now<self.entry_cutoff,"PACKAGE_AUTH_EXPIRED_OR_WINDOW_CHANGED")?;
  let f=&self.facts;let s=&self.scope;for r in [&f.account,&f.cash_balance,&f.positions,&f.open_orders]{fresh(r,now,20000)?;}
  need(f.account.endpoint==format!("/tournaments/{}",s.tournament_slug)&&f.account.data["id"]==s.tournament_id&&f.account.data["slug"]==s.tournament_slug&&f.account.data["status"]=="active"&&clock(f.account.data["endDate"].as_str().ok_or("PACKAGE_AUTH_END_DATE")?)?>=self.entry_cutoff,"PACKAGE_AUTH_TOURNAMENT")?;
  need(f.cash_balance.endpoint==format!("/account?tournamentId={}",s.tournament_id),"PACKAGE_AUTH_CASH_SCOPE")?;let cash=relationships::parse_buying_power(&f.cash_balance.data,s)?.gross_cash_units.ok_or("PACKAGE_AUTH_CASH_REQUIRED")?;
  need(cash==self.expected_cash&&cash>=self.maximum_cost&&cash>=self.reserved_total,"PACKAGE_AUTH_CASH_RECONCILIATION")?;
  need(f.positions.endpoint==format!("/tournaments/{}/portfolio/positions",s.tournament_slug)&&f.positions.data.get("pagination").is_none(),"PACKAGE_AUTH_POSITIONS_SCOPE")?;
  let mut position_ids=BTreeSet::new();for p in f.positions.data["positions"].as_array().ok_or("PACKAGE_AUTH_POSITIONS")?{let ex=p["exchangeId"].as_str().ok_or("PACKAGE_AUTH_POSITION")?;need(position_ids.insert(ex),"PACKAGE_AUTH_POSITION_DUPLICATE")?;let n=p["quantity"].as_i64().ok_or("PACKAGE_AUTH_POSITION")?;if let Some(l)=self.legs.iter().find(|l|l.exchange==ex){need(p["marketId"]==l.market&&n==0,"PACKAGE_AUTH_FOREIGN_COMPONENT_POSITION")?;}}
  need(f.open_orders.endpoint==format!("/orders?status=open&limit=200&tournamentId={}",s.tournament_id)&&f.open_orders.data["pagination"]["hasMore"]==false&&f.open_orders.data["coverage"]["complete"]==true,"PACKAGE_AUTH_OPEN_ORDER_SCOPE")?;
  let mut order_ids=BTreeSet::new();for o in f.open_orders.data["data"].as_array().ok_or("PACKAGE_AUTH_OPEN_ORDERS")?{let order_id=id(&o["id"])?;need(order_ids.insert(order_id.clone())&&owned.is_none_or(|ids|ids.contains(&order_id)),"PACKAGE_AUTH_FOREIGN_OPEN_ORDER")?;need(o["tournamentId"]==s.tournament_id&&o["exchangeId"].as_str().is_some(),"PACKAGE_AUTH_OPEN_ORDER_SCOPE")?;need(!self.legs.iter().any(|l|o["exchangeId"]==l.exchange),"PACKAGE_AUTH_COMPONENT_OPEN_ORDER")?;}
  for l in &self.legs{let f=self.facts.components.get(&l.exchange).ok_or("PACKAGE_AUTH_COMPONENT_FACTS")?;for r in [&f.market,&f.contract,&f.book]{fresh(r,now,10000)?;}
   need(f.market.endpoint==format!("/markets/{}?tournamentId={}",l.market,s.tournament_id)&&f.market.data["id"]==l.market&&f.market.data["status"]=="open"&&f.market.data["isComposite"]==false&&f.market.data["isMultiOutcome"]==false&&context(&f.market.data,&s.tournament_id)?["status"]=="open","PACKAGE_AUTH_MARKET_INACTIVE")?;
   need(f.contract.endpoint==format!("/markets/{}/nodes?tournamentId={}",l.market,s.tournament_id)&&f.contract.data["market_id"]==l.market&&context(&f.contract.data,&s.tournament_id).is_ok()&&f.contract.data["root"]["node_type"]=="contract"&&id(&f.contract.data["root"]["id"])?==l.contract&&self.rule_sources.contains(&hash(&f.contract)?),"PACKAGE_AUTH_UNREVIEWED_CONTRACT")?;
   need([20,200].iter().any(|d|f.book.endpoint==format!("/markets/{}/orderbook?tournamentId={}&depth={d}",l.market,s.tournament_id)),"PACKAGE_AUTH_BOOK_ENDPOINT")?;
   let books=context(&f.book.data,&s.tournament_id)?["orderbook"]["exchanges"].as_array().ok_or("PACKAGE_AUTH_BOOK_SCOPE")?;
   let mut observed=BTreeSet::new();for b in books{let ex=b["exchangeId"].as_str().ok_or("PACKAGE_AUTH_BOOK_SCOPE")?;need(s.exchanges.get(ex)==Some(&l.market)&&observed.insert(ex),"PACKAGE_AUTH_BOOK_SCOPE")?;}
   let expected:BTreeSet<_>=s.exchanges.iter().filter(|(_,market)|**market==l.market).map(|(ex,_)|ex.as_str()).collect();need(observed==expected,"PACKAGE_AUTH_BOOK_SCOPE")?;
   let rows:Vec<_>=books.iter().filter(|b|b["exchangeId"]==l.exchange).collect();need(rows.len()==1,"PACKAGE_AUTH_BOOK_SCOPE")?;let b=rows[0];let at=clock(b["asOf"]["at"].as_str().ok_or("PACKAGE_AUTH_BOOK_CLOCK")?)?;
   need(b["asOf"]["sequence"].as_u64().is_some()&&at<=now&&now-at<=10000,"PACKAGE_AUTH_BOOK_STALE")?;
   let mut parsed=vec![];for(key,ascending)in [("bids",false),("asks",true)]{let mut levels=vec![];for level in b[key].as_array().filter(|a|!a.is_empty()&&a.len()<=200).ok_or("PACKAGE_AUTH_BOOK_DEPTH")?{let p=prices::official_tick_units(&level["price"])?;let n=decimal_units(&level["quantity"].to_string())?;need((5000..=995000).contains(&p)&&n>0&&levels.last().is_none_or(|(old,_)|if ascending{p>*old}else{p<*old}),"PACKAGE_AUTH_BOOK_LEVEL")?;levels.push((p,n));}parsed.push(levels);}
   need(parsed[0][0].0<parsed[1][0].0,"PACKAGE_AUTH_CROSSED_BOOK")?;
   let mut left=l.quantity as i128*1_000_000;for(p,n)in if l.side==multi_leg::Side::Yes{&parsed[1]}else{&parsed[0]}{let ask=if l.side==multi_leg::Side::Yes{*p}else{1_000_000-*p};if ask>l.limit{break};left=left.saturating_sub(*n).max(0);if left==0{break}}
   need(left==0,"PACKAGE_AUTH_INSUFFICIENT_FIXED_LIMIT_DEPTH")?;
  }Ok(())
 }
}
