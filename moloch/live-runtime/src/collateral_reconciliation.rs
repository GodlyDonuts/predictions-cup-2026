//! Diagnostic consistency checks for ALL cash/debt, never ledger authority.
//! Primary saved official OpenAPI SHA256:
//! 6b5ff9a661c06f1a2d1dda131bf870a70f4f79699ac9937375a278861e1f81bf
//! Pointers: /components/schemas/AllExecutionEconomics/properties;
//! /paths/~1account/get/description; /paths/~1portfolio~1collateral/get.
//! Sequential account/collateral reads are NOT an atomic account-wide barrier.
//! ACK ALL describes cash economics but ACKs never establish actual fills.
use baal_transport_service::{Scope,Receipt,decimal_units,relationships,multi_leg::{self,Basket}};
use crate::feedback;
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::collections::BTreeMap;
type Checked<T> = std::result::Result<T,&'static str>;
pub enum Assessment<T>{Available(T),Unavailable(&'static str)}
impl<T> Assessment<T>{pub fn reason(&self)->Option<&'static str>{match self{Self::Available(_)=>None,Self::Unavailable(why)=>Some(why)}}}
fn assess<T>(r:Checked<T>)->Assessment<T>{match r{Ok(v)=>Assessment::Available(v),Err(e)=>Assessment::Unavailable(e)}}
fn need(ok:bool,why:&'static str)->Checked<()>{if ok{Ok(())}else{Err(why)}}
fn hash<T:serde::Serialize>(v:&T)->Checked<String>{Ok(format!("{:x}",Sha256::digest(serde_json::to_vec(v).map_err(|_|"JSON_SERIALIZATION")?)))}
fn clock(s:&str)->Checked<i64>{Ok(chrono::DateTime::parse_from_rfc3339(s).map_err(|_|"SOURCE_CLOCK")?.timestamp_millis())}
fn hex(s:&str)->bool{s.len()==64&&s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))}
fn add(a:i128,b:i128)->Checked<i128>{a.checked_add(b).ok_or("ECONOMIC_OVERFLOW")}
fn sub(a:i128,b:i128)->Checked<i128>{a.checked_sub(b).ok_or("ECONOMIC_OVERFLOW")}
fn receipt(r:&Receipt,endpoint:&str)->Checked<(i64,i64)>{need(r.status==200&&r.endpoint==endpoint,"RECEIPT_SCOPE_OR_STATUS")?;let start=clock(&r.request_started_at)?;let end=clock(&r.received_at)?;need(start>=0&&start<=end,"RECEIPT_CLOCK")?;Ok((start,end))}
/// Source references identify retained evidence, not its authenticity. Construct
/// them only from the sole worker's original verified policy/bank receipts.
pub struct MoneyEvidence{units:i128,source_sha256:String,started:i64,received:i64}
impl MoneyEvidence{pub fn new(units:i128,source_sha256:&str,request_started_at:&str,received_at:&str)->Assessment<Self>{assess((||{let started=clock(request_started_at)?;let received=clock(received_at)?;need(units>=0&&hex(source_sha256)&&started>=0&&started<=received,"MONEY_EVIDENCE")?;Ok(Self{units,source_sha256:source_sha256.into(),started,received})})())}}
/// Caller provenance only: hashes cannot certify that no other account activity
/// happened. No boolean or hash upgrades this diagnostic into account authority.
pub struct WindowProvenance{pub lease_sha256:String,pub before_journal_sha256:String,pub after_journal_sha256:String}
pub struct Bounds{pub maximum_snapshot_span_ms:u64,pub maximum_window_ms:u64}
#[derive(Clone)]struct Component{profile:String,covered:Vec<String>,relationships:Vec<String>,advance:i128}
/// No Deserialize/Serialize and no public economic fields. Original source
/// hashes remain private; aggregate outputs omit authenticated profile data.
pub struct Snapshot{scope_hash:String,profile:String,cash:i128,power:i128,available:i128,liability:i128,components:BTreeMap<String,Component>,started:i64,received:i64,source_hashes:[String;2]}
impl Snapshot{
 pub fn observe(scope:&Scope,account:&Receipt,collateral:&Receipt,bounds:&Bounds)->Assessment<Self>{assess(Self::checked(scope,account,collateral,bounds))}
 fn checked(scope:&Scope,a:&Receipt,c:&Receipt,b:&Bounds)->Checked<Self>{
  need(relationships::uuid(&scope.tournament_id)&&(1..=20_000).contains(&b.maximum_snapshot_span_ms)&&(1..=120_000).contains(&b.maximum_window_ms),"BOUNDS_OR_SCOPE")?;
  let(sa,ea)=receipt(a,&format!("/account?tournamentId={}",scope.tournament_id))?;let(sc,ec)=receipt(c,&format!("/portfolio/collateral?tournamentId={}",scope.tournament_id))?;
  let started=sa.min(sc);let received=ea.max(ec);need((ea<=sc||ec<=sa)&&received-started<=b.maximum_snapshot_span_ms as i64,"SNAPSHOT_CLOCK_OR_SPAN")?;
  let power=relationships::parse_buying_power(&a.data,scope).map_err(|_|"BUYING_POWER_INVALID")?;let collateral=relationships::parse_collateral(&c.data,scope).map_err(|_|"COLLATERAL_INVALID")?;
  let cash=power.gross_cash_units.ok_or("CASH_UNAVAILABLE")?;let buying=power.buying_power_units.ok_or("CASH_UNAVAILABLE")?;need(power.available_advance_units==collateral.available_advance_units,"SNAPSHOT_AVAILABLE_ADVANCE_CHANGED")?;
  let profile=a.data["id"].as_str().filter(|id|relationships::uuid(id)).ok_or("ACCOUNT_PROFILE")?;let mut components=BTreeMap::new();
  for row in collateral.components{need(row.profile_id==profile,"COLLATERAL_PROFILE_SCOPE")?;let mut covered=row.covered_exchange_ids;covered.sort();let mut relationships=row.relationship_ids;relationships.sort();components.insert(row.component_id,Component{profile:row.profile_id,covered,relationships,advance:row.outstanding_advance_units});}
  Ok(Self{scope_hash:hash(scope)?,profile:profile.into(),cash,power:buying,available:power.available_advance_units,liability:collateral.outstanding_liability_units,components,started,received,source_hashes:[feedback::digest(a).map_err(|_|"SOURCE_HASH")?,feedback::digest(c).map_err(|_|"SOURCE_HASH")?]})
 }
}
/// Opaque diagnostic only. No deserialization, mutator, native event type or
/// accounting-ready flag exists. Root must independently establish a closed
/// reconciled account window before integrating any financial continuation.
pub struct Plan{aggregate:Value,evidence:Value}
impl Plan{
 pub fn aggregate(&self)->&Value{&self.aggregate}
 /// Contains private source hashes/component IDs; do not publish raw evidence.
 pub fn private_evidence(&self)->&Value{&self.evidence}
}
/// Actual fill tapes are essential. Complete ACKs merely supply original ALL
/// reports and canonical physical order IDs, never positions or cash events.
pub fn reconcile(scope:&Scope,before:&Snapshot,after:&Snapshot,basket:&Basket,ack:&Receipt,tapes:&[Receipt],independent_fees:Option<&MoneyEvidence>,already_native_redemption:Option<&MoneyEvidence>,window:&WindowProvenance,bounds:&Bounds)->Assessment<Plan>{
 assess(reconcile_checked(scope,before,after,basket,ack,tapes,independent_fees,already_native_redemption,window,bounds))
}
fn reconcile_checked(scope:&Scope,before:&Snapshot,after:&Snapshot,basket:&Basket,ack:&Receipt,tapes:&[Receipt],fees:Option<&MoneyEvidence>,native_redemption:Option<&MoneyEvidence>,window:&WindowProvenance,bounds:&Bounds)->Checked<Plan>{
 let fees=fees.ok_or("FEES_UNAVAILABLE")?;let native_redemption=native_redemption.ok_or("NATIVE_REDEMPTION_UNAVAILABLE")?;
 need(before.scope_hash==hash(scope)?&&after.scope_hash==before.scope_hash&&before.profile==after.profile&&basket.tournament_id==scope.tournament_id,"RECONCILIATION_SCOPE")?;
 need((1..=120_000).contains(&bounds.maximum_window_ms)&&after.received>=before.started&&after.received-before.started<=bounds.maximum_window_ms as i64,"RECONCILIATION_WINDOW")?;
 need([&window.lease_sha256,&window.before_journal_sha256,&window.after_journal_sha256].into_iter().all(|s|hex(s)),"WINDOW_PROVENANCE")?;
 let ack_clocks=receipt(ack,"/orders/multi-leg")?;need(before.received<=ack_clocks.0&&ack_clocks.1<=after.started,"EXECUTION_WINDOW")?;
 need(fees.started<=fees.received&&fees.received<=after.received&&native_redemption.started>=before.started&&native_redemption.received<=after.received,"INDEPENDENT_MONEY_CLOCK")?;
 let reported=multi_leg::reported_fill(ack,basket).map_err(|_|"FULL_ACK_INVALID")?;
 need(tapes.len()>=reported.legs.len()&&tapes.len()<=12,"FILL_TAPE_COUNT")?;
 let mut canonical:BTreeMap<String,&Receipt>=BTreeMap::new();let mut source_hashes=Vec::new();
 for tape in tapes{
  let id=tape.data["orderId"].as_str().map(str::to_owned).or_else(||tape.data["orderId"].as_u64().map(|v|v.to_string())).ok_or("FILL_ORDER_ID")?;
  need(reported.legs.iter().any(|l|l.order_id==id),"FOREIGN_FILL_ORDER")?;
  let(start,end)=receipt(tape,&format!("/orders/{id}/fills?limit=200&tournamentId={}",scope.tournament_id))?;
  need(start>=ack_clocks.1&&end<=after.started,"FILL_OBSERVATION_WINDOW")?;
  source_hashes.push(feedback::digest(tape).map_err(|_|"SOURCE_HASH")?);
  if let Some(old)=canonical.get(&id){need(old.data==tape.data,"CHANGED_CUMULATIVE_TAPE")?;}else{canonical.insert(id,tape);}
 }
 need(canonical.len()==reported.legs.len(),"MISSING_COMPLETE_LEG_TAPE")?;
 let mut gross=0;let mut drawn=0;let mut repayment=0;let mut redemption=0;let mut power_impact=0;let mut own_after:BTreeMap<String,i128>=BTreeMap::new();let mut fill_count=0usize;
 for leg in reported.legs{
  let raw=&ack.data["results"][leg.index]["data"];
  let economics=relationships::parse_execution_economics(&raw["all"]).map_err(|_|"ALL_ECONOMICS_INVALID")?.ok_or("ALL_ECONOMICS_UNAVAILABLE")?;
  let tape=canonical.get(&leg.order_id).ok_or("MISSING_COMPLETE_LEG_TAPE")?;
  need(tape.data["data"].as_array().is_some_and(|a|a.len()<=100_000),"FILL_ROWS")?;
  let market=scope.exchanges.get(&leg.exchange_id).ok_or("FILL_EXCHANGE_SCOPE")?;
  let q=json!({"id":leg.index+1,"exchangeId":leg.exchange_id,"marketId":market,"side":if leg.side==multi_leg::Side::Yes{"YES"}else{"NO"},"action":"buy","quantity":leg.requested_quantity,"price":basket.legs[leg.index].limit_price});
  let events=feedback::fills(&q,&leg.order_id,&scope.tournament_id,tape,&tape.received_at).map_err(|_|"FILL_TAPE_INVALID")?;
  let mut quantity=0u64;let mut notional=0i128;
  for event in events{let n=event["quantity"].as_u64().ok_or("FILL_QUANTITY")?;quantity=quantity.checked_add(n).ok_or("ECONOMIC_OVERFLOW")?;let p=decimal_units(event["price"].as_str().ok_or("FILL_PRICE")?).map_err(|_|"FILL_PRICE")?;notional=add(notional,p.checked_mul(n as i128).ok_or("ECONOMIC_OVERFLOW")?)?;let executed=clock(event["executedAt"].as_str().ok_or("FILL_EXECUTION_CLOCK")?)?;need(before.received<=executed&&executed<=ack_clocks.1,"FILL_EXECUTION_WINDOW")?;fill_count+=1;}
  need(quantity==leg.requested_quantity as u64&&notional==leg.reported_notional_units&&notional==economics.full_notional_cost_units,"ACTUAL_NOTIONAL_OR_QUANTITY_MISMATCH")?;
  gross=add(gross,notional)?;drawn=add(drawn,economics.collateral_savings_units)?;repayment=add(repayment,economics.collateral_repayment_units)?;redemption=add(redemption,economics.redemption_credit_units)?;power_impact=add(power_impact,economics.net_buying_power_impact_units)?;
  if let Some(id)=economics.component_id{
   // Multiple legs may repeat the SAME after-state; these are never deltas.
   // The ACK index does not prove execution chronology. Different after-states
   // on the same component require additional official ordering evidence.
   if let Some(old)=own_after.insert(id.clone(),economics.outstanding_advance_after_units){need(old==economics.outstanding_advance_after_units,"AMBIGUOUS_COMPONENT_AFTER_ORDER")?;}
   let final_component=after.components.get(&id).ok_or("OWN_COMPONENT_MISSING_AFTER")?;need(final_component.advance==economics.outstanding_advance_after_units,"OWN_COMPONENT_AFTER_CHANGED")?;
  }else{need(economics.outstanding_advance_after_units==0,"UNLINKED_OWN_COMPONENT_AFTER")?;}
 }
 for(id,old)in &before.components{if let Some(new)=after.components.get(id){need(old.profile==new.profile&&old.covered==new.covered&&old.relationships==new.relationships,"COMPONENT_IDENTITY_CHANGED")?;}}
 let liability_delta=sub(after.liability,before.liability)?;let cash_delta=sub(after.cash,before.cash)?;
 need(liability_delta==sub(drawn,repayment)?,"LIABILITY_FLOW_MISMATCH")?;
 let expected_cash=add(sub(sub(drawn,gross)?,fees.units)?,sub(redemption,repayment)?)?;
 need(cash_delta==expected_cash,"CASH_FLOW_MISMATCH")?;
 need(sub(before.power,after.power)?==power_impact,"BUYING_POWER_FLOW_MISMATCH")?;
 let adjustment=add(sub(drawn,repayment)?,sub(redemption,native_redemption.units)?)?;
 let aggregate=json!({"diagnosticOnly":true,"atomicAccountBarrier":false,"actualFillCount":fill_count,"canonicalOrderCount":canonical.len(),"componentAfterStateCount":own_after.len(),"grossPositionCost":gross,"independentFees":fees.units,"advanceDrawnConsistent":drawn,"advanceDelta":liability_delta,"repayment":repayment,"redemption":redemption,"alreadyNativeRedemption":native_redemption.units,"actualCashDelta":cash_delta,"netCashAdjustment":adjustment,"netWealthExAdvance":sub(cash_delta,liability_delta)?,"wealthDefinition":"cash-change less debt-change; excludes position valuation and is not profit","availableAdvanceBefore":before.available,"availableAdvanceAfter":after.available,"outstandingLiabilityBefore":before.liability,"outstandingLiabilityAfter":after.liability,"moneyUnit":"one-millionth SUSQie"});
 let evidence=json!({"type":"collateral_reconciliation_diagnostic","source":"moloch_offline_consistency","aggregate":aggregate,"snapshotBefore":before.source_hashes,"snapshotAfter":after.source_hashes,"originalMultiLegReceiptHash":feedback::digest(ack).map_err(|_|"SOURCE_HASH")?,"completeFillTapeReceiptHashes":source_hashes,"independentFeesSourceHash":fees.source_sha256,"alreadyNativeRedemptionSourceHash":native_redemption.source_sha256,"leaseSourceHash":window.lease_sha256,"journalBeforeHash":window.before_journal_sha256,"journalAfterHash":window.after_journal_sha256,"componentAfterStates":own_after});
 Ok(Plan{aggregate,evidence})
}
