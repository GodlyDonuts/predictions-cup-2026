use baal_transport_service::{self as api,Authority,Config,Endpoint,Order,Permit,Receipt,RequestSpec,Scope,SystemClock,Transport,ReadOnlyAuthority,credentials::UnixFileCredentials,decimal_units};
use moloch_live_runtime::{authority::{Activation,ProductionAuthority,SharedAuthority,SnapshotSet,CancelSnapshotSet},dispatch::DispatchLog,feedback::{self,Result}};
use moloch_replay::{session::{Session,ExclusiveLock,atomic_json},source::RestSource,money};
use moloch_replay::realtime::{Auth,Incoming,Realtime};
use moloch_live_runtime::breadth::Recorder;
use moloch_live_runtime::universe::Universe;
use moloch_live_runtime::reconciliation::{self,Reconciler};
use moloch_live_runtime::fundamental_policy::{self as fundamental,Cohorts,ReviewAuthority};
use moloch_live_runtime::stream_recovery::{StreamFailure,RecoveryBudget,RECOVERY_WINDOW_MS,RECOVERY_MAXIMUM_ATTEMPTS};
use serde::{Serialize,Deserialize};
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::{fs,path::{Path,PathBuf},sync::Arc,time::{SystemTime,UNIX_EPOCH,Duration}};
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Control{#[serde(default)]execution_distribution:Option<moloch_live_runtime::execution_distribution::Config>,#[serde(default)]fundamental_policy:Option<fundamental::Settings>,#[serde(default)]maker_policy:Option<moloch_live_runtime::maker_policy::Settings>,#[serde(default)]package_allocation:Option<moloch_live_runtime::package_allocation::Settings>,#[serde(default)]collateral_accounting:bool,#[serde(default)]execution_model:Option<moloch_live_runtime::execution_edge::Config>,#[serde(default)]packages:Option<moloch_live_runtime::package_settings::Settings>,#[serde(default)]daily_entry_windows:Option<moloch_live_runtime::daily_windows::DailyEntryWindows>,activation:Activation,scope:Scope,policy:Value,root:PathBuf,credentials:PathBuf,owner_lock:PathBuf,account_scope:String,#[serde(default)]all_markets:bool,#[serde(default="default_bank")]bank_name:String,#[serde(default)]predecessor_bank:Option<String>,#[serde(default)]manage_existing_positions:bool}
fn default_bank()->String{"bank".into()}
struct FundamentalRuntime{cohorts:Cohorts,authority:ReviewAuthority}
impl FundamentalRuntime{
 fn eligible(&self,bank:&Session,log:&DispatchLog,scope:&Scope,qid:u64,at:u64)->Result<bool>{
  self.authority.validate()?;
  if !self.cohorts.manages(quote(bank,qid)?["exchangeId"].as_str().ok_or("LIVE_QUOTE_SCOPE")?){return Ok(true)}
  let n=self.cohorts.planned_entry_quantity(bank,log,scope,qid,self.authority.trusted(),at)?;
  let decision=if let Some(n)=n{self.cohorts.assess_quantity(bank,log,scope,qid,n,self.authority.trusted(),at)?}else{self.cohorts.assess(bank,log,scope,qid,self.authority.trusted(),at)?};
  Ok(matches!(decision,fundamental::Decision::Entry{..}|fundamental::Decision::Exit{..}))
 }
}
fn validate_maker_configuration(c:&Control)->Result<()>{
 if let Some(settings)=&c.maker_policy{
  let model=c.execution_model.as_ref().ok_or("LIVE_MAKER_MODEL_REQUIRED")?;model.validate()?;
  if model.markout_ms!=settings.expected_inventory_age_ms||c.policy["maximumInventoryAgeMs"].as_u64()!=Some(settings.expected_inventory_age_ms)||c.policy["inventoryAgeClock"]!="execution"{return Err("LIVE_MAKER_HORIZON_MISMATCH".into())}
 }Ok(())
}
fn expected_maker_expiry(error:&dyn std::error::Error)->bool{matches!(error.to_string().as_str(),"MAKER_DECISION_EXPIRED"|"MAKER_QUOTE_EXPIRED")}
const MAX_REALTIME_FRAMES_PER_DRAIN:usize=64;
const RELATIONSHIP_CENSUS_REFRESH_MS:u64=300_000;
fn now()->u64{SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64}
fn iso(t:u64)->String{chrono::DateTime::from_timestamp_millis(t as i64).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis,true)}
fn hash(v:&Value)->String{format!("{:x}",Sha256::digest(serde_json::to_vec(v).unwrap()))}
fn load(p:&Path)->Result<Value>{Ok(serde_json::from_slice(&fs::read(p)?)?)}
fn private_dir(p:&Path)->Result<()>{use std::os::unix::fs::PermissionsExt;fs::create_dir_all(p)?;let m=fs::symlink_metadata(p)?;if m.file_type().is_symlink()||!m.is_dir(){return Err("LIVE_DIRECTORY".into())}fs::set_permissions(p,fs::Permissions::from_mode(0o700))?;Ok(())}
fn transport(c:&Control,auth:Box<dyn Authority>,writes:bool)->Result<Transport>{Ok(Transport::new_with_read_budget(Config{queue_dir:c.root.join("account-queue"),account_scope:c.account_scope.clone(),scope:c.scope.clone(),maximum_pages:20,writes_enabled:writes},Box::new(UnixFileCredentials::approved_file(c.credentials.clone())?),Box::new(SystemClock),auth,100,600)?.with_full_account_observation())}
fn observe_execution(bank:&Session,sequence:u64,event:&Value){moloch_live_runtime::execution_edge::observe_runtime_committed(bank,"moloch-live",sequence,event);moloch_live_runtime::execution_distribution::observe_runtime_committed(bank,"moloch-live",sequence,event);}
fn accept(bank:&mut Session,event:&Value)->Result<()>{let mut e=event.clone();e["processedAt"]=json!(iso(now()));let seq=bank.last_source_sequence("moloch-live")+1;if bank.accept("moloch-live",seq,&e)?{observe_execution(bank,seq,&e);}Ok(())}
/// The original dispatch receipt is still fully validated before this call.
/// Recheck a previously committed ACK against the real native validator without
/// writing the same acknowledgement and every derived report again on restart.
/// A crash before the original native ACK retains the ordinary durable path.
fn restore_ack(bank:&mut Session,event:&Value)->Result<bool>{
 if event["type"]!="actual_order_ack"{return Err("LIVE_RESTORE_ACK_KIND".into())}
 let id=event["orderId"].as_u64().ok_or("LIVE_RESTORE_ACK_ID")?;
 if bank.engine().live_order_bindings().contains_key(&id){
  let mut checked=bank.engine().clone();let mut e=event.clone();e["processedAt"]=json!(iso(now()));checked.advance(&e)?;return Ok(false)
 }
 accept(bank,event)?;Ok(true)
}
// A newly reviewed exchange can contain older, already closed ordinary orders.
// They retain their original native validation, without inventing a forecast
// binding. This grants no new dispatch or recovery of an open/unknown order.
fn closed_legacy_ack(bank:&Session,log:&DispatchLog,qid:u64,event:&Value)->bool{
 log.closed.contains(&qid)&&log.intents.get(&qid).is_some_and(|intent|intent.get("fundamental").is_none_or(Value::is_null))
  &&event["type"]=="actual_order_ack"&&event["orderId"].as_u64()==Some(qid)
  &&bank.engine().live_order_bindings().get(&qid).is_some_and(|actual|event["actualOrderId"]==actual.as_str())
  &&bank.engine().order_records().iter().any(|q|q["id"].as_u64()==Some(qid)&&q["retired"]==true)
}
fn ensure_entry_window(c:&Control,bank:&mut Session,authority:&ProductionAuthority)->Result<()>{
 let Some(cfg)=&c.daily_entry_windows else{return Ok(())};let at=now();cfg.validate(&c.activation,at)?;
 let current=moloch_live_runtime::daily_windows::clock(bank.engine().policy()["entryStopAt"].as_str().ok_or("DAILY_POLICY_STOP")?)?;
 let end=cfg.end_at(at)?;
 if at>=cfg.campaign_end_ms{return Ok(())}
 if current>end{return Err("DAILY_POLICY_AHEAD".into())}
 if end>current{
  let stamp=iso(at);let event=json!({"type":"live_entry_window","source":"moloch_live_dispatcher","receivedAt":stamp,"processedAt":stamp,"previousEntryStopAt":iso(current),"entryStopAt":iso(end),"mandate":cfg.mandate(&c.activation)});
  if bank.accept("moloch-live",bank.last_source_sequence("moloch-live")+1,&event)?{
   // Policy identity changed durably. Lose model confidence and cold-start at
   // this actual prefix; never reinterpret past samples under the new policy.
   if let Some(cfg)=&c.execution_model{let _=moloch_live_runtime::execution_edge::initialize_observer(bank,Some(cfg.clone()));}
   let _=moloch_live_runtime::execution_distribution::initialize_observer(bank,c.execution_distribution.clone());
  }
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
  if outcome.appended_rows!=entries.len()||outcome.accepted.len()!=entries.len()||outcome.accepted.iter().any(|accepted|!*accepted){return Err("LIVE_SOURCE_BATCH_PREFIX".into())} for entry in &entries{observe_execution(bank,entry.sequence,entry.event);}
 }Ok(())
}
fn record_receipt(root:&Path,r:&Receipt)->Result<()> {let p=root.join("receipts").join(format!("{}.json",feedback::digest(r)?));if !p.exists(){atomic_json(&p,&serde_json::to_value(r)?)?}Ok(())}
fn validate_collateral_accounting(c:&Control)->Result<()>{
 // Ordinary passive partial/late fills require a separately verified native
 // continuation before this mixed worker may enable ALL accounting.
 if c.collateral_accounting{return Err("LIVE_COLLATERAL_ORDINARY_FLOW_UNSUPPORTED".into())}Ok(())
}
/// Original-evidence financial continuation, inactive in the current mixed
/// worker until partial/late ordinary-fill accounting passes integration. True
/// defers NEW orders; existing cancellation and reconciliation remain available.
fn collateral_accounting(t:&mut Transport,c:&Control,bank:&mut Session,ordinary:&DispatchLog,packages:&moloch_live_runtime::package_dispatch::PackageDispatchLog,lease:&ExclusiveLock,last_check:&mut u64)->Result<bool>{
 use moloch_live_runtime::collateral_feedback::{self as cf,RetainedReceipt,ReceiptStore,Snapshot,Operation,Boundary};
 let initialized=bank.engine().collateral_snapshot_receipts().is_some();let pending=if initialized{bank.engine().collateral_report()?["pendingNativeFillEconomics"]!=false}else{false};
 if !c.collateral_accounting{return Ok(pending)}
 let at=now();if *last_check!=0&&at<last_check.saturating_add(30_000){return Ok(pending)}
 if ordinary.require_resolved_attempts().is_err()||ordinary.acknowledgments.keys().any(|id|!ordinary.closed.contains(id))||packages.unresolved()||bank.engine().package_blocks_new_submissions(){return Ok(pending)}
 match t.package_queue_quiet(){Ok(_)=>{},Err(e)if e.code=="PACKAGE_QUEUE_NOT_QUIET"=>return Ok(pending),Err(e)=>return Err(e.into())}
 *last_check=at;
 let evidence=(||->Result<_>{
  let before=bank.engine().collateral_snapshot_receipts().cloned();let mut acks=Vec::new();let mut tapes=Vec::new();
  if pending{
   let count=bank.engine().collateral_accounted_fill_count().ok_or("LIVE_COLLATERAL_FILL_CURSOR")?;let fresh=bank.engine().fills().get(count..).ok_or("LIVE_COLLATERAL_FILL_CURSOR")?;
   let ids:std::collections::BTreeSet<_>=fresh.iter().map(|f|f["actualOrderId"].as_str().map(str::to_owned).ok_or("LIVE_COLLATERAL_ACTUAL_ID")).collect::<std::result::Result<_,_>>()?;let mut ack_hashes=std::collections::BTreeSet::new();
   for actual in ids{let qid=*bank.engine().live_order_bindings().iter().find(|(_,id)|**id==actual).ok_or("LIVE_COLLATERAL_BINDING")?.0;
    if let Some(raw)=ordinary.acknowledgments.get(&qid){let r:Receipt=serde_json::from_value(raw.clone())?;if r.endpoint!="/orders"{return Err("LIVE_COLLATERAL_ORIGINAL_POST_UNAVAILABLE".into())}let digest=feedback::digest(&r)?;if ack_hashes.insert(digest){acks.push(RetainedReceipt::open(&c.root,&r,ReceiptStore::Ordinary)?);}}
    else{let mut found=false;for p in packages.records(){let Some(raw)=p["outcome"].get("receipt")else{continue};let r:Receipt=serde_json::from_value(raw.clone())?;if r.data["results"].as_array().is_some_and(|rs|rs.iter().any(|v|v["data"]["orderId"].as_str().map(str::to_owned).or_else(||v["data"]["orderId"].as_u64().map(|n|n.to_string())).as_deref()==Some(actual.as_str()))){found=true;let digest=feedback::digest(&r)?;if ack_hashes.insert(digest){acks.push(RetainedReceipt::open(&c.root,&r,ReceiptStore::Package)?);}}}if !found{return Err("LIVE_COLLATERAL_ORIGINAL_ACK_UNAVAILABLE".into())}}
   }
   let digests:std::collections::BTreeSet<_>=fresh.iter().map(|f|f["receiptHash"].as_str().map(str::to_owned).ok_or("LIVE_COLLATERAL_FILL_SOURCE")).collect::<std::result::Result<_,_>>()?;
   for digest in digests{let store=if c.root.join("receipts").join(format!("{digest}.json")).exists(){ReceiptStore::Ordinary}else{ReceiptStore::Package};tapes.push(RetainedReceipt::from_recorded_digest(&c.root,&digest,store)?);}
  }
  let before=if let Some(raw)=before{let a:Receipt=serde_json::from_value(raw["account"].clone())?;let k:Receipt=serde_json::from_value(raw["collateral"].clone())?;Some((RetainedReceipt::open(&c.root,&a,ReceiptStore::Ordinary)?,RetainedReceipt::open(&c.root,&k,ReceiptStore::Ordinary)?))}else{None};
  // Observe settlement BEFORE the after-cash pair; shares never imply side.
  let settlements=if initialized&&!pending&&bank.engine().wallet.positions().as_array().is_some_and(|p|!p.is_empty()){let r=request(t,&c.root,Endpoint::Settlements)?;Some(RetainedReceipt::open(&c.root,&r,ReceiptStore::Ordinary)?)}else{None};
  let account=request(t,&c.root,Endpoint::Account)?;let collateral=request(t,&c.root,Endpoint::Collateral)?;let after=(RetainedReceipt::open(&c.root,&account,ReceiptStore::Ordinary)?,RetainedReceipt::open(&c.root,&collateral,ReceiptStore::Ordinary)?);
  let mut ids=Vec::<String>::new();if let(Some(s),Some((a,k)))=(&settlements,&before){let earliest=moloch_live_runtime::daily_windows::clock(&a.receipt().received_at)?.max(moloch_live_runtime::daily_windows::clock(&k.receipt().received_at)?);
   for row in s.receipt().data["data"].as_array().ok_or("LIVE_COLLATERAL_SETTLEMENT_ROWS")?{
    let Some(id)=row["id"].as_str()else{continue};if bank.engine().collateral_settlement_recorded(id)||!["win","loss","refund"].iter().any(|v|row["type"]==*v){continue}let Some(created)=row["createdAt"].as_str()else{continue};if moloch_live_runtime::daily_windows::clock(created)?<earliest{continue}
    let ex=row["exchangeId"].as_str().map(str::to_owned).or_else(||row["exchangeId"].as_u64().map(|n|n.to_string()));let market=row["marketId"].as_str().map(str::to_owned).or_else(||row["marketId"].as_u64().map(|n|n.to_string()));let(Some(ex),Some(market))=(ex,market)else{continue};if c.scope.exchanges.get(&ex)!=Some(&market){continue}
    let side=match row["settlementOption"].as_str(){Some("YES")=>moloch_replay::Side::Yes,Some("NO")=>moloch_replay::Side::No,_=>continue};let other=if side==moloch_replay::Side::Yes{moloch_replay::Side::No}else{moloch_replay::Side::Yes};let Ok(shares)=decimal_units(&row["shares"].to_string())else{continue};
    if shares<=0||shares%1_000_000!=0||u64::try_from(shares/1_000_000).ok()!=Some(bank.engine().wallet.quantity(&ex,side))||bank.engine().wallet.quantity(&ex,other)!=0{continue}ids.push(id.into());
   }
  }Ok((before,after,acks,tapes,settlements,ids))
 })();
 let(before,after,acks,tapes,settlements,ids)=match evidence{Ok(v)=>v,Err(e)=>{atomic_json(&c.root.join("collateral-accounting-status.json"),&json!({"at":iso(now()),"state":"postponed","reason":e.to_string(),"bankChanged":false,"newOrdersDeferred":true}))?;return Ok(true)}};
 let ar:Vec<_>=acks.iter().collect();let tr:Vec<_>=tapes.iter().collect();let after_view=Snapshot{account:&after.0,collateral:&after.1};let b=Boundary{root:&c.root,owner_lock_path:&c.owner_lock,owner_lease:lease,transport:t,ordinary,packages};
 let operation=if let Some((a,k))=&before{let before=Snapshot{account:a,collateral:k};if pending{if acks.iter().all(|r|r.receipt().endpoint=="/orders"&&r.receipt().data.get("all")==Some(&Value::Null)){Operation::OrdinaryCashSync{before,after:after_view,acknowledgments:&ar,fill_tapes:&tr}}else{Operation::Execution{before,after:after_view,acknowledgments:&ar,fill_tapes:&tr}}}else if let Some(s)=settlements.as_ref().filter(|_|!ids.is_empty()){Operation::Settlement{before,after:after_view,settlements:s,settlement_ids:&ids}}else{Operation::Checkpoint{before,after:after_view}}}else{Operation::Baseline{after:after_view}};
 let stamp=iso(now());let tx=match cf::prepare(bank,&b,operation,&stamp){Ok(v)=>v,Err(e)=>{atomic_json(&c.root.join("collateral-accounting-status.json"),&json!({"at":stamp,"state":"postponed","reason":e.to_string(),"bankChanged":false,"newOrdersDeferred":true}))?;return Ok(true)}};
 // A failed durable commit is fatal, never downgraded to postponement.
 let committed=tx.commit(bank,&b)?;atomic_json(&c.root.join("collateral-accounting-status.json"),&json!({"at":stamp,"state":"committed","sourceSequence":committed.source_sequence,"eventSha256":committed.event_sha256,"nativeHead":committed.durable_head,"snapshotIsolation":"not_documented","futureAdvanceFundsSizing":false}))?;Ok(false)
}
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
  fn close(&mut self,id:u64,e:&Value,order:&Receipt,fills:&Receipt)->Result<()>{accept(self.bank,e)?;self.log.append(json!({"kind":"closed","qid":id,"orderReceiptHash":feedback::digest(order)?,"fillReceiptHash":feedback::digest(fills)?}))?;moloch_live_runtime::execution_distribution::confirm_runtime_terminal(self.bank,self.log,id,order,fills,now());Ok(())}
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
 snapshots_for_market(c,q["marketId"].as_str().ok_or("LIVE_MARKET_ID")?,|ep|request(t,&c.root,ep))
}
/// A read-only target is not a native quote. In particular, inherited value
/// exits obtain all seven original facts BEFORE creating a genuine SELL qid.
fn snapshots_for_market(c:&Control,market_id:&str,mut read:impl FnMut(Endpoint)->Result<Receipt>)->Result<SnapshotSet>{
 if !c.scope.exchanges.values().any(|id|id==market_id){return Err("LIVE_MARKET_OUTSIDE_SCOPE".into())}
 let account=read(Endpoint::Tournament)?;let positions=read(Endpoint::Positions)?;let open_orders=read(Endpoint::Orders{status:"open".into()})?;
 if account.data["status"]!="active"||open_orders.data["coverage"]["complete"]!=true||open_orders.data["pagination"]["hasMore"]!=false{return Err("LIVE_ACCOUNT_INCOMPLETE_OR_INACTIVE".into())}
 let cash_balance=read(Endpoint::Account)?;let market_id=market_id.to_owned();
 let market=read(Endpoint::Market{market_id:market_id.clone()})?;
 let contract=read(Endpoint::Nodes{market_id:market_id.clone()})?;
 let book=read(Endpoint::OrderBook{market_id,depth:20})?;
 Ok(SnapshotSet{account,cash_balance,positions,open_orders,market,contract,book})
}
/// Session already verified this bank's journal on open. Recover the ORIGINAL
/// adoption, not the derived transfer JSON, and verify its exact committed
/// source membership against that actual Session. Never load the whole journal.
fn committed_adoption(bank:&Session,path:&Path)->Result<(String,u64,Value)>{
 use std::io::BufReader;
 let metadata=fs::symlink_metadata(path)?;if !metadata.is_file()||metadata.file_type().is_symlink(){return Err("LIVE_ADOPTION_ORIGINAL_JOURNAL_REQUIRED".into())}
 committed_adoption_from(bank,BufReader::new(fs::File::open(path)?))
}
fn committed_adoption_from(bank:&Session,mut reader:impl std::io::BufRead)->Result<(String,u64,Value)>{
 use std::io::{Read,BufRead};
 const MAX_ADOPTION_ROW:usize=8*1024*1024;
 let report=bank.engine().report()?;let inherited=&report["inheritedInventory"];
 if inherited["receiptHash"].as_str().is_none()||!inherited["sourcePositions"].is_array(){return Err("LIVE_COMMITTED_ADOPTION_REQUIRED".into())}
 let mut line=Vec::new();loop{
  line.clear();let n=reader.by_ref().take((MAX_ADOPTION_ROW+1) as u64).read_until(b'\n',&mut line)?;if n==0{break}
  if line.len()>MAX_ADOPTION_ROW||line.last()!=Some(&b'\n'){return Err("LIVE_ADOPTION_JOURNAL_ROW_BOUND_OR_TORN".into())}
  let row:Value=serde_json::from_slice(&line)?;let event=&row["event"];if event["type"]!="actual_position_adoption"{continue}
  let reference=&event["sourceRef"];let id=reference["id"].as_str().ok_or("LIVE_ADOPTION_SOURCE_REF")?;let sequence=reference["sequence"].as_u64().filter(|n|*n>0).ok_or("LIVE_ADOPTION_SOURCE_REF")?;
  let mut original=event.clone();original.as_object_mut().ok_or("LIVE_ADOPTION_EVENT")?.remove("sourceRef");let sha=hash(&original);
  if reference["hash"]!=sha||bank.committed_source_hash(id,sequence)!=Some(sha.as_str())||event["receiptHash"]!=inherited["receiptHash"]||event["positions"]!=inherited["sourcePositions"]||event["authority"]!="human_moloch_inherited_management_20261015"{return Err("LIVE_COMMITTED_ADOPTION_MISMATCH".into())}
  return Ok((id.into(),sequence,event.clone()))
 }Err("LIVE_COMMITTED_ADOPTION_REQUIRED".into())
}
fn open_worker_fundamental(c:&Control,loaded:fundamental::LoadedSettings,bank:&mut Session,log:&mut DispatchLog,lease:Arc<ExclusiveLock>,t:&Transport)->Result<(Cohorts,ReviewAuthority,Vec<String>,std::collections::BTreeSet<String>)>{
 let existing=loaded.existing_exchanges();
 let(cohorts,authority,stale)=if existing.is_empty(){loaded.open(&c.root,bank,log,lease,now())?}else{
  let(id,sequence,event)=committed_adoption(bank,&c.root.join(&c.bank_name).join("events.jsonl"))?;
  t.package_queue_quiet()?;
  // Scope selection is not adoption authority. The actual committed source,
  // inherited FIFO, queue, lease, report and policy remain checked separately.
  for ex in &existing{retire_existing_drafts(bank,log,ex)?;}
  loaded.open_with_existing(&c.root,bank,log,lease,now(),t,&id,sequence,&event)?
 };Ok((cohorts,authority,stale,existing))
}
/// Recovery is unconditional: removing new-entry configuration never drops
/// already durable package liabilities or opens a second transport.
fn recover_packages(t:&mut Transport,c:&Control,bank:&mut Session,ordinary:&DispatchLog,authority:&ProductionAuthority,packages:&mut moloch_live_runtime::package_dispatch::PackageDispatchLog)->Result<bool>{
 for p in bank.engine().package_records().as_array().ok_or("LIVE_PACKAGE_RECORDS")?{let id=p["packageId"].as_str().ok_or("LIVE_PACKAGE_ID")?;if packages.record(id).is_none(){return Err("LIVE_PACKAGE_ORIGINAL_JOURNAL_MISSING".into())}}
 let receipts=c.root.join("package-receipts");let mut backend=moloch_live_runtime::package_worker::Live{transport:t,authority,ordinary_dispatch:ordinary,receipts:&receipts};
 let legacy=moloch_live_runtime::package_worker::recover(bank,packages,&mut backend)?;let portfolio=moloch_live_runtime::package_portfolio_worker::recover(bank,packages,&mut backend)?;Ok(legacy.blocks_new_submissions||portfolio.blocks_new_submissions)
}
fn canonical_package_receipt(census:&moloch_live_runtime::relationship_census::Census)->Result<Receipt>{
 let source=census.report.relationships.source.as_ref().ok_or("LIVE_PACKAGE_CANONICAL_UNAVAILABLE")?;
 let name=Path::new(&source.private_file);if name.components().count()!=1||name.file_name().is_none(){return Err("LIVE_PACKAGE_CANONICAL_PATH".into())}
 let path=census.cache.private_evidence_directory.join(name);let m=fs::symlink_metadata(&path)?;if !m.is_file()||m.file_type().is_symlink()||m.len()>8*1024*1024{return Err("LIVE_PACKAGE_CANONICAL_FILE".into())}
 use std::io::Read;let mut bytes=Vec::new();fs::File::open(path)?.take(8*1024*1024+1).read_to_end(&mut bytes)?;if bytes.len()>8*1024*1024{return Err("LIVE_PACKAGE_CANONICAL_FILE".into())}let r:Receipt=serde_json::from_slice(&bytes)?;if feedback::digest(&r)?!=source.receipt_sha256||r.request_started_at!=source.request_started_at||r.received_at!=source.received_at||r.status!=source.status{return Err("LIVE_PACKAGE_CANONICAL_CHANGED".into())}Ok(r)
}
fn retire_for_package(bank:&mut Session,log:&mut DispatchLog,q:&Value)->Result<()>{
 let id=q["id"].as_u64().ok_or("LIVE_QUOTE_ID")?;if !log.never_attempted(id){return Err("LIVE_PACKAGE_COMPONENT_ALREADY_ATTEMPTED".into())}
 let proof=log.proof(id);log.append(json!({"kind":"not_sent","qid":id,"proof":proof}))?;
 accept(bank,&json!({"type":"unsubmitted_intent_retired","source":"moloch_live_dispatcher","orderId":id,"confirmedNetworkAttempts":0,"receiptHash":hash(&proof),"deferPlanningForPackage":true,"receivedAt":iso(now())}))
}
/// Keep physical liabilities and origin-bound reservations intact. This is the
/// SAME positive original-dispatch proof used by package preparation; absence
/// from an account snapshot is never proof that a draft was not submitted.
fn retire_existing_drafts(bank:&mut Session,log:&mut DispatchLog,exchange:&str)->Result<usize>{
 log.require_resolved_attempts()?;let mut retired=0;
 for q in bank.engine().order_records(){let id=q["id"].as_u64().ok_or("LIVE_QUOTE_ID")?;
  if q["exchangeId"]!=exchange||q["retired"]!=false||bank.engine().is_package_order(id)||q.get("reviewedInventoryExit").is_some()||q.get("reviewedFundamentalEntry").is_some(){continue}
  if !log.never_attempted(id)||bank.engine().live_order_bindings().contains_key(&id){continue}
  retire_for_package(bank,log,&q)?;retired+=1;
 }Ok(retired)
}
fn existing_target_busy(bank:&Session,log:&DispatchLog,exchange:&str)->Result<bool>{
 log.require_resolved_attempts()?;let orders=bank.engine().order_records();
 for id in log.acknowledgments.keys().filter(|id|!log.closed.contains(id)){
  let q=orders.iter().find(|q|q["id"]==*id).ok_or("LIVE_ACK_QUOTE_MISSING")?;if q["exchangeId"]==exchange{return Ok(true)}
 }
 Ok(orders.iter().any(|q|q["exchangeId"]==exchange&&q["retired"]==false&&(q.get("reviewedInventoryExit").is_some()||q.get("reviewedFundamentalEntry").is_some()||q["id"].as_u64().is_some_and(|id|bank.engine().is_package_order(id)||!log.never_attempted(id)))))
}
/// ONLY a reviewed native FRESH_MARKET_CAP denial calls this fallback. Yield
/// one entirely flat ordinary proposal market, with no intent/attempt/ACK,
/// package lease or reviewed origin. Never cancel a physical order to free it.
fn retire_flat_draft_competitor(bank:&mut Session,log:&mut DispatchLog,managed:&std::collections::BTreeSet<String>)->Result<Option<String>>{
 log.require_resolved_attempts()?;let orders=bank.engine().order_records();let mut exchanges=std::collections::BTreeSet::new();
 for q in &orders{if q["retired"]==false{exchanges.insert(q["exchangeId"].as_str().ok_or("LIVE_QUOTE_SCOPE")?.to_owned());}}
 for ex in exchanges{
  if managed.contains(&ex)||bank.engine().wallet.quantity(&ex,moloch_replay::Side::Yes)!=0||bank.engine().wallet.quantity(&ex,moloch_replay::Side::No)!=0||existing_target_busy(bank,log,&ex)?{continue}
  let target:Vec<_>=orders.iter().filter(|q|q["retired"]==false&&q["exchangeId"]==ex).collect();
  if target.is_empty()||target.iter().any(|q|q["id"].as_u64().is_none_or(|id|!log.never_attempted(id)||log.intents.contains_key(&id)||bank.engine().live_order_bindings().contains_key(&id)||bank.engine().is_package_order(id))||q.get("reviewedInventoryExit").is_some()||q.get("reviewedFundamentalEntry").is_some())||bank.engine().package_records().as_array().ok_or("LIVE_PACKAGE_RECORDS")?.iter().any(|p|p["phase"]!="no_execution"&&p["protectionReleased"]!=true&&(p["legs"].as_array().is_some_and(|a|a.iter().any(|l|l["exchangeId"]==ex))||p["portfolio"]["componentExchanges"].as_array().is_some_and(|a|a.iter().any(|e|e==&ex)))){continue}
  if retire_existing_drafts(bank,log,&ex)?>0{return Ok(Some(ex))}
 }Ok(None)
}
fn ordinary_liability_due(bank:&Session,log:&DispatchLog,at:u64)->Result<bool>{
 for q in bank.engine().order_records(){let id=q["id"].as_u64().ok_or("LIVE_QUOTE_ID")?;
  if bank.engine().is_package_order(id)||q["retired"]==true{continue}
  let expires=q["expiresAt"].as_u64().ok_or("LIVE_QUOTE_EXPIRY")?;
  let cancel_due=q["cancelEffectiveAt"].as_u64().is_some_and(|t|t<=at);
  if expires>at&&cancel_due&&!log.never_attempted(id)&&!log.closed.contains(&id)&&!log.cancel_acknowledged.contains(&id){return Ok(true)}
  if q["action"]=="sell"&&q["inventoryExitReason"]=="maximumAge"&&log.never_attempted(id)&&expires>at&&!cancel_due{return Ok(true)}
 }Ok(false)
}
fn allocation_budget(t:&Transport,c:&Control,bank:&Session,cash:&Receipt)->Result<moloch_live_runtime::package_allocation::Budget>{
 let state=t.package_queue_quiet()?;let at=now();
 if state.state.last_clock>at||state.state.writes.iter().any(|v|*v>at){return Err("LIVE_ALLOCATION_QUEUE_CLOCK".into())}
 let cap=c.policy["writesPerMinute"].as_u64().filter(|v|(1..=24).contains(v)).ok_or("LIVE_ALLOCATION_WRITE_CAP")?;
 let used=state.state.writes.iter().filter(|v|at.saturating_sub(**v)<=60_000).count() as u64;
 let available=if state.state.cooldown>at{0}else{cap.saturating_sub(used)};
 moloch_live_runtime::package_allocation::Budget::from_receipt(bank,&c.activation,cash,available as u32,at)
}
fn package_component_busy(bank:&Session,ordinary:&DispatchLog,exchanges:&std::collections::BTreeSet<String>)->bool{
 bank.engine().order_records().iter().any(|q|q["exchangeId"].as_str().is_some_and(|e|exchanges.contains(e))&&(ordinary.acknowledgments.contains_key(&q["id"].as_u64().unwrap_or(0))&&!ordinary.closed.contains(&q["id"].as_u64().unwrap_or(0))||q["retired"]==false&&!bank.engine().is_package_order(q["id"].as_u64().unwrap_or(0))&&!ordinary.never_attempted(q["id"].as_u64().unwrap_or(0))))
}
fn allocation_certificate(t:&mut Transport,c:&Control,bank:&Session,ordinary:&DispatchLog,authority:&ProductionAuthority,census:&moloch_live_runtime::relationship_census::Census,canonical:&Receipt,bundle:&moloch_live_runtime::package_settings::Bundle,approved:&[&moloch_live_runtime::relationship_strategy::SettlementCertificate],start:usize,config:&moloch_live_runtime::package_allocation::Settings,fairness:&mut moloch_live_runtime::package_allocation::Fairness)->Result<Option<usize>>{
 use moloch_live_runtime::{package_allocation as allocation,package_portfolio,relationship_strategy::{self,Certificates}};
 config.validate()?;let at=now();let mut reviewed=Vec::new();
 for n in 0..approved.len().min(32){let index=(start+n)%approved.len();let cert=approved[index];let digest=relationship_strategy::certificate_sha256(cert)?;if config.costs_for(&digest,at).is_some(){reviewed.push((index,digest));}}
 if reviewed.is_empty(){atomic_json(&c.root.join("package-allocation.json"),&json!({"at":iso(at),"state":"no_current_reviewed_cost_policy","executionAuthority":false}))?;return Ok(None)}
 let cash=request(t,&c.root,Endpoint::Account)?;let budget=match allocation_budget(t,c,bank,&cash){Ok(v)=>v,Err(e)=>{atomic_json(&c.root.join("package-allocation.json"),&json!({"at":iso(now()),"state":"budget_unavailable","reason":e.to_string(),"executionAuthority":false}))?;return Ok(None)}};let settings=c.packages.as_ref().ok_or("LIVE_ALLOCATION_PACKAGE_SETTINGS")?;
 let mut hints=Vec::new();let mut fallback=None;let mut deferred=std::collections::BTreeMap::new();
 for(index,digest)in &reviewed{
  let cert=approved[*index];let exchanges=cert.contracts.iter().map(|v|v.exchange_id.clone()).collect();
  if package_component_busy(bank,ordinary,&exchanges){deferred.insert(digest.clone(),"existing_physical_component_liability".to_string());continue}
  let Some(costs)=config.costs_for(digest,now())else{continue};if fallback.is_none(){fallback=Some(*index)}
  let policy=settings.policy(&c.activation,now(),cert.contracts.len())?;let certs=Certificates{values:std::slice::from_ref(cert),trusted_sha256:&settings.trusted_certificate_sha256,rule_receipts:&bundle.original_receipts};
  let component=match relationship_strategy::certified_component(census,canonical,&certs,&cert.relationship_id,&policy){Ok(v)=>v,Err(e)=>{deferred.insert(digest.clone(),e.to_string());continue}};
  let records=bank.engine().package_records();let parent=records.as_array().ok_or("LIVE_PACKAGE_RECORDS")?.iter().find(|p|p["phase"]=="complete"&&p.get("protectionReleased")!=Some(&json!(true))&&p["legs"].as_array().is_some_and(|legs|!legs.is_empty()&&legs.iter().all(|l|l["exchangeId"].as_str().is_some_and(|e|exchanges.contains(e))))).and_then(|p|p["packageId"].as_str());
  if let Some(parent)=parent{if let Ok(Some(plan))=package_portfolio::whole_unwind(bank,&c.activation,&component,parent,&policy,authority.entry_until_ms()){
   if let Ok(hint)=allocation::ReadHint::portfolio(bank,&plan,&budget,&costs,now()){hints.push(hint);continue}
  }}
  match allocation::ReadHint::scan(bank,&c.activation,&component,&policy,&budget,&costs){Ok(Some(hint))=>hints.push(hint),Ok(None)=>{deferred.insert(digest.clone(),"no_current_funded_hint".into());},Err(e)=>{deferred.insert(digest.clone(),e.to_string());}}
 }
 let(selected,decision)=match allocation::select_hints(bank,&budget,fairness,hints,now()){Ok(v)=>v,Err(e)=>{atomic_json(&c.root.join("package-allocation.json"),&json!({"at":iso(now()),"state":"selection_unavailable","reason":e.to_string(),"executionAuthority":false}))?;return Ok(None)}};
 let index=if let Some(hint)=&selected{reviewed.iter().find(|(_,digest)|digest==hint.certificate_sha256()).map(|(index,_)|*index)}else{fallback};
 atomic_json(&c.root.join("package-allocation.json"),&json!({"at":iso(now()),"state":if selected.is_some(){"ranked_source_refresh"}else{"reviewed_source_refresh_only"},"decision":decision,"sourceDeferred":deferred,"executionAuthority":false}))?;Ok(index)
}
fn allocation_plan_selected(t:&Transport,c:&Control,bank:&Session,cash:&Receipt,admission:Option<&moloch_live_runtime::package_admission::Admission>,plan:Option<&moloch_live_runtime::package_portfolio::Plan>)->Result<bool>{
 use moloch_live_runtime::package_allocation as allocation;
 let Some(config)=&c.package_allocation else{return Ok(true)};let at=now();
 let certificate=match(admission,plan){(Some(p),None)=>p.certificate_sha256(),(None,Some(p))=>p.proof()["component"]["certificateSha256"].as_str().ok_or("LIVE_ALLOCATION_CERTIFICATE")?,_=>return Err("LIVE_ALLOCATION_PLAN_KIND".into())};
 let Some(costs)=config.costs_for(certificate,at)else{return Ok(false)};let budget=match allocation_budget(t,c,bank,cash){Ok(v)=>v,Err(e)=>{atomic_json(&c.root.join("package-allocation-final.json"),&json!({"at":iso(at),"state":"budget_unavailable","reason":e.to_string(),"executionAuthority":false}))?;return Ok(false)}};
 let eligibility=if let Some(p)=admission{allocation::Eligible::admission(bank,p,&budget,&costs,at)}else{allocation::Eligible::portfolio(bank,plan.unwrap(),&budget,&costs,at)};
 let eligible=match eligibility{Ok(v)=>v,Err(e)=>{atomic_json(&c.root.join("package-allocation-final.json"),&json!({"at":iso(at),"state":"plan_unavailable","reason":e.to_string(),"executionAuthority":false}))?;return Ok(false)}};
 // Final eligibility must not erase other candidates' source-selection waits.
 let mut validation_fairness=allocation::Fairness::new(config.maximum_wait)?;
 let(selected,decision)=allocation::select_plans(bank,&budget,&mut validation_fairness,vec![eligible],at)?;
 atomic_json(&c.root.join("package-allocation-final.json"),&json!({"at":iso(at),"decision":decision,"executionAuthority":false}))?;Ok(selected.is_some())
}
/// One bounded certificate cohort per scan. All reads/writes borrow the existing
/// account worker. Every price still passes the ordinary source adapter and the
/// complete physical package authority; indicative selection is not authority.
fn package_opportunity(t:&mut Transport,c:&Control,bank:&mut Session,ordinary:&mut DispatchLog,authority:&ProductionAuthority,packages:&mut moloch_live_runtime::package_dispatch::PackageDispatchLog,census:&mut Option<moloch_live_runtime::relationship_census::Census>,source_state:&mut RestSource,stream:&Realtime,breadth:&mut Option<Recorder>,universe:&mut Option<Universe>,windows:&mut std::collections::BTreeMap<String,u64>,last_scan:&mut u64,cursor:&mut usize,allocation_fairness:&mut Option<moloch_live_runtime::package_allocation::Fairness>,fundamental_exchanges:&std::collections::BTreeSet<String>)->Result<()>{
 use moloch_live_runtime::{package_worker,package_admission,package_portfolio,package_portfolio_worker,relationship_strategy::{self,Certificates}};
 let Some(settings)=&c.packages else{return Ok(())};let at=now();if !authority.can_read(&c.scope){return Err("LIVE_PACKAGE_OWNER_LOST".into())}require_stream(stream)?;
 // Existing due cancellations and covered inventory exits retain priority over
 // this optional, expensive new-entry source cohort on every pass.
 if ordinary_liability_due(bank,ordinary,at)?{return Ok(())}
 if *last_scan>at{return Err("LIVE_PACKAGE_SCAN_CLOCK".into())}if *last_scan>0&&at-*last_scan<settings.refresh_ms{return Ok(())}*last_scan=at;
 if packages.unresolved()||bank.engine().package_blocks_submissions(){return Ok(())}
 let bundle=settings.load(&c.scope)?;
 let approved=settings.approved(&bundle,now())?;
 if approved.is_empty(){atomic_json(&c.root.join("package-status.json"),&json!({"at":iso(now()),"state":"no_current_independently_approved_certificate","ordersSubmitted":0}))?;return Ok(())}
 if census.as_ref().is_none_or(|v|at.saturating_sub(v.report.completed_at_ms)>=settings.refresh_ms){
  let fresh=moloch_live_runtime::relationship_census::capture(t,&c.scope,&c.root.join("relationship-receipts"),&[]);
  atomic_json(&c.root.join("relationship-census.json"),&serde_json::to_value(&fresh.report)?)?;
  if fresh.report.transport_guard_observed{return Err("LIVE_PACKAGE_CENSUS_GUARD".into())}*census=Some(fresh);
  drain_stream(stream,bank,source_state,breadth,universe)?;
 }
 let start=*cursor%approved.len();*cursor=(*cursor).wrapping_add(1);
 let selected=if let Some(config)=&c.package_allocation{
  let Some(current)=census.as_ref()else{return Ok(())};
  if current.cache.relationships.is_none()||current.report.active_relationship_count==0{return Ok(())}
  let canonical=canonical_package_receipt(current)?;
  let selected=allocation_certificate(t,c,bank,ordinary,authority,current,&canonical,&bundle,&approved,start,config,allocation_fairness.as_mut().ok_or("LIVE_ALLOCATION_FAIRNESS")?)?;
  let Some(selected)=selected else{return Ok(())};selected
 }else{start};
 let chosen=approved[selected];if chosen.contracts.iter().any(|r|fundamental_exchanges.contains(&r.exchange_id)){return Ok(())}
 let exchanges:std::collections::BTreeSet<_>=chosen.contracts.iter().map(|r|r.exchange_id.clone()).collect();
 let held=chosen.contracts.iter().any(|r|bank.engine().wallet.quantity(&r.exchange_id,moloch_replay::Side::Yes)>0||bank.engine().wallet.quantity(&r.exchange_id,moloch_replay::Side::No)>0);
 let records=bank.engine().order_records();
 // ACKed orders remain liabilities until authoritative closure even when native
 // expiry or a full native fill has already retired the quote.
 if records.iter().any(|q|q["exchangeId"].as_str().is_some_and(|e|exchanges.contains(e))&&(ordinary.acknowledgments.contains_key(&q["id"].as_u64().unwrap_or(0))&&!ordinary.closed.contains(&q["id"].as_u64().unwrap_or(0))||q["retired"]==false&&!bank.engine().is_package_order(q["id"].as_u64().unwrap_or(0))&&!ordinary.never_attempted(q["id"].as_u64().unwrap_or(0)))){return Ok(())}
 if !held&&now()>=authority.entry_until_ms(){return Ok(())}
 let Some(current)=census.as_ref() else{return Ok(())};if current.cache.relationships.is_none()||current.report.active_relationship_count==0{return Ok(())}
 let canonical=canonical_package_receipt(current)?;let mut rules=bundle.original_receipts.clone();
 for contract in &chosen.contracts{
  rules.push(request(t,&c.root,Endpoint::Nodes{market_id:contract.market_id.clone()})?);
  if !authority.can_read(&c.scope){return Err("LIVE_PACKAGE_OWNER_LOST".into())}require_stream(stream)?;
  drain_stream(stream,bank,source_state,breadth,universe)?;
  let from=*windows.get(&contract.exchange_id).ok_or("LIVE_PACKAGE_SOURCE_WINDOW")?;
  let from=source_state.recovery_start_ms(&contract.market_id).map_or(from,|r|from.min(r));let to=now();
  for(kind,endpoint)in [("trades",Endpoint::Trades{exchange_id:contract.exchange_id.clone(),from:iso(from),to:iso(to)}),("book",Endpoint::OrderBook{market_id:contract.market_id.clone(),depth:20})]{
   let r=request(t,&c.root,endpoint)?;if !authority.can_read(&c.scope){return Err("LIVE_PACKAGE_OWNER_LOST".into())}require_stream(stream)?;
   let wrap=json!({"source":"official_sig_api_v1","kind":kind,"tournamentId":c.scope.tournament_id,"marketId":contract.market_id,"exchangeId":contract.exchange_id,"endpoint":r.endpoint,"status":r.status,"requestStartedAt":r.request_started_at,"receivedAt":r.received_at,"data":r.data});accept_source_events(bank,&source_state.accept(&wrap)?)?;
   drain_stream(stream,bank,source_state,breadth,universe)?;
  }windows.insert(contract.exchange_id.clone(),to.saturating_sub(2000));
 }
 // Do not clear a resting/unknown account order to make a package fit. Only
 // original dispatcher-proven, never-physical native quotes can yield priority.
 let quotes:Vec<_>=bank.engine().order_records().into_iter().filter(|q|q["retired"]==false&&!bank.engine().is_package_order(q["id"].as_u64().unwrap_or(0))&&q["exchangeId"].as_str().is_some_and(|e|exchanges.contains(e))).collect();
 if quotes.iter().any(|q|!ordinary.never_attempted(q["id"].as_u64().unwrap_or(0))){return Ok(())}
 for q in &quotes{retire_for_package(bank,ordinary,q)?;}
 // No source/tick occurs between retirement and the durable package reserve.
 // Source refresh/retirement invalidated any hint's head. Obtain actual current
 // cash on the same Transport and rebuild the selected opaque financial plan.
 let cash_receipt=request(t,&c.root,Endpoint::Account)?;
 let cash=baal_transport_service::relationships::parse_buying_power(&cash_receipt.data,&c.scope)?.gross_cash_units.ok_or("LIVE_PACKAGE_GROSS_CASH_UNAVAILABLE")?;
 let at=now();let policy=settings.policy(&c.activation,at,chosen.contracts.len())?;
 let certs=Certificates{values:std::slice::from_ref(chosen),trusted_sha256:&settings.trusted_certificate_sha256,rule_receipts:&rules};
 if held{
  let component=relationship_strategy::certified_component(census.as_ref().unwrap(),&canonical,&certs,&chosen.relationship_id,&policy)?;
  let native_packages=bank.engine().package_records();let parent=native_packages.as_array().ok_or("LIVE_PACKAGE_RECORDS")?.iter().find(|p|p["phase"]=="complete"&&p.get("protectionReleased")!=Some(&json!(true))&&p["legs"].as_array().is_some_and(|legs|!legs.is_empty()&&legs.iter().all(|l|l["exchangeId"].as_str().is_some_and(|e|exchanges.contains(e))))).and_then(|p|p["packageId"].as_str());
  // A covered whole-package SELL remains eligible after the BUY deadline.
  // Acquisition defaults to a positive STATEWISE delta; reviewed floor hedges
  // never gain automatic execution authority from a floor difference.
  let mut plan=if let Some(id)=parent{package_portfolio::whole_unwind(bank,&c.activation,&component,id,&policy,authority.entry_until_ms())?}else{None};
  if plan.is_none()&&now()<authority.entry_until_ms(){plan=package_portfolio::acquire(bank,&c.activation,&component,cash,&policy,authority.entry_until_ms())?;}
  let Some(plan)=plan else{atomic_json(&c.root.join("package-status.json"),&json!({"at":iso(now()),"state":"no_current_portfolio_executable_edge","ordersSubmitted":0}))?;return Ok(())};
  if !allocation_plan_selected(t,c,bank,&cash_receipt,None,Some(&plan))?{return Ok(())}
  if plan.minimum_statewise_delta()<=0{atomic_json(&c.root.join("package-status.json"),&json!({"at":iso(now()),"state":"portfolio_floor_hedge_requires_explicit_risk_policy","ordersSubmitted":0}))?;return Ok(())}
  let permit=Permit{run_id:c.activation.run_id.clone(),intent_id:plan.package_id().into(),owner:"Codex".into(),expires_at_ms:plan.expires_at_ms(),maximum_quantity:c.activation.maximum_quantity,maximum_cost:money(plan.gross_funding())};
  let activation=plan.proof()["atMs"].as_u64().ok_or("LIVE_PACKAGE_ACTIVATION")?.checked_add(bank.engine().policy()["latencyMs"].as_u64().ok_or("LIVE_PACKAGE_ACTIVATION")?).ok_or("LIVE_PACKAGE_ACTIVATION")?;
  let receipts=c.root.join("package-receipts");let mut backend=package_worker::Live{transport:t,authority,ordinary_dispatch:ordinary,receipts:&receipts};
  let staged=package_portfolio_worker::stage(bank,packages,&backend,plan,permit,package_portfolio_worker::RiskPolicy::StatewiseOnly)?;
  while now()<activation{if !authority.can_read(&c.scope){return Err("LIVE_PACKAGE_OWNER_LOST".into())}require_stream(stream)?;drain_stream(stream,bank,source_state,breadth,universe)?;std::thread::sleep(Duration::from_millis(activation.saturating_sub(now()).min(100)));}
  let facts=moloch_live_runtime::package_preflight::collect_portfolio_plan(&mut backend,&c.scope,staged.plan(),staged.basket(),|r|{
   record_receipt(&c.root,r)?;if !authority.can_read(&c.scope){return Err("LIVE_PACKAGE_OWNER_LOST".into())}require_stream(stream)?;
   for contract in &chosen.contracts{if r.endpoint==format!("/markets/{}/orderbook?tournamentId={}&depth=20",contract.market_id,c.scope.tournament_id){let wrap=json!({"source":"official_sig_api_v1","kind":"book","tournamentId":c.scope.tournament_id,"marketId":contract.market_id,"exchangeId":contract.exchange_id,"endpoint":r.endpoint,"status":r.status,"requestStartedAt":r.request_started_at,"receivedAt":r.received_at,"data":r.data});accept_source_events(bank,&source_state.accept(&wrap)?)?;}}
   drain_stream(stream,bank,source_state,breadth,universe)
  });
  let facts=match facts{Ok(f)=>f,Err(error)=>{package_portfolio_worker::recover(bank,packages,&mut backend)?;return Err(error)}};
  let result=package_portfolio_worker::submit_once(bank,packages,&mut backend,staged,facts)?;let recovered=package_portfolio_worker::recover(bank,packages,&mut backend)?;
  atomic_json(&c.root.join("package-status.json"),&json!({"at":iso(now()),"state":format!("{result:?}"),"mode":"portfolio","blocksNewSubmissions":recovered.blocks_new_submissions,"actualFillEvents":recovered.fill_events,"completedPackages":recovered.completed_packages,"profitabilityEstablished":false}))?;return Ok(())
 }
 if now()>=authority.entry_until_ms(){return Ok(())}
 let admission=package_admission::admit(bank,&c.activation,census.as_ref().unwrap(),&canonical,&certs,cash,&policy,authority.entry_until_ms())?;
 let Some(admission)=admission else{atomic_json(&c.root.join("package-status.json"),&json!({"at":iso(now()),"state":"no_current_certified_executable_surplus","ordersSubmitted":0}))?;return Ok(())};
 if !allocation_plan_selected(t,c,bank,&cash_receipt,Some(&admission),None)?{return Ok(())}
 let permit=Permit{run_id:c.activation.run_id.clone(),intent_id:admission.package_id().into(),owner:"Codex".into(),expires_at_ms:admission.expires_at_ms(),maximum_quantity:c.activation.maximum_quantity,maximum_cost:admission.basket().maximum_cost.clone()};
 let receipts=c.root.join("package-receipts");let mut backend=package_worker::Live{transport:t,authority,ordinary_dispatch:ordinary,receipts:&receipts};
 let staged=package_worker::stage(bank,packages,&backend,&admission,permit)?;
 let activation=admission.proof()["atMs"].as_u64().ok_or("LIVE_PACKAGE_ACTIVATION")?.checked_add(bank.engine().policy()["latencyMs"].as_u64().ok_or("LIVE_PACKAGE_ACTIVATION")?).ok_or("LIVE_PACKAGE_ACTIVATION")?;
 while now()<activation{if !authority.can_read(&c.scope){return Err("LIVE_PACKAGE_OWNER_LOST".into())}require_stream(stream)?;drain_stream(stream,bank,source_state,breadth,universe)?;std::thread::sleep(Duration::from_millis(activation.saturating_sub(now()).min(100)));}
 let facts=moloch_live_runtime::package_preflight::collect(&mut backend,&c.scope,admission.basket(),|r|{
  record_receipt(&c.root,r)?;if !authority.can_read(&c.scope){return Err("LIVE_PACKAGE_OWNER_LOST".into())}require_stream(stream)?;
  // Real books enter via RestSource; it can withhold them pending complete tape.
  for contract in &chosen.contracts{if r.endpoint==format!("/markets/{}/orderbook?tournamentId={}&depth=20",contract.market_id,c.scope.tournament_id){let wrap=json!({"source":"official_sig_api_v1","kind":"book","tournamentId":c.scope.tournament_id,"marketId":contract.market_id,"exchangeId":contract.exchange_id,"endpoint":r.endpoint,"status":r.status,"requestStartedAt":r.request_started_at,"receivedAt":r.received_at,"data":r.data});accept_source_events(bank,&source_state.accept(&wrap)?)?;}}
  drain_stream(stream,bank,source_state,breadth,universe)
 });
 let facts=match facts{Ok(f)=>f,Err(error)=>{package_worker::recover(bank,packages,&mut backend)?;return Err(error)}};
 let result=package_worker::submit_once(bank,packages,&mut backend,staged,facts)?;
 let recovered=package_worker::recover(bank,packages,&mut backend)?;
 atomic_json(&c.root.join("package-status.json"),&json!({"at":iso(now()),"state":format!("{result:?}"),"blocksNewSubmissions":recovered.blocks_new_submissions,"actualFillEvents":recovered.fill_events,"completedPackages":recovered.completed_packages,"profitabilityEstablished":false}))?;Ok(())
}
fn cancel_snapshots(t:&mut Transport,c:&Control)->Result<CancelSnapshotSet>{
 // Four real reads through the existing sole transport/limiter; no unused
 // market/nodes/book GETs, invented receipts or reuse across causal boundaries.
 cancel_snapshots_with(|endpoint|request(t,&c.root,endpoint))
}
fn cancel_snapshots_with(mut read:impl FnMut(Endpoint)->Result<Receipt>)->Result<CancelSnapshotSet>{
 let account=read(Endpoint::Tournament)?;let positions=read(Endpoint::Positions)?;
 let open_orders=read(Endpoint::Orders{status:"open".into()})?;
 if account.data["status"]!="active"||open_orders.data["coverage"]["complete"]!=true||open_orders.data["pagination"]["hasMore"]!=false{return Err("LIVE_ACCOUNT_INCOMPLETE_OR_INACTIVE".into())}
 let cash_balance=read(Endpoint::Account)?;
 Ok(CancelSnapshotSet{account,cash_balance,positions,open_orders})
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
fn record_original_attempt(bank:&Session,log:&mut DispatchLog,qid:u64,config:Option<&moloch_live_runtime::execution_edge::Config>,at:u64)->Result<()>{
 let q=quote(bank,qid)?;
 if !log.never_attempted(qid)||log.intents.get(&qid).is_none_or(|intent|intent["quote"]!=q){return Err("LIVE_ATTEMPT_ORIGINAL_QUOTE_REQUIRED".into())}
 let sources:[(&str,u64);2]=[("moloch-live",bank.last_source_sequence("moloch-live")),("moloch-package",bank.last_source_sequence("moloch-package"))];
 let frontier:Value=sources.into_iter().map(|(source,sequence)|(source.to_owned(),json!({"sequence":sequence,"originalPreparedEventSha256":bank.committed_source_hash(source,sequence)}))).collect::<serde_json::Map<String,Value>>().into();
 // Match the observer's typed Config serialization, rather than reordering it
 // through a JSON Value and assigning a different digest to the same policy.
 let config_sha=config.map(|value|serde_json::to_vec(value).map(|bytes|format!("{:x}",Sha256::digest(bytes)))).transpose()?;
 let event=json!({"kind":"attempt","qid":qid,"at":iso(at),"captureAtMs":at,"evidenceVersion":1,"nativeHead":bank.durable_head(),"sourceFrontier":frontier,"originalQuoteSha256":hash(&q),"dispatchBefore":log.proof(qid),"modelConfigSha256":config_sha,"modelAlgorithmSha256":format!("{:x}",Sha256::digest(include_bytes!("execution_edge.rs")))});
 log.append(event)?;moloch_live_runtime::execution_distribution::track_runtime_attempt(bank,log,qid,at);if config.is_some(){moloch_live_runtime::execution_edge::track_runtime_attempt(bank,log,qid,at);}Ok(())
}
fn submit_order(t:&mut Transport,c:&Control,bank:&mut Session,log:&mut DispatchLog,authority:&Arc<ProductionAuthority>,q:&Value,facts:SnapshotSet,maker:Option<(Arc<moloch_live_runtime::maker_policy::Decision>,&moloch_live_runtime::maker_policy::Reviewed)>,mut fundamental:Option<&mut FundamentalRuntime>)->Result<()>{
 let qid=q["id"].as_u64().ok_or("LIVE_QUOTE_ID")?;let(mut order,mut request_spec,mut p)=physical_order(c,q)?;
 let fundamental_decision=if let Some(f)=fundamental.as_mut(){
  f.authority.validate()?;
  if !f.eligible(bank,log,&c.scope,qid,now())?{authority.clear_ready();return Ok(())}
  let n=f.cohorts.planned_entry_quantity(bank,log,&c.scope,qid,f.authority.trusted(),now())?.unwrap_or(u64::from(order.quantity));
  order.quantity=u32::try_from(n)?;request_spec=spec(&order)?;p.maximum_quantity=order.quantity;p.maximum_cost=money(decimal_units(&order.price)?.checked_mul(i128::from(n)).ok_or("LIVE_COST_OVERFLOW")?);
  f.cohorts.bind_quantity(bank,log,&c.scope,qid,n,&request_spec,f.authority.trusted(),now())?;
  Some(Arc::new(f.cohorts.physical_decision(bank,log,&c.scope,qid,&request_spec,&p,&f.authority,now())?))
 }else{None};
 if let Some((decision,review))=&maker{if let Err(error)=decision.validate_current(bank,review,log,now()){
  if !expected_maker_expiry(error.as_ref()){return Err(error)}authority.clear_ready();atomic_json(&c.root.join("maker-policy-deferred.json"),&json!({"at":iso(now()),"qid":qid,"reason":error.to_string(),"physicalAttempted":false}))?;return Ok(())
 }}
 authority.prepare_with_policies(bank,qid,request_spec.clone(),p.clone(),facts,maker.as_ref().map(|(decision,_)|decision.clone()),fundamental_decision)?;
 if let Err(e)=authority.authorize(&c.scope,&request_spec,&p,false,now()){authority.clear_ready();atomic_json(&c.root.join("deferred.json"),&json!({"at":iso(now()),"qid":qid,"code":e.code}))?;return Ok(())}
 if log.intents.contains_key(&qid)||!log.never_attempted(qid){return Err("LIVE_EXISTING_UNRESOLVED_INTENT".into())}
 log.append(json!({"kind":"intent","qid":qid,"quote":q,"spec":request_spec,"permit":p,"at":iso(now()),"maker":maker.as_ref().map(|(decision,_)|decision.proof()),"fundamental":fundamental.as_ref().and_then(|f|f.cohorts.durable_tag(qid))}))?;
 record_original_attempt(bank,log,qid,if fundamental.is_some(){None}else{c.execution_model.as_ref()},now())?;
 if let Some(f)=fundamental.as_ref(){f.authority.validate()?;f.cohorts.check_bound(bank,log,&c.scope,qid,&request_spec,f.authority.trusted(),now())?;}
 let ack=match t.place(order,p){Ok(r)=>r,Err(e)=>{log.append(json!({"kind":"error","qid":qid,"error":e,"at":iso(now())}))?;return Err("LIVE_WRITE_NOT_CONFIRMED_NO_RETRY".into())}};
 record_receipt(&c.root,&ack)?;log.append(json!({"kind":"ack","qid":qid,"receipt":ack}))?;
 let event=feedback::acknowledgment(q,&ack,&iso(now()))?;if let Some(f)=fundamental.as_ref(){f.cohorts.validate_acknowledgment(qid,&event)?;}authority.bind_owned(event["actualOrderId"].as_str().ok_or("LIVE_ACTUAL_ID")?.into());accept(bank,&event)?;authority.clear_ready();Ok(())
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
fn refresh_exit(t:&mut Transport,c:&Control,bank:&mut Session,log:&mut DispatchLog,authority:&Arc<ProductionAuthority>,source:&mut RestSource,stream:&Realtime,old:&Value,hint:&moloch_live_runtime::exit_refresh::ReadHint,flow_started:u64,maximum_qid_before:u64,unknown:bool,prior_window:u64,fundamental:Option<&mut FundamentalRuntime>)->Result<()>{
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
 submit_order(t,c,bank,log,authority,plan.quote(),facts,None,fundamental)
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
fn existing_exit_deferred(c:&Control,exchange:&str,stage:&str,reason:&str)->Result<()>{
 atomic_json(&c.root.join("reviewed-existing-exit.json"),&json!({"at":iso(now()),"exchangeId":exchange,"stage":stage,"reason":reason,"physicalAttempted":false,"costFloorChanged":false}))
}
/// Conservative pre-creation clock check. ProductionAuthority rechecks these
/// same original receipts and exact economics at the physical boundary.
fn existing_facts_until(f:&SnapshotSet,at:u64)->Result<Option<u64>>{
 let mut until=u64::MAX;
 for(r,age)in [(&f.account,20_000),(&f.positions,20_000),(&f.open_orders,20_000),(&f.cash_balance,20_000),(&f.market,10_000),(&f.contract,10_000),(&f.book,10_000)]{
  let start=moloch_live_runtime::daily_windows::clock(&r.request_started_at)?;let end=moloch_live_runtime::daily_windows::clock(&r.received_at)?;
  if r.status!=200||start>end||end>at{return Ok(None)}
  until=until.min(start.checked_add(age).ok_or("LIVE_EXISTING_FACT_CLOCK_RANGE")?).min(end.checked_add(age).ok_or("LIVE_EXISTING_FACT_CLOCK_RANGE")?);
 }Ok((at<until).then_some(until))
}
fn existing_reservation_stage(q:&Value,original:&Value,at:u64,facts_until:u64)->Result<Option<u64>>{
 if q!=original{return Err("LIVE_EXISTING_RESERVED_QUOTE_CHANGED".into())}
 if q["reviewedInventoryExit"].is_null()||q["action"]!="sell"||q["retired"]!=false||q["cancelEffectiveAt"].is_u64()||q["expiresAt"].as_u64().is_none_or(|end|end<=at)||at>=facts_until{return Err("LIVE_EXISTING_RESERVATION_OR_FACTS_EXPIRED".into())}
 let activation=q["activationAt"].as_u64().ok_or("LIVE_QUOTE_ACTIVATION")?;Ok((activation>at).then_some(activation))
}
/// No physical authority is minted here. Only a successful original Session
/// commit may expose the genuine new qid to bind/intent/attempt/SDK execution.
fn commit_existing_reservation(bank:&mut Session,event:&Value)->Result<Option<Value>>{
 let before=bank.engine().order_records().iter().filter_map(|q|q["id"].as_u64()).max().unwrap_or(0);
 let mut event=event.clone();event["processedAt"]=json!(iso(now()));let mut preview=bank.engine().clone();
 if let Err(error)=preview.advance(&event){if matches!(error,"REVIEWED_EXIT_WRITE_BUDGET"|"REVIEWED_EXIT_BOOK"|"REVIEWED_EXIT_CLOCK"){return Ok(None)}return Err(error.into())}
 accept(bank,&event)?;
 let candidates:Vec<_>=bank.engine().order_records().into_iter().filter(|q|q["id"].as_u64().is_some_and(|n|n>before)&&q["retired"]==false&&q["exchangeId"]==event["exchangeId"]&&q["action"]=="sell"&&q["reviewedInventoryExit"]["originSha256"]==event["originSha256"]&&q["reviewedInventoryExit"]["reviewSha256"]==event["reviewSha256"]).collect();
 if candidates.len()!=1{return Err("LIVE_EXISTING_NATIVE_RESERVATION_NOT_UNIQUE".into())}Ok(candidates.into_iter().next())
}
fn reviewed_existing_exit(t:&mut Transport,c:&Control,bank:&mut Session,log:&mut DispatchLog,authority:&Arc<ProductionAuthority>,source:&mut RestSource,stream:&Realtime,f:&mut FundamentalRuntime,hint:&fundamental::ExistingExitHint,prior_window:u64)->Result<()>{
 let ex=hint.exchange_id.as_str();let market=hint.market_id.as_str();refresh_controls(c,authority,stream)?;log.require_resolved_attempts()?;f.authority.validate()?;
 if existing_target_busy(bank,log,ex)?{return Ok(())}
 // A REST book cannot heal an unobserved tape interval by itself.
 if let Some(from)=exit_recovery_from(source,market,prior_window){
  let through=now();let tape=request(t,&c.root,Endpoint::Trades{exchange_id:ex.into(),from:iso(from),to:iso(through)})?;
  let wrapped=json!({"source":"official_sig_api_v1","kind":"trades","tournamentId":c.scope.tournament_id,"marketId":market,"exchangeId":ex,"endpoint":tape.endpoint,"status":tape.status,"requestStartedAt":tape.request_started_at,"receivedAt":tape.received_at,"data":tape.data});
  accept_source_events(bank,&source.accept(&wrapped)?)?;
  let scopes=source.scopes();if !scopes.as_array().ok_or("LIVE_SOURCE_SCOPES")?.iter().find(|s|s["marketId"]==market).ok_or("LIVE_SOURCE_SCOPE")?["tapeReady"].as_bool().ok_or("LIVE_SOURCE_TAPE_STATE")?{return existing_exit_deferred(c,ex,"coverage","COMPLETE_TARGET_TAPE_REQUIRED")}
 }
 refresh_controls(c,authority,stream)?;
 let facts=snapshots_for_market(c,market,|ep|request(t,&c.root,ep))?;
 let Some(facts_until)=existing_facts_until(&facts,now())?else{return existing_exit_deferred(c,ex,"facts","ORIGINAL_PACKET_STALE")};
 let target=json!({"marketId":market,"exchangeId":ex});accept_fresh_book(bank,c,&target,&facts.book,source)?;
 if existing_target_busy(bank,log,ex)?{return existing_exit_deferred(c,ex,"liability","EXISTING_TARGET_PENDING")}
 // This positive not-sent retirement suppresses planning for this event only;
 // no tick/source event may run between it and the covered native reservation.
 retire_existing_drafts(bank,log,ex)?;
 // Fresh source admission may itself create ordinary quotes and consume the
 // existing native write gap. Wait, never bypass the financial write budget.
 let gap=bank.engine().policy()["writeGapMs"].as_u64().filter(|v|*v<=3000).ok_or("LIVE_EXISTING_NATIVE_WRITE_GAP")?;
 let activation=bank.engine().policy()["latencyMs"].as_u64().filter(|v|*v<=3000).ok_or("LIVE_EXISTING_NATIVE_ACTIVATION")?;
 let ready=now().checked_add(gap).ok_or("LIVE_EXISTING_CLOCK_RANGE")?;
 if ready.checked_add(activation).is_none_or(|end|end>=facts_until){return existing_exit_deferred(c,ex,"facts","INSUFFICIENT_ORIGINAL_PACKET_WINDOW")}
 while now()<ready{refresh_controls(c,authority,stream)?;f.authority.validate()?;std::thread::sleep(Duration::from_millis(ready.saturating_sub(now()).clamp(1,100)));}
 refresh_controls(c,authority,stream)?;f.authority.validate()?;
 if now()>=facts_until{return existing_exit_deferred(c,ex,"facts","ORIGINAL_PACKET_STALE")}
 let Some(plan)=f.cohorts.prepare_existing_exit(bank,log,t,&c.scope,ex,f.authority.trusted(),now())?else{return existing_exit_deferred(c,ex,"value","NO_CURRENT_EXECUTABLE_REVIEWED_EXIT")};
 // Preview exactly the financial transition, without creating a qid or
 // weakening a native denial. A full minute write cap is ordinary deferral.
 let Some(original)=commit_existing_reservation(bank,plan.event())?else{return existing_exit_deferred(c,ex,"native","ORIGINAL_NATIVE_BUDGET_OR_WINDOW_DENIED")};let qid=original["id"].as_u64().ok_or("LIVE_QUOTE_ID")?;
 loop{refresh_controls(c,authority,stream)?;f.authority.validate()?;
  let current=quote(bank,qid)?;match existing_reservation_stage(&current,&original,now(),facts_until){
   Ok(Some(at))=>std::thread::sleep(Duration::from_millis(at.saturating_sub(now()).clamp(1,100))),Ok(None)=>break,
   Err(e)if e.to_string()=="LIVE_EXISTING_RESERVATION_OR_FACTS_EXPIRED"=>return existing_exit_deferred(c,ex,"activation","RESERVATION_OR_ORIGINAL_PACKET_EXPIRED"),Err(e)=>return Err(e),
  }
 }
 // Same sole Transport and original global queue. The capability and physical
 // authority retain all original unknown, receipt, source and depth guards.
 if queue_has_unknown(t)?{return existing_exit_deferred(c,ex,"queue","GLOBAL_UNKNOWN_ACCEPTANCE")}
 let current=quote(bank,qid)?;if !f.eligible(bank,log,&c.scope,qid,now())?{return existing_exit_deferred(c,ex,"value","CURRENT_REVIEWED_EXIT_INELIGIBLE")}
 submit_order(t,c,bank,log,authority,&current,facts,None,Some(f))
}
fn expected_fundamental_entry_denial(reason:&str)->bool{
 matches!(reason,"FUNDAMENTAL_ENTRY_RECEIPT_SCOPE_OR_STALE"|"FUNDAMENTAL_ENTRY_RECEIPT_ORDER"|"FUNDAMENTAL_ENTRY_NOT_FLAT"|"FUNDAMENTAL_ENTRY_PHYSICAL_LIABILITY"|"FUNDAMENTAL_ENTRY_CASH_RECONCILIATION"|"FUNDAMENTAL_ENTRY_MARKET"|"FUNDAMENTAL_REVIEWED_CONTRACT_CHANGED"|"FUNDAMENTAL_ENTRY_BOOK_NOT_ADMITTED"|"FUNDAMENTAL_ENTRY_NATIVE_BOOK"|"FUNDAMENTAL_ENTRY_PLAN_CHANGED"|"REVIEWED_ENTRY_DEADLINE"|"REVIEWED_ENTRY_EXISTING_EXPOSURE"|"REVIEWED_ENTRY_REVIEW_EXPIRED"|"REVIEWED_ENTRY_CLOCK"|"REVIEWED_ENTRY_BOOK"|"REVIEWED_ENTRY_PASSIVE_PRICE"|"REVIEWED_ENTRY_FUNDING"|"REVIEWED_ENTRY_DEPTH"|"REVIEWED_ENTRY_MARKET_CAP"|"REVIEWED_ENTRY_FRESH_MARKET_CAP"|"REVIEWED_ENTRY_WRITE_BUDGET")
}
fn fundamental_entry_deferred(c:&Control,exchange:&str,stage:&str,reason:&str)->Result<()>{atomic_json(&c.root.join("fundamental-entry.json"),&json!({"at":iso(now()),"exchangeId":exchange,"stage":stage,"reason":reason,"physicalAttempted":false,"nativeRiskCapsChanged":false}))}
/// Locate original prepared bytes in the already validated Session journal.
/// Source membership, rather than a reconstructed event or elapsed timeout,
/// is the authority for the later sidecar-only lifecycle transition.
fn committed_cohort_event(bank:&Session,path:&Path,qid:u64,kind:&str)->Result<(String,u64,Value)>{
 use std::io::{Read,BufRead,BufReader};
 let metadata=fs::symlink_metadata(path)?;if !metadata.is_file()||metadata.file_type().is_symlink(){return Err("LIVE_COHORT_ORIGINAL_JOURNAL_REQUIRED".into())}
 let mut reader=BufReader::new(fs::File::open(path)?);let mut line=Vec::new();
 loop{line.clear();let n=reader.by_ref().take(8*1024*1024+1).read_until(b'\n',&mut line)?;if n==0{break}
  if line.len()>8*1024*1024||line.last()!=Some(&b'\n'){return Err("LIVE_COHORT_JOURNAL_ROW_BOUND_OR_TORN".into())}
  let row:Value=serde_json::from_slice(&line)?;let e=&row["event"];if e["type"]!=kind||e["orderId"]!=qid{continue}
  let r=&e["sourceRef"];let source=r["id"].as_str().ok_or("LIVE_COHORT_SOURCE_REF")?;let seq=r["sequence"].as_u64().filter(|n|*n>0).ok_or("LIVE_COHORT_SOURCE_REF")?;
  let mut original=e.clone();original.as_object_mut().ok_or("LIVE_COHORT_EVENT")?.remove("sourceRef");let digest=hash(&original);
  if r["hash"]!=digest||bank.committed_source_hash(source,seq)!=Some(digest.as_str()){return Err("LIVE_COHORT_COMMITTED_SOURCE_MISMATCH".into())}
  return Ok((source.into(),seq,e.clone()))
 }Err("LIVE_COHORT_ORIGINAL_EVENT_MISSING".into())
}
fn retained_cohort_receipt(root:&Path,digest:&str)->Result<Receipt>{
 use std::io::Read;
 if digest.len()!=64||!digest.bytes().all(|b|b.is_ascii_hexdigit()){return Err("LIVE_COHORT_RECEIPT_DIGEST".into())}
 let path=root.join("receipts").join(format!("{digest}.json"));let meta=fs::symlink_metadata(&path)?;
 if !meta.is_file()||meta.file_type().is_symlink()||meta.len()>8*1024*1024{return Err("LIVE_COHORT_ORIGINAL_RECEIPT_REQUIRED".into())}
 let mut bytes=Vec::new();fs::File::open(path)?.take(8*1024*1024+1).read_to_end(&mut bytes)?;
 if bytes.len()>8*1024*1024{return Err("LIVE_COHORT_RECEIPT_BOUND".into())}let r:Receipt=serde_json::from_slice(&bytes)?;
 if feedback::digest(&r)?!=digest{return Err("LIVE_COHORT_RECEIPT_CHANGED".into())}Ok(r)
}
fn expected_cohort_refresh_denial(e:&str)->bool{
 matches!(e,"FUNDAMENTAL_REARM_ACCOUNT_CLOCK"|"FUNDAMENTAL_REARM_CASH"|"FUNDAMENTAL_REARM_NOT_FLAT"|"FUNDAMENTAL_REARM_OPEN_ORDER"|"FUNDAMENTAL_ABANDON_RECEIPT_ORDER"|"FUNDAMENTAL_ABANDON_CASH"|"FUNDAMENTAL_ABANDON_NOT_FLAT"|"FUNDAMENTAL_ABANDON_OPEN_ORDER"|"FUNDAMENTAL_ENTRY_RECEIPT_SCOPE_OR_STALE")
}
/// At most one fresh account packet per pass, on the sole shared Transport.
/// Archival grants no order: a later pass must originate a NEW native qid.
fn refresh_fundamental_cohort(t:&mut Transport,c:&Control,bank:&Session,log:&DispatchLog,authority:&Arc<ProductionAuthority>,stream:&Realtime,f:&mut FundamentalRuntime)->Result<bool>{
 f.authority.validate()?;log.require_resolved_attempts()?;
 let rearm=f.cohorts.rearm_hints(bank,log,&c.scope,now())?;
 let abandon=f.cohorts.abandonment_hints(bank,log,&c.scope,now())?.into_iter().find(|h|bank.engine().order_records().iter().any(|q|q["id"]==h.qid&&q["retired"]==true));
 let Some((ex,qid,kind))=rearm.first().map(|h|(h.exchange_id.clone(),h.qid,"actual_order_terminal")).or_else(||abandon.as_ref().map(|h|(h.exchange_id.clone(),h.qid,"unsubmitted_intent_retired")))else{return Ok(false)};
 refresh_controls(c,authority,stream)?;if queue_has_unknown(t)?{return Ok(false)}
 let(source,seq,event)=committed_cohort_event(bank,&c.root.join(&c.bank_name).join("events.jsonl"),qid,kind)?;
 let retained=if kind=="actual_order_terminal"{Some((retained_cohort_receipt(&c.root,event["orderReceiptHash"].as_str().ok_or("LIVE_COHORT_ORDER_RECEIPT")?)?,retained_cohort_receipt(&c.root,event["receiptHash"].as_str().ok_or("LIVE_COHORT_FILL_RECEIPT")?)?))}else{None};
 let facts=cancel_snapshots(t,c)?;refresh_controls(c,authority,stream)?;f.authority.validate()?;
 let result=if let Some(hint)=rearm.first(){let(order,fills)=retained.as_ref().ok_or("LIVE_COHORT_TERMINAL_RECEIPTS")?;
  f.cohorts.prepare_rearm(bank,log,t,&c.scope,hint,&facts,order,fills,&source,seq,&event,now()).and_then(|plan|f.cohorts.apply_rearm(plan,bank,log,t,now()))
 }else{let hint=abandon.as_ref().ok_or("LIVE_COHORT_ABANDON_HINT")?;f.cohorts.prepare_abandon(bank,log,t,&c.scope,hint,&facts,&source,seq,&event,now()).and_then(|plan|f.cohorts.apply_abandon(plan,bank,log,t,now()))};
 match result{Ok(())=>atomic_json(&c.root.join("fundamental-cohort.json"),&json!({"at":iso(now()),"exchangeId":ex,"originalQid":qid,"stage":"archived_for_new_logical_entry","originalEvidenceType":kind,"physicalAttempted":false,"financialStateChanged":false}))?,Err(e)if expected_cohort_refresh_denial(&e.to_string())=>fundamental_entry_deferred(c,&ex,"cohort_refresh",&e.to_string())?,Err(e)=>return Err(e)}
 Ok(true)
}
/// Only evidenced exits preempt new entry. A stale-book-only holding refresh
/// competes fairly with entry reads using the SAME persistent preflight cursor.
fn fundamental_read_rotation(existing:&[fundamental::ExistingExitHint],entries:&[fundamental::EntryHint],cursor:Option<&str>)->Option<(bool,usize,String)>{
 let mut choices:Vec<_>=existing.iter().enumerate().map(|(i,h)|(true,i,format!("{}:{}:sell",h.exchange_id,h.side))).chain(entries.iter().enumerate().map(|(i,h)|(false,i,format!("{}:{}:buy",h.exchange_id,h.side)))).collect();
 choices.sort_by(|a,b|a.2.cmp(&b.2));let keys:Vec<_>=choices.iter().map(|c|c.2.clone()).collect();moloch_live_runtime::scheduling::preflight_rotation(&keys,cursor).first().map(|index|choices[*index].clone())
}
fn reviewed_fundamental_entry(t:&mut Transport,c:&Control,bank:&mut Session,log:&mut DispatchLog,authority:&Arc<ProductionAuthority>,source:&mut RestSource,stream:&Realtime,f:&mut FundamentalRuntime,hint:&fundamental::EntryHint,prior_window:u64)->Result<()>{
 let ex=hint.exchange_id.as_str();let market=hint.market_id.as_str();refresh_controls(c,authority,stream)?;log.require_resolved_attempts()?;f.authority.validate()?;
 if existing_target_busy(bank,log,ex)?{return Ok(())}
 if let Some(from)=exit_recovery_from(source,market,prior_window){let tape=request(t,&c.root,Endpoint::Trades{exchange_id:ex.into(),from:iso(from),to:iso(now())})?;
  let wrapped=json!({"source":"official_sig_api_v1","kind":"trades","tournamentId":c.scope.tournament_id,"marketId":market,"exchangeId":ex,"endpoint":tape.endpoint,"status":tape.status,"requestStartedAt":tape.request_started_at,"receivedAt":tape.received_at,"data":tape.data});accept_source_events(bank,&source.accept(&wrapped)?)?;
  let scopes=source.scopes();if !scopes.as_array().ok_or("LIVE_SOURCE_SCOPES")?.iter().find(|s|s["marketId"]==market).ok_or("LIVE_SOURCE_SCOPE")?["tapeReady"].as_bool().ok_or("LIVE_SOURCE_TAPE_STATE")?{return fundamental_entry_deferred(c,ex,"coverage","COMPLETE_TARGET_TAPE_REQUIRED")}
 }
 let facts=snapshots_for_market(c,market,|ep|request(t,&c.root,ep))?;let Some(facts_until)=existing_facts_until(&facts,now())?else{return fundamental_entry_deferred(c,ex,"facts","ORIGINAL_PACKET_STALE")};
 let target=json!({"marketId":market,"exchangeId":ex});accept_fresh_book(bank,c,&target,&facts.book,source)?;if existing_target_busy(bank,log,ex)?{return fundamental_entry_deferred(c,ex,"liability","EXISTING_TARGET_PENDING")}
 retire_existing_drafts(bank,log,ex)?;
 let gap=bank.engine().policy()["writeGapMs"].as_u64().filter(|v|*v<=3000).ok_or("LIVE_EXISTING_NATIVE_WRITE_GAP")?;let latency=bank.engine().policy()["latencyMs"].as_u64().filter(|v|*v<=3000).ok_or("LIVE_EXISTING_NATIVE_ACTIVATION")?;let ready=now().checked_add(gap).ok_or("LIVE_EXISTING_CLOCK_RANGE")?;
 if ready.checked_add(latency).is_none_or(|end|end>=facts_until){return fundamental_entry_deferred(c,ex,"facts","INSUFFICIENT_ORIGINAL_PACKET_WINDOW")}
 while now()<ready{refresh_controls(c,authority,stream)?;f.authority.validate()?;std::thread::sleep(Duration::from_millis(ready.saturating_sub(now()).clamp(1,100)));}
 refresh_controls(c,authority,stream)?;
 let mut yielded=Vec::new();let(plan,event)=loop{
  let plan=match f.cohorts.prepare_entry(bank,log,t,&c.scope,hint,&facts,&f.authority,now()){Ok(Some(v))=>v,Ok(None)=>return fundamental_entry_deferred(c,ex,"value","NO_CURRENT_REVIEWED_ENTRY"),Err(e)if expected_fundamental_entry_denial(&e.to_string())=>return fundamental_entry_deferred(c,ex,"preflight",&e.to_string()),Err(e)=>return Err(e)};
  if let Err(e)=plan.validate_current(bank,log,t,&f.authority,now()){if expected_fundamental_entry_denial(&e.to_string()){return fundamental_entry_deferred(c,ex,"current",&e.to_string())}return Err(e)}
  let mut event=plan.event().clone();event["processedAt"]=json!(iso(now()));let mut preview=bank.engine().clone();
  match preview.advance(&event){Ok(())=>break(plan,event),Err("REVIEWED_ENTRY_FRESH_MARKET_CAP")if yielded.len()<3=>{
   if let Some(other)=retire_flat_draft_competitor(bank,log,&f.cohorts.managed_exchanges())?{yielded.push(other);continue}return fundamental_entry_deferred(c,ex,"native","REVIEWED_ENTRY_FRESH_MARKET_CAP_NO_PROVED_FLAT_DRAFT")
  },Err(e)if expected_fundamental_entry_denial(e)=>return fundamental_entry_deferred(c,ex,"native",e),Err(e)=>return Err(e.into())}
 };
 let before=bank.engine().order_records().iter().filter_map(|q|q["id"].as_u64()).max().unwrap_or(0);
 // No I/O source or tick can alter the frozen plan before this commit. A
 // native financial/durability failure at actual acceptance remains fatal.
 accept(bank,&event)?;let qs:Vec<_>=bank.engine().order_records().into_iter().filter(|q|q["id"].as_u64().is_some_and(|id|id>before)&&q["retired"]==false&&q["exchangeId"]==ex&&q["action"]=="buy"&&q["reviewedFundamentalEntry"]["reviewSha256"]==event["reviewSha256"]&&q["reviewedFundamentalEntry"]["originSha256"]==event["originSha256"]).collect();
 if qs.len()!=1{return Err("LIVE_REVIEWED_ENTRY_NATIVE_RESERVATION_NOT_UNIQUE".into())}let q=&qs[0];let id=q["id"].as_u64().ok_or("LIVE_QUOTE_ID")?;
 atomic_json(&c.root.join("fundamental-entry.json"),&json!({"at":iso(now()),"exchangeId":ex,"stage":"native_reserved_not_dispatched","qid":id,"quantity":q["quantity"],"yieldedProvedFlatDraftExchanges":yielded,"nativeRiskCapsChanged":false,"physicalAttempted":false}))?;
 loop{refresh_controls(c,authority,stream)?;f.authority.validate()?;let at=now();if at>=facts_until.min(plan.valid_until_ms())||q["expiresAt"].as_u64().is_none_or(|end|end<=at){return fundamental_entry_deferred(c,ex,"activation","RESERVATION_OR_ORIGINAL_PACKET_EXPIRED")}
  if quote(bank,id)?!=*q{return Err("LIVE_REVIEWED_ENTRY_RESERVED_QUOTE_CHANGED".into())}let active=q["activationAt"].as_u64().ok_or("LIVE_QUOTE_ACTIVATION")?;if at>=active{break}std::thread::sleep(Duration::from_millis(active.saturating_sub(now()).clamp(1,100)));
 }
 if queue_has_unknown(t)?{return fundamental_entry_deferred(c,ex,"queue","GLOBAL_UNKNOWN_ACCEPTANCE")}
 if !f.eligible(bank,log,&c.scope,id,now())?{return fundamental_entry_deferred(c,ex,"value","CURRENT_REVIEWED_ENTRY_INELIGIBLE")}
 // Native clipped the actual funded quantity. submit_order binds that exact
 // genuine quote/slice, never the read hint or an invented quantity.
 submit_order(t,c,bank,log,authority,q,facts,None,Some(f))
}
fn dispatch(t:&mut Transport,c:&Control,bank:&mut Session,log:&mut DispatchLog,authority:&Arc<ProductionAuthority>,source:&mut RestSource,allow_new_submissions:bool,preflight_cursor:&mut Option<String>,stream:&Realtime,windows:&std::collections::BTreeMap<String,u64>,maker_review:Option<&moloch_live_runtime::maker_policy::Reviewed>,mut fundamental:Option<&mut FundamentalRuntime>)->Result<()>{
 let orders=bank.engine().order_records();
 let flow_started=now();let maximum_qid_before=orders.iter().filter_map(|q|q["id"].as_u64()).max().unwrap_or(0);
 let mut hints=std::collections::BTreeMap::<String,(Value,moloch_live_runtime::exit_refresh::ReadHint)>::new();
 let mut refresh_unknown=false;
 if allow_new_submissions&&orders.iter().any(|q|q["action"]=="sell"&&q["inventoryExitReason"]=="maximumAge"&&q["expiresAt"].as_u64().is_some_and(|t|t<=flow_started)&&q["id"].as_u64().is_some_and(|id|log.never_attempted(id)&&!log.intents.contains_key(&id))){
  let unknown=queue_has_unknown(t)?;refresh_unknown=unknown;
  for old in latest_refresh_hints(&orders,flow_started,|id|log.never_attempted(id)&&!log.intents.contains_key(&id))?.values(){
   if let Some(f)=fundamental.as_ref(){let ex=old["exchangeId"].as_str().ok_or("LIVE_QUOTE_SCOPE")?;f.authority.validate()?;if f.cohorts.manages(ex)&&!f.cohorts.exit_refresh_needed(bank,log,&c.scope,ex,f.authority.trusted(),flow_started)?{continue}}
   if orders.iter().any(|q|q["exchangeId"]==old["exchangeId"]&&q["side"]==old["side"]&&q["action"]=="sell"&&q["retired"]==false&&q["expiresAt"].as_u64().is_some_and(|t|t>flow_started)){continue}
   let safety=refresh_safety(c,bank,log,authority,stream,old,unknown)?;
   if let Ok(hint)=moloch_live_runtime::exit_refresh::ReadHint::from_expired(old,safety,flow_started){let key=format!("{}:{}:sell",hint.exchange(),old["side"].as_str().ok_or("LIVE_SIDE")?);hints.insert(key,(old.clone(),hint));}
  }
 }
 // Cancellations and reconciliation take priority over new purchases.
 let fundamental_cancels=if let Some(f)=fundamental.as_ref(){f.authority.validate()?;let mut ids=f.cohorts.entry_cancellations(bank,log,f.authority.trusted(),now())?;ids.extend(f.cohorts.existing_exit_cancellations(bank,log,f.authority.trusted(),now())?);ids}else{Vec::new()};
 for old in &orders {
  let qid=old["id"].as_u64().ok_or("LIVE_QUOTE_ID")?;if bank.engine().is_package_order(qid)||old["retired"]==true{continue}
  let expired=old["expiresAt"].as_u64().is_none_or(|t|t<=now());let canceled=old["cancelEffectiveAt"].as_u64().is_some_and(|t|t<=now());
  if !expired&&!canceled&&!fundamental_cancels.contains(&qid){continue}
  if log.never_attempted(qid){retire_never_sent(bank,log,old)?;continue}
  if expired||log.closed.contains(&qid)||log.cancel_acknowledged.contains(&qid){continue} // Wait for authoritative terminal GET; never repeat a confirmed cancellation.
  let actual=bank.engine().live_order_bindings().get(&qid).ok_or("LIVE_CANCEL_BINDING")?.clone();
  let p=Permit{run_id:c.activation.run_id.clone(),intent_id:format!("{qid}-cancel"),owner:"Codex".into(),expires_at_ms:now()+10000,maximum_quantity:c.activation.maximum_quantity,maximum_cost:c.activation.maximum_committed.clone()};
  let spec=RequestSpec{method:"DELETE".into(),endpoint:format!("/orders/{actual}"),body:None,body_json:None};
  let facts=cancel_snapshots(t,c)?;authority.prepare_cancel(bank,qid,spec.clone(),p.clone(),facts)?;
  if let Err(e)=authority.authorize(&c.scope,&spec,&p,false,now()){authority.clear_ready();atomic_json(&c.root.join("deferred.json"),&json!({"at":iso(now()),"qid":qid,"code":e.code}))?;return Ok(())}
  log.append(json!({"kind":"cancel_attempt","qid":qid,"spec":spec,"permit":p,"at":iso(now())}))?;
  let ack=match t.cancel(actual.clone(),p){Ok(r)=>r,Err(e)=>{
   log.append(json!({"kind":"error","operation":"cancel","qid":qid,"error":e,"at":iso(now())}))?;authority.clear_ready();
   if moloch_live_runtime::read_recovery::reconcilable_cancel_rejection(&e){
    let observed=request(t,&c.root,Endpoint::OwnedOrder{order_id:actual.clone()})?;let filled=request(t,&c.root,Endpoint::OwnedFills{order_id:actual.clone()})?;
    for e in feedback::fills(old,&actual,&c.scope.tournament_id,&filled,&iso(now()))?{accept(bank,&e)?}
    if let Some(e)=feedback::terminal(old,&actual,&c.scope.tournament_id,&observed,&filled,&iso(now()))?{accept(bank,&e)?;log.append(json!({"kind":"closed","qid":qid,"orderReceiptHash":feedback::digest(&observed)?,"fillReceiptHash":feedback::digest(&filled)?,"afterRejectedCancel":true}))?;moloch_live_runtime::execution_distribution::confirm_runtime_terminal(bank,log,qid,&observed,&filled,now());return Ok(())}
   }return Err("LIVE_CANCEL_REJECTED_RECONCILIATION_REQUIRED".into())
  }};record_receipt(&c.root,&ack)?;log.append(json!({"kind":"cancel_ack","qid":qid,"receipt":ack}))?;authority.clear_ready();return Ok(())
 }
 if !allow_new_submissions||bank.engine().package_blocks_submissions(){return Ok(())}
 if let Some(f)=fundamental.as_deref_mut(){if refresh_fundamental_cohort(t,c,bank,log,authority,stream,f)?{return Ok(())}}
 let mut preflight_budget=moloch_live_runtime::scheduling::PreflightBudget::default();
 let mut stale_existing_hints=Vec::new();
 if let Some(f)=fundamental.as_deref_mut(){
  f.authority.validate()?;let mut existing_hints=Vec::new();
  for ex in f.cohorts.managed_exchanges(){if !existing_target_busy(bank,log,&ex)?{if let Some(hint)=f.cohorts.existing_exit_hint(bank,log,&c.scope,&ex,f.authority.trusted(),now())?{if hint.reason=="reviewed_inventory_book_refresh"{stale_existing_hints.push(hint)}else{existing_hints.push(hint);}}}}
  if !existing_hints.is_empty(){let keys:Vec<_>=existing_hints.iter().map(|h|format!("{}:{}:sell",h.exchange_id,h.side)).collect();let index=moloch_live_runtime::scheduling::preflight_rotation(&keys,preflight_cursor.as_deref())[0];
   if !preflight_budget.try_start(){return Ok(())}*preflight_cursor=Some(keys[index].clone());let hint=&existing_hints[index];
   reviewed_existing_exit(t,c,bank,log,authority,source,stream,f,hint,*windows.get(&hint.exchange_id).ok_or("LIVE_RECOVERY_WINDOW_MISSING")?)?;return Ok(())
  }
 }
 if !hints.is_empty(){
  let keys:Vec<_>=hints.keys().cloned().collect();let index=moloch_live_runtime::scheduling::preflight_rotation(&keys,preflight_cursor.as_deref())[0];let key=&keys[index];
  let(old,hint)=hints.get(key).ok_or("LIVE_EXIT_REFRESH_HINT")?;
  if !preflight_budget.try_start(){return Ok(())}*preflight_cursor=Some(key.clone());
  refresh_exit(t,c,bank,log,authority,source,stream,old,hint,flow_started,maximum_qid_before,refresh_unknown,*windows.get(hint.exchange()).ok_or("LIVE_RECOVERY_WINDOW_MISSING")?,fundamental.as_deref_mut().filter(|f|f.cohorts.manages(hint.exchange())))?;
  return Ok(()) // Ineligible refresh still consumes this pass; never another packet.
 }
 if let Some(f)=fundamental.as_deref_mut(){f.authority.validate()?;let entry_hints:Vec<_>=f.cohorts.entry_hints(bank,log,&c.scope,f.authority.trusted(),now())?.into_iter().filter(|h|!bank.engine().order_records().iter().any(|q|q["exchangeId"]==h.exchange_id&&q["retired"]==false&&q.get("reviewedFundamentalEntry").is_some())).collect();
  if let Some((is_existing,index,key))=fundamental_read_rotation(&stale_existing_hints,&entry_hints,preflight_cursor.as_deref()){if !preflight_budget.try_start(){return Ok(())}*preflight_cursor=Some(key);
   if is_existing{let hint=&stale_existing_hints[index];reviewed_existing_exit(t,c,bank,log,authority,source,stream,f,hint,*windows.get(&hint.exchange_id).ok_or("LIVE_RECOVERY_WINDOW_MISSING")?)?;}
   else{let hint=&entry_hints[index];reviewed_fundamental_entry(t,c,bank,log,authority,source,stream,f,hint,*windows.get(&hint.exchange_id).ok_or("LIVE_RECOVERY_WINDOW_MISSING")?)?;}return Ok(())
  }
 }
 // At most one new physical order per pass; subsequent fills change funding.
 // Retired history and already-physical orders have separate reconciliation
 // paths above; they cannot become a new submission or consume its ranking.
 let quotes:Vec<_>=bank.engine().order_records().into_iter().filter(|q|q["retired"]==false&&q["id"].as_u64().is_some_and(|id|log.never_attempted(id)&&!bank.engine().is_package_order(id))).collect();
 let keys:Vec<String>=quotes.iter().map(|q|format!("{}:{}:{}",q["exchangeId"].as_str().unwrap_or(""),q["side"].as_str().unwrap_or(""),q["action"].as_str().unwrap_or(""))).collect();
 let selection=moloch_live_runtime::quote_priority::from_native(bank,&c.scope,&quotes,now(),preflight_cursor.as_deref())?;
 let _=atomic_json(&c.root.join("quote-priority.json"),&json!({"at":iso(now()),"selection":selection,"pricesChanged":false,"profitabilityEstablished":false}));
 let mut ranked=Vec::new();let mut maker_assessments=Vec::new();let mut maker_decisions=Vec::new();
 for index in selection.ordered_indices{
  let q=&quotes[index];
  if let Some(f)=fundamental.as_ref(){if f.cohorts.manages(q["exchangeId"].as_str().ok_or("LIVE_QUOTE_SCOPE")?){if f.eligible(bank,log,&c.scope,q["id"].as_u64().ok_or("LIVE_QUOTE_ID")?,now())?{ranked.push((if q["action"]=="sell"{0}else{1},index));}continue}}
  if q["action"]=="buy"{if let Some(review)=maker_review{
   let decision=match moloch_live_runtime::maker_policy::decide(bank,&c.scope,log,review,q["id"].as_u64().ok_or("LIVE_QUOTE_ID")?,now()){Ok(d)=>d,Err(error)if expected_maker_expiry(error.as_ref())=>{continue},Err(error)=>return Err(error)};
   maker_assessments.push(decision.proof().clone());if !decision.allowed(){continue}maker_decisions.push((index,decision));continue;
  }}ranked.push((0,index));
 }
 if !maker_decisions.is_empty(){let decisions:Vec<_>=maker_decisions.iter().map(|(_,decision)|decision).collect();for position in moloch_live_runtime::maker_policy::rank(&decisions,now())?{let(index,decision)=&maker_decisions[position];ranked.push((decision.priority(),*index));}}
 if maker_review.is_some(){let _=atomic_json(&c.root.join("maker-policy.json"),&json!({"at":iso(now()),"assessments":maker_assessments,"executionAuthority":false}));}
 ranked.sort_by_key(|(priority,_)|*priority);
 for(_,index)in ranked{let q=quotes[index].clone();
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
  let refreshed=moloch_live_runtime::quote_priority::from_native(bank,&c.scope,std::slice::from_ref(&current),now(),None)?;
  if refreshed.ordered_indices.is_empty(){authority.clear_ready();let _=atomic_json(&c.root.join("quote-priority-deferred.json"),&json!({"at":iso(now()),"qid":qid,"assessment":refreshed,"physicalAttempted":false,"priceChanged":false}));return Ok(())}
  let fundamental_managed=fundamental.as_ref().is_some_and(|f|f.cohorts.manages(current["exchangeId"].as_str().unwrap_or("")));
  if fundamental_managed&&!fundamental.as_ref().unwrap().eligible(bank,log,&c.scope,qid,now())?{authority.clear_ready();continue}
  let maker=if !fundamental_managed&&current["action"]=="buy"{if let Some(review)=maker_review{
   let decision=match moloch_live_runtime::maker_policy::decide(bank,&c.scope,log,review,qid,now()){Ok(d)=>Arc::new(d),Err(error)if expected_maker_expiry(error.as_ref())=>{authority.clear_ready();continue},Err(error)=>return Err(error)};
   if !decision.allowed(){authority.clear_ready();let _=atomic_json(&c.root.join("maker-policy-deferred.json"),&json!({"at":iso(now()),"decision":decision.proof(),"physicalAttempted":false}));continue}
   Some((decision,review))
  }else{None}}else{None};
  submit_order(t,c,bank,log,authority,&current,facts,maker,if fundamental_managed{fundamental.as_deref_mut()}else{None})?;return Ok(())
 }Ok(())
}
fn connect_stream(t:&mut Transport,c:&Control,bank:&mut Session,source:&mut RestSource)->Result<(Realtime,String)>{
 let marker=c.root.join("stream-auth-admission.json");let at=now();
 if marker.exists()&&load(&marker)?["atMs"].as_u64().is_none_or(|old|at<old+60000){return Err("LIVE_AUTH_MINT_BACKOFF".into())}
 atomic_json(&marker,&json!({"atMs":at}))?;
 let material=t.realtime_token()?;let auth=Auth::from_official_response(material)?;
 // Only the native connection boundary produces this retryable type. HTTP,
 // credentials, JOIN and durable source errors retain their original types.
 let stream=Realtime::connect(auth,&c.scope.tournament_id).map_err(moloch_live_runtime::stream_connect_retry::ConnectionFailure::from_connect_error)?;
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
fn connect_with_budget(t:&mut Transport,c:&Control,bank:&mut Session,log:&DispatchLog,authority:&Arc<ProductionAuthority>,source:&mut RestSource,budget:&mut RecoveryBudget)->Result<(Realtime,String)>{
 use moloch_live_runtime::stream_connect_retry::{self,ConnectionFailure};
 loop{
  log.require_resolved_attempts()?;recovery_queue_clear(&t.admission_state()?)?;
  if !authority.can_read(&c.scope){return Err("LIVE_STREAM_RECOVERY_CONTROL_OR_STOP".into())}
  let marker=c.root.join("stream-auth-admission.json");
  let auth=if marker.exists(){Some(load(&marker)?["atMs"].as_u64().ok_or("LIVE_AUTH_MINT_MARKER")?)}else{None};
  let at=now();let until=stream_connect_retry::next_attempt(budget,at,auth)?;
  while now()<until{budget.validate(now())?;if !authority.can_read(&c.scope){return Err("LIVE_STREAM_RECOVERY_CONTROL_OR_STOP".into())}std::thread::sleep(Duration::from_millis(until.saturating_sub(now()).min(200)));}
  if !authority.can_read(&c.scope){return Err("LIVE_STREAM_RECOVERY_CONTROL_OR_STOP".into())}
  log.require_resolved_attempts()?;recovery_queue_clear(&t.admission_state()?)?;
  let next=stream_connect_retry::prepare_attempt(budget,now(),auth)?;
  atomic_json(&c.root.join("stream-recovery-budget.json"),&serde_json::to_value(&next)?)?;*budget=next;
  match connect_stream(t,c,bank,source){
   Ok(joined)=>return Ok(joined),
   Err(error)=>{
    let Some(failure)=error.downcast_ref::<ConnectionFailure>()else{return Err(error)};
    // Failed setup never updates source coverage or binds a healthy stream.
    let auth=load(&marker)?["atMs"].as_u64().ok_or("LIVE_AUTH_MINT_MARKER")?;
    let until=stream_connect_retry::after_connect_failure(budget,now(),Some(auth),*failure)?;
    atomic_json(&c.root.join("stream-recovery.json"),&json!({"at":iso(now()),"stage":"connection_retry_wait","reason":failure.reason(),"notBefore":iso(until),"attempts":budget.attempts(),"orderRetried":false,"coverageResetToHealthy":false}))?;
   }
  }
 }
}
fn recover_stream(t:&mut Transport,c:&Control,bank:&mut Session,log:&DispatchLog,authority:&Arc<ProductionAuthority>,source:&mut RestSource,stream:&mut Option<Realtime>,budget:&mut RecoveryBudget,failure:StreamFailure,breadth:&mut Option<Recorder>,universe:&mut Option<Universe>,windows:&mut std::collections::BTreeMap<String,u64>)->Result<()>{
 if !failure.is_recoverable(){return Err(failure.into())}
 log.require_resolved_attempts()?;recovery_queue_clear(&t.admission_state()?)?;
 if !authority.can_read(&c.scope){return Err("LIVE_STREAM_RECOVERY_CONTROL_OR_STOP".into())}
 // Drop joins the sole old socket before any replacement token or connection.
 drop(stream.take());authority.clear_ready();
 let(connected,joined_at)=connect_with_budget(t,c,bank,log,authority,source,budget)?;
 if let Some(r)=breadth{r.observe("stream_reset",&json!({"localBoundary":true,"reason":"confirmed_recovery"}),&joined_at)}
 if let Some(u)=universe{u.observe("stream_reset",&json!({"localBoundary":true}),&joined_at)?;}
 let from=now().saturating_sub(60_000);for old in windows.values_mut(){*old=(*old).min(from);}
 atomic_json(&c.root.join("stream-recovery.json"),&json!({"at":iso(now()),"stage":"joined_requires_scope_recovery","reason":failure.reason(),"joinedAt":joined_at,"attempts":budget.attempts(),"orderRetried":false,"coverageResetToHealthy":false}))?;
 require_stream(&connected)?;*stream=Some(connected);let connected=stream.as_ref().unwrap();authority.bind_stream(connected.failure_signal(),connected.deadline_ms);Ok(())
}
pub fn main()->Result<()>{
 let args:Vec<_>=std::env::args().collect();if args.len()!=3||!["preflight","run"].contains(&args[1].as_str()){return Err("usage: moloch-live-runtime preflight|run /absolute/control.json".into())}
 let path=PathBuf::from(&args[2]);if !path.is_absolute(){return Err("LIVE_CONTROL_ABSOLUTE".into())}let c:Control=serde_json::from_value(load(&path)?)?;
 validate_collateral_accounting(&c)?;
 validate_maker_configuration(&c)?;
 let fundamental_loaded=c.fundamental_policy.as_ref().map(fundamental::Settings::load).transpose()?;
 if let Some(settings)=&c.package_allocation{settings.validate()?;if c.packages.is_none(){return Err("LIVE_ALLOCATION_PACKAGE_SETTINGS".into())}}
 if !c.bank_name.starts_with("bank")||c.bank_name.contains(['/', '\\'])||c.predecessor_bank.as_ref().is_some_and(|p|p==&c.bank_name){return Err("LIVE_BANK_NAME".into())}
 if !c.root.is_absolute()||!c.credentials.is_absolute()||!c.owner_lock.is_absolute()||c.activation.maximum_committed!=c.policy["maximumCommitted"].as_str().unwrap_or("")||c.activation.maximum_market_cost!=c.policy["maximumMarketCost"].as_str().unwrap_or("")||c.policy["quantity"].as_u64()!=Some(c.activation.maximum_quantity as u64)||c.policy["entryStopAt"]!=iso(c.activation.entry_until_ms){return Err("LIVE_CONTROL_POLICY_BINDING".into())}
 if let Some(settings)=&c.packages{settings.load(&c.scope)?;} if let Some(cfg)=&c.execution_model{cfg.validate()?;}
 private_dir(&c.root)?;private_dir(&c.root.join("receipts"))?;private_dir(&c.root.join("package-receipts"))?;
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
  let maker_review=c.maker_policy.as_ref().map(|settings|moloch_live_runtime::maker_policy::Reviewed::load(settings.clone(),&bank)).transpose()?;
  if let Some(review)=&maker_review{review.validate_model(c.execution_model.as_ref().ok_or("LIVE_MAKER_MODEL_REQUIRED")?)?;authority.require_maker_review(review.sha256())?;}
  let mut log=DispatchLog::open(&c.root.join("dispatch.ndjson"))?;log.require_resolved_attempts()?;
  // End the startup reader before constructing the ONE continuing worker. The
  // existing-origin baseline borrows this same transport's real quiet queue;
  // it cannot create an independent account client or infer unknown absence.
  drop(read);let mut t=transport(&c,Box::new(SharedAuthority(authority.clone())),true)?;
  let mut fundamental=if let Some(loaded)=fundamental_loaded{
   let(cohorts,review_authority,stale,existing)=open_worker_fundamental(&c,loaded,&mut bank,&mut log,lease.clone(),&t)?;
   authority.require_fundamental_exchanges(&cohorts.managed_exchanges())?;atomic_json(&c.root.join("fundamental-policy.json"),&json!({"at":iso(now()),"managedExchanges":cohorts.managed_exchanges(),"existingOriginExchanges":existing,"staleReviews":stale,"initializationDoesNotSubmit":true}))?;Some(FundamentalRuntime{cohorts,authority:review_authority})}else{
   if c.root.join("fundamental.initialized.json").exists()||c.root.join("fundamental.ndjson").exists(){return Err("LIVE_FUNDAMENTAL_SETTINGS_REQUIRED_FOR_EXISTING_COHORT".into())}None
  };
  // A durable official acknowledgment can be replayed after a crash before ledger commit.
  for (qid,v) in log.acknowledgments.clone(){let r:Receipt=serde_json::from_value(v)?;let q=quote(&bank,qid)?;let e=if let Some(recovery)=log.recovered.get(&qid){feedback::recovered_acknowledgment(&q,recovery["actualOrderId"].as_str().ok_or("LIVE_RECOVERY_ACTUAL_ID")?,&c.scope.tournament_id,&r,&serde_json::from_value(recovery["fills"].clone())?,&iso(now()))?}else{feedback::acknowledgment(&q,&r,&iso(now()))?};if let Some(f)=fundamental.as_ref(){if !closed_legacy_ack(&bank,&log,qid,&e){f.cohorts.validate_restored_acknowledgment(&bank,&log,qid,&e)?;}}authority.bind_owned(e["actualOrderId"].as_str().ok_or("LIVE_ACTUAL_ID")?.into());restore_ack(&mut bank,&e)?;}
  // Always recover original liabilities even when new package scanning is off.
  // Recovery never recreates a Staged capability or retries a physical send.
  let mut packages=moloch_live_runtime::package_dispatch::PackageDispatchLog::open(&c.root.join("package-dispatch"))?;
  recover_packages(&mut t,&c,&mut bank,&log,&authority,&mut packages)?;
  let state=t.admission_state()?;if state["state"]["unknown"].as_object().is_some_and(|m|!m.is_empty()){return Err("LIVE_QUEUE_UNKNOWN_ACCEPTANCE".into())}
  ensure_entry_window(&c,&mut bank,&authority)?;
  let fully_managed=bank.engine().report()?.get("inheritedInventory").is_some();
  let empty_positions=json!({"positions":[]});
  // Verified adoption updates the native managed-market limit while preserving
  // the original fresh-entry limit. REST coverage must use that replayed limit.
  let managed_market_limit=bank.engine().policy()["maximumMarkets"].as_u64().ok_or("LIVE_MARKET_CAP")? as usize;
  let discovery_read_limit=c.policy["maximumMarkets"].as_u64().ok_or("LIVE_MARKET_CAP")? as usize;
  let mut universe=if c.all_markets{Some(Universe::new(&c.scope.exchanges,if fully_managed{&empty_positions}else{seed.get("excludedPositionsAtTransfer").ok_or("LIVE_UNIVERSE_SEED")?},managed_market_limit)?.with_discovery_limit(discovery_read_limit)?.with_tournament(&c.scope.tournament_id)?)}else{None};
  // Previously owned holdings are filtered from the financial feed as well as discovery.
  let pairs:Value=if let Some(u)=&universe{u.pairs(&c.scope.exchanges)}else{c.scope.exchanges.iter().map(|(e,m)|json!({"exchangeId":e,"marketId":m})).collect::<Vec<_>>().into()};
  if let Err(error)=moloch_live_runtime::execution_distribution::initialize_observer(&bank,c.execution_distribution.clone()){let _=atomic_json(&c.root.join("execution-distribution.json"),&json!({"at":iso(now()),"state":"unavailable","reason":error,"executionAuthority":false}));}
  if let Err(error)=moloch_live_runtime::execution_edge::initialize_observer(&bank,c.execution_model.clone()){let _=atomic_json(&c.root.join("execution-model.json"),&json!({"at":iso(now()),"state":"unavailable","reason":error,"executionAuthority":false}));}
  let start=now();let mut source_state=RestSource::new(&pairs,&c.scope.tournament_id,&iso(start))?;source_state.seed_trade_facts(bank.source_facts())?;
  let recovery_budget_path=c.root.join("stream-recovery-budget.json");let mut recovery_budget=recovery_budget(&recovery_budget_path,now())?;
  if !recovery_budget_path.exists(){atomic_json(&recovery_budget_path,&serde_json::to_value(&recovery_budget)?)?;}
  let (connected,joined_at)=connect_with_budget(&mut t,&c,&mut bank,&log,&authority,&mut source_state,&mut recovery_budget)?;require_stream(&connected)?;let mut stream=Some(connected);
  authority.bind_stream(stream.as_ref().unwrap().failure_signal(),stream.as_ref().unwrap().deadline_ms);
  let mut relationship_census=None;let mut preflight_cursor=None;let mut package_last_scan=0;let mut package_certificate_cursor=0;
  let graph_rotation_path=c.root.join("relationship-graph-rotation.json");
  let(mut graph_rotation,mut graph_rotation_state_error)=match moloch_live_runtime::graph_discovery::Rotation::load(&graph_rotation_path,&c.scope){Ok(state)=>(state,None),Err(reason)=>(moloch_live_runtime::graph_discovery::Rotation::new(&c.scope),Some(reason))};
  let mut package_allocation_fairness=c.package_allocation.as_ref().map(|v|moloch_live_runtime::package_allocation::Fairness::new(v.maximum_wait)).transpose()?;
  let mut reconciliation_schedule=moloch_live_runtime::scheduling::ReconciliationSchedule::new(10_000)?;
  let pending_version=|log:&DispatchLog|moloch_live_runtime::scheduling::PendingVersion{acknowledged:log.acknowledgments.len() as u64,terminal:log.closed.len() as u64};
  let breadth_path=c.root.join(format!("breadth-{}-{}.jsonl",std::process::id(),now()));
  let mut breadth=Recorder::new(&breadth_path).ok();
  if let Some(r)=&mut breadth{r.observe("stream_reset",&json!({"localBoundary":true,"reason":"confirmed_subscription"}),&joined_at)}
  if let Some(u)=&mut universe{u.observe("stream_reset",&json!({"localBoundary":true}),&joined_at)?;}
  let mut windows:std::collections::BTreeMap<String,u64>=c.scope.exchanges.keys().map(|e|(e.clone(),start.saturating_sub(60000))).collect();
  let mut cycles=0u64;let mut reconciliation_cache=Reconciler::default();let mut collateral_last_check=0u64;
  loop{
   let cycle_result=(||->Result<()>{
   if stop.exists()||global_stop.exists(){return Err("LIVE_EXECUTION_STOP".into())}
   ensure_entry_window(&c,&mut bank,&authority)?;
   drain_stream(stream.as_ref().ok_or("LIVE_STREAM_REQUIRED")?,&mut bank,&mut source_state,&mut breadth,&mut universe)?;
   let mut package_blocked=recover_packages(&mut t,&c,&mut bank,&log,&authority,&mut packages)?;
   let mut reconciled=reconcile(&mut t,&c,&mut bank,&mut log,&mut reconciliation_cache)?;
   reconciliation_schedule.completed(now(),pending_version(&log),reconciled.blocks_new_submissions)?;
   let mut collateral_blocked=collateral_accounting(&mut t,&c,&mut bank,&log,&packages,lease.as_ref(),&mut collateral_last_check)?;
   accept(&mut bank,&json!({"type":"tick","receivedAt":iso(now())}))?;
   let mut required=bank.engine().required_exchanges();if let Some(f)=fundamental.as_ref(){required.extend(f.cohorts.managed_exchanges());}
   let selected=if let Some(u)=&mut universe{u.select_at(&required,now())?}else{c.scope.exchanges.keys().cloned().collect()};
   for (selected_index,ex) in selected.iter().enumerate(){require_stream(stream.as_ref().ok_or("LIVE_STREAM_REQUIRED")?)?;let market=&c.scope.exchanges[ex];let from=windows[ex];let next=source(&mut t,&c,&mut bank,&mut source_state,ex,market,from)?;windows.insert(ex.clone(),next);drain_stream(stream.as_ref().ok_or("LIVE_STREAM_REQUIRED")?,&mut bank,&mut source_state,&mut breadth,&mut universe)?;
    if reconciliation_schedule.due(now(),pending_version(&log))?.is_some(){
     reconciled=reconcile(&mut t,&c,&mut bank,&mut log,&mut reconciliation_cache)?;
     reconciliation_schedule.completed(now(),pending_version(&log),reconciled.blocks_new_submissions)?;
     collateral_blocked=collateral_accounting(&mut t,&c,&mut bank,&log,&packages,lease.as_ref(),&mut collateral_last_check)?;
    }
    ensure_entry_window(&c,&mut bank,&authority)?;
    if !package_blocked&&!collateral_blocked&&reconciliation_schedule.permits_submission(now(),pending_version(&log))?{
     package_opportunity(&mut t,&c,&mut bank,&mut log,&authority,&mut packages,&mut relationship_census,&mut source_state,stream.as_ref().ok_or("LIVE_STREAM_REQUIRED")?,&mut breadth,&mut universe,&mut windows,&mut package_last_scan,&mut package_certificate_cursor,&mut package_allocation_fairness,&fundamental.as_ref().map(|f|f.cohorts.managed_exchanges()).unwrap_or_default())?;
    }
    package_blocked=recover_packages(&mut t,&c,&mut bank,&log,&authority,&mut packages)?;
    collateral_blocked=collateral_accounting(&mut t,&c,&mut bank,&log,&packages,lease.as_ref(),&mut collateral_last_check)?;
    // Package preflight consumes real account time; never reuse an expired
    // ordinary reconciliation permission after those paced reads.
    let permit_new=!package_blocked&&!collateral_blocked&&reconciliation_schedule.permits_submission(now(),pending_version(&log))?;
    dispatch(&mut t,&c,&mut bank,&mut log,&authority,&mut source_state,permit_new,&mut preflight_cursor,stream.as_ref().ok_or("LIVE_STREAM_REQUIRED")?,&windows,maker_review.as_ref(),fundamental.as_mut())?;
    let _=atomic_json(&c.root.join("progress.json"),&json!({"at":iso(now()),"pid":std::process::id(),"phase":"trading","completedReadScopes":selected_index+1,"selectedReadScopes":selected.len(),"maximumRealtimeFramesPerDrain":MAX_REALTIME_FRAMES_PER_DRAIN,"realtimeHealthy":stream.as_ref().is_some_and(|s|s.healthy()),"actualOrders":log.acknowledgments.len(),"terminalOrders":log.closed.len(),"actualFills":bank.engine().fills().len(),"profitabilityEstablished":false}));
   }
   cycles+=1; if let Some(model)=moloch_live_runtime::execution_edge::observer_diagnostic(){let _=atomic_json(&c.root.join("execution-model.json"),&json!({"at":iso(now()),"model":model,"executionAuthority":false}));}
   atomic_json(&c.root.join("status.json"),&json!({"at":iso(now()),"pid":std::process::id(),"mode":"actual-contest-trading","runId":c.activation.run_id,"bank":c.bank_name,"cycles":cycles,"realtimeHealthy":stream.as_ref().is_some_and(|s|s.healthy()),"universe":universe.as_ref().map(Universe::status),"selectedReadScopes":selected,"ownedReconciliation":reconciled,"sourceScopes":source_state.scopes(),"breadthCapture":{"path":breadth_path,"state":breadth.as_ref().map(Recorder::status)},"report":bank.engine().report()?,"queue":t.admission_state()?,"actualOrders":log.acknowledgments.len(),"terminalOrders":log.closed.len(),"entryAnchor":iso(c.activation.entry_until_ms),"entryUntil":iso(authority.entry_until_ms()),"profitabilityEstablished":false}))?;
   // Ordinary trading precedes optional discovery. Empty certificate approval
   // must not freeze official relationship/collateral discovery until restart.
   // Four logical surfaces plus <=3 rotating graph roots share the actual
   // Transport/pagination/backoff. The existing five-minute due gate is unchanged.
   if relationship_census.as_ref().is_none_or(|v|now().saturating_sub(v.report.completed_at_ms)>=RELATIONSHIP_CENSUS_REFRESH_MS){
    let managed=fundamental.as_ref().map(|f|f.cohorts.managed_exchanges()).unwrap_or_default();
    let owned=c.scope.exchanges.keys().filter(|ex|bank.engine().wallet.quantity(ex,moloch_replay::Side::Yes)>0||bank.engine().wallet.quantity(ex,moloch_replay::Side::No)>0).cloned().collect();
    let mut roots=match graph_rotation.next(&c.scope,&managed,&owned){Ok(roots)=>roots,Err(reason)=>{graph_rotation_state_error=Some(reason);vec![]}};
    // Durable scheduling prevents repeated process restarts scanning first3.
    // A failed optional cursor write withholds graph probes, not account trading.
    if !roots.is_empty()&&serde_json::to_value(&graph_rotation).ok().and_then(|v|atomic_json(&graph_rotation_path,&v).ok()).is_none(){graph_rotation_state_error=Some("GRAPH_ROTATION_PERSISTENCE");roots.clear();}
    let census=moloch_live_runtime::relationship_census::capture(&mut t,&c.scope,&c.root.join("relationship-receipts"),&roots);
    if let Ok(mut report)=serde_json::to_value(&census.report){let observed:Vec<_>=census.report.graphs.iter().filter(|(_,surface)|surface.source.is_some()).map(|(root,_)|root.clone()).collect();report["graphDiscovery"]=json!({"requestedRoots":roots,"observedRoots":observed,"allGraphsComplete":false,"maximumRootsPerCensus":3,"rotationStateError":graph_rotation_state_error,"executionAuthority":false});let _=atomic_json(&c.root.join("relationship-census.json"),&report);}
    if census.report.transport_guard_observed{return Err("LIVE_RELATIONSHIP_CENSUS_GUARD".into())}relationship_census=Some(census);
   }
   if let Some(model)=moloch_live_runtime::execution_distribution::observer_diagnostic(){let _=atomic_json(&c.root.join("execution-distribution.json"),&json!({"at":iso(now()),"model":model,"executionAuthority":false}));}
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
#[cfg(test)]mod collateral_accounting_config_tests{
 use super::*;
 fn control()->Control{serde_json::from_value(json!({"activation":{"runId":"test","entryUntilMs":9000000,"maximumCommitted":"1000.000000","maximumMarketCost":"100.000000","maximumQuantity":2,"tournamentId":"cup","tournamentSlug":"cup","binarySha256":"a".repeat(64)},"scope":{"tournamentId":"cup","tournamentSlug":"cup","exchanges":{}},"policy":{},"root":"/fixture","credentials":"/unused","ownerLock":"/unused.lock","accountScope":"fixture"})).unwrap()}
 #[test]fn old_controls_keep_accounting_disabled(){let c=control();assert!(!c.collateral_accounting);assert!(validate_collateral_accounting(&c).is_ok());}
 #[test]fn explicit_enable_cannot_activate_unsupported_passive_flow(){let mut c=control();c.collateral_accounting=true;assert_eq!(validate_collateral_accounting(&c).unwrap_err().to_string(),"LIVE_COLLATERAL_ORDINARY_FLOW_UNSUPPORTED");}
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

#[cfg(test)]mod cancel_snapshot_tests{
 use super::*;
 #[test]fn cancellation_read_path_consumes_exactly_four_account_operations(){let mut endpoints=vec![];let f=cancel_snapshots_with(|ep|{endpoints.push(serde_json::to_value(&ep).unwrap());Ok(Receipt{endpoint:format!("read-{}",endpoints.len()),request_started_at:"2026-10-15T00:00:00Z".into(),received_at:"2026-10-15T00:00:00Z".into(),status:200,data:match ep{Endpoint::Tournament=>json!({"status":"active"}),Endpoint::Positions=>json!({"positions":[]}),Endpoint::Orders{..}=>json!({"coverage":{"complete":true},"pagination":{"hasMore":false},"data":[]}),Endpoint::Account=>json!({"balance":1000}),_=>panic!("irrelevant contract/book GET")}})}).unwrap();let expected=[Endpoint::Tournament,Endpoint::Positions,Endpoint::Orders{status:"open".into()},Endpoint::Account].iter().map(|e|serde_json::to_value(e).unwrap()).collect::<Vec<_>>();assert_eq!(endpoints,expected);assert_eq!(f.cash_balance.endpoint,"read-4");}
 #[test]fn incomplete_account_cancellation_reads_fail_before_cash_without_fabricated_receipts(){let mut count=0;let r=cancel_snapshots_with(|ep|{count+=1;Ok(Receipt{endpoint:"fixture".into(),request_started_at:"2026-10-15T00:00:00Z".into(),received_at:"2026-10-15T00:00:00Z".into(),status:200,data:match ep{Endpoint::Tournament=>json!({"status":"active"}),Endpoint::Orders{..}=>json!({"coverage":{"complete":false},"pagination":{"hasMore":false}}),_=>json!({})}})});assert!(r.is_err());assert_eq!(count,3);}
}

#[cfg(test)]mod reviewed_existing_worker_tests{
 use super::*;
 struct Temp(PathBuf);impl Temp{fn new()->Self{static NEXT:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);Self(std::env::temp_dir().join(format!("reviewed-existing-worker-{}-{}-{}",std::process::id(),now(),NEXT.fetch_add(1,std::sync::atomic::Ordering::Relaxed))))}}impl Drop for Temp{fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}
 const TID:&str="11111111-1111-1111-1111-111111111111";
 fn scope()->Scope{Scope{tournament_id:TID.into(),tournament_slug:"cup".into(),exchanges:[("101".into(),"201".into())].into()}}
 fn fixture()->(Temp,Session,DispatchLog,Value,Value,Value){
  let root=Temp::new();private_dir(&root.0).unwrap();let at=now()-7500;
  let policy=json!({"initialCash":"1000.000000","maximumCommitted":"1000.000000","maximumMarketCost":"100.000000","maximumMarkets":3,"quantity":2,"entryStopAt":iso(at+3600000),"inventoryExitMode":"fixedGain","inventoryMinimumGain":"0.005000","maximumInventoryAgeMs":60000,"inventoryAgeClock":"execution"});
  let seed=json!({"runId":"existing-worker-fixture","availableCash":"1000.000000","accountSnapshotHash":"a".repeat(64),"observedAt":iso(at-1000)});
  let mut bank=Session::open_live(&root.0.join("bank"),&policy,&seed).unwrap();
  let adoption=json!({"type":"actual_position_adoption","source":"official_sig_api_v1","authority":"human_moloch_inherited_management_20261015","complete":true,"receiptHash":"d".repeat(64),"requestStartedAt":iso(at-1),"receivedAt":iso(at),"processedAt":iso(at),"tournamentId":TID,"openingCost":"1.900000","effectiveMaximumMarkets":4,"positions":[{"exchangeId":"101","marketId":"201","quantity":-3,"lotPrincipal":"1.900000","reportedCostBasis":"1.900000","reportedMinusLotPrincipal":"0.000000","basisClassification":"exact","lots":[{"lotId":"501","quantity":1,"side":"NO","entryPrice":"0.900000","openedAt":iso(at-86400000)},{"lotId":"502","quantity":2,"side":"NO","entryPrice":"0.500000","openedAt":iso(at-43200000)}]}]});
  bank.accept("original-adoption",1,&adoption).unwrap();let at=at+1000;
  bank.accept("original-book",1,&json!({"type":"book","receivedAt":iso(at),"book":{"marketId":"201","exchangeId":"101","tournamentId":TID,"sequence":"1","sourceAt":iso(at),"requestStartedAt":iso(at),"receivedAt":iso(at),"bids":[{"price":"0.300000","quantity":100}],"asks":[{"price":"0.320000","quantity":100}]}})).unwrap();
  let log=DispatchLog::open(&root.0.join("dispatch.ndjson")).unwrap();(root,bank,log,adoption,policy,seed)
 }
 fn control(root:&Path)->Control{serde_json::from_value(json!({"activation":{"runId":"existing-worker-fixture","entryUntilMs":now()+3600000,"maximumCommitted":"1000.000000","maximumMarketCost":"100.000000","maximumQuantity":2,"tournamentId":TID,"tournamentSlug":"cup","binarySha256":"a".repeat(64)},"scope":scope(),"policy":{},"root":root,"credentials":"/unused","ownerLock":root.join("owner.lock"),"accountScope":"synthetic-existing-worker"})).unwrap()}
 fn receipt(data:Value,at:u64)->Receipt{Receipt{endpoint:"synthetic original receipt".into(),request_started_at:iso(at-1),received_at:iso(at),status:200,data}}
 #[test]fn cohort_retirement_lookup_requires_exact_original_session_membership_after_restart(){
  let(root,mut bank,mut log,_,policy,seed)=fixture();let q=bank.engine().order_records().into_iter().find(|q|q["retired"]==false).unwrap();let qid=q["id"].as_u64().unwrap();
  retire_never_sent(&mut bank,&mut log,&q).unwrap();let path=root.0.join("bank/events.jsonl");let(source,seq,event)=committed_cohort_event(&bank,&path,qid,"unsubmitted_intent_retired").unwrap();
  assert_eq!(source,"moloch-live");assert_eq!(event["confirmedNetworkAttempts"],0);assert_eq!(bank.committed_source_hash(&source,seq),event["sourceRef"]["hash"].as_str());
  drop(bank);let bank=Session::open_live(&root.0.join("bank"),&policy,&seed).unwrap();assert_eq!(committed_cohort_event(&bank,&path,qid,"unsubmitted_intent_retired").unwrap().2,event);
  let bad=root.0.join("edited-journal");let mut edited=event.clone();edited["receiptHash"]=json!("e".repeat(64));fs::write(&bad,format!("{}\n",json!({"event":edited}))).unwrap();assert!(committed_cohort_event(&bank,&bad,qid,"unsubmitted_intent_retired").is_err());
  assert!(committed_cohort_event(&bank,&path,qid,"actual_order_terminal").is_err());fs::write(&bad,b"{\"event\":").unwrap();assert!(committed_cohort_event(&bank,&bad,qid,"actual_order_terminal").is_err());
 }
 #[test]fn retained_terminal_receipts_reject_changed_bytes_wrong_hash_and_symlink(){
  let(root,_,_,_,_,_)=fixture();private_dir(&root.0.join("receipts")).unwrap();let r=receipt(json!({"open":false,"quantityFilled":0}),now());let digest=feedback::digest(&r).unwrap();record_receipt(&root.0,&r).unwrap();
  assert_eq!(feedback::digest(&retained_cohort_receipt(&root.0,&digest).unwrap()).unwrap(),digest);assert!(retained_cohort_receipt(&root.0,"../escape").is_err());let path=root.0.join("receipts").join(format!("{digest}.json"));let mut bad=r.clone();bad.data["quantityFilled"]=json!(1);atomic_json(&path,&serde_json::to_value(bad).unwrap()).unwrap();assert!(retained_cohort_receipt(&root.0,&digest).is_err());
  fs::remove_file(&path).unwrap();std::os::unix::fs::symlink(root.0.join("dispatch.ndjson"),&path).unwrap();assert!(retained_cohort_receipt(&root.0,&digest).is_err());
 }
 fn packet(c:&Control,at:u64)->SnapshotSet{snapshots_for_market(c,"201",|ep|Ok(receipt(match ep{Endpoint::Tournament=>json!({"status":"active"}),Endpoint::Orders{..}=>json!({"data":[],"coverage":{"complete":true},"pagination":{"hasMore":false}}),_=>json!({})},at))).unwrap()}
 fn reserve_event(bank:&Session,adoption:&Value)->Value{
  let at=now();let(_,hashes)=moloch_live_runtime::package_admission::native_books(bank,&scope(),at,10_000).unwrap();
  json!({"type":"reviewed_inventory_exit_reserved_v1","source":"moloch_live_dispatcher","receivedAt":iso(at),"bankScope":{"runId":"existing-worker-fixture","bankManifestHash":bank.manifest_hash(),"tournamentId":TID},"bankHead":bank.durable_head(),"expectedReportSha256":hash(&bank.engine().report().unwrap()),"exchangeId":"101","marketId":"201","side":"NO","quantity":2,"limitPrice":"0.680000","expiresAt":iso(at+60000),"originSha256":"a".repeat(64),"reviewSha256":"b".repeat(64),"adoptionRef":{"id":"original-adoption","sequence":1,"sha256":hash(adoption)},"adoptionReceiptHash":adoption["receiptHash"],"expectedPositionSha256":hash(&bank.engine().wallet.positions()["101"]),"bookSha256":hashes["101"],"reason":"refreshed_value_exit","expectedPayout":"0.600000","reviewCurrentUntilMs":at+60000,"holdingDeadlineMs":at+600000,"revoked":false})
 }
 #[test]fn original_adoption_is_recovered_from_the_committed_bank_not_a_transfer_projection(){
  let(root,mut bank,_,adoption,policy,seed)=fixture();let original=fs::read(root.0.join("bank/events.jsonl")).unwrap();let(id,n,event)=committed_adoption(&bank,&root.0.join("bank/events.jsonl")).unwrap();assert_eq!((id.as_str(),n),("original-adoption",1));let mut expected=adoption.clone();expected["sourceRef"]=json!({"id":id,"sequence":n,"hash":hash(&adoption)});assert_eq!(event,expected);
  accept(&mut bank,&json!({"type":"tick","receivedAt":iso(now())})).unwrap();drop(bank);let bank=Session::open_live(&root.0.join("bank"),&policy,&seed).unwrap();assert_eq!(committed_adoption(&bank,&root.0.join("bank/events.jsonl")).unwrap().2,event);
  let derived=serde_json::to_vec(&json!({"event":adoption})).unwrap();assert!(committed_adoption_from(&bank,std::io::Cursor::new([derived,b"\n".to_vec()].concat())).is_err());assert!(original.starts_with(b"{"));
 }
 #[test]fn copied_or_mutated_adoption_refs_never_gain_session_membership_and_torn_rows_fail(){
  let(root,bank,_,_,_,_)=fixture();let bytes=fs::read(root.0.join("bank/events.jsonl")).unwrap();let first=bytes.split_inclusive(|b|*b==b'\n').next().unwrap();let row:Value=serde_json::from_slice(first).unwrap();
  for key in ["sequence","hash","receipt","positions"]{let mut bad=row.clone();match key{"sequence"=>bad["event"]["sourceRef"]["sequence"]=json!(2),"hash"=>bad["event"]["sourceRef"]["hash"]=json!("f".repeat(64)),"receipt"=>bad["event"]["receiptHash"]=json!("f".repeat(64)),_=>bad["event"]["positions"][0]["quantity"]=json!(-2)}let mut data=serde_json::to_vec(&bad).unwrap();data.push(b'\n');assert!(committed_adoption_from(&bank,std::io::Cursor::new(data)).is_err(),"{key}");}
  assert!(committed_adoption_from(&bank,std::io::Cursor::new(&first[..first.len()-1])).is_err());assert!(committed_adoption_from(&bank,std::io::Cursor::new(Vec::<u8>::new())).is_err());
 }
 #[test]fn read_hint_gets_exactly_seven_original_facts_without_a_fabricated_native_qid(){
  let root=Temp::new();let c=control(&root.0);let mut reads=Vec::new();let facts=snapshots_for_market(&c,"201",|ep|{reads.push(serde_json::to_value(&ep).unwrap());Ok(receipt(match ep{Endpoint::Tournament=>json!({"status":"active"}),Endpoint::Orders{..}=>json!({"coverage":{"complete":true},"pagination":{"hasMore":false},"data":[]}),_=>json!({})},now()))}).unwrap();
  let expected=[Endpoint::Tournament,Endpoint::Positions,Endpoint::Orders{status:"open".into()},Endpoint::Account,Endpoint::Market{market_id:"201".into()},Endpoint::Nodes{market_id:"201".into()},Endpoint::OrderBook{market_id:"201".into(),depth:20}].iter().map(|e|serde_json::to_value(e).unwrap()).collect::<Vec<_>>();assert_eq!(reads,expected);assert_eq!(facts.book.status,200);
  let mut called=false;assert!(snapshots_for_market(&c,"foreign",|_|{called=true;unreachable!()}).is_err());assert!(!called);
 }
 #[test]fn incomplete_original_account_packet_stops_before_contract_or_book_reads(){
  let root=Temp::new();let c=control(&root.0);let mut count=0;let result=snapshots_for_market(&c,"201",|ep|{count+=1;Ok(receipt(match ep{Endpoint::Tournament=>json!({"status":"active"}),Endpoint::Orders{..}=>json!({"coverage":{"complete":false},"pagination":{"hasMore":false},"data":[]}),_=>json!({})},now()))});assert!(result.is_err());assert_eq!(count,3);
 }
 #[test]fn original_fact_window_includes_the_whole_packet_and_rejects_future_or_reversed_receipts(){
  let root=Temp::new();let c=control(&root.0);let at=now();let f=packet(&c,at);assert_eq!(existing_facts_until(&f,at).unwrap(),Some(at+9999));assert!(existing_facts_until(&f,at+9999).unwrap().is_none());
  for case in ["old_account","old_contract","future","reversed","status"]{let mut bad=f.clone();match case{"old_account"=>bad.account.request_started_at=iso(at-20_000),"old_contract"=>bad.contract.request_started_at=iso(at-10_000),"future"=>bad.book.received_at=iso(at+1),"reversed"=>bad.positions.request_started_at=iso(at+1),_=>bad.cash_balance.status=503}assert!(existing_facts_until(&bad,at).unwrap().is_none(),"{case}");}
 }
 #[test]fn proven_drafts_retire_once_then_real_reviewed_reservation_waits_activation_and_keeps_fifo(){
  let(root,mut bank,mut log,adoption,policy,seed)=fixture();let old=bank.engine().order_records();assert!(!old.is_empty());let original_cash=bank.engine().wallet.cash;let original_lots=bank.engine().wallet.positions();
  assert!(retire_existing_drafts(&mut bank,&mut log,"101").unwrap()>0);assert!(bank.engine().order_records().iter().all(|q|q["retired"]==true));assert_eq!(bank.engine().order_records().len(),old.len());assert!(!existing_target_busy(&bank,&log,"101").unwrap());
  let event=reserve_event(&bank,&adoption);let q=commit_existing_reservation(&mut bank,&event).unwrap().unwrap();let id=q["id"].as_u64().unwrap();assert!(id>old.iter().filter_map(|q|q["id"].as_u64()).max().unwrap());assert_eq!(q["side"],"NO");assert_eq!(q["price"],"0.680000");assert_eq!(q["quantity"],2);assert_eq!(bank.engine().wallet.cash,original_cash);assert_eq!(bank.engine().wallet.positions(),original_lots);assert!(bank.engine().fills().is_empty());assert!(log.intents.is_empty());assert!(existing_target_busy(&bank,&log,"101").unwrap());
  let activation=q["activationAt"].as_u64().unwrap();assert_eq!(existing_reservation_stage(&q,&q,activation-1,activation+10000).unwrap(),Some(activation));assert_eq!(existing_reservation_stage(&q,&q,activation,activation+10000).unwrap(),None);assert!(existing_reservation_stage(&q,&q,activation,activation).is_err());let mut changed=q.clone();changed["quantity"]=json!(1);assert!(existing_reservation_stage(&changed,&q,activation,activation+10000).is_err());
  let head=bank.durable_head();assert_eq!(retire_existing_drafts(&mut bank,&mut log,"101").unwrap(),0);assert_eq!(bank.durable_head(),head);let report=bank.engine().report().unwrap();drop(log);drop(bank);let bank=Session::open_live(&root.0.join("bank"),&policy,&seed).unwrap();assert_eq!(bank.engine().report().unwrap(),report);assert_eq!(quote(&bank,id).unwrap(),q);
 }
 #[test]fn unknown_original_attempt_blocks_cleanup_without_erasing_native_or_dispatch_history(){
  let(root,mut bank,mut log,_,_,_)=fixture();let q=bank.engine().order_records()[0].clone();let id=q["id"].as_u64().unwrap();log.append(json!({"kind":"intent","qid":id,"quote":q})).unwrap();log.append(json!({"kind":"attempt","qid":id})).unwrap();let head=bank.durable_head();let bytes=fs::read(root.0.join("dispatch.ndjson")).unwrap();assert!(retire_existing_drafts(&mut bank,&mut log,"101").is_err());assert!(existing_target_busy(&bank,&log,"101").is_err());assert_eq!(bank.durable_head(),head);assert_eq!(fs::read(root.0.join("dispatch.ndjson")).unwrap(),bytes);
 }
 #[test]fn known_actual_target_cannot_be_retired_as_an_unsubmitted_draft(){
  let(_root,mut bank,mut log,_,_,_)=fixture();let q=bank.engine().order_records()[0].clone();let id=q["id"].as_u64().unwrap();let ack=json!({"type":"actual_order_ack","source":"official_sig_api_v1","receiptHash":"a".repeat(64),"orderId":id,"actualOrderId":"901","exchangeId":"101","marketId":"201","side":q["side"],"action":q["action"],"quantity":q["quantity"],"price":q["price"],"receivedAt":iso(now())});
  for row in [json!({"kind":"intent","qid":id,"quote":q}),json!({"kind":"attempt","qid":id}),json!({"kind":"ack","qid":id,"receipt":{"data":{"orderId":"901"}}})]{log.append(row).unwrap();}accept(&mut bank,&ack).unwrap();assert!(existing_target_busy(&bank,&log,"101").unwrap());retire_existing_drafts(&mut bank,&mut log,"101").unwrap();assert_eq!(quote(&bank,id).unwrap()["retired"],false);assert_eq!(bank.engine().live_order_bindings().get(&id).map(String::as_str),Some("901"));assert!(!log.closed.contains(&id));
 }
 fn add_flat_competitor(bank:&mut Session){let at=now()-3500;bank.accept("other-flat-book",1,&json!({"type":"book","receivedAt":iso(at),"book":{"marketId":"202","exchangeId":"102","tournamentId":TID,"sequence":"1","sourceAt":iso(at),"requestStartedAt":iso(at),"receivedAt":iso(at),"bids":[{"price":"0.400000","quantity":100}],"asks":[{"price":"0.450000","quantity":100}]}})).unwrap();assert!(bank.engine().order_records().iter().any(|q|q["exchangeId"]=="102"&&q["retired"]==false));}
 #[test]fn fresh_cap_fallback_releases_only_a_whole_proven_flat_ordinary_draft_market(){
  let(_root,mut bank,mut log,_,_,_)=fixture();add_flat_competitor(&mut bank);let positions=bank.engine().wallet.positions();let cash=bank.engine().wallet.cash;let n=bank.engine().order_records().len();
  assert_eq!(retire_flat_draft_competitor(&mut bank,&mut log,&["101".into()].into()).unwrap(),Some("102".into()));let orders=bank.engine().order_records();assert!(orders.iter().filter(|q|q["exchangeId"]=="102").all(|q|q["retired"]==true));assert!(orders.iter().any(|q|q["exchangeId"]=="101"&&q["retired"]==false));assert_eq!(orders.len(),n);assert_eq!(bank.engine().wallet.positions(),positions);assert_eq!(bank.engine().wallet.cash,cash);assert!(log.intents.is_empty());assert!(log.attempted.is_empty());assert!(log.acknowledgments.is_empty());assert!(bank.engine().fills().is_empty());
 }
 #[test]fn competitor_with_a_review_intent_or_actual_execution_is_never_discarded_for_capacity(){
  for case in ["managed","intent","actual","unknown"]{let(root,mut bank,mut log,_,_,_)=fixture();add_flat_competitor(&mut bank);let q=bank.engine().order_records().into_iter().find(|q|q["exchangeId"]=="102"&&q["retired"]==false).unwrap();let id=q["id"].as_u64().unwrap();let mut managed=std::collections::BTreeSet::new();
   match case{"managed"=>{managed.insert("102".into());},"intent"=>log.append(json!({"kind":"intent","qid":id,"quote":q})).unwrap(),"actual"=>{
    for row in [json!({"kind":"intent","qid":id,"quote":q}),json!({"kind":"attempt","qid":id}),json!({"kind":"ack","qid":id,"receipt":{"data":{"orderId":"902"}}})]{log.append(row).unwrap();}
    accept(&mut bank,&json!({"type":"actual_order_ack","source":"official_sig_api_v1","receiptHash":"a".repeat(64),"orderId":id,"actualOrderId":"902","exchangeId":"102","marketId":"202","side":q["side"],"action":q["action"],"quantity":q["quantity"],"price":q["price"],"receivedAt":iso(now())})).unwrap();
   },_=>{log.append(json!({"kind":"intent","qid":999})).unwrap();log.append(json!({"kind":"attempt","qid":999})).unwrap();}}
   let head=bank.durable_head();let bytes=fs::read(root.0.join("dispatch.ndjson")).unwrap();let result=retire_flat_draft_competitor(&mut bank,&mut log,&managed);if case=="unknown"{assert!(result.is_err());}else{assert_eq!(result.unwrap(),None,"{case}");}assert_eq!(bank.durable_head(),head,"{case}");assert_eq!(fs::read(root.0.join("dispatch.ndjson")).unwrap(),bytes,"{case}");
  }
 }
 #[test]fn stale_holding_refresh_and_reviewed_entry_share_one_cursor_without_ttl_starvation(){
  let existing=vec![fundamental::ExistingExitHint{exchange_id:"1068".into(),market_id:"379".into(),contract_id:"380".into(),side:"YES".into(),reason:"reviewed_inventory_book_refresh".into()}];
  let entries=vec![fundamental::EntryHint{exchange_id:"1042".into(),market_id:"353".into(),contract_id:"354".into(),side:"YES".into(),review_sha256:"a".repeat(64)}];let mut cursor=None;let mut selected=Vec::new();
  // Selection is independent of a ten-second TTL: even if every pass occurs
  // sixty seconds later, a stale held book cannot retake all entry packets.
  for _ in 0..6{let(is_existing,index,key)=fundamental_read_rotation(&existing,&entries,cursor.as_deref()).unwrap();assert_eq!(index,0);selected.push(is_existing);cursor=Some(key);}
  assert_eq!(selected,vec![false,true,false,true,false,true]);assert!(fundamental_read_rotation(&[],&[],None).is_none());
 }
 #[test]fn pure_entry_denials_defer_but_durability_source_and_unknown_failures_are_fatal(){
  for code in ["FUNDAMENTAL_ENTRY_BOOK_NOT_ADMITTED","FUNDAMENTAL_ENTRY_CASH_RECONCILIATION","REVIEWED_ENTRY_FRESH_MARKET_CAP"]{assert!(expected_fundamental_entry_denial(code));}
  for code in ["PAPER_SESSION_POISONED","FUNDAMENTAL_ORIGINAL_DISPATCH_REQUIRED","FUNDAMENTAL_COMMITTED_ADOPTION_REQUIRED","DISPATCH_UNKNOWN_ACCEPTANCE_MANUAL_RECONCILIATION","QUEUE_DURABLE_FAILURE","FUNDAMENTAL_ENTRY_TRANSPORT_SCOPE"]{assert!(!expected_fundamental_entry_denial(code));}
 }
}

