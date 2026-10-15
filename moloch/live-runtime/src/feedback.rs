use baal_transport_service::{Receipt,prices::official_tick_units};
use moloch_replay::money;
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::collections::BTreeSet;
pub type Result<T> = std::result::Result<T,Box<dyn std::error::Error>>;
fn id(v:&Value)->Result<String>{let text=v.as_str().map(str::to_owned).or_else(||v.as_u64().map(|n|n.to_string())).ok_or("ACCOUNT_ID")?;if text.is_empty()||text.starts_with('0')||!text.bytes().all(|b|b.is_ascii_digit()){return Err("ACCOUNT_ID".into())}Ok(text)}
pub fn digest(r:&Receipt)->Result<String>{Ok(format!("{:x}",Sha256::digest(serde_json::to_vec(r)?)))}
fn base(kind:&str,q:&Value,actual:&str,r:&Receipt,processed_at:&str)->Result<Value>{if r.status!=200{return Err("ACCOUNT_RECEIPT_STATUS".into())}let received=chrono::DateTime::parse_from_rfc3339(&r.received_at)?;let processed=chrono::DateTime::parse_from_rfc3339(processed_at)?;if processed<received{return Err("ACCOUNT_PROCESSING_CLOCK".into())}Ok(json!({"type":kind,"source":"official_sig_api_v1","receiptHash":digest(r)?,"orderId":q["id"],"actualOrderId":actual,"exchangeId":q["exchangeId"],"marketId":q["marketId"],"side":q["side"],"action":q["action"],"receivedAt":r.received_at,"processedAt":processed_at}))}
/// A canonicalized side/action or increased size is not silently adopted.
pub fn acknowledgment(q:&Value,r:&Receipt,processed_at:&str)->Result<Value>{
 let v=&r.data;let actual=id(&v["orderId"])?;let n=v["quantity"].as_u64().filter(|n|*n>0).ok_or("ACCOUNT_ACK_QUANTITY")?;
 if v["exchangeId"]!=q["exchangeId"]||v["side"].as_str()!=q["side"].as_str().map(|s|if s=="YES"{"yes"}else{"no"})||v["action"]!=q["action"]||n>q["quantity"].as_u64().ok_or("INTENT_QUANTITY")?||money(official_tick_units(&v["price"])?)!=q["price"].as_str().ok_or("INTENT_PRICE")?{return Err("ACCOUNT_ACK_INTENT_MISMATCH".into())}
 let mut e=base("actual_order_ack",q,&actual,r,processed_at)?;e["quantity"]=json!(n);e["price"]=q["price"].clone();Ok(e)
}
/// Fill quantity is outcome-signed. Its action comes from the owned order.
pub fn fills(q:&Value,actual:&str,tournament:&str,r:&Receipt,processed_at:&str)->Result<Vec<Value>>{
 let v=&r.data;if id(&v["orderId"])?!=actual||v["exchangeId"]!=q["exchangeId"]||v["tournamentId"]!=tournament||v["coverage"]["complete"]!=true||v["pagination"]["hasMore"]!=false||!v["pagination"]["nextCursor"].is_null(){return Err("ACCOUNT_FILL_SCOPE_OR_COVERAGE".into())}
 let total=v["totalQuantityFilled"].as_i64().ok_or("ACCOUNT_FILL_TOTAL")?;let no=q["side"]=="NO";if total!=0&&((total<0)!=no){return Err("ACCOUNT_FILL_TOTAL_SIDE".into())}
 let mut seen=BTreeSet::new();let mut sum=0u64;let mut events=Vec::new();
 for row in v["data"].as_array().ok_or("ACCOUNT_FILL_ROWS")? {
  let fill_id=id(&row["id"])?;let signed=row["quantity"].as_i64().filter(|n|*n!=0).ok_or("ACCOUNT_FILL_QUANTITY")?;
  if (signed<0)!=no||row["side"]!=if no{"no"}else{"yes"}||!seen.insert(fill_id.clone()){return Err("ACCOUNT_FILL_SIDE_OR_DUPLICATE".into())}
  let n=signed.checked_abs().ok_or("ACCOUNT_FILL_QUANTITY")? as u64;sum=sum.checked_add(n).ok_or("ACCOUNT_FILL_QUANTITY")?;
  let mut e=base("actual_fill",q,actual,r,processed_at)?;e["fillId"]=json!(fill_id);e["quantity"]=json!(n);e["price"]=json!(money(official_tick_units(&row["price"])?));e["fee"]=json!("0.000000");e["executedAt"]=row["filledAt"].clone();events.push(e);
 }
 if sum!=total.checked_abs().ok_or("ACCOUNT_FILL_TOTAL")? as u64||sum>q["quantity"].as_u64().ok_or("INTENT_QUANTITY")?{return Err("ACCOUNT_FILL_TOTAL_MISMATCH".into())}Ok(events)
}
/// A closed-order quantity is interpreted separately from a resting remainder.
pub fn terminal(q:&Value,actual:&str,tournament:&str,order:&Receipt,filled:&Receipt,processed_at:&str)->Result<Option<Value>>{
 let v=&order.data;if order.status!=200||id(&v["id"])?!=actual||v["exchangeId"]!=q["exchangeId"]||v["tournamentId"]!=tournament||v["side"].as_str()!=q["side"].as_str().map(|s|if s=="YES"{"yes"}else{"no"})||v["action"]!=q["action"]||money(official_tick_units(&v["priceLimit"])?)!=q["price"].as_str().ok_or("INTENT_PRICE")?{return Err("ACCOUNT_ORDER_SCOPE".into())}
 let events=fills(q,actual,tournament,filled,processed_at)?;let sum: u64=events.iter().map(|e|e["quantity"].as_u64().unwrap()).sum();
 if v["open"]==true{return Ok(None)}if v["open"]!=false||v["quantityFilled"].as_u64()!=Some(sum)||v["quantity"].as_u64().is_none_or(|n|n<sum||n>q["quantity"].as_u64().unwrap_or(0)){return Err("ACCOUNT_TERMINAL_TOTAL_MISMATCH".into())}
 if chrono::DateTime::parse_from_rfc3339(&filled.request_started_at)?<chrono::DateTime::parse_from_rfc3339(&order.received_at)?{return Err("ACCOUNT_TERMINAL_FILL_OBSERVATION_ORDER".into())}
 let mut e=base("actual_order_terminal",q,actual,filled,processed_at)?;e["orderReceiptHash"]=json!(digest(order)?);e["fillsComplete"]=json!(true);e["filledQuantity"]=json!(sum);e["status"]=json!(if sum==q["quantity"].as_u64().unwrap(){"filled"}else if v["terminalReasonCode"]=="Expired"{"expired"}else{"canceled"});Ok(Some(e))
}

