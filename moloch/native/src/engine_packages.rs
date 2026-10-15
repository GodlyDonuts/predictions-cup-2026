//! Native funding/lifecycle only. A proof hash or claimed floor is NOT settlement
//! certification, human authority, an HTTP acknowledgment, or a fill.
//!
//! Reserve is one transactional event for 2--3 distinct, flat BUY components.
//! The bank reserves sum(quantity * frozen limit), not a weighted-depth estimate.
//! Only zero-fee policies are supported. The runtime must independently certify
//! settlement/refund states, source/account/bank scope and physical AON semantics.
//! Native code validates source hashes, freshness, full executable depth and caps;
//! it does not interpret proofHash as a certificate of any claimed payout.
//!
//! Attempt consumes a single stable request/key binding. Its bookHashes map pins
//! current raw books while preserving the original limits, quantities and expiry.
//! Unknown retains every unexecuted reservation. An attempt can release funding
//! only through typed, complete, allowlisted zero-execution response evidence, or
//! ordinary verified actual terminal/fill feedback. ACK alone never credits cash.
//!
//! Complete requires every original quantity actually filled and every actual
//! binding terminal-reconciled. Component positions remain protected from all
//! ordinary strategy exits after completion: this version holds to settlement.
//! Partial/canceled packages require manual resolution; no automatic repair or
//! component liquidation is implemented. Restart must replay these events; seed
//! import of package state is explicitly rejected rather than silently dropping it.
use super::*;

