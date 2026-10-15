//! Read-plan execution through the existing serialized worker. Caches are only
//! hints, start empty on restart, and never substitute for complete fill tapes.
use crate::{feedback::{self,Result},reconciliation_plan::{self as planner,FillWatermark,OrderDecision,OwnedOrder,ProjectionWatermark}};
use baal_transport_service::{Endpoint,Receipt,Scope};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

pub trait Backend {
 fn now_ms(&self)->u64;
 /// Production supplies its existing request helper, including durable receipt
 /// storage, shared account admission/backoff and bounded read retry.
 fn read(&mut self,endpoint:Endpoint)->Result<Receipt>;
 fn quote(&self,logical_id:u64)->Result<Value>;
 /// Must return only after the native event is durably committed.
 fn apply(&mut self,event:&Value)->Result<()>;
 /// Must append the native terminal and durable dispatch `closed` proof.
 fn close(&mut self,logical_id:u64,event:&Value,order:&Receipt,fills:&Receipt)->Result<()>;
}
#[derive(Default)]
pub struct Reconciler {
 fills:BTreeMap<String,FillWatermark>,projection:Option<ProjectionWatermark>,
}
#[derive(Debug,Default,Serialize)]
#[serde(rename_all="camelCase")]
pub struct Cycle {
 pub pending_orders:usize,pub aggregate_reads:usize,pub exact_order_reads:usize,
 pub complete_fill_reads:usize,pub validated_member_observations:usize,
 pub hint_fallbacks:usize,pub watermark_invalidations:usize,
 pub blocks_new_submissions:bool,
}
fn iso(ms:u64)->Result<String>{Ok(chrono::DateTime::from_timestamp_millis(i64::try_from(ms)?).ok_or("LIVE_RECONCILIATION_CLOCK")?.to_rfc3339_opts(chrono::SecondsFormat::Millis,true))}

/// Quote quantity is the admitted native size AFTER any actual ACK resize, not
/// the current open-list remainder. A fully filled/retired quote stays pending
/// until its dispatch log has independent terminal proof.
pub fn owned_from_quote(q:&Value,actual:&str,cancel_pending:bool,uncertain:bool)->Result<OwnedOrder>{
 Ok(OwnedOrder{logical_id:q["id"].as_u64().ok_or("LIVE_QUOTE_ID")?,actual_order_id:actual.into(),exchange_id:q["exchangeId"].as_str().ok_or("LIVE_QUOTE_SCOPE")?.into(),market_id:q["marketId"].as_str().ok_or("LIVE_QUOTE_SCOPE")?.into(),side:match q["side"].as_str(){Some("YES")=>"yes",Some("NO")=>"no",_=>return Err("LIVE_QUOTE_SIDE".into())}.into(),action:q["action"].as_str().ok_or("LIVE_QUOTE_ACTION")?.into(),limit_price:q["price"].as_str().ok_or("LIVE_QUOTE_PRICE")?.into(),admitted_quantity:q["quantity"].as_u64().ok_or("LIVE_QUOTE_QUANTITY")?,credited_filled_quantity:q["filled"].as_u64().ok_or("LIVE_QUOTE_FILLED")?,created_at_ms:q["createdAt"].as_u64().ok_or("LIVE_QUOTE_CREATED")?,expires_at_ms:q["expiresAt"].as_u64().ok_or("LIVE_QUOTE_EXPIRY")?,cancel_pending:cancel_pending||q.get("cancelEffectiveAt").is_some(),acceptance_uncertain:uncertain})
}
impl Reconciler {
 pub fn retained_fill_watermarks(&self)->usize{self.fills.len()}
 pub fn cycle<B:Backend>(&mut self,scope:&Scope,pending:&[OwnedOrder],unresolved_acceptance:bool,b:&mut B)->Result<Cycle>{
  let mut stats=Cycle{pending_orders:pending.len(),blocks_new_submissions:unresolved_acceptance,..Default::default()};
  // Prune only this in-memory cache; never modify account/native history.
  self.fills.retain(|id,_|pending.iter().any(|q|&q.actual_order_id==id));
  if pending.is_empty(){return Ok(stats)}
  let aggregate=if pending.len()>1{stats.aggregate_reads+=1;Some(b.read(Endpoint::Orders{status:"open".into()})?)}else{None};
  let watermarks:Vec<_>=self.fills.values().cloned().collect();
  let plan=planner::plan(scope,pending,&watermarks,self.projection.as_ref(),aggregate.as_ref(),unresolved_acceptance,b.now_ms(),planner::Limits::default())?;
  stats.blocks_new_submissions|=plan.blocks_new_submissions;
  if let Some(point)=plan.observed_projection{self.projection=Some(point);}
  for (decision,owned) in plan.decisions.into_iter().zip(pending){
   let q=b.quote(owned.logical_id)?;
   let mut member_observation=None;
   if let OrderDecision::OpenMemberNeedsFillTape(member)=decision{
    if planner::member_is_fresh(&member,b.now_ms()){
     stats.complete_fill_reads+=1;
     let fills=b.read(Endpoint::OwnedFills{order_id:owned.actual_order_id.clone()})?;
     // Admission waiting may invalidate a previously fresh aggregate. Validate
     // again against the ACTUAL full-tape request-start time before any event.
     if planner::validate_member_fills(&member,&fills,b.now_ms()).is_ok(){
      if let Ok(events)=feedback::fills(&q,&owned.actual_order_id,&scope.tournament_id,&fills,&iso(b.now_ms())?){
       member_observation=Some((fills,events));
      }
     }
    }
    if member_observation.is_none(){stats.hint_fallbacks+=1;}
   }
   let (fills,events,terminal)=if let Some((fills,events))=member_observation{
    stats.validated_member_observations+=1;(fills,events,None)
   }else{
    stats.exact_order_reads+=1;
    let order=b.read(Endpoint::OwnedOrder{order_id:owned.actual_order_id.clone()})?;
    stats.complete_fill_reads+=1;
    let fills=b.read(Endpoint::OwnedFills{order_id:owned.actual_order_id.clone()})?;
    let at=iso(b.now_ms())?;
    let events=feedback::fills(&q,&owned.actual_order_id,&scope.tournament_id,&fills,&at)?;
    // Validate terminal/fill temporal order before applying ANY tape events.
    let terminal=feedback::terminal(&q,&owned.actual_order_id,&scope.tournament_id,&order,&fills,&at)?.map(|e|(e,order));
    (fills,events,terminal)
   };
   for event in events{b.apply(&event)?;}
   // A cache failure cannot discard valid, durably applied fills or stop later
   // owned-order reconciliation. The next cycle simply uses exact reads.
   let current=b.quote(owned.logical_id)?;
   let fresh_owned=owned_from_quote(&current,&owned.actual_order_id,owned.cancel_pending,owned.acceptance_uncertain)?;
   match planner::fill_watermark(scope,&fresh_owned,&fills,b.now_ms()){
    Ok(mark)=>{self.fills.insert(owned.actual_order_id.clone(),mark);},
    Err(_)=>{self.fills.remove(&owned.actual_order_id);stats.watermark_invalidations+=1;},
   }
   if let Some((event,order))=terminal{
    b.close(owned.logical_id,&event,&order,&fills)?;self.fills.remove(&owned.actual_order_id);
   }
  }
  Ok(stats)
 }
}