#[cfg(test)]mod tests{
 use super::*;
 fn q()->Value{json!({"id":1,"marketId":"101","exchangeId":"994","side":"NO","action":"sell","quantity":2,"price":"0.055000"})}
 fn r(data:Value,start:&str,end:&str)->Receipt{Receipt{endpoint:"/fixture".into(),status:200,request_started_at:start.into(),received_at:end.into(),data}}
 fn tape()->Receipt{r(json!({"orderId":201,"exchangeId":"994","tournamentId":"cup","coverage":{"complete":true,"projectedThroughSequence":100},"pagination":{"hasMore":false,"nextCursor":null},"totalQuantityFilled":-2,"data":[{"id":301,"price":0.06000000000000005,"quantity":-1,"side":"no","filledAt":"2026-10-15T00:00:01Z"},{"id":302,"price":0.055,"quantity":-1,"side":"no","filledAt":"2026-10-15T00:00:02Z"}]}),"2026-10-15T00:00:04Z","2026-10-15T00:00:05Z")}
 fn order()->Receipt{r(json!({"id":201,"exchangeId":"994","tournamentId":"cup","side":"no","action":"sell","priceLimit":0.055,"quantity":2,"quantityFilled":2,"open":false,"terminalReasonCode":null}),"2026-10-15T00:00:02Z","2026-10-15T00:00:03Z")}
 #[test]fn no_signed_quantity_does_not_flip_a_covered_sale_into_purchase(){let events=fills(&q(),"201","cup",&tape(),"2026-10-15T00:00:06Z").unwrap();assert_eq!(events.len(),2);assert_eq!(events[0]["quantity"],1);assert_eq!(events[0]["action"],"sell");assert_eq!(events[0]["side"],"NO");assert_eq!(events[0]["price"],"0.060000");}
 #[test]fn canonical_action_changes_and_missing_order_ids_are_not_adopted(){let mut response=r(json!({"orderId":201,"exchangeId":"994","side":"yes","action":"buy","quantity":2,"price":0.945}),"2026-10-15T00:00:01Z","2026-10-15T00:00:02Z");assert!(acknowledgment(&q(),&response,"2026-10-15T00:00:03Z").is_err());response.data=json!({"orderId":201,"exchangeId":"994","side":"no","action":"sell","quantity":1,"price":0.055});assert_eq!(acknowledgment(&q(),&response,"2026-10-15T00:00:03Z").unwrap()["quantity"],1);response.data["orderId"]=Value::Null;assert!(acknowledgment(&q(),&response,"2026-10-15T00:00:03Z").is_err());}
 #[test]fn partial_pages_duplicates_and_foreign_fills_are_rejected(){for key in ["pagination","totalQuantityFilled","orderId","duplicate"]{let mut v=tape();match key{"pagination"=>v.data["pagination"]["hasMore"]=json!(true),"totalQuantityFilled"=>v.data[key]=json!(-3),"orderId"=>v.data[key]=json!(999),_=>v.data["data"][1]["id"]=json!(301)}assert!(fills(&q(),"201","cup",&v,"2026-10-15T00:00:06Z").is_err());}}
 #[test]fn open_quantity_is_remainder_and_never_a_terminal_fill_count(){let mut o=order();o.data["open"]=json!(true);o.data["quantity"]=json!(0);o.data["quantityFilled"]=Value::Null;assert!(terminal(&q(),"201","cup",&o,&tape(),"2026-10-15T00:00:06Z").unwrap().is_none());o=order();assert_eq!(terminal(&q(),"201","cup",&o,&tape(),"2026-10-15T00:00:06Z").unwrap().unwrap()["status"],"filled");o.data["quantityFilled"]=json!(1);assert!(terminal(&q(),"201","cup",&o,&tape(),"2026-10-15T00:00:06Z").is_err());}
 #[test]fn terminal_releases_wait_for_an_independent_later_fill_observation(){let mut old=tape();old.request_started_at="2026-10-15T00:00:01Z".into();assert!(terminal(&q(),"201","cup",&order(),&old,"2026-10-15T00:00:06Z").is_err());assert!(fills(&q(),"201","cup",&tape(),"2026-10-15T00:00:04Z").is_err());}
}
