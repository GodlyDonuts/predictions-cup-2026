//! Local retirement selection only. No receipt, account/queue call, financial
//! credit, physical cancellation, repricing, retry or execution authority.
use crate::{dispatch::DispatchLog,feedback::Result,quote_priority::{self,Class}};
use baal_transport_service::Scope;
use moloch_replay::{session::Session,Side,amount,fee};
use serde_json::Value;
use std::collections::{BTreeMap,BTreeSet};
pub const MINIMUM_DRAFT_AGE_MS:u64=60_000;
#[derive(Clone,Copy,PartialEq,Eq)]
enum SelectionKind { Stale, UnqualifiedFlat }
pub struct Draft {quote:Value,proof:Value,reserved:i128}
impl Draft {
 pub fn qid(&self)->u64{self.quote["id"].as_u64().unwrap()}
 pub fn quote(&self)->&Value{&self.quote}
 pub fn reserved_units(&self)->i128{self.reserved}
}
pub struct Selection {head:Value,dispatch:Value,scope:Scope,managed:BTreeSet<String>,at:u64,kind:SelectionKind,drafts:Vec<Draft>}
impl Selection {
 pub fn native_head(&self)->&Value{&self.head}
 pub fn dispatch_descriptor_identity(&self)->&Value{&self.dispatch}
 pub fn drafts(&self)->&[Draft]{&self.drafts}
 /// Validate the WHOLE unchanged selection before the sole worker begins its
 /// serialized no-HTTP retirement batch. Any intervening native/log event
 /// requires reselection. Each actual retirement still needs the dispatcher's
 /// original durable never-sent proof; this token cannot create that proof.
 pub fn validate_current(&self,bank:&Session,log:&DispatchLog,scope:&Scope,managed:&BTreeSet<String>,now:u64)->Result<()>{
  if now<self.at||bank.durable_head()!=self.head||log.descriptor_identity()?!=self.dispatch||scope.tournament_id!=self.scope.tournament_id||scope.tournament_slug!=self.scope.tournament_slug||scope.exchanges!=self.scope.exchanges||managed!=&self.managed{return Err("STALE_DRAFT_SELECTION_CHANGED".into())}
  let current=select_for(bank,log,scope,managed,now,self.kind)?;
  for d in &self.drafts{if !current.drafts.iter().any(|c|c.quote==d.quote&&c.proof==d.proof&&c.reserved==d.reserved){return Err("STALE_DRAFT_SELECTION_CHANGED".into())}}
  Ok(())
 }
}
pub fn select(bank:&Session,log:&DispatchLog,scope:&Scope,managed:&BTreeSet<String>,now:u64)->Result<Selection>{
 select_for(bank,log,scope,managed,now,SelectionKind::Stale)
}
/// No maker capability exists for these flat ordinary drafts. Freshness and
/// price are irrelevant to the restriction; every original no-send, ownership,
/// package and current-head proof remains mandatory. This grants no spending.
pub fn select_unqualified_new_entries(bank:&Session,log:&DispatchLog,scope:&Scope,managed:&BTreeSet<String>,now:u64)->Result<Selection>{
 select_for(bank,log,scope,managed,now,SelectionKind::UnqualifiedFlat)
}
fn select_for(bank:&Session,log:&DispatchLog,scope:&Scope,managed:&BTreeSet<String>,now:u64,kind:SelectionKind)->Result<Selection>{
 if !bank.engine().is_live()||bank.live_account_seed().is_none()||scope.tournament_id.is_empty()||scope.exchanges.len()>4096||managed.iter().any(|e|!scope.exchanges.contains_key(e)){return Err("STALE_DRAFT_SCOPE_OR_BANK".into())}
 log.require_resolved_attempts()?;
 let orders=bank.engine().order_records();let byid:BTreeMap<_,_>=orders.iter().map(|q|q["id"].as_u64().map(|id|(id,q)).ok_or("STALE_DRAFT_NATIVE_ID")).collect::<std::result::Result<_,_>>()?;
 let mut busy=BTreeSet::new();
 // An unsent mate can be intended cover for a physical order that may fill
 // later. Closed original ownership, never an open-list omission, is required.
 for id in log.attempted.iter().filter(|id|!log.closed.contains(id)&&!log.proved_no_network(**id)).chain(log.acknowledgments.keys().filter(|id|!log.closed.contains(id))).chain(bank.engine().live_order_bindings().keys().filter(|id|!log.closed.contains(id))){
  let q=byid.get(id).ok_or("STALE_DRAFT_ORIGINAL_ORDER_MISSING")?;busy.insert(q["exchangeId"].as_str().ok_or("STALE_DRAFT_SCOPE")?.to_owned());
 }
 if kind==SelectionKind::UnqualifiedFlat{for id in log.intents.keys().filter(|id|!log.closed.contains(id)&&!log.proved_no_network(**id)){
  let q=byid.get(id).ok_or("STALE_DRAFT_ORIGINAL_ORDER_MISSING")?;busy.insert(q["exchangeId"].as_str().ok_or("STALE_DRAFT_SCOPE")?.to_owned());
 }}
 let actual_fills:BTreeSet<_>=bank.engine().fills().iter().map(|f|f["orderId"].as_u64().ok_or("STALE_DRAFT_ACTUAL_FILL_ID")).collect::<std::result::Result<_,_>>()?;
 let mut protected=BTreeSet::new();
 for p in bank.engine().package_records().as_array().ok_or("STALE_DRAFT_PACKAGES")?{
  if p["phase"]=="no_execution"||p["protectionReleased"]==true{continue}
  for leg in p["legs"].as_array().ok_or("STALE_DRAFT_PACKAGE_LEGS")?{protected.insert(leg["exchangeId"].as_str().ok_or("STALE_DRAFT_PACKAGE_SCOPE")?.to_owned());}
  if let Some(xs)=p["portfolio"]["componentExchanges"].as_array(){for ex in xs{protected.insert(ex.as_str().ok_or("STALE_DRAFT_PACKAGE_SCOPE")?.to_owned());}}
 }
 let mut candidates=Vec::new();
 for q in orders{
  if kind==SelectionKind::UnqualifiedFlat&&bank.engine().policy()["quoteMode"]!="pairedSpread"{continue}
  if q["action"]!="buy"||q["retired"]!=false||q["filled"]!=0||q.get("reviewedFundamentalEntry").is_some()||q.get("reviewedInventoryExit").is_some()||q.get("cancelEffectiveAt").is_some_and(|v|!v.is_null()){continue}
  let id=q["id"].as_u64().ok_or("STALE_DRAFT_NATIVE_ID")?;let ex=q["exchangeId"].as_str().ok_or("STALE_DRAFT_SCOPE")?;
  if scope.exchanges.get(ex).is_none_or(|m|q["marketId"]!=*m){return Err("STALE_DRAFT_SCOPE".into())}
  if managed.contains(ex)||protected.contains(ex)||busy.contains(ex)||bank.engine().is_package_order(id)||!log.never_attempted(id)||log.intents.contains_key(&id)||log.attempted.contains(&id)||log.acknowledgments.contains_key(&id)||bank.engine().live_order_bindings().contains_key(&id)||actual_fills.contains(&id){continue}
  if bank.engine().wallet.quantity(ex,Side::Yes)!=0||bank.engine().wallet.quantity(ex,Side::No)!=0{continue}
  let created=q["createdAt"].as_u64().ok_or("STALE_DRAFT_CLOCK")?;let expires=q["expiresAt"].as_u64().ok_or("STALE_DRAFT_CLOCK")?;
  if created>now{return Err("STALE_DRAFT_FUTURE_CLOCK".into())}
  if expires<=now||(kind==SelectionKind::Stale&&now-created<MINIMUM_DRAFT_AGE_MS){continue}
  candidates.push(q);
 }
 let selected=if kind==SelectionKind::Stale{
  quote_priority::from_native(bank,scope,&candidates,now,None)?.assessments.into_iter().filter(|a|a.decision.class==Class::NeedsRefresh&&a.decision.reason=="current_book_unavailable").map(|a|a.index).collect::<Vec<_>>()
 }else{(0..candidates.len()).collect()};let mut drafts=Vec::new();
 for index in selected{
  let q=candidates[index].clone();let id=q["id"].as_u64().unwrap();let n=q["quantity"].as_u64().filter(|n|*n>0).ok_or("STALE_DRAFT_QUANTITY")?;let price=amount(q["price"].as_str().ok_or("STALE_DRAFT_PRICE")?)?;
  let unit=price.checked_add(fee(price,1,bank.engine().wallet.fee_bps,bank.engine().wallet.fee_per_share)?).ok_or("STALE_DRAFT_RANGE")?;
  drafts.push(Draft{quote:q,proof:log.proof(id),reserved:unit.checked_mul(n as i128).ok_or("STALE_DRAFT_RANGE")?});
 }
 // Capital cleanup priority, not market/EV ranking or new sizing authority.
 drafts.sort_by(|a,b|b.reserved.cmp(&a.reserved).then_with(||a.qid().cmp(&b.qid())));
 Ok(Selection{head:bank.durable_head(),dispatch:log.descriptor_identity()?,scope:scope.clone(),managed:managed.clone(),at:now,kind,drafts})
}
