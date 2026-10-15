use baal_transport_service::{self as api,Authority,Config,Endpoint,Order,Permit,Receipt,RequestSpec,Scope,SystemClock,Transport,ReadOnlyAuthority,credentials::UnixFileCredentials,decimal_units};
use moloch_live_runtime::{authority::{Activation,ProductionAuthority,SharedAuthority,SnapshotSet},dispatch::DispatchLog,feedback::{self,Result}};
use moloch_replay::{session::{Session,ExclusiveLock,atomic_json},source::RestSource,money};
use moloch_replay::realtime::{Auth,Incoming,Realtime};
use moloch_live_runtime::breadth::Recorder;
use moloch_live_runtime::universe::Universe;
use serde::{Serialize,Deserialize};
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::{fs,path::{Path,PathBuf},sync::Arc,time::{SystemTime,UNIX_EPOCH,Duration}};
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Control{activation:Activation,scope:Scope,policy:Value,root:PathBuf,credentials:PathBuf,owner_lock:PathBuf,account_scope:String,#[serde(default)]all_markets:bool,#[serde(default="default_bank")]bank_name:String,#[serde(default)]predecessor_bank:Option<String>}
fn default_bank()->String{"bank".into()}
fn now()->u64{SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64}
fn iso(t:u64)->String{chrono::DateTime::from_timestamp_millis(t as i64).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis,true)}
fn hash(v:&Value)->String{format!("{:x}",Sha256::digest(serde_json::to_vec(v).unwrap()))}
fn load(p:&Path)->Result<Value>{Ok(serde_json::from_slice(&fs::read(p)?)?)}
fn private_dir(p:&Path)->Result<()>{use std::os::unix::fs::PermissionsExt;fs::create_dir_all(p)?;let m=fs::symlink_metadata(p)?;if m.file_type().is_symlink()||!m.is_dir(){return Err("LIVE_DIRECTORY".into())}fs::set_permissions(p,fs::Permissions::from_mode(0o700))?;Ok(())}
fn transport(c:&Control,auth:Box<dyn Authority>,writes:bool)->Result<Transport>{Ok(Transport::new_with_read_budget(Config{queue_dir:c.root.join("account-queue"),account_scope:c.account_scope.clone(),scope:c.scope.clone(),maximum_pages:20,writes_enabled:writes},Box::new(UnixFileCredentials::approved_file(c.credentials.clone())?),Box::new(SystemClock),auth,100,600)?.with_full_account_observation())}
fn accept(bank:&mut Session,event:&Value)->Result<()>{let mut e=event.clone();e["processedAt"]=json!(iso(now()));bank.accept("moloch-live",bank.last_source_sequence("moloch-live")+1,&e)?;Ok(())}
fn record_receipt(root:&Path,r:&Receipt)->Result<()> {let p=root.join("receipts").join(format!("{}.json",feedback::digest(r)?));if !p.exists(){atomic_json(&p,&serde_json::to_value(r)?)?}Ok(())}
fn request(t:&mut Transport,root:&Path,ep:Endpoint)->Result<Receipt>{
 for attempt in 0..3u64{
  match t.request(ep.clone()){
   Ok(r)=>{record_receipt(root,&r)?;return Ok(r)},
   Err(error)=>{
    if attempt==2||!moloch_live_runtime::read_recovery::retryable_read(&error){return Err(error.into())}
    atomic_json(&root.join("read-recovery.json"),&json!({"at":iso(now()),"endpoint":ep,"attempt":attempt+1,"code":error.code,"status":error.status,"retryAfterMs":error.retry_after_ms,"ordersRetried":false}))?;
    std::thread::sleep(Duration::from_millis(2000*(attempt+1)));
   }
  }
 }Err("LIVE_READ_RETRY_EXHAUSTED".into())
}
fn account(t:&mut Transport,root:&Path)->Result<(Receipt,Receipt,Receipt)>{
 let a=request(t,root,Endpoint::Tournament)?;let p=request(t,root,Endpoint::Positions)?;let o=request(t,root,Endpoint::Orders{status:"open".into()})?;
 if a.data["status"]!="active"||o.data["coverage"]["complete"]!=true||o.data["pagination"]["hasMore"]!=false{return Err("LIVE_ACCOUNT_INCOMPLETE_OR_INACTIVE".into())}Ok((a,p,o))
}
fn spec(order:&Order)->Result<RequestSpec>{
 let p=decimal_units(&order.price)?;let number=serde_json::from_str::<Value>(&format!("{}.{:06}",p/1_000_000,p%1_000_000))?;
 let body=json!({"idempotencyKey":order.idempotency_key,"exchangeId":order.exchange_id,"side":order.side,"action":order.action,"quantity":order.quantity,"price":number,"expirationDate":order.expiration_date,"tournamentId":order.tournament_id});
 Ok(RequestSpec{method:"POST".into(),endpoint:"/orders".into(),body_json:Some(serde_json::to_string(&body)?),body:Some(body)})
}
fn quote(bank:&Session,id:u64)->Result<Value>{bank.engine().order_records().into_iter().find(|q|q["id"].as_u64()==Some(id)).ok_or("LIVE_QUOTE_MISSING".into())}
fn retire_never_sent(bank:&mut Session,log:&mut DispatchLog,q:&Value)->Result<()>{
 let id=q["id"].as_u64().ok_or("LIVE_QUOTE_ID")?;if !log.never_attempted(id){return Err("LIVE_CANNOT_RETIRE_ATTEMPTED_INTENT".into())}
 let proof=log.proof(id);log.append(json!({"kind":"not_sent","qid":id,"proof":proof}))?;
 accept(bank,&json!({"type":"unsubmitted_intent_retired","source":"moloch_live_dispatcher","orderId":id,"confirmedNetworkAttempts":0,"receiptHash":hash(&proof),"receivedAt":iso(now())}))
}
fn reconcile(t:&mut Transport,c:&Control,bank:&mut Session,log:&mut DispatchLog)->Result<()>{
 let pending:Vec<_>=log.acknowledgments.iter().filter(|(q,_)|!log.closed.contains(q)).map(|(q,_)|*q).collect();
 for qid in pending{
  let q=quote(bank,qid)?;let actual=bank.engine().live_order_bindings().get(&qid).ok_or("LIVE_ACK_BINDING_MISSING")?.clone();
  // Read the order first. The later complete fill observation determines retirement.
  let order=request(t,&c.root,Endpoint::OwnedOrder{order_id:actual.clone()})?;
  let filled=request(t,&c.root,Endpoint::OwnedFills{order_id:actual.clone()})?;
  for e in feedback::fills(&q,&actual,&c.scope.tournament_id,&filled,&iso(now()))?{accept(bank,&e)?}
  if let Some(e)=feedback::terminal(&q,&actual,&c.scope.tournament_id,&order,&filled,&iso(now()))?{accept(bank,&e)?;log.append(json!({"kind":"closed","qid":qid,"orderReceiptHash":feedback::digest(&order)?,"fillReceiptHash":feedback::digest(&filled)?}))?;}
 }
 Ok(())
}
fn source(t:&mut Transport,c:&Control,bank:&mut Session,s:&mut RestSource,ex:&str,market:&str,from:u64)->Result<u64>{
 let from=s.recovery_start_ms(market).map_or(from,|recovery|from.min(recovery));let to=now();let tape=request(t,&c.root,Endpoint::Trades{exchange_id:ex.into(),from:iso(from),to:iso(to)})?;
 let wrap=|r:&Receipt,kind:&str|json!({"source":"official_sig_api_v1","kind":kind,"tournamentId":c.scope.tournament_id,"marketId":market,"exchangeId":ex,"endpoint":r.endpoint,"status":r.status,"requestStartedAt":r.request_started_at,"receivedAt":r.received_at,"data":r.data});
 for e in s.accept(&wrap(&tape,"trades"))?{accept(bank,&e)?}
 let book=request(t,&c.root,Endpoint::OrderBook{market_id:market.into(),depth:20})?;
 for e in s.accept(&wrap(&book,"book"))?{accept(bank,&e)?}Ok(to.saturating_sub(2000))
}
fn snapshots(t:&mut Transport,c:&Control,q:&Value)->Result<SnapshotSet>{
 let (account,positions,open_orders)=account(t,&c.root)?;
 let market_id=q["marketId"].as_str().ok_or("LIVE_MARKET_ID")?.to_owned();
 let market=request(t,&c.root,Endpoint::Market{market_id:market_id.clone()})?;
 let contract=request(t,&c.root,Endpoint::Nodes{market_id:market_id.clone()})?;
 let book=request(t,&c.root,Endpoint::OrderBook{market_id,depth:20})?;
 Ok(SnapshotSet{account,positions,open_orders,market,contract,book})
}
fn accept_fresh_book(bank:&mut Session,c:&Control,q:&Value,r:&Receipt,source:&mut RestSource)->Result<()>{
 let wrapped=json!({"source":"official_sig_api_v1","kind":"book","tournamentId":c.scope.tournament_id,"marketId":q["marketId"],"exchangeId":q["exchangeId"],"endpoint":r.endpoint,"status":r.status,"requestStartedAt":r.request_started_at,"receivedAt":r.received_at,"data":r.data});
 for e in source.accept(&wrapped)?{accept(bank,&e)?}Ok(())
}
fn dispatch(t:&mut Transport,c:&Control,bank:&mut Session,log:&mut DispatchLog,authority:&Arc<ProductionAuthority>,source:&mut RestSource)->Result<()>{
 let orders=bank.engine().order_records();
 // Cancellations and reconciliation take priority over new purchases.
 for old in &orders {
  let qid=old["id"].as_u64().ok_or("LIVE_QUOTE_ID")?;if old["retired"]==true{continue}
  let expired=old["expiresAt"].as_u64().is_none_or(|t|t<=now());let canceled=old["cancelEffectiveAt"].as_u64().is_some_and(|t|t<=now());
  if !expired&&!canceled{continue}
  if log.never_attempted(qid){retire_never_sent(bank,log,old)?;continue}
  if expired||log.closed.contains(&qid){continue} // Server expiration is reconciled, never assumed.
  let actual=bank.engine().live_order_bindings().get(&qid).ok_or("LIVE_CANCEL_BINDING")?.clone();
  let p=Permit{run_id:c.activation.run_id.clone(),intent_id:format!("{qid}-cancel"),owner:"Codex".into(),expires_at_ms:now()+10000,maximum_quantity:c.activation.maximum_quantity,maximum_cost:c.activation.maximum_committed.clone()};
  let spec=RequestSpec{method:"DELETE".into(),endpoint:format!("/orders/{actual}"),body:None,body_json:None};
  let facts=snapshots(t,c,old)?;authority.prepare(bank,qid,spec.clone(),p.clone(),facts)?;
  if let Err(e)=authority.authorize(&c.scope,&spec,&p,false,now()){authority.clear_ready();atomic_json(&c.root.join("deferred.json"),&json!({"at":iso(now()),"qid":qid,"code":e.code}))?;return Ok(())}
  log.append(json!({"kind":"cancel_attempt","qid":qid,"spec":spec,"permit":p,"at":iso(now())}))?;
  let ack=t.cancel(actual,p)?;record_receipt(&c.root,&ack)?;log.append(json!({"kind":"cancel_ack","qid":qid,"receipt":ack}))?;authority.clear_ready();return Ok(())
 }
 // At most one new physical order per pass; subsequent fills change funding.
 for q in bank.engine().order_records(){
  let qid=q["id"].as_u64().ok_or("LIVE_QUOTE_ID")?;
  if q["retired"]==true||!log.never_attempted(qid)||q["activationAt"].as_u64().is_none_or(|t|t>now())||q["expiresAt"].as_u64().is_none_or(|t|t<=now()+5000)||q["cancelEffectiveAt"].as_u64().is_some(){continue}
  if q["action"]=="buy"&&now()>=c.activation.entry_until_ms{retire_never_sent(bank,log,&q)?;continue}
  let order=Order{idempotency_key:format!("{}:{qid}",c.activation.run_id),exchange_id:q["exchangeId"].as_str().ok_or("LIVE_SCOPE")?.into(),side:if q["side"]=="YES"{"yes"}else{"no"}.into(),action:q["action"].as_str().ok_or("LIVE_ACTION")?.into(),quantity:u32::try_from(q["quantity"].as_u64().ok_or("LIVE_SIZE")?)?,price:q["price"].as_str().ok_or("LIVE_PRICE")?.into(),expiration_date:iso(q["expiresAt"].as_u64().ok_or("LIVE_EXPIRY")?),tournament_id:c.scope.tournament_id.clone()};
  let request_spec=spec(&order)?;let p=Permit{run_id:c.activation.run_id.clone(),intent_id:qid.to_string(),owner:"Codex".into(),expires_at_ms:now()+15000,maximum_quantity:order.quantity,maximum_cost:money(decimal_units(&order.price)?*order.quantity as i128)};
  let facts=snapshots(t,c,&q)?;accept_fresh_book(bank,c,&q,&facts.book,source)?;
  let current=quote(bank,qid)?;if current["retired"]==true||current["cancelEffectiveAt"].is_u64()||current["expiresAt"].as_u64().is_none_or(|v|v<=now()+3000){if current["retired"]==false{retire_never_sent(bank,log,&current)?}continue}
  authority.prepare(bank,qid,request_spec.clone(),p.clone(),facts)?;
  if let Err(e)=authority.authorize(&c.scope,&request_spec,&p,false,now()){authority.clear_ready();atomic_json(&c.root.join("deferred.json"),&json!({"at":iso(now()),"qid":qid,"code":e.code}))?;continue}
  if log.intents.contains_key(&qid){return Err("LIVE_EXISTING_UNRESOLVED_INTENT".into())}
  log.append(json!({"kind":"intent","qid":qid,"quote":q,"spec":request_spec,"permit":p,"at":iso(now())}))?;
  log.append(json!({"kind":"attempt","qid":qid,"at":iso(now())}))?;
  let ack=match t.place(order,p){Ok(r)=>r,Err(e)=>{log.append(json!({"kind":"error","qid":qid,"error":e,"at":iso(now())}))?;return Err("LIVE_WRITE_NOT_CONFIRMED_NO_RETRY".into())}};
  record_receipt(&c.root,&ack)?;log.append(json!({"kind":"ack","qid":qid,"receipt":ack}))?;
  let event=feedback::acknowledgment(&q,&ack,&iso(now()))?;authority.bind_owned(event["actualOrderId"].as_str().ok_or("LIVE_ACTUAL_ID")?.into());accept(bank,&event)?;authority.clear_ready();return Ok(())
 }Ok(())
}
fn connect_stream(t:&mut Transport,c:&Control,bank:&mut Session,source:&mut RestSource)->Result<Realtime>{
 let marker=c.root.join("stream-auth-admission.json");let at=now();
 if marker.exists()&&load(&marker)?["atMs"].as_u64().is_none_or(|old|at<old+60000){return Err("LIVE_AUTH_MINT_BACKOFF".into())}
 atomic_json(&marker,&json!({"atMs":at}))?;
 let material=t.realtime_token()?;let stream=Realtime::connect(Auth::from_official_response(material)?,&c.scope.tournament_id)?;
 match stream.incoming.recv_timeout(Duration::from_secs(12)){
  Ok(Incoming::Joined{received_at})=>{for e in source.subscribed(&received_at)?{accept(bank,&e)?}}
  _=>return Err("LIVE_STREAM_JOIN_FAILED".into())
 }Ok(stream)
}
fn drain_stream(stream:&Realtime,bank:&mut Session,source:&mut RestSource,breadth:&mut Option<Recorder>,universe:&mut Option<Universe>)->Result<()>{
 if !stream.healthy(){return Err("LIVE_STREAM_FAILED".into())}
 while let Ok(input)=stream.incoming.try_recv(){match input{
  Incoming::Broadcast{event,payload,received_at}=>{if let Some(recorder)=breadth{recorder.observe(&event,&payload,&received_at)}if let Some(u)=universe{u.observe(&event,&payload,&received_at)?}for e in source.broadcast(&event,&payload,&received_at)?{accept(bank,&e)?}},
  Incoming::Failed(code)=>return Err(code.into()),
  Incoming::Joined{..}=>return Err("LIVE_STREAM_UNEXPECTED_REJOIN".into())
 }}
 Ok(())
}
pub fn main()->Result<()>{
 let args:Vec<_>=std::env::args().collect();if args.len()!=3||!["preflight","run"].contains(&args[1].as_str()){return Err("usage: moloch-live-runtime preflight|run /absolute/control.json".into())}
 let path=PathBuf::from(&args[2]);if !path.is_absolute(){return Err("LIVE_CONTROL_ABSOLUTE".into())}let c:Control=serde_json::from_value(load(&path)?)?;
 if !c.bank_name.starts_with("bank")||c.bank_name.contains(['/', '\\'])||c.predecessor_bank.as_ref().is_some_and(|p|p==&c.bank_name){return Err("LIVE_BANK_NAME".into())}
 if !c.root.is_absolute()||!c.credentials.is_absolute()||!c.owner_lock.is_absolute()||c.activation.maximum_committed!=c.policy["maximumCommitted"].as_str().unwrap_or("")||c.activation.maximum_market_cost!=c.policy["maximumMarketCost"].as_str().unwrap_or("")||c.policy["quantity"].as_u64()!=Some(c.activation.maximum_quantity as u64)||c.policy["entryStopAt"]!=iso(c.activation.entry_until_ms){return Err("LIVE_CONTROL_POLICY_BINDING".into())}
 private_dir(&c.root)?;private_dir(&c.root.join("receipts"))?;
 let lease=Arc::new(ExclusiveLock::acquire(&c.owner_lock)?);
 let mut read=transport(&c,Box::new(ReadOnlyAuthority),false)?;
 let marker=c.root.join("quiet-boundary.json");if !marker.exists(){read.reserve_legacy_migration_boundary(0,false)?;atomic_json(&marker,&json!({"at":iso(now()),"reason":"single-owner account budget migration"}))?;}
 let (a,p,o)=account(&mut read,&c.root)?;
 let recent=request(&mut read,&c.root,Endpoint::PortfolioFills)?;
 atomic_json(&c.root.join("preflight.json"),&json!({"at":iso(now()),"account":a,"positions":p,"openOrders":o,"recentFills":recent,"adoptsHoldings":false}))?;
 if args[1]=="preflight"{println!("Read-only preflight complete; no orders submitted");return Ok(())}
 let stop=c.root.join("EXECUTION_STOP");let global_stop=c.owner_lock.parent().ok_or("LIVE_LOCK_PARENT")?.join("EXECUTION_STOP");if stop.exists()||global_stop.exists(){return Err("LIVE_EXECUTION_STOP".into())}
 let result=(||->Result<()>{
  if now()>=c.activation.entry_until_ms&&!c.root.join(&c.bank_name).join("manifest.json").exists(){return Err("LIVE_ENTRY_ACTIVATION_EXPIRED".into())}
  let seed_path=c.root.join(if c.bank_name=="bank"{"account-seed.json".into()}else{format!("{}-account-seed.json",c.bank_name)});let seed=if seed_path.exists(){load(&seed_path)?}else{
   if !o.data["data"].as_array().ok_or("LIVE_OPEN_ORDERS")?.is_empty(){return Err("LIVE_FOREIGN_OPEN_ORDER".into())}
   if !c.all_markets{for row in p.data["positions"].as_array().ok_or("LIVE_POSITIONS")?{if row["quantity"].as_i64()!=Some(0)&&row["exchangeId"].as_str().is_some_and(|e|c.scope.exchanges.contains_key(e)){return Err("LIVE_INITIAL_SCOPE_ALREADY_HELD".into())}}}
   let mut v=json!({"runId":c.activation.run_id,"accountSnapshotHash":feedback::digest(&a)?,"availableCash":money(decimal_units(&a.data["myBalance"].to_string())?),"observedAt":a.received_at});
   if let Some(previous)=&c.predecessor_bank{v["continuation"]=moloch_live_runtime::handoff::verify_flat_predecessor(&c.root,previous,&c.policy)?;}
   if c.all_markets{v["excludedPositionsAtTransfer"]=p.data.clone();}
   atomic_json(&seed_path,&v)?;v
  };
  let mut bank=Session::open_live(&c.root.join(&c.bank_name),&c.policy,&seed)?;
  let authority=Arc::new(ProductionAuthority::new(c.activation.clone(),c.scope.clone(),vec![stop.clone(),global_stop.clone()],path.clone(),std::env::current_exe()?,lease.clone())?);
  let mut log=DispatchLog::open(&c.root.join("dispatch.ndjson"))?;log.require_resolved_attempts()?;
  // A durable official acknowledgment can be replayed after a crash before ledger commit.
  for (qid,v) in log.acknowledgments.clone(){let r:Receipt=serde_json::from_value(v)?;let q=quote(&bank,qid)?;let e=feedback::acknowledgment(&q,&r,&iso(now()))?;authority.bind_owned(e["actualOrderId"].as_str().ok_or("LIVE_ACTUAL_ID")?.into());accept(&mut bank,&e)?;}
  drop(read);let mut t=transport(&c,Box::new(SharedAuthority(authority.clone())),true)?;
  let state=t.admission_state()?;if state["state"]["unknown"].as_object().is_some_and(|m|!m.is_empty()){return Err("LIVE_QUEUE_UNKNOWN_ACCEPTANCE".into())}
  let mut universe=if c.all_markets{Some(Universe::new(&c.scope.exchanges,seed.get("excludedPositionsAtTransfer").ok_or("LIVE_UNIVERSE_SEED")?,c.policy["maximumMarkets"].as_u64().ok_or("LIVE_MARKET_CAP")? as usize)?)}else{None};
  // Previously owned holdings are filtered from the financial feed as well as discovery.
  let pairs:Value=if let Some(u)=&universe{u.pairs(&c.scope.exchanges)}else{c.scope.exchanges.iter().map(|(e,m)|json!({"exchangeId":e,"marketId":m})).collect::<Vec<_>>().into()};
  let start=now();let mut source_state=RestSource::new(&pairs,&c.scope.tournament_id,&iso(start))?;source_state.seed_trade_facts(bank.source_facts())?;
  let mut stream=Some(connect_stream(&mut t,&c,&mut bank,&mut source_state)?);
  authority.bind_stream(stream.as_ref().unwrap().failure_signal(),stream.as_ref().unwrap().deadline_ms);
  let breadth_path=c.root.join(format!("breadth-{}-{}.jsonl",std::process::id(),now()));
  let mut breadth=Recorder::new(&breadth_path).ok();
  if let Some(r)=&mut breadth{r.observe("stream_reset",&json!({"localBoundary":true,"reason":"confirmed_subscription"}),&iso(now()))}
  if let Some(u)=&mut universe{u.observe("stream_reset",&json!({"localBoundary":true}),&iso(now()))?;}
  let mut windows:std::collections::BTreeMap<String,u64>=c.scope.exchanges.keys().map(|e|(e.clone(),start.saturating_sub(60000))).collect();
  let mut cycles=0u64;
  loop{
   if stop.exists()||global_stop.exists(){return Err("LIVE_EXECUTION_STOP".into())}
   if now()>=stream.as_ref().ok_or("LIVE_STREAM_REQUIRED")?.deadline_ms{
    drop(stream.take());stream=Some(connect_stream(&mut t,&c,&mut bank,&mut source_state)?);
    authority.bind_stream(stream.as_ref().unwrap().failure_signal(),stream.as_ref().unwrap().deadline_ms);
    if let Some(r)=&mut breadth{r.observe("stream_reset",&json!({"localBoundary":true,"reason":"confirmed_resubscription"}),&iso(now()))}
    if let Some(u)=&mut universe{u.observe("stream_reset",&json!({"localBoundary":true}),&iso(now()))?;}
    for from in windows.values_mut(){*from=now().saturating_sub(60000)}
   }
   drain_stream(stream.as_ref().ok_or("LIVE_STREAM_REQUIRED")?,&mut bank,&mut source_state,&mut breadth,&mut universe)?;
   reconcile(&mut t,&c,&mut bank,&mut log)?;
   accept(&mut bank,&json!({"type":"tick","receivedAt":iso(now())}))?;
   let selected=if let Some(u)=&universe{u.select(&bank.engine().required_exchanges())?}else{c.scope.exchanges.keys().cloned().collect()};
   for ex in &selected{let market=&c.scope.exchanges[ex];let from=windows[ex];let next=source(&mut t,&c,&mut bank,&mut source_state,ex,market,from)?;windows.insert(ex.clone(),next);drain_stream(stream.as_ref().ok_or("LIVE_STREAM_REQUIRED")?,&mut bank,&mut source_state,&mut breadth,&mut universe)?;dispatch(&mut t,&c,&mut bank,&mut log,&authority,&mut source_state)?;}
   cycles+=1;
   atomic_json(&c.root.join("status.json"),&json!({"at":iso(now()),"pid":std::process::id(),"mode":"actual-contest-trading","runId":c.activation.run_id,"bank":c.bank_name,"cycles":cycles,"realtimeHealthy":stream.as_ref().is_some_and(|s|s.healthy()),"universe":universe.as_ref().map(Universe::status),"selectedReadScopes":selected,"sourceScopes":source_state.scopes(),"breadthCapture":{"path":breadth_path,"state":breadth.as_ref().map(Recorder::status)},"report":bank.engine().report()?,"queue":t.admission_state()?,"actualOrders":log.acknowledgments.len(),"terminalOrders":log.closed.len(),"entryUntil":iso(c.activation.entry_until_ms),"profitabilityEstablished":false}))?;
   if cycles%20==0{t.compact()?;}
   std::thread::sleep(Duration::from_millis(if selected.is_empty(){600}else{100}));
  }
 })();
 if let Err(e)=&result{atomic_json(&stop,&json!({"at":iso(now()),"reason":e.to_string(),"action":"no automatic write retry or activation renewal"}))?;atomic_json(&c.root.join("status.json"),&json!({"at":iso(now()),"pid":std::process::id(),"mode":"stopped","reason":e.to_string(),"profitabilityEstablished":false}))?;}
 result
}
