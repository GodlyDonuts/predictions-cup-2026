use baal_transport_service::{self as api,Authority,Config,Endpoint,Order,Permit,Receipt,RequestSpec,Scope,SystemClock,Transport,ReadOnlyAuthority,credentials::UnixFileCredentials,decimal_units};
use moloch_live_runtime::{authority::{Activation,ProductionAuthority,SharedAuthority,SnapshotSet},dispatch::DispatchLog,feedback::{self,Result}};
use moloch_replay::{session::{Session,ExclusiveLock,atomic_json},source::RestSource,money};
use moloch_replay::realtime::{Auth,Incoming,Realtime};
use moloch_live_runtime::breadth::Recorder;
use moloch_live_runtime::universe::Universe;
use moloch_live_runtime::reconciliation::{self,Reconciler};
use moloch_live_runtime::stream_recovery::{StreamFailure,RecoveryBudget,RECOVERY_WINDOW_MS,RECOVERY_MAXIMUM_ATTEMPTS};
use serde::{Serialize,Deserialize};
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::{fs,path::{Path,PathBuf},sync::Arc,time::{SystemTime,UNIX_EPOCH,Duration}};
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Control{#[serde(default)]daily_entry_windows:Option<moloch_live_runtime::daily_windows::DailyEntryWindows>,activation:Activation,scope:Scope,policy:Value,root:PathBuf,credentials:PathBuf,owner_lock:PathBuf,account_scope:String,#[serde(default)]all_markets:bool,#[serde(default="default_bank")]bank_name:String,#[serde(default)]predecessor_bank:Option<String>,#[serde(default)]manage_existing_positions:bool}
fn default_bank()->String{"bank".into()}
const MAX_REALTIME_FRAMES_PER_DRAIN:usize=64;
fn now()->u64{SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64}
fn iso(t:u64)->String{chrono::DateTime::from_timestamp_millis(t as i64).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis,true)}
fn hash(v:&Value)->String{format!("{:x}",Sha256::digest(serde_json::to_vec(v).unwrap()))}
fn load(p:&Path)->Result<Value>{Ok(serde_json::from_slice(&fs::read(p)?)?)}
fn private_dir(p:&Path)->Result<()>{use std::os::unix::fs::PermissionsExt;fs::create_dir_all(p)?;let m=fs::symlink_metadata(p)?;if m.file_type().is_symlink()||!m.is_dir(){return Err("LIVE_DIRECTORY".into())}fs::set_permissions(p,fs::Permissions::from_mode(0o700))?;Ok(())}
fn transport(c:&Control,auth:Box<dyn Authority>,writes:bool)->Result<Transport>{Ok(Transport::new_with_read_budget(Config{queue_dir:c.root.join("account-queue"),account_scope:c.account_scope.clone(),scope:c.scope.clone(),maximum_pages:20,writes_enabled:writes},Box::new(UnixFileCredentials::approved_file(c.credentials.clone())?),Box::new(SystemClock),auth,100,600)?.with_full_account_observation())}
fn accept(bank:&mut Session,event:&Value)->Result<()>{let mut e=event.clone();e["processedAt"]=json!(iso(now()));bank.accept("moloch-live",bank.last_source_sequence("moloch-live")+1,&e)?;Ok(())}
fn ensure_entry_window(c:&Control,bank:&mut Session,authority:&ProductionAuthority)->Result<()>{
 let Some(cfg)=&c.daily_entry_windows else{return Ok(())};let at=now();cfg.validate(&c.activation,at)?;
 let current=moloch_live_runtime::daily_windows::clock(bank.engine().policy()["entryStopAt"].as_str().ok_or("DAILY_POLICY_STOP")?)?;
 let end=cfg.end_at(at)?;
 if at>=cfg.campaign_end_ms{return Ok(())}
 if current>end{return Err("DAILY_POLICY_AHEAD".into())}
 if end>current{
  let stamp=iso(at);let event=json!({"type":"live_entry_window","source":"moloch_live_dispatcher","receivedAt":stamp,"processedAt":stamp,"previousEntryStopAt":iso(current),"entryStopAt":iso(end),"mandate":cfg.mandate(&c.activation)});
  bank.accept("moloch-live",bank.last_source_sequence("moloch-live")+1,&event)?;
 }
 if authority.entry_until_ms()!=end||end==cfg.anchor_ms{authority.bind_entry_window(bank,cfg,at)?;}Ok(())
}
fn accept_source_events(bank:&mut Session,events:&[Value])->Result<()>{
 use moloch_replay::session::{BatchEntry,SESSION_BATCH_MAX_EVENTS};
 for chunk in events.chunks(SESSION_BATCH_MAX_EVENTS){
  let first=bank.last_source_sequence("moloch-live").checked_add(1).ok_or("LIVE_SOURCE_SEQUENCE_RANGE")?;
  let prepared:Vec<_>=chunk.iter().map(|event|{let mut e=event.clone();e["processedAt"]=json!(iso(now()));e}).collect();
  let entries:Vec<_>=prepared.iter().enumerate().map(|(index,event)|Ok(BatchEntry{source_id:"moloch-live",sequence:first.checked_add(index as u64).ok_or("LIVE_SOURCE_SEQUENCE_RANGE")?,event})).collect::<Result<_>>()?;
  let outcome=bank.accept_batch(&entries)?;
  if outcome.appended_rows!=entries.len()||outcome.accepted.len()!=entries.len()||outcome.accepted.iter().any(|accepted|!*accepted){return Err("LIVE_SOURCE_BATCH_PREFIX".into())}
 }Ok(())
}
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
fn reconcile(t:&mut Transport,c:&Control,bank:&mut Session,log:&mut DispatchLog,cache:&mut Reconciler)->Result<reconciliation::Cycle>{
 log.require_resolved_attempts()?;
 let pending:Vec<_>=log.acknowledgments.iter().filter(|(q,_)|!log.closed.contains(q)).map(|(q,_)|*q).collect();
 let mut owned=vec![];
 for qid in pending{let q=quote(bank,qid)?;let actual=bank.engine().live_order_bindings().get(&qid).ok_or("LIVE_ACK_BINDING_MISSING")?;
  owned.push(reconciliation::owned_from_quote(&q,actual,log.cancel_acknowledged.contains(&qid),false)?);
 }
 struct Live<'a>{t:&'a mut Transport,c:&'a Control,bank:&'a mut Session,log:&'a mut DispatchLog}
 impl reconciliation::Backend for Live<'_>{
  fn now_ms(&self)->u64{now()}
  fn read(&mut self,ep:Endpoint)->Result<Receipt>{request(self.t,&self.c.root,ep)}
  fn quote(&self,id:u64)->Result<Value>{quote(self.bank,id)}
  fn apply(&mut self,e:&Value)->Result<()>{accept(self.bank,e)}
  fn close(&mut self,id:u64,e:&Value,order:&Receipt,fills:&Receipt)->Result<()>{accept(self.bank,e)?;self.log.append(json!({"kind":"closed","qid":id,"orderReceiptHash":feedback::digest(order)?,"fillReceiptHash":feedback::digest(fills)?}))}
 }
 cache.cycle(&c.scope,&owned,false,&mut Live{t,c,bank,log})
}
fn source(t:&mut Transport,c:&Control,bank:&mut Session,s:&mut RestSource,ex:&str,market:&str,from:u64)->Result<u64>{
 let from=s.recovery_start_ms(market).map_or(from,|recovery|from.min(recovery));let to=now();let tape=request(t,&c.root,Endpoint::Trades{exchange_id:ex.into(),from:iso(from),to:iso(to)})?;
 let wrap=|r:&Receipt,kind:&str|json!({"source":"official_sig_api_v1","kind":kind,"tournamentId":c.scope.tournament_id,"marketId":market,"exchangeId":ex,"endpoint":r.endpoint,"status":r.status,"requestStartedAt":r.request_started_at,"receivedAt":r.received_at,"data":r.data});
 accept_source_events(bank,&s.accept(&wrap(&tape,"trades"))?)?;
 let book=request(t,&c.root,Endpoint::OrderBook{market_id:market.into(),depth:20})?;
 accept_source_events(bank,&s.accept(&wrap(&book,"book"))?)?;Ok(to.saturating_sub(2000))
}
fn snapshots(t:&mut Transport,c:&Control,q:&Value)->Result<SnapshotSet>{
 let (account,positions,open_orders)=account(t,&c.root)?;
 let cash_balance=request(t,&c.root,Endpoint::Account)?;
 let market_id=q["marketId"].as_str().ok_or("LIVE_MARKET_ID")?.to_owned();
 let market=request(t,&c.root,Endpoint::Market{market_id:market_id.clone()})?;
 let contract=request(t,&c.root,Endpoint::Nodes{market_id:market_id.clone()})?;
 let book=request(t,&c.root,Endpoint::OrderBook{market_id,depth:20})?;
 Ok(SnapshotSet{account,cash_balance,positions,open_orders,market,contract,book})
}
fn accept_fresh_book(bank:&mut Session,c:&Control,q:&Value,r:&Receipt,source:&mut RestSource)->Result<Vec<Value>>{
 let wrapped=json!({"source":"official_sig_api_v1","kind":"book","tournamentId":c.scope.tournament_id,"marketId":q["marketId"],"exchangeId":q["exchangeId"],"endpoint":r.endpoint,"status":r.status,"requestStartedAt":r.request_started_at,"receivedAt":r.received_at,"data":r.data});
 let events=source.accept(&wrapped)?;accept_source_events(bank,&events)?;Ok(events)
}
fn physical_order(c:&Control,q:&Value)->Result<(Order,RequestSpec,Permit)>{
 let qid=q["id"].as_u64().ok_or("LIVE_QUOTE_ID")?;
 let order=Order{idempotency_key:format!("{}:{qid}",c.activation.run_id),exchange_id:q["exchangeId"].as_str().ok_or("LIVE_SCOPE")?.into(),side:match q["side"].as_str(){Some("YES")=>"yes",Some("NO")=>"no",_=>return Err("LIVE_SIDE".into())}.into(),action:q["action"].as_str().ok_or("LIVE_ACTION")?.into(),quantity:u32::try_from(q["quantity"].as_u64().ok_or("LIVE_SIZE")?)?,price:q["price"].as_str().ok_or("LIVE_PRICE")?.into(),expiration_date:iso(q["expiresAt"].as_u64().ok_or("LIVE_EXPIRY")?),tournament_id:c.scope.tournament_id.clone()};
 let request_spec=spec(&order)?;let p=Permit{run_id:c.activation.run_id.clone(),intent_id:qid.to_string(),owner:"Codex".into(),expires_at_ms:now()+15000,maximum_quantity:order.quantity,maximum_cost:money(decimal_units(&order.price)?.checked_mul(order.quantity as i128).ok_or("LIVE_COST_OVERFLOW")?)};
 Ok((order,request_spec,p))
}
fn submit_order(t:&mut Transport,c:&Control,bank:&mut Session,log:&mut DispatchLog,authority:&Arc<ProductionAuthority>,q:&Value,facts:SnapshotSet)->Result<()>{
 let qid=q["id"].as_u64().ok_or("LIVE_QUOTE_ID")?;let(order,request_spec,p)=physical_order(c,q)?;
 authority.prepare(bank,qid,request_spec.clone(),p.clone(),facts)?;
 if let Err(e)=authority.authorize(&c.scope,&request_spec,&p,false,now()){authority.clear_ready();atomic_json(&c.root.join("deferred.json"),&json!({"at":iso(now()),"qid":qid,"code":e.code}))?;return Ok(())}
 if log.intents.contains_key(&qid)||!log.never_attempted(qid){return Err("LIVE_EXISTING_UNRESOLVED_INTENT".into())}
 log.append(json!({"kind":"intent","qid":qid,"quote":q,"spec":request_spec,"permit":p,"at":iso(now())}))?;
 log.append(json!({"kind":"attempt","qid":qid,"at":iso(now())}))?;
 let ack=match t.place(order,p){Ok(r)=>r,Err(e)=>{log.append(json!({"kind":"error","qid":qid,"error":e,"at":iso(now())}))?;return Err("LIVE_WRITE_NOT_CONFIRMED_NO_RETRY".into())}};
 record_receipt(&c.root,&ack)?;log.append(json!({"kind":"ack","qid":qid,"receipt":ack}))?;
 let event=feedback::acknowledgment(q,&ack,&iso(now()))?;authority.bind_owned(event["actualOrderId"].as_str().ok_or("LIVE_ACTUAL_ID")?.into());accept(bank,&event)?;authority.clear_ready();Ok(())
}
fn queue_has_unknown(t:&mut Transport)->Result<bool>{
 queue_unknown_from_state(&t.admission_state()?)
}
fn queue_unknown_from_state(state:&Value)->Result<bool>{
 Ok(!state["state"]["unknown"].as_object().ok_or("LIVE_QUEUE_UNKNOWN_SHAPE")?.is_empty())
}
fn native_head(c:&Control)->Result<[u8;32]>{Ok(Sha256::digest(fs::read(c.root.join(&c.bank_name).join("head.json"))?).into())}
fn require_stream(stream:&Realtime)->Result<()>{
 if !stream.healthy(){return Err(StreamFailure::new(stream.exit_reason().unwrap_or("STREAM_WORKER_EXIT_UNEXPLAINED")).into())}
 if now()>=stream.deadline_ms{return Err(StreamFailure::new("STREAM_DEADLINE_REACHED").into())}Ok(())
}
fn refresh_controls(c:&Control,authority:&ProductionAuthority,stream:&Realtime)->Result<()>{
 require_stream(stream)?;
 if !authority.can_read(&c.scope){return Err("LIVE_EXIT_REFRESH_CONTROL_OR_STOP".into())}Ok(())
}
fn refresh_safety(c:&Control,bank:&Session,log:&DispatchLog,authority:&ProductionAuthority,stream:&Realtime,q:&Value,global_unknown:bool)->Result<moloch_live_runtime::exit_refresh::Safety>{
 refresh_controls(c,authority,stream)?;log.require_resolved_attempts()?;
 let qid=q["id"].as_u64().ok_or("LIVE_QUOTE_ID")?;let ex=q["exchangeId"].as_str().ok_or("LIVE_SCOPE")?;
 let side=match q["side"].as_str(){Some("YES")=>moloch_replay::Side::Yes,Some("NO")=>moloch_replay::Side::No,_=>return Err("LIVE_SIDE".into())};
 let records=bank.engine().order_records();let mut physical=false;let mut reserved=0u64;
 for old in &records{let id=old["id"].as_u64().ok_or("LIVE_QUOTE_ID")?;if old["exchangeId"]!=ex||id==qid{continue}
  if (log.attempted.contains(&id)||log.acknowledgments.contains_key(&id))&&!log.closed.contains(&id){physical=true;}
  if old["side"]==q["side"]&&old["action"]=="sell"&&old["retired"]==false&&old["expiresAt"].as_u64().ok_or("LIVE_EXPIRY")?>now(){
   reserved=reserved.checked_add(old["quantity"].as_u64().ok_or("LIVE_SIZE")?.checked_sub(old["filled"].as_u64().ok_or("LIVE_FILLED")?).ok_or("LIVE_FILLED")?).ok_or("LIVE_SIZE_OVERFLOW")?;
  }
 }
 // A missing native record for an ACK is integrity failure, never absent liability.
 for id in log.acknowledgments.keys().filter(|id|!log.closed.contains(id)){if !records.iter().any(|q|q["id"].as_u64()==Some(*id)){return Err("LIVE_ACK_QUOTE_MISSING".into())}}
 let lifecycle=json!({"manifest":bank.manifest_hash(),"policy":bank.engine().policy(),"economics":bank.engine().report()?,"fills":bank.engine().fills(),"dispatch":log.proof(0)});
 Ok(moloch_live_runtime::exit_refresh::Safety{never_attempted:log.never_attempted(qid),has_intent:log.intents.contains_key(&qid),unresolved_attempts:false,global_unknown,stop:false,stream_healthy:true,owned_quantity:bank.engine().wallet.quantity(ex,side),reserved_sell_quantity:reserved,same_exchange_pending_physical_order:physical,lifecycle_fingerprint:Sha256::digest(serde_json::to_vec(&lifecycle)?).into(),native_head_fingerprint:native_head(c)?})
}
fn refresh_deferred(c:&Control,stage:&str,error:&dyn std::fmt::Display)->Result<()>{atomic_json(&c.root.join("exit-refresh.json"),&json!({"at":iso(now()),"stage":stage,"code":error.to_string(),"expiryExtended":false,"writeAttempted":false}))}
fn exit_recovery_from(source:&RestSource,market:&str,prior_window:u64)->Option<u64>{source.recovery_start_ms(market).map(|from|from.min(prior_window))}
fn refresh_exit(t:&mut Transport,c:&Control,bank:&mut Session,log:&mut DispatchLog,authority:&Arc<ProductionAuthority>,source:&mut RestSource,stream:&Realtime,old:&Value,hint:&moloch_live_runtime::exit_refresh::ReadHint,flow_started:u64,maximum_qid_before:u64,unknown:bool,prior_window:u64)->Result<()>{
 use moloch_live_runtime::exit_refresh::{RefreshFacts,BookAcceptance,RefreshPlan,RefreshStage};
 refresh_controls(c,authority,stream)?;
 // A fresh book alone cannot unblock a source after a real delivery gap. Recover
 // its exact missing trade interval before starting the timed seven-fact packet.
 let market=old["marketId"].as_str().ok_or("LIVE_MARKET_ID")?;
 if let Some(from)=exit_recovery_from(source,market,prior_window){
  let ex=old["exchangeId"].as_str().ok_or("LIVE_SCOPE")?;let through=now();
  let tape=request(t,&c.root,Endpoint::Trades{exchange_id:ex.into(),from:iso(from),to:iso(through)})?;
  let wrapped=json!({"source":"official_sig_api_v1","kind":"trades","tournamentId":c.scope.tournament_id,"marketId":market,"exchangeId":ex,"endpoint":tape.endpoint,"status":tape.status,"requestStartedAt":tape.request_started_at,"receivedAt":tape.received_at,"data":tape.data});
  accept_source_events(bank,&source.accept(&wrapped)?)?;
  let scopes=source.scopes();let scope=scopes.as_array().ok_or("LIVE_SOURCE_SCOPES")?.iter().find(|s|s["marketId"]==market).ok_or("LIVE_SOURCE_SCOPE")?;
  if !scope["tapeReady"].as_bool().ok_or("LIVE_SOURCE_TAPE_STATE")?{refresh_deferred(c,"coverage_ineligible",&"COMPLETE_TARGET_TAPE_REQUIRED")?;return Ok(())}
  refresh_controls(c,authority,stream)?;
 }
 let facts=snapshots(t,c,old)?;
 let bound=match RefreshFacts::new(&c.scope,hint,[facts.account.clone(),facts.positions.clone(),facts.open_orders.clone(),facts.cash_balance.clone(),facts.market.clone(),facts.contract.clone(),facts.book.clone()],now()){
  Ok(v)=>v,Err(e) if matches!(e.to_string().as_str(),"EXIT_REFRESH_FACTS_STALE"|"EXIT_REFRESH_BOOK_STALE")=>{refresh_deferred(c,"facts_ineligible",e.as_ref())?;return Ok(())},Err(e)=>return Err(e)
 };
 let started=now();let events=accept_fresh_book(bank,c,old,&facts.book,source)?;let completed=now();let head=native_head(c)?;
 let acceptance=match BookAcceptance::from_events(&bound,&events,true,head,started,completed){
  Ok(v)=>v,Err(e)=>{refresh_deferred(c,"book_ineligible",e.as_ref())?;return Ok(())}
 };
 // Every I/O/control failure is propagated; only pure helper denials defer.
 let candidates:Vec<_>=bank.engine().order_records().into_iter().filter(|q|q["id"].as_u64().is_some_and(|id|id>maximum_qid_before&&id!=hint.old_qid())&&q["retired"]==false&&q["marketId"]==old["marketId"]&&q["exchangeId"]==old["exchangeId"]&&q["side"]==old["side"]&&q["action"]=="sell"&&q["executionStyle"]=="inventoryAON"&&q["inventoryExitReason"]=="maximumAge"&&q["filled"].as_u64()==Some(0)&&q.get("cancelEffectiveAt").is_none_or(Value::is_null)).collect();
 let mut safety=std::collections::BTreeMap::new();
 for q in &candidates{if q["exchangeId"]==old["exchangeId"]&&q["side"]==old["side"]&&q["action"]=="sell"{safety.insert(q["id"].as_u64().ok_or("LIVE_QUOTE_ID")?,refresh_safety(c,bank,log,authority,stream,q,unknown)?);}}
 let plan=match RefreshPlan::select(&bound,&acceptance,&candidates,maximum_qid_before,flow_started,completed,now(),|id|*safety.get(&id).expect("validated native candidate safety")){
  Ok(v)=>v,Err(e)=>{refresh_deferred(c,"candidate_ineligible",e.as_ref())?;return Ok(())}
 };
 let qid=plan.quote()["id"].as_u64().ok_or("LIVE_QUOTE_ID")?;
 loop{
  let current=quote(bank,qid)?;let current_safety=refresh_safety(c,bank,log,authority,stream,&current,unknown)?;
  match plan.stage(&current,&bound,&c.scope,current_safety,now()){
   Ok(RefreshStage::WaitUntil(at))=>{std::thread::sleep(Duration::from_millis(at.saturating_sub(now()).clamp(1,100)));}
   Ok(RefreshStage::Ready)=>break,
   Err(e)=>{refresh_deferred(c,"wait_ineligible",e.as_ref())?;return Ok(())}
  }
 }
 // Query the real shared queue once after waiting, never synthesize empty unknowns.
 let unknown=queue_has_unknown(t)?;let current=quote(bank,qid)?;
 match plan.stage(&current,&bound,&c.scope,refresh_safety(c,bank,log,authority,stream,&current,unknown)?,now()){
  Ok(RefreshStage::Ready)=>{},Ok(_)=>{refresh_deferred(c,"final_ineligible",&"ACTIVATION_NOT_READY")?;return Ok(())},Err(e)=>{refresh_deferred(c,"final_ineligible",e.as_ref())?;return Ok(())}
 }
 // Rebuild complete physical identity from this NEW frozen quote only after wait.
 submit_order(t,c,bank,log,authority,plan.quote(),facts)
}
fn latest_refresh_hints(orders:&[Value],at:u64,eligible:impl Fn(u64)->bool)->Result<std::collections::BTreeMap<String,Value>>{
 let mut latest=std::collections::BTreeMap::<String,Value>::new();
 for q in orders{
  if q["action"]!="sell"||q["inventoryExitReason"]!="maximumAge"||q["executionStyle"]!="inventoryAON"||q["filled"].as_u64()!=Some(0)||q.get("cancelEffectiveAt").is_some_and(|v|!v.is_null())||q["expiresAt"].as_u64().is_none_or(|t|t>at){continue}
  let id=q["id"].as_u64().ok_or("LIVE_QUOTE_ID")?;if !eligible(id){continue}
  let key=format!("{}:{}:sell",q["exchangeId"].as_str().ok_or("LIVE_SCOPE")?,q["side"].as_str().ok_or("LIVE_SIDE")?);
  if latest.get(&key).is_none_or(|old|old["id"].as_u64().unwrap()<id){latest.insert(key,q.clone());}
 }Ok(latest)
}
fn dispatch(t:&mut Transport,c:&Control,bank:&mut Session,log:&mut DispatchLog,authority:&Arc<ProductionAuthority>,source:&mut RestSource,allow_new_submissions:bool,preflight_cursor:&mut Option<String>,stream:&Realtime,windows:&std::collections::BTreeMap<String,u64>)->Result<()>{
 let orders=bank.engine().order_records();
 let flow_started=now();let maximum_qid_before=orders.iter().filter_map(|q|q["id"].as_u64()).max().unwrap_or(0);
 let mut hints=std::collections::BTreeMap::<String,(Value,moloch_live_runtime::exit_refresh::ReadHint)>::new();
 let mut refresh_unknown=false;
 if allow_new_submissions&&orders.iter().any(|q|q["action"]=="sell"&&q["inventoryExitReason"]=="maximumAge"&&q["expiresAt"].as_u64().is_some_and(|t|t<=flow_started)&&q["id"].as_u64().is_some_and(|id|log.never_attempted(id)&&!log.intents.contains_key(&id))){
  let unknown=queue_has_unknown(t)?;refresh_unknown=unknown;
  for old in latest_refresh_hints(&orders,flow_started,|id|log.never_attempted(id)&&!log.intents.contains_key(&id))?.values(){
   if orders.iter().any(|q|q["exchangeId"]==old["exchangeId"]&&q["side"]==old["side"]&&q["action"]=="sell"&&q["retired"]==false&&q["expiresAt"].as_u64().is_some_and(|t|t>flow_started)){continue}
   let safety=refresh_safety(c,bank,log,authority,stream,old,unknown)?;
   if let Ok(hint)=moloch_live_runtime::exit_refresh::ReadHint::from_expired(old,safety,flow_started){let key=format!("{}:{}:sell",hint.exchange(),old["side"].as_str().ok_or("LIVE_SIDE")?);hints.insert(key,(old.clone(),hint));}
  }
 }
 // Cancellations and reconciliation take priority over new purchases.
 for old in &orders {
  let qid=old["id"].as_u64().ok_or("LIVE_QUOTE_ID")?;if bank.engine().is_package_order(qid)||old["retired"]==true{continue}
  let expired=old["expiresAt"].as_u64().is_none_or(|t|t<=now());let canceled=old["cancelEffectiveAt"].as_u64().is_some_and(|t|t<=now());
  if !expired&&!canceled{continue}
  if log.never_attempted(qid){retire_never_sent(bank,log,old)?;continue}
  if expired||log.closed.contains(&qid)||log.cancel_acknowledged.contains(&qid){continue} // Wait for authoritative terminal GET; never repeat a confirmed cancellation.
  let actual=bank.engine().live_order_bindings().get(&qid).ok_or("LIVE_CANCEL_BINDING")?.clone();
  let p=Permit{run_id:c.activation.run_id.clone(),intent_id:format!("{qid}-cancel"),owner:"Codex".into(),expires_at_ms:now()+10000,maximum_quantity:c.activation.maximum_quantity,maximum_cost:c.activation.maximum_committed.clone()};
  let spec=RequestSpec{method:"DELETE".into(),endpoint:format!("/orders/{actual}"),body:None,body_json:None};
  let facts=snapshots(t,c,old)?;authority.prepare(bank,qid,spec.clone(),p.clone(),facts)?;
  if let Err(e)=authority.authorize(&c.scope,&spec,&p,false,now()){authority.clear_ready();atomic_json(&c.root.join("deferred.json"),&json!({"at":iso(now()),"qid":qid,"code":e.code}))?;return Ok(())}
  log.append(json!({"kind":"cancel_attempt","qid":qid,"spec":spec,"permit":p,"at":iso(now())}))?;
  let ack=match t.cancel(actual.clone(),p){Ok(r)=>r,Err(e)=>{
   log.append(json!({"kind":"error","operation":"cancel","qid":qid,"error":e,"at":iso(now())}))?;authority.clear_ready();
   if moloch_live_runtime::read_recovery::reconcilable_cancel_rejection(&e){
    let observed=request(t,&c.root,Endpoint::OwnedOrder{order_id:actual.clone()})?;let filled=request(t,&c.root,Endpoint::OwnedFills{order_id:actual.clone()})?;
    for e in feedback::fills(old,&actual,&c.scope.tournament_id,&filled,&iso(now()))?{accept(bank,&e)?}
    if let Some(e)=feedback::terminal(old,&actual,&c.scope.tournament_id,&observed,&filled,&iso(now()))?{accept(bank,&e)?;log.append(json!({"kind":"closed","qid":qid,"orderReceiptHash":feedback::digest(&observed)?,"fillReceiptHash":feedback::digest(&filled)?,"afterRejectedCancel":true}))?;return Ok(())}
   }return Err("LIVE_CANCEL_REJECTED_RECONCILIATION_REQUIRED".into())
  }};record_receipt(&c.root,&ack)?;log.append(json!({"kind":"cancel_ack","qid":qid,"receipt":ack}))?;authority.clear_ready();return Ok(())
 }
 if !allow_new_submissions||bank.engine().package_blocks_submissions(){return Ok(())}
 let mut preflight_budget=moloch_live_runtime::scheduling::PreflightBudget::default();
 if !hints.is_empty(){
  let keys:Vec<_>=hints.keys().cloned().collect();let index=moloch_live_runtime::scheduling::preflight_rotation(&keys,preflight_cursor.as_deref())[0];let key=&keys[index];
  let(old,hint)=hints.get(key).ok_or("LIVE_EXIT_REFRESH_HINT")?;
  if !preflight_budget.try_start(){return Ok(())}*preflight_cursor=Some(key.clone());
  refresh_exit(t,c,bank,log,authority,source,stream,old,hint,flow_started,maximum_qid_before,refresh_unknown,*windows.get(hint.exchange()).ok_or("LIVE_RECOVERY_WINDOW_MISSING")?)?;
  return Ok(()) // Ineligible refresh still consumes this pass; never another packet.
 }
 // At most one new physical order per pass; subsequent fills change funding.
 let quotes=bank.engine().order_records();
 let keys:Vec<String>=quotes.iter().map(|q|format!("{}:{}:{}",q["exchangeId"].as_str().unwrap_or(""),q["side"].as_str().unwrap_or(""),q["action"].as_str().unwrap_or(""))).collect();
 for index in moloch_live_runtime::scheduling::preflight_rotation(&keys,preflight_cursor.as_deref()){let q=quotes[index].clone();
  let qid=q["id"].as_u64().ok_or("LIVE_QUOTE_ID")?;if bank.engine().is_package_order(qid){continue}
  let prefetch_started=now();
  let early_exit_prefetch=moloch_live_runtime::dispatch_window::may_prefetch(&q,log.never_attempted(qid),log.intents.contains_key(&qid),prefetch_started);
  if q["retired"]==true||!log.never_attempted(qid)||(!early_exit_prefetch&&q["activationAt"].as_u64().is_none_or(|t|t>prefetch_started))||q["expiresAt"].as_u64().is_none_or(|t|t<=prefetch_started+5000)||q["cancelEffectiveAt"].as_u64().is_some(){continue}
  if q["action"]=="buy"&&now()>=authority.entry_until_ms(){retire_never_sent(bank,log,&q)?;continue}
  // Covered maximum-age exits may spend their activation delay fetching these
  // same read-only facts. No intent/attempt or HTTP write exists at this stage.
  if !preflight_budget.try_start(){return Ok(())}*preflight_cursor=Some(keys[index].clone());
  if early_exit_prefetch{let _=atomic_json(&c.root.join("exit-prefetch.json"),&json!({"at":iso(prefetch_started),"qid":qid,"stage":"started","createdAtMs":q["createdAt"],"activationAtMs":q["activationAt"],"expiresAtMs":q["expiresAt"],"prefetchStartedAtMs":prefetch_started,"expiryExtended":false}));}
  let facts=snapshots(t,c,&q)?;accept_fresh_book(bank,c,&q,&facts.book,source)?;
  let current=quote(bank,qid)?;let final_at=now();
  let native_active=current["activationAt"].as_u64().is_some_and(|t|t<=final_at);
  if early_exit_prefetch{let _=atomic_json(&c.root.join("exit-prefetch.json"),&json!({"at":iso(final_at),"qid":qid,"stage":"facts_complete","createdAtMs":q["createdAt"],"activationAtMs":q["activationAt"],"expiresAtMs":q["expiresAt"],"prefetchStartedAtMs":prefetch_started,"prefetchElapsedMs":final_at.saturating_sub(prefetch_started),"remainingLifetimeMs":current["expiresAt"].as_u64().map(|t|t.saturating_sub(final_at)),"nativeActive":native_active,"retired":current["retired"],"cancelPending":current["cancelEffectiveAt"].is_u64(),"expiryExtended":false}));}
  if current["retired"]==true||current["cancelEffectiveAt"].is_u64()||current["expiresAt"].as_u64().is_none_or(|v|v<=final_at+3000){if current["retired"]==false{retire_never_sent(bank,log,&current)?}continue}
  // A fast preflight never authorizes an early order. The final production
  // authority independently checks activation again at the physical boundary.
  if !native_active{continue}
  submit_order(t,c,bank,log,authority,&current,facts)?;return Ok(())
 }Ok(())
}
fn connect_stream(t:&mut Transport,c:&Control,bank:&mut Session,source:&mut RestSource)->Result<(Realtime,String)>{
 let marker=c.root.join("stream-auth-admission.json");let at=now();
 if marker.exists()&&load(&marker)?["atMs"].as_u64().is_none_or(|old|at<old+60000){return Err("LIVE_AUTH_MINT_BACKOFF".into())}
 atomic_json(&marker,&json!({"atMs":at}))?;
 let material=t.realtime_token()?;let stream=Realtime::connect(Auth::from_official_response(material)?,&c.scope.tournament_id)?;
 let joined_at=match stream.incoming.recv_timeout(Duration::from_secs(12)){
  Ok(Incoming::Joined{received_at})=>{accept_source_events(bank,&source.subscribed(&received_at)?)?;received_at}
  _=>return Err("LIVE_STREAM_JOIN_FAILED".into())
 };Ok((stream,joined_at))
}
fn drain_stream(stream:&Realtime,bank:&mut Session,source:&mut RestSource,breadth:&mut Option<Recorder>,universe:&mut Option<Universe>)->Result<()>{
 require_stream(stream)?;
 for _ in 0..MAX_REALTIME_FRAMES_PER_DRAIN{require_stream(stream)?;let Ok(input)=stream.incoming.try_recv()else{break};match input{
  Incoming::Broadcast{event,payload,received_at}=>{if let Some(recorder)=breadth{recorder.observe(&event,&payload,&received_at)}if let Some(u)=universe{u.observe(&event,&payload,&received_at)?}accept_source_events(bank,&source.broadcast(&event,&payload,&received_at)?)?;},
  Incoming::Failed(code)=>return Err(StreamFailure::new(code).into()),
  Incoming::Joined{..}=>return Err("LIVE_STREAM_UNEXPECTED_REJOIN".into())
 }}
 Ok(())
}
fn recovery_budget(path:&Path,at:u64)->Result<RecoveryBudget>{
 let budget:RecoveryBudget=match fs::symlink_metadata(path){
  Ok(m)=>{if !m.is_file()||m.file_type().is_symlink()||m.len()>4096{return Err("LIVE_RECOVERY_BUDGET_FILE".into())}let bytes=fs::read(path)?;if bytes.len()>4096{return Err("LIVE_RECOVERY_BUDGET_FILE".into())}serde_json::from_slice(&bytes)?},
  Err(e)if e.kind()==std::io::ErrorKind::NotFound=>{if path.with_file_name("stream-recovery.json").exists(){return Err("LIVE_RECOVERY_BUDGET_MISSING_AFTER_USE".into())}RecoveryBudget::new(at)?},Err(e)=>return Err(e.into()),
 };budget.validate(at)?;Ok(budget)
}
fn recovery_queue_clear(state:&Value)->Result<()>{
 if queue_unknown_from_state(state)?{return Err("LIVE_STREAM_RECOVERY_UNKNOWN_ACCEPTANCE".into())}
 let s=&state["state"];if !s.get("active").is_some_and(Value::is_null)||s["manual_review"].as_bool()!=Some(false){return Err("LIVE_STREAM_RECOVERY_QUEUE_NOT_QUIET".into())}Ok(())
}
fn recovery_not_before(budget:&RecoveryBudget,at:u64,auth:u64)->Result<u64>{
 budget.validate(at)?;
 if budget.attempts().iter().filter(|old|at-**old<=RECOVERY_WINDOW_MS).count()>=RECOVERY_MAXIMUM_ATTEMPTS{return Err("LIVE_STREAM_RECOVERY_ATTEMPTS_EXHAUSTED".into())}
 if auth>at{return Err("LIVE_AUTH_MINT_CLOCK_REGRESSION".into())}
 Ok(budget.next_allowed(at)?.max(auth.checked_add(60_000).ok_or("LIVE_AUTH_MINT_CLOCK_RANGE")?))
}
fn recover_stream(t:&mut Transport,c:&Control,bank:&mut Session,log:&DispatchLog,authority:&Arc<ProductionAuthority>,source:&mut RestSource,stream:&mut Option<Realtime>,budget:&mut RecoveryBudget,failure:StreamFailure,breadth:&mut Option<Recorder>,universe:&mut Option<Universe>,windows:&mut std::collections::BTreeMap<String,u64>)->Result<()>{
 if !failure.is_recoverable(){return Err(failure.into())}
 log.require_resolved_attempts()?;recovery_queue_clear(&t.admission_state()?)?;
 if !authority.can_read(&c.scope){return Err("LIVE_STREAM_RECOVERY_CONTROL_OR_STOP".into())}
 let at=now();
 let auth=load(&c.root.join("stream-auth-admission.json"))?["atMs"].as_u64().ok_or("LIVE_AUTH_MINT_MARKER")?;
 let until=recovery_not_before(budget,at,auth)?;
 // Drop joins the sole old socket before any replacement token or connection.
 drop(stream.take());authority.clear_ready();
 atomic_json(&c.root.join("stream-recovery.json"),&json!({"at":iso(at),"stage":"waiting","reason":failure.reason(),"notBefore":iso(until),"orderRetried":false}))?;
 while now()<until{budget.validate(now())?;if !authority.can_read(&c.scope){return Err("LIVE_STREAM_RECOVERY_CONTROL_OR_STOP".into())}std::thread::sleep(Duration::from_millis(until.saturating_sub(now()).min(200)));}
 if !authority.can_read(&c.scope){return Err("LIVE_STREAM_RECOVERY_CONTROL_OR_STOP".into())}
 log.require_resolved_attempts()?;recovery_queue_clear(&t.admission_state()?)?;
 budget.record_attempt(now())?;atomic_json(&c.root.join("stream-recovery-budget.json"),&serde_json::to_value(&*budget)?)?;
 // One replacement attempt only. Auth/join/source/durability failures propagate
 // to STOP, never become a generic retry or erase this durable attempt.
 let(connected,joined_at)=connect_stream(t,c,bank,source)?;
 if let Some(r)=breadth{r.observe("stream_reset",&json!({"localBoundary":true,"reason":"confirmed_recovery"}),&joined_at)}
 if let Some(u)=universe{u.observe("stream_reset",&json!({"localBoundary":true}),&joined_at)?;}
 let from=now().saturating_sub(60_000);for old in windows.values_mut(){*old=(*old).min(from);}
 atomic_json(&c.root.join("stream-recovery.json"),&json!({"at":iso(now()),"stage":"joined_requires_scope_recovery","reason":failure.reason(),"joinedAt":joined_at,"attempts":budget.attempts(),"orderRetried":false,"coverageResetToHealthy":false}))?;
 require_stream(&connected)?;*stream=Some(connected);let connected=stream.as_ref().unwrap();authority.bind_stream(connected.failure_signal(),connected.deadline_ms);Ok(())
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
 let exact_cash=request(&mut read,&c.root,Endpoint::Account)?;
 let recent=request(&mut read,&c.root,Endpoint::PortfolioFills)?;
 atomic_json(&c.root.join("preflight.json"),&json!({"at":iso(now()),"account":a,"cashBalance":exact_cash,"positions":p,"openOrders":o,"recentFills":recent,"adoptsHoldings":false}))?;
 if let Some(cfg)=&c.daily_entry_windows{cfg.validate(&c.activation,now())?;if a.data["endDate"].as_str().map(moloch_live_runtime::daily_windows::clock).transpose()?.is_none_or(|end|cfg.campaign_end_ms>end){return Err("DAILY_OFFICIAL_CAMPAIGN_END".into())}}
 if args[1]=="preflight"{println!("Read-only preflight complete; no orders submitted");return Ok(())}
 let stop=c.root.join("EXECUTION_STOP");let global_stop=c.owner_lock.parent().ok_or("LIVE_LOCK_PARENT")?.join("EXECUTION_STOP");if stop.exists()||global_stop.exists(){return Err("LIVE_EXECUTION_STOP".into())}
 let result=(||->Result<()>{
  if now()>=c.activation.entry_until_ms&&!c.root.join(&c.bank_name).join("manifest.json").exists(){return Err("LIVE_ENTRY_ACTIVATION_EXPIRED".into())}
  let seed_path=c.root.join(if c.bank_name=="bank"{"account-seed.json".into()}else{format!("{}-account-seed.json",c.bank_name)});let seed=if seed_path.exists(){load(&seed_path)?}else{
   if !o.data["data"].as_array().ok_or("LIVE_OPEN_ORDERS")?.is_empty(){return Err("LIVE_FOREIGN_OPEN_ORDER".into())}
   if !c.all_markets{for row in p.data["positions"].as_array().ok_or("LIVE_POSITIONS")?{if row["quantity"].as_i64()!=Some(0)&&row["exchangeId"].as_str().is_some_and(|e|c.scope.exchanges.contains_key(e)){return Err("LIVE_INITIAL_SCOPE_ALREADY_HELD".into())}}}
   let mut v=json!({"runId":c.activation.run_id,"accountSnapshotHash":feedback::digest(&exact_cash)?,"availableCash":money(decimal_units(&exact_cash.data["balance"].to_string())?),"observedAt":exact_cash.received_at});
   if let Some(previous)=&c.predecessor_bank{v["continuation"]=moloch_live_runtime::handoff::verify_flat_predecessor(&c.root,previous,&c.policy)?;}
   if c.all_markets{v["excludedPositionsAtTransfer"]=p.data.clone();}
   atomic_json(&seed_path,&v)?;v
  };
  let mut bank=Session::open_live(&c.root.join(&c.bank_name),&c.policy,&seed)?;
  if c.manage_existing_positions&&bank.engine().report()?.get("inheritedInventory").is_none(){
   let event=moloch_live_runtime::inventory::adoption(&p,bank.engine(),&c.scope.tournament_id,&c.scope.tournament_slug,&c.scope.exchanges,&iso(now()))?;
   accept(&mut bank,&event)?;atomic_json(&c.root.join("inherited-position-transfer.json"),&json!({"positionsReceipt":p,"event":event,"report":bank.engine().report()?,"sourceBankPreserved":true}))?;
  }
  let authority=Arc::new(ProductionAuthority::new(c.activation.clone(),c.scope.clone(),vec![stop.clone(),global_stop.clone()],path.clone(),std::env::current_exe()?,lease.clone())?);
  let mut log=DispatchLog::open(&c.root.join("dispatch.ndjson"))?;log.require_resolved_attempts()?;
  // A durable official acknowledgment can be replayed after a crash before ledger commit.
  for (qid,v) in log.acknowledgments.clone(){let r:Receipt=serde_json::from_value(v)?;let q=quote(&bank,qid)?;let e=if let Some(recovery)=log.recovered.get(&qid){feedback::recovered_acknowledgment(&q,recovery["actualOrderId"].as_str().ok_or("LIVE_RECOVERY_ACTUAL_ID")?,&c.scope.tournament_id,&r,&serde_json::from_value(recovery["fills"].clone())?,&iso(now()))?}else{feedback::acknowledgment(&q,&r,&iso(now()))?};authority.bind_owned(e["actualOrderId"].as_str().ok_or("LIVE_ACTUAL_ID")?.into());accept(&mut bank,&e)?;}
  drop(read);let mut t=transport(&c,Box::new(SharedAuthority(authority.clone())),true)?;
  let state=t.admission_state()?;if state["state"]["unknown"].as_object().is_some_and(|m|!m.is_empty()){return Err("LIVE_QUEUE_UNKNOWN_ACCEPTANCE".into())}
  ensure_entry_window(&c,&mut bank,&authority)?;
  let fully_managed=bank.engine().report()?.get("inheritedInventory").is_some();
  let empty_positions=json!({"positions":[]});
  // Verified adoption updates the native managed-market limit while preserving
  // the original fresh-entry limit. REST coverage must use that replayed limit.
  let managed_market_limit=bank.engine().policy()["maximumMarkets"].as_u64().ok_or("LIVE_MARKET_CAP")? as usize;
  let discovery_read_limit=c.policy["maximumMarkets"].as_u64().ok_or("LIVE_MARKET_CAP")? as usize;
  let mut universe=if c.all_markets{Some(Universe::new(&c.scope.exchanges,if fully_managed{&empty_positions}else{seed.get("excludedPositionsAtTransfer").ok_or("LIVE_UNIVERSE_SEED")?},managed_market_limit)?.with_discovery_limit(discovery_read_limit)?)}else{None};
  // Previously owned holdings are filtered from the financial feed as well as discovery.
  let pairs:Value=if let Some(u)=&universe{u.pairs(&c.scope.exchanges)}else{c.scope.exchanges.iter().map(|(e,m)|json!({"exchangeId":e,"marketId":m})).collect::<Vec<_>>().into()};
  let start=now();let mut source_state=RestSource::new(&pairs,&c.scope.tournament_id,&iso(start))?;source_state.seed_trade_facts(bank.source_facts())?;
  let recovery_budget_path=c.root.join("stream-recovery-budget.json");let mut recovery_budget=recovery_budget(&recovery_budget_path,now())?;
  if !recovery_budget_path.exists(){atomic_json(&recovery_budget_path,&serde_json::to_value(&recovery_budget)?)?;}
  let (connected,joined_at)=connect_stream(&mut t,&c,&mut bank,&mut source_state)?;require_stream(&connected)?;let mut stream=Some(connected);
  authority.bind_stream(stream.as_ref().unwrap().failure_signal(),stream.as_ref().unwrap().deadline_ms);
  let mut relationship_census=None;let mut preflight_cursor=None;
  let mut reconciliation_schedule=moloch_live_runtime::scheduling::ReconciliationSchedule::new(10_000)?;
  let pending_version=|log:&DispatchLog|moloch_live_runtime::scheduling::PendingVersion{acknowledged:log.acknowledgments.len() as u64,terminal:log.closed.len() as u64};
  let breadth_path=c.root.join(format!("breadth-{}-{}.jsonl",std::process::id(),now()));
  let mut breadth=Recorder::new(&breadth_path).ok();
  if let Some(r)=&mut breadth{r.observe("stream_reset",&json!({"localBoundary":true,"reason":"confirmed_subscription"}),&joined_at)}
  if let Some(u)=&mut universe{u.observe("stream_reset",&json!({"localBoundary":true}),&joined_at)?;}
  let mut windows:std::collections::BTreeMap<String,u64>=c.scope.exchanges.keys().map(|e|(e.clone(),start.saturating_sub(60000))).collect();
  let mut cycles=0u64;let mut reconciliation_cache=Reconciler::default();
  loop{
   let cycle_result=(||->Result<()>{
   if stop.exists()||global_stop.exists(){return Err("LIVE_EXECUTION_STOP".into())}
   ensure_entry_window(&c,&mut bank,&authority)?;
   drain_stream(stream.as_ref().ok_or("LIVE_STREAM_REQUIRED")?,&mut bank,&mut source_state,&mut breadth,&mut universe)?;
   let mut reconciled=reconcile(&mut t,&c,&mut bank,&mut log,&mut reconciliation_cache)?;
   reconciliation_schedule.completed(now(),pending_version(&log),reconciled.blocks_new_submissions)?;
   accept(&mut bank,&json!({"type":"tick","receivedAt":iso(now())}))?;
   let selected=if let Some(u)=&mut universe{u.select(&bank.engine().required_exchanges())?}else{c.scope.exchanges.keys().cloned().collect()};
   for (selected_index,ex) in selected.iter().enumerate(){require_stream(stream.as_ref().ok_or("LIVE_STREAM_REQUIRED")?)?;let market=&c.scope.exchanges[ex];let from=windows[ex];let next=source(&mut t,&c,&mut bank,&mut source_state,ex,market,from)?;windows.insert(ex.clone(),next);drain_stream(stream.as_ref().ok_or("LIVE_STREAM_REQUIRED")?,&mut bank,&mut source_state,&mut breadth,&mut universe)?;
    if reconciliation_schedule.due(now(),pending_version(&log))?.is_some(){
     reconciled=reconcile(&mut t,&c,&mut bank,&mut log,&mut reconciliation_cache)?;
     reconciliation_schedule.completed(now(),pending_version(&log),reconciled.blocks_new_submissions)?;
    }
    let permit_new=reconciliation_schedule.permits_submission(now(),pending_version(&log))?;
    ensure_entry_window(&c,&mut bank,&authority)?;
    dispatch(&mut t,&c,&mut bank,&mut log,&authority,&mut source_state,permit_new,&mut preflight_cursor,stream.as_ref().ok_or("LIVE_STREAM_REQUIRED")?,&windows)?;
    let _=atomic_json(&c.root.join("progress.json"),&json!({"at":iso(now()),"pid":std::process::id(),"phase":"trading","completedReadScopes":selected_index+1,"selectedReadScopes":selected.len(),"maximumRealtimeFramesPerDrain":MAX_REALTIME_FRAMES_PER_DRAIN,"realtimeHealthy":stream.as_ref().is_some_and(|s|s.healthy()),"actualOrders":log.acknowledgments.len(),"terminalOrders":log.closed.len(),"actualFills":bank.engine().fills().len(),"profitabilityEstablished":false}));
   }
   cycles+=1;
   atomic_json(&c.root.join("status.json"),&json!({"at":iso(now()),"pid":std::process::id(),"mode":"actual-contest-trading","runId":c.activation.run_id,"bank":c.bank_name,"cycles":cycles,"realtimeHealthy":stream.as_ref().is_some_and(|s|s.healthy()),"universe":universe.as_ref().map(Universe::status),"selectedReadScopes":selected,"ownedReconciliation":reconciled,"sourceScopes":source_state.scopes(),"breadthCapture":{"path":breadth_path,"state":breadth.as_ref().map(Recorder::status)},"report":bank.engine().report()?,"queue":t.admission_state()?,"actualOrders":log.acknowledgments.len(),"terminalOrders":log.closed.len(),"entryAnchor":iso(c.activation.entry_until_ms),"entryUntil":iso(authority.entry_until_ms()),"profitabilityEstablished":false}))?;
   // Existing orders and the first trading cycle precede optional research reads.
   // Diagnostic persistence never blocks the financial journal or ordinary trading.
   if relationship_census.is_none(){let census=moloch_live_runtime::relationship_census::capture(&mut t,&c.scope,&c.root.join("relationship-receipts"),&[]);if let Ok(report)=serde_json::to_value(&census.report){let _=atomic_json(&c.root.join("relationship-census.json"),&report);}relationship_census=Some(census);}
   if cycles%20==0{t.compact()?;}
   std::thread::sleep(Duration::from_millis(if selected.is_empty(){600}else{100}));
   Ok(())})();
   if let Err(error)=cycle_result{if let Some(failure)=error.downcast_ref::<StreamFailure>(){
    recover_stream(&mut t,&c,&mut bank,&log,&authority,&mut source_state,&mut stream,&mut recovery_budget,*failure,&mut breadth,&mut universe,&mut windows)?;
   }else{return Err(error)}}
  }
 })();
 if let Err(e)=&result{atomic_json(&stop,&json!({"at":iso(now()),"reason":e.to_string(),"action":"no automatic write retry or activation renewal"}))?;atomic_json(&c.root.join("status.json"),&json!({"at":iso(now()),"pid":std::process::id(),"mode":"stopped","reason":e.to_string(),"profitabilityEstablished":false}))?;}
 result
}

