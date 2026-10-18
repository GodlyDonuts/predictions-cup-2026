//! Actual-account feedback. Public prints and depth cannot fill live orders.
use super::*;
impl Engine {
 pub fn managed_inventory_planning_record(&self)->Option<&Value>{self.managed_inventory_planning.as_ref()}
 pub fn managed_inventory_planning_excludes(&self,ex:&str)->bool{self.managed_inventory_planning.as_ref().is_some_and(|r|r["managedExchangeIds"].as_array().is_some_and(|a|a.iter().any(|v|v==ex)))}
 pub(super) fn apply_managed_inventory_planning(&mut self,e:&Value,at:i64)->Result<()>{
  if !self.live_mode||e["source"]!="moloch_live_dispatcher"||ms(&e["processedAt"])?!=at||ms(&e["receivedAt"])? >at{return Err("MANAGED_INVENTORY_SOURCE")}
  for key in ["controlSha256","approvedReviewSetSha256","generationSha256"]{if e[key].as_str().is_none_or(|s|s.len()!=64||!s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))){return Err("MANAGED_INVENTORY_PROOF")}}
  let scope=&e["bankScope"];if scope.as_object().is_none_or(|o|o.len()!=3)||scope["runId"].as_str().is_none_or(str::is_empty)||scope["tournamentId"].as_str().is_none_or(str::is_empty)||scope["bankManifestHash"].as_str().is_none_or(|s|s.len()!=64){return Err("MANAGED_INVENTORY_SCOPE")}
  let ids=e["managedExchangeIds"].as_array().filter(|a|a.len()<=256).ok_or("MANAGED_INVENTORY_IDS")?;let mut last=None;
  for raw in ids{let ex=id(raw)?;if raw.as_str()!=Some(ex.as_str())||last.as_ref().is_some_and(|old:&String|old>=&ex){return Err("MANAGED_INVENTORY_IDS")};last=Some(ex);}
  let previous=self.managed_inventory_planning.as_ref().map_or(Value::Null,|r|r["sha256"].clone());if e.get("expectedPreviousModeSha256")!=Some(&previous){return Err("MANAGED_INVENTORY_PREVIOUS")}
  if self.managed_inventory_planning.as_ref().is_some_and(|r|r["bankScope"]!=*scope){return Err("MANAGED_INVENTORY_SCOPE_CHANGED")}
  let mut record=json!({"version":1,"bankScope":scope,"managedExchangeIds":ids,"controlSha256":e["controlSha256"],"approvedReviewSetSha256":e["approvedReviewSetSha256"],"generationSha256":e["generationSha256"],"installedAtMs":at,"previousSha256":previous});record["sha256"]=json!(hash(&record));self.managed_inventory_planning=Some(record);Ok(())
 }
 /// Replay-derived optional policy. A source/hash string is audit provenance,
 /// not human authority: the sole runtime authenticates the pinned Control.
 pub fn profitable_depth_mode_record(&self)->Option<&Value>{self.profitable_depth_mode.as_ref()}
 pub fn profitable_depth_enabled(&self)->bool{self.profitable_depth_mode.as_ref().is_some_and(|r|r["enabled"]==true)}
 pub(super) fn apply_profitable_depth_mode(&mut self,e:&Value,at:i64)->Result<()>{
  let digest=|v:&Value|v.as_str().is_some_and(|s|s.len()==64&&s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b)));
  if !self.live_mode||e["source"]!="moloch_live_dispatcher"{return Err("PROFITABLE_DEPTH_SOURCE")}
  if ms(&e["processedAt"])?!=at||ms(&e["receivedAt"])? >at{return Err("PROFITABLE_DEPTH_CLOCK")}
  let scope=&e["bankScope"];
  if scope.as_object().is_none_or(|m|m.len()!=3)||scope["runId"].as_str().is_none_or(|s|s.is_empty()||s.len()>128)||scope["tournamentId"].as_str().is_none_or(|s|s.is_empty())||!digest(&scope["bankManifestHash"]){return Err("PROFITABLE_DEPTH_SCOPE")}
  if !digest(&e["controlSha256"])||e["expectedPolicySha256"]!=hash(&self.policy.raw){return Err("PROFITABLE_DEPTH_POLICY_BINDING")}
  let enabled=e["enabled"].as_bool().ok_or("PROFITABLE_DEPTH_ENABLED")?;
  if e["depthBps"].as_u64()!=Some(5000)||self.policy.raw["inventoryExitDepthBps"].as_u64()!=Some(5000){return Err("PROFITABLE_DEPTH_FRACTION")}
  let previous=self.profitable_depth_mode.as_ref().map_or(Value::Null,|r|r["sha256"].clone());
  if e.get("expectedPreviousModeSha256")!=Some(&previous){return Err("PROFITABLE_DEPTH_PREVIOUS")}
  if let Some(old)=&self.profitable_depth_mode{if old["bankScope"]!=*scope{return Err("PROFITABLE_DEPTH_SCOPE_CHANGED")};if old["enabled"]==enabled{return Ok(())}}
  let revision=self.profitable_depth_mode.as_ref().map_or(Some(1),|r|r["revision"].as_u64().and_then(|n|n.checked_add(1))).ok_or("PROFITABLE_DEPTH_REVISION")?;
  let mut record=json!({"version":1,"revision":revision,"bankScope":scope,"enabled":enabled,"depthBps":5000,"controlSha256":e["controlSha256"],"policySha256":e["expectedPolicySha256"],"installedAtMs":at,"previousSha256":previous});record["sha256"]=json!(hash(&record));self.profitable_depth_mode=Some(record);Ok(())
 }
 /// Own-side executable levels in descending price order. Count deeper shares
 /// only at their actual marginal price, never at the top price.
 pub(super) fn profitable_depth_target(&self,b:&Book,side:Side,maximum:u64,worst:i128)->Result<Option<(u64,i128)>>{
  let minimum=self.policy.raw.get("inventoryMinimumGain").map(dec).transpose()?.unwrap_or(0);
  let levels=if side==Side::Yes{&b.bids}else{&b.asks};
  let mut total=0u64;
  for &(yes_price,quantity) in levels{
   let price=if side==Side::Yes{yes_price}else{SCALE-yes_price};
   if price-worst-self.policy.fee(worst,1)?-self.policy.fee(price,1)?<minimum{break}
   total=total.checked_add(quantity).ok_or("PROFITABLE_DEPTH_RANGE")?;
  }
  let quantity=maximum.min((u128::from(total)*5000/10000) as u64);
  if quantity==0{return Ok(None)}
  let mut covered=0u64;
  for &(yes_price,size) in levels{
   let limit=if side==Side::Yes{yes_price}else{SCALE-yes_price};
   covered=covered.checked_add(size).ok_or("PROFITABLE_DEPTH_RANGE")?;
   if covered>=quantity{
    if limit-worst-self.policy.fee(worst,1)?-self.policy.fee(limit,1)?<minimum{return Err("PROFITABLE_DEPTH_GAIN")}
    return Ok(Some((quantity,limit)))
   }
  }Err("PROFITABLE_DEPTH_COVERAGE")
 }

 pub fn new_live(policy:&Value)->Result<Self>{let mut e=Self::new(policy)?;if e.policy.conditional{return Err("LIVE_CONDITIONAL_FILL_MODEL")};e.live_mode=true;Ok(e)}
 pub fn is_live(&self)->bool{self.live_mode}
 pub fn live_order_bindings(&self)->&BTreeMap<u64,String>{&self.live_orders}
 pub(super) fn apply_live(&mut self,e:&Value,_at:i64)->Result<()> {
  if !self.live_mode {return Err("ACTUAL_EVENT_SOURCE")}
  if e["type"].as_str().is_some_and(|s|s.starts_with("actual_collateral_")){return self.apply_collateral(e,_at)}
  if e["type"]=="live_entry_window" {return self.apply_entry_window(e,_at)}
  if e["type"]=="actual_position_adoption" {return self.adopt_positions(e,_at)}
  if e["type"]=="expired_attempt_exposure_reconciled_v1" {
   // Account-level recovery is distinct from an ACK, terminal venue order, or
   // proof of zero network attempts. The sole dispatcher verifies complete
   // post-expiry history AND unchanged full-account cash/positions first.
   if e["source"]!="moloch_live_dispatcher"||e["networkAttemptRetained"]!=true||e["sameIntentRetryAuthorized"]!=false||e["venueOrderIdentityKnown"]!=false||e["accountEffectsReconciled"]!=true{return Err("EXPIRED_EXPOSURE_PROOF")}
   for key in ["receiptHash","originalIntentSha256","originalAttemptSha256","originalErrorSha256"]{if e[key].as_str().is_none_or(|s|s.len()!=64||!s.bytes().all(|b|b.is_ascii_hexdigit())){return Err("EXPIRED_EXPOSURE_PROOF")}}
   let qid=e["orderId"].as_u64().ok_or("ACTUAL_INTENT_ID")?;
   let index=self.orders.iter().position(|q|q.id==qid).ok_or("ACTUAL_UNOWNED_ORDER")?;let q=&self.orders[index];
   if self.is_package_order(qid)||self.live_orders.contains_key(&qid)||q.filled!=0||q.retired||q.end()>=_at||e["expiresAt"].as_i64()!=Some(q.expires)||e["exchangeId"]!=q.exchange||e["marketId"]!=q.market{return Err("EXPIRED_EXPOSURE_INTENT")}
   if q.buy{self.wallet.release(&qid.to_string())?}self.orders[index].retired=true;self.count("expiredAttemptExposuresReconciled");return Ok(())
  }
  if e["type"]=="unsubmitted_intent_retired" {
   if e.get("deferPlanningForPackage").is_some_and(|v|!v.is_boolean()){return Err("UNSUBMITTED_INTENT_PLANNING_FLAG")}
   if e["source"]!="moloch_live_dispatcher"||e["confirmedNetworkAttempts"].as_u64()!=Some(0)||e["receiptHash"].as_str().is_none_or(|s|s.len()!=64||!s.bytes().all(|b|b.is_ascii_hexdigit())){return Err("UNSUBMITTED_INTENT_PROOF")}
   let qid=e["orderId"].as_u64().ok_or("ACTUAL_INTENT_ID")?;if self.is_package_order(qid){return Err("PACKAGE_CANNOT_BE_ABANDONED_AS_ORDINARY_INTENT")};let index=self.orders.iter().position(|q|q.id==qid).ok_or("ACTUAL_UNOWNED_ORDER")?;
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
   self.package_ack_guard(qid,admitted)?;
   if e["exchangeId"]!=q.exchange||e["marketId"]!=q.market||e["price"]!=money(q.limit)||e["side"]!=label(q.side)||e["action"]!=if q.buy{"buy"}else{"sell"}{return Err("ACTUAL_ACK_INTENT_MISMATCH")}
   if let Some(old)=self.live_orders.get(&qid){if old!=&actual||admitted!=q.quantity{return Err("ACTUAL_ORDER_BINDING_CHANGED")};return Ok(())}
   if self.orders[index].retired||self.live_orders.values().any(|v|v==&actual){return Err("ACTUAL_ORDER_ALREADY_BOUND")}
   if admitted<self.orders[index].quantity {
    let q=&self.orders[index];if q.filled!=0{return Err("ACTUAL_RESIZE_AFTER_FILL")}
    if q.buy{let r=Reservation{exchange:q.exchange.clone(),side:q.side,quantity:admitted,limit:q.limit};self.wallet.release(&qid.to_string())?;self.wallet.reserve(&qid.to_string(),r)?;}
    self.orders[index].quantity=admitted;self.count("actualOrderResizes");
   }
   self.live_orders.insert(qid,actual);self.package_acknowledged(qid);self.count("actualOrderAcknowledgments");return Ok(())
  }
  if self.live_orders.get(&qid)!=Some(&actual){return Err("ACTUAL_UNOWNED_ORDER")}
  if e["type"]=="actual_order_terminal" {
   let q=&self.orders[index];let status=e["status"].as_str().ok_or("ACTUAL_TERMINAL_STATUS")?;
   if !["canceled","expired","filled"].contains(&status)||e["fillsComplete"]!=true||e["filledQuantity"].as_u64()!=Some(q.filled)||(status=="filled"&&q.filled!=q.quantity){return Err("ACTUAL_TERMINAL_FILL_RECONCILIATION_REQUIRED")}
   if !q.retired {if q.buy{self.wallet.release(&qid.to_string())?};self.orders[index].retired=true;self.count("actualOrderTerminals")};self.package_terminal(qid);return Ok(())
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
 pub(super) fn consume_inherited(&mut self,exchange:&str,side:Side,mut quantity:u64,proceeds_per_share:i128)->Result<()>{
  let Some(inherited)=self.inherited.as_mut() else{return Ok(())};
  let Some(lots)=inherited.remaining.get_mut(&(exchange.into(),side)) else{return Ok(())};
  while quantity>0&&!lots.is_empty(){let lot=lots.front_mut().ok_or("ADOPTION_LOTS")?;let n=quantity.min(lot.quantity);inherited.realized_gross=inherited.realized_gross.checked_add((proceeds_per_share-lot.price).checked_mul(n as i128).ok_or("ADOPTION_RANGE")?).ok_or("ADOPTION_RANGE")?;quantity-=n;lot.quantity-=n;if lot.quantity==0{lots.pop_front();}}
  Ok(())
 }
 fn adopt_positions(&mut self,e:&Value,at:i64)->Result<()>{
  if !self.live_packages.is_empty(){return Err("PACKAGE_STATE_CANNOT_BE_REPLACED_BY_ADOPTION")}
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

#[cfg(test)]mod expired_exposure_tests{
 use super::*;
 #[test]fn expired_uncertain_attempt_retires_only_after_bound_proof_without_ack_or_fill(){
  let mut engine=Engine::new_live(&json!({"quantity":1,"minimumSpreadTicks":1,"requireCurrentFlow":false})).unwrap();
  engine.advance(&json!({"type":"book","receivedAt":"2026-10-18T00:00:00Z","book":{"marketId":"201","exchangeId":"101","tournamentId":"cup","sequence":"1","sourceAt":"2026-10-18T00:00:00Z","requestStartedAt":"2026-10-18T00:00:00Z","receivedAt":"2026-10-18T00:00:00Z","bids":[{"price":"0.400000","quantity":100}],"asks":[{"price":"0.420000","quantity":100}]}})).unwrap();
  let q=engine.order_records().into_iter().find(|q|q["action"]=="buy").expect("native quote");let end=q["expiresAt"].as_i64().unwrap();let at=chrono::DateTime::from_timestamp_millis(end+60001).unwrap().to_rfc3339();
  let e=json!({"type":"expired_attempt_exposure_reconciled_v1","source":"moloch_live_dispatcher","orderId":q["id"],"marketId":q["marketId"],"exchangeId":q["exchangeId"],"expiresAt":end,"receiptHash":"a".repeat(64),"originalIntentSha256":"b".repeat(64),"originalAttemptSha256":"c".repeat(64),"originalErrorSha256":"d".repeat(64),"networkAttemptRetained":true,"sameIntentRetryAuthorized":false,"venueOrderIdentityKnown":false,"accountEffectsReconciled":true,"receivedAt":at,"processedAt":at});
  let cash=engine.wallet.cash;let before=engine.wallet.reserved().unwrap();let positions=engine.wallet.positions();
  for key in ["networkAttemptRetained","sameIntentRetryAuthorized","venueOrderIdentityKnown","accountEffectsReconciled"]{let mut bad=e.clone();bad[key]=json!(!bad[key].as_bool().unwrap());assert!(engine.clone().advance(&bad).is_err());}
  let mut early=e.clone();early["receivedAt"]=json!(chrono::DateTime::from_timestamp_millis(end).unwrap().to_rfc3339());early["processedAt"]=early["receivedAt"].clone();assert!(engine.clone().advance(&early).is_err());
  engine.advance(&e).unwrap();assert_eq!(engine.wallet.cash,cash);assert_eq!(engine.wallet.positions(),positions);assert!(engine.fills().is_empty()&&engine.live_order_bindings().is_empty());assert!(engine.wallet.reserved().unwrap()<before);assert_eq!(engine.order_records().into_iter().find(|r|r["id"]==q["id"]).unwrap()["retired"],true);assert!(engine.advance(&e).is_err());
 }
}
