//! Actual-account feedback. Public prints and depth cannot fill live orders.
use super::*;
impl Engine {
 pub fn new_live(policy:&Value)->Result<Self>{let mut e=Self::new(policy)?;if e.policy.conditional{return Err("LIVE_CONDITIONAL_FILL_MODEL")};e.live_mode=true;Ok(e)}
 pub fn is_live(&self)->bool{self.live_mode}
 pub fn live_order_bindings(&self)->&BTreeMap<u64,String>{&self.live_orders}
 pub(super) fn apply_live(&mut self,e:&Value,_at:i64)->Result<()> {
  if !self.live_mode {return Err("ACTUAL_EVENT_SOURCE")}
  if e["type"]=="live_entry_window" {return self.apply_entry_window(e,_at)}
  if e["type"]=="actual_position_adoption" {return self.adopt_positions(e,_at)}
  if e["type"]=="unsubmitted_intent_retired" {
   if e["source"]!="moloch_live_dispatcher"||e["confirmedNetworkAttempts"].as_u64()!=Some(0)||e["receiptHash"].as_str().is_none_or(|s|s.len()!=64||!s.bytes().all(|b|b.is_ascii_hexdigit())){return Err("UNSUBMITTED_INTENT_PROOF")}
   let qid=e["orderId"].as_u64().ok_or("ACTUAL_INTENT_ID")?;let index=self.orders.iter().position(|q|q.id==qid).ok_or("ACTUAL_UNOWNED_ORDER")?;
   if self.live_orders.contains_key(&qid)||self.orders[index].filled!=0||self.orders[index].retired{return Err("DISPATCHED_INTENT_CANNOT_BE_ABANDONED")}
   if self.orders[index].buy{self.wallet.release(&qid.to_string())?}self.orders[index].retired=true;self.count("unsubmittedIntentsRetired");return Ok(())
  }
  if e["source"]!="official_sig_api_v1"{return Err("ACTUAL_EVENT_SOURCE")}
  let receipt=e["receiptHash"].as_str().filter(|s|s.len()==64&&s.bytes().all(|b|b.is_ascii_hexdigit())).ok_or("ACTUAL_RECEIPT_HASH")?;
  let qid=e["orderId"].as_u64().ok_or("ACTUAL_INTENT_ID")?;
  let actual=id(&e["actualOrderId"])?;
  let index=self.orders.iter().position(|q|q.id==qid).ok_or("ACTUAL_UNOWNED_ORDER")?;
  if e["type"]=="actual_order_ack" {
   let q=&self.orders[index];
   let admitted=e["quantity"].as_u64().filter(|n|*n>0&&*n<=q.quantity).ok_or("ACTUAL_ACK_QUANTITY")?;
   if e["exchangeId"]!=q.exchange||e["marketId"]!=q.market||e["price"]!=money(q.limit)||e["side"]!=label(q.side)||e["action"]!=if q.buy{"buy"}else{"sell"}{return Err("ACTUAL_ACK_INTENT_MISMATCH")}
   if let Some(old)=self.live_orders.get(&qid){if old!=&actual||admitted!=q.quantity{return Err("ACTUAL_ORDER_BINDING_CHANGED")};return Ok(())}
   if self.orders[index].retired||self.live_orders.values().any(|v|v==&actual){return Err("ACTUAL_ORDER_ALREADY_BOUND")}
   if admitted<self.orders[index].quantity {
    let q=&self.orders[index];if q.filled!=0{return Err("ACTUAL_RESIZE_AFTER_FILL")}
    if q.buy{let r=Reservation{exchange:q.exchange.clone(),side:q.side,quantity:admitted,limit:q.limit};self.wallet.release(&qid.to_string())?;self.wallet.reserve(&qid.to_string(),r)?;}
    self.orders[index].quantity=admitted;self.count("actualOrderResizes");
   }
   self.live_orders.insert(qid,actual);self.count("actualOrderAcknowledgments");return Ok(())
  }
  if self.live_orders.get(&qid)!=Some(&actual){return Err("ACTUAL_UNOWNED_ORDER")}
  if e["type"]=="actual_order_terminal" {
   let q=&self.orders[index];let status=e["status"].as_str().ok_or("ACTUAL_TERMINAL_STATUS")?;
   if !["canceled","expired","filled"].contains(&status)||e["fillsComplete"]!=true||e["filledQuantity"].as_u64()!=Some(q.filled)||(status=="filled"&&q.filled!=q.quantity){return Err("ACTUAL_TERMINAL_FILL_RECONCILIATION_REQUIRED")}
   if !q.retired {if q.buy{self.wallet.release(&qid.to_string())?};self.orders[index].retired=true;self.count("actualOrderTerminals")};return Ok(())
  }
  let fill_id=id(&e["fillId"])?;let n=e["quantity"].as_u64().filter(|n|*n>0).ok_or("ACTUAL_FILL_QUANTITY")?;
  let executed=ns(&e["executedAt"])?;let p=price(dec(&e["price"])?)?;let charge=dec(&e["fee"])?;
  let q=&self.orders[index];
  if e["exchangeId"]!=q.exchange||e["marketId"]!=q.market||e["side"]!=label(q.side)||e["action"]!=if q.buy{"buy"}else{"sell"}{return Err("ACTUAL_FILL_SCOPE")}
  let facts=hash(&json!({"actualOrderId":actual,"orderId":qid,"marketId":q.market,"exchangeId":q.exchange,"side":label(q.side),"action":if q.buy{"buy"}else{"sell"},"quantity":n,"price":money(p),"fee":money(charge),"executedAtNs":executed}));
  if let Some(prior)=self.live_fill_ids.get(&fill_id){if prior!=&facts{return Err("ACTUAL_FILL_ID_CONFLICT")};self.count("duplicateActualFills");return Ok(())}
  if q.retired||n>q.remaining()||executed<q.created*1_000_000||executed>ns(&e["receivedAt"])?||charge<0||(if q.buy{p>q.limit}else{p<q.limit}){return Err("ACTUAL_FILL_BOUNDS")}
  let inherited_close=(q.exchange.clone(),if q.buy{q.side.other()}else{q.side},n,if q.buy{SCALE-p}else{p});
  if q.buy {self.wallet.buy_fill_exact(&qid.to_string(),n,p,charge)?;if let Some(lot)=self.wallet.lots.get_mut(&(q.exchange.clone(),q.side)).and_then(|lots|lots.back_mut()){lot.executed_at=Some(e["executedAt"].clone())}}
  else {self.wallet.sell_fill_exact(&q.exchange,q.side,n,p,charge)?}
  self.fills.push(json!({"orderId":qid,"actualOrderId":actual,"sourceId":fill_id,"marketId":q.market,"exchangeId":q.exchange,"side":label(q.side),"action":if q.buy{"buy"}else{"sell"},"quantity":n,"price":money(p),"fee":money(charge),"at":e.get("processedAt").unwrap_or(&e["receivedAt"]),"executedAt":e["executedAt"],"counterfactual":false,"receiptHash":receipt}));
  self.live_fill_ids.insert(fill_id,facts);let q=&mut self.orders[index];q.filled+=n;if q.remaining()==0{q.retired=true};self.count("actualFills");self.consume_inherited(&inherited_close.0,inherited_close.1,inherited_close.2,inherited_close.3)?;Ok(())
 }
 /// A local, durable mandate transition. The runtime binds human provenance and
 /// account/bank identity; no API payload or source label alone grants authority.
 fn apply_entry_window(&mut self,e:&Value,at:i64)->Result<()> {
  const DAY:i64=86_400_000;
  if e["source"]!="moloch_live_dispatcher"{return Err("LIVE_ENTRY_WINDOW_SOURCE")}
  let processed=ns(&e["processedAt"])?;
  if processed/1_000_000!=at{return Err("LIVE_ENTRY_WINDOW_CLOCK")}
  let mandate=e["mandate"].as_object().ok_or("LIVE_ENTRY_WINDOW_MANDATE")?;
  let keys=["authorizationId","approvedAt","anchorMs","campaignEndMs","intervalMs","maximumCommitted","maximumMarketCost","maximumQuantity"];
  if mandate.len()!=keys.len()||keys.iter().any(|key|!mandate.contains_key(*key)){return Err("LIVE_ENTRY_WINDOW_MANDATE_KEYS")}
  let authorization=mandate["authorizationId"].as_str().filter(|s|!s.trim().is_empty()&&s.len()<=256&&!s.chars().any(char::is_control)).ok_or("LIVE_ENTRY_WINDOW_AUTHORIZATION")?;
  let _=authorization;
  if ns(&mandate["approvedAt"])? > processed{return Err("LIVE_ENTRY_WINDOW_APPROVED_IN_FUTURE")}
  let anchor=mandate["anchorMs"].as_i64().filter(|n|*n>=0).ok_or("LIVE_ENTRY_WINDOW_ANCHOR")?;
  let campaign=mandate["campaignEndMs"].as_i64().filter(|n|*n>=0).ok_or("LIVE_ENTRY_WINDOW_CAMPAIGN")?;
  let duration=campaign.checked_sub(anchor).filter(|n|*n>0&&*n<=31*DAY).ok_or("LIVE_ENTRY_WINDOW_CAMPAIGN")?;
  let _=duration;
  if mandate["intervalMs"].as_i64()!=Some(DAY){return Err("LIVE_ENTRY_WINDOW_INTERVAL")}
  if mandate["maximumCommitted"]!=self.policy.raw["maximumCommitted"]||mandate["maximumMarketCost"]!=self.policy.raw["maximumMarketCost"]||mandate["maximumQuantity"]!=self.policy.raw["quantity"]{return Err("LIVE_ENTRY_WINDOW_CAPS_CHANGED")}
  let previous=self.policy.stop.ok_or("LIVE_ENTRY_WINDOW_PREVIOUS_STOP")?;
  if ns(&e["previousEntryStopAt"])?!=previous.checked_mul(1_000_000).ok_or("LIVE_ENTRY_WINDOW_CLOCK")?{return Err("LIVE_ENTRY_WINDOW_PREVIOUS_STOP")}
  if let Some(existing)=&self.daily_entry_windows {
   if existing!=&e["mandate"]{return Err("LIVE_ENTRY_WINDOW_MANDATE_CHANGED")}
  }else if anchor!=previous{return Err("LIVE_ENTRY_WINDOW_ANCHOR")}
  if at<anchor||at>=campaign{return Err("LIVE_ENTRY_WINDOW_OUTSIDE_CAMPAIGN")}
  let periods=(at-anchor)/DAY+1;
  let expected=anchor.checked_add(periods.checked_mul(DAY).ok_or("LIVE_ENTRY_WINDOW_CLOCK")?).ok_or("LIVE_ENTRY_WINDOW_CLOCK")?.min(campaign);
  if expected<=previous||ns(&e["entryStopAt"])?!=expected.checked_mul(1_000_000).ok_or("LIVE_ENTRY_WINDOW_CLOCK")?{return Err("LIVE_ENTRY_WINDOW_NOT_CURRENT_END")}
  let mut raw=self.policy.raw.clone();raw["entryStopAt"]=e["entryStopAt"].clone();
  // Do not route through general update_policy: only this one field may change.
  let policy=Policy::new(&raw)?;
  self.policy=policy;self.daily_entry_windows=Some(e["mandate"].clone());Ok(())
 }
 /// Constant attribution principal, independent of later sales and account cash.
 pub fn opening_inventory_cost(&self)->i128{self.inherited.as_ref().map_or(0,|i|i.opening_cost)}
 fn consume_inherited(&mut self,exchange:&str,side:Side,mut quantity:u64,proceeds_per_share:i128)->Result<()>{
  let Some(inherited)=self.inherited.as_mut() else{return Ok(())};
  let Some(lots)=inherited.remaining.get_mut(&(exchange.into(),side)) else{return Ok(())};
  while quantity>0&&!lots.is_empty(){let lot=lots.front_mut().ok_or("ADOPTION_LOTS")?;let n=quantity.min(lot.quantity);inherited.realized_gross=inherited.realized_gross.checked_add((proceeds_per_share-lot.price).checked_mul(n as i128).ok_or("ADOPTION_RANGE")?).ok_or("ADOPTION_RANGE")?;quantity-=n;lot.quantity-=n;if lot.quantity==0{lots.pop_front();}}
  Ok(())
 }
 fn adopt_positions(&mut self,e:&Value,at:i64)->Result<()>{
  if self.inherited.is_some(){return Err("ADOPTION_ALREADY_RECORDED")}
  if e["source"]!="official_sig_api_v1"||e["authority"]!="human_moloch_inherited_management_20261015"||e["complete"]!=true{return Err("ADOPTION_AUTHORITY_OR_COVERAGE")}
  let receipt=e["receiptHash"].as_str().filter(|s|s.len()==64&&s.bytes().all(|b|b.is_ascii_hexdigit())).ok_or("ACTUAL_RECEIPT_HASH")?;
  let received=ns(&e["receivedAt"])?;let started=ns(&e["requestStartedAt"])?;
  if started>received||at.checked_mul(1_000_000).ok_or("ADOPTION_CLOCK")?-received>30_000_000_000{return Err("ADOPTION_CLOCK")}
  let tournament=string(&e["tournamentId"])?;if tournament.is_empty(){return Err("ADOPTION_SCOPE")}
  if self.orders.iter().any(|q|!q.retired)||self.wallet.reserved()?!=0{return Err("ADOPTION_UNRESOLVED_ORDERS")}
  let rows=e["positions"].as_array().filter(|rows|!rows.is_empty()&&rows.len()<=4096).ok_or("ADOPTION_POSITIONS")?;
  let mut seen=BTreeSet::new();let mut seen_lots=BTreeSet::new();let mut adopted=BTreeMap::new();let mut principal=0i128;let mut count=0usize;
  for row in rows {
   let exchange=id(&row["exchangeId"])?;let market=id(&row["marketId"])?;
   if !seen.insert(exchange.clone()){return Err("ADOPTION_DUPLICATE_POSITION")}
   if self.books.get(&exchange).is_some_and(|b|b.market!=market||b.tournament!=tournament)||self.orders.iter().any(|q|q.exchange==exchange&&(q.market!=market||q.tournament!=tournament)){return Err("ADOPTION_SCOPE")}
   let signed=row["quantity"].as_i64().filter(|n|*n!=i64::MIN&&n.unsigned_abs()<=9_007_199_254_740_991).ok_or("ADOPTION_QUANTITY")?;
   let side=if signed<0{Side::No}else{Side::Yes};let quantity=signed.unsigned_abs();
   let lots=row["lots"].as_array().ok_or("ADOPTION_LOTS")?;let mut total=0u64;let mut cost=0i128;let mut last_opened=None;let mut normalized=std::collections::VecDeque::new();
   if lots.len()>65536{return Err("ADOPTION_LOTS_LIMIT")}
   for lot in lots {
    let lot_id=id(&lot["lotId"])?;if !seen_lots.insert(lot_id){return Err("ADOPTION_DUPLICATE_LOT")}
    let n=lot["quantity"].as_u64().filter(|n|*n>0&&*n<=9_007_199_254_740_991).ok_or("ADOPTION_LOT_QUANTITY")?;
    if lot["side"]!=label(side){return Err("ADOPTION_LOT_SIDE")}
    let entry=dec(&lot["entryPrice"])?;if !(0..=SCALE).contains(&entry){return Err("ADOPTION_LOT_PRICE")}
    let opened=ns(&lot["openedAt"])?;if opened > received||last_opened.is_some_and(|old|opened<old){return Err("ADOPTION_LOT_CLOCK")}last_opened=Some(opened);
    total=total.checked_add(n).ok_or("ADOPTION_RANGE")?;cost=cost.checked_add(entry.checked_mul(n as i128).ok_or("ADOPTION_RANGE")?).ok_or("ADOPTION_RANGE")?;
    normalized.push_back(crate::Lot{quantity:n,price:entry,executed_at:None});
   }
   if total!=quantity||cost!=dec(&row["lotPrincipal"])?{return Err("ADOPTION_QUANTITY_OR_COST_MISMATCH")}
   let reported=dec(&row["reportedCostBasis"])?;let difference=reported.checked_sub(cost).ok_or("ADOPTION_RANGE")?;
   let classification=if difference==0{"exact"}else if difference.unsigned_abs()<=5000{"cent_rounding"}else{"reported_discrepancy"};
   if reported<0||row["reportedMinusLotPrincipal"]!=money(difference)||row["basisClassification"]!=classification{return Err("ADOPTION_REPORTED_BASIS_METADATA")}
   let yes=self.wallet.quantity(&exchange,Side::Yes);let no=self.wallet.quantity(&exchange,Side::No);
   if yes>0||no>0 {if (side==Side::Yes&&(yes!=quantity||no!=0))||(side==Side::No&&(no!=quantity||yes!=0)){return Err("ADOPTION_OWNED_POSITION_MISMATCH")}continue}
   if quantity==0{continue}
   principal=principal.checked_add(cost).ok_or("ADOPTION_RANGE")?;count+=1;adopted.insert((exchange,side),normalized);
  }
  for ((exchange,_),lots) in &self.wallet.lots{if !lots.is_empty()&&!seen.contains(exchange){return Err("ADOPTION_OWNED_POSITION_MISSING")}}
  if adopted.is_empty()||principal!=dec(&e["openingCost"])?{return Err("ADOPTION_OPENING_COST")}
  let maximum=self.policy.markets.checked_add(count).ok_or("ADOPTION_RANGE")?;
  if maximum>4096||e["effectiveMaximumMarkets"].as_u64()!=Some(maximum as u64){return Err("ADOPTION_MARKET_LIMIT")}
  let fresh_market_limit=self.policy.markets;let mut policy=self.policy.raw.clone();policy["maximumMarkets"]=json!(maximum);self.update_policy(&policy)?;
  self.wallet.cash=self.wallet.cash.checked_sub(principal).ok_or("ADOPTION_RANGE")?;
  for (key,lots) in &adopted{self.wallet.lots.insert(key.clone(),lots.clone());}
  self.inherited=Some(InheritedInventory{receipt_hash:receipt.into(),adopted_at:e.get("processedAt").unwrap_or(&e["receivedAt"]).clone(),opening_cost:principal,fresh_market_limit,source_positions:e["positions"].clone(),remaining:adopted,realized_gross:0});
  self.wallet.audit()?;self.count("actualPositionAdoptions");Ok(())
 }

}
