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
/// Bind an exact already-attempted order from an official GET, never a synthetic POST.
pub fn recovered_acknowledgment(q:&Value,actual:&str,tournament:&str,r:&Receipt,filled:&Receipt,processed_at:&str)->Result<Value>{
 let v=&r.data;let quantity=v["quantity"].as_u64().filter(|n|*n>0&&*n<=q["quantity"].as_u64().unwrap_or(0)).ok_or("RECOVERED_ORDER_QUANTITY")?;
 let side=if q["side"]=="NO"{"no"}else{"yes"};
 if r.endpoint!=format!("/orders/{actual}")||id(&v["id"])?!=actual||v["exchangeId"]!=q["exchangeId"]||v["tournamentId"]!=tournament||v["side"]!=side||v["action"]!=q["action"]||money(official_tick_units(&v["priceLimit"])?)!=q["price"]||!v["open"].is_boolean(){return Err("RECOVERED_ORDER_MISMATCH".into())}
 let created=chrono::DateTime::parse_from_rfc3339(v["createdAt"].as_str().ok_or("RECOVERED_ORDER_CLOCK")?)?.timestamp_millis();let expires=chrono::DateTime::parse_from_rfc3339(v["expirationDate"].as_str().ok_or("RECOVERED_ORDER_EXPIRY")?)?.timestamp_millis();
 if created<q["createdAt"].as_i64().ok_or("RECOVERED_INTENT_CLOCK")?||Some(expires)!=q["expiresAt"].as_i64(){return Err("RECOVERED_ORDER_CLOCK".into())}
 if fills(q,actual,tournament,filled,processed_at)?.iter().map(|f|f["quantity"].as_u64().unwrap()).sum::<u64>()>quantity{return Err("RECOVERED_ORDER_FILL_TOTAL".into())}
 let mut e=base("actual_order_ack",q,actual,r,processed_at)?;e["quantity"]=json!(quantity);e["price"]=q["price"].clone();e["recoveredFromKnownOrder"]=json!(true);e["fillReceiptHash"]=json!(digest(filled)?);Ok(e)
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
  let mut e=base("actual_fill",q,actual,r,processed_at)?;e["fillId"]=json!(fill_id);e["quantity"]=json!(n);e["price"]=json!(money(official_tick_units(&row["price"])?));e["fee"]=json!("0.000000");e["executedAt"]=row["filledAt"].clone();
  let executed=chrono::DateTime::parse_from_rfc3339(row["filledAt"].as_str().ok_or("ACCOUNT_FILL_CLOCK")?)?;events.push((executed,e));
 }
 if sum!=total.checked_abs().ok_or("ACCOUNT_FILL_TOTAL")? as u64||sum>q["quantity"].as_u64().ok_or("INTENT_QUANTITY")?{return Err("ACCOUNT_FILL_TOTAL_MISMATCH".into())}
 // Official order tapes are newest first; FIFO requires oldest first. Preserve
 // the original receipt and its hash. This orders one complete tape only, not
 // late observations across separately reconciled orders or historical events.
 events.sort_by(|a,b|a.0.cmp(&b.0));
 for pair in events.windows(2){if pair[0].0==pair[1].0&&pair[0].1["price"]!=pair[1].1["price"]{return Err("ACCOUNT_FILL_ORDER_AMBIGUOUS".into())}}
 Ok(events.into_iter().map(|(_,event)|event).collect())
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
 #[test]fn known_order_recovery_preserves_original_get_and_requires_exact_intent(){
  let mut q=q();q["createdAt"]=json!(1760000000000i64);q["expiresAt"]=json!(1760000600000i64);
  let mut order=order();order.endpoint="/orders/201".into();order.data["createdAt"]=json!(chrono::DateTime::from_timestamp_millis(1760000001000).unwrap().to_rfc3339());order.data["expirationDate"]=json!(chrono::DateTime::from_timestamp_millis(1760000600000).unwrap().to_rfc3339());
  let e=recovered_acknowledgment(&q,"201","cup",&order,&tape(),"2026-10-15T00:00:06Z").unwrap();assert_eq!(e["recoveredFromKnownOrder"],true);assert_eq!(e["receiptHash"],digest(&order).unwrap());
  let mut changed=order.clone();changed.data["priceLimit"]=json!(0.06);assert!(recovered_acknowledgment(&q,"201","cup",&changed,&tape(),"2026-10-15T00:00:06Z").is_err());assert!(recovered_acknowledgment(&q,"202","cup",&order,&tape(),"2026-10-15T00:00:06Z").is_err());
 }
 fn q()->Value{json!({"id":1,"marketId":"101","exchangeId":"994","side":"NO","action":"sell","quantity":2,"price":"0.055000"})}
 fn r(data:Value,start:&str,end:&str)->Receipt{Receipt{endpoint:"/fixture".into(),status:200,request_started_at:start.into(),received_at:end.into(),data}}
 fn tape()->Receipt{r(json!({"orderId":201,"exchangeId":"994","tournamentId":"cup","coverage":{"complete":true,"projectedThroughSequence":100},"pagination":{"hasMore":false,"nextCursor":null},"totalQuantityFilled":-2,"data":[{"id":301,"price":0.06000000000000005,"quantity":-1,"side":"no","filledAt":"2026-10-15T00:00:01Z"},{"id":302,"price":0.055,"quantity":-1,"side":"no","filledAt":"2026-10-15T00:00:02Z"}]}),"2026-10-15T00:00:04Z","2026-10-15T00:00:05Z")}
 fn order()->Receipt{r(json!({"id":201,"exchangeId":"994","tournamentId":"cup","side":"no","action":"sell","priceLimit":0.055,"quantity":2,"quantityFilled":2,"open":false,"terminalReasonCode":null}),"2026-10-15T00:00:02Z","2026-10-15T00:00:03Z")}
 #[test]fn no_signed_quantity_does_not_flip_a_covered_sale_into_purchase(){let events=fills(&q(),"201","cup",&tape(),"2026-10-15T00:00:06Z").unwrap();assert_eq!(events.len(),2);assert_eq!(events[0]["quantity"],1);assert_eq!(events[0]["action"],"sell");assert_eq!(events[0]["side"],"NO");assert_eq!(events[0]["price"],"0.060000");}
 #[test]fn canonical_action_changes_and_missing_order_ids_are_not_adopted(){let mut response=r(json!({"orderId":201,"exchangeId":"994","side":"yes","action":"buy","quantity":2,"price":0.945}),"2026-10-15T00:00:01Z","2026-10-15T00:00:02Z");assert!(acknowledgment(&q(),&response,"2026-10-15T00:00:03Z").is_err());response.data=json!({"orderId":201,"exchangeId":"994","side":"no","action":"sell","quantity":1,"price":0.055});assert_eq!(acknowledgment(&q(),&response,"2026-10-15T00:00:03Z").unwrap()["quantity"],1);response.data["orderId"]=Value::Null;assert!(acknowledgment(&q(),&response,"2026-10-15T00:00:03Z").is_err());}
 #[test]fn partial_pages_duplicates_and_foreign_fills_are_rejected(){for key in ["pagination","totalQuantityFilled","orderId","duplicate"]{let mut v=tape();match key{"pagination"=>v.data["pagination"]["hasMore"]=json!(true),"totalQuantityFilled"=>v.data[key]=json!(-3),"orderId"=>v.data[key]=json!(999),_=>v.data["data"][1]["id"]=json!(301)}assert!(fills(&q(),"201","cup",&v,"2026-10-15T00:00:06Z").is_err());}}
 #[test]fn open_quantity_is_remainder_and_never_a_terminal_fill_count(){let mut o=order();o.data["open"]=json!(true);o.data["quantity"]=json!(0);o.data["quantityFilled"]=Value::Null;assert!(terminal(&q(),"201","cup",&o,&tape(),"2026-10-15T00:00:06Z").unwrap().is_none());o=order();assert_eq!(terminal(&q(),"201","cup",&o,&tape(),"2026-10-15T00:00:06Z").unwrap().unwrap()["status"],"filled");o.data["quantityFilled"]=json!(1);assert!(terminal(&q(),"201","cup",&o,&tape(),"2026-10-15T00:00:06Z").is_err());}
 #[test]fn terminal_releases_wait_for_an_independent_later_fill_observation(){let mut old=tape();old.request_started_at="2026-10-15T00:00:01Z".into();assert!(terminal(&q(),"201","cup",&order(),&old,"2026-10-15T00:00:06Z").is_err());assert!(fills(&q(),"201","cup",&tape(),"2026-10-15T00:00:04Z").is_err());}
 fn varied_buy()->(Value,Receipt){
  let mut quote=q();quote["side"]=json!("YES");quote["action"]=json!("buy");quote["price"]=json!("0.400000");
  let mut receipt=tape();receipt.data["totalQuantityFilled"]=json!(2);receipt.data["data"]=json!([
   {"id":302,"price":0.40,"quantity":1,"side":"yes","filledAt":"2026-10-15T00:00:02.0000002Z"},
   {"id":301,"price":0.20,"quantity":1,"side":"yes","filledAt":"2026-10-15T00:00:02.0000001Z"}]);(quote,receipt)
 }
 #[test]fn newest_first_complete_tape_preserves_receipt_and_actual_fifo_redemption(){
  use moloch_replay::{Wallet,Reservation,Side,amount};
  let(quote,receipt)=varied_buy();let original=digest(&receipt).unwrap();let events=fills(&quote,"201","cup",&receipt,"2026-10-15T00:00:06Z").unwrap();
  assert_eq!(digest(&receipt).unwrap(),original);assert_eq!(receipt.data["data"][0]["id"],302);assert_eq!(events[0]["fillId"],"301");assert_eq!(events[1]["fillId"],"302");assert_eq!(events[0]["receiptHash"],original);
  let mut wallet=Wallet::new(amount("100").unwrap(),amount("100").unwrap(),0,0).unwrap();
  wallet.reserve("yes",Reservation{exchange:"994".into(),side:Side::Yes,quantity:2,limit:amount("0.40").unwrap()}).unwrap();
  for event in events{wallet.buy_fill_exact("yes",1,amount(event["price"].as_str().unwrap()).unwrap(),0).unwrap();}
  wallet.reserve("no",Reservation{exchange:"994".into(),side:Side::No,quantity:1,limit:amount("0.50").unwrap()}).unwrap();wallet.buy_fill_exact("no",1,amount("0.50").unwrap(),0).unwrap();
  assert_eq!(money(wallet.cash),"99.900000");assert_eq!(money(wallet.realized),"0.300000");assert_eq!(money(wallet.owned_cost()),"0.400000");assert_eq!(wallet.quantity("994",Side::Yes),1);assert_eq!(wallet.quantity("994",Side::No),0);wallet.audit().unwrap();
 }
 #[test]fn unequal_price_timestamp_ties_fail_without_inventing_trade_id_order(){let(quote,mut receipt)=varied_buy();receipt.data["data"][1]["filledAt"]=receipt.data["data"][0]["filledAt"].clone();assert!(fills(&quote,"201","cup",&receipt,"2026-10-15T00:00:06Z").is_err());receipt.data["data"][1]["price"]=json!(0.40);assert_eq!(fills(&quote,"201","cup",&receipt,"2026-10-15T00:00:06Z").unwrap().len(),2);}
 #[test]fn sorting_one_tape_does_not_prove_cross_order_late_observation_fifo(){
  let(mut quote,mut receipt)=varied_buy();quote["quantity"]=json!(1);receipt.data["totalQuantityFilled"]=json!(1);
  let older=receipt.data["data"][1].clone();receipt.data["data"]=json!([receipt.data["data"][0].clone()]);
  let mut admitted=fills(&quote,"201","cup",&receipt,"2026-10-15T00:00:06Z").unwrap();
  quote["id"]=json!(2);receipt.data["orderId"]=json!(202);receipt.data["data"]=json!([older]);
  admitted.extend(fills(&quote,"202","cup",&receipt,"2026-10-15T00:00:07Z").unwrap());
  let clocks=admitted.iter().map(|e|chrono::DateTime::parse_from_rfc3339(e["executedAt"].as_str().unwrap()).unwrap()).collect::<Vec<_>>();
  assert!(clocks[0]>clocks[1],"a future global staging barrier is still required");
 }
}
