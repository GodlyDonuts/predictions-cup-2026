//! Selection only: never changes a frozen quote, submits/cancels, releases a
//! reservation or certifies EV. Original risk, ownership, freshness, quantity,
//! activation and physical authority checks remain mandatory after selection.
use crate::{feedback::Result,package_admission};
use baal_transport_service::Scope;
use moloch_replay::{session::Session,amount,TICK,SCALE};
use moloch_relationship_arbitrage::Book;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
#[derive(Clone,Copy,Debug,PartialEq,Eq,Serialize)]
#[serde(rename_all="snake_case")]
pub enum Class{RiskExit,CoveredMarketable,Competitive,NeedsRefresh,Deferred,Inactive}
#[derive(Clone,Debug,Serialize)]
pub struct Decision{pub class:Class,pub reason:&'static str,pub distance_ticks:u64,pub executable_quantity:u64,pub book_received_ms:Option<u64>,pub book_observed_ms:Option<u64>}
impl Decision{fn empty(class:Class,reason:&'static str)->Self{Self{class,reason,distance_ticks:0,executable_quantity:0,book_received_ms:None,book_observed_ms:None}}}
fn need(ok:bool,why:&'static str)->Result<()>{if ok{Ok(())}else{Err(why.into())}}
fn bucket(c:Class)->u8{match c{Class::RiskExit=>0,Class::CoveredMarketable=>1,Class::Competitive=>2,Class::NeedsRefresh=>3,Class::Deferred=>4,Class::Inactive=>5}}
/// `book` must be the current scoped native source. The typed native_books
/// adapter interprets WS freshness from source/receipt clocks and REST freshness
/// from source/request/receipt clocks. A missing/stale book is never far-price
/// evidence: retain a refresh candidate instead of permanently suppressing it.
pub fn assess(q:&Value,book:Option<&Book>,now:u64,maximum_book_age_ms:u64)->Result<Decision>{
 need(maximum_book_age_ms>0,"QUOTE_PRIORITY_AGE")?;
 let action=q["action"].as_str().filter(|v|["buy","sell"].contains(v)).ok_or("QUOTE_PRIORITY_ACTION")?;let side=q["side"].as_str().filter(|v|["YES","NO"].contains(v)).ok_or("QUOTE_PRIORITY_SIDE")?;
 let retired=q["retired"].as_bool().ok_or("QUOTE_PRIORITY_STATE")?;let cancel=q.get("cancelEffectiveAt").filter(|v|!v.is_null());if let Some(c)=cancel{need(c.as_u64().is_some(),"QUOTE_PRIORITY_CANCEL")?;}
 let expires=q["expiresAt"].as_u64().ok_or("QUOTE_PRIORITY_CLOCK")?;
 if retired||cancel.is_some()||expires<=now{return Ok(Decision::empty(Class::Inactive,"retired_canceling_or_expired"))}
 let limit=amount(q["price"].as_str().ok_or("QUOTE_PRIORITY_PRICE")?)?;need(limit>0&&limit<SCALE&&limit%TICK==0,"QUOTE_PRIORITY_PRICE")?;
 let quantity=q["quantity"].as_u64().filter(|n|*n>0).ok_or("QUOTE_PRIORITY_QUANTITY")?;let filled=q["filled"].as_u64().ok_or("QUOTE_PRIORITY_QUANTITY")?;let remaining=quantity.checked_sub(filled).ok_or("QUOTE_PRIORITY_QUANTITY")?;
 if remaining==0{return Ok(Decision::empty(Class::Inactive,"no_remaining_quantity"))}
 // Execution style/visible bid depth does not itself establish a deadline.
 let urgent=action=="sell"&&q["inventoryExitReason"]=="maximumAge";
 let Some(b)=book else{return Ok(Decision::empty(if urgent{Class::RiskExit}else{Class::NeedsRefresh},if urgent{"risk_exit_requires_fresh_preflight"}else{"current_book_unavailable"}))};
 need(q["exchangeId"]==b.exchange_id,"QUOTE_PRIORITY_BOOK_SCOPE")?;
 let fresh=b.source.received_ms<=now&&b.observed_ms<=now&&now-b.source.received_ms<=maximum_book_age_ms&&now-b.observed_ms<=maximum_book_age_ms;
 if !fresh{return Ok(Decision::empty(if urgent{Class::RiskExit}else{Class::NeedsRefresh},if urgent{"risk_exit_requires_fresh_preflight"}else{"current_book_stale_or_future"}))}
 let(bids,asks)=if side=="YES"{(&b.yes_bids,&b.yes_asks)}else{(&b.no_bids,&b.no_asks)};
 let(Some(bid),Some(ask))=(bids.first(),asks.first())else{return Ok(Decision::empty(if urgent{Class::RiskExit}else{Class::NeedsRefresh},"current_book_incomplete"))};
 need(bid.price<ask.price,"QUOTE_PRIORITY_CROSSED_BOOK")?;
 let opposite=if action=="sell"{bids}else{asks};let mut executable=0u64;for l in opposite{if if action=="sell"{l.price>=limit}else{l.price<=limit}{executable=executable.checked_add(l.quantity).ok_or("QUOTE_PRIORITY_DEPTH_RANGE")?;}}
 let mut d=Decision{class:Class::Competitive,reason:"competitive_passive_quote",distance_ticks:0,executable_quantity:executable.min(remaining),book_received_ms:Some(b.source.received_ms),book_observed_ms:Some(b.observed_ms)};
 if urgent{d.class=Class::RiskExit;d.reason="owned_risk_exit_unchanged";return Ok(d)}
 if action=="sell"{
  if limit>ask.price{d.class=Class::Deferred;d.reason="sell_floor_above_current_ask";d.distance_ticks=u64::try_from((limit-ask.price+TICK-1)/TICK)?;}
  else if executable>0{d.class=Class::CoveredMarketable;d.reason="covered_sell_has_current_bid_depth";}
 }else if limit>=ask.price{d.class=Class::Deferred;d.reason="new_buy_would_cross_current_ask";}
 else if limit<bid.price{d.class=Class::Deferred;d.reason="buy_behind_current_best_bid";d.distance_ticks=u64::try_from((bid.price-limit+TICK-1)/TICK)?;}
 Ok(d)
}
#[derive(Clone,Debug,Serialize)]pub struct Ranked{pub index:usize,pub key:String,pub decision:Decision}
#[derive(Debug,Serialize)]pub struct Selection{pub ordered_indices:Vec<usize>,pub assessments:Vec<Ranked>}
/// Stable fair rotation within each priority class. A deferred floor is kept
/// visible in assessments and omitted from preflight selection until a later
/// fresh source makes it competitive. Cursor uses the worker's existing key.
pub fn rotate(quotes:&[Value],books:&BTreeMap<String,Book>,now:u64,age:u64,cursor:Option<&str>)->Result<Selection>{
 let mut assessments=Vec::with_capacity(quotes.len());for(index,q)in quotes.iter().enumerate(){
  let ex=q["exchangeId"].as_str().ok_or("QUOTE_PRIORITY_SCOPE")?;let key=format!("{}:{}:{}",ex,q["side"].as_str().ok_or("QUOTE_PRIORITY_SIDE")?,q["action"].as_str().ok_or("QUOTE_PRIORITY_ACTION")?);
  assessments.push(Ranked{index,key,decision:assess(q,books.get(ex),now,age)?});
 }
 let mut order:Vec<_>=assessments.iter().filter(|a|!matches!(a.decision.class,Class::Deferred|Class::Inactive)).collect();
 order.sort_by(|a,b|bucket(a.decision.class).cmp(&bucket(b.decision.class)).then_with(||{
  let group=|key:&str|u8::from(cursor.is_some_and(|c|key<=c));group(&a.key).cmp(&group(&b.key)).then_with(||a.key.cmp(&b.key)).then_with(||a.index.cmp(&b.index))
 }));
 Ok(Selection{ordered_indices:order.iter().map(|a|a.index).collect(),assessments})
}
/// Production entry: only actual valid books already admitted by this bank.
/// This function creates no network request and grants no dispatch permission.
pub fn from_native(bank:&Session,scope:&Scope,quotes:&[Value],now:u64,cursor:Option<&str>)->Result<Selection>{
 let age=bank.engine().policy()["bookAgeMs"].as_u64().filter(|n|*n>0).ok_or("QUOTE_PRIORITY_NATIVE_POLICY")?;
 for q in quotes{let ex=q["exchangeId"].as_str().ok_or("QUOTE_PRIORITY_SCOPE")?;need(scope.exchanges.get(ex).is_some_and(|market|q["marketId"]==*market),"QUOTE_PRIORITY_SCOPE")?;}
 let(books,_)=package_admission::native_books(bank,scope,now,age)?;rotate(quotes,&books,now,age,cursor)
}