#[cfg(test)]mod refresh_worker_tests{
 use super::*;
 struct BankTemp(PathBuf);
 impl BankTemp{fn new()->Self{static NEXT:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);Self(std::env::temp_dir().join(format!("moloch-worker-batch-{}-{}-{}",std::process::id(),now(),NEXT.fetch_add(1,std::sync::atomic::Ordering::SeqCst))))}}
 impl Drop for BankTemp{fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}
 fn source_ticks(n:usize)->Vec<Value>{(0..n).map(|i|json!({"type":"tick","receivedAt":iso(1_760_000_000_000+i as u64)})).collect()}
 #[test]fn urgent_other_scope_keeps_pre_outage_window_after_late_join(){
  let old=1_760_000_000_000;let joined=old+900_000;let mut source=RestSource::new(&json!([{"marketId":"1","exchangeId":"2"}]),"fixture",&iso(old)).unwrap();source.subscribed(&iso(joined)).unwrap();assert!(source.recovery_start_ms("1").unwrap()>old);assert_eq!(exit_recovery_from(&source,"1",old),Some(old));assert_eq!(exit_recovery_from(&source,"1",joined+1),Some(joined-1));
 }
 #[test]fn recovery_combines_auth_budget_and_refuses_exhaustion_without_long_auto_wait(){
  let mut budget=RecoveryBudget::new(0).unwrap();budget.record_attempt(0).unwrap();assert_eq!(recovery_not_before(&budget,1000,500).unwrap(),60_500);assert!(recovery_not_before(&budget,1000,1001).is_err());budget.record_attempt(60_000).unwrap();budget.record_attempt(120_000).unwrap();assert!(recovery_not_before(&budget,180_000,120_000).is_err());assert!(budget.next_allowed(180_000).unwrap()>180_000);assert_eq!(recovery_not_before(&budget,600_001,500_000).unwrap(),600_001);
 }
 #[test]fn stream_recovery_requires_global_quiet_queue_without_inferred_missing_fields(){
  assert!(recovery_queue_clear(&json!({"state":{"unknown":{},"active":null,"manual_review":false}})).is_ok());
  for state in [json!({}),json!({"state":{"unknown":{}}}),json!({"state":{"unknown":{},"active":null,"manual_review":true}}),json!({"state":{"unknown":{},"active":{"request":"ongoing"},"manual_review":false}}),json!({"state":{"unknown":{"other":{"intent":"uncertain"}},"active":null,"manual_review":false}})]{assert!(recovery_queue_clear(&state).is_err());}
 }
 #[test]fn durable_stream_budget_corruption_or_regressed_restart_never_reseeds(){
  let temp=BankTemp::new();fs::create_dir(&temp.0).unwrap();let path=temp.0.join("budget.json");assert!(recovery_budget(&path,1000).unwrap().attempts().is_empty());let mut budget=RecoveryBudget::new(1000).unwrap();budget.record_attempt(1000).unwrap();atomic_json(&path,&serde_json::to_value(&budget).unwrap()).unwrap();let before=fs::read(&path).unwrap();assert!(recovery_budget(&path,999).is_err());assert_eq!(fs::read(&path).unwrap(),before);assert_eq!(recovery_budget(&path,2000).unwrap().attempts(),&[1000]);
  for bytes in [b"{}".to_vec(),vec![b' ';4097]]{fs::write(&path,&bytes).unwrap();assert!(recovery_budget(&path,2000).is_err());assert_eq!(fs::read(&path).unwrap(),bytes);}
  fs::remove_file(&path).unwrap();fs::write(temp.0.join("stream-recovery.json"),b"{}").unwrap();assert!(recovery_budget(&path,2000).is_err());assert!(!path.exists());
 }
 #[test]fn source_chunks_preserve_restart_cursor_and_stop_at_invalid_batch_without_replaying_prefix(){
  for bad in [false,true]{let temp=BankTemp::new();let policy=json!({});let mut bank=Session::open(&temp.0,None,&policy).unwrap();let mut events=source_ticks(65);if bad{events[40]["type"]=json!("unsupported");}
   let result=accept_source_events(&mut bank,&events);assert_eq!(result.is_err(),bad);let expected=if bad{32}else{65};assert_eq!(bank.last_source_sequence("moloch-live"),expected);
   let report=bank.engine().report().unwrap();let bytes=fs::read(temp.0.join("events.jsonl")).unwrap();assert_eq!(bytes.iter().filter(|b|**b==b'\n').count(),expected as usize);drop(bank);
   let mut restored=Session::open(&temp.0,None,&policy).unwrap();assert_eq!(restored.last_source_sequence("moloch-live"),expected);assert_eq!(restored.engine().report().unwrap(),report);
   if bad{events[40]["type"]=json!("tick");accept_source_events(&mut restored,&events[32..]).unwrap();assert_eq!(restored.last_source_sequence("moloch-live"),65);}
  }
 }
 #[test]fn long_expired_history_coalesces_by_scope_without_reviving_attempted_or_cancelled_hints(){
  let mut orders=(1..=10_002u64).map(|id|json!({"id":id,"action":"sell","inventoryExitReason":"maximumAge","executionStyle":"inventoryAON","filled":0,"expiresAt":100,"exchangeId":format!("ex{}",id%3),"side":"NO","retired":true})).collect::<Vec<_>>();
  let mut cancelled=orders.last().unwrap().clone();cancelled["id"]=json!(10_003);cancelled["cancelEffectiveAt"]=json!(101);orders.push(cancelled);
  let hints=latest_refresh_hints(&orders,200,|id|id!=10_002).unwrap();assert_eq!(hints.len(),3);let mut ids=hints.values().map(|q|q["id"].as_u64().unwrap()).collect::<Vec<_>>();ids.sort();assert_eq!(ids,vec![9_999,10_000,10_001]);
  assert!(latest_refresh_hints(&orders,99,|_|true).unwrap().is_empty());
 }
 fn control()->Control{serde_json::from_value(json!({"activation":{"runId":"new-run","entryUntilMs":9_000_000,"maximumCommitted":"800.000000","maximumMarketCost":"100.000000","maximumQuantity":300,"tournamentId":"cup","tournamentSlug":"cup","binarySha256":"a".repeat(64)},"scope":{"tournamentId":"cup","tournamentSlug":"cup","exchanges":{"1012":"525"}},"policy":{},"root":"/fixture","credentials":"/unused","ownerLock":"/unused.lock","accountScope":"fixture"})).unwrap()}
 #[test]fn rebuilt_physical_binding_uses_new_qid_quantity_price_and_original_expiry(){
  let c=control();let new=json!({"id":466,"exchangeId":"1012","marketId":"525","side":"NO","action":"sell","quantity":169,"price":"0.650000","expiresAt":1_000_000});
  let(order,spec,permit)=physical_order(&c,&new).unwrap();let body=spec.body.unwrap();
  assert_eq!(order.idempotency_key,"new-run:466");assert_eq!(permit.intent_id,"466");assert_eq!(permit.maximum_quantity,169);assert_eq!(permit.maximum_cost,"109.850000");
  assert_eq!(body["quantity"],169);assert_eq!(decimal_units(&body["price"].to_string()).unwrap(),650_000);assert_eq!(body["expirationDate"],iso(1_000_000));assert_eq!(body["side"],"no");assert_eq!(body["action"],"sell");
  assert!(body.get("allOrNone").is_none()); // Native style is not venue atomicity.
 }
 #[test]fn exact_unknown_shape_is_required_and_nonempty_global_state_blocks(){
  assert!(!queue_unknown_from_state(&json!({"state":{"unknown":{}}})).unwrap());
  assert!(queue_unknown_from_state(&json!({"state":{"unknown":{"other":{"intent":"other"}}}})).unwrap());
  for malformed in [json!({}),json!({"state":{}}),json!({"state":{"unknown":[]}}),json!({"hasUnknown":false})]{assert!(queue_unknown_from_state(&malformed).is_err());}
 }
}
