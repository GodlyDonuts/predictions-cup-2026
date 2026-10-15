//! Actual-account feedback. Public prints and depth cannot fill live orders.
use super::*;
impl Engine {
 pub fn new_live(policy:&Value)->Result<Self>{let mut e=Self::new(policy)?;if e.policy.conditional{return Err("LIVE_CONDITIONAL_FILL_MODEL")};e.live_mode=true;Ok(e)}
 pub fn is_live(&self)->bool{self.live_mode}
 pub fn live_order_bindings(&self)->&BTreeMap<u64,String>{&self.live_orders}
 pub(super) fn apply_live(&mut self,e:&Value,_at:i64)->Result<()> {
  if !self.live_mode {return Err("ACTUAL_EVENT_SOURCE")}
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
  if q.buy {self.wallet.buy_fill_exact(&qid.to_string(),n,p,charge)?;if let Some(lot)=self.wallet.lots.get_mut(&(q.exchange.clone(),q.side)).and_then(|lots|lots.back_mut()){lot.executed_at=Some(e["executedAt"].clone())}}
  else {self.wallet.sell_fill_exact(&q.exchange,q.side,n,p,charge)?}
  self.fills.push(json!({"orderId":qid,"actualOrderId":actual,"sourceId":fill_id,"marketId":q.market,"exchangeId":q.exchange,"side":label(q.side),"action":if q.buy{"buy"}else{"sell"},"quantity":n,"price":money(p),"fee":money(charge),"at":e.get("processedAt").unwrap_or(&e["receivedAt"]),"executedAt":e["executedAt"],"counterfactual":false,"receiptHash":receipt}));
  self.live_fill_ids.insert(fill_id,facts);let q=&mut self.orders[index];q.filled+=n;if q.remaining()==0{q.retired=true};self.count("actualFills");Ok(())
 }
}