#[derive(Clone,Copy,PartialEq,Eq)]
enum Phase {Reserved,Attempted,Acknowledged,Unknown,NoExecution,Complete}
impl Phase {
 fn label(self)->&'static str{match self{Self::Reserved=>"reserved",Self::Attempted=>"attempted",Self::Acknowledged=>"acknowledged",Self::Unknown=>"unknown",Self::NoExecution=>"no_execution",Self::Complete=>"complete"}}
 fn closed(self)->bool{matches!(self,Self::NoExecution|Self::Complete)}
}
#[derive(Clone)]
struct Leg {qid:u64,market:String,exchange:String,side:Side,quantity:u64,limit:i128,book_hash:String,terminal:bool}
#[derive(Clone)]
pub(super) struct LivePackage {
 id:String,proof:String,scope:Value,economics:Value,created:i64,expires:i64,phase:Phase,
 legs:Vec<Leg>,request:Option<String>,key:Option<String>,attempted_at:Option<i64>,zero_evidence:Option<Value>,
}
fn digest(v:&Value)->Result<String>{Ok(v.as_str().filter(|s|s.len()==64&&s.bytes().all(|b|b.is_ascii_hexdigit()&&!b.is_ascii_uppercase())).ok_or("PACKAGE_HASH")?.to_owned())}
fn package_id(v:&Value)->Result<String>{Ok(v.as_str().filter(|s|!s.is_empty()&&s.len()<=128&&s.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-'||b==b'_')).ok_or("PACKAGE_ID")?.to_owned())}
fn side(v:&Value)->Result<Side>{match v.as_str(){Some("YES")=>Ok(Side::Yes),Some("NO")=>Ok(Side::No),_=>Err("PACKAGE_SIDE")}}
fn exact_keys(v:&Value,keys:&[&str])->Result<()>{let o=v.as_object().ok_or("PACKAGE_OBJECT")?;if o.len()!=keys.len()||keys.iter().any(|k|!o.contains_key(*k)){return Err("PACKAGE_FIELDS")};Ok(())}
fn zero_execution(status:u64,v:&Value,count:usize)->bool{
 if v.get("results").is_some()||!v["error"]["message"].is_string(){return false}
 match(status,v["error"]["code"].as_str()){
  (400,Some("VALIDATION_ERROR"))=>v["error"]["details"]["rejectionReasonCode"]=="AllOrNoneNotFilled"&&v["error"]["details"]["leg"].as_u64().is_some_and(|n|n<(count as u64)),
  (429,Some("RATE_LIMITED"))|(401,Some("MISSING_API_KEY"|"INVALID_API_KEY"|"API_KEY_REVOKED"|"API_KEY_EXPIRED"|"UNAUTHORIZED"))|(403,Some("FORBIDDEN"|"INSUFFICIENT_SCOPES"|"TERMS_NOT_ACKNOWLEDGED"|"ACCOUNT_BANNED"|"RESIDENCE_UPDATE_REQUIRED"))|(404,Some("NOT_FOUND"))|(422,Some("RELATIONSHIP_VIOLATION"))=>true,
  _=>false,
 }
}
impl Engine {
 pub fn live_package_books(&self)->Vec<Value>{self.books.values().filter(|b|!b.invalid).map(|b|b.raw.clone()).collect()}
 pub fn is_package_order(&self,id:u64)->bool{self.package_orders.contains_key(&id)}
 pub fn package_blocks_new_submissions(&self)->bool{self.live_packages.values().any(|p|!p.phase.closed())}
 pub fn package_blocks_submissions(&self)->bool{self.package_blocks_new_submissions()}
 pub fn package_records(&self)->Value{json!(self.live_packages.values().map(|p|json!({"packageId":p.id,"proofHash":p.proof,"bankScope":p.scope,"economics":p.economics,"createdAt":p.created,"expiresAt":p.expires,"phase":p.phase.label(),"requestHash":p.request,"idempotencyKey":p.key,"attemptedAt":p.attempted_at,"zeroExecutionEvidence":p.zero_evidence,"legs":p.legs.iter().map(|l|json!({"qid":l.qid,"marketId":l.market,"exchangeId":l.exchange,"side":label(l.side),"quantity":l.quantity,"limitPrice":money(l.limit),"bookHash":l.book_hash,"actualOrderId":self.live_orders.get(&l.qid),"filled":self.orders.iter().find(|q|q.id==l.qid).map(|q|q.filled),"terminalReconciled":l.terminal})).collect::<Vec<_>>()})).collect::<Vec<_>>())}
 pub(super) fn package_protects_exchange(&self,exchange:&str)->bool{
  self.live_packages.values().any(|p|p.phase!=Phase::NoExecution&&p.legs.iter().any(|l|l.exchange==exchange))
 }
 pub(super) fn package_ack_guard(&self,qid:u64,quantity:u64)->Result<()>{
  if let Some(id)=self.package_orders.get(&qid){let p=self.live_packages.get(id).ok_or("PACKAGE_INDEX")?;let l=p.legs.iter().find(|l|l.qid==qid).ok_or("PACKAGE_LEG")?;
   if (!matches!(p.phase,Phase::Attempted|Phase::Acknowledged|Phase::Unknown)&&!(p.phase==Phase::Complete&&self.live_orders.contains_key(&qid)))||quantity!=l.quantity{return Err("PACKAGE_ACK_OUTSIDE_ATTEMPT_OR_QUANTITY")}
  }Ok(())
 }
 pub(super) fn package_acknowledged(&mut self,qid:u64){
  if let Some(id)=self.package_orders.get(&qid){if let Some(p)=self.live_packages.get_mut(id){if p.phase==Phase::Attempted&&p.legs.iter().all(|l|self.live_orders.contains_key(&l.qid)){p.phase=Phase::Acknowledged;}}}
 }
 pub(super) fn package_terminal(&mut self,qid:u64){
  if let Some(id)=self.package_orders.get(&qid){if let Some(p)=self.live_packages.get_mut(id){if let Some(l)=p.legs.iter_mut().find(|l|l.qid==qid){l.terminal=true;}}}
 }
 pub(super) fn audit_packages(&self)->Result<()>{
  for(id,p)in &self.live_packages{if id!=&p.id{return Err("PACKAGE_INDEX")}
   for l in &p.legs{let q=self.orders.iter().find(|q|q.id==l.qid).ok_or("PACKAGE_ORDER_MISSING")?;
    if self.package_orders.get(&l.qid)!=Some(id)||!q.buy||q.market!=l.market||q.exchange!=l.exchange||q.side!=l.side||q.quantity!=l.quantity||q.limit!=l.limit||q.expires!=p.expires||q.cancel.is_some(){return Err("PACKAGE_ORDER_CHANGED")}
    let reserved=self.wallet.reservations.get(&q.id.to_string());
    if !q.retired&&q.remaining()>0 {let r=reserved.ok_or("PACKAGE_RESERVATION_MISSING")?;if r.quantity!=q.remaining()||r.limit!=q.limit||r.exchange!=q.exchange||r.side!=q.side{return Err("PACKAGE_RESERVATION_CHANGED")}}
    else if reserved.is_some(){return Err("PACKAGE_RESERVATION_AFTER_RETIREMENT")}
    if p.phase==Phase::NoExecution&&(!q.retired||q.filled!=0||self.live_orders.contains_key(&q.id)){return Err("PACKAGE_NO_EXECUTION_CONFLICT")}
    if p.phase==Phase::Complete&&(!l.terminal||q.filled!=l.quantity||!self.live_orders.contains_key(&q.id)){return Err("PACKAGE_COMPLETION_CONFLICT")}
   }
  }Ok(())
 }
 pub(super) fn apply_package(&mut self,e:&Value,at:i64)->Result<()>{
  if !self.live_mode||e["source"]!="moloch_live_dispatcher"{return Err("PACKAGE_LIVE_SOURCE")}
  let id=package_id(&e["packageId"])?;
  if e["type"]=="live_package_reserve"{return self.reserve_package(e,at,id)}
  let snapshot=self.live_packages.get(&id).ok_or("PACKAGE_UNKNOWN_ID")?.clone();
  if e["proofHash"]!=snapshot.proof||e["bankScope"]!=snapshot.scope{return Err("PACKAGE_BINDING_CHANGED")}
  match e["type"].as_str(){
   Some("live_package_attempt")=>{
    if snapshot.phase!=Phase::Reserved||at>=snapshot.expires||self.policy.stop.is_none_or(|stop|at>=stop)||at<snapshot.created+self.policy.latency{return Err("PACKAGE_ATTEMPT_WINDOW_OR_REPEAT")}
    // Freeze economics, not a stale book revision. The caller supplies the
    // current exact raw hashes; revalidate full depth at the ORIGINAL limits.
    let current=e["bookHashes"].as_object().filter(|m|m.len()==snapshot.legs.len()).ok_or("PACKAGE_ATTEMPT_BOOK_HASHES")?;
    for l in &snapshot.legs{if !self.policy.entry(&l.exchange,at){return Err("PACKAGE_ENTRY_DISABLED")};let mut fresh=l.clone();fresh.book_hash=digest(current.get(&l.exchange).ok_or("PACKAGE_ATTEMPT_BOOK_HASHES")?)?;self.package_book(&fresh,at,&snapshot.scope)?;}
    let request=digest(&e["requestHash"])?;let key=e["idempotencyKey"].as_str().filter(|s|!s.is_empty()&&s.len()<=256).ok_or("PACKAGE_IDEMPOTENCY_KEY")?.to_owned();
    let p=self.live_packages.get_mut(&id).unwrap();p.request=Some(request);p.key=Some(key);p.attempted_at=Some(at);p.phase=Phase::Attempted;
   },
   Some("live_package_unknown")=>{
    if !matches!(snapshot.phase,Phase::Attempted|Phase::Acknowledged|Phase::Unknown)||e["requestHash"].as_str()!=snapshot.request.as_deref(){return Err("PACKAGE_UNKNOWN_OUTSIDE_ATTEMPT")}
    self.live_packages.get_mut(&id).unwrap().phase=Phase::Unknown;
   },
   Some("live_package_no_execution")=>{
    if snapshot.phase.closed()||snapshot.legs.iter().any(|l|self.live_orders.contains_key(&l.qid)||self.orders.iter().any(|q|q.id==l.qid&&q.filled!=0)){return Err("PACKAGE_NO_EXECUTION_AFTER_ACK_OR_FILL")}
    let proof=&e["evidence"];
    if snapshot.phase==Phase::Reserved {
     exact_keys(proof,&["kind","confirmedNetworkAttempts"])?;
     if proof["kind"]!="never_attempted"||proof["confirmedNetworkAttempts"].as_u64()!=Some(0){return Err("PACKAGE_NEVER_ATTEMPTED_PROOF")}
    }else if proof["kind"]=="local_queue_not_sent_v1"{
     local_not_sent(&snapshot,proof,at)?;
    }else{
     if proof["kind"]!="official_multi_leg_zero_execution"||proof["completeBody"]!=true||proof["requestHash"].as_str()!=snapshot.request.as_deref()||digest(&proof["receiptHash"]).is_err()||digest(&proof["responseHash"])?!=hash(&proof["response"])||!zero_execution(proof["status"].as_u64().ok_or("PACKAGE_ZERO_STATUS")?,&proof["response"],snapshot.legs.len()){return Err("PACKAGE_TYPED_ZERO_EXECUTION_REQUIRED")}
     let started=ms(&proof["requestStartedAt"])?;let received=ms(&proof["receivedAt"])?;
     if snapshot.attempted_at.is_none_or(|attempt|started<attempt)||started>received||received>at{return Err("PACKAGE_ZERO_EXECUTION_CLOCK")}
    }
    for l in &snapshot.legs{self.wallet.release(&l.qid.to_string())?;self.orders.iter_mut().find(|q|q.id==l.qid).ok_or("PACKAGE_ORDER_MISSING")?.retired=true;}
    let p=self.live_packages.get_mut(&id).unwrap();p.phase=Phase::NoExecution;p.zero_evidence=Some(proof.clone());
   },
   Some("live_package_ack")=>{
    let rows=e["acknowledgments"].as_array().filter(|rows|rows.len()==snapshot.legs.len()).ok_or("PACKAGE_ACK_ROWS")?;let mut seen=BTreeSet::new();
    for row in rows{let qid=row["orderId"].as_u64().ok_or("PACKAGE_ACK_QID")?;if row["type"]!="actual_order_ack"||!snapshot.legs.iter().any(|l|l.qid==qid)||!seen.insert(qid)||ms(&row["receivedAt"])? > at{return Err("PACKAGE_ACK_ROWS")};self.apply_live(row,at)?;}
   },
   Some("live_package_complete")=>{
    if !matches!(snapshot.phase,Phase::Attempted|Phase::Acknowledged|Phase::Unknown)||snapshot.legs.iter().any(|l|!l.terminal||!self.live_orders.contains_key(&l.qid)||self.orders.iter().find(|q|q.id==l.qid).is_none_or(|q|q.filled!=l.quantity||!q.retired)){return Err("PACKAGE_COMPLETE_REQUIRES_ACTUAL_RECONCILIATION")}
    self.live_packages.get_mut(&id).unwrap().phase=Phase::Complete;
   },
   _=>return Err("PACKAGE_EVENT"),
  }Ok(())
 }
 fn package_book(&self,l:&Leg,at:i64,scope:&Value)->Result<()>{
  let b=self.books.get(&l.exchange).ok_or("PACKAGE_BOOK_MISSING")?;
  if !b.fresh(&self.policy,at)||at-b.source>self.policy.book_age||b.market!=l.market||b.tournament!=scope["tournamentId"].as_str().ok_or("PACKAGE_SCOPE")?||hash(&b.raw)!=l.book_hash{return Err("PACKAGE_BOOK_STALE_OR_CHANGED")}
  let mut needed=l.quantity;
  for(p,n)in if l.side==Side::Yes{&b.asks}else{&b.bids}{let ask=if l.side==Side::Yes{*p}else{SCALE-*p};if ask>l.limit{break};needed=needed.saturating_sub(*n);if needed==0{break}}
  if needed!=0{return Err("PACKAGE_FULL_DEPTH_REQUIRED")};Ok(())
 }
 fn reserve_package(&mut self,e:&Value,at:i64,package_name:String)->Result<()>{
  if self.live_packages.contains_key(&package_name){return Err("PACKAGE_DUPLICATE_ID")}
  if self.package_blocks_new_submissions(){return Err("PACKAGE_UNRESOLVED")}
  if self.policy.fee_bps!=0||self.policy.fee_share!=0{return Err("PACKAGE_ZERO_FEES_ONLY")}
  let proof=digest(&e["proofHash"])?;let scope=e["bankScope"].clone();exact_keys(&scope,&["runId","bankManifestHash","tournamentId"])?;
  if scope["runId"].as_str().is_none_or(|s|s.is_empty()||s.len()>128)||scope["tournamentId"].as_str().is_none_or(|s|s.is_empty())||digest(&scope["bankManifestHash"]).is_err(){return Err("PACKAGE_SCOPE")}
  if self.live_packages.values().any(|p|p.scope!=scope){return Err("PACKAGE_BANK_SCOPE_CHANGED")}
  let expires=ms(&e["expiresAt"])?;
  if expires<=at.checked_add(self.policy.latency).ok_or("PACKAGE_CLOCK")?||self.policy.stop.is_none_or(|stop|at>=stop||expires>stop)||expires-at>self.policy.book_age{return Err("PACKAGE_ENTRY_OR_EXPIRY")}
  let rows=e["legs"].as_array().filter(|xs|(2..=3).contains(&xs.len())).ok_or("PACKAGE_LEG_COUNT")?;
  let mut legs=vec![];let mut exchanges=BTreeSet::new();let mut total=0i128;
  let mut markets:BTreeSet<String>=self.wallet.lots.iter().filter(|(_,lots)|!lots.is_empty()).map(|((ex,_),_)|ex.clone()).collect();
  markets.extend(self.orders.iter().filter(|q|!q.retired).map(|q|q.exchange.clone()));
  for row in rows{
   exact_keys(row,&["marketId","exchangeId","side","quantity","limitPrice","bookHash"])?;
   let exchange=id(&row["exchangeId"])?;let market=id(&row["marketId"])?;let leg_side=side(&row["side"])?;
   if !exchanges.insert(exchange.clone())||self.wallet.quantity(&exchange,Side::Yes)>0||self.wallet.quantity(&exchange,Side::No)>0||self.orders.iter().any(|q|q.exchange==exchange&&!q.retired){return Err("PACKAGE_COMPONENT_NOT_FLAT_OR_UNRESOLVED")}
   if !self.policy.entry(&exchange,at){return Err("PACKAGE_ENTRY_DISABLED")}
   let quantity=row["quantity"].as_u64().filter(|n|*n>0&&*n<=self.policy.quantity).ok_or("PACKAGE_QUANTITY")?;let limit=price(dec(&row["limitPrice"])?)?;
   let cost=limit.checked_mul(quantity as i128).ok_or("PACKAGE_RANGE")?;if cost>self.policy.market{return Err("PACKAGE_MARKET_CAP")};total=total.checked_add(cost).ok_or("PACKAGE_RANGE")?;
   let qid=self.prior_orders.checked_add(self.orders.len() as u64).and_then(|n|n.checked_add(legs.len() as u64+1)).ok_or("PACKAGE_QID_RANGE")?;
   let leg=Leg{qid,market,exchange:exchange.clone(),side:leg_side,quantity,limit,book_hash:digest(&row["bookHash"])?,terminal:false};self.package_book(&leg,at,&scope)?;markets.insert(exchange);legs.push(leg);
  }
  if legs.iter().try_fold(0u64,|n,l|n.checked_add(l.quantity)).is_none_or(|n|n>self.policy.quantity){return Err("PACKAGE_AGGREGATE_QUANTITY_CAP")}
  let inherited_market=|exchange:&String|self.inherited.as_ref().is_some_and(|i|i.remaining.iter().any(|((ex,_),lots)|ex==exchange&&!lots.is_empty()));
  let fresh_limit=self.inherited.as_ref().map_or(self.policy.markets,|i|i.fresh_market_limit).min(3);
  if markets.len()>self.policy.markets||markets.iter().filter(|ex|!inherited_market(ex)).count()>fresh_limit{return Err("PACKAGE_MARKET_COUNT_CAP")}
  let economics=e["economics"].clone();exact_keys(&economics,&["maximumCost","minimumPayout","minimumSurplus","feeTotal"])?;
  let payout=dec(&economics["minimumPayout"])?;let surplus=dec(&economics["minimumSurplus"])?;
  let maximum_payout=legs.iter().try_fold(0i128,|n,l|n.checked_add(SCALE.checked_mul(l.quantity as i128).ok_or("PACKAGE_RANGE")?).ok_or("PACKAGE_RANGE"))?;
  if dec(&economics["maximumCost"])?!=total||dec(&economics["feeTotal"])?!=0||surplus<=0||payout>maximum_payout||payout.checked_sub(total)!=Some(surplus){return Err("PACKAGE_ECONOMICS")}
  for l in &legs{
   self.wallet.reserve(&l.qid.to_string(),Reservation{exchange:l.exchange.clone(),side:l.side,quantity:l.quantity,limit:l.limit})?;
   self.orders.push(Order{id:l.qid,market:l.market.clone(),exchange:l.exchange.clone(),tournament:scope["tournamentId"].as_str().unwrap().into(),side:l.side,buy:true,quantity:l.quantity,filled:0,limit:l.limit,created:at,activation:at+self.policy.latency,expires,cancel:None,retired:false,credit:0,barrier:None,same:0,better:0,visible_same:0,visible_better:0,touch_remainder:0,aon:false,age_reason:false,through:false,participation:10000,conditional:false,touch_bps:0});
   self.package_orders.insert(l.qid,package_name.clone());
  }
  self.live_packages.insert(package_name.clone(),LivePackage{id:package_name,proof,scope,economics,created:at,expires,phase:Phase::Reserved,legs,request:None,key:None,attempted_at:None,zero_evidence:None});Ok(())
 }
}

// These embedded rows are independently checked by the read-only SDK source
// verifier. Native hash/transition validation is a binding fence, not proof of
// authenticity or a replacement for the original private queue/dispatch files.
fn local_queue_state(row:&Value,raw:&Value)->Result<Value>{
 let text=raw.as_str().filter(|s|s.len()<=8*1024*1024).ok_or("PACKAGE_LOCAL_QUEUE_STATE")?;
 let state:Value=serde_json::from_str(text).map_err(|_|"PACKAGE_LOCAL_QUEUE_STATE")?;
 if state!=row["state"]||state["schema_version"]!=1{return Err("PACKAGE_LOCAL_QUEUE_STATE")}
 let sequence=row["sequence"].as_u64().ok_or("PACKAGE_LOCAL_QUEUE_SEQUENCE")?;
 let previous=row["previous"].as_str().ok_or("PACKAGE_LOCAL_QUEUE_PREVIOUS")?;
 let bytes=format!("[{},{},{}]",sequence,serde_json::to_string(previous).map_err(|_|"PACKAGE_LOCAL_QUEUE_STATE")?,text);
 if digest(&row["hash"])?!=format!("{:x}",Sha256::digest(bytes.as_bytes())){return Err("PACKAGE_LOCAL_QUEUE_ROW_HASH")};Ok(state)
}
fn local_not_sent(p:&LivePackage,proof:&Value,at:i64)->Result<()>{
 if at<0||p.phase!=Phase::Attempted||proof["kind"]!="local_queue_not_sent_v1"||proof["requestHash"].as_str()!=p.request.as_deref(){return Err("PACKAGE_LOCAL_NOT_SENT_PHASE")}
 let error=&proof["sdkError"];
 if error["mayHaveExecuted"]!=false||error.get("status")!=Some(&Value::Null)||error.get("orderId")!=Some(&Value::Null)||error.get("apiCode")!=Some(&Value::Null)||!error["code"].as_str().is_some_and(|s|!s.is_empty()){return Err("PACKAGE_LOCAL_NOT_SENT_ERROR")}
 let dispatch=&proof["dispatchRecord"];if digest(&proof["dispatchRecordHash"])?!=hash(dispatch)||dispatch["phase"]!="not_sent"||dispatch["outcome"]["kind"]!="not_sent"||dispatch["outcome"]["error"]!=*error||proof["dispatchHead"]["poisoned"]!=false||proof["dispatchHead"]["sequence"].as_u64().is_none_or(|n|n==0)||digest(&proof["dispatchHead"]["hash"]).is_err(){return Err("PACKAGE_LOCAL_DISPATCH_PROOF")}
 let admission=&dispatch["admission"];let request_text=proof["requestJson"].as_str().ok_or("PACKAGE_LOCAL_REQUEST")?;let request:Value=serde_json::from_str(request_text).map_err(|_|"PACKAGE_LOCAL_REQUEST")?;
 if admission["request_hash"].as_str()!=p.request.as_deref()||format!("{:x}",Sha256::digest(request_text.as_bytes()))!=p.request.as_deref().ok_or("PACKAGE_LOCAL_REQUEST")?||request!=admission["spec"]||request["method"]!="POST"||request["endpoint"]!="/orders/multi-leg"||request["body"]["executionMode"]!="allOrNone"||request["body"]["idempotencyKey"].as_str()!=p.key.as_deref()||admission["reserve"]["packageId"]!=p.id||admission["reserve"]["proofHash"]!=p.proof||admission["reserve"]["bankScope"]!=p.scope||admission["permit"]["runId"]!=p.scope["runId"]||admission["permit"]["intentId"]!=p.id{return Err("PACKAGE_LOCAL_REQUEST")}
 let scope=&admission["scope"];if scope["tournamentId"]!=p.scope["tournamentId"]{return Err("PACKAGE_LOCAL_REQUEST_SCOPE")}
 let legs=request["body"]["legs"].as_array().filter(|l|l.len()==p.legs.len()).ok_or("PACKAGE_LOCAL_REQUEST_LEGS")?;
 for(l,w)in p.legs.iter().zip(legs){if w["exchangeId"]!=l.exchange||w["side"]!=(if l.side==Side::Yes{"yes"}else{"no"})||w["action"]!="buy"||w["quantity"]!=l.quantity||w["tournamentId"]!=p.scope["tournamentId"]||amount(&w["price"].to_string())?!=l.limit||scope["exchanges"][&l.exchange]!=l.market{return Err("PACKAGE_LOCAL_REQUEST_LEGS")}}
 let q=&proof["queueProof"];let binding=&q["binding"];let queue_scope=q["queueScope"].as_str().filter(|s|!s.is_empty()).ok_or("PACKAGE_LOCAL_QUEUE_SCOPE")?;
 if q["schemaVersion"]!=1||admission["queue_scope"]!=queue_scope||binding!=&admission["queue_binding"]||binding["intent"]!=format!("{}:{}",p.scope["runId"].as_str().ok_or("PACKAGE_LOCAL_RUN")?,p.id)||binding["key"].as_str()!=p.key.as_deref()||binding["payload_hash"]!=hash(&json!({"scope":scope,"spec":request})){return Err("PACKAGE_LOCAL_QUEUE_BINDING")}
 let before=local_queue_state(&q["beforeRecord"],&q["beforeStateJson"])?;let after=local_queue_state(&q["completionRecord"],&q["completionStateJson"])?;let tail=local_queue_state(&q["tailRecord"],&q["tailStateJson"])?;
 let request_id=q["request"].as_str().filter(|s|!s.is_empty()).ok_or("PACKAGE_LOCAL_QUEUE_REQUEST")?;
 let before_clock=before["last_clock"].as_u64().ok_or("PACKAGE_LOCAL_QUEUE_CLOCK")?;let completion_clock=after["last_clock"].as_u64().ok_or("PACKAGE_LOCAL_QUEUE_CLOCK")?;let tail_clock=tail["last_clock"].as_u64().ok_or("PACKAGE_LOCAL_QUEUE_CLOCK")?;
 if before_clock<p.attempted_at.ok_or("PACKAGE_LOCAL_ATTEMPT")? as u64||completion_clock<before_clock||tail_clock<completion_clock||tail_clock>at as u64||q["completionRecord"]["sequence"].as_u64()!=q["beforeRecord"]["sequence"].as_u64().and_then(|n|n.checked_add(1))||q["completionRecord"]["previous"]!=q["beforeRecord"]["hash"]||q["tailRecord"]["sequence"].as_u64()<q["completionRecord"]["sequence"].as_u64(){return Err("PACKAGE_LOCAL_QUEUE_CLOCK_OR_CHAIN")}
 for state in [&before,&after,&tail]{if state["scope"]!=queue_scope||state["manual_review"]!=false||!state["unknown"].as_object().is_some_and(|m|m.is_empty())||state["intents"][binding["intent"].as_str().ok_or("PACKAGE_LOCAL_QUEUE_BINDING")?]!=*binding||state["keys"][binding["key"].as_str().ok_or("PACKAGE_LOCAL_QUEUE_BINDING")?]!=*binding{return Err("PACKAGE_LOCAL_QUEUE_GLOBAL")}}
 if before["active"]["request"]!=request_id||before["active"]["write"]!=true||before["active"]["network_started"]!=false||before["active"]["binding"]!=*binding||after.get("active")!=Some(&Value::Null)||tail.get("active")!=Some(&Value::Null){return Err("PACKAGE_LOCAL_QUEUE_NETWORK")}
 let mut expected=before.clone();expected["last_clock"]=json!(completion_clock);expected["active"]=Value::Null;
 for field in ["reads","writes"]{let history=before[field].as_array().ok_or("PACKAGE_LOCAL_QUEUE_HISTORY")?.iter().map(Value::as_u64).collect::<Option<Vec<_>>>().ok_or("PACKAGE_LOCAL_QUEUE_HISTORY")?;expected[field]=json!(history.into_iter().filter(|n|completion_clock.saturating_sub(*n)<=60_000).collect::<Vec<_>>());}
 let next=before["next"].as_u64().ok_or("PACKAGE_LOCAL_QUEUE_NEXT")?.max(completion_clock.checked_add(2500).ok_or("PACKAGE_LOCAL_QUEUE_NEXT")?).max(before["cooldown"].as_u64().ok_or("PACKAGE_LOCAL_QUEUE_NEXT")?);expected["next"]=json!(next);
 if expected!=after||digest(&q["journalSha256"]).is_err()||q["journalBytes"].as_u64().is_none_or(|n|n==0||n>64*1024*1024)||dispatch["outcome"]["queueEvidence"]["request"]!=request_id||dispatch["outcome"]["queueEvidence"]["binding"]!=*binding{return Err("PACKAGE_LOCAL_QUEUE_TRANSITION")};
 if ms(&dispatch["outcome"]["at"])? > at{return Err("PACKAGE_LOCAL_DISPATCH_CLOCK")};Ok(())
}
