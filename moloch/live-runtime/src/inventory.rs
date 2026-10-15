//! Normalize an already obtained complete official positions receipt. No network.
//! Lot principal is an explicit opening basis, not official realized-account P&L.
use crate::feedback::{Result,digest};
use baal_transport_service::{Receipt,decimal_units,prices::official_tick_units};
use moloch_replay::{engine::Engine,money};
use serde_json::{Value,json};
use std::collections::{BTreeMap,BTreeSet};
fn identifier(value:&Value)->Result<String>{let id=value.as_str().map(str::to_owned).or_else(||value.as_u64().map(|n|n.to_string())).ok_or("ADOPTION_SCOPE")?;if id.is_empty()||id.starts_with('0')||!id.bytes().all(|b|b.is_ascii_digit()){return Err("ADOPTION_SCOPE".into())}Ok(id)}
fn instant(value:&Value)->Result<i64>{Ok(chrono::DateTime::parse_from_rfc3339(value.as_str().ok_or("ADOPTION_CLOCK")?)?.timestamp_nanos_opt().ok_or("ADOPTION_CLOCK_RANGE")?)}
pub fn adoption(receipt:&Receipt,engine:&Engine,tournament:&str,tournament_slug:&str,exchanges:&BTreeMap<String,String>,processed_at:&str)->Result<Value>{
 if !engine.is_live()||engine.report()?.get("inheritedInventory").is_some()||receipt.status!=200||receipt.endpoint!=format!("/tournaments/{tournament_slug}/portfolio/positions")||receipt.data.get("pagination").is_some(){return Err("ADOPTION_RECEIPT_OR_MODE".into())}
 let started=instant(&json!(receipt.request_started_at))?;let received=instant(&json!(receipt.received_at))?;let processed=instant(&json!(processed_at))?;
 if started>received||processed<received||processed-received>30_000_000_000{return Err("ADOPTION_CLOCK".into())}
 let report=engine.report()?;if report["unresolvedOrders"]!=0||report["reservedCash"]!="0.000000"{return Err("ADOPTION_UNRESOLVED_ORDERS".into())}
 let mut rows=Vec::new();let mut seen=BTreeSet::new();let mut seen_lots=BTreeSet::new();let mut opening=0i128;let mut markets=0u64;
 for row in receipt.data["positions"].as_array().ok_or("ADOPTION_POSITIONS")?{
  let exchange=identifier(&row["exchangeId"])?;let market=identifier(&row["marketId"])?;
  if exchanges.get(&exchange)!=Some(&market)||!seen.insert(exchange.clone()){return Err("ADOPTION_SCOPE_OR_DUPLICATE".into())}
  let signed=row["quantity"].as_i64().filter(|n|*n!=i64::MIN).ok_or("ADOPTION_QUANTITY")?;let side=if signed<0{"NO"}else{"YES"};let mut lots=Vec::new();let mut principal=0i128;let mut quantity=0u64;
  for lot in row["lots"].as_array().ok_or("ADOPTION_LOTS")?{
   let lot_id=identifier(&lot["lotId"])?;let n=lot["quantity"].as_u64().filter(|n|*n>0).ok_or("ADOPTION_LOT_QUANTITY")?;
   if !seen_lots.insert(lot_id.clone())||lot["side"]!=side||instant(&lot["openedAt"])? > received{return Err("ADOPTION_LOT_SIDE_ID_OR_CLOCK".into())}
   let price=official_tick_units(&lot["entryPrice"])?;quantity=quantity.checked_add(n).ok_or("ADOPTION_RANGE")?;principal=principal.checked_add(price.checked_mul(n as i128).ok_or("ADOPTION_RANGE")?).ok_or("ADOPTION_RANGE")?;
   lots.push(json!({"lotId":lot_id,"quantity":n,"side":side,"entryPrice":money(price),"openedAt":lot["openedAt"]}));
  }
  lots.sort_by(|a,b|instant(&a["openedAt"]).expect("validated timestamp").cmp(&instant(&b["openedAt"]).expect("validated timestamp")).then_with(||a["lotId"].as_str().cmp(&b["lotId"].as_str())));
  if quantity!=signed.unsigned_abs(){return Err("ADOPTION_QUANTITY_MISMATCH".into())}
  let reported=decimal_units(&row["costBasis"].to_string())?;let difference=reported.checked_sub(principal).ok_or("ADOPTION_RANGE")?;
  let classification=if difference==0{"exact"}else if difference.unsigned_abs()<=5000{"cent_rounding"}else{"reported_discrepancy"};
  let held=report["positions"].get(&exchange).is_some_and(|p|["YES","NO"].iter().any(|side|p[*side].as_array().is_some_and(|lots|!lots.is_empty())));
  if signed!=0&&!held{opening=opening.checked_add(principal).ok_or("ADOPTION_RANGE")?;markets+=1;}
  rows.push(json!({"exchangeId":exchange,"marketId":market,"quantity":signed,"lotPrincipal":money(principal),"reportedCostBasis":money(reported),"reportedMinusLotPrincipal":money(difference),"basisClassification":classification,"lots":lots,"metadata":{"title":row["title"],"reportedMarketValue":row["marketValue"],"reportedAverageCost":row["avgCost"],"settled":row["settled"]}}));
 }
 if markets==0{return Err("ADOPTION_NO_FOREIGN_INVENTORY".into())}
 let maximum=engine.policy()["maximumMarkets"].as_u64().ok_or("ADOPTION_MARKET_LIMIT")?.checked_add(markets).ok_or("ADOPTION_MARKET_LIMIT")?;
 Ok(json!({"type":"actual_position_adoption","source":"official_sig_api_v1","authority":"human_moloch_inherited_management_20261015","complete":true,"tournamentId":tournament,"receiptHash":digest(receipt)?,"requestStartedAt":receipt.request_started_at,"receivedAt":receipt.received_at,"processedAt":processed_at,"positions":rows,"openingCost":money(opening),"effectiveMaximumMarkets":maximum,"basisPolicy":"verified_open_lot_principal_fifo_not_official_realized_pnl"}))
}
