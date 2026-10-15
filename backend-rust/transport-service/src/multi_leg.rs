//! Strict, buy-only two/three-leg full-fill atomic baskets. No production
//! authority is created here; the existing Authority must admit the exact POST.
//! An ACK is never substituted for an independently reconciled complete fill tape.
use crate::{Result,Scope,Permit,RequestSpec,Receipt,TransportError,decimal_units,money_value,need,fail,id_value,prices,relationships};
use serde::{Serialize,Deserialize};
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::{collections::BTreeSet,path::{Path,PathBuf},fs::{self,OpenOptions},io::Write};

#[derive(Debug,Clone,Copy,Serialize,Deserialize,PartialEq,Eq)]
#[serde(rename_all="lowercase")]
pub enum Side {Yes,No}
impl Side {fn wire(self)->&'static str{match self{Self::Yes=>"yes",Self::No=>"no"}}}
#[derive(Debug,Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Leg {pub exchange_id:String,pub side:Side,pub quantity:u32,pub limit_price:String}
#[derive(Debug,Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Basket {
    pub idempotency_key:String,pub tournament_id:String,pub legs:Vec<Leg>,
    pub maximum_cost:String,pub relationship_constraint:Option<String>,
}
#[derive(Debug,Clone,Serialize)]
#[serde(rename_all="camelCase")]
pub struct ResponseEvidence {
    pub private_body_path:PathBuf,pub private_metadata_path:PathBuf,
    pub body_sha256:String,pub request_sha256:String,pub status:u16,
    pub request_started_at:String,pub received_at:String,pub complete_body:bool,
}
#[derive(Debug,Clone,Serialize)]
#[serde(rename_all="camelCase")]
pub struct RequestEvidence {pub private_path:PathBuf,pub request_sha256:String,pub logical_payload_sha256:String}
#[derive(Debug,Clone,Serialize)]
#[serde(rename_all="camelCase")]
pub struct AcknowledgedLeg {
    pub index:usize,pub order_id:String,pub exchange_id:String,pub side:Side,
    pub requested_quantity:u32,pub reported_quantity_traded_units:i128,
    pub reported_notional_units:i128,pub reported_last_fill_price_units:i128,
    pub all:Option<relationships::ExecutionEconomics>,
}
#[derive(Debug,Clone,Serialize)]
#[serde(rename_all="camelCase")]
pub struct FullFillAcknowledgment {
    pub legs:Vec<AcknowledgedLeg>,pub reported_gross_notional_units:i128,
    pub requires_complete_fill_reconciliation:bool,
}
#[derive(Debug,Clone,Serialize,PartialEq,Eq)]
#[serde(tag="kind",rename_all="snake_case")]
pub enum ZeroExecutionReason {AllOrNoneNotFilled{leg:usize},RateLimited,Unauthorized,Forbidden,NotFound,RelationshipViolation}
/// No auto-retries. Unknown keeps the same durable package binding unresolved.
/// Even FullFillReported is a transport ACK; inventory/accounting requires fills.
pub enum Outcome {
    FullFillReported{receipt:Receipt,acknowledgment:FullFillAcknowledgment,evidence:ResponseEvidence,request_evidence:RequestEvidence},
    NoExecution{error:TransportError,reason:ZeroExecutionReason,evidence:ResponseEvidence,request_evidence:RequestEvidence},
    Unknown{error:TransportError,evidence:Option<ResponseEvidence>,request_evidence:RequestEvidence},
}
pub(crate) fn record_request(directory:&Path,spec:&RequestSpec,basket:&Basket,permit:&Permit)->Result<RequestEvidence>{
    let request_sha256=format!("{:x}",Sha256::digest(serde_json::to_vec(spec).map_err(|_|fail("BASKET_REQUEST_SERIALIZATION"))?));
    let logical=json!({"request":spec,"basket":basket,"runId":permit.run_id,"intentId":permit.intent_id,"owner":permit.owner});
    let bytes=serde_json::to_vec(&logical).map_err(|_|fail("BASKET_REQUEST_SERIALIZATION"))?;
    let logical_payload_sha256=format!("{:x}",Sha256::digest(&bytes));
    let private_path=directory.join(format!("basket-request-{logical_payload_sha256}.json"));
    let mut options=OpenOptions::new();options.write(true).create_new(true);#[cfg(unix)]{use std::os::unix::fs::OpenOptionsExt;options.mode(0o600);}
    match options.open(&private_path){
        Ok(mut file)=>{file.write_all(&bytes).and_then(|_|file.sync_all()).map_err(|_|fail("BASKET_REQUEST_EVIDENCE_DURABILITY"))?;#[cfg(unix)]fs::File::open(directory).and_then(|f|f.sync_all()).map_err(|_|fail("BASKET_REQUEST_EVIDENCE_DURABILITY"))?;},
        Err(e) if e.kind()==std::io::ErrorKind::AlreadyExists=>{need(fs::read(&private_path).map_err(|_|fail("BASKET_REQUEST_EVIDENCE_READ"))?==bytes,"BASKET_REQUEST_EVIDENCE_CHANGED")?;},
        Err(_)=>return Err(fail("BASKET_REQUEST_EVIDENCE_DURABILITY")),
    }
    Ok(RequestEvidence{private_path,request_sha256,logical_payload_sha256})
}
pub(crate) fn request(basket:&Basket,scope:&Scope,permit:&Permit)->Result<RequestSpec>{
    need(basket.tournament_id==scope.tournament_id&&relationships::uuid(&basket.tournament_id),"BASKET_TOURNAMENT_SCOPE")?;
    need((2..=3).contains(&basket.legs.len())&&!basket.idempotency_key.is_empty()&&basket.idempotency_key.len()<=255&&!basket.idempotency_key.chars().any(char::is_control),"BASKET_SHAPE")?;
    let maximum=decimal_units(&basket.maximum_cost)?;let permit_cost=decimal_units(&permit.maximum_cost)?;
    need(maximum>0&&maximum<=permit_cost,"BASKET_MAXIMUM_COST")?;
    let mut seen_exchanges=BTreeSet::new();let mut seen_sides=BTreeSet::new();let mut gross=0i128;let mut quantity=0u64;let mut legs=vec![];
    for leg in &basket.legs {
        need(scope.exchanges.contains_key(&leg.exchange_id),"BASKET_EXCHANGE_SCOPE_OR_DUPLICATE")?;
        // Official multi-leg duplicate-exchange rejection applies only when a
        // relationshipConstraint is supplied. Unconstrained complementary BUYs
        // may share an exchange; still reject repeated same-side BUY legs.
        need(seen_sides.insert((&leg.exchange_id,leg.side.wire())),"BASKET_EXCHANGE_SCOPE_OR_DUPLICATE")?;
        need(basket.relationship_constraint.is_none()||seen_exchanges.insert(&leg.exchange_id),"BASKET_CONSTRAINED_DUPLICATE_EXCHANGE")?;
        need(leg.quantity>0&&leg.quantity<=2_147_483_647,"BASKET_QUANTITY")?;
        quantity=quantity.checked_add(leg.quantity as u64).ok_or_else(||fail("BASKET_QUANTITY_OVERFLOW"))?;
        let price=decimal_units(&leg.limit_price)?;
        need((5000..=995000).contains(&price)&&price%5000==0,"BASKET_LIMIT_TICK")?;
        gross=gross.checked_add(price.checked_mul(leg.quantity as i128).ok_or_else(||fail("BASKET_COST_OVERFLOW"))?).ok_or_else(||fail("BASKET_COST_OVERFLOW"))?;
        let wire=format!("{}.{:06}",price/1_000_000,price%1_000_000);
        legs.push(json!({"exchangeId":leg.exchange_id,"side":leg.side.wire(),"action":"buy","quantity":leg.quantity,"price":serde_json::Number::from_string_unchecked(wire),"tournamentId":basket.tournament_id}));
    }
    // Existing Permit.maximum_quantity bounds aggregate submitted shares here,
    // not quantity per leg. Full gross principal is reserved without ALL loans.
    need(gross<=maximum&&quantity<=permit.maximum_quantity as u64,"BASKET_COST_OR_QUANTITY_CAP")?;
    let mut body=json!({"idempotencyKey":basket.idempotency_key,"executionMode":"allOrNone","legs":legs});
    if let Some(id)=&basket.relationship_constraint {need(relationships::uuid(id),"BASKET_RELATIONSHIP_ID")?;body["relationshipConstraint"]=json!(id);}
    let body_json=serde_json::to_string(&body).map_err(|_|fail("BASKET_SERIALIZATION"))?;
    Ok(RequestSpec{method:"POST".into(),endpoint:"/orders/multi-leg".into(),body:Some(body),body_json:Some(body_json)})
}
pub(crate) fn validate_ack(value:&Value,basket:&Basket)->Result<FullFillAcknowledgment>{
    let rows=value["results"].as_array().ok_or_else(||fail("BASKET_ACK_RESULTS"))?;
    need(rows.len()==basket.legs.len()&&value.get("error").is_none(),"BASKET_ACK_INCOMPLETE")?;
    let mut ids=BTreeSet::new();let mut legs=vec![];let mut total=0i128;
    for (index,(row,leg)) in rows.iter().zip(&basket.legs).enumerate(){
        need(row["index"].as_u64()==Some(index as u64),"BASKET_ACK_INDEX")?;
        let data=&row["data"];
        let id=id_value(&data["orderId"]).ok_or_else(||fail("BASKET_ACK_ORDER_ID"))?;
        need(ids.insert(id.clone())&&data["exchangeId"]==leg.exchange_id&&data["side"]==leg.side.wire()&&data["action"]=="buy","BASKET_ACK_SCOPE_OR_CANONICAL_CHANGE")?;
        need(data["open"]==false&&data["quantity"].as_u64()==Some(leg.quantity as u64)&&money_value(&data["remainingQuantity"])?==0,"BASKET_ACK_NOT_FULL")?;
        let traded=money_value(&data["quantityTraded"])?;
        // NO quantities can be signed in official ACKs. Compare magnitude, not
        // a fabricated signed notional. Side identity was checked independently.
        need(traded.unsigned_abs()==leg.quantity as u128*1_000_000,"BASKET_ACK_PARTIAL_FILL")?;
        let limit=decimal_units(&leg.limit_price)?;
        need(prices::official_tick_units(&data["price"])?==limit,"BASKET_ACK_LIMIT_CHANGED")?;
        let notional=money_value(&data["totalCost"])?;
        need(notional>=0&&notional<=limit.checked_mul(leg.quantity as i128).ok_or_else(||fail("BASKET_COST_OVERFLOW"))?,"BASKET_ACK_NOTIONAL_CAP")?;
        let last=prices::official_tick_units(&data["fillPrice"])?;need((0..=limit).contains(&last),"BASKET_ACK_LAST_FILL_LIMIT")?;
        need(data.get("terminalReasonCode").is_none_or(|v|v.is_null()),"BASKET_ACK_TERMINAL_REJECTION")?;
        let all=relationships::parse_execution_economics(data.get("all").ok_or_else(||fail("BASKET_ACK_ALL_MISSING"))?)?;
        if let Some(economics)=&all {need(economics.full_notional_cost_units==notional,"BASKET_ACK_ALL_NOTIONAL")?;}
        total=total.checked_add(notional).ok_or_else(||fail("BASKET_COST_OVERFLOW"))?;
        legs.push(AcknowledgedLeg{index,order_id:id,exchange_id:leg.exchange_id.clone(),side:leg.side,requested_quantity:leg.quantity,reported_quantity_traded_units:traded,reported_notional_units:notional,reported_last_fill_price_units:last,all});
    }
    need(total<=decimal_units(&basket.maximum_cost)?,"BASKET_ACK_AGGREGATE_CAP")?;
    Ok(FullFillAcknowledgment{legs,reported_gross_notional_units:total,requires_complete_fill_reconciliation:true})
}
/// Only documented code/status combinations establish zero execution. Unknown
/// JSON, malformed error shapes, 409 claims and all 5xx stay unresolved.
pub(crate) fn zero_execution(status:u16,data:&Value,leg_count:usize)->Option<ZeroExecutionReason>{
    if data.get("results").is_some()||!data["error"]["message"].is_string(){return None;}
    let code=data["error"]["code"].as_str()?;
    match (status,code){
        (400,"VALIDATION_ERROR") if data["error"]["details"]["rejectionReasonCode"]=="AllOrNoneNotFilled"=>{let leg=usize::try_from(data["error"]["details"]["leg"].as_u64()?).ok()?;(leg<leg_count).then_some(ZeroExecutionReason::AllOrNoneNotFilled{leg})},
        (429,"RATE_LIMITED")=>Some(ZeroExecutionReason::RateLimited),
        (401,"MISSING_API_KEY"|"INVALID_API_KEY"|"API_KEY_REVOKED"|"API_KEY_EXPIRED"|"UNAUTHORIZED")=>Some(ZeroExecutionReason::Unauthorized),
        (403,"FORBIDDEN"|"INSUFFICIENT_SCOPES"|"TERMS_NOT_ACKNOWLEDGED"|"ACCOUNT_BANNED"|"RESIDENCE_UPDATE_REQUIRED")=>Some(ZeroExecutionReason::Forbidden),
        (404,"NOT_FOUND")=>Some(ZeroExecutionReason::NotFound),
        (422,"RELATIONSHIP_VIOLATION")=>Some(ZeroExecutionReason::RelationshipViolation),
        _=>None,
    }
}
pub(crate) fn record_response(directory:&Path,request:&str,spec:&RequestSpec,status:u16,started:&str,received:&str,bytes:&[u8],complete:bool)->Result<ResponseEvidence>{
    let body_path=directory.join(format!("basket-response-{request}.body"));
    let metadata_path=directory.join(format!("basket-response-{request}.json"));
    let request_hash=format!("{:x}",Sha256::digest(serde_json::to_vec(spec).map_err(|_|fail("BASKET_REQUEST_SERIALIZATION"))?));
    let evidence=ResponseEvidence{private_body_path:body_path.clone(),private_metadata_path:metadata_path.clone(),body_sha256:format!("{:x}",Sha256::digest(bytes)),request_sha256:request_hash,status,request_started_at:started.into(),received_at:received.into(),complete_body:complete};
    let write=|path:&Path,bytes:&[u8]|->std::io::Result<()>{let mut options=OpenOptions::new();options.write(true).create_new(true);#[cfg(unix)]{use std::os::unix::fs::OpenOptionsExt;options.mode(0o600);}let mut file=options.open(path)?;file.write_all(bytes)?;file.sync_all()};
    let result=(||->std::io::Result<()>{write(&body_path,bytes)?;write(&metadata_path,&serde_json::to_vec(&json!({"evidence":evidence,"request":spec}))?)?;#[cfg(unix)]fs::File::open(directory)?.sync_all()?;Ok(())})();
    result.map_err(|_|fail("BASKET_RESPONSE_EVIDENCE_DURABILITY"))?;Ok(evidence)
}
pub(crate) fn read_evidenced_error(evidence:&ResponseEvidence)->Result<Value>{
    need(evidence.complete_body,"BASKET_RESPONSE_BODY_INCOMPLETE")?;
    let bytes=fs::read(&evidence.private_body_path).map_err(|_|fail("BASKET_RESPONSE_EVIDENCE_READ"))?;
    need(bytes.len()<=8*1024*1024&&format!("{:x}",Sha256::digest(&bytes))==evidence.body_sha256,"BASKET_RESPONSE_EVIDENCE_CHANGED")?;
    serde_json::from_slice(&bytes).map_err(|_|fail("INVALID_RESPONSE_JSON"))
}

/// Reconstruct the exact existing wire request without sending or authorizing it.
pub fn request_spec(basket:&Basket,scope:&Scope,permit:&Permit)->Result<RequestSpec>{request(basket,scope,permit)}
/// A reported all-filled response remains distinct from official fill evidence.
pub fn reported_fill(receipt:&Receipt,basket:&Basket)->Result<FullFillAcknowledgment>{
    need(receipt.endpoint=="/orders/multi-leg"&&receipt.status==200&&(2..=3).contains(&basket.legs.len()),"BASKET_RECEIPT_SCOPE")?;
    let start=chrono::DateTime::parse_from_rfc3339(&receipt.request_started_at).map_err(|_|fail("BASKET_RECEIPT_CLOCK"))?;
    let end=chrono::DateTime::parse_from_rfc3339(&receipt.received_at).map_err(|_|fail("BASKET_RECEIPT_CLOCK"))?;
    need(start<=end,"BASKET_RECEIPT_CLOCK")?;validate_ack(&receipt.data,basket)
}
/// Verify retained original response bytes; callers must additionally bind metadata.
pub fn evidenced_response(evidence:&ResponseEvidence)->Result<Value>{read_evidenced_error(evidence)}
/// Use the same documented zero-execution allowlist as the physical SDK.
pub fn zero_execution_reason(status:u16,data:&Value,leg_count:usize)->Option<ZeroExecutionReason>{zero_execution(status,data,leg_count)}

#[cfg(test)]mod tests;
