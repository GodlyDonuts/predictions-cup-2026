//! A short-lived NEGATIVE price screen, never a book, depth or EV estimate.
//! Primary schema: official OpenAPI BulkExchangePrice and /exchanges/prices.
//! Passing/unknown quotes still require every original physical preflight fact.
//! There is no exchange source/asOf clock in this schema: request/receipt age
//! bounds use of this observation, not a claim that the market cannot change.
use crate::feedback::{self,Result};
use baal_transport_service::{Endpoint,Receipt,Scope,prices::official_tick_units};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet};
pub const MAXIMUM_IDS:usize=100;
pub const MAXIMUM_RECEIPT_AGE_MS:u64=1500;
const UNIT:i128=1_000_000;
const TICK:i128=5000;
fn need(ok:bool,why:&'static str)->Result<()>{if ok{Ok(())}else{Err(why.into())}}
fn hash(v:&impl Serialize)->Result<String>{Ok(format!("{:x}",Sha256::digest(serde_json::to_vec(v)?)))}
fn clock(s:&str)->Result<u64>{Ok(u64::try_from(chrono::DateTime::parse_from_rfc3339(s)?.timestamp_millis())?)}
fn id(s:&str)->bool{!s.is_empty()&&!s.starts_with('0')&&s.bytes().all(|b|b.is_ascii_digit())&&s.parse::<u64>().is_ok_and(|n|n<=9_007_199_254_740_991)}
fn requested(scope:&Scope,ids:&[String])->Result<()>{
 need(baal_transport_service::relationships::uuid(&scope.tournament_id),"BULK_SCREEN_TOURNAMENT")?;
 need(!ids.is_empty()&&ids.len()<=MAXIMUM_IDS,"BULK_SCREEN_REQUEST_BOUND")?;let mut seen=BTreeSet::new();
 for ex in ids{need(id(ex)&&scope.exchanges.get(ex).is_some_and(|m|id(m))&&seen.insert(ex),"BULK_SCREEN_REQUEST_SCOPE")?;}Ok(())
}
/// Construct the EXISTING typed endpoint; the caller uses its sole Transport.
pub fn endpoint(scope:&Scope,ids:&[String])->Result<Endpoint>{requested(scope,ids)?;Ok(Endpoint::Prices{exchange_ids:ids.to_vec()})}
#[derive(Clone,Copy)]struct Quote{bid:i128,ask:i128}
/// Only genuine parsed original receipt data creates this cached observation.
/// No Deserialize, file/network work or internally renewed freshness clock.
pub struct Batch{scope_sha:String,receipt_sha:String,started:u64,received:u64,until:u64,quotes:BTreeMap<String,Option<Quote>>}
#[derive(Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct Deferral{reason:&'static str,distance_ticks:u64,quote_sha256:String,receipt_sha256:String,request_started_at_ms:u64,received_at_ms:u64,valid_until_ms:u64}
impl Deferral{
 pub fn reason(&self)->&'static str{self.reason}
 pub fn valid_until_ms(&self)->u64{self.valid_until_ms}
 /// Recheck immediately before honoring a cached veto; never carry it across a
 /// changed quote/price/quantity/TTL or past the ORIGINAL snapshot's lifetime.
 pub fn matches_quote(&self,q:&Value,now:u64)->Result<bool>{Ok(now>=self.received_at_ms&&now<self.valid_until_ms&&hash(q)?==self.quote_sha256)}
}
impl Batch{
 pub fn from_original(scope:&Scope,ids:&[String],r:&Receipt,now:u64)->Result<Self>{
  requested(scope,ids)?;need(r.status==200,"BULK_SCREEN_STATUS")?;
  need(r.endpoint==format!("/exchanges/prices?ids={}&tournamentId={}",ids.join(","),scope.tournament_id),"BULK_SCREEN_ENDPOINT")?;
  let started=clock(&r.request_started_at)?;let received=clock(&r.received_at)?;need(started<=received&&received<=now,"BULK_SCREEN_CLOCK")?;
  let until=started.checked_add(MAXIMUM_RECEIPT_AGE_MS).ok_or("BULK_SCREEN_CLOCK")?;
  let rows=r.data["data"].as_array().ok_or("BULK_SCREEN_ROWS")?;let missing=r.data["missingIds"].as_array().ok_or("BULK_SCREEN_MISSING")?;
  need(rows.len()<=ids.len()&&missing.len()<=ids.len(),"BULK_SCREEN_ROWS_BOUND")?;
  let mut quotes=BTreeMap::new();let mut last=None;
  for row in rows{
   let ex=row["exchangeId"].as_str().ok_or("BULK_SCREEN_ROW_SCOPE")?;let index=ids.iter().position(|x|x==ex).ok_or("BULK_SCREEN_ROW_SCOPE")?;
   need(last.is_none_or(|old|index>old)&&scope.exchanges.get(ex).is_some_and(|m|row["marketId"]==*m)&&!quotes.contains_key(ex),"BULK_SCREEN_ROW_SCOPE")?;last=Some(index);
   for field in ["latestPrice","bestBid","bestAsk","spread"]{let v=row.get(field).ok_or("BULK_SCREEN_ROW_FIELDS")?;need(v.is_null()||v.is_number(),"BULK_SCREEN_PRICE_TYPE")?;
    if !v.is_null(){let n=if ["bestBid","bestAsk"].contains(&field){official_tick_units(v)?}else{baal_transport_service::decimal_units(&v.to_string())?};need((0..=UNIT).contains(&n),"BULK_SCREEN_PRICE_RANGE")?;}}
   let option=row.get("option").ok_or("BULK_SCREEN_ROW_FIELDS")?;need(option.is_null()||option.is_string(),"BULK_SCREEN_OPTION")?;
   // Canonical YES interpretation only. Null/other option labels are unknown,
   // never an implicit permission to complement an already-NO quote twice.
   let quote=if option=="YES"&&!row["bestBid"].is_null()&&!row["bestAsk"].is_null(){
    let bid=official_tick_units(&row["bestBid"])?;let ask=official_tick_units(&row["bestAsk"])?;
    if bid<ask&&bid>0&&ask<UNIT{Some(Quote{bid,ask})}else{None}
   }else{None};quotes.insert(ex.to_owned(),quote);
  }
  for raw in missing{let ex=raw.as_str().ok_or("BULK_SCREEN_MISSING")?;need(ids.iter().any(|x|x==ex)&&quotes.insert(ex.into(),None).is_none(),"BULK_SCREEN_DUPLICATE_OR_FOREIGN")?;}
  need(quotes.len()==ids.len(),"BULK_SCREEN_INCOMPLETE")?;
  Ok(Self{scope_sha:hash(scope)?,receipt_sha:feedback::digest(r)?,started,received,until,quotes})
 }
 pub fn valid_until_ms(&self)->u64{self.until}
 pub fn receipt_sha256(&self)->&str{&self.receipt_sha}
 /// Some means only "defer this exact ordinary quote for this short receipt
 /// lifetime". None means NEED A BOOK, not competitive/executable/positive EV.
 pub fn defer(&self,scope:&Scope,q:&Value,now:u64)->Result<Option<Deferral>>{
  need(hash(scope)?==self.scope_sha,"BULK_SCREEN_SCOPE_CHANGED")?;
  if now<self.received||now>=self.until{return Ok(None)}
  let ex=q["exchangeId"].as_str().ok_or("BULK_SCREEN_QUOTE_SCOPE")?;need(scope.exchanges.get(ex).is_some_and(|m|q["marketId"]==*m),"BULK_SCREEN_QUOTE_SCOPE")?;
  let action=q["action"].as_str().filter(|s|["buy","sell"].contains(s)).ok_or("BULK_SCREEN_QUOTE_ACTION")?;
  let side=q["side"].as_str().filter(|s|["YES","NO"].contains(s)).ok_or("BULK_SCREEN_QUOTE_SIDE")?;
  need(q["retired"].is_boolean()&&q["quantity"].as_u64().is_some_and(|n|n>0)&&q["filled"].as_u64().is_some_and(|n|q["quantity"].as_u64().is_some_and(|total|n<=total)),"BULK_SCREEN_QUOTE_SHAPE")?;
  if q["retired"]==true||q.get("cancelEffectiveAt").is_some_and(|v|!v.is_null())||q["expiresAt"].as_u64().is_none_or(|t|t<=now){return Ok(None)}
  // Risk-reducing exits retain their existing immediate fresh-book route.
  if action=="sell"&&(q["inventoryExitReason"]=="maximumAge"||q["executionStyle"]=="inventoryAON"||q.get("reviewedInventoryExit").is_some_and(|v|!v.is_null())){return Ok(None)}
  if q.get("reviewedFundamentalEntry").is_some_and(|v|!v.is_null()){return Ok(None)}
  let Some(Some(b))=self.quotes.get(ex)else{return Ok(None)};
  let limit=official_tick_units(q.get("price").ok_or("BULK_SCREEN_QUOTE_PRICE")?)?;need(limit>0&&limit<UNIT,"BULK_SCREEN_QUOTE_PRICE")?;
  let(bid,ask)=if side=="YES"{(b.bid,b.ask)}else{(UNIT-b.ask,UNIT-b.bid)};
  let(reason,distance)=if action=="sell"&&limit>ask{("sell_floor_above_current_ask",limit-ask)}
   else if action=="buy"&&limit<bid{("buy_behind_current_best_bid",bid-limit)}
   else if action=="buy"&&limit>=ask{("new_buy_would_cross_current_ask",limit-ask)}else{return Ok(None)};
  Ok(Some(Deferral{reason,distance_ticks:u64::try_from((distance+TICK-1)/TICK)?,quote_sha256:hash(q)?,receipt_sha256:self.receipt_sha.clone(),request_started_at_ms:self.started,received_at_ms:self.received,valid_until_ms:self.until}))
 }
}
