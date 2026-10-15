//! Typed signed all-or-none portfolios. Same official endpoint, original client,
//! limiter and Authority; no new account or execution permission. Covered SELL
//! positions and the exact full-gross funding plan belong to native/Authority.
//!
//! Primary saved OpenAPI /paths/~1orders~1multi-leg/post permits 1..10 legs;
//! this initial checked SDK admits 1..6. OrderInput.action is buy|sell. The
//! MultiLegOrderData canonical action/side can change on an uncovered SELL;
//! that economic change is UNKNOWN here, never silently adopted as a BUY.
//! Optional relationshipConstraint is a pre-lock YES-price guard: NO limit
//! prices are complemented, exhaustive exclusion requires sum(YES)=1. It is
//! neither an atomic settlement certificate nor a floor. In particular it can
//! reject a profitable certified dual-NO packet; never auto-add it from a cert.
use crate::{Result,Scope,Permit,RequestSpec,Receipt,TransportError,need,fail,decimal_units,money_value,id_value,prices,relationships,multi_leg};
use serde::{Serialize,Deserialize};
use serde_json::{Value,json};
use std::{collections::BTreeSet,fs::{self,OpenOptions},io::Write,path::Path};
pub use multi_leg::{Side,RequestEvidence,ResponseEvidence,ZeroExecutionReason};
#[derive(Debug,Clone,Copy,Serialize,Deserialize,PartialEq,Eq)]
#[serde(rename_all="lowercase")]
pub enum Action{Buy,Sell}
impl Action{pub fn wire(self)->&'static str{match self{Self::Buy=>"buy",Self::Sell=>"sell"}}}
fn side(s:Side)->&'static str{match s{Side::Yes=>"yes",Side::No=>"no"}}
#[derive(Debug,Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Leg{pub exchange_id:String,pub side:Side,pub action:Action,pub quantity:u32,pub limit_price:String}
#[derive(Debug,Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Basket{pub idempotency_key:String,pub tournament_id:String,pub legs:Vec<Leg>,pub maximum_gross_buy_cost:String,pub minimum_gross_sell_proceeds:String,pub relationship_constraint:Option<String>}
#[derive(Debug,Clone,Serialize)]
#[serde(rename_all="camelCase")]
pub struct AcknowledgedLeg{pub index:usize,pub order_id:String,pub exchange_id:String,pub side:Side,pub action:Action,pub requested_quantity:u32,pub reported_quantity_traded_units:i128,pub reported_notional_units:i128,pub reported_last_fill_price_units:i128,pub all:Option<relationships::ExecutionEconomics>}
#[derive(Debug,Clone,Serialize)]
#[serde(rename_all="camelCase")]
pub struct FullFillAcknowledgment{pub legs:Vec<AcknowledgedLeg>,pub gross_buy_notional_units:i128,pub gross_sell_proceeds_units:i128,pub requires_complete_fill_reconciliation:bool}
/// A full-fill report is an ACK only. No automatic fill/cash/debt accounting.
pub enum Outcome{
 FullFillReported{receipt:Receipt,acknowledgment:FullFillAcknowledgment,evidence:ResponseEvidence,request_evidence:RequestEvidence},
 NoExecution{error:TransportError,reason:ZeroExecutionReason,evidence:ResponseEvidence,request_evidence:RequestEvidence},
 Unknown{error:TransportError,evidence:Option<ResponseEvidence>,request_evidence:RequestEvidence},
}
pub fn request_spec(b:&Basket,s:&Scope,p:&Permit)->Result<RequestSpec>{
 need(b.tournament_id==s.tournament_id&&relationships::uuid(&b.tournament_id),"PORTFOLIO_TOURNAMENT_SCOPE")?;
 need((1..=6).contains(&b.legs.len())&&!b.idempotency_key.is_empty()&&b.idempotency_key.len()<=255&&!b.idempotency_key.chars().any(char::is_control),"PORTFOLIO_SHAPE")?;
 let cap=decimal_units(&b.maximum_gross_buy_cost)?;let minimum=decimal_units(&b.minimum_gross_sell_proceeds)?;let permit_cap=decimal_units(&p.maximum_cost)?;
 need(cap>=0&&minimum>=0&&permit_cap>=0&&cap<=permit_cap,"PORTFOLIO_GROSS_CAP")?;
 let mut buy=0i128;let mut sell=0i128;let mut quantity=0u64;let mut seen=BTreeSet::new();let mut exchanges=BTreeSet::new();let mut legs=vec![];
 for l in &b.legs{
  need(s.exchanges.contains_key(&l.exchange_id)&&seen.insert((&l.exchange_id,side(l.side),l.action.wire())),"PORTFOLIO_EXCHANGE_SCOPE_OR_DUPLICATE")?;
  need(b.relationship_constraint.is_none()||exchanges.insert(&l.exchange_id),"PORTFOLIO_CONSTRAINED_DUPLICATE_EXCHANGE")?;
  need(l.quantity>0&&l.quantity<=2_147_483_647,"PORTFOLIO_QUANTITY")?;quantity=quantity.checked_add(l.quantity as u64).ok_or_else(||fail("PORTFOLIO_QUANTITY_OVERFLOW"))?;
  let price=decimal_units(&l.limit_price)?;need((5000..=995000).contains(&price)&&price%5000==0,"PORTFOLIO_LIMIT_TICK")?;
  let n=price.checked_mul(l.quantity as i128).ok_or_else(||fail("PORTFOLIO_OVERFLOW"))?;let target=if l.action==Action::Buy{&mut buy}else{&mut sell};*target=target.checked_add(n).ok_or_else(||fail("PORTFOLIO_OVERFLOW"))?;
  legs.push(json!({"exchangeId":l.exchange_id,"side":side(l.side),"action":l.action.wire(),"quantity":l.quantity,"price":serde_json::Number::from_string_unchecked(format!("{}.{:06}",price/1_000_000,price%1_000_000)),"tournamentId":b.tournament_id}));
 }
 // SELL proceeds, redemption and ALL credit never fund gross BUY admission.
 need(buy<=cap&&minimum<=sell&&quantity<=p.maximum_quantity as u64,"PORTFOLIO_COST_OR_QUANTITY_CAP")?;
 let mut body=json!({"idempotencyKey":b.idempotency_key,"executionMode":"allOrNone","legs":legs});
 if let Some(id)=&b.relationship_constraint{need(relationships::uuid(id),"PORTFOLIO_RELATIONSHIP_ID")?;body["relationshipConstraint"]=json!(id);}
 let body_json=serde_json::to_string(&body).map_err(|_|fail("PORTFOLIO_SERIALIZATION"))?;
 Ok(RequestSpec{method:"POST".into(),endpoint:"/orders/multi-leg".into(),body:Some(body),body_json:Some(body_json)})
}
pub(crate) fn record_request(root:&Path,spec:&RequestSpec,basket:&Basket,p:&Permit)->Result<RequestEvidence>{
 let request_sha256=crate::digest(&serde_json::to_vec(spec).map_err(|_|fail("PORTFOLIO_SERIALIZATION"))?);
 let logical=json!({"request":spec,"basket":basket,"runId":p.run_id,"intentId":p.intent_id,"owner":p.owner});let bytes=serde_json::to_vec(&logical).map_err(|_|fail("PORTFOLIO_SERIALIZATION"))?;let logical_payload_sha256=crate::digest(&bytes);let private_path=root.join(format!("basket-request-{logical_payload_sha256}.json"));
 let mut o=OpenOptions::new();o.write(true).create_new(true);#[cfg(unix)]{use std::os::unix::fs::OpenOptionsExt;o.mode(0o600);}
 // First-call tickets separately reject every repeat, including identical old
 // artifacts. This preserves the existing raw RequestEvidence record format.
 match o.open(&private_path){Ok(mut f)=>{f.write_all(&bytes).and_then(|_|f.sync_all()).map_err(|_|fail("PORTFOLIO_REQUEST_EVIDENCE_DURABILITY"))?;#[cfg(unix)]fs::File::open(root).and_then(|f|f.sync_all()).map_err(|_|fail("PORTFOLIO_REQUEST_EVIDENCE_DURABILITY"))?;},Err(_)=>return Err(fail("PORTFOLIO_REQUEST_EVIDENCE_EXISTS_OR_UNAVAILABLE"))}
 Ok(RequestEvidence{private_path,request_sha256,logical_payload_sha256})
}
pub(crate) fn validate_ack(v:&Value,b:&Basket)->Result<FullFillAcknowledgment>{
 need((1..=6).contains(&b.legs.len()),"PORTFOLIO_ACK_SHAPE")?;
 let rows=v["results"].as_array().ok_or_else(||fail("PORTFOLIO_ACK_RESULTS"))?;need(rows.len()==b.legs.len()&&v.get("error").is_none(),"PORTFOLIO_ACK_INCOMPLETE")?;
 let mut ids=BTreeSet::new();let mut legs=vec![];let mut buy=0i128;let mut sell=0i128;
 for(index,(r,l))in rows.iter().zip(&b.legs).enumerate(){let d=&r["data"];need(r["index"].as_u64()==Some(index as u64),"PORTFOLIO_ACK_INDEX")?;
  let id=id_value(&d["orderId"]).ok_or_else(||fail("PORTFOLIO_ACK_ORDER_ID"))?;
  need(ids.insert(id.clone())&&d["exchangeId"]==l.exchange_id&&d["side"]==side(l.side)&&d["action"]==l.action.wire(),"PORTFOLIO_ACK_CANONICAL_CHANGE")?;
  need(d["open"]==false&&d["quantity"].as_u64()==Some(l.quantity as u64)&&money_value(&d["remainingQuantity"])?==0,"PORTFOLIO_ACK_NOT_FULL")?;
  let traded=money_value(&d["quantityTraded"])?;need(traded.unsigned_abs()==l.quantity as u128*1_000_000,"PORTFOLIO_ACK_PARTIAL")?;
  need(l.quantity>0&&l.quantity<=2_147_483_647,"PORTFOLIO_ACK_QUANTITY")?;
  let limit=decimal_units(&l.limit_price)?;need((5000..=995000).contains(&limit)&&limit%5000==0,"PORTFOLIO_ACK_LIMIT_TICK")?;need(prices::official_tick_units(&d["price"])?==limit,"PORTFOLIO_ACK_LIMIT_CHANGED")?;
  let notional=money_value(&d["totalCost"])?;let bound=limit.checked_mul(l.quantity as i128).ok_or_else(||fail("PORTFOLIO_OVERFLOW"))?;let maximum=1_000_000i128.checked_mul(l.quantity as i128).ok_or_else(||fail("PORTFOLIO_OVERFLOW"))?;
  need(notional>=0&&notional<=maximum&&(if l.action==Action::Buy{notional<=bound}else{notional>=bound}),"PORTFOLIO_ACK_NOTIONAL_BOUND")?;
  let last=prices::official_tick_units(&d["fillPrice"])?;need((if l.action==Action::Buy{0<=last&&last<=limit}else{limit<=last&&last<=1_000_000}),"PORTFOLIO_ACK_FILL_LIMIT")?;
  need(d.get("terminalReasonCode").is_none_or(|c|c.is_null()),"PORTFOLIO_ACK_TERMINAL_REASON")?;
  let all=relationships::parse_execution_economics(d.get("all").ok_or_else(||fail("PORTFOLIO_ACK_ALL_MISSING"))?)?;
  if let Some(a)=&all{need(a.full_notional_cost_units==notional,"PORTFOLIO_ACK_ALL_NOTIONAL")?;if l.action==Action::Sell{need(a.effective_entry_cost_units==notional&&a.collateral_savings_units==0,"PORTFOLIO_ACK_SALE_ALL")?;}}
  let total=if l.action==Action::Buy{&mut buy}else{&mut sell};*total=total.checked_add(notional).ok_or_else(||fail("PORTFOLIO_OVERFLOW"))?;
  legs.push(AcknowledgedLeg{index,order_id:id,exchange_id:l.exchange_id.clone(),side:l.side,action:l.action,requested_quantity:l.quantity,reported_quantity_traded_units:traded,reported_notional_units:notional,reported_last_fill_price_units:last,all});
 }
 need(buy<=decimal_units(&b.maximum_gross_buy_cost)?&&sell>=decimal_units(&b.minimum_gross_sell_proceeds)?,"PORTFOLIO_ACK_AGGREGATE_CAP")?;
 Ok(FullFillAcknowledgment{legs,gross_buy_notional_units:buy,gross_sell_proceeds_units:sell,requires_complete_fill_reconciliation:true})
}
pub fn reported_fill(r:&Receipt,b:&Basket)->Result<FullFillAcknowledgment>{
 need(r.endpoint=="/orders/multi-leg"&&r.status==200&&(1..=6).contains(&b.legs.len()),"PORTFOLIO_RECEIPT_SCOPE")?;
 let a=chrono::DateTime::parse_from_rfc3339(&r.request_started_at).map_err(|_|fail("PORTFOLIO_RECEIPT_CLOCK"))?;let z=chrono::DateTime::parse_from_rfc3339(&r.received_at).map_err(|_|fail("PORTFOLIO_RECEIPT_CLOCK"))?;need(a<=z,"PORTFOLIO_RECEIPT_CLOCK")?;validate_ack(&r.data,b)
}