#[cfg(test)]mod package_hook_tests{
 use super::*;
 struct Temp(PathBuf);impl Temp{fn new()->Self{static NEXT:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);let serial=NEXT.fetch_add(1,std::sync::atomic::Ordering::Relaxed);Self(std::env::temp_dir().join(format!("moloch-package-hook-{}-{}-{serial}",std::process::id(),SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos())))}}impl Drop for Temp{fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}
 fn setup(path:&Path,n:usize)->(Session,DispatchLog,Value,Value){
  private_dir(path).unwrap();let at=now().saturating_sub(7500);let policy=json!({"initialCash":"1000.000000","maximumCommitted":"100.000000","maximumMarketCost":"50.000000","quantity":12,"maximumMarkets":3,"entryStopAt":iso(at+60_000)});let seed=json!({"runId":"package-hook-fixture","availableCash":"1000.000000","accountSnapshotHash":"a".repeat(64),"observedAt":iso(at)});
  let mut bank=Session::open_live(&path.join("bank"),&policy,&seed).unwrap();
  for i in 0..n{let ex=(101+i).to_string();let stamp=iso(at+i as u64*2500);bank.accept("official-book",i as u64+1,&json!({"type":"book","receivedAt":stamp,"book":{"tournamentId":"cup","marketId":ex,"exchangeId":ex,"sequence":"1","sourceAt":stamp,"requestStartedAt":stamp,"receivedAt":stamp,"bids":[{"price":"0.400000","quantity":100}],"asks":[{"price":"0.450000","quantity":100}]}})).unwrap();}
  (bank,DispatchLog::open(&path.join("dispatch.ndjson")).unwrap(),policy,seed)
 }
 #[test]fn execution_observer_sees_only_exact_successfully_committed_prepared_bytes(){
  use moloch_live_runtime::execution_edge::{self,Config};
  let temp=Temp::new();let(mut bank,_,_,_)=setup(&temp.0,1);let cfg=Config{fill_window_ms:1000,markout_ms:1000,post_window_ms:500,embargo_ms:500,maximum_book_age_ms:10_000,maximum_delivery_ms:1000,continuity_proof_grace_ms:0,maximum_training_age_ms:60_000,minimum_samples:2,minimum_coverage_bps:9000,probability_haircut_bps:1000,uncertainty_per_share:5000,exit_fee_per_share:0,fee_policy_source_sha256:"a".repeat(64),maximum_keys:16,maximum_pending:16,retained_samples:8};
  execution_edge::initialize_observer(&bank,Some(cfg)).unwrap();
  let at=iso(now());let e=json!({"type":"book","receivedAt":at,"book":{"tournamentId":"cup","marketId":"101","exchangeId":"101","sequence":"2","sourceAt":at,"requestStartedAt":at,"receivedAt":at,"bids":[{"price":"0.400000","quantity":100}],"asks":[{"price":"0.450000","quantity":100}]}});
  accept_source_events(&mut bank,&[e]).unwrap();let seq=bank.last_source_sequence("moloch-live");
  let before=execution_edge::observer_diagnostic().unwrap();assert_eq!(before["disabled"],Value::Null);assert!(before["lastObservedAtMs"].as_u64().unwrap()>0);
  assert!(accept(&mut bank,&json!({"type":"not_a_native_event","receivedAt":at})).is_err());assert_eq!(bank.last_source_sequence("moloch-live"),seq);
  assert_eq!(execution_edge::observer_diagnostic().unwrap(),before);execution_edge::clear_observer();
 }
 #[test]fn original_attempt_persists_one_clock_exact_native_head_and_dispatch_frontier_without_financial_write(){
  let temp=Temp::new();let(bank,mut log,_,_)=setup(&temp.0,1);let q=bank.engine().order_records()[0].clone();let id=q["id"].as_u64().unwrap();moloch_live_runtime::execution_edge::clear_observer();
  log.append(json!({"kind":"intent","qid":id,"quote":q})).unwrap();let prior=log.proof(id);let head=bank.durable_head();let financial=fs::read(temp.0.join("bank/events.jsonl")).unwrap();let capture=now();record_original_attempt(&bank,&mut log,id,None,capture).unwrap();
  let raw=fs::read_to_string(temp.0.join("dispatch.ndjson")).unwrap();let row:Value=serde_json::from_str(raw.lines().last().unwrap()).unwrap();let e=&row["event"];assert_eq!(e["captureAtMs"],capture);assert_eq!(e["at"],iso(capture));assert_eq!(e["nativeHead"],head);assert_eq!(e["dispatchBefore"],prior);assert_eq!(e["originalQuoteSha256"],hash(&q));assert_eq!(e["evidenceVersion"],1);assert_eq!(e["modelAlgorithmSha256"].as_str().unwrap().len(),64);assert_eq!(e["sourceFrontier"]["moloch-live"]["sequence"],bank.last_source_sequence("moloch-live"));assert_eq!(bank.durable_head(),head);assert_eq!(fs::read(temp.0.join("bank/events.jsonl")).unwrap(),financial);
  assert!(record_original_attempt(&bank,&mut log,id,None,capture+1).is_err());assert_eq!(fs::read_to_string(temp.0.join("dispatch.ndjson")).unwrap(),raw);
 }
 #[test]fn attempt_anchor_cannot_reinterpret_a_changed_original_intent_as_the_native_quote(){
  let temp=Temp::new();let(bank,mut log,_,_)=setup(&temp.0,1);let mut q=bank.engine().order_records()[0].clone();let id=q["id"].as_u64().unwrap();q["price"]=json!("0.005000");log.append(json!({"kind":"intent","qid":id,"quote":q})).unwrap();let original=fs::read(temp.0.join("dispatch.ndjson")).unwrap();assert!(record_original_attempt(&bank,&mut log,id,None,now()).is_err());assert!(log.never_attempted(id));assert_eq!(fs::read(temp.0.join("dispatch.ndjson")).unwrap(),original);
 }
 #[test]fn original_dispatch_proved_retirement_does_not_recreate_component_quotes_and_replays(){
  for n in [2,3]{let temp=Temp::new();let(mut bank,mut log,policy,seed)=setup(&temp.0,n);let original=bank.engine().order_records();assert_eq!(original.len(),n*2);assert!(!ordinary_liability_due(&bank,&log,now()).unwrap());
   for q in &original{retire_for_package(&mut bank,&mut log,q).unwrap();assert_eq!(bank.engine().order_records().len(),original.len());}
   assert!(bank.engine().order_records().iter().all(|q|q["retired"]==true));let report=bank.engine().report().unwrap();assert_eq!(report["reservedCash"],"0.000000");assert_eq!(report["cash"],"1000.000000");assert!(bank.engine().fills().is_empty());drop(log);drop(bank);
   let mut bank=Session::open_live(&temp.0.join("bank"),&policy,&seed).unwrap();let log=DispatchLog::open(&temp.0.join("dispatch.ndjson")).unwrap();assert_eq!(bank.engine().report().unwrap(),report);assert!(original.iter().all(|q|log.never_attempted(q["id"].as_u64().unwrap())));
   accept(&mut bank,&json!({"type":"tick","receivedAt":iso(now())})).unwrap();assert!(bank.engine().order_records().len()>original.len());
  }
 }
 #[test]fn physical_attempt_in_original_dispatch_journal_cannot_yield_to_a_package(){
  let temp=Temp::new();let(mut bank,mut log,_,_)=setup(&temp.0,2);let q=bank.engine().order_records()[0].clone();let id=q["id"].as_u64().unwrap();log.append(json!({"kind":"intent","qid":id})).unwrap();log.append(json!({"kind":"attempt","qid":id})).unwrap();let report=bank.engine().report().unwrap();let rows=fs::read(temp.0.join("dispatch.ndjson")).unwrap();assert!(retire_for_package(&mut bank,&mut log,&q).is_err());assert_eq!(bank.engine().report().unwrap(),report);assert_eq!(fs::read(temp.0.join("dispatch.ndjson")).unwrap(),rows);assert!(log.require_resolved_attempts().is_err());
 }
 fn ack(q:&Value)->Value{json!({"type":"actual_order_ack","source":"official_sig_api_v1","receiptHash":"d".repeat(64),"orderId":q["id"],"actualOrderId":format!("9{}",q["id"].as_u64().unwrap()),"exchangeId":q["exchangeId"],"marketId":q["marketId"],"side":q["side"],"action":q["action"],"quantity":q["quantity"],"price":q["price"],"receivedAt":iso(now())})}
 #[test]fn only_closed_original_untagged_native_ack_uses_legacy_forecast_restore(){
  let temp=Temp::new();let(mut bank,mut log,policy,seed)=setup(&temp.0,1);let q=bank.engine().order_records()[0].clone();let id=q["id"].as_u64().unwrap();let e=ack(&q);
  for row in [json!({"kind":"intent","qid":id,"quote":q}),json!({"kind":"attempt","qid":id}),json!({"kind":"ack","qid":id,"receipt":{"data":{"orderId":e["actualOrderId"]}}})]{log.append(row).unwrap();}
  restore_ack(&mut bank,&e).unwrap();assert!(!closed_legacy_ack(&bank,&log,id,&e));
  let mut terminal=e.clone();terminal["type"]=json!("actual_order_terminal");terminal["status"]=json!("canceled");terminal["filledQuantity"]=json!(0);terminal["fillsComplete"]=json!(true);accept(&mut bank,&terminal).unwrap();log.append(json!({"kind":"closed","qid":id})).unwrap();assert!(closed_legacy_ack(&bank,&log,id,&e));
  let mut changed=e.clone();changed["actualOrderId"]=json!("foreign");assert!(!closed_legacy_ack(&bank,&log,id,&changed));
  let old=log.intents[&id].clone();log.intents.get_mut(&id).unwrap()["fundamental"]=json!({"bindingSha256":"a".repeat(64)});assert!(!closed_legacy_ack(&bank,&log,id,&e));log.intents.insert(id,old);
  drop(log);drop(bank);let mut bank=Session::open_live(&temp.0.join("bank"),&policy,&seed).unwrap();let log=DispatchLog::open(&temp.0.join("dispatch.ndjson")).unwrap();let head=bank.durable_head();assert!(closed_legacy_ack(&bank,&log,id,&e));assert!(!restore_ack(&mut bank,&e).unwrap());assert_eq!(bank.durable_head(),head);
 }
 #[test]fn recovered_ack_is_durable_once_then_restart_revalidation_writes_no_duplicate_history(){
  let temp=Temp::new();let(mut bank,_,policy,seed)=setup(&temp.0,1);let e=ack(&bank.engine().order_records()[0]);assert!(restore_ack(&mut bank,&e).unwrap());let report=bank.engine().report().unwrap();let original=fs::read(temp.0.join("bank/events.jsonl")).unwrap();assert!(!restore_ack(&mut bank,&e).unwrap());assert_eq!(bank.engine().report().unwrap(),report);assert_eq!(fs::read(temp.0.join("bank/events.jsonl")).unwrap(),original);drop(bank);
  let mut bank=Session::open_live(&temp.0.join("bank"),&policy,&seed).unwrap();assert!(!restore_ack(&mut bank,&e).unwrap());assert_eq!(bank.engine().report().unwrap(),report);assert_eq!(fs::read(temp.0.join("bank/events.jsonl")).unwrap(),original);assert_eq!(bank.engine().live_order_bindings().len(),1);assert!(bank.engine().fills().is_empty());
 }
 #[test]fn known_binding_never_skips_native_validation_of_changed_ack_identity_and_terms(){
  let temp=Temp::new();let(mut bank,_,_,_)=setup(&temp.0,1);let e=ack(&bank.engine().order_records()[0]);restore_ack(&mut bank,&e).unwrap();let report=bank.engine().report().unwrap();let original=fs::read(temp.0.join("bank/events.jsonl")).unwrap();
  for(field,value)in [("actualOrderId",json!("other")),("quantity",json!(1)),("side",json!("other")),("price",json!("0.995000")),("receiptHash",json!("invalid")),("type",json!("actual_order_terminal"))]{let mut bad=e.clone();bad[field]=value;assert!(restore_ack(&mut bank,&bad).is_err(),"accepted {field}");assert_eq!(bank.engine().report().unwrap(),report);assert_eq!(fs::read(temp.0.join("bank/events.jsonl")).unwrap(),original);}
 }
}
