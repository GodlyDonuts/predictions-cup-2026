use baal_transport_service::{self as api,Authority,Config,Endpoint,Order,Permit,Receipt,RequestSpec,Scope,SystemClock,Transport,ReadOnlyAuthority,credentials::UnixFileCredentials,decimal_units};
use moloch_live_runtime::{authority::{Activation,ProductionAuthority,SharedAuthority,SnapshotSet,CancelSnapshotSet},dispatch::DispatchLog,feedback::{self,Result}};
use moloch_replay::{session::{Session,ExclusiveLock,atomic_json},source::RestSource,money};
use moloch_replay::realtime::{Auth,Incoming,Realtime};
use moloch_live_runtime::breadth::Recorder;
use moloch_live_runtime::universe::Universe;
use moloch_live_runtime::reconciliation::{self,Reconciler};
use moloch_live_runtime::fundamental_policy::{self as fundamental,Cohorts,ReviewAuthority};
use moloch_live_runtime::stream_recovery::{StreamFailure,RecoveryBudget,RECOVERY_WINDOW_MS,RECOVERY_MAXIMUM_ATTEMPTS};
#[path="realtime_book_catchup.rs"] mod realtime_book_catchup;
use serde::{Serialize,Deserialize};
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::{fs,path::{Path,PathBuf},sync::Arc,time::{SystemTime,UNIX_EPOCH,Duration}};
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Control{#[serde(default,skip_serializing_if="is_false")]managed_inventory_planning:bool,#[serde(default,skip_serializing_if="is_false")]fundamental_reload:bool,#[serde(default,skip_serializing_if="is_false")]account_realtime:bool,#[serde(default,skip_serializing_if="is_false")]profitable_depth_mode:bool,#[serde(default)]defensive_cancel:bool,#[serde(default)]ordinary_side_edge:moloch_live_runtime::ordinary_side_edge::Mode,#[serde(default)]owner_capital_allocation:Option<moloch_live_runtime::capital_allocation::Settings>,#[serde(default)]bulk_negative_screen:bool,#[serde(default)]paper5:Option<moloch_live_runtime::paper5_runtime::Settings>,#[serde(default)]execution_distribution:Option<moloch_live_runtime::execution_distribution::Config>,#[serde(default,skip_serializing_if="Option::is_none")]fundamental_acquisition:Option<moloch_live_runtime::execution_distribution::AcquisitionConfig>,#[serde(default)]fundamental_policy:Option<fundamental::Settings>,#[serde(default)]maker_policy:Option<moloch_live_runtime::maker_policy::Settings>,#[serde(default)]package_allocation:Option<moloch_live_runtime::package_allocation::Settings>,#[serde(default)]collateral_accounting:bool,#[serde(default)]execution_model:Option<moloch_live_runtime::execution_edge::Config>,#[serde(default)]packages:Option<moloch_live_runtime::package_settings::Settings>,#[serde(default)]daily_entry_windows:Option<moloch_live_runtime::daily_windows::DailyEntryWindows>,activation:Activation,scope:Scope,policy:Value,root:PathBuf,credentials:PathBuf,owner_lock:PathBuf,account_scope:String,#[serde(default)]all_markets:bool,#[serde(default="default_bank")]bank_name:String,#[serde(default)]predecessor_bank:Option<String>,#[serde(default)]manage_existing_positions:bool}
fn is_false(v:&bool)->bool{!*v}
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
fn observe_execution(bank:&Session,sequence:u64,event:&Value){moloch_live_runtime::execution_edge::observe_runtime_committed(bank,"moloch-live",sequence,event);moloch_live_runtime::execution_distribution::observe_runtime_committed(bank,"moloch-live",sequence,event);moloch_live_runtime::execution_distribution::observe_acquisition_committed(bank,"moloch-live",sequence,event);}
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
 if !owner_capital_ready(c,bank)?{validate_owner_daily(c,bank)?;return Ok(())}
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
// An explicit optional contribution mandate can raise the outer activation
// bound, never the immutable replay seed. Until the source-bound event commits,
// native sizing remains at its original cap and BUYs are withheld.
fn validate_owner_capital_control(c:&Control)->Result<bool>{
 let Some(settings)=&c.owner_capital_allocation else{return Ok(c.activation.maximum_committed==c.policy["maximumCommitted"].as_str().unwrap_or(""))};
 settings.validate_control(&c.activation,&c.scope,&c.account_scope,&c.policy)?;
 let cfg=c.daily_entry_windows.as_ref().ok_or("LIVE_OWNER_CAPITAL_DAILY_REQUIRED")?;
 if cfg.authorization_id!=settings.mandate.authorization_id||cfg.approved_at!=settings.mandate.approved_at{return Err("LIVE_OWNER_CAPITAL_DAILY_APPROVAL".into())}Ok(true)
}
fn owner_capital_ready(c:&Control,bank:&Session)->Result<bool>{match &c.owner_capital_allocation{None=>Ok(true),Some(settings)=>settings.ready(bank)}}
fn validate_owner_daily(c:&Control,bank:&Session)->Result<()>{
 let Some(settings)=&c.owner_capital_allocation else{return Ok(())};
 let old=bank.engine().daily_entry_mandate().ok_or("CAPITAL_PREVIOUS_DAILY_MANDATE_REQUIRED")?;
 if hash(&old)!=settings.previous_daily_mandate_sha256{return Err("CAPITAL_PREVIOUS_DAILY_MANDATE_MISMATCH".into())}
 let next=c.daily_entry_windows.as_ref().ok_or("LIVE_OWNER_CAPITAL_DAILY_REQUIRED")?.mandate(&c.activation);
 for field in ["anchorMs","campaignEndMs","intervalMs","maximumMarketCost","maximumQuantity"]{if old[field]!=next[field]{return Err("CAPITAL_MANDATE_GEOMETRY_CHANGED".into())}}Ok(())
}
fn owner_capital_buys_allowed(c:&Control,bank:&Session,action:&Value)->Result<bool>{Ok(action!="buy"||owner_capital_ready(c,bank)?)}
/// No independent account client. Original physical liabilities are reconciled
/// by the existing loop. Pending allocation pauses BUYs but keeps owned SELL,
/// cancellation and recovery lanes live; it never forces a cancel or a fill.
fn expected_owner_capital_defer(code:&str)->bool{matches!(code,"CAPITAL_COMMIT_EXPIRED"|"CAPITAL_MANDATE_EXPIRED"|"CAPITAL_ORIGINAL_CASH_MISMATCH"|"CAPITAL_POSITION_MISMATCH"|"CAPITAL_POSITION_MISSING"|"CAPITAL_ORIGINAL_SOURCE_CLOCK"|"CAPITAL_RECEIPT_CLOCK"|"CAPITAL_OPEN_ORDERS_OR_COVERAGE"|"CAPITAL_DEBT_OR_COMPONENT_SCOPE"|"CAPITAL_CASH_OR_ADVANCE")}
fn owner_capital_receipts_with(mut read:impl FnMut(Endpoint)->Result<Receipt>)->Result<[Receipt;4]>{
 Ok([read(Endpoint::Account)?,read(Endpoint::Collateral)?,read(Endpoint::Positions)?,read(Endpoint::Orders{status:"open".into()})?])
}
fn advance_owner_capital(t:&mut Transport,c:&Control,control_path:&Path,bank:&mut Session,ordinary:&DispatchLog,packages:&moloch_live_runtime::package_dispatch::PackageDispatchLog,lease:&ExclusiveLock,last_check:&mut u64)->Result<bool>{
 use moloch_live_runtime::{capital_allocation as ca,collateral_feedback::{Boundary,RetainedReceipt,ReceiptStore}};
 let Some(settings)=&c.owner_capital_allocation else{return Ok(false)};
 if settings.ready(bank)?{return Ok(false)}validate_owner_daily(c,bank)?;
 // Also pins the actual SharedAuthority Control/STOP boundary on crash resume.
 match t.package_queue_quiet(){Ok(_)=>{},Err(e)if e.code=="PACKAGE_QUEUE_NOT_QUIET"=>return Ok(true),Err(e)=>return Err(e.into())}
 let current_control=||->Result<String>{Ok(format!("{:x}",Sha256::digest(fs::read(control_path)?)))};
 let control_sha=current_control()?;
 if !settings.contributed(bank)?{
  ordinary.require_resolved_attempts()?;
  let open=ordinary.attempted.iter().any(|q|!ordinary.closed.contains(q)&&!ordinary.proved_no_network(*q))||ordinary.acknowledgments.keys().any(|q|!ordinary.closed.contains(q))||packages.unresolved()||bank.engine().package_blocks_new_submissions();
  if open{let _=atomic_json(&c.root.join("owner-capital-allocation.json"),&json!({"at":iso(now()),"state":"waiting_for_original_physical_terminal","buyEntriesPaused":true,"coveredSellsAndReconciliationEnabled":true}));return Ok(true)}
  match t.package_queue_quiet(){Ok(_)=>{},Err(e)if e.code=="PACKAGE_QUEUE_NOT_QUIET"=>return Ok(true),Err(e)=>return Err(e.into())}
  let at=now();if *last_check!=0&&at.saturating_sub(*last_check)<30_000{return Ok(true)}*last_check=at;
  // Each request persists the genuine original Receipt. No clocks or projected
  // fields are synthesized; the native event independently verifies all four.
  let [a,k,p,o]=owner_capital_receipts_with(|endpoint|request(t,&c.root,endpoint))?;
  let a=RetainedReceipt::open(&c.root,&a,ReceiptStore::Ordinary)?;let k=RetainedReceipt::open(&c.root,&k,ReceiptStore::Ordinary)?;let p=RetainedReceipt::open(&c.root,&p,ReceiptStore::Ordinary)?;let o=RetainedReceipt::open(&c.root,&o,ReceiptStore::Ordinary)?;
  if current_control()?!=control_sha{return Err("LIVE_OWNER_CAPITAL_CONTROL_CHANGED".into())}
  let boundary=Boundary{root:&c.root,owner_lock_path:&c.owner_lock,owner_lease:lease,transport:t,ordinary,packages};let approved=settings.approved()?;
  let result=ca::prepare_with_control(bank,&boundary,&approved,ca::Sources{account:&a,collateral:&k,positions:&p,orders:&o},&iso(now()),&control_sha).and_then(|prepared|prepared.commit(bank,&boundary,&iso(now())));
  match result{Ok(_)=>{},Err(e)if expected_owner_capital_defer(&e.to_string())=>{atomic_json(&c.root.join("owner-capital-allocation.json"),&json!({"at":iso(now()),"state":"source_reconciliation_deferred","reason":e.to_string(),"buyEntriesPaused":true,"coveredSellsAndReconciliationEnabled":true}))?;return Ok(true)},Err(e)=>return Err(e)}
 }
 // Crash after the contribution is safe: this branch resumes only the mandate
 // revision, and cannot contribute the same cash twice or rewrite an old event.
 if current_control()?!=control_sha{return Err("LIVE_OWNER_CAPITAL_CONTROL_CHANGED".into())}
 let stamp=iso(now());let event=settings.revision_event(bank,c.daily_entry_windows.as_ref().ok_or("LIVE_OWNER_CAPITAL_DAILY_REQUIRED")?.mandate(&c.activation),&control_sha,&stamp)?;
 let seq=bank.last_source_sequence("moloch-owner-mandate").checked_add(1).ok_or("LIVE_SOURCE_SEQUENCE_RANGE")?;
 if !bank.accept("moloch-owner-mandate",seq,&event)?{return Err("LIVE_OWNER_CAPITAL_REVISION_COMMIT".into())}
 if !settings.ready(bank)?{return Err("LIVE_OWNER_CAPITAL_REVISION_BINDING".into())}
 atomic_json(&c.root.join("owner-capital-allocation.json"),&json!({"at":iso(now()),"state":"original_cash_attributed","mandateSha256":settings.approved_mandate_sha256,"record":bank.engine().owner_capital_allocation_record(),"dailyMandateRevision":bank.engine().owner_mandate_revision_record(),"actualAccountCashChanged":false,"profitCredited":false}))?;Ok(false)
}
fn accept_source_events(bank:&mut Session,events:&[Value])->Result<()>{
 use moloch_replay::session::SESSION_BATCH_MAX_EVENTS;
 for chunk in events.chunks(SESSION_BATCH_MAX_EVENTS){
  let prepared:Vec<_>=chunk.iter().map(|event|{let mut e=event.clone();e["processedAt"]=json!(iso(now()));e}).collect();
  accept_prepared_source_events(bank,&prepared)?;
 }Ok(())
}
fn accept_prepared_source_events(bank:&mut Session,prepared:&[Value])->Result<()>{
 use moloch_replay::session::{BatchEntry,SESSION_BATCH_MAX_EVENTS};
 for chunk in prepared.chunks(SESSION_BATCH_MAX_EVENTS){
  let first=bank.last_source_sequence("moloch-live").checked_add(1).ok_or("LIVE_SOURCE_SEQUENCE_RANGE")?;
  let entries:Vec<_>=chunk.iter().enumerate().map(|(index,event)|Ok(BatchEntry{source_id:"moloch-live",sequence:first.checked_add(index as u64).ok_or("LIVE_SOURCE_SEQUENCE_RANGE")?,event})).collect::<Result<_>>()?;
  let outcome=bank.accept_batch(&entries)?;
  if outcome.appended_rows!=entries.len()||outcome.accepted.len()!=entries.len()||outcome.accepted.iter().any(|accepted|!*accepted){return Err("LIVE_SOURCE_BATCH_PREFIX".into())} for entry in &entries{observe_execution(bank,entry.sequence,entry.event);}
 }Ok(())
}
#[derive(Default)]
struct QueuedSourceBatch{prepared:Vec<Value>,bytes:usize}
impl QueuedSourceBatch{
 fn push_at(&mut self,bank:&mut Session,events:&[Value],at:u64)->Result<()>{
  for event in events{let mut e=event.clone();e["processedAt"]=json!(iso(at));let bytes=serde_json::to_vec(&e)?.len().checked_add(1024).ok_or("LIVE_SOURCE_BATCH_BYTES")?;
   let budget=moloch_replay::session::SESSION_BATCH_MAX_BYTES/2;
   if !self.prepared.is_empty()&&self.bytes.checked_add(bytes).is_none_or(|n|n>budget){self.flush(bank)?;}
   self.bytes=self.bytes.checked_add(bytes).ok_or("LIVE_SOURCE_BATCH_BYTES")?;self.prepared.push(e);
   if self.prepared.len()==moloch_replay::session::SESSION_BATCH_MAX_EVENTS||self.bytes>=budget{self.flush(bank)?;}
  }Ok(())
 }
 fn flush(&mut self,bank:&mut Session)->Result<()>{
  if !self.prepared.is_empty(){accept_prepared_source_events(bank,&self.prepared)?;self.prepared.clear();self.bytes=0;}Ok(())
 }
 fn finish(&mut self,bank:&mut Session,result:Result<()>)->Result<()>{self.flush(bank)?;result}
}
thread_local!{static PAPER5_NODE_CACHE:std::cell::RefCell<std::collections::BTreeMap<String,Receipt>>=std::cell::RefCell::new(std::collections::BTreeMap::new());}
fn cache_original_node(r:&Receipt){
 if r.status!=200||!r.endpoint.starts_with("/markets/")||!r.endpoint.contains("/nodes?tournamentId="){return}
 let Some(market)=r.endpoint.strip_prefix("/markets/").and_then(|s|s.split('/').next()).filter(|s|!s.is_empty()&&s.bytes().all(|b|b.is_ascii_digit()))else{return};
 PAPER5_NODE_CACHE.with(|cache|{let mut cache=cache.borrow_mut();if cache.len()<512||cache.contains_key(market){let replace=cache.get(market).is_none_or(|old|moloch_live_runtime::daily_windows::clock(&r.received_at).ok()>moloch_live_runtime::daily_windows::clock(&old.received_at).ok());if replace{cache.insert(market.into(),r.clone());}}});
}
fn paper5_nodes(scope:&Scope)->std::collections::BTreeMap<String,Receipt>{PAPER5_NODE_CACHE.with(|cache|{let cache=cache.borrow();scope.exchanges.iter().filter_map(|(ex,market)|cache.get(market).map(|r|(ex.clone(),r.clone()))).collect()})}
#[cfg(test)]mod paper5_cache_tests{
 use super::*;
 fn receipt(at:&str)->Receipt{Receipt{endpoint:"/markets/277/nodes?tournamentId=cup".into(),request_started_at:at.into(),received_at:at.into(),status:200,data:json!({"original":true})}}
 #[test]fn original_node_clock_is_not_refreshed_and_foreign_scope_is_not_imported(){PAPER5_NODE_CACHE.with(|c|c.borrow_mut().clear());let original=receipt("2026-10-16T05:00:00Z");cache_original_node(&original);cache_original_node(&receipt("2026-10-16T04:00:00Z"));let scope=Scope{tournament_id:"cup".into(),tournament_slug:"cup".into(),exchanges:[("966".into(),"277".into()),("foreign".into(),"999".into())].into()};let nodes=paper5_nodes(&scope);assert_eq!(nodes.len(),1);assert_eq!(nodes["966"].received_at,original.received_at);assert_eq!(nodes["966"].data,original.data);}
 #[test]fn receipt_cache_is_bounded_and_failed_or_nonnode_reads_are_ignored(){PAPER5_NODE_CACHE.with(|c|c.borrow_mut().clear());for id in 1..=600{let mut r=receipt("2026-10-16T05:00:00Z");r.endpoint=format!("/markets/{id}/nodes?tournamentId=cup");cache_original_node(&r);}assert_eq!(PAPER5_NODE_CACHE.with(|c|c.borrow().len()),512);let mut r=receipt("2026-10-16T06:00:00Z");r.status=500;cache_original_node(&r);r.status=200;r.endpoint="/markets/277/orderbook?tournamentId=cup&depth=20".into();cache_original_node(&r);assert_eq!(PAPER5_NODE_CACHE.with(|c|c.borrow()["277"].received_at.clone()),"2026-10-16T05:00:00Z");}
}
fn record_receipt(root:&Path,r:&Receipt)->Result<()> {let p=root.join("receipts").join(format!("{}.json",feedback::digest(r)?));if !p.exists(){atomic_json(&p,&serde_json::to_value(r)?)?}cache_original_node(r);cache_snapshot_receipt(root,r,&p);Ok(())}
// Four latest successful original receipts only, <=8MiB each. No reads are
// requested for research and neither original source nor receipt clocks change.
struct SnapshotReceiptCache{root:PathBuf,rows:[Option<Receipt>;4]}
thread_local!{static SNAPSHOT_RECEIPTS:std::cell::RefCell<Option<SnapshotReceiptCache>>=const{std::cell::RefCell::new(None)};}
fn snapshot_receipt_slot(endpoint:&str)->Option<usize>{
 if endpoint.starts_with("/account?tournamentId="){Some(0)}else if endpoint.starts_with("/tournaments/")&&endpoint.ends_with("/portfolio/positions"){Some(1)}else if endpoint.starts_with("/orders?status=open&limit=200&tournamentId="){Some(2)}else if endpoint.starts_with("/portfolio/collateral?tournamentId="){Some(3)}else{None}
}
fn cache_snapshot_receipt(root:&Path,r:&Receipt,path:&Path){
 let Some(slot)=snapshot_receipt_slot(&r.endpoint)else{return};if r.status!=200||!path.metadata().is_ok_and(|m|m.is_file()&&m.len()<=8*1024*1024){return}
 SNAPSHOT_RECEIPTS.with(|cache|{let mut cache=cache.borrow_mut();let c=cache.get_or_insert_with(||SnapshotReceiptCache{root:root.into(),rows:std::array::from_fn(|_|None)});if c.root!=root{*c=SnapshotReceiptCache{root:root.into(),rows:std::array::from_fn(|_|None)};}c.rows[slot]=Some(r.clone());});
}
#[derive(Deserialize)]#[serde(rename_all="camelCase",deny_unknown_fields)]
struct SnapshotRequest{version:u32,id:String,expected_control_sha256:String,expected_generation_sha256:String,research_only:bool,execution_authority:bool}
#[derive(Default)]struct SnapshotRequestState{last_version:Option<Value>,last_request_sha256:Option<String>,seen:std::collections::BTreeSet<String>,attempts:usize}
fn snapshot_atom(s:&str)->bool{!s.is_empty()&&s.len()<=64&&s.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-'||b==b'_')}
fn snapshot_hex(s:&str)->bool{s.len()==64&&s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))}
fn snapshot_file_version(m:&fs::Metadata)->Value{use std::os::unix::fs::MetadataExt;json!({"dev":m.dev(),"ino":m.ino(),"bytes":m.len(),"mtime":m.mtime(),"mtimeNs":m.mtime_nsec(),"ctime":m.ctime(),"ctimeNs":m.ctime_nsec()})}
fn snapshot_private_new(path:&Path,bytes:&[u8])->Result<()>{use std::{io::Write,os::unix::fs::OpenOptionsExt};let mut f=fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)?;f.write_all(bytes)?;f.sync_all()?;Ok(())}
fn snapshot_immutable_source(path:&Path,bytes:&[u8])->Result<()>{
 if path.exists(){let m=fs::symlink_metadata(path)?;if !m.is_file()||m.file_type().is_symlink()||m.len()!=bytes.len()as u64||fs::read(path)?!=bytes{return Err("TARTARUS_GENERATION_SOURCE_CHANGED".into())}Ok(())}else{snapshot_private_new(path,bytes)}
}
fn snapshot_request(root:&Path,state:&mut SnapshotRequestState)->Result<Option<(String,SnapshotRequest)>>{
 use std::{io::Read,os::unix::fs::MetadataExt};
 if state.attempts>=4{return Ok(None)}let path=root.join("tartarus-snapshot-request.json");let m=match fs::symlink_metadata(&path){Ok(m)=>m,Err(e)if e.kind()==std::io::ErrorKind::NotFound=>return Ok(None),Err(e)=>return Err(e.into())};
 let version=snapshot_file_version(&m);if state.last_version.as_ref()==Some(&version){return Ok(None)}state.last_version=Some(version.clone());
 if !m.is_file()||m.file_type().is_symlink()||m.nlink()!=1||m.uid()!=fs::metadata(root)?.uid()||m.mode()&0o077!=0||m.len()>4096{state.attempts+=1;return Err("TARTARUS_PRIVATE_REQUEST_BOUND".into())}
 let mut raw=Vec::new();fs::File::open(&path)?.take(4097).read_to_end(&mut raw)?;
 if raw.len()>4096||snapshot_file_version(&fs::symlink_metadata(&path)?)!=version{return Err("TARTARUS_REQUEST_CHANGED".into())}
 let sha=format!("{:x}",Sha256::digest(&raw));state.last_request_sha256=Some(sha.clone());if !state.seen.insert(sha.clone()){return Ok(None)}
 let attempts=root.join("tartarus-snapshot-attempts");private_dir(&attempts)?;
 let marker=attempts.join(format!("{sha}.json"));if marker.exists(){return Ok(None)}
 if fs::read_dir(&attempts)?.take(4).count()>=4{return Err("TARTARUS_TOTAL_REQUEST_LIMIT".into())}
 state.attempts+=1;let mut proof=serde_json::to_vec(&json!({"at":iso(now()),"requestSha256":sha,"researchOnly":true,"executionAuthority":false,"attemptedOnce":true}))?;proof.push(b'\n');snapshot_private_new(&marker,&proof)?;
 let request:SnapshotRequest=serde_json::from_slice(&raw)?;
 if request.version!=1||!snapshot_atom(&request.id)||!snapshot_hex(&request.expected_control_sha256)||!snapshot_hex(&request.expected_generation_sha256)||!request.research_only||request.execution_authority{return Err("TARTARUS_RESEARCH_REQUEST".into())}
 Ok(Some((sha,request)))
}
// Explicit end-of-cycle service only: no live physical permit or operation is
// held. Errors are research diagnostics, never new trading/STOP authority.
#[allow(clippy::too_many_arguments)]
fn service_tartarus_snapshot(c:&Control,control_path:&Path,original_control:&[u8],bank:&Session,t:&Transport,ordinary:&DispatchLog,packages:&moloch_live_runtime::package_dispatch::PackageDispatchLog,lease:&ExclusiveLock,authority:&ProductionAuthority,f:Option<&FundamentalRuntime>,state:&mut SnapshotRequestState){
 let result=(||->Result<Option<Value>>{
  let Some((request_sha,request))=snapshot_request(&c.root,state)?else{return Ok(None)};
  if !authority.can_read(&c.scope)||fs::read(control_path)?!=original_control||format!("{:x}",Sha256::digest(original_control))!=request.expected_control_sha256{return Err("TARTARUS_CURRENT_CONTROL_REQUIRED".into())}
  let generation=f.map(|f|f.cohorts.generation_sha()).unwrap_or(moloch_live_runtime::fundamental_reload::ZERO);
  if generation!=request.expected_generation_sha256{return Err("TARTARUS_CURRENT_GENERATION_REQUIRED".into())}if let Some(f)=f{f.authority.validate()?;}
  let rows=SNAPSHOT_RECEIPTS.with(|cache|cache.borrow().as_ref().filter(|v|v.root==c.root).map(|v|v.rows.clone())).ok_or("TARTARUS_ORIGINAL_RECEIPTS_UNAVAILABLE")?;
  use moloch_live_runtime::{tartarus_snapshot as ts,collateral_feedback::{Boundary,RetainedReceipt,ReceiptStore}};
  let account=RetainedReceipt::open(&c.root,rows[0].as_ref().ok_or("TARTARUS_ACCOUNT_UNAVAILABLE")?,ReceiptStore::Ordinary)?;
  let positions=RetainedReceipt::open(&c.root,rows[1].as_ref().ok_or("TARTARUS_POSITIONS_UNAVAILABLE")?,ReceiptStore::Ordinary)?;
  let open=RetainedReceipt::open(&c.root,rows[2].as_ref().ok_or("TARTARUS_OPEN_ORDERS_UNAVAILABLE")?,ReceiptStore::Ordinary)?;
  let collateral=rows[3].as_ref().map(|r|RetainedReceipt::open(&c.root,r,ReceiptStore::Ordinary)).transpose()?;
  let mut sources=vec![ts::SourceFile{label:"control",root:control_path.parent().ok_or("TARTARUS_CONTROL_PATH")?,relative:Path::new(control_path.file_name().ok_or("TARTARUS_CONTROL_PATH")?),sha256:&request.expected_control_sha256},ts::SourceFile{label:"snapshot-request",root:&c.root,relative:Path::new("tartarus-snapshot-request.json"),sha256:&request_sha}];
  let mut generation_file=None;let mut generation_control=None;let mut generation_control_sha=None;
  if generation!=moloch_live_runtime::fundamental_reload::ZERO{
   let meta=f.and_then(|f|f.cohorts.active_generation()).ok_or("TARTARUS_GENERATION_ORIGINAL_UNAVAILABLE")?;
   let raw=meta["requestBodyText"].as_str().filter(|s|s.len()<=16384).ok_or("TARTARUS_GENERATION_ORIGINAL_UNAVAILABLE")?;
   if format!("{:x}",Sha256::digest(raw.as_bytes()))!=generation||serde_json::from_str::<Value>(raw)?!=meta["request"]{return Err("TARTARUS_GENERATION_ORIGINAL_CHANGED".into())}
   let dir=c.root.join("tartarus-snapshot-sources");private_dir(&dir)?;let path=dir.join(format!("{generation}.json"));snapshot_immutable_source(&path,raw.as_bytes())?;generation_file=Some(path);
   generation_control=Some(PathBuf::from(meta["request"]["controlPath"].as_str().ok_or("TARTARUS_GENERATION_CONTROL")?));generation_control_sha=Some(meta["request"]["controlSha256"].as_str().ok_or("TARTARUS_GENERATION_CONTROL")?.to_owned());
  }
  if let Some(path)=&generation_file{sources.push(ts::SourceFile{label:"generation-request",root:path.parent().unwrap(),relative:Path::new(path.file_name().unwrap()),sha256:generation});}
  if let(Some(path),Some(sha))=(&generation_control,&generation_control_sha){sources.push(ts::SourceFile{label:"generation-control",root:path.parent().ok_or("TARTARUS_GENERATION_CONTROL")?,relative:Path::new(path.file_name().ok_or("TARTARUS_GENERATION_CONTROL")?),sha256:sha});}
  let boundary=Boundary{root:&c.root,owner_lock_path:&c.owner_lock,owner_lease:lease,transport:t,ordinary,packages};
  let packet=ts::capture(bank,&boundary,ts::Receipts{account:&account,positions:&positions,open_orders:&open,collateral:collateral.as_ref()},ts::Pins{files:&sources,generation_sha256:generation},i64::try_from(now())?)?;
  let name=format!("{}-{}",request.id,&request_sha[..16]);let path=packet.write_new(bank,&boundary,&name)?;
  let metadata_raw=fs::read(path.join("metadata.json"))?;
  Ok(Some(json!({"at":iso(now()),"state":"captured_original_evidence","requestSha256":request_sha,"packetPath":path,"metadataSha256":format!("{:x}",Sha256::digest(&metadata_raw)),"expectedControlSha256":request.expected_control_sha256,"expectedGenerationSha256":generation,"sourceObservationMs":packet.metadata()["sourceObservationMs"],"originalReceipts":packet.metadata()["originalReceipts"],"economicFreshnessQualified":false,"researchOnly":true,"executionAuthority":false,"networkRequests":0})))
 })();
 match result{Ok(Some(value))=>{let _=atomic_json(&c.root.join("tartarus-snapshot-status.json"),&value);},Ok(None)=>{},Err(error)=>{let _=atomic_json(&c.root.join("tartarus-snapshot-status.json"),&json!({"at":iso(now()),"state":"unavailable","requestSha256":state.last_request_sha256,"reason":error.to_string(),"researchOnly":true,"executionAuthority":false,"networkRequests":0,"retryAutomatic":false}));}}
}
#[cfg(test)]mod tartarus_snapshot_worker_tests{
 use super::*;
 struct Temp(PathBuf);impl Temp{fn new()->Self{let p=std::env::temp_dir().join(format!("tartarus-worker-{}-{}",std::process::id(),SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));private_dir(&p).unwrap();Self(p)}}impl Drop for Temp{fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}
 fn request_value(id:&str)->Value{json!({"version":1,"id":id,"expectedControlSha256":"a".repeat(64),"expectedGenerationSha256":moloch_live_runtime::fundamental_reload::ZERO,"researchOnly":true,"executionAuthority":false})}
 fn put(root:&Path,v:&Value){snapshot_private_new(&root.join("tartarus-snapshot-request.json"),&serde_json::to_vec(v).unwrap()).unwrap();}
 #[test]fn absent_request_has_no_output_or_attempt_work(){let t=Temp::new();let mut state=SnapshotRequestState::default();assert!(snapshot_request(&t.0,&mut state).unwrap().is_none());assert_eq!(state.attempts,0);assert_eq!(fs::read_dir(&t.0).unwrap().count(),0);}
 #[test]fn exact_request_is_once_only_including_restart_and_original_raw_sha(){
  let t=Temp::new();let v=request_value("audit-one");put(&t.0,&v);let raw=fs::read(t.0.join("tartarus-snapshot-request.json")).unwrap();let mut state=SnapshotRequestState::default();let(sha,r)=snapshot_request(&t.0,&mut state).unwrap().unwrap();assert_eq!(sha,format!("{:x}",Sha256::digest(&raw)));assert_eq!(r.id,"audit-one");assert_eq!(state.attempts,1);assert!(snapshot_request(&t.0,&mut state).unwrap().is_none());assert!(snapshot_request(&t.0,&mut SnapshotRequestState::default()).unwrap().is_none());assert_eq!(fs::read(t.0.join("tartarus-snapshot-request.json")).unwrap(),raw);assert_eq!(fs::read_dir(t.0.join("tartarus-snapshot-attempts")).unwrap().count(),1);
 }
 #[test]fn permission_claim_or_path_id_never_authorizes_research_export(){for(change,value)in [("executionAuthority",json!(true)),("researchOnly",json!(false)),("id",json!("../escape"))]{let t=Temp::new();let mut v=request_value("audit");v[change]=value;put(&t.0,&v);let mut state=SnapshotRequestState::default();assert!(snapshot_request(&t.0,&mut state).is_err());assert!(snapshot_request(&t.0,&mut state).unwrap().is_none());assert!(!t.0.join("tartarus-snapshots").exists());assert!(!t.0.join("EXECUTION_STOP").exists());}}
 #[test]fn changed_requests_cannot_grow_unbounded_disk_attempts(){let t=Temp::new();let mut state=SnapshotRequestState::default();for i in 0..5{let path=t.0.join("tartarus-snapshot-request.json");if path.exists(){fs::remove_file(&path).unwrap();}put(&t.0,&request_value(&format!("audit-{i}")));assert_eq!(snapshot_request(&t.0,&mut state).unwrap().is_some(),i<4);}assert_eq!(state.attempts,4);assert_eq!(fs::read_dir(t.0.join("tartarus-snapshot-attempts")).unwrap().count(),4);}
 #[test]fn oversized_or_public_request_is_diagnostic_once_without_source_read(){use std::os::unix::fs::PermissionsExt;let t=Temp::new();let p=t.0.join("tartarus-snapshot-request.json");snapshot_private_new(&p,&vec![b'x';4097]).unwrap();let mut state=SnapshotRequestState::default();assert!(snapshot_request(&t.0,&mut state).is_err());assert!(snapshot_request(&t.0,&mut state).unwrap().is_none());assert!(state.last_request_sha256.is_none());fs::remove_file(&p).unwrap();put(&t.0,&request_value("private"));fs::set_permissions(&p,fs::Permissions::from_mode(0o644)).unwrap();assert!(snapshot_request(&t.0,&mut state).is_err());assert!(!t.0.join("tartarus-snapshot-attempts").exists());}
 #[test]fn cache_keeps_exact_original_clocks_only_for_successful_matching_surfaces(){let t=Temp::new();private_dir(&t.0.join("receipts")).unwrap();SNAPSHOT_RECEIPTS.with(|v|*v.borrow_mut()=None);let r=Receipt{endpoint:"/account?tournamentId=fixture".into(),status:200,request_started_at:iso(1000),received_at:iso(1001),data:json!({"balance":42})};record_receipt(&t.0,&r).unwrap();let mut bad=r.clone();bad.status=503;bad.received_at=iso(2000);record_receipt(&t.0,&bad).unwrap();let mut other=r.clone();other.endpoint="/markets/1/orderbook".into();record_receipt(&t.0,&other).unwrap();SNAPSHOT_RECEIPTS.with(|cache|{let c=cache.borrow();let c=c.as_ref().unwrap();assert_eq!(serde_json::to_value(c.rows[0].as_ref().unwrap()).unwrap(),serde_json::to_value(&r).unwrap());assert!(c.rows[1..].iter().all(Option::is_none));});let bytes=fs::read(t.0.join("receipts").join(format!("{}.json",feedback::digest(&r).unwrap()))).unwrap();assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(),serde_json::to_value(r).unwrap());}
 #[test]fn hash_named_generation_source_is_create_new_or_exact_not_overwritten(){let t=Temp::new();let p=t.0.join("original.json");snapshot_immutable_source(&p,b"original bytes").unwrap();snapshot_immutable_source(&p,b"original bytes").unwrap();assert!(snapshot_immutable_source(&p,b"different bytes").is_err());assert_eq!(fs::read(p).unwrap(),b"original bytes");}
}
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
fn quote(bank:&Session,id:u64)->Result<Value>{moloch_live_runtime::order_snapshot::quote(bank,id).ok_or("LIVE_QUOTE_MISSING".into())}
fn retire_never_sent(bank:&mut Session,log:&mut DispatchLog,q:&Value)->Result<()>{
 let id=q["id"].as_u64().ok_or("LIVE_QUOTE_ID")?;if !log.never_attempted(id){return Err("LIVE_CANNOT_RETIRE_ATTEMPTED_INTENT".into())}
 let proof=log.proof(id);log.append(json!({"kind":"not_sent","qid":id,"proof":proof}))?;
 accept(bank,&json!({"type":"unsubmitted_intent_retired","source":"moloch_live_dispatcher","orderId":id,"confirmedNetworkAttempts":0,"receiptHash":hash(&proof),"receivedAt":iso(now())}))
}
fn proof_config(c:&Control)->Config{Config{queue_dir:c.root.join("account-queue"),account_scope:c.account_scope.clone(),scope:c.scope.clone(),maximum_pages:20,writes_enabled:false}}
fn reverify_aborted_records(c:&Control,log:&mut DispatchLog)->Result<()>{
 let records:Vec<_>=log.aborted_records().iter().map(|(qid,row)|(*qid,row["proof"].clone())).collect();
 for(qid,evidence)in records{let(spec,permit,at,error)=log.pre_admission_originals(qid)?;
  let proof=api::verify_retained_ordinary_pre_admission(&proof_config(c),&spec,&permit,at,&error,&evidence)?;
  log.verify_aborted_before_admission(qid,&proof)?;
 }Ok(())
}
fn retire_proved_unadmitted(bank:&mut Session,log:&DispatchLog,qid:u64)->Result<()>{
 if !log.proved_no_network(qid)||log.acknowledgments.contains_key(&qid)||bank.engine().live_order_bindings().contains_key(&qid)||bank.engine().fills().iter().any(|f|f["orderId"]==qid){return Err("LIVE_UNADMITTED_RETIREMENT_PROOF_REQUIRED".into())}
 let q=quote(bank,qid)?;if q["retired"]==true{return Ok(())}
 let original=&log.intents.get(&qid).ok_or("LIVE_UNADMITTED_ORIGINAL_INTENT")?["quote"];
 for field in ["id","marketId","exchangeId","side","action","quantity","price"]{if q[field]!=original[field]{return Err("LIVE_UNADMITTED_NATIVE_QUOTE_CHANGED".into())}}
 if q["filled"]!=0{return Err("LIVE_UNADMITTED_NATIVE_FILL".into())}
 let proof=&log.aborted_records().get(&qid).ok_or("LIVE_UNADMITTED_DURABLE_PROOF")?["proof"];
 accept(bank,&json!({"type":"unsubmitted_intent_retired","source":"moloch_live_dispatcher","orderId":qid,"confirmedNetworkAttempts":0,"receiptHash":hash(proof),"deferPlanningForPackage":true,"receivedAt":iso(now())}))
}
/// Explicit local incident repair: no Transport, credentials or HTTP. The STOP
/// remains armed. Original SDK generations must verify before an abort append.
fn recover_unadmitted(c:&Control)->Result<()>{
 let stop=c.root.join("EXECUTION_STOP");if load(&stop)?["reason"]!="LIVE_WRITE_NOT_CONFIRMED_NO_RETRY"{return Err("LIVE_UNADMITTED_INCIDENT_STOP_REQUIRED".into())}
 let mut log=DispatchLog::open(&c.root.join("dispatch.ndjson"))?;reverify_aborted_records(c,&mut log)?;
 let pending:Vec<_>=log.attempted.iter().filter(|qid|!log.acknowledgments.contains_key(qid)&&!log.proved_no_network(**qid)).copied().collect();let mut recovered=Vec::new();
 for qid in pending{let(spec,permit,at,error)=log.pre_admission_originals(qid)?;
  if permit.run_id!=c.activation.run_id||spec.body.as_ref().is_none_or(|b|b["tournamentId"]!=c.scope.tournament_id){return Err("LIVE_UNADMITTED_ORIGINAL_SCOPE".into())}
  let proof=api::verify_frozen_ordinary_pre_admission(&proof_config(c),&spec,&permit,at,&error)?;
  log.append_aborted_before_admission(qid,&proof,&iso(now()))?;recovered.push(qid);
 }log.require_resolved_attempts()?;
 atomic_json(&c.root.join("pre-admission-recovery.json"),&json!({"at":iso(now()),"recoveredOriginalQids":recovered,"sdkArchiveReverified":true,"dispatchHead":log.descriptor_identity()?,"networkRequests":0,"stopPreserved":stop.exists(),"sameIntentRetryAuthorized":false,"financialStateChanged":false}))?;Ok(())
}
/// Service only already queued official source frames BEFORE a physical read.
/// Once a Receipt returns, reconciliation validates/applies its complete tape
/// without another hook. Errors cannot discard a tape already handed back.
fn reconciliation_read_with(bank:&mut Session,endpoint:Endpoint,mut service:impl FnMut(&mut Session)->Result<()>,mut read:impl FnMut(Endpoint)->Result<Receipt>)->Result<Receipt>{
 service(bank)?;read(endpoint)
}
fn reconcile(t:&mut Transport,c:&Control,bank:&mut Session,log:&mut DispatchLog,cache:&mut Reconciler,stream:&Realtime,source:&mut RestSource,breadth:&mut Option<Recorder>,universe:&mut Option<Universe>)->Result<reconciliation::Cycle>{
 log.require_resolved_attempts()?;
 let account_signal=stream.account_signal();let account_start=account_signal.as_ref().map(|s|s.generation());
 let pending:Vec<_>=log.acknowledgments.iter().filter(|(q,_)|!log.closed.contains(q)).map(|(q,_)|*q).collect();
 let mut owned=vec![];
 for qid in pending{let q=quote(bank,qid)?;let actual=bank.engine().live_order_bindings().get(&qid).ok_or("LIVE_ACK_BINDING_MISSING")?;
  owned.push(reconciliation::owned_from_quote(&q,actual,log.cancel_acknowledged.contains(&qid),false)?);
 }
 if let Some(signal)=&account_signal{moloch_live_runtime::account_realtime::prioritize(&mut owned,|q|q.exchange_id.as_str(),signal);}
 struct Live<'a>{t:&'a mut Transport,c:&'a Control,bank:&'a mut Session,log:&'a mut DispatchLog,stream:&'a Realtime,source:&'a mut RestSource,breadth:&'a mut Option<Recorder>,universe:&'a mut Option<Universe>}
 impl reconciliation::Backend for Live<'_>{
  fn now_ms(&self)->u64{now()}
  fn read(&mut self,ep:Endpoint)->Result<Receipt>{
   let Self{t,c,bank,stream,source,breadth,universe,..}=self;
   reconciliation_read_with(bank,ep,|bank|drain_stream(stream,bank,source,breadth,universe),|ep|request(t,&c.root,ep))
  }
  fn quote(&self,id:u64)->Result<Value>{quote(self.bank,id)}
  fn apply(&mut self,e:&Value)->Result<()>{accept(self.bank,e)}
  fn close(&mut self,id:u64,e:&Value,order:&Receipt,fills:&Receipt)->Result<()>{accept(self.bank,e)?;self.log.append(json!({"kind":"closed","qid":id,"orderReceiptHash":feedback::digest(order)?,"fillReceiptHash":feedback::digest(fills)?}))?;moloch_live_runtime::execution_distribution::confirm_runtime_terminal(self.bank,self.log,id,order,fills,now());moloch_live_runtime::execution_distribution::confirm_acquisition_terminal(self.bank,self.log,id,order,fills,now());Ok(())}
 }
 let result=cache.cycle(&c.scope,&owned,false,&mut Live{t,c,bank,log,stream,source,breadth,universe})?;
 if !result.blocks_new_submissions{if let(Some(signal),Some(generation))=(account_signal,account_start){signal.reconciled_through(generation);}}
 Ok(result)
}
fn source(c:&Control,bank:&mut Session,s:&mut RestSource,ex:&str,market:&str,from:u64,reads:&mut impl OrdinaryPreflightReads)->Result<u64>{
 // The public tape keeps its existing blocking GET and immediate application.
 // Only the already-allowlisted book GET uses the existing exclusive I/O task.
 reads.service_before_read(bank,s)?;
 let from=s.recovery_start_ms(market).map_or(from,|recovery|from.min(recovery));let to=reads.now_ms();let tape=reads.read(Endpoint::Trades{exchange_id:ex.into(),from:iso(from),to:iso(to)})?;
 let wrap=|r:&Receipt,kind:&str|json!({"source":"official_sig_api_v1","kind":kind,"tournamentId":c.scope.tournament_id,"marketId":market,"exchangeId":ex,"endpoint":r.endpoint,"status":r.status,"requestStartedAt":r.request_started_at,"receivedAt":r.received_at,"data":r.data});
 accept_source_events(bank,&s.accept(&wrap(&tape,"trades"))?)?;
 // Never insert a hook between a returned tape and its original commit.
 reads.service_before_read(bank,s)?;
 let book=reads.read_serviced(bank,s,Endpoint::OrderBook{market_id:market.into(),depth:20})?;
 accept_source_events(bank,&s.accept(&wrap(&book,"book"))?)?;Ok(to.saturating_sub(2000))
}
fn snapshots(t:&mut Transport,c:&Control,authority:&ProductionAuthority,bank:&mut Session,source:&mut RestSource,stream:&Realtime,breadth:&mut Option<Recorder>,universe:&mut Option<Universe>,q:&Value)->Result<SnapshotSet>{
 preflight_snapshots(c,bank,source,q["marketId"].as_str().ok_or("LIVE_MARKET_ID")?,None,&mut LivePreflightReads{t,c,authority,stream,breadth,universe})
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
 let legacy=moloch_live_runtime::package_worker::recover(bank,packages,&mut backend)?;let portfolio=moloch_live_runtime::package_portfolio_worker::recover(bank,packages,&mut backend)?;let probabilistic=moloch_live_runtime::probabilistic_portfolio_worker::recover(bank,packages,&mut backend)?;Ok(legacy.blocks_new_submissions||portfolio.blocks_new_submissions||probabilistic.blocks_new_submissions)
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
/// Configured reviewed managers own these markets. Ordinary planning must not
/// reserve their flat-entry slots, even when current forecast prices deny entry.
/// This never retires an intent/attempt/ACK, reviewed reservation or package.
fn install_managed_entry_exclusions(c:&Control,control_path:&Path,bank:&mut Session,log:&mut DispatchLog,f:Option<&FundamentalRuntime>)->Result<()>{
 let managed=if let Some(f)=f{f.authority.validate()?;f.cohorts.managed_exchanges()}else{Default::default()};
 if !c.managed_inventory_planning&&bank.engine().managed_inventory_planning_record().is_some(){return Err("LIVE_MANAGED_INVENTORY_CONTROL_MISMATCH".into())}
 if c.managed_inventory_planning&&f.is_none(){return Err("LIVE_MANAGED_INVENTORY_REVIEW_REQUIRED".into())}
 if managed.iter().any(|ex|!c.scope.exchanges.contains_key(ex)){return Err("LIVE_MANAGED_EXCLUSION_SCOPE".into())}
 if bank.engine().ordinary_entry_exclusions()!=managed{
  let reviewed=if let Some(f)=f{hash(&serde_json::to_value(f.authority.trusted())?)}else{hash(&json!([]))};
  let at=now();let event=json!({"type":"ordinary_entry_exclusions_v1","source":"moloch_live_dispatcher","receivedAt":iso(at),"processedAt":iso(at),"bankScope":{"runId":c.activation.run_id,"bankManifestHash":bank.manifest_hash(),"tournamentId":c.scope.tournament_id},"bankHead":bank.durable_head(),"expectedReportSha256":hash(&bank.engine().report()?),"managedExchangeIds":managed,"approvedReviewSetSha256":reviewed,"controlSha256":format!("{:x}",Sha256::digest(fs::read(control_path)?)),"expectedPreviousExclusionSha256":bank.engine().ordinary_entry_exclusion_record().map(|v|v["sha256"].clone()).unwrap_or(Value::Null)});
  if let Some(f)=f{f.authority.validate()?;}accept(bank,&event)?;
 }
 if c.managed_inventory_planning{
  let f=f.ok_or("LIVE_MANAGED_INVENTORY_REVIEW_REQUIRED")?;f.authority.validate()?;let reviewed=hash(&serde_json::to_value(f.authority.trusted())?);let generation=f.cohorts.generation_sha();let control_sha=format!("{:x}",Sha256::digest(fs::read(control_path)?));
  let previous=bank.engine().managed_inventory_planning_record().cloned();
  if previous.as_ref().is_none_or(|r|r["managedExchangeIds"]!=json!(managed)||r["approvedReviewSetSha256"]!=reviewed||r["generationSha256"]!=generation||r["controlSha256"]!=control_sha){
   let at=now();let e=json!({"type":"managed_inventory_planning_v1","source":"moloch_live_dispatcher","receivedAt":iso(at),"processedAt":iso(at),"bankScope":{"runId":c.activation.run_id,"bankManifestHash":bank.manifest_hash(),"tournamentId":c.scope.tournament_id},"bankHead":bank.durable_head(),"expectedReportSha256":hash(&bank.engine().report()?),"managedExchangeIds":managed,"approvedReviewSetSha256":reviewed,"generationSha256":generation,"controlSha256":control_sha,"expectedPreviousModeSha256":previous.as_ref().map_or(Value::Null,|r|r["sha256"].clone())});
   f.authority.validate()?;accept(bank,&e)?;
  }
 }
 let mut retired=0;for ex in &managed{retired+=retire_existing_drafts(bank,log,ex)?;}
 let _=atomic_json(&c.root.join("ordinary-entry-exclusions.json"),&json!({"at":iso(now()),"record":bank.engine().ordinary_entry_exclusion_record(),"retiredProvedNeverSentDrafts":retired,"doesNotSubmit":true}));Ok(())
}
fn urgent_source_exchanges(orders:&[Value],pending:&std::collections::BTreeSet<u64>)->Result<std::collections::BTreeSet<String>>{
 let mut urgent=std::collections::BTreeSet::new();
 for q in orders{if q["retired"]==true{continue}let id=q["id"].as_u64().ok_or("LIVE_SOURCE_QUOTE_ID")?;
  if pending.contains(&id)||q["inventoryExitReason"]=="maximumAge"||q["executionStyle"]=="inventoryAON"{urgent.insert(q["exchangeId"].as_str().ok_or("LIVE_SOURCE_QUOTE_SCOPE")?.to_owned());}
 }Ok(urgent)
}
/// Sole pinned-Control startup transition; no quote, intent or order is sent.
fn install_profitable_depth_mode(c:&Control,control_path:&Path,bank:&mut Session,authority:&ProductionAuthority,t:&Transport,log:&DispatchLog)->Result<()>{
 if !c.profitable_depth_mode&&bank.engine().profitable_depth_mode_record().is_none(){return Ok(())}
 if bank.engine().profitable_depth_enabled()==c.profitable_depth_mode{return Ok(())}
 if !authority.can_read(&c.scope){return Err("LIVE_PROFITABLE_DEPTH_CONTROL_OR_STOP".into())}
 log.require_resolved_attempts()?;t.package_queue_quiet()?;
 let at=now();let event=json!({"type":"ordinary_profitable_depth_mode_v1","source":"moloch_live_dispatcher","receivedAt":iso(at),"processedAt":iso(at),"bankScope":{"runId":c.activation.run_id,"bankManifestHash":bank.manifest_hash(),"tournamentId":c.scope.tournament_id},"bankHead":bank.durable_head(),"expectedReportSha256":hash(&bank.engine().report()?),"expectedPolicySha256":hash(bank.engine().policy()),"controlSha256":format!("{:x}",Sha256::digest(fs::read(control_path)?)),"enabled":c.profitable_depth_mode,"depthBps":5000,"expectedPreviousModeSha256":bank.engine().profitable_depth_mode_record().map_or(Value::Null,|v|v["sha256"].clone())});
 if !authority.can_read(&c.scope){return Err("LIVE_PROFITABLE_DEPTH_CONTROL_OR_STOP".into())}accept(bank,&event)
}
fn fundamental_reload_rejection(error:&Box<dyn std::error::Error>)->bool{
 if error.downcast_ref::<moloch_live_runtime::account_realtime::AccountGenerationChanged>().is_some(){return true}
 matches!(error.to_string().as_str(),"RELOAD_REQUEST_BINDING_OR_EXPIRED"|"RELOAD_SETTINGS_NOT_APPROVED"|"RELOAD_CONTROL_CHANGED"|"FUNDAMENTAL_PINNED_FILE_CHANGED"|"FUNDAMENTAL_GENERATION_REMOVAL_OR_COUNT"|"FUNDAMENTAL_GENERATION_RECEIPT_ORDER"|"FUNDAMENTAL_ENTRY_RECEIPT_SCOPE_OR_STALE"|"FUNDAMENTAL_GENERATION_ACCOUNT_COVERAGE"|"FUNDAMENTAL_GENERATION_CASH"|"FUNDAMENTAL_GENERATION_POSITIONS"|"FUNDAMENTAL_GENERATION_ORDERS"|"FUNDAMENTAL_GENERATION_POSITION_SCOPE"|"FUNDAMENTAL_GENERATION_POSITION_DUPLICATE_OR_QUANTITY"|"FUNDAMENTAL_GENERATION_OPEN_SCOPE"|"FUNDAMENTAL_GENERATION_SCOPE"|"FUNDAMENTAL_INSTALL_REVIEW_EXPIRED"|"FUNDAMENTAL_GENERATION_POSITION_ATTRIBUTION"|"FUNDAMENTAL_GENERATION_TARGET_OPEN"|"FUNDAMENTAL_GENERATION_NEW_SCOPE_NOT_FLAT"|"FUNDAMENTAL_GENERATION_OWNED_CAP"|"FUNDAMENTAL_GENERATION_ORIGIN_CHANGED"|"FUNDAMENTAL_REVIEW_REVISION"|"FUNDAMENTAL_CANNOT_EXTEND_COHORT"|"FUNDAMENTAL_CANNOT_REINTERPRET_COHORT_OBJECTIVE")
}
// Only called between completed serialized actions, never with a live permit.
// The original immutable Control and continuing Transport/Session are preserved.
#[allow(clippy::too_many_arguments)]
fn service_fundamental_reload(channel:Option<&moloch_live_runtime::fundamental_reload::Channel>,c:&Control,base_path:&Path,base:&[u8],t:&mut Transport,bank:&mut Session,log:&mut DispatchLog,packages:&moloch_live_runtime::package_dispatch::PackageDispatchLog,authority:&ProductionAuthority,lease:&ExclusiveLock,f:Option<&mut FundamentalRuntime>,stream:&Realtime,source_state:&mut RestSource,breadth:&mut Option<Recorder>,universe:&mut Option<Universe>)->Result<()>{
 let Some(channel)=channel else{return Ok(())};let f=f.ok_or("LIVE_FUNDAMENTAL_RELOAD_RUNTIME_REQUIRED")?;
 atomic_json(&c.root.join("fundamental-reload/status.json"),&channel.status(base,bank,f.cohorts.generation_sha())?)?;
 let mut pending=match channel.poll(){Ok(Some(v))=>v,Ok(None)=>return Ok(()),Err(error)=>{
  // No generation/financial change occurred. Malformed local owner requests
  // cannot take down the continuing worker or imply submission permission.
  atomic_json(&c.root.join("fundamental-reload/last-request.json"),&json!({"at":iso(now()),"state":"rejected_before_staging","reason":error.to_string()}))?;return Ok(())}};
 let approved=pending.approved.take().ok_or("LIVE_RELOAD_REQUEST_CONSUMED")?;
 if approved.generation_sha()==f.cohorts.generation_sha(){let _=pending.reply(&json!({"state":"already_active","generationSha256":f.cohorts.generation_sha()}));return Ok(())}
 refresh_controls(c,authority,stream)?;f.authority.validate()?;
 moloch_live_runtime::fundamental_reload::quiescent(log)?;t.package_queue_quiet()?;
 if packages.records().iter().any(|p|!["ack","definitive_zero","first_call_not_sent","not_sent","never_attempted"].iter().any(|phase|p["phase"]==*phase))||bank.engine().package_records().as_array().is_none_or(|ps|ps.iter().any(|p|!["complete","no_execution"].iter().any(|phase|p["phase"]==*phase))){let _=pending.reply(&json!({"state":"deferred_package_unresolved"}));return Ok(())}
 // Candidate-only file/config errors have no durable effect. Errors in the
 // continuing worker, actual transport, STOP or original journals remain fatal.
 let staged=(||->Result<_>{
  approved.validate(bank,log,lease,base,f.cohorts.generation_sha(),now())?;
  let control:Control=serde_json::from_slice(&approved.control_bytes()?)?;
  if !control.fundamental_reload{return Err("LIVE_RELOAD_CANNOT_DISABLE".into())}
  approved.load(base)
 })();
 let loaded=match staged{Ok(v)=>v,Err(error)=>{let _=pending.reply(&json!({"state":"rejected_before_commit","reason":error.to_string()}));return Ok(())}};
 let facts=cancel_snapshots_with(|endpoint|{
  refresh_controls(c,authority,stream)?;let receipt=request(t,&c.root,endpoint)?;
  drain_stream(stream,bank,source_state,breadth,universe)?;refresh_controls(c,authority,stream)?;Ok(receipt)
 })?;
 let account_stamp=match moloch_live_runtime::account_realtime::capture(&stream.account_signal(),&[&facts.account,&facts.positions,&facts.open_orders,&facts.cash_balance]){
  Ok(v)=>v,Err(error)if error.downcast_ref::<moloch_live_runtime::account_realtime::AccountGenerationChanged>().is_some()=>{let _=pending.reply(&json!({"state":"deferred_account_generation_changed"}));return Ok(())},Err(error)=>return Err(error)
 };
 t.package_queue_quiet()?;
 authority.begin_fundamental_generation();
 if let Err(error)=f.cohorts.install_generation(&approved,&loaded,bank,log,&c.scope,&facts,base,now(),||{
  refresh_controls(c,authority,stream)?;t.package_queue_quiet()?;
  moloch_live_runtime::account_realtime::validate(&account_stamp,&stream.account_signal())
 }){
  if f.cohorts.generation_poisoned()||!fundamental_reload_rejection(&error){return Err(error)}
  authority.reject_uncommitted_fundamental_generation();let _=pending.reply(&json!({"state":"rejected_before_commit","reason":error.to_string()}));return Ok(())
 }
 // From this durable point every error is fatal, not a failed request/retry.
 // Startup replays exactly this generation before any dispatch and completes
 // the original exclusion update idempotently; no pointer file is authority.
 let active=moloch_live_runtime::fundamental_reload::Active::installed(&f.cohorts,approved,&loaded)?;
 f.authority=loaded.into_authority();
 install_managed_entry_exclusions(c,base_path,bank,log,Some(f))?;
 authority.install_fundamental_generation(active)?;
 atomic_json(&c.root.join("fundamental-reload/status.json"),&channel.status(base,bank,f.cohorts.generation_sha())?)?;
 let _=pending.reply(&json!({"state":"committed","generationSha256":f.cohorts.generation_sha(),"nativeHead":bank.durable_head(),"baseFinancialControlUnchanged":true}));Ok(())
}
fn install_human_market_breadth(c:&Control,control_path:&Path,bank:&mut Session)->Result<()>{
 if !c.all_markets||bank.engine().fresh_market_limit()>=12{return Ok(())}
 let policy=bank.engine().policy();
 if (policy["maximumCommitted"]!=c.activation.maximum_committed&&!(c.owner_capital_allocation.is_some()&&!owner_capital_ready(c,bank)?&&policy["maximumCommitted"]==c.policy["maximumCommitted"]))||policy["maximumMarketCost"]!=c.activation.maximum_market_cost||policy["quantity"].as_u64()!=Some(u64::from(c.activation.maximum_quantity)){return Err("LIVE_BREADTH_MONETARY_CAP_MISMATCH".into())}
 let old=bank.engine().fresh_market_limit();let maximum=policy["maximumMarkets"].as_u64().ok_or("LIVE_BREADTH_MARKET_LIMIT")?;let effective=maximum.checked_add(12u64.checked_sub(old as u64).ok_or("LIVE_BREADTH_MARKET_LIMIT")?).ok_or("LIVE_BREADTH_MARKET_LIMIT")?;
 let at=now();let event=json!({"type":"live_market_capacity_v1","source":"moloch_live_dispatcher","receivedAt":iso(at),"processedAt":iso(at),"bankScope":{"runId":c.activation.run_id,"bankManifestHash":bank.manifest_hash(),"tournamentId":c.scope.tournament_id},"bankHead":bank.durable_head(),"expectedReportSha256":hash(&bank.engine().report()?),"expectedPolicySha256":hash(&policy),"expectedPreviousFreshMarketLimit":old,"expectedPreviousMaximumMarkets":maximum,"freshMarketLimit":12,"effectiveMaximumMarkets":effective,"scopeExchangeIds":c.scope.exchanges.keys().collect::<Vec<_>>(),"controlSha256":format!("{:x}",Sha256::digest(fs::read(control_path)?)),"authorizationId":"human_moloch_breadth_20261016","expectedPreviousCapacitySha256":bank.engine().live_market_capacity_record().map(|v|v["sha256"].clone()).unwrap_or(Value::Null)});
 accept(bank,&event)?;let _=atomic_json(&c.root.join("ordinary-market-breadth.json"),&json!({"at":iso(now()),"record":bank.engine().live_market_capacity_record(),"monetaryCapsChanged":false,"additionalDiscoveryReadSlots":0}));Ok(())
}
fn existing_target_busy(bank:&Session,log:&DispatchLog,exchange:&str)->Result<bool>{
 log.require_resolved_attempts()?;let orders=bank.engine().order_records();
 for id in log.acknowledgments.keys().filter(|id|!log.closed.contains(id)){
  let q=orders.iter().find(|q|q["id"]==*id).ok_or("LIVE_ACK_QUOTE_MISSING")?;if q["exchangeId"]==exchange{return Ok(true)}
 }
 Ok(orders.iter().any(|q|q["exchangeId"]==exchange&&q["retired"]==false&&(q.get("reviewedInventoryExit").is_some()||q.get("reviewedFundamentalEntry").is_some()||q["id"].as_u64().is_some_and(|id|bank.engine().is_package_order(id)||!log.never_attempted(id)))))
}
/// Selection only. A passive value reservation may outlive its first packet,
/// but an original intent/attempt or physical liability never grants a retry.
fn passive_reserved_exit(bank:&Session,log:&DispatchLog,scope:&Scope,exchange:&str,at:u64)->Result<Option<Value>>{
 log.require_resolved_attempts()?;
 if !bank.engine().managed_inventory_planning_excludes(exchange)||bank.engine().package_blocks_submissions()
  ||profit_refresh_protected(scope,&bank.engine().package_records())?.contains(exchange){return Ok(None)}
 let snapshot=moloch_live_runtime::order_snapshot::snapshot(bank);
 let mut active=snapshot.records().iter().filter(|q|q["exchangeId"]==exchange&&q["retired"]==false);
 let Some(q)=active.next()else{return Ok(None)};if active.next().is_some(){return Ok(None)}
 let id=q["id"].as_u64().ok_or("LIVE_QUOTE_ID")?;
 if q["reviewedInventoryExit"]["executionMode"]!="passiveValueLimit"||q["action"]!="sell"
  ||scope.exchanges.get(exchange).is_none_or(|market|q["marketId"]!=*market)||q["filled"].as_u64()!=Some(0)
  ||q["quantity"].as_u64().is_none_or(|n|n==0)||q["expiresAt"].as_u64().is_none_or(|end|end<=at)
  ||q.get("cancelEffectiveAt").is_some_and(|v|!v.is_null())||bank.engine().is_package_order(id)
  ||!log.never_attempted(id)||log.intents.contains_key(&id)||log.recovered.contains_key(&id)
  ||log.closed.contains(&id)||log.cancel_attempted(id)||bank.engine().live_order_bindings().contains_key(&id)
  ||bank.engine().fills().iter().any(|f|f["orderId"]==id){return Ok(None)}
 for id in log.acknowledgments.keys().chain(bank.engine().live_order_bindings().keys()).filter(|id|!log.closed.contains(id)){
  let other=snapshot.quote(*id).ok_or("LIVE_ACK_QUOTE_MISSING")?;if other["exchangeId"]==exchange{return Ok(None)}
 }
 Ok(Some(q.clone()))
}
fn existing_exit_read_hint(bank:&Session,log:&DispatchLog,scope:&Scope,f:&FundamentalRuntime,exchange:&str,at:u64)->Result<Option<fundamental::ExistingExitHint>>{
 let untouched_passive=if existing_target_busy(bank,log,exchange)?{if passive_reserved_exit(bank,log,scope,exchange,at)?.is_none(){return Ok(None)}true}else{false};
 let mut hint=f.cohorts.existing_exit_hint(bank,log,scope,exchange,f.authority.trusted(),at)?;
 // This owned reservation already exists; a stale book nominates its SAME
 // seven-fact retry, rather than relegating it to the ordinary read-hint union.
 // Selection grants no freshness or physical authority and never renews qid/TTL.
 if untouched_passive{if let Some(h)=hint.as_mut(){if h.reason=="reviewed_inventory_book_refresh"{h.reason="reviewed_passive_reservation_retry".into();}}}
 Ok(hint)
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
fn paper5_due(last:&mut u64,at:u64)->Result<bool>{if *last>at{return Err("LIVE_P5_SCAN_CLOCK".into())}if *last>0&&at-*last<60_000{return Ok(false)}*last=at;Ok(true)}
fn paper5_expected_defer(error:&dyn std::error::Error)->bool{matches!(error.to_string().as_str(),
 "P5_EXECUTION_NOT_CONFIGURED"|"P5_EXECUTION_RESEARCH_MODEL"|"P5_EXECUTION_ORIGINAL_MODEL_CLOCK"|"P5_EXECUTION_REVIEW_EXPIRED"|"P5_EXECUTION_TARGET_BOUND"|"P5_EXECUTION_WORK_BOUND"|"P5_EXECUTION_SCOPE"|"P5_EXECUTION_CURRENT_NODE_MISSING"|"P5_EXECUTION_EXPIRED_OR_RESEARCH"|"P5_EXECUTION_BANK_CHANGED"|
 "SEMANTIC_REVIEW_STALE"|"SEMANTIC_CURRENT_STALE"|"SEMANTIC_CONTRACT_CHANGED"|"SURFACE_BANK_OR_CLOCK"|"SURFACE_CURRENT_CONTRACT"|"SURFACE_CURRENT_CONTRACT_CHANGED"|"SURFACE_CURRENT_NATIVE_BOOK_REQUIRED"|"SURFACE_TERMINAL_EXPIRED_OR_REVOKED"|"SURFACE_MODEL_REVIEW_PROBABILITY_DRIFT"|"SURFACE_NOT_CURRENT"|"SURFACE_FROZEN_BOUNDARY_CHANGED"|
 "PROB_CONTEXT"|"PROB_PENDING_COMPONENT"|"PROB_PROTECTED_COMPONENT"|"PROB_OPPOSITE_HELD_NETTING_UNSUPPORTED"|"PROB_FRESH_BOOK_REQUIRED"|"PROB_PLAN_CHANGED_OR_EXPIRED"|"PROB_RESERVATION_WINDOW"|"PROB_MODEL_NOT_EXECUTION_QUALIFIED"|
 "PACKAGE_PREFLIGHT_RECEIPT_CLOCK_OR_STATUS"|"PACKAGE_PREFLIGHT_BOOK_CLOCK"|"PACKAGE_PREFLIGHT_TOURNAMENT_ENDED")}
fn paper5_status(c:&Control,runtime:&moloch_live_runtime::paper5_runtime::Runtime,stage:&str,reason:&str){let _=atomic_json(&c.root.join("paper5-execution.json"),&json!({"at":iso(now()),"state":"deferred","stage":stage,"reason":reason,"sourceSha256":runtime.source_sha256(),"physicalAttempted":false,"profitabilityEstablished":false}));}
/// Only positive original-dispatch never-attempted ordinary drafts can yield.
/// Closed history is preserved; any known open binding or origin-bound draft
/// withholds the opportunity instead of canceling or taking its inventory.
fn paper5_component_drafts(bank:&Session,ordinary:&DispatchLog,exchanges:&std::collections::BTreeSet<String>)->Result<Option<Vec<Value>>>{
 ordinary.require_resolved_attempts()?;let mut drafts=Vec::new();
 for q in bank.engine().order_records(){if !q["exchangeId"].as_str().is_some_and(|ex|exchanges.contains(ex)){continue}let id=q["id"].as_u64().ok_or("LIVE_QUOTE_ID")?;
  if (ordinary.acknowledgments.contains_key(&id)||bank.engine().live_order_bindings().contains_key(&id))&&!ordinary.closed.contains(&id){return Ok(None)}
  if q["retired"]==true{continue}
  if bank.engine().is_package_order(id)||q.get("reviewedInventoryExit").is_some()||q.get("reviewedFundamentalEntry").is_some()||!ordinary.never_attempted(id)||bank.engine().live_order_bindings().contains_key(&id){return Ok(None)}drafts.push(q);
 }Ok(Some(drafts))
}
/// Optional, bounded opportunity lane using the SAME account worker/limiter.
/// Cached source approval gates all extra reads; inference precedes any permit.
/// Research/expired models cannot stage, and restart recovery never resends.
fn paper5_opportunity(t:&mut Transport,c:&Control,bank:&mut Session,ordinary:&mut DispatchLog,authority:&ProductionAuthority,packages:&mut moloch_live_runtime::package_dispatch::PackageDispatchLog,runtime:Option<&moloch_live_runtime::paper5_runtime::Runtime>,source_state:&mut RestSource,stream:&Realtime,breadth:&mut Option<Recorder>,universe:&mut Option<Universe>,windows:&mut std::collections::BTreeMap<String,u64>,last_scan:&mut u64,managed:&std::collections::BTreeSet<String>)->Result<()>{
 use moloch_live_runtime::{package_worker,probabilistic_portfolio_worker as worker};
 let Some(runtime)=runtime else{return Ok(())};let at=now();if !paper5_due(last_scan,at)?{return Ok(())}
 if !authority.can_read(&c.scope){return Err("LIVE_PACKAGE_OWNER_LOST".into())}require_stream(stream)?;
 if at>=authority.entry_until_ms()||ordinary_liability_due(bank,ordinary,at)?||packages.unresolved()||bank.engine().package_blocks_submissions(){paper5_status(c,runtime,"priority","ENTRY_CLOSED_OR_EXISTING_LIABILITY");return Ok(())}
 ordinary.require_resolved_attempts()?;match t.package_queue_quiet(){Ok(_)=>{},Err(e)if e.code=="PACKAGE_QUEUE_NOT_QUIET"=>{paper5_status(c,runtime,"queue",&e.code);return Ok(())},Err(e)=>return Err(e.into())}
 let targets=match runtime.execution_read_targets(bank,&c.scope,ordinary,now()){Ok(v)=>v,Err(e)if paper5_expected_defer(e.as_ref())=>{paper5_status(c,runtime,"sources",&e.to_string());return Ok(())},Err(e)=>return Err(e)};
 let exchanges=targets.iter().map(|(ex,_)|ex.clone()).collect::<std::collections::BTreeSet<_>>();
 if exchanges.iter().any(|ex|managed.contains(ex))||paper5_component_drafts(bank,ordinary,&exchanges)?.is_none(){paper5_status(c,runtime,"ownership","MANAGED_OR_PHYSICAL_COMPONENT_BUSY");return Ok(())}
 for(ex,market)in &targets{
  request(t,&c.root,Endpoint::Nodes{market_id:market.clone()})?;if !authority.can_read(&c.scope){return Err("LIVE_PACKAGE_OWNER_LOST".into())}require_stream(stream)?;drain_stream(stream,bank,source_state,breadth,universe)?;
  let from=*windows.get(ex).ok_or("LIVE_PACKAGE_SOURCE_WINDOW")?;let next=source(c,bank,source_state,ex,market,from,&mut LivePreflightReads{t,c,authority,stream,breadth,universe})?;windows.insert(ex.clone(),next);
  if !authority.can_read(&c.scope){return Err("LIVE_PACKAGE_OWNER_LOST".into())}require_stream(stream)?;drain_stream(stream,bank,source_state,breadth,universe)?;
 }
 let nodes=paper5_nodes(&c.scope);
 // Bind all current source/contract facts BEFORE yielding any ordinary draft.
 let context=match runtime.execution_context(bank,&c.scope,ordinary,&nodes,now()){Ok(v)=>v,Err(e)if paper5_expected_defer(e.as_ref())=>{paper5_status(c,runtime,"current_context",&e.to_string());return Ok(())},Err(e)=>return Err(e)};context.validate_current(bank,&c.scope,ordinary,now())?;drop(context);
 let Some(drafts)=paper5_component_drafts(bank,ordinary,&exchanges)?else{paper5_status(c,runtime,"ownership","COMPONENT_BECAME_PHYSICALLY_BUSY");return Ok(())};for q in drafts{retire_for_package(bank,ordinary,&q)?;}
 // Retirement changes the true head. Rebind and optimize it, never re-use an
 // earlier Plan or relabel a previously retired/attempted intent.
 let context=match runtime.execution_context(bank,&c.scope,ordinary,&nodes,now()){Ok(v)=>v,Err(e)if paper5_expected_defer(e.as_ref())=>{paper5_status(c,runtime,"rebind",&e.to_string());return Ok(())},Err(e)=>return Err(e)};
 let plan=match context.optimize(bank,&c.scope,ordinary,now()){Ok(Some(v))=>v,Ok(None)=>{paper5_status(c,runtime,"value","NO_CURRENT_EXECUTABLE_MODEL_EDGE");return Ok(())},Err(e)if paper5_expected_defer(e.as_ref())=>{paper5_status(c,runtime,"optimize",&e.to_string());return Ok(())},Err(e)=>return Err(e)};
 let receipts=c.root.join("package-receipts");let mut backend=package_worker::Live{transport:t,authority,ordinary_dispatch:ordinary,receipts:&receipts};
 let staged=match worker::stage(bank,packages,&backend,plan,context.surface(),context.semantics(),context.policy(),&c.activation){Ok(v)=>v,Err(e)if paper5_expected_defer(e.as_ref())=>{worker::recover(bank,packages,&mut backend)?;paper5_status(c,runtime,"stage",&e.to_string());return Ok(())},Err(e)=>return Err(e)};
 let native=bank.engine().package_records();let native=native.as_array().ok_or("LIVE_PACKAGE_RECORDS")?.iter().find(|p|p["packageId"]==staged.package_id()).ok_or("LIVE_PACKAGE_ID")?;let orders=bank.engine().order_records();
 let ready=native["legs"].as_array().ok_or("LIVE_PACKAGE_LEGS")?.iter().map(|leg|orders.iter().find(|q|q["id"]==leg["qid"]).and_then(|q|q["activationAt"].as_u64()).ok_or("LIVE_PACKAGE_ACTIVATION")).collect::<std::result::Result<Vec<_>,_>>()?.into_iter().max().ok_or("LIVE_PACKAGE_ACTIVATION")?;
 while now()<ready{
  if !authority.can_read(&c.scope){return Err("LIVE_PACKAGE_OWNER_LOST".into())}require_stream(stream)?;drain_stream(stream,bank,source_state,breadth,universe)?;
  if now()>=staged.plan().expires_at_ms(){worker::recover(bank,packages,&mut backend)?;paper5_status(c,runtime,"activation","ORIGINAL_PLAN_EXPIRED");return Ok(())}std::thread::sleep(Duration::from_millis(ready.saturating_sub(now()).min(100)));
 }
 let facts=moloch_live_runtime::package_preflight::collect_probabilistic_staged(&mut backend,&c.scope,&staged,|r|{
  record_receipt(&c.root,r)?;if !authority.can_read(&c.scope){return Err("LIVE_PACKAGE_OWNER_LOST".into())}require_stream(stream)?;
  for(ex,market)in &targets{if r.endpoint==format!("/markets/{market}/orderbook?tournamentId={}&depth=20",c.scope.tournament_id){accept_fresh_book(bank,c,&json!({"marketId":market,"exchangeId":ex}),r,source_state)?;}}
  drain_stream(stream,bank,source_state,breadth,universe)
 });
 let facts=match facts{Ok(v)=>v,Err(e)=>{worker::recover(bank,packages,&mut backend)?;if paper5_expected_defer(e.as_ref()){paper5_status(c,runtime,"physical_packet",&e.to_string());return Ok(())}return Err(e)}};
 let result=worker::submit_once(bank,packages,&mut backend,staged,facts)?;let recovered=worker::recover(bank,packages,&mut backend)?;
 let _=atomic_json(&c.root.join("paper5-execution.json"),&json!({"at":iso(now()),"state":format!("{result:?}"),"sourceSha256":runtime.source_sha256(),"blocksNewSubmissions":recovered.blocks_new_submissions,"actualFillEvents":recovered.fill_events,"completedPackages":recovered.completed_packages,"profitabilityEstablished":false}));Ok(())
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
 log.append(event)?;moloch_live_runtime::execution_distribution::track_runtime_attempt(bank,log,qid,at);moloch_live_runtime::execution_distribution::track_acquisition_attempt(bank,log,qid,at);if config.is_some(){moloch_live_runtime::execution_edge::track_runtime_attempt(bank,log,qid,at);}Ok(())
}
/// Only an account-generation fence BEFORE the original operation attempt may
/// defer. No SDK error, control/STOP error, or already issued operation is hidden.
fn defer_account_preintent(c:&Control,bank:&Session,log:&DispatchLog,authority:&ProductionAuthority,qid:u64,cancel:bool,error:&(dyn std::error::Error+'static))->Result<bool>{
 if !error.is::<moloch_live_runtime::account_realtime::AccountGenerationChanged>(){return Ok(false)}
 let untouched=if cancel{cancel_may_start(log,qid)&&bank.engine().live_order_bindings().contains_key(&qid)&&log.acknowledgments.contains_key(&qid)}else{
  log.require_resolved_attempts()?;
  log.never_attempted(qid)&&!log.intents.contains_key(&qid)&&!log.acknowledgments.contains_key(&qid)&&!bank.engine().live_order_bindings().contains_key(&qid)&&!bank.engine().fills().iter().any(|f|f["orderId"]==qid)
 };
 if !untouched{return Err("LIVE_ACCOUNT_DEFER_AFTER_ORIGINAL_OPERATION".into())}
 authority.clear_ready();atomic_json(&c.root.join("account-realtime-deferred.json"),&json!({"at":iso(now()),"qid":qid,"operation":if cancel{"cancel"}else{"order"},"reason":error.to_string(),"nativeHead":bank.durable_head(),"dispatch":log.descriptor_identity()?,"physicalAttempted":false,"requiresAuthoritativeRefresh":true}))?;Ok(true)
}
/// A freshness loss during bind or physical capability minting can decline an
/// untouched quote. This helper is never used after an original intent/attempt.
fn fundamental_book_expiry_error(error:&dyn std::error::Error)->bool{
 matches!(error.to_string().as_str(),"FUNDAMENTAL_PHYSICAL_BOOK_STALE"|"FUNDAMENTAL_NOT_ELIGIBLE"|"FUNDAMENTAL_PHYSICAL_EXIT_NOT_ELIGIBLE")
}
fn fundamental_stale_preintent(bank:&Session,log:&DispatchLog,scope:&Scope,expected:&Value,error:&dyn std::error::Error,sdk_unknown:bool,at:u64)->Result<bool>{
 if !fundamental_book_expiry_error(error){return Ok(false)}
 log.require_resolved_attempts()?;
 if sdk_unknown{return Err("LIVE_FUNDAMENTAL_STALE_DEFER_UNKNOWN_ACCEPTANCE".into())}
 let qid=expected["id"].as_u64().ok_or("LIVE_QUOTE_ID")?;let current=quote(bank,qid)?;
 if current!=*expected{return Err("LIVE_FUNDAMENTAL_STALE_DEFER_QUOTE_CHANGED".into())}
 if !log.never_attempted(qid)||log.intents.contains_key(&qid)||log.acknowledgments.contains_key(&qid)||bank.engine().live_order_bindings().contains_key(&qid)||bank.engine().fills().iter().any(|f|f["orderId"]==qid){return Err("LIVE_FUNDAMENTAL_STALE_DEFER_AFTER_ORIGINAL_OPERATION".into())}
 let ex=current["exchangeId"].as_str().ok_or("LIVE_SCOPE")?;
 let age=bank.engine().policy()["bookAgeMs"].as_u64().ok_or("FUNDAMENTAL_BOOK_POLICY")?;
 let(books,_)=moloch_live_runtime::package_admission::native_books(bank,scope,at,age)?;
 if books.contains_key(ex){return Err("LIVE_FUNDAMENTAL_STALE_DEFER_CURRENT_BOOK".into())}Ok(true)
}
enum FundamentalPreintent{Ready(fundamental::PhysicalDecision),Deferred(String)}
/// Keep BOTH original fallible preparation calls inside this pre-intent guard.
/// bind_quantity assesses the current book before it can append a binding; its
/// NOT_ELIGIBLE must receive the same positive proof as mint's BOOK_STALE.
fn fundamental_preintent(f:&mut FundamentalRuntime,bank:&Session,log:&DispatchLog,scope:&Scope,q:&Value,n:u64,request:&RequestSpec,permit:&Permit,mut clock:impl FnMut()->u64,quiet_queue:impl FnOnce()->Result<()>)->Result<FundamentalPreintent>{
 let qid=q["id"].as_u64().ok_or("LIVE_QUOTE_ID")?;
 let prepared=(||{
  f.cohorts.bind_quantity(bank,log,scope,qid,n,request,f.authority.trusted(),clock())?;
  f.cohorts.physical_decision(bank,log,scope,qid,request,permit,&f.authority,clock())
 })();
 match prepared{
  Ok(decision)=>Ok(FundamentalPreintent::Ready(decision)),
  Err(error)=>{
   if fundamental_book_expiry_error(error.as_ref()){
    // The original SDK snapshot must exclude unknown acceptance, active
    // admission and manual review before any expected freshness defer.
    quiet_queue()?;
    if fundamental_stale_preintent(bank,log,scope,q,error.as_ref(),false,clock())?{return Ok(FundamentalPreintent::Deferred(error.to_string()))}
   }Err(error)
  }
 }
}
fn submit_order(t:&mut Transport,c:&Control,bank:&mut Session,log:&mut DispatchLog,authority:&Arc<ProductionAuthority>,q:&Value,facts:SnapshotSet,maker:Option<(Arc<moloch_live_runtime::maker_policy::Decision>,&moloch_live_runtime::maker_policy::Reviewed)>,mut fundamental:Option<&mut FundamentalRuntime>)->Result<()>{
 if !owner_capital_buys_allowed(c,bank,&q["action"])?{return Ok(())}
 use moloch_live_runtime::decision_trace::{Trace,Stage};
 // Only bounded in-memory timestamps precede the physical call. Diagnostic IO
 // happens after the original ACK and native accounting have committed.
 let trace_start=now();let mut trace=Trace::new(&c.activation.run_id,q["id"].as_u64().ok_or("LIVE_QUOTE_ID")?,trace_start)?;
 trace.candidate_created_at_ms=q["createdAt"].as_u64();trace.book_received_at_ms=moloch_live_runtime::daily_windows::clock(&facts.book.received_at).ok();
 for receipt in [&facts.account,&facts.cash_balance,&facts.positions,&facts.open_orders,&facts.market,&facts.contract,&facts.book]{if let(Ok(start),Ok(end))=(moloch_live_runtime::daily_windows::clock(&receipt.request_started_at),moloch_live_runtime::daily_windows::clock(&receipt.received_at)){let _=trace.record(Stage::PreflightRead,start,end);}}
 let qid=q["id"].as_u64().ok_or("LIVE_QUOTE_ID")?;let(mut order,mut request_spec,mut p)=physical_order(c,q)?;
 trace.permit_issued_at_ms=Some(p.expires_at_ms.saturating_sub(15000));trace.permit_expires_at_ms=Some(p.expires_at_ms);
 let fundamental_decision=if let Some(f)=fundamental.as_mut(){
  f.authority.validate()?;
  if !f.eligible(bank,log,&c.scope,qid,now())?{authority.clear_ready();return Ok(())}
  let n=f.cohorts.planned_entry_quantity(bank,log,&c.scope,qid,f.authority.trusted(),now())?.unwrap_or(u64::from(order.quantity));
  order.quantity=u32::try_from(n)?;request_spec=spec(&order)?;p.maximum_quantity=order.quantity;p.maximum_cost=money(decimal_units(&order.price)?.checked_mul(i128::from(n)).ok_or("LIVE_COST_OVERFLOW")?);
  let decision=match fundamental_preintent(f,bank,log,&c.scope,q,n,&request_spec,&p,now,||recovery_queue_clear(&t.admission_state()?))?{
   FundamentalPreintent::Ready(decision)=>decision,
   FundamentalPreintent::Deferred(reason)=>{
    authority.clear_ready();atomic_json(&c.root.join("fundamental-preintent-deferred.json"),&json!({"at":iso(now()),"qid":qid,"reason":reason,"nativeHead":bank.durable_head(),"dispatch":log.descriptor_identity()?,"physicalAttempted":false,"requiresAuthoritativeRefresh":true,"cohortBindingPreserved":true}))?;return Ok(())
   }
  };Some(Arc::new(decision))
 }else{None};
 if let Some((decision,review))=&maker{if let Err(error)=decision.validate_current(bank,review,log,now()){
  if !expected_maker_expiry(error.as_ref()){return Err(error)}authority.clear_ready();atomic_json(&c.root.join("maker-policy-deferred.json"),&json!({"at":iso(now()),"qid":qid,"reason":error.to_string(),"physicalAttempted":false}))?;return Ok(())
 }}
 // Configuration or a marker is not an entry capability. Recompute covered
 // opposite inventory against the final fresh state before any durable intent.
 if q["action"]=="buy"&&fundamental_decision.is_none()&&maker.is_none()&&!unreviewed_inventory_reduction(c,bank,log,q)?{
  authority.clear_ready();atomic_json(&c.root.join("ordinary-entry-quarantine-deferred.json"),&json!({"at":iso(now()),"qid":qid,"reason":"unreviewed_new_exposure","physicalAttempted":false}))?;return Ok(())
 }
 if let Err(error)=authority.prepare_with_policies(bank,qid,request_spec.clone(),p.clone(),facts,maker.as_ref().map(|(decision,_)|decision.clone()),fundamental_decision){if defer_account_preintent(c,bank,log,authority,qid,false,error.as_ref())?{return Ok(())}return Err(error)}
 if let Err(e)=authority.authorize(&c.scope,&request_spec,&p,false,now()){authority.clear_ready();atomic_json(&c.root.join("deferred.json"),&json!({"at":iso(now()),"qid":qid,"code":e.code}))?;return Ok(())}
 if log.intents.contains_key(&qid)||!log.never_attempted(qid){return Err("LIVE_EXISTING_UNRESOLVED_INTENT".into())}
 let durability_start=now();let _=trace.record(Stage::PolicyAndRisk,trace_start,durability_start);
 log.append(json!({"kind":"intent","qid":qid,"quote":q,"spec":request_spec,"permit":p,"at":iso(now()),"maker":maker.as_ref().map(|(decision,_)|decision.proof()),"fundamental":fundamental.as_ref().and_then(|f|f.cohorts.durable_tag(qid))}))?;
 record_original_attempt(bank,log,qid,if fundamental.is_some(){None}else{c.execution_model.as_ref()},now())?;
 if let Some(f)=fundamental.as_ref(){f.authority.validate()?;f.cohorts.check_bound(bank,log,&c.scope,qid,&request_spec,f.authority.trusted(),now())?;}
 let transport_start=now();let _=trace.record(Stage::IntentDurability,durability_start,transport_start);
 let ack=match t.place(order,p){Ok(r)=>r,Err(e)=>{let end=now();let _=trace.record(Stage::TransportCall,transport_start,end);if let Ok(summary)=trace.finish(end){let _=atomic_json(&c.root.join("decision-trace-latest.json"),&json!({"trace":trace,"summary":summary,"outcome":"transport_error","diagnosticOnly":true}));}log.append(json!({"kind":"error","qid":qid,"error":e,"at":iso(now())}))?;
  if let Ok((spec,permit,at,error))=log.pre_admission_originals(qid){
   if let Ok(proof)=t.verify_current_ordinary_pre_admission(&spec,&permit,at,&error){log.append_aborted_before_admission(qid,&proof,&iso(now()))?;authority.clear_ready();retire_proved_unadmitted(bank,log,qid)?;return Ok(())}
  }
  return Err("LIVE_WRITE_NOT_CONFIRMED_NO_RETRY".into())}};
 let ack_start=now();let _=trace.record(Stage::TransportCall,transport_start,ack_start);trace.ack_received_at_ms=moloch_live_runtime::daily_windows::clock(&ack.received_at).ok();if let(Ok(start),Ok(end))=(moloch_live_runtime::daily_windows::clock(&ack.request_started_at),moloch_live_runtime::daily_windows::clock(&ack.received_at)){let _=trace.record(Stage::RecordedRequestService,start,end);}
 record_receipt(&c.root,&ack)?;log.append(json!({"kind":"ack","qid":qid,"receipt":ack}))?;
 let event=feedback::acknowledgment(q,&ack,&iso(now()))?;if let Some(f)=fundamental.as_ref(){f.cohorts.validate_acknowledgment(qid,&event)?;}authority.bind_owned(event["actualOrderId"].as_str().ok_or("LIVE_ACTUAL_ID")?.into());accept(bank,&event)?;authority.clear_ready();let end=now();let _=trace.record(Stage::NativeAckApply,ack_start,end);if let Ok(summary)=trace.finish(end){let _=atomic_json(&c.root.join("decision-trace-latest.json"),&json!({"trace":trace,"summary":summary,"outcome":"owned_ack_applied","diagnosticOnly":true}));}Ok(())
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
/// The two booleans come only from the CURRENT actual Session projections.
/// An SDK-proved local abort is never retryable and grants no synthetic close.
/// Contradictory actual execution is integrity failure, even for a retired qid.
fn refresh_physical_pending(log:&DispatchLog,qid:u64,native_binding:bool,actual_fill:bool)->Result<bool>{
 let actual=log.acknowledgments.contains_key(&qid)||native_binding||actual_fill;
 if log.proved_no_network(qid)&&actual{return Err("LIVE_ABORT_PROOF_CONTRADICTS_ACTUAL_EXECUTION".into())}
 Ok(!log.closed.contains(&qid)&&(actual||(log.attempted.contains(&qid)&&!log.proved_no_network(qid))))
}
fn refresh_safety(c:&Control,bank:&Session,log:&DispatchLog,authority:&ProductionAuthority,stream:&Realtime,q:&Value,global_unknown:bool)->Result<moloch_live_runtime::exit_refresh::Safety>{
 refresh_controls(c,authority,stream)?;log.require_resolved_attempts()?;
 let qid=q["id"].as_u64().ok_or("LIVE_QUOTE_ID")?;let ex=q["exchangeId"].as_str().ok_or("LIVE_SCOPE")?;
 let side=match q["side"].as_str(){Some("YES")=>moloch_replay::Side::Yes,Some("NO")=>moloch_replay::Side::No,_=>return Err("LIVE_SIDE".into())};
 let records=bank.engine().order_records();let mut physical=false;let mut reserved=0u64;
 let actual_fills:std::collections::BTreeSet<_>=bank.engine().fills().iter().map(|f|f["orderId"].as_u64().ok_or("LIVE_ACTUAL_FILL_NATIVE_ID")).collect::<std::result::Result<_,_>>()?;
 for old in &records{let id=old["id"].as_u64().ok_or("LIVE_QUOTE_ID")?;if old["exchangeId"]!=ex||id==qid{continue}
  if refresh_physical_pending(log,id,bank.engine().live_order_bindings().contains_key(&id),actual_fills.contains(&id))?{physical=true;}
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
// A preparation hint only: final selection still binds the new quote to all
// seven original receipts and reruns current lifecycle/coverage/profit checks.
fn profit_successor_exists(bank:&Session,old:&Value,hint:&moloch_live_runtime::exit_refresh::ReadHint,cutoff:u64,at:u64)->Result<bool>{
 for q in bank.engine().order_records(){
  if q["id"].as_u64().is_some_and(|id|id>cutoff)&&q["marketId"]==hint.market()&&q["exchangeId"]==hint.exchange()&&q["side"]==old["side"]
   &&q["retired"]==false&&q.get("cancelEffectiveAt").is_none_or(Value::is_null)
   &&q["expiresAt"].as_u64().is_some_and(|end|end>at.saturating_add(3000))
   &&moloch_live_runtime::exit_refresh::ReadKind::from_quote(&q)==Some(moloch_live_runtime::exit_refresh::ReadKind::Profit)
   &&moloch_live_runtime::exit_refresh::current_profit_qualified(bank,&q)?{return Ok(true)}
 }Ok(false)
}
/// Negative selection only. Nonnegative native fees make gross best bid minus
/// worst remaining lot an UPPER bound on every deeper level's net gain. Equality,
/// missing evidence or changed context never vetoes the original final packet.
fn negative_profit_preparation(bank:&Session,scope:&Scope,log:&DispatchLog,old:&Value,hint:&moloch_live_runtime::exit_refresh::ReadHint,original:&Receipt,accepted:&[Value],admitted_head:&Value,managed:bool,at:u64)->bool{
 (||->Result<bool>{
  use moloch_live_runtime::exit_refresh::ReadKind;
  if bank.durable_head()!=*admitted_head||managed
   ||bank.engine().ordinary_entry_exclusions().contains(hint.exchange())||bank.engine().managed_inventory_planning_excludes(hint.exchange())
   ||hint.kind()!=ReadKind::Profit||ReadKind::from_quote(old)!=Some(ReadKind::Profit)
   ||old.get("reviewedInventoryExit").is_some_and(|v|!v.is_null())||old.get("reviewedFundamentalEntry").is_some_and(|v|!v.is_null())
   ||old["filled"].as_u64()!=Some(0)||old["id"].as_u64()!=Some(hint.old_qid())
   ||old["exchangeId"]!=hint.exchange()||old["marketId"]!=hint.market()
   ||!log.never_attempted(hint.old_qid())||log.intents.contains_key(&hint.old_qid())||bank.engine().is_package_order(hint.old_qid())
   ||!profit_hint_scope_allowed(old,&Default::default(),&profit_refresh_protected(scope,&bank.engine().package_records())?){return Ok(false)}
  log.require_resolved_attempts()?;
  if scope.exchanges.get(hint.exchange()).map(String::as_str)!=Some(hint.market())||original.status!=200
   ||original.endpoint!=format!("/markets/{}/orderbook?tournamentId={}&depth=20",hint.market(),scope.tournament_id){return Ok(false)}
  let policy=bank.engine().policy();let age=policy["bookAgeMs"].as_u64().filter(|n|*n>0).ok_or("LIVE_PROFIT_NEGATIVE_AGE")?;
  let start=moloch_live_runtime::daily_windows::clock(&original.request_started_at)?;let received=moloch_live_runtime::daily_windows::clock(&original.received_at)?;
  if start>received||received>at||at-start>=age{return Ok(false)}
  // The caller passes ONLY events returned by the successful actual admission.
  // Check their final target state, then bind to BOTH original normalization and
  // the currently valid native book (a later gap removes it from this view).
  let mut final_book=None;
  for e in accepted{
   if e["type"]=="gap"&&e["exchangeId"]==hint.exchange(){final_book=None;}
   if e["type"]=="book"&&e["book"]["exchangeId"]==hint.exchange(){
    if e["book"]["marketId"]!=hint.market(){return Ok(false)}
    if e["book"]["invalid"]==true{final_book=None;continue}
    final_book=Some(&e["book"]);
   }
  }
  let Some(admitted)=final_book else{return Ok(false)};
  let wrapped=json!({"source":"official_sig_api_v1","kind":"book","tournamentId":scope.tournament_id,"marketId":hint.market(),"exchangeId":hint.exchange(),"endpoint":original.endpoint,"status":original.status,"requestStartedAt":original.request_started_at,"receivedAt":original.received_at,"data":original.data});
  let expected=moloch_replay::source::receipt_events(&wrapped)?;let expected=expected.iter().find(|e|e["type"]=="book").ok_or("LIVE_PROFIT_NEGATIVE_BOOK")?;
  let normalize=|v:&Value|{let mut v=v.clone();if let Some(o)=v.as_object_mut(){o.remove("invalid");}v};
  if normalize(admitted)!=expected["book"]{return Ok(false)}
  let books=bank.engine().live_package_books();let matching:Vec<_>=books.iter().filter(|b|b["exchangeId"]==hint.exchange()).collect();
  if matching.len()!=1||normalize(matching[0])!=expected["book"]{return Ok(false)}
  let b=matching[0];if b["tournamentId"]!=scope.tournament_id||b["marketId"]!=hint.market(){return Ok(false)}
  let source=moloch_live_runtime::daily_windows::clock(b["sourceAt"].as_str().ok_or("LIVE_PROFIT_NEGATIVE_CLOCK")?)?;
  if source>received||at-source>=age{return Ok(false)}
  let levels=|key:&str,ascending:bool|->Result<Vec<i128>>{
   let rows=b[key].as_array().filter(|a|!a.is_empty()).ok_or("LIVE_PROFIT_NEGATIVE_DEPTH")?;let mut prices=Vec::new();
   for row in rows{let p=decimal_units(row["price"].as_str().ok_or("LIVE_PROFIT_NEGATIVE_DEPTH")?)?;
    if p<=0||p>=1_000_000||row["quantity"].as_u64().is_none_or(|n|n==0){return Err("LIVE_PROFIT_NEGATIVE_DEPTH".into())}
    if prices.last().is_some_and(|last|if ascending{p<=*last}else{p>=*last}){return Err("LIVE_PROFIT_NEGATIVE_DEPTH".into())}prices.push(p);
   }Ok(prices)
  };
  let bids=levels("bids",false)?;let asks=levels("asks",true)?;if bids[0]>=asks[0]{return Ok(false)}
  let positions=bank.engine().wallet.positions();let side=old["side"].as_str().ok_or("LIVE_SIDE")?;
  let lots=positions.get(hint.exchange()).and_then(|p|p.get(side)).and_then(Value::as_array).filter(|a|!a.is_empty()).ok_or("LIVE_PROFIT_NEGATIVE_HOLDING")?;
  let mut worst=0;for lot in lots{
   // A new execution/cohort lot does not inherit this inherited-only screen.
   if lot["quantity"].as_u64().is_none_or(|n|n==0)||lot.get("executedAt").is_some_and(|v|!v.is_null()){return Ok(false)}
   worst=worst.max(decimal_units(lot["price"].as_str().ok_or("LIVE_PROFIT_NEGATIVE_BASIS")?)?);
  }
  if worst<=0{return Ok(false)}
  let bps=policy["feeBps"].as_u64().ok_or("LIVE_PROFIT_NEGATIVE_FEES")?;
  let per=decimal_units(policy["feePerShare"].as_str().ok_or("LIVE_PROFIT_NEGATIVE_FEES")?)?;
  if bps>u32::MAX as u64||per<0{return Ok(false)}
  let minimum=policy.get("inventoryMinimumGain").map(|v|->Result<i128>{Ok(decimal_units(v.as_str().ok_or("LIVE_PROFIT_NEGATIVE_GAIN")?)?)}).transpose()?.unwrap_or(0);
  let bid=match side{"YES"=>bids[0],"NO"=>1_000_000-asks[0],_=>return Ok(false)};
  Ok(bid-worst<minimum)
 })().unwrap_or(false)
}
fn exit_recovery_from(source:&RestSource,market:&str,prior_window:u64)->Option<u64>{source.recovery_start_ms(market).map(|from|from.min(prior_window))}
fn refresh_exit(t:&mut Transport,c:&Control,bank:&mut Session,log:&mut DispatchLog,authority:&Arc<ProductionAuthority>,source:&mut RestSource,stream:&Realtime,old:&Value,hint:&moloch_live_runtime::exit_refresh::ReadHint,flow_started:u64,maximum_qid_before:u64,unknown:bool,prior_window:u64,breadth:&mut Option<Recorder>,universe:&mut Option<Universe>,fundamental:Option<&mut FundamentalRuntime>)->Result<()>{
 use moloch_live_runtime::exit_refresh::{RefreshFacts,BookAcceptance,PostBookPlanning,RefreshPlan,RefreshStage};
 atomic_json(&c.root.join("exit-refresh-nominee.json"),&json!({"at":iso(now()),"qid":old["id"],"exchangeId":old["exchangeId"],"side":old["side"],"reason":old["inventoryExitReason"],"physicalAttempted":false,"expiryExtended":false}))?;
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
 if hint.kind()==moloch_live_runtime::exit_refresh::ReadKind::Profit{
  if queue_has_unknown(t)?{return refresh_deferred(c,"preparation",&"GLOBAL_UNKNOWN_ACCEPTANCE")}
  // Give the ordinary native planner the actual target book BEFORE consuming
  // the timed seven-fact packet. Otherwise unrelated profitable drafts may use
  // the real write gap and the target appears only after that packet is over.
  // This extra read is not one of the final seven and grants no HTTP authority.
  let initial=preflight_read(bank,source,&mut LivePreflightReads{t,c,authority,stream,breadth,universe},None,Endpoint::OrderBook{market_id:market.into(),depth:20})?;
  let target=json!({"marketId":market,"exchangeId":hint.exchange()});let admitted=accept_fresh_book(bank,c,&target,&initial,source)?;
  let admitted_head=bank.durable_head();
  let target_managed=fundamental.as_ref().is_some_and(|f|f.cohorts.manages(hint.exchange()));
  if negative_profit_preparation(bank,&c.scope,log,old,hint,&initial,&admitted,&admitted_head,target_managed,now()){
   return refresh_deferred(c,"preparation_negative",&"CURRENT_GROSS_BID_BELOW_NATIVE_PROFIT_MINIMUM")
  }
  // A current unused inherited draft will be reconsidered AFTER the final
  // packet. Waiting for a successor before those reads only ages that draft.
  if !profit_successor_exists(bank,old,hint,maximum_qid_before,now())?
   &&profit_replan_draft(bank,log,&c.scope,hint,maximum_qid_before,flow_started,now())?.is_none(){
   let gap=bank.engine().policy()["writeGapMs"].as_u64().ok_or("LIVE_PROFIT_REFRESH_WRITE_GAP")?;
   if gap>3000{return refresh_deferred(c,"preparation",&"NATIVE_GAP_EXCEEDS_BOUNDED_PREPARATION")}
   let ready=now().checked_add(gap).ok_or("LIVE_PROFIT_REFRESH_CLOCK_RANGE")?;
   while now()<ready{refresh_controls(c,authority,stream)?;std::thread::sleep(Duration::from_millis(ready.saturating_sub(now()).clamp(1,100)));}
   refresh_controls(c,authority,stream)?;
   // A real current-time tick, not a rewritten book/source clock. Native
   // budgets and ranking still determine whether ANY new quote is generated.
   accept(bank,&json!({"type":"tick","receivedAt":iso(now())}))?;
  }
 }
 let facts=snapshots(t,c,authority,bank,source,stream,breadth,universe,old)?;
 let bound=match RefreshFacts::new(&c.scope,hint,[facts.account.clone(),facts.positions.clone(),facts.open_orders.clone(),facts.cash_balance.clone(),facts.market.clone(),facts.contract.clone(),facts.book.clone()],now()){
  Ok(v)=>v,Err(e) if matches!(e.to_string().as_str(),"EXIT_REFRESH_FACTS_STALE"|"EXIT_REFRESH_BOOK_STALE")=>{refresh_deferred(c,"facts_ineligible",e.as_ref())?;return Ok(())},Err(e)=>return Err(e)
 };
 let started=now();let events=accept_fresh_book(bank,c,old,&facts.book,source)?;let completed=now();let head=native_head(c)?;
 let acceptance=match BookAcceptance::from_events(&bound,&events,true,head,started,completed){
  Ok(v)=>v,Err(e)=>{refresh_deferred(c,"book_ineligible",e.as_ref())?;return Ok(())}
 };
 // Every I/O/control failure is propagated; only pure helper denials defer.
 let candidates:Vec<_>=bank.engine().order_records().into_iter().filter(|q|q["id"].as_u64().is_some_and(|id|id>maximum_qid_before&&id!=hint.old_qid())&&q["retired"]==false&&q["marketId"]==old["marketId"]&&q["exchangeId"]==old["exchangeId"]&&q["side"]==old["side"]&&q["action"]=="sell"&&q["executionStyle"]=="inventoryAON"&&moloch_live_runtime::exit_refresh::ReadKind::from_quote(q)==Some(hint.kind())&&q["filled"].as_u64()==Some(0)&&q.get("cancelEffectiveAt").is_none_or(Value::is_null)).collect();
 let mut safety=std::collections::BTreeMap::new();
 let candidates=if hint.kind()==moloch_live_runtime::exit_refresh::ReadKind::Profit{
  let mut qualified=Vec::new();for q in candidates{if moloch_live_runtime::exit_refresh::current_profit_qualified(bank,&q)?{qualified.push(q)}}qualified
 }else{candidates};
 for q in &candidates{if q["exchangeId"]==old["exchangeId"]&&q["side"]==old["side"]&&q["action"]=="sell"{safety.insert(q["id"].as_u64().ok_or("LIVE_QUOTE_ID")?,refresh_safety(c,bank,log,authority,stream,q,unknown)?);}}
 let plan=match RefreshPlan::select(&bound,&acceptance,&candidates,maximum_qid_before,flow_started,completed,now(),|id|*safety.get(&id).expect("validated native candidate safety")){
  Ok(v)=>v,
  Err(e)if hint.kind()==moloch_live_runtime::exit_refresh::ReadKind::Profit&&matches!(e.to_string().as_str(),"EXIT_REFRESH_NO_FRESH_CANDIDATE"|"EXIT_REFRESH_EXPIRY_HEADROOM")=>{
   let Some(draft)=profit_replan_draft(bank,log,&c.scope,hint,maximum_qid_before,flow_started,now())?else{return refresh_deferred(c,"candidate_ineligible",e.as_ref())};
   let unknown=queue_has_unknown(t)?;let checked=refresh_safety(c,bank,log,authority,stream,&draft,unknown)?;
   let at=now();let guard=if draft["expiresAt"].as_u64().is_some_and(|end|end>at){moloch_live_runtime::exit_refresh::ReadHint::from_live_profit(&draft,checked,at)}
    else{moloch_live_runtime::exit_refresh::ReadHint::from_expired_kind(&draft,checked,at,moloch_live_runtime::exit_refresh::ReadKind::Profit)};
   if let Err(error)=guard{return refresh_deferred(c,"replan_ineligible",error.as_ref())}
   if checked.owned_quantity.checked_sub(checked.reserved_sell_quantity).is_none_or(|n|draft["quantity"].as_u64().is_none_or(|q|q>n))
    ||checked.native_head_fingerprint!=head||native_head(c)?!=head{return refresh_deferred(c,"replan_ineligible",&"EXIT_REFRESH_CURRENT_COVERAGE_OR_HEAD_CHANGED")}
   let planning_started=now();
   // Existing durable zero-network proof + native normal reconciliation only.
   // This never cancels/replaces a physical order or extends the old deadline.
   retire_never_sent(bank,log,&draft)?;
   let planning_completed=now();
   let planning=match PostBookPlanning::from_session(&bound,&acceptance,bank,planning_started,planning_completed,native_head(c)?){
    Ok(v)=>v,Err(error)=>return refresh_deferred(c,"post_book_planning_ineligible",error.as_ref())
   };
   let current=bank.engine().order_records();let mut checked_candidates=std::collections::BTreeMap::new();
   for q in &current{if q["id"].as_u64().is_some_and(|id|id>maximum_qid_before)&&q["exchangeId"]==hint.exchange()&&q["side"]==hint.side()&&q["action"]=="sell"&&q["retired"]==false{
    checked_candidates.insert(q["id"].as_u64().ok_or("LIVE_QUOTE_ID")?,refresh_safety(c,bank,log,authority,stream,q,unknown)?);
   }}
   let plan=match RefreshPlan::select_after_planning(&bound,&planning,bank,maximum_qid_before,now(),|id|*checked_candidates.get(&id).expect("validated post-book candidate safety")){
    Ok(v)=>v,Err(error)=>return refresh_deferred(c,"post_book_candidate_ineligible",error.as_ref())
   };
   atomic_json(&c.root.join("exit-refresh-replan.json"),&json!({"at":iso(now()),"exchangeId":hint.exchange(),"side":hint.side(),"retiredUnusedQid":draft["id"],"newNativeQid":plan.quote()["id"],"planningStartedAt":iso(planning_started),"planningCompletedAt":iso(planning_completed),"originalBookReceivedAt":facts.book.received_at,"nativeHead":bank.durable_head(),"physicalAttempted":false,"expiryExtended":false}))?;
   plan
  },
  Err(e)=>{refresh_deferred(c,"candidate_ineligible",e.as_ref())?;return Ok(())}
 };
 let qid=plan.quote()["id"].as_u64().ok_or("LIVE_QUOTE_ID")?;
 loop{
  let current=quote(bank,qid)?;
  if hint.kind()==moloch_live_runtime::exit_refresh::ReadKind::Profit&&!moloch_live_runtime::exit_refresh::current_profit_qualified(bank,&current)?{refresh_deferred(c,"profit_ineligible",&"EXIT_REFRESH_CURRENT_GAIN_REQUIRED")?;return Ok(())}
  let current_safety=refresh_safety(c,bank,log,authority,stream,&current,unknown)?;
  match plan.stage(&current,&bound,&c.scope,current_safety,now()){
   Ok(RefreshStage::WaitUntil(at))=>{std::thread::sleep(Duration::from_millis(at.saturating_sub(now()).clamp(1,100)));}
   Ok(RefreshStage::Ready)=>break,
   Err(e)=>{refresh_deferred(c,"wait_ineligible",e.as_ref())?;return Ok(())}
  }
 }
 // Query the real shared queue once after waiting, never synthesize empty unknowns.
 let unknown=queue_has_unknown(t)?;let current=quote(bank,qid)?;
 if hint.kind()==moloch_live_runtime::exit_refresh::ReadKind::Profit&&!moloch_live_runtime::exit_refresh::current_profit_qualified(bank,&current)?{refresh_deferred(c,"profit_ineligible",&"EXIT_REFRESH_CURRENT_GAIN_REQUIRED")?;return Ok(())}
 match plan.stage(&current,&bound,&c.scope,refresh_safety(c,bank,log,authority,stream,&current,unknown)?,now()){
  Ok(RefreshStage::Ready)=>{},Ok(_)=>{refresh_deferred(c,"final_ineligible",&"ACTIVATION_NOT_READY")?;return Ok(())},Err(e)=>{refresh_deferred(c,"final_ineligible",e.as_ref())?;return Ok(())}
 }
 // Rebuild complete physical identity from this NEW frozen quote only after wait.
 submit_order(t,c,bank,log,authority,plan.quote(),facts,None,fundamental)
}
fn profit_hint_scope_allowed(q:&Value,managed:&std::collections::BTreeSet<String>,protected:&std::collections::BTreeSet<String>)->bool{
 // This extra conservative exclusion is profit-only; preserve the original
 // age path's separate native/package/review authority checks.
 if q["inventoryExitReason"]=="maximumAge"{return true}
 let Some(ex)=q["exchangeId"].as_str()else{return false};
 !protected.contains(ex)&&!managed.contains(ex)
}
fn profit_refresh_protected(scope:&Scope,packages:&Value)->Result<std::collections::BTreeSet<String>>{
 let mut protected=std::collections::BTreeSet::new();
 for p in packages.as_array().ok_or("LIVE_EXIT_REFRESH_PACKAGES")?{
  if p["phase"]=="no_execution"||p["protectionReleased"]==true{continue}
  // Public probabilistic summaries omit some non-wire held components. No
  // profit nomination or priority may infer those components are unprotected.
  if p.get("probabilistic").is_some_and(|v|!v.is_null()){protected.extend(scope.exchanges.keys().cloned());}
  if let Some(legs)=p["legs"].as_array(){for leg in legs{if let Some(ex)=leg["exchangeId"].as_str(){protected.insert(ex.to_owned());}}}
  if let Some(xs)=p["portfolio"]["componentExchanges"].as_array(){for ex in xs{if let Some(ex)=ex.as_str(){protected.insert(ex.to_owned());}}}
 }Ok(protected)
}
/// This new timing lane is restricted to the already adopted, unclocked lots.
/// Reviewed holdings and executed new-entry cohorts keep their own objectives.
fn unused_inherited_profit_draft(bank:&Session,log:&DispatchLog,scope:&Scope,q:&Value)->Result<bool>{
 use moloch_live_runtime::exit_refresh::ReadKind;
 if ReadKind::from_quote(q)!=Some(ReadKind::Profit)||q["retired"]!=false||q["filled"].as_u64()!=Some(0)
  ||q.get("cancelEffectiveAt").is_some_and(|v|!v.is_null())
  ||q.get("reviewedInventoryExit").is_some_and(|v|!v.is_null())||q.get("reviewedFundamentalEntry").is_some_and(|v|!v.is_null()){return Ok(false)}
 let id=q["id"].as_u64().ok_or("LIVE_QUOTE_ID")?;let ex=q["exchangeId"].as_str().ok_or("LIVE_SCOPE")?;let side=q["side"].as_str().ok_or("LIVE_SIDE")?;
 if scope.exchanges.get(ex).map(String::as_str)!=q["marketId"].as_str()||!log.never_attempted(id)||log.intents.contains_key(&id)
  ||log.acknowledgments.contains_key(&id)||bank.engine().is_package_order(id)||bank.engine().live_order_bindings().contains_key(&id)
  ||bank.engine().fills().iter().any(|f|f["orderId"]==id)
  ||bank.engine().ordinary_entry_exclusions().contains(ex)||bank.engine().managed_inventory_planning_excludes(ex)
  ||!profit_hint_scope_allowed(q,&Default::default(),&profit_refresh_protected(scope,&bank.engine().package_records())?){return Ok(false)}
 let positions=bank.engine().wallet.positions();let Some(lots)=positions.get(ex).and_then(|p|p.get(side)).and_then(Value::as_array)else{return Ok(false)};
 if lots.is_empty()||lots.iter().any(|l|l["quantity"].as_u64().is_none_or(|n|n==0)||l.get("executedAt").is_some_and(|v|!v.is_null())){return Ok(false)}
 let inherited=bank.engine().report()?["inheritedInventory"].clone();
 Ok(inherited["basis"]=="verified_open_lot_principal_fifo"&&inherited["adoptedExchangeIds"].as_array().is_some_and(|ids|ids.iter().any(|id|id==ex)))
}
/// Select at most one CURRENT unused draft. Never retire a physical order,
/// intent, partial fill or sibling just to force a successor into existence.
fn profit_replan_draft(bank:&Session,log:&DispatchLog,scope:&Scope,hint:&moloch_live_runtime::exit_refresh::ReadHint,cutoff:u64,flow_started:u64,at:u64)->Result<Option<Value>>{
 use moloch_live_runtime::exit_refresh::{ReadKind,current_profit_qualified};
 if hint.kind()!=ReadKind::Profit{return Ok(None)}
 log.require_resolved_attempts()?;let mut matching=Vec::new();
 for q in bank.engine().order_records(){
  if q["retired"]==false&&q["marketId"]==hint.market()&&q["exchangeId"]==hint.exchange()&&q["action"]=="sell"{
   // A different side/kind or occupied sibling is a reason to defer, not clean up.
   if q["side"]!=hint.side()||!unused_inherited_profit_draft(bank,log,scope,&q)?{return Ok(None)}
   matching.push(q);
  }
 }
 if matching.len()!=1{return Ok(None)}let q=matching.remove(0);
 let id=q["id"].as_u64().ok_or("LIVE_QUOTE_ID")?;let created=q["createdAt"].as_u64().ok_or("LIVE_QUOTE_CLOCK")?;
 let short=q["expiresAt"].as_u64().ok_or("LIVE_EXPIRY")?<=at.saturating_add(3000);
 if !(id<=cutoff||created<flow_started||short)||!current_profit_qualified(bank,&q)?{return Ok(None)}
 Ok(Some(q))
}
fn latest_refresh_hints(orders:&[Value],at:u64,eligible:impl Fn(u64)->bool)->Result<std::collections::BTreeMap<String,Value>>{
 let mut latest=std::collections::BTreeMap::<String,Value>::new();
 for q in orders{
  let kind=moloch_live_runtime::exit_refresh::ReadKind::from_quote(q);
  let live_profit=kind==Some(moloch_live_runtime::exit_refresh::ReadKind::Profit)&&q["retired"]==false;
  if kind.is_none()||q["filled"].as_u64()!=Some(0)||q.get("cancelEffectiveAt").is_some_and(|v|!v.is_null())||q["expiresAt"].as_u64().is_none_or(|t|t>at&&!live_profit){continue}
  let id=q["id"].as_u64().ok_or("LIVE_QUOTE_ID")?;if !eligible(id){continue}
  let mut key=format!("{}:{}:sell",q["exchangeId"].as_str().ok_or("LIVE_SCOPE")?,q["side"].as_str().ok_or("LIVE_SIDE")?);
  // Distinct historical kinds must not hide one another before current age checks.
  if moloch_live_runtime::exit_refresh::ReadKind::from_quote(q)==Some(moloch_live_runtime::exit_refresh::ReadKind::Profit){key.push_str(":profit");}
  if latest.get(&key).is_none_or(|old|old["id"].as_u64().unwrap()<id){latest.insert(key,q.clone());}
 }Ok(latest)
}
/// A historical maximumAge label is only a read hint. Recompute its deadline
/// from CURRENT remaining FIFO lots; never restart a timer from observation time.
fn remaining_exit_age_due(policy:&Value,positions:&Value,inherited:&Value,ex:&str,side:&str,at:u64)->Result<bool>{
 if policy["inventoryAgeClock"]!="execution"{return Ok(true)} // Unsupported clocks retain conservative priority.
 let Some(age)=policy["maximumInventoryAgeMs"].as_u64()else{return Ok(true)};
 let lots=positions.get(ex).and_then(|v|v.get(side)).and_then(Value::as_array).ok_or("LIVE_EXIT_AGE_LOTS")?;
 let has_unclocked=lots.iter().any(|l|l.get("executedAt").is_none_or(Value::is_null));
 let adopted=if has_unclocked{
  // Only positive native inherited provenance permits the opening-lot exception.
  if inherited["basis"]!="verified_open_lot_principal_fifo"||!inherited["adoptedExchangeIds"].as_array().is_some_and(|a|a.iter().any(|v|v==ex)){return Ok(true)}
  Some(moloch_live_runtime::daily_windows::clock(inherited["managementStartedAt"].as_str().ok_or("LIVE_EXIT_AGE_ADOPTION_CLOCK")?)?)
 }else{None};
 let mut oldest=None;
 for l in lots{
  if l["quantity"].as_u64().is_none_or(|n|n==0){return Err("LIVE_EXIT_AGE_LOT_QUANTITY".into())}
  if let Some(clock)=l.get("executedAt").filter(|v|!v.is_null()){
   let t=moloch_live_runtime::daily_windows::clock(clock.as_str().ok_or("LIVE_EXIT_AGE_EXECUTION_CLOCK")?)?;
   if t>at{return Err("LIVE_EXIT_AGE_FUTURE_EXECUTION".into())}
   if adopted.is_none_or(|a|t>=a){oldest=Some(oldest.map_or(t,|old:u64|old.min(t)));}
  }
 }
 match oldest{Some(t)=>Ok(t.checked_add(age).ok_or("LIVE_EXIT_AGE_CLOCK_RANGE")?<=at),None=>Ok(false)}
}
/// Selection only. Retired history or a forecast Hold cannot justify a queue
/// status write/full lifecycle fingerprint. Surviving candidates still require
/// the ORIGINAL current queue observation, complete Safety and typed ReadHint.
fn prepare_expired_exit_hints(
 orders:&[Value],at:u64,eligible:impl Fn(u64)->bool,
 mut owned:impl FnMut(&str,&str)->Result<u64>,mut policy_allows:impl FnMut(&str)->Result<bool>,
 mut age_due:impl FnMut(&str,&str)->Result<bool>,
 mut queue_unknown:impl FnMut()->Result<bool>,
 mut safety:impl FnMut(&Value,bool)->Result<moloch_live_runtime::exit_refresh::Safety>,
)->Result<(std::collections::BTreeMap<String,(Value,moloch_live_runtime::exit_refresh::ReadHint)>,bool)>{
 let latest=latest_refresh_hints(orders,at,eligible)?;let mut candidates=Vec::new();let mut hints=std::collections::BTreeMap::new();
 if latest.is_empty(){return Ok((hints,false))}
 let mut live_sells=std::collections::BTreeMap::<String,Vec<u64>>::new();
 for q in orders.iter().filter(|q|q["action"]=="sell"&&q["retired"]==false&&q["expiresAt"].as_u64().is_some_and(|t|t>at)){
  let key=format!("{}:{}:sell",q["exchangeId"].as_str().ok_or("LIVE_SCOPE")?,q["side"].as_str().ok_or("LIVE_SIDE")?);
  live_sells.entry(key).or_default().push(q["id"].as_u64().ok_or("LIVE_QUOTE_ID")?);
 }
 for(key,old)in latest{
  let ex=old["exchangeId"].as_str().ok_or("LIVE_QUOTE_SCOPE")?;let side=old["side"].as_str().ok_or("LIVE_SIDE")?;
  let other_live=live_sells.get(&format!("{ex}:{side}:sell")).is_some_and(|ids|ids.iter().any(|id|Some(*id)!=old["id"].as_u64()));
  if owned(ex,side)?==0||other_live||!policy_allows(ex)?||(old["inventoryExitReason"]=="maximumAge"&&!age_due(ex,side)?){continue}candidates.push((key,old));
 }
 if candidates.is_empty(){return Ok((hints,false))}
 let unknown=queue_unknown()?;
 for(key,old)in candidates{let checked=safety(&old,unknown)?;
  let hint=if old["expiresAt"].as_u64().is_some_and(|end|end>at){moloch_live_runtime::exit_refresh::ReadHint::from_live_profit(&old,checked,at)}
   else{moloch_live_runtime::exit_refresh::ReadHint::from_expired_kind(&old,checked,at,moloch_live_runtime::exit_refresh::ReadKind::from_quote(&old).ok_or("LIVE_EXIT_REFRESH_KIND")?)};
  if let Ok(hint)=hint{hints.insert(key,(old,hint));}
 }
 Ok((hints,unknown))
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
fn passive_reserved_preflight(c:&Control,bank:&mut Session,log:&DispatchLog,source:&mut RestSource,f:&FundamentalRuntime,original:&Value,reads:&mut impl OrdinaryPreflightReads)->Result<Option<(SnapshotSet,u64)>>{
 let ex=original["exchangeId"].as_str().ok_or("LIVE_QUOTE_SCOPE")?;
 if passive_reserved_exit(bank,log,&c.scope,ex,reads.now_ms())?.as_ref()!=Some(original){return Ok(None)}
 f.authority.validate()?;if !reads.guarded()?{return Ok(None)}
 let market=original["marketId"].as_str().ok_or("LIVE_MARKET_ID")?;
 let facts=match preflight_snapshots(c,bank,source,market,Some(original),reads){Ok(facts)=>facts,Err(e)if e.is::<PreflightQuoteInvalidated>()=>return Ok(None),Err(e)=>return Err(e)};
 let Some(until)=existing_facts_until(&facts,reads.now_ms())?else{return Ok(None)};
 accept_fresh_book(bank,c,original,&facts.book,source)?;
 if passive_reserved_exit(bank,log,&c.scope,ex,reads.now_ms())?.as_ref()!=Some(original){return Ok(None)}
 // A durable Cohorts binding made before a deferred intent is permitted only
 // through the original current-review/ownership gate and exact later bind.
 if !f.eligible(bank,log,&c.scope,original["id"].as_u64().ok_or("LIVE_QUOTE_ID")?,reads.now_ms())?{return Ok(None)}
 Ok(Some((facts,until)))
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
fn reviewed_existing_exit(t:&mut Transport,c:&Control,bank:&mut Session,log:&mut DispatchLog,authority:&Arc<ProductionAuthority>,source:&mut RestSource,stream:&Realtime,f:&mut FundamentalRuntime,hint:&fundamental::ExistingExitHint,prior_window:u64,breadth:&mut Option<Recorder>,universe:&mut Option<Universe>)->Result<()>{
 let ex=hint.exchange_id.as_str();let market=hint.market_id.as_str();refresh_controls(c,authority,stream)?;log.require_resolved_attempts()?;f.authority.validate()?;
 let retry=passive_reserved_exit(bank,log,&c.scope,ex,now())?;
 if retry.is_none()&&existing_target_busy(bank,log,ex)?{return Ok(())}
 // A REST book cannot heal an unobserved tape interval by itself.
 if let Some(from)=exit_recovery_from(source,market,prior_window){
  let through=now();let tape=request(t,&c.root,Endpoint::Trades{exchange_id:ex.into(),from:iso(from),to:iso(through)})?;
  let wrapped=json!({"source":"official_sig_api_v1","kind":"trades","tournamentId":c.scope.tournament_id,"marketId":market,"exchangeId":ex,"endpoint":tape.endpoint,"status":tape.status,"requestStartedAt":tape.request_started_at,"receivedAt":tape.received_at,"data":tape.data});
  accept_source_events(bank,&source.accept(&wrapped)?)?;
  let scopes=source.scopes();if !scopes.as_array().ok_or("LIVE_SOURCE_SCOPES")?.iter().find(|s|s["marketId"]==market).ok_or("LIVE_SOURCE_SCOPE")?["tapeReady"].as_bool().ok_or("LIVE_SOURCE_TAPE_STATE")?{return existing_exit_deferred(c,ex,"coverage","COMPLETE_TARGET_TAPE_REQUIRED")}
 }
 refresh_controls(c,authority,stream)?;
 if let Some(original)=retry{
  let Some((facts,until))=passive_reserved_preflight(c,bank,log,source,f,&original,&mut LivePreflightReads{t,c,authority,stream,breadth,universe})?else{return existing_exit_deferred(c,ex,"reserved_preflight","CURRENT_PASSIVE_RESERVATION_OR_PACKET_INELIGIBLE")};
  return submit_existing_reservation(t,c,bank,log,authority,stream,f,&original,facts,until)
 }
 if hint.reason=="reviewed_inventory_book_refresh"{
  let target=json!({"marketId":market,"exchangeId":ex});
  let eligible=reviewed_book_prefilter(c,bank,source,&target,&mut LivePreflightReads{t,c,authority,stream,breadth,universe},|bank,at|{
   Ok(f.cohorts.existing_exit_hint(bank,log,&c.scope,ex,f.authority.trusted(),at)?.is_some())
  })?;
  if !eligible{return existing_exit_deferred(c,ex,"book_prefilter","NO_CURRENT_EXECUTABLE_REVIEWED_EXIT")}
 }
 let facts=preflight_snapshots(c,bank,source,market,None,&mut LivePreflightReads{t,c,authority,stream,breadth,universe})?;
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
 let Some(original)=commit_existing_reservation(bank,plan.event())?else{return existing_exit_deferred(c,ex,"native","ORIGINAL_NATIVE_BUDGET_OR_WINDOW_DENIED")};
 submit_existing_reservation(t,c,bank,log,authority,stream,f,&original,facts,facts_until)
}
fn submit_existing_reservation(t:&mut Transport,c:&Control,bank:&mut Session,log:&mut DispatchLog,authority:&Arc<ProductionAuthority>,stream:&Realtime,f:&mut FundamentalRuntime,original:&Value,facts:SnapshotSet,facts_until:u64)->Result<()>{
 let ex=original["exchangeId"].as_str().ok_or("LIVE_QUOTE_SCOPE")?;let qid=original["id"].as_u64().ok_or("LIVE_QUOTE_ID")?;
 loop{refresh_controls(c,authority,stream)?;f.authority.validate()?;
  let current=quote(bank,qid)?;match existing_reservation_stage(&current,original,now(),facts_until){
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
 matches!(reason,"FUNDAMENTAL_ENTRY_RECEIPT_SCOPE_OR_STALE"|"FUNDAMENTAL_ENTRY_RECEIPT_ORDER"|"FUNDAMENTAL_ENTRY_NOT_FLAT"|"FUNDAMENTAL_ENTRY_PHYSICAL_LIABILITY"|"FUNDAMENTAL_ENTRY_CASH_RECONCILIATION"|"FUNDAMENTAL_ENTRY_MARKET"|"FUNDAMENTAL_REVIEWED_CONTRACT_CHANGED"|"FUNDAMENTAL_ENTRY_BOOK_NOT_ADMITTED"|"FUNDAMENTAL_ENTRY_NATIVE_BOOK"|"FUNDAMENTAL_ENTRY_PLAN_CHANGED"|"REVIEWED_ENTRY_DEADLINE"|"REVIEWED_ENTRY_EXISTING_EXPOSURE"|"REVIEWED_ENTRY_REVIEW_EXPIRED"|"REVIEWED_ENTRY_CLOCK"|"REVIEWED_ENTRY_BOOK"|"REVIEWED_ENTRY_PASSIVE_PRICE"|"REVIEWED_ENTRY_IMPROVE_PRICE"|"REVIEWED_ENTRY_BOUNDED_PRICE"|"REVIEWED_ENTRY_FUNDING"|"REVIEWED_ENTRY_DEPTH"|"REVIEWED_ENTRY_MARKET_CAP"|"REVIEWED_ENTRY_FRESH_MARKET_CAP"|"REVIEWED_ENTRY_WRITE_BUDGET")
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
#[cfg(test)]fn fundamental_read_rotation(existing:&[fundamental::ExistingExitHint],entries:&[fundamental::EntryHint],cursor:Option<&str>)->Option<(bool,usize,String)>{
 let mut choices:Vec<_>=existing.iter().enumerate().map(|(i,h)|(true,i,format!("{}:{}:sell",h.exchange_id,h.side))).chain(entries.iter().enumerate().map(|(i,h)|(false,i,format!("{}:{}:buy",h.exchange_id,h.side)))).collect();
 choices.sort_by(|a,b|a.2.cmp(&b.2));let keys:Vec<_>=choices.iter().map(|c|c.2.clone()).collect();moloch_live_runtime::scheduling::preflight_rotation(&keys,cursor).first().map(|index|choices[*index].clone())
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum SharedPreflight{ExistingRead(usize),EntryRead(usize),ProfitRead(usize),Physical(usize)}
/// Read hints cannot permanently consume the sole expensive packet. Use ONE
/// stable market/side/action cursor for the union, including unqualified hints.
/// A real ready quote supersedes a same-key hint; only evidenced owned exits
/// preempt this fair union. `physical` is already native-priority/value filtered.
#[cfg(test)]fn shared_preflight_rotation(existing:&[fundamental::ExistingExitHint],entries:&[fundamental::EntryHint],physical:&[(String,usize,bool)],cursor:Option<&str>)->Option<(SharedPreflight,String)>{
 shared_preflight_with_profit(existing,entries,&[],physical,&Default::default(),cursor)
}
fn shared_preflight_with_profit(existing:&[fundamental::ExistingExitHint],entries:&[fundamental::EntryHint],profit_keys:&[String],physical:&[(String,usize,bool)],covered:&std::collections::BTreeSet<usize>,cursor:Option<&str>)->Option<(SharedPreflight,String)>{
 if let Some((key,index,_))=physical.iter().find(|(_,_,urgent)|*urgent){return Some((SharedPreflight::Physical(*index),key.clone()))}
 if let Some((key,index,_))=physical.iter().find(|(_,index,_)|covered.contains(index)){return Some((SharedPreflight::Physical(*index),key.clone()))}
 let mut choices=std::collections::BTreeMap::new();
 for(i,h)in existing.iter().enumerate(){choices.insert(format!("{}:{}:sell",h.exchange_id,h.side),SharedPreflight::ExistingRead(i));}
 for(i,h)in entries.iter().enumerate(){choices.insert(format!("{}:{}:buy",h.exchange_id,h.side),SharedPreflight::EntryRead(i));}
 for(i,key)in profit_keys.iter().enumerate(){choices.insert(key.clone(),SharedPreflight::ProfitRead(i));}
 let mut seen=std::collections::BTreeSet::new();for(key,index,_)in physical{if seen.insert(key){choices.insert(key.clone(),SharedPreflight::Physical(*index));}}
 let keys:Vec<_>=choices.keys().cloned().collect();let index=*moloch_live_runtime::scheduling::preflight_rotation(&keys,cursor).first()?;let key=keys[index].clone();Some((choices[&key],key))
}
/// Selection only: the caller supplies an ACTUAL current reviewed Exit decision,
/// or the current remaining-FIFO deadline. Style and marketability grant no urgency.
fn physical_exit_urgent(q:&Value,current_age_due:bool,reviewed_exit:bool)->bool{
 q["action"]=="sell"&&(reviewed_exit||(q["inventoryExitReason"]=="maximumAge"&&current_age_due))
}
fn physical_preflight_ready(q:&Value,never_attempted:bool,has_intent:bool,at:u64)->bool{
 let early=moloch_live_runtime::dispatch_window::may_prefetch(q,never_attempted,has_intent,at);
 q["retired"]==false&&never_attempted&&(early||q["activationAt"].as_u64().is_some_and(|v|v<=at))&&q["expiresAt"].as_u64().is_some_and(|v|v>at.saturating_add(5000))&&!q["cancelEffectiveAt"].is_u64()
}
trait OrdinaryPreflightReads{
 fn now_ms(&self)->u64;
 fn guarded(&mut self)->Result<bool>;
 fn read(&mut self,endpoint:Endpoint)->Result<Receipt>;
 fn read_serviced(&mut self,_bank:&mut Session,_source:&mut RestSource,endpoint:Endpoint)->Result<Receipt>{self.read(endpoint)}
  fn service_before_read(&mut self,_bank:&mut Session,_source:&mut RestSource)->Result<()>{Ok(())}
}
// Only this private typed error is an expected quote invalidation. A stream,
// STOP, SDK or durable failure with similar text must not be swallowed.
#[derive(Debug)]struct PreflightQuoteInvalidated;
impl std::fmt::Display for PreflightQuoteInvalidated{fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result{f.write_str("LIVE_PREFLIGHT_QUOTE_INVALIDATED")}}
impl std::error::Error for PreflightQuoteInvalidated{}
fn preflight_read(bank:&mut Session,source:&mut RestSource,reads:&mut impl OrdinaryPreflightReads,expected:Option<&Value>,endpoint:Endpoint)->Result<Receipt>{
 reads.service_before_read(bank,source)?;
 if let Some(original)=expected{
  let id=original["id"].as_u64().ok_or("LIVE_QUOTE_ID")?;let current=quote(bank,id)?;
  if current!=*original||current["retired"]!=false||current.get("cancelEffectiveAt").is_some_and(|v|!v.is_null())||current["expiresAt"].as_u64().is_none_or(|end|end<=reads.now_ms()){return Err(PreflightQuoteInvalidated.into())}
 }
 // There is deliberately no callback after the returned receipt. Its original
 // identity/clocks reach the existing validation/application unchanged.
 reads.read_serviced(bank,source,endpoint)
}
fn preflight_snapshots(c:&Control,bank:&mut Session,source:&mut RestSource,market:&str,expected:Option<&Value>,reads:&mut impl OrdinaryPreflightReads)->Result<SnapshotSet>{
 snapshots_for_market(c,market,|ep|preflight_read(bank,source,reads,expected,ep))
}
fn bulk_ordinary_quote(q:&Value,managed:&std::collections::BTreeSet<String>)->bool{
 q["exchangeId"].as_str().is_some_and(|ex|!managed.contains(ex))
  &&q.get("reviewedInventoryExit").is_none_or(Value::is_null)&&q.get("reviewedFundamentalEntry").is_none_or(Value::is_null)
  &&q["executionStyle"]!="inventoryAON"&&q["inventoryExitReason"]!="maximumAge"
}
/// One optional negative-only request per dispatch pass. Only a chosen stale
/// ordinary quote plus at least one OTHER eligible scope justifies this read.
/// Positive/unknown/expired observations still need every original book/fact.
fn bulk_negative_vetoes(c:&Control,quotes:&[Value],physical:&[(String,usize,bool)],selection:&moloch_live_runtime::quote_priority::Selection,managed:&std::collections::BTreeSet<String>,chosen:usize,reads:&mut impl OrdinaryPreflightReads)->Result<Vec<(usize,moloch_live_runtime::bulk_price_screen::Deferral)>>{
 use moloch_live_runtime::{bulk_price_screen as bulk,quote_priority::Class};
 if !c.bulk_negative_screen{return Ok(Vec::new())}
 let eligible=|index:usize,urgent:bool|->Result<bool>{let q=quotes.get(index).ok_or("LIVE_BULK_QUOTE_INDEX")?;
  let a=selection.assessments.iter().find(|a|a.index==index).ok_or("LIVE_BULK_ASSESSMENT")?;
  Ok(!urgent&&a.decision.class==Class::NeedsRefresh&&bulk_ordinary_quote(q,managed))};
 let Some((_,_,urgent))=physical.iter().find(|(_,i,_)|*i==chosen)else{return Err("LIVE_BULK_CHOICE".into())};
 if !eligible(chosen,*urgent)?{return Ok(Vec::new())}
 let mut indices=vec![chosen];for(_,index,urgent)in physical{if *index!=chosen&&eligible(*index,*urgent)?{indices.push(*index)}}
 let mut ids=Vec::new();let mut seen=std::collections::BTreeSet::new();let mut included=Vec::new();
 for index in indices{let q=&quotes[index];let ex=q["exchangeId"].as_str().ok_or("LIVE_BULK_SCOPE")?;
  if c.scope.exchanges.get(ex).is_none_or(|m|q["marketId"]!=*m){return Err("LIVE_BULK_SCOPE".into())}
  if seen.contains(ex){included.push(index);continue}
  if ids.len()==bulk::MAXIMUM_IDS{continue}seen.insert(ex.to_owned());ids.push(ex.to_owned());included.push(index);
 }
 if ids.len()<2{return Ok(Vec::new())}
 let endpoint=bulk::endpoint(&c.scope,&ids)?;
 if !reads.guarded()?{return Ok(Vec::new())}
 let receipt=reads.read(endpoint)?;let parsed=bulk::Batch::from_original(&c.scope,&ids,&receipt,reads.now_ms());
 let batch=match parsed{Ok(batch)=>batch,Err(error)=>{
  atomic_json(&c.root.join("bulk-negative-screen.json"),&json!({"at":iso(reads.now_ms()),"state":"unavailable_original_response","reason":error.to_string(),"requestedExchangeCount":ids.len(),"negativeOnly":true,"executionAuthority":false}))?;return Ok(Vec::new())
 }};
 if !reads.guarded()?{return Ok(Vec::new())}
 let mut vetoes=Vec::new();for index in included{if let Some(veto)=batch.defer(&c.scope,&quotes[index],reads.now_ms())?{
  if veto.matches_quote(&c.scope,&quotes[index],reads.now_ms())?{vetoes.push((index,veto));}
 }}
 atomic_json(&c.root.join("bulk-negative-screen.json"),&json!({"at":iso(reads.now_ms()),"state":"original_negative_screen","receiptSha256":batch.receipt_sha256(),"validUntilMs":batch.valid_until_ms(),"requestedExchangeCount":ids.len(),"vetoedQids":vetoes.iter().map(|(i,_)|quotes[*i]["id"].clone()).collect::<Vec<_>>(),"negativeOnly":true,"executionAuthority":false,"pricesChanged":false,"expiryExtended":false}))?;
 Ok(vetoes)
}
/// Recheck original quote binding and receipt TTL at the actual rotation boundary.
fn apply_bulk_vetoes(scope:&Scope,quotes:&[Value],physical:&mut Vec<(String,usize,bool)>,vetoes:&[(usize,moloch_live_runtime::bulk_price_screen::Deferral)],chosen:usize,at:u64)->Result<bool>{
 let mut kept=Vec::new();let mut chosen_deferred=false;
 for item in physical.iter(){let q=quotes.get(item.1).ok_or("LIVE_BULK_QUOTE_INDEX")?;
  let vetoed=if item.2{false}else{match vetoes.iter().find(|(i,_)|*i==item.1){Some((_,v))=>v.matches_quote(scope,q,at)?,None=>false}};
  if vetoed{chosen_deferred|=item.1==chosen;}else{kept.push(item.clone());}
 }*physical=kept;Ok(chosen_deferred)
}
struct LivePreflightReads<'a>{t:&'a mut Transport,c:&'a Control,authority:&'a ProductionAuthority,stream:&'a Realtime,breadth:&'a mut Option<Recorder>,universe:&'a mut Option<Universe>}
impl OrdinaryPreflightReads for LivePreflightReads<'_>{
 fn now_ms(&self)->u64{now()}
 fn guarded(&mut self)->Result<bool>{refresh_controls(self.c,self.authority,self.stream)?;Ok(!queue_has_unknown(self.t)?)}
 fn read(&mut self,ep:Endpoint)->Result<Receipt>{request(self.t,&self.c.root,ep)}
 fn read_serviced(&mut self,bank:&mut Session,source:&mut RestSource,ep:Endpoint)->Result<Receipt>{
  let Self{t,c,authority,stream,breadth,universe}=self;
  let mut service=||{refresh_controls(c,authority,stream)?;drain_stream(stream,bank,source,breadth,universe)?;refresh_controls(c,authority,stream)};
  for attempt in 0..3u64{
   // One exclusive SDK GET; source admission stays on this Session thread.
   // Outer service errors are fatal and never enter SDK retry classification.
   match moloch_live_runtime::serviced_read::request(t,ep.clone(),&mut service)?{
    Ok(receipt)=>{record_receipt(&c.root,&receipt)?;return Ok(receipt)},
    Err(error)=>{
     if attempt==2||!moloch_live_runtime::read_recovery::retryable_read(&error){return Err(error.into())}
     atomic_json(&c.root.join("read-recovery.json"),&json!({"at":iso(now()),"endpoint":ep,"attempt":attempt+1,"code":error.code,"status":error.status,"retryAfterMs":error.retry_after_ms,"ordersRetried":false}))?;
     let until=now().checked_add(2000*(attempt+1)).ok_or("LIVE_SERVICED_READ_CLOCK")?;
     while now()<until{service()?;std::thread::sleep(Duration::from_millis(until.saturating_sub(now()).clamp(1,25)));}
    }
   }
  }Err("LIVE_READ_RETRY_EXHAUSTED".into())
 }
  fn service_before_read(&mut self,bank:&mut Session,source:&mut RestSource)->Result<()>{
   refresh_controls(self.c,self.authority,self.stream)?;
   drain_stream(self.stream,bank,source,self.breadth,self.universe)?;
   refresh_controls(self.c,self.authority,self.stream)
  }
}
/// The callback evaluates the genuine admitted book and pinned policy. It may
/// retire proved-unused local drafts before comparing actual available cash;
/// it never replaces the independent physical packet or grants a submission.
fn reviewed_book_prefilter(c:&Control,bank:&mut Session,source:&mut RestSource,target:&Value,reads:&mut impl OrdinaryPreflightReads,mut eligible:impl FnMut(&mut Session,u64)->Result<bool>)->Result<bool>{
 if !reads.guarded()?{return Ok(false)}
 let book=preflight_read(bank,source,reads,None,Endpoint::OrderBook{market_id:target["marketId"].as_str().ok_or("LIVE_MARKET_ID")?.into(),depth:20})?;
 accept_fresh_book(bank,c,target,&book,source)?;
 eligible(bank,reads.now_ms())
}
// Additional reviewed runtime selection only. A shadow/logging failure cannot
// change the old dispatch flow; Enforce still defers if evidence is unavailable.
fn covered_opposite_buy(bank:&Session,log:&DispatchLog,q:&Value)->bool{
 if q["action"]!="buy"{return false}
 let opposite=match q["side"].as_str(){Some("YES")=>Some(moloch_replay::Side::No),Some("NO")=>Some(moloch_replay::Side::Yes),_=>None};
 if let(Some(side),Some(ex))=(opposite,q["exchangeId"].as_str()){
  let held=bank.engine().wallet.quantity(ex,side);
  if held>0{
   let records=bank.engine().order_records();
   let unclosed=log.attempted.difference(&log.closed).filter(|id|{
    // Only the SDK-hydrated capability can clear this encumbrance. A raw
    // replayed abort is untrusted, and any actual execution evidence wins.
    !log.proved_no_network(**id)||log.acknowledgments.contains_key(*id)||bank.engine().live_order_bindings().contains_key(*id)||!records.iter().any(|o|o["id"]==**id&&o["retired"]==true&&o["filled"]==0)
   }).copied().collect();
   if moloch_live_runtime::ordinary_side_edge::fully_covered_opposite_buy(q,held,&records,&unclosed){return true}
  }
 }
 false
}
/// Whole-quantity inventory reduction only, never permission to open exposure or
/// bypass a reviewed hold/package. Existing physical/gross-funding gates remain.
fn unreviewed_inventory_reduction(c:&Control,bank:&Session,log:&DispatchLog,q:&Value)->Result<bool>{
 log.require_resolved_attempts()?;
 let id=q["id"].as_u64().ok_or("LIVE_QUOTE_ID")?;let ex=q["exchangeId"].as_str().ok_or("LIVE_QUOTE_SCOPE")?;
 if c.scope.exchanges.get(ex).is_none_or(|m|q["marketId"]!=*m){return Err("LIVE_QUOTE_SCOPE".into())}
 if q["action"]!="buy"||q["retired"]!=false||q.get("reviewedFundamentalEntry").is_some()||q.get("reviewedInventoryExit").is_some()
  ||bank.engine().ordinary_entry_exclusions().contains(ex)||bank.engine().managed_inventory_planning_excludes(ex)
  ||bank.engine().is_package_order(id)||bank.engine().package_blocks_submissions()||profit_refresh_protected(&c.scope,&bank.engine().package_records())?.contains(ex){return Ok(false)}
 if !bank.engine().order_records().iter().any(|current|current==q){return Ok(false)}
 Ok(covered_opposite_buy(bank,log,q))
}
fn ordinary_side_edge_allows(c:&Control,bank:&Session,log:&DispatchLog,q:&Value,book:&Receipt,fundamental_managed:bool,at:u64)->bool{
 use moloch_live_runtime::ordinary_side_edge::{self,Mode};
 if c.ordinary_side_edge==Mode::Disabled||fundamental_managed||q["action"]!="buy"||bank.engine().is_package_order(q["id"].as_u64().unwrap_or(0)){return true}
 if covered_opposite_buy(bank,log,q){return true}
 let assessment=ordinary_side_edge::assess_original(bank.engine().policy(),&c.scope,q,book,at);
 let deferred=c.ordinary_side_edge.defers(&assessment);
 let _=atomic_json(&c.root.join("ordinary-side-edge.json"),&json!({"at":iso(at),"qid":q["id"],"mode":c.ordinary_side_edge,"assessment":assessment,"nativeHead":bank.durable_head(),"deferred":deferred,"physicalAttempted":false,"priceChanged":false,"quantityChanged":false,"executionAuthority":false}));
 !deferred
}
/// A stale book is not evidence that a quote is competitive. Refresh only the
/// original book first; a far floor must not consume six unrelated account
/// reads. A positive result still obtains the entire independent seven-fact
/// packet. No earlier book is substituted for its final physical book receipt.
fn ordinary_preflight(c:&Control,bank:&mut Session,log:&DispatchLog,source:&mut RestSource,q:&Value,reads:&mut impl OrdinaryPreflightReads)->Result<Option<(Value,SnapshotSet)>>{
 use moloch_live_runtime::quote_priority::{self,Class};
 log.require_resolved_attempts()?;let id=q["id"].as_u64().ok_or("LIVE_QUOTE_ID")?;
 let mut current=quote(bank,id)?;
 if !physical_preflight_ready(&current,log.never_attempted(id),log.intents.contains_key(&id),reads.now_ms()){return Ok(None)}
 let initial=quote_priority::from_native(bank,&c.scope,std::slice::from_ref(&current),reads.now_ms(),None)?;
 let class=initial.assessments.first().ok_or("LIVE_QUOTE_PRIORITY_MISSING")?.decision.class;
 if matches!(class,Class::Deferred|Class::Inactive){return Ok(None)}
 if !reads.guarded()?{return Ok(None)}
 if class==Class::NeedsRefresh{
  let market=current["marketId"].as_str().ok_or("LIVE_MARKET_ID")?.to_owned();
  let book=match preflight_read(bank,source,reads,Some(&current),Endpoint::OrderBook{market_id:market,depth:20}){Ok(r)=>r,Err(e)if e.is::<PreflightQuoteInvalidated>()=>return Ok(None),Err(e)=>return Err(e)};
  accept_fresh_book(bank,c,&current,&book,source)?;current=quote(bank,id)?;
  let at=reads.now_ms();
  if !physical_preflight_ready(&current,log.never_attempted(id),log.intents.contains_key(&id),at){return Ok(None)}
  let refreshed=quote_priority::from_native(bank,&c.scope,std::slice::from_ref(&current),at,None)?;
  let class=refreshed.assessments.first().ok_or("LIVE_QUOTE_PRIORITY_MISSING")?.decision.class;
  if matches!(class,Class::NeedsRefresh|Class::Deferred|Class::Inactive){return Ok(None)}
  if !reads.guarded()?{return Ok(None)}
  // The first pass could not veto a stale book. Re-assess only after its
  // genuine refresh/source drain, using the unchanged negative-only proof.
  // Native exclusions are installed from the current manager generation;
  // reviewed/package/any opposite-owned exposure retain their bypasses.
  use moloch_live_runtime::ordinary_side_edge::{self,Mode};
  if c.ordinary_side_edge!=Mode::Disabled{
   let managed=bank.engine().ordinary_entry_exclusions();
   let assessed_at=reads.now_ms();
   let assessment=ordinary_side_edge::assess_early(bank,&c.scope,log,&managed,id,assessed_at);
   let veto=c.ordinary_side_edge==Mode::Enforce&&assessment.negative().is_some_and(|proof|proof.validate_current(bank,&c.scope,log,&managed,reads.now_ms()).is_ok());
   let _=atomic_json(&c.root.join("ordinary-side-edge-refreshed.json"),&json!({"at":iso(assessed_at),"qid":id,"mode":c.ordinary_side_edge,"class":assessment.class,"reason":&assessment.reason,"evidence":assessment.negative().map(|proof|proof.evidence()),"vetoedThisPass":veto,"accountPacketRead":false,"reservationReleased":false,"physicalAttempted":false,"executionAuthority":false}));
   if veto{return Ok(None)}
  }
 }
 let facts=match preflight_snapshots(c,bank,source,current["marketId"].as_str().ok_or("LIVE_MARKET_ID")?,Some(&current),reads){Ok(f)=>f,Err(e)if e.is::<PreflightQuoteInvalidated>()=>return Ok(None),Err(e)=>return Err(e)};
 accept_fresh_book(bank,c,&current,&facts.book,source)?;
 let final_quote=quote(bank,id)?;if final_quote!=current{return Ok(None)}
 Ok(Some((final_quote,facts)))
}
/// Post-commit activation waits on the genuine reservation, not the consumed
/// planning packet/EntryPlan. HTTP authority is minted only from FINAL fresh
/// receipts below; this helper grants neither source freshness nor dispatch.
fn reviewed_entry_activation_stage(q:&Value,original:&Value,at:u64)->Result<Option<u64>>{
 if q!=original{return Err("LIVE_REVIEWED_ENTRY_RESERVED_QUOTE_CHANGED".into())}
 if q["action"]!="buy"||!q["reviewedFundamentalEntry"].is_object()||q["retired"]!=false
  ||q.get("cancelEffectiveAt").is_some_and(|v|!v.is_null())||q["filled"].as_u64()!=Some(0)
  ||q["expiresAt"].as_u64().is_none_or(|end|at>=end)
  ||q["reviewedFundamentalEntry"]["reviewCurrentUntilMs"].as_u64().is_none_or(|end|at>=end){return Err("LIVE_REVIEWED_ENTRY_RESERVATION_EXPIRED".into())}
 let active=q["activationAt"].as_u64().ok_or("LIVE_QUOTE_ACTIVATION")?;
 Ok((at<active).then_some(active))
}
/// Only an actual native write-budget denial can request one bounded wait.
/// The subsequent loop must rebuild/validate its plan and preview again.
fn reviewed_entry_budget_wait(error:&str,already_waited:bool,at:u64,gap:u64,latency:u64,facts_until:u64)->Result<Option<u64>>{
 if error!="REVIEWED_ENTRY_WRITE_BUDGET"||already_waited{return Ok(None)}
 let ready=at.checked_add(gap).ok_or("LIVE_EXISTING_CLOCK_RANGE")?;
 Ok(ready.checked_add(latency).filter(|end|*end<facts_until).map(|_|ready))
}
fn reviewed_fundamental_entry(t:&mut Transport,c:&Control,bank:&mut Session,log:&mut DispatchLog,authority:&Arc<ProductionAuthority>,source:&mut RestSource,stream:&Realtime,f:&mut FundamentalRuntime,hint:&fundamental::EntryHint,prior_window:u64,breadth:&mut Option<Recorder>,universe:&mut Option<Universe>)->Result<()>{
 let ex=hint.exchange_id.as_str();let market=hint.market_id.as_str();refresh_controls(c,authority,stream)?;log.require_resolved_attempts()?;f.authority.validate()?;
 if existing_target_busy(bank,log,ex)?{return Ok(())}
 if let Some(from)=exit_recovery_from(source,market,prior_window){let tape=request(t,&c.root,Endpoint::Trades{exchange_id:ex.into(),from:iso(from),to:iso(now())})?;
  let wrapped=json!({"source":"official_sig_api_v1","kind":"trades","tournamentId":c.scope.tournament_id,"marketId":market,"exchangeId":ex,"endpoint":tape.endpoint,"status":tape.status,"requestStartedAt":tape.request_started_at,"receivedAt":tape.received_at,"data":tape.data});accept_source_events(bank,&source.accept(&wrapped)?)?;
  let scopes=source.scopes();if !scopes.as_array().ok_or("LIVE_SOURCE_SCOPES")?.iter().find(|s|s["marketId"]==market).ok_or("LIVE_SOURCE_SCOPE")?["tapeReady"].as_bool().ok_or("LIVE_SOURCE_TAPE_STATE")?{return fundamental_entry_deferred(c,ex,"coverage","COMPLETE_TARGET_TAPE_REQUIRED")}
 }
 let target=json!({"marketId":market,"exchangeId":ex});
 let quarantine=c.maker_policy.is_none()&&!queue_has_unknown(t)?;let managed=f.cohorts.managed_exchanges();
 let eligible=reviewed_book_prefilter(c,bank,source,&target,&mut LivePreflightReads{t,c,authority,stream,breadth,universe},|bank,at|{
  let at=if quarantine{retire_unqualified_entry_drafts(c,bank,log,&managed)?;now()}else{at};
  let(books,_)=moloch_live_runtime::package_admission::native_books(bank,&c.scope,at,bank.engine().policy()["bookAgeMs"].as_u64().ok_or("LIVE_BOOK_POLICY")?)?;
  if !books.contains_key(ex){return Ok(true)} // Unavailable/legacy comparison is not price evidence.
  Ok(f.cohorts.compare_entry_actions_with_authority(bank,&c.scope,hint,&f.authority,at)?.is_none_or(|comparison|!comparison.candidates.is_empty()))
 })?;
 if !eligible{return fundamental_entry_deferred(c,ex,"book_prefilter","NO_CURRENT_REVIEWED_VALUE_OR_DEPTH")}
 let facts=preflight_snapshots(c,bank,source,market,None,&mut LivePreflightReads{t,c,authority,stream,breadth,universe})?;let Some(facts_until)=existing_facts_until(&facts,now())?else{return fundamental_entry_deferred(c,ex,"facts","ORIGINAL_PACKET_STALE")};
 let target=json!({"marketId":market,"exchangeId":ex});accept_fresh_book(bank,c,&target,&facts.book,source)?;if existing_target_busy(bank,log,ex)?{return fundamental_entry_deferred(c,ex,"liability","EXISTING_TARGET_PENDING")}
 retire_existing_drafts(bank,log,ex)?;
 // Serviced reads can regenerate ordinary drafts. Release their ACTUAL local
 // reservations again after the final book, before the funded comparison.
 if c.maker_policy.is_none(){if queue_has_unknown(t)?{return fundamental_entry_deferred(c,ex,"queue","GLOBAL_UNKNOWN_ACCEPTANCE")}
  retire_unqualified_entry_drafts(c,bank,log,&managed)?;
 }
 let gap=bank.engine().policy()["writeGapMs"].as_u64().filter(|v|*v<=3000).ok_or("LIVE_EXISTING_NATIVE_WRITE_GAP")?;let latency=bank.engine().policy()["latencyMs"].as_u64().filter(|v|*v<=3000).ok_or("LIVE_EXISTING_NATIVE_ACTIVATION")?;
 if now().checked_add(latency).is_none_or(|end|end>=facts_until){return fundamental_entry_deferred(c,ex,"facts","INSUFFICIENT_ORIGINAL_PACKET_WINDOW")}
 refresh_controls(c,authority,stream)?;
 // Compare review-bound actions at this genuine fresh prefix. This diagnostic
 // cannot change the currently join-only reservation or physical capability.
 let comparison=match f.cohorts.compare_entry_actions_with_authority(bank,&c.scope,hint,&f.authority,now()){
  Ok(v)=>json!({"comparison":v,"executionAuthority":false,"activeExecutionMode":"explicit_review_policy_or_legacy_join"}),
  Err(e)=>json!({"state":"unavailable","reason":e.to_string(),"executionAuthority":false,"activeExecutionMode":"explicit_review_policy_or_legacy_join"}),
 };let _=atomic_json(&c.root.join("fundamental-actions.json"),&json!({"at":iso(now()),"exchangeId":ex,"result":comparison}));
 let mut yielded=Vec::new();let mut budget_waited=false;let event=loop{
  let plan=match f.cohorts.prepare_entry(bank,log,t,&c.scope,hint,&facts,&f.authority,now()){Ok(Some(v))=>v,Ok(None)=>return fundamental_entry_deferred(c,ex,"value","NO_CURRENT_REVIEWED_ENTRY"),Err(e)if expected_fundamental_entry_denial(&e.to_string())=>return fundamental_entry_deferred(c,ex,"preflight",&e.to_string()),Err(e)=>return Err(e)};
  let validation_at=now();
  if let Err(e)=plan.validate_current(bank,log,t,&f.authority,validation_at){
   let until=plan.valid_until_ms();
   let _=atomic_json(&c.root.join("fundamental-entry-plan-validation.json"),&json!({"at":iso(validation_at),"exchangeId":ex,"stage":"current","reason":e.to_string(),"validationAtMs":validation_at,"validUntilMs":until,"freshnessRemainingMs":until.saturating_sub(validation_at),"expired":validation_at>=until,"mintedAt":plan.event()["receivedAt"],"physicalAttempted":false,"diagnosticOnly":true}));
   if expected_fundamental_entry_denial(&e.to_string()){return fundamental_entry_deferred(c,ex,"current",&e.to_string())}return Err(e)
  }
  let mut event=plan.event().clone();event["processedAt"]=json!(iso(now()));let mut preview=bank.engine().clone();
  match preview.advance(&event){Ok(())=>break event,Err("REVIEWED_ENTRY_FRESH_MARKET_CAP")if yielded.len()<3=>{
   if let Some(other)=retire_flat_draft_competitor(bank,log,&f.cohorts.managed_exchanges())?{yielded.push(other);continue}return fundamental_entry_deferred(c,ex,"native","REVIEWED_ENTRY_FRESH_MARKET_CAP_NO_PROVED_FLAT_DRAFT")
  },Err("REVIEWED_ENTRY_WRITE_BUDGET")if !budget_waited=>{
   let Some(ready)=reviewed_entry_budget_wait("REVIEWED_ENTRY_WRITE_BUDGET",budget_waited,now(),gap,latency,facts_until)?else{return fundamental_entry_deferred(c,ex,"native","REVIEWED_ENTRY_WRITE_BUDGET")};
   budget_waited=true;
   while now()<ready{refresh_controls(c,authority,stream)?;f.authority.validate()?;std::thread::sleep(Duration::from_millis(ready.saturating_sub(now()).clamp(1,100)));}
   refresh_controls(c,authority,stream)?;f.authority.validate()?;
   // Discard the failed preview and plan. Rebuild with fresh now against the
   // SAME original receipts; no read/source clock or native budget is renewed.
   continue
  },Err(e)if expected_fundamental_entry_denial(e)=>return fundamental_entry_deferred(c,ex,"native",e),Err(e)=>return Err(e.into())}
 };
 let before=bank.engine().order_records().iter().filter_map(|q|q["id"].as_u64()).max().unwrap_or(0);
 // No I/O source or tick can alter the frozen plan before this commit. A
 // native financial/durability failure at actual acceptance remains fatal.
 accept(bank,&event)?;let qs:Vec<_>=bank.engine().order_records().into_iter().filter(|q|q["id"].as_u64().is_some_and(|id|id>before)&&q["retired"]==false&&q["exchangeId"]==ex&&q["action"]=="buy"&&q["reviewedFundamentalEntry"]["reviewSha256"]==event["reviewSha256"]&&q["reviewedFundamentalEntry"]["originSha256"]==event["originSha256"]).collect();
 if qs.len()!=1{return Err("LIVE_REVIEWED_ENTRY_NATIVE_RESERVATION_NOT_UNIQUE".into())}let q=&qs[0];let id=q["id"].as_u64().ok_or("LIVE_QUOTE_ID")?;
 atomic_json(&c.root.join("fundamental-entry.json"),&json!({"at":iso(now()),"exchangeId":ex,"stage":"native_reserved_not_dispatched","qid":id,"quantity":q["quantity"],"yieldedProvedFlatDraftExchanges":yielded,"nativeRiskCapsChanged":false,"physicalAttempted":false}))?;
 loop{refresh_controls(c,authority,stream)?;f.authority.validate()?;let current=quote(bank,id)?;
  match reviewed_entry_activation_stage(&current,q,now()){
   Ok(None)=>break,Ok(Some(active))=>std::thread::sleep(Duration::from_millis(active.saturating_sub(now()).clamp(1,100))),
   Err(e)if e.to_string()=="LIVE_REVIEWED_ENTRY_RESERVATION_EXPIRED"=>return fundamental_entry_deferred(c,ex,"activation","RESERVATION_OR_REVIEW_EXPIRED"),Err(e)=>return Err(e),
  }
 }
 if queue_has_unknown(t)?{return fundamental_entry_deferred(c,ex,"queue","GLOBAL_UNKNOWN_ACCEPTANCE")}
 // Activation and native write pacing intentionally consume time. Obtain a
 // FINAL genuine seven-receipt packet after that wait instead of dispatching
 // against the earlier planning packet. Keep the exact funded qid and price.
 let final_facts=match preflight_snapshots(c,bank,source,market,Some(q),&mut LivePreflightReads{t,c,authority,stream,breadth,universe}){
  Ok(facts)=>facts,Err(e)if e.is::<PreflightQuoteInvalidated>()=>return fundamental_entry_deferred(c,ex,"final_facts","RESERVED_QUOTE_CHANGED_OR_RETIRED"),Err(e)=>return Err(e)
 };
 accept_fresh_book(bank,c,&target,&final_facts.book,source)?;
 let current=quote(bank,id)?;
 if current!=*q||current["retired"]!=false{return fundamental_entry_deferred(c,ex,"final_facts","RESERVED_QUOTE_CHANGED_OR_RETIRED")}
 if !f.eligible(bank,log,&c.scope,id,now())?{return fundamental_entry_deferred(c,ex,"value","CURRENT_REVIEWED_ENTRY_INELIGIBLE")}
 // Native clipped the actual funded quantity. submit_order binds that exact
 // genuine quote/slice, never the read hint or an invented quantity.
 submit_order(t,c,bank,log,authority,&current,final_facts,None,Some(f))
}
/// Release only local, never-dispatched reservations. The selector protects
/// held inventory, pending mates, reviewed managers and all package scopes.
/// No source/transport work may interleave validation and this serialized batch.
fn retire_stale_entry_drafts(c:&Control,bank:&mut Session,log:&mut DispatchLog,managed:&std::collections::BTreeSet<String>)->Result<usize>{
 let at=now();let selected=moloch_live_runtime::stale_entry_drafts::select(bank,log,&c.scope,managed,at)?;
 retire_selected_entry_drafts(c,bank,log,managed,&selected,"stale_unsubmitted_ordinary_buy","stale-entry-drafts.json")
}
fn retire_unqualified_entry_drafts(c:&Control,bank:&mut Session,log:&mut DispatchLog,managed:&std::collections::BTreeSet<String>)->Result<usize>{
 if c.maker_policy.is_some(){return Ok(0)} // Retention only, not physical permission.
 let selected=moloch_live_runtime::stale_entry_drafts::select_unqualified_new_entries(bank,log,&c.scope,managed,now())?;
 retire_selected_entry_drafts(c,bank,log,managed,&selected,"unqualified_flat_ordinary_buy","ordinary-entry-quarantine.json")
}
fn retire_selected_entry_drafts(c:&Control,bank:&mut Session,log:&mut DispatchLog,managed:&std::collections::BTreeSet<String>,selected:&moloch_live_runtime::stale_entry_drafts::Selection,reason:&str,diagnostic:&str)->Result<usize>{
 if selected.drafts().is_empty(){return Ok(0)}
 selected.validate_current(bank,log,&c.scope,managed,now())?;
 let mut rows=Vec::new();
 for draft in selected.drafts(){let id=draft.qid();let current=quote(bank,id)?;
  if current!=*draft.quote()||!log.never_attempted(id)||log.intents.contains_key(&id)||log.attempted.contains(&id)||log.acknowledgments.contains_key(&id)||bank.engine().live_order_bindings().contains_key(&id)||bank.engine().fills().iter().any(|f|f["orderId"]==id){return Err("LIVE_STALE_DRAFT_ORIGINAL_CHANGED".into())}
  let proof=log.proof(id);log.append(json!({"kind":"not_sent","qid":id,"proof":proof}))?;
  // Existing native defer flag suppresses immediate replanning. It grants no
  // package authority and does not cancel or release any physical order.
  accept(bank,&json!({"type":"unsubmitted_intent_retired","source":"moloch_live_dispatcher","orderId":id,"confirmedNetworkAttempts":0,"receiptHash":hash(&proof),"deferPlanningForPackage":true,"receivedAt":iso(now())}))?;
  rows.push(json!({"qid":id,"releasedLocalReservationUnits":draft.reserved_units().to_string()}));
 }
 atomic_json(&c.root.join(diagnostic),&json!({"at":iso(now()),"pid":std::process::id(),"reason":reason,"originalNativeHead":selected.native_head(),"originalDispatchDescriptor":selected.dispatch_descriptor_identity(),"retired":rows,"nativeHead":bank.durable_head(),"networkRequests":0,"physicalOrdersCanceled":0,"pricesChanged":false}))?;
 Ok(rows.len())
}
/// A prior durable cancellation is not a retry ticket, even when the original
/// process died before SDK admission. Exact order/fill reconciliation continues.
fn cancel_may_start(log:&DispatchLog,qid:u64)->bool{!log.closed.contains(&qid)&&!log.cancel_acknowledged.contains(&qid)&&!log.cancel_attempted(qid)}
fn defensive_cancel_window(valid_until:u64,at:u64,write_gap:u64)->Option<u64>{
 let end=valid_until.min(at.checked_add(10_000)?);(write_gap>=2500&&at.checked_add(write_gap).is_some_and(|ready|ready<end)).then_some(end)
}
fn defensive_cancel_current(decision:&moloch_live_runtime::ordinary_side_edge::RestingDecision,bank:&Session,scope:&Scope,log:&DispatchLog,managed:&std::collections::BTreeSet<String>,at:u64)->Result<bool>{
 match decision.validate_current(bank,scope,log,managed,at){Ok(())=>Ok(true),Err(e)if matches!(e.to_string().as_str(),"RESTING_EDGE_EXPIRED"|"RESTING_EDGE_NO_LONGER_BELOW"|"RESTING_EDGE_CONTEXT_CHANGED")=>Ok(false),Err(e)=>Err(e)}
}
fn record_cancel_ack(c:&Control,log:&mut DispatchLog,qid:u64,ack:&Receipt)->Result<()>{
 record_receipt(&c.root,ack)?;log.append(json!({"kind":"cancel_ack","qid":qid,"receipt":ack}))
}
/// Both mandatory and optional lanes retain the original DELETE/ACK/full-tape
/// lifecycle. Neither ACK nor a local error changes native financial state.
fn cancel_owned(t:&mut Transport,c:&Control,bank:&mut Session,log:&mut DispatchLog,authority:&ProductionAuthority,old:&Value,facts:CancelSnapshotSet,defense:Option<&moloch_live_runtime::ordinary_side_edge::RestingDecision>)->Result<()>{
 let qid=old["id"].as_u64().ok_or("LIVE_QUOTE_ID")?;if !cancel_may_start(log,qid){return Ok(())}
 let actual=bank.engine().live_order_bindings().get(&qid).ok_or("LIVE_CANCEL_BINDING")?.clone();let at=now();
 let expires=if let Some(decision)=defense{
  if decision.qid()!=qid||decision.actual_order_id()!=actual{return Err("LIVE_DEFENSIVE_CANCEL_BINDING".into())}
  let gap=bank.engine().policy()["writeGapMs"].as_u64().ok_or("LIVE_DEFENSIVE_CANCEL_WRITE_GAP")?;
  let Some(end)=defensive_cancel_window(decision.valid_until_ms(),at,gap)else{return Ok(())};end
 }else{at.checked_add(10_000).ok_or("LIVE_CANCEL_CLOCK_RANGE")?};
 // Facts and the final current signal precede permit creation. A queued SDK
 // call still retains this original expiry and every physical guard.
 let p=Permit{run_id:c.activation.run_id.clone(),intent_id:format!("{qid}-cancel"),owner:"Codex".into(),expires_at_ms:expires,maximum_quantity:c.activation.maximum_quantity,maximum_cost:c.activation.maximum_committed.clone()};
 let spec=RequestSpec{method:"DELETE".into(),endpoint:format!("/orders/{actual}"),body:None,body_json:None};
 if let Err(error)=authority.prepare_cancel(bank,qid,spec.clone(),p.clone(),facts){if defer_account_preintent(c,bank,log,authority,qid,true,error.as_ref())?{return Ok(())}return Err(error)}
 if let Err(e)=authority.authorize(&c.scope,&spec,&p,false,now()){authority.clear_ready();if defense.is_some()&&!authority.can_read(&c.scope){return Err("LIVE_DEFENSIVE_CANCEL_CONTROL_OR_STOP".into())}atomic_json(&c.root.join("deferred.json"),&json!({"at":iso(now()),"qid":qid,"code":e.code,"optionalDefensiveCancel":defense.is_some()}))?;return Ok(())}
 if now()>=expires{authority.clear_ready();return Ok(())}
 let attempt_at=now();
 log.append(json!({"kind":"cancel_attempt","qid":qid,"spec":spec,"permit":p,"at":iso(attempt_at),"defensive":defense.map(|d|d.evidence())}))?;
 let ack=match t.cancel(actual.clone(),p.clone()){Ok(r)=>r,Err(e)=>{
  log.append(json!({"kind":"error","operation":"cancel","qid":qid,"error":e,"at":iso(now())}))?;
  if defense.is_some(){if let Ok(proof)=t.take_cancel_no_http_proof(){
   proof.validate_binding(&spec,&p,attempt_at,&e)?;
   let scope_sha=format!("{:x}",Sha256::digest(serde_json::to_vec(&c.scope)?));
   if proof.evidence()["accountScope"]!=c.account_scope||proof.evidence()["scopeSha256"]!=scope_sha||proof.evidence()["orderId"]!=actual{return Err("LIVE_CANCEL_NO_HTTP_SCOPE".into())}
   // Persist historical evidence without creating an ACK, terminal, retry
   // capability or native retirement. The original attempt stays nonretryable.
   log.append(json!({"kind":"source","operation":"defensive_cancel_no_http","qid":qid,"at":iso(now()),"originalAttemptAt":iso(attempt_at),"proof":proof.evidence()}))?;
   authority.clear_ready();
   atomic_json(&c.root.join("defensive-cancel.json"),&json!({"at":iso(now()),"qid":qid,"state":"deferred_proved_no_http","reason":e.code,"retryPermitted":false,"orderTerminal":false}))?;
   return Ok(())
  }}
  authority.clear_ready();
  if moloch_live_runtime::read_recovery::reconcilable_cancel_rejection(&e){
   let observed=request(t,&c.root,Endpoint::OwnedOrder{order_id:actual.clone()})?;let filled=request(t,&c.root,Endpoint::OwnedFills{order_id:actual.clone()})?;
   // No source hook may interrupt a returned actual fill tape and its commit.
   for e in feedback::fills(old,&actual,&c.scope.tournament_id,&filled,&iso(now()))?{accept(bank,&e)?}
   if let Some(e)=feedback::terminal(old,&actual,&c.scope.tournament_id,&observed,&filled,&iso(now()))?{accept(bank,&e)?;log.append(json!({"kind":"closed","qid":qid,"orderReceiptHash":feedback::digest(&observed)?,"fillReceiptHash":feedback::digest(&filled)?,"afterRejectedCancel":true}))?;moloch_live_runtime::execution_distribution::confirm_runtime_terminal(bank,log,qid,&observed,&filled,now());moloch_live_runtime::execution_distribution::confirm_acquisition_terminal(bank,log,qid,&observed,&filled,now());return Ok(())}
  }
  // No mayHaveExecuted/string-based no-network inference: missing typed
  // evidence, writes after the marker and uncertain outcomes remain fatal.
  return Err("LIVE_CANCEL_REJECTED_RECONCILIATION_REQUIRED".into())
 }};record_cancel_ack(c,log,qid,&ack)?;authority.clear_ready();Ok(())
}
fn defensive_cancel_candidate(c:&Control,bank:&Session,log:&DispatchLog,managed:&std::collections::BTreeSet<String>,cursor:Option<&str>,at:u64)->Result<Option<(String,moloch_live_runtime::ordinary_side_edge::RestingDecision)>>{
 if !c.defensive_cancel{return Ok(None)}log.require_resolved_attempts()?;
 let mut nominations=Vec::new();for id in log.acknowledgments.keys().copied(){if !cancel_may_start(log,id){continue}
  let assessed=moloch_live_runtime::ordinary_side_edge::assess_resting(bank,&c.scope,log,managed,id,at);
  if let Some(decision)=assessed.into_nomination(){let gap=bank.engine().policy()["writeGapMs"].as_u64().ok_or("LIVE_DEFENSIVE_CANCEL_WRITE_GAP")?;if defensive_cancel_window(decision.valid_until_ms(),at,gap).is_none(){continue}let q=quote(bank,id)?;let key=format!("{}:{}:cancel",q["exchangeId"].as_str().ok_or("LIVE_SCOPE")?,q["side"].as_str().ok_or("LIVE_SIDE")?);nominations.push((key,decision));}
 }
 let keys:Vec<_>=nominations.iter().map(|(k,_)|k.clone()).collect();let Some(index)=moloch_live_runtime::scheduling::preflight_rotation(&keys,cursor).first().copied()else{return Ok(None)};Ok(Some(nominations.swap_remove(index)))
}
/// Four original global facts only. Re-evaluate after source service/reads;
/// stale/unavailable/covered/filled input cannot become cancellation authority.
fn defensive_cancel_preflight(c:&Control,bank:&mut Session,log:&DispatchLog,source:&mut RestSource,managed:&std::collections::BTreeSet<String>,qid:u64,reads:&mut impl OrdinaryPreflightReads)->Result<Option<(moloch_live_runtime::ordinary_side_edge::RestingDecision,CancelSnapshotSet)>>{
 if !c.defensive_cancel||!cancel_may_start(log,qid)||!reads.guarded()?{return Ok(None)}
 let Some(initial)=moloch_live_runtime::ordinary_side_edge::assess_resting(bank,&c.scope,log,managed,qid,reads.now_ms()).into_nomination()else{return Ok(None)};
 let gap=bank.engine().policy()["writeGapMs"].as_u64().ok_or("LIVE_DEFENSIVE_CANCEL_WRITE_GAP")?;
 if defensive_cancel_window(initial.valid_until_ms(),reads.now_ms(),gap).is_none(){return Ok(None)}
 let facts=cancel_snapshots_with(|ep|preflight_read(bank,source,reads,None,ep))?;
 if !reads.guarded()?||!cancel_may_start(log,qid){return Ok(None)}
 let Some(decision)=moloch_live_runtime::ordinary_side_edge::assess_resting(bank,&c.scope,log,managed,qid,reads.now_ms()).into_nomination()else{return Ok(None)};
 let gap=bank.engine().policy()["writeGapMs"].as_u64().ok_or("LIVE_DEFENSIVE_CANCEL_WRITE_GAP")?;
 if defensive_cancel_window(decision.valid_until_ms(),reads.now_ms(),gap).is_none(){return Ok(None)}
 if !defensive_cancel_current(&decision,bank,&c.scope,log,managed,reads.now_ms())?{return Ok(None)}Ok(Some((decision,facts)))
}
// A negative-only native assessment removes untouched drafts from this pass,
// before bulk prices or seven facts. It never consumes/renews the shared cursor
// or preflight permit; remaining candidates retain their original fair order.
fn early_native_side_filter(mode:moloch_live_runtime::ordinary_side_edge::Mode,bank:&Session,scope:&Scope,log:&DispatchLog,managed:&std::collections::BTreeSet<String>,quotes:&[Value],physical:&mut Vec<(String,usize,bool)>,at:u64)->Vec<Value>{
 use moloch_live_runtime::ordinary_side_edge::{self,Mode};
 if mode==Mode::Disabled{return Vec::new()}
 let Ok(screen)=ordinary_side_edge::EarlyScreen::capture(bank,log)else{return Vec::new()};
 let mut diagnostics=Vec::new();
 physical.retain(|(_,index,urgent)|{
  if *urgent{return true}
  let Some(q)=quotes.get(*index)else{return true};let Some(qid)=q["id"].as_u64()else{return true};
  let assessment=screen.assess(bank,scope,log,managed,qid,at);
  let veto=mode==Mode::Enforce&&assessment.negative().is_some_and(|d|d.validate_current(bank,scope,log,managed,now()).is_ok());
  diagnostics.push(json!({"qid":qid,"class":assessment.class,"reason":&assessment.reason,"evidence":assessment.negative().map(|d|d.evidence()),"vetoedThisPass":veto}));!veto
 });diagnostics
}
fn dispatch(t:&mut Transport,c:&Control,bank:&mut Session,log:&mut DispatchLog,authority:&Arc<ProductionAuthority>,source:&mut RestSource,allow_new_submissions:bool,preflight_cursor:&mut Option<String>,stream:&Realtime,breadth:&mut Option<Recorder>,universe:&mut Option<Universe>,windows:&std::collections::BTreeMap<String,u64>,maker_review:Option<&moloch_live_runtime::maker_policy::Reviewed>,mut fundamental:Option<&mut FundamentalRuntime>)->Result<()>{
 if allow_new_submissions&&!bank.engine().package_blocks_submissions(){
  let managed=if let Some(f)=fundamental.as_ref(){f.authority.validate()?;f.cohorts.managed_exchanges()}else{Default::default()};
  if maker_review.is_none(){if !queue_has_unknown(t)?{retire_unqualified_entry_drafts(c,bank,log,&managed)?;}}
  else{retire_stale_entry_drafts(c,bank,log,&managed)?;}
 }
 let orders=bank.engine().order_records();
 let flow_started=now();let maximum_qid_before=orders.iter().filter_map(|q|q["id"].as_u64()).max().unwrap_or(0);
 let managed_refresh=fundamental.as_ref().map(|f|f.cohorts.managed_exchanges()).unwrap_or_default();
 let protected_refresh=profit_refresh_protected(&c.scope,&bank.engine().package_records())?;
 let mut refresh_eligible=std::collections::BTreeSet::new();
 for q in &orders{
  let Some(id)=q["id"].as_u64()else{continue};
  if !log.never_attempted(id)||log.intents.contains_key(&id)||bank.engine().is_package_order(id)||!profit_hint_scope_allowed(q,&managed_refresh,&protected_refresh){continue}
  if q["expiresAt"].as_u64().is_some_and(|end|end>flow_started)&&moloch_live_runtime::exit_refresh::ReadKind::from_quote(q)==Some(moloch_live_runtime::exit_refresh::ReadKind::Profit)&&!unused_inherited_profit_draft(bank,log,&c.scope,q)?{continue}
  refresh_eligible.insert(id);
 }
 let mut age_positions=None;let mut age_inherited=None;
 let(hints,refresh_unknown)=if allow_new_submissions{prepare_expired_exit_hints(&orders,flow_started,
  |id|refresh_eligible.contains(&id),
  |ex,side|Ok(bank.engine().wallet.quantity(ex,match side{"YES"=>moloch_replay::Side::Yes,"NO"=>moloch_replay::Side::No,_=>return Err("LIVE_SIDE".into())})),
  |ex|{if let Some(f)=fundamental.as_ref(){f.authority.validate()?;if f.cohorts.manages(ex){return f.cohorts.exit_refresh_needed(bank,log,&c.scope,ex,f.authority.trusted(),flow_started)}}Ok(true)},
  |ex,side|{
   // Managed cohorts already passed their CURRENT reviewed deadline/revocation/
   // executable-value gate. Their explicit objective must not inherit native age.
   if fundamental.as_ref().is_some_and(|f|f.cohorts.manages(ex)){return Ok(true)}
   if age_positions.is_none(){age_positions=Some(bank.engine().wallet.positions());}
   let positions=age_positions.as_ref().unwrap();
   if age_inherited.is_none()&&positions.get(ex).and_then(|v|v.get(side)).and_then(Value::as_array).is_some_and(|lots|lots.iter().any(|l|l.get("executedAt").is_none_or(Value::is_null))){age_inherited=Some(bank.engine().report()?["inheritedInventory"].clone());}
   remaining_exit_age_due(bank.engine().policy(),positions,age_inherited.as_ref().unwrap_or(&Value::Null),ex,side,flow_started)
  },
  ||queue_has_unknown(t),|old,unknown|refresh_safety(c,bank,log,authority,stream,old,unknown))?
 }else{(std::collections::BTreeMap::new(),false)};
 // Cancellations and reconciliation take priority over new purchases.
 let fundamental_cancels=if let Some(f)=fundamental.as_ref(){f.authority.validate()?;let mut ids=f.cohorts.entry_cancellations(bank,log,f.authority.trusted(),now())?;ids.extend(f.cohorts.existing_exit_cancellations(bank,log,f.authority.trusted(),now())?);ids}else{Vec::new()};
 for old in &orders {
  let qid=old["id"].as_u64().ok_or("LIVE_QUOTE_ID")?;if bank.engine().is_package_order(qid)||old["retired"]==true{continue}
  let expired=old["expiresAt"].as_u64().is_none_or(|t|t<=now());let canceled=old["cancelEffectiveAt"].as_u64().is_some_and(|t|t<=now());
  if !expired&&!canceled&&!fundamental_cancels.contains(&qid){continue}
  if log.never_attempted(qid){retire_never_sent(bank,log,old)?;continue}
  if expired||log.closed.contains(&qid)||log.cancel_acknowledged.contains(&qid){continue} // Wait for authoritative terminal GET; never repeat a confirmed cancellation.
  if !cancel_may_start(log,qid){continue} // Unmatched prior attempt waits for exact reconciliation, never a repeat.
  let facts=cancel_snapshots(t,c)?;cancel_owned(t,c,bank,log,authority,old,facts,None)?;return Ok(())
 }
 if !allow_new_submissions||bank.engine().package_blocks_submissions(){return Ok(())}
 if let Some(f)=fundamental.as_deref_mut(){if refresh_fundamental_cohort(t,c,bank,log,authority,stream,f)?{return Ok(())}}
 let mut preflight_budget=moloch_live_runtime::scheduling::PreflightBudget::default();
 let mut stale_existing_hints=Vec::new();
 if let Some(f)=fundamental.as_deref_mut(){
  f.authority.validate()?;let mut existing_hints=Vec::new();
  for ex in f.cohorts.managed_exchanges(){if let Some(hint)=existing_exit_read_hint(bank,log,&c.scope,f,&ex,now())?{if hint.reason=="reviewed_inventory_book_refresh"{stale_existing_hints.push(hint)}else{existing_hints.push(hint);}}}
  if !existing_hints.is_empty(){let keys:Vec<_>=existing_hints.iter().map(|h|format!("{}:{}:sell",h.exchange_id,h.side)).collect();let index=moloch_live_runtime::scheduling::preflight_rotation(&keys,preflight_cursor.as_deref())[0];
   if !preflight_budget.try_start(){return Ok(())}*preflight_cursor=Some(keys[index].clone());let hint=&existing_hints[index];
   reviewed_existing_exit(t,c,bank,log,authority,source,stream,f,hint,*windows.get(&hint.exchange_id).ok_or("LIVE_RECOVERY_WINDOW_MISSING")?,breadth,universe)?;return Ok(())
  }
 }
 let profit_keys:Vec<_>=hints.iter().filter(|(_,(_,h))|h.kind()==moloch_live_runtime::exit_refresh::ReadKind::Profit).map(|(key,_)|key.clone()).collect();
 let risk_keys:Vec<_>=hints.iter().filter(|(_,(_,h))|h.kind()==moloch_live_runtime::exit_refresh::ReadKind::MaximumAge).map(|(key,_)|key.clone()).collect();
 if !risk_keys.is_empty(){
  let keys=risk_keys;let index=moloch_live_runtime::scheduling::preflight_rotation(&keys,preflight_cursor.as_deref())[0];let key=&keys[index];
  let(old,hint)=hints.get(key).ok_or("LIVE_EXIT_REFRESH_HINT")?;
  if !preflight_budget.try_start(){return Ok(())}*preflight_cursor=Some(key.clone());
  refresh_exit(t,c,bank,log,authority,source,stream,old,hint,flow_started,maximum_qid_before,refresh_unknown,*windows.get(hint.exchange()).ok_or("LIVE_RECOVERY_WINDOW_MISSING")?,breadth,universe,fundamental.as_deref_mut().filter(|f|f.cohorts.manages(hint.exchange())))?;
  return Ok(()) // Ineligible refresh still consumes this pass; never another packet.
 }
 let entry_hints=if !owner_capital_ready(c,bank)?{Vec::new()}else if let Some(f)=fundamental.as_ref(){f.authority.validate()?;f.cohorts.entry_hints(bank,log,&c.scope,f.authority.trusted(),now())?.into_iter().filter(|h|!moloch_live_runtime::order_snapshot::snapshot(bank).records().iter().any(|q|q["exchangeId"]==h.exchange_id&&q["retired"]==false&&q.get("reviewedFundamentalEntry").is_some())).collect::<Vec<_>>()}else{Vec::new()};
 // At most one new physical order per pass; subsequent fills change funding.
 // Retired history and already-physical orders have separate reconciliation
 // paths above; they cannot become a new submission or consume its ranking.
 let quotes:Vec<_>=moloch_live_runtime::order_snapshot::snapshot(bank).records().iter().filter(|q|q["retired"]==false&&q["id"].as_u64().is_some_and(|id|log.never_attempted(id)&&!bank.engine().is_package_order(id))).cloned().collect();
 let keys:Vec<String>=quotes.iter().map(|q|format!("{}:{}:{}",q["exchangeId"].as_str().unwrap_or(""),q["side"].as_str().unwrap_or(""),q["action"].as_str().unwrap_or(""))).collect();
 let selection=moloch_live_runtime::quote_priority::from_native(bank,&c.scope,&quotes,now(),preflight_cursor.as_deref())?;
 let _=atomic_json(&c.root.join("quote-priority.json"),&json!({"at":iso(now()),"selection":selection,"pricesChanged":false,"profitabilityEstablished":false}));
 let mut ranked=Vec::new();let mut reviewed_exit_urgency=std::collections::BTreeSet::new();let mut maker_assessments=Vec::new();let mut maker_decisions=Vec::new();
 for index in selection.ordered_indices.iter().copied(){
  let q=&quotes[index];if !owner_capital_buys_allowed(c,bank,&q["action"])?{continue}
  if let Some(f)=fundamental.as_ref(){if f.cohorts.manages(q["exchangeId"].as_str().ok_or("LIVE_QUOTE_SCOPE")?){if f.eligible(bank,log,&c.scope,q["id"].as_u64().ok_or("LIVE_QUOTE_ID")?,now())?{
    // A managed SELL passes eligible only through current Decision::Exit:
    // exact covered depth plus reviewed deadline, revocation or value breach.
    if q["action"]=="sell"{reviewed_exit_urgency.insert(q["id"].as_u64().ok_or("LIVE_QUOTE_ID")?);}
    ranked.push((if q["action"]=="sell"{0}else{1},index));}continue}}
  if q["action"]=="buy"{if let Some(review)=maker_review{
   let decision=match moloch_live_runtime::maker_policy::decide(bank,&c.scope,log,review,q["id"].as_u64().ok_or("LIVE_QUOTE_ID")?,now()){Ok(d)=>d,Err(error)if expected_maker_expiry(error.as_ref())=>{continue},Err(error)=>return Err(error)};
   maker_assessments.push(decision.proof().clone());if !decision.allowed(){continue}maker_decisions.push((index,decision));continue;
  }else if !unreviewed_inventory_reduction(c,bank,log,q)?{continue}}
  ranked.push((0,index));
 }
 if !maker_decisions.is_empty(){let decisions:Vec<_>=maker_decisions.iter().map(|(_,decision)|decision).collect();for position in moloch_live_runtime::maker_policy::rank(&decisions,now())?{let(index,decision)=&maker_decisions[position];ranked.push((decision.priority(),*index));}}
 if maker_review.is_some(){let _=atomic_json(&c.root.join("maker-policy.json"),&json!({"at":iso(now()),"assessments":maker_assessments,"executionAuthority":false}));}
 ranked.sort_by_key(|(priority,_)|*priority);
 let selection_at=now();let mut physical=Vec::new();let mut covered_profit=std::collections::BTreeSet::new();let mut urgency_positions=None;let mut urgency_inherited=None;
 for(_,index)in &ranked{let q=&quotes[*index];let qid=q["id"].as_u64().ok_or("LIVE_QUOTE_ID")?;
  if !physical_preflight_ready(q,log.never_attempted(qid),log.intents.contains_key(&qid),selection_at){continue}
  if q["action"]=="buy"&&selection_at>=authority.entry_until_ms(){retire_never_sent(bank,log,q)?;continue}
  let reviewed_exit=reviewed_exit_urgency.contains(&qid);
  let due_age=if q["action"]=="sell"&&q["inventoryExitReason"]=="maximumAge"&&!reviewed_exit{
   let ex=q["exchangeId"].as_str().ok_or("LIVE_QUOTE_SCOPE")?;let side=q["side"].as_str().ok_or("LIVE_SIDE")?;
   let native_side=match side{"YES"=>moloch_replay::Side::Yes,"NO"=>moloch_replay::Side::No,_=>return Err("LIVE_SIDE".into())};
   if bank.engine().wallet.quantity(ex,native_side)==0{false}else{
   if urgency_positions.is_none(){urgency_positions=Some(bank.engine().wallet.positions());}
   let positions=urgency_positions.as_ref().unwrap();
   if urgency_inherited.is_none()&&positions.get(ex).and_then(|v|v.get(side)).and_then(Value::as_array).is_some_and(|lots|lots.iter().any(|l|l.get("executedAt").is_none_or(Value::is_null))){urgency_inherited=Some(bank.engine().report()?["inheritedInventory"].clone());}
   remaining_exit_age_due(bank.engine().policy(),positions,urgency_inherited.as_ref().unwrap_or(&Value::Null),ex,side,selection_at)?}
  }else{false};
  let urgent=physical_exit_urgent(q,due_age,reviewed_exit);
  if !urgent&&profit_hint_scope_allowed(q,&managed_refresh,&protected_refresh)
   &&moloch_live_runtime::exit_refresh::ReadKind::from_quote(q)==Some(moloch_live_runtime::exit_refresh::ReadKind::Profit)
   &&selection.assessments.iter().any(|a|a.index==*index&&a.decision.class==moloch_live_runtime::quote_priority::Class::CoveredMarketable)
   &&moloch_live_runtime::exit_refresh::current_profit_qualified(bank,q)?{covered_profit.insert(*index);}
  physical.push((keys[*index].clone(),*index,urgent));
 }
 // Mandatory cancels and actual urgent exits above retain priority. One
 // optional defense replaces this pass's speculative packet, never adds one.
 if c.defensive_cancel&&covered_profit.is_empty()&&!physical.iter().any(|(_,_,urgent)|*urgent){
  let managed=fundamental.as_ref().map(|f|f.cohorts.managed_exchanges()).unwrap_or_default();
  if let Some((key,decision))=defensive_cancel_candidate(c,bank,log,&managed,preflight_cursor.as_deref(),now())?{
   if !preflight_budget.try_start(){return Ok(())}*preflight_cursor=Some(key);
   let qid=decision.qid();let _=decision;
   if let Some((current,facts))=defensive_cancel_preflight(c,bank,log,source,&managed,qid,&mut LivePreflightReads{t,c,authority,stream,breadth,universe})?{
    refresh_controls(c,authority,stream)?;
    match t.package_queue_quiet(){Ok(_)=>{},Err(e)if e.code=="PACKAGE_QUEUE_NOT_QUIET"=>return Ok(()),Err(e)=>return Err(e.into())}
    if !defensive_cancel_current(&current,bank,&c.scope,log,&managed,now())?{return Ok(())}
    let q=quote(bank,qid)?;cancel_owned(t,c,bank,log,authority,&q,facts,Some(&current))?;
   }return Ok(())
  }
 }
 let managed=fundamental.as_ref().map(|f|f.cohorts.managed_exchanges()).unwrap_or_default();
 let early_at=now();let early=early_native_side_filter(c.ordinary_side_edge,bank,&c.scope,log,&managed,&quotes,&mut physical,early_at);
 if !early.is_empty(){let _=atomic_json(&c.root.join("ordinary-side-edge-early.json"),&json!({"at":iso(early_at),"mode":c.ordinary_side_edge,"assessments":early,"httpReads":0,"reservationReleased":false,"physicalAttempted":false,"executionAuthority":false}));}
 let mut chosen=shared_preflight_with_profit(&stale_existing_hints,&entry_hints,&profit_keys,&physical,&covered_profit,preflight_cursor.as_deref());
 if let Some((SharedPreflight::Physical(index),key))=&chosen{
  let managed=fundamental.as_ref().map(|f|f.cohorts.managed_exchanges()).unwrap_or_default();
  let vetoes=bulk_negative_vetoes(c,&quotes,&physical,&selection,&managed,*index,&mut LivePreflightReads{t,c,authority,stream,breadth,universe})?;
  if !vetoes.is_empty(){
   if apply_bulk_vetoes(&c.scope,&quotes,&mut physical,&vetoes,*index,now())?{*preflight_cursor=Some(key.clone());}
   chosen=shared_preflight_with_profit(&stale_existing_hints,&entry_hints,&profit_keys,&physical,&covered_profit,preflight_cursor.as_deref());
  }
 }
 let Some((chosen,key))=chosen else{return Ok(())};
 let chosen_index=match chosen{
  SharedPreflight::Physical(index)=>index,
  SharedPreflight::ProfitRead(index)=>{
   if !preflight_budget.try_start(){return Ok(())}*preflight_cursor=Some(key.clone());
   let(old,hint)=hints.get(&profit_keys[index]).ok_or("LIVE_EXIT_REFRESH_HINT")?;
   refresh_exit(t,c,bank,log,authority,source,stream,old,hint,flow_started,maximum_qid_before,refresh_unknown,*windows.get(hint.exchange()).ok_or("LIVE_RECOVERY_WINDOW_MISSING")?,breadth,universe,None)?;return Ok(())
  },
  SharedPreflight::ExistingRead(index)|SharedPreflight::EntryRead(index)=>{
   if !preflight_budget.try_start(){return Ok(())}*preflight_cursor=Some(key);
   let f=fundamental.as_deref_mut().ok_or("LIVE_FUNDAMENTAL_READ_RUNTIME")?;f.authority.validate()?;
   if matches!(chosen,SharedPreflight::ExistingRead(_)){let hint=&stale_existing_hints[index];reviewed_existing_exit(t,c,bank,log,authority,source,stream,f,hint,*windows.get(&hint.exchange_id).ok_or("LIVE_RECOVERY_WINDOW_MISSING")?,breadth,universe)?;}
   else{let hint=&entry_hints[index];reviewed_fundamental_entry(t,c,bank,log,authority,source,stream,f,hint,*windows.get(&hint.exchange_id).ok_or("LIVE_RECOVERY_WINDOW_MISSING")?,breadth,universe)?;}return Ok(())
  }
 };
 // A covered inherited Profit quote must not be frozen while seven real GETs
 // consume its short original lifetime. Use it only to nominate fresh reads;
 // the final book/planner must provide a genuinely new native quote afterward.
 let nominated=&quotes[chosen_index];
 if unused_inherited_profit_draft(bank,log,&c.scope,nominated)?{
  if !preflight_budget.try_start(){return Ok(())}*preflight_cursor=Some(keys[chosen_index].clone());
  let unknown=queue_has_unknown(t)?;
  let checked=refresh_safety(c,bank,log,authority,stream,nominated,unknown)?;
  let hint=match moloch_live_runtime::exit_refresh::ReadHint::from_live_profit(nominated,checked,now()){
   Ok(v)=>v,Err(e)=>return refresh_deferred(c,"live_hint_ineligible",e.as_ref())
  };
  refresh_exit(t,c,bank,log,authority,source,stream,nominated,&hint,flow_started,maximum_qid_before,unknown,*windows.get(hint.exchange()).ok_or("LIVE_RECOVERY_WINDOW_MISSING")?,breadth,universe,None)?;return Ok(())
 }
 ranked.retain(|(_,index)|*index==chosen_index);
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
  let Some((current,facts))=ordinary_preflight(c,bank,log,source,&q,&mut LivePreflightReads{t,c,authority,stream,breadth,universe})?else{
   authority.clear_ready();let assessment=moloch_live_runtime::quote_priority::from_native(bank,&c.scope,&[quote(bank,qid)?],now(),None)?;
   let _=atomic_json(&c.root.join("quote-book-prefilter.json"),&json!({"at":iso(now()),"qid":qid,"assessment":assessment,"stage":"deferred_before_account_packet","physicalAttempted":false,"priceChanged":false,"expiryExtended":false}));return Ok(())
  };let final_at=now();
  let native_active=current["activationAt"].as_u64().is_some_and(|t|t<=final_at);
  if early_exit_prefetch{let _=atomic_json(&c.root.join("exit-prefetch.json"),&json!({"at":iso(final_at),"qid":qid,"stage":"facts_complete","createdAtMs":q["createdAt"],"activationAtMs":q["activationAt"],"expiresAtMs":q["expiresAt"],"prefetchStartedAtMs":prefetch_started,"prefetchElapsedMs":final_at.saturating_sub(prefetch_started),"remainingLifetimeMs":current["expiresAt"].as_u64().map(|t|t.saturating_sub(final_at)),"nativeActive":native_active,"retired":current["retired"],"cancelPending":current["cancelEffectiveAt"].is_u64(),"expiryExtended":false}));}
  if current["retired"]==true||current["cancelEffectiveAt"].is_u64()||current["expiresAt"].as_u64().is_none_or(|v|v<=final_at+3000){if current["retired"]==false{retire_never_sent(bank,log,&current)?}continue}
  // A fast preflight never authorizes an early order. The final production
  // authority independently checks activation again at the physical boundary.
  if !native_active{continue}
  let refreshed=moloch_live_runtime::quote_priority::from_native(bank,&c.scope,std::slice::from_ref(&current),now(),None)?;
  if refreshed.ordered_indices.is_empty(){authority.clear_ready();let _=atomic_json(&c.root.join("quote-priority-deferred.json"),&json!({"at":iso(now()),"qid":qid,"assessment":refreshed,"physicalAttempted":false,"priceChanged":false}));return Ok(())}
  let fundamental_managed=fundamental.as_ref().is_some_and(|f|f.cohorts.manages(current["exchangeId"].as_str().unwrap_or("")));
  // Cursor and the one-preflight budget were consumed before these ORIGINAL
  // seven facts. A veto returns rather than buying another packet this pass.
  if !ordinary_side_edge_allows(c,bank,log,&current,&facts.book,fundamental_managed,now()){authority.clear_ready();return Ok(())}
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
 let material=t.realtime_token().map_err(moloch_live_runtime::stream_token_retry::TokenFailure::from_token_error)?;let auth=Auth::from_official_response(material)?;
 // Only the native connection boundary produces this retryable type. HTTP,
 // credentials, JOIN and durable source errors retain their original types.
 let stream=if c.account_realtime{Realtime::connect_with_account(auth,&c.scope.tournament_id,moloch_live_runtime::account_realtime::options(&c.scope,&c.root)?)}else{Realtime::connect(auth,&c.scope.tournament_id)}.map_err(moloch_live_runtime::stream_connect_retry::ConnectionFailure::from_connect_error)?;
 let joined_at=match stream.incoming.recv_timeout(Duration::from_secs(12)){
  Ok(Incoming::Joined{received_at})=>{accept_source_events(bank,&source.subscribed(&received_at)?)?;received_at}
  _=>return Err("LIVE_STREAM_JOIN_FAILED".into())
 };Ok((stream,joined_at))
}
fn account_reconciliation_due(stream:&Realtime)->bool{stream.account_signal().is_some_and(|s|s.requires_reconciliation())}
fn drain_stream(stream:&Realtime,bank:&mut Session,source:&mut RestSource,breadth:&mut Option<Recorder>,universe:&mut Option<Universe>)->Result<()>{
 require_stream(stream)?;
 let mut pending=QueuedSourceBatch::default();
 let account=stream.account_signal();let generation=||account.as_ref().map(|s|s.generation());
 let mut catchup=realtime_book_catchup::Drain::new(generation());
 let emit=|pending:&mut QueuedSourceBatch,bank:&mut Session,rows:Vec<realtime_book_catchup::Stamped>|->Result<()>{for(event,_normalized_at)in rows{pending.push_at(bank,std::slice::from_ref(&event),now())?;}Ok(())};
 for _ in 0..MAX_REALTIME_FRAMES_PER_DRAIN{
  catchup.observe_generation(generation());
  if let Err(error)=require_stream(stream){emit(&mut pending,bank,catchup.finish(&generation,false))?;return pending.finish(bank,Err(error))}let Ok(input)=stream.incoming.try_recv()else{break};match input{
  Incoming::Broadcast{event,payload,received_at}=>{
   let events=(||->Result<Vec<Value>>{if let Some(recorder)=breadth{recorder.observe(&event,&payload,&received_at)}if let Some(u)=universe{u.observe(&event,&payload,&received_at)?}Ok(source.broadcast(&event,&payload,&received_at)?)})();
   let at=now();catchup.observe_generation(generation());
   match events{Ok(events)=>{let rows=catchup.frame(events,at,realtime_book_catchup::book_only_frame(&event,&payload));emit(&mut pending,bank,rows)?;},Err(error)=>{emit(&mut pending,bank,catchup.finish(&generation,false))?;return pending.finish(bank,Err(error))}}
  },
  Incoming::Failed(code)=>{emit(&mut pending,bank,catchup.finish(&generation,false))?;return pending.finish(bank,Err(StreamFailure::new(code).into()))},
  Incoming::Joined{..}=>{emit(&mut pending,bank,catchup.finish(&generation,false))?;return pending.finish(bank,Err("LIVE_STREAM_UNEXPECTED_REJOIN".into()))}
 }}
 // Last observed account generation immediately precedes the first selected
 // native admission. Later private updates still invalidate physical authority.
 // Buffered normalization stamps are diagnostic only; actual native queue
 // admission above uses the current clock, preserving all original book clocks.
 if let Err(error)=require_stream(stream){emit(&mut pending,bank,catchup.finish(&generation,false))?;return pending.finish(bank,Err(error))}
 emit(&mut pending,bank,catchup.finish(&generation,true))?;
 pending.finish(bank,Ok(()))
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
 use moloch_live_runtime::{stream_connect_retry::{self,ConnectionFailure},stream_token_retry::{self,TokenFailure}};
 let mut token_not_before=0;
 loop{
  log.require_resolved_attempts()?;recovery_queue_clear(&t.admission_state()?)?;
  if !authority.can_read(&c.scope){return Err("LIVE_STREAM_RECOVERY_CONTROL_OR_STOP".into())}
  let marker=c.root.join("stream-auth-admission.json");
  let auth=if marker.exists(){Some(load(&marker)?["atMs"].as_u64().ok_or("LIVE_AUTH_MINT_MARKER")?)}else{None};
  let at=now();let until=stream_connect_retry::next_attempt(budget,at,auth)?.max(token_not_before);
  while now()<until{budget.validate(now())?;if !authority.can_read(&c.scope){return Err("LIVE_STREAM_RECOVERY_CONTROL_OR_STOP".into())}std::thread::sleep(Duration::from_millis(until.saturating_sub(now()).min(200)));}
  if !authority.can_read(&c.scope){return Err("LIVE_STREAM_RECOVERY_CONTROL_OR_STOP".into())}
  log.require_resolved_attempts()?;recovery_queue_clear(&t.admission_state()?)?;
  let next=stream_connect_retry::prepare_attempt(budget,now(),auth)?;
  atomic_json(&c.root.join("stream-recovery-budget.json"),&serde_json::to_value(&next)?)?;*budget=next;
  match connect_stream(t,c,bank,source){
   Ok(joined)=>return Ok(joined),
   Err(error)=>{
    // Token HTTP failures remain a distinct type; never classify Auth/JOIN,
    // source admission, arbitrary error text or financial requests as retries.
    if let Some(failure)=error.downcast_ref::<TokenFailure>(){
     let auth=load(&marker)?["atMs"].as_u64().ok_or("LIVE_AUTH_MINT_MARKER")?;
     token_not_before=stream_token_retry::after_token_failure(budget,now(),Some(auth),failure)?;
     atomic_json(&c.root.join("stream-recovery.json"),&json!({"at":iso(now()),"stage":"token_retry_wait","reason":failure.reason(),"status":failure.status(),"notBefore":iso(token_not_before),"attempts":budget.attempts(),"orderRetried":false,"coverageResetToHealthy":false}))?;
    }else if let Some(failure)=error.downcast_ref::<ConnectionFailure>(){
     let auth=load(&marker)?["atMs"].as_u64().ok_or("LIVE_AUTH_MINT_MARKER")?;
     let until=stream_connect_retry::after_connect_failure(budget,now(),Some(auth),*failure)?;
     atomic_json(&c.root.join("stream-recovery.json"),&json!({"at":iso(now()),"stage":"connection_retry_wait","reason":failure.reason(),"notBefore":iso(until),"attempts":budget.attempts(),"orderRetried":false,"coverageResetToHealthy":false}))?;
    }else{return Err(error)}
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
 require_stream(&connected)?;*stream=Some(connected);let connected=stream.as_ref().unwrap();authority.bind_stream(connected.failure_signal(),connected.deadline_ms);authority.bind_account_stream(connected.account_signal())?;Ok(())
}
pub fn main()->Result<()>{
 let args:Vec<_>=std::env::args().collect();
 // This branch reads no credentials and constructs no Transport/Session. The
 // existing OS owner contacts the already-running private control socket.
 if args.len()==5&&args[1]=="fundamental-reload"{
  let path=PathBuf::from(&args[2]);if !path.is_absolute(){return Err("LIVE_CONTROL_ABSOLUTE".into())}
  let c:Control=serde_json::from_slice(&fs::read(path)?)?;if !c.fundamental_reload{return Err("LIVE_FUNDAMENTAL_RELOAD_DISABLED".into())}
  let response=moloch_live_runtime::fundamental_reload::send(&c.root,Path::new(&args[3]),&args[4])?;
  println!("{}",serde_json::to_string(&response)?);return Ok(())
 }
 if args.len()!=3||!["preflight","run","recover-unadmitted"].contains(&args[1].as_str()){return Err("usage: moloch-live-runtime preflight|run|recover-unadmitted /absolute/control.json".into())}
 let path=PathBuf::from(&args[2]);if !path.is_absolute(){return Err("LIVE_CONTROL_ABSOLUTE".into())}let original_control_bytes=fs::read(&path)?;let c:Control=serde_json::from_slice(&original_control_bytes)?;
 if c.fundamental_reload&&c.fundamental_policy.is_none(){return Err("LIVE_FUNDAMENTAL_RELOAD_REQUIRES_BASE_POLICY".into())}
 validate_collateral_accounting(&c)?;
 validate_maker_configuration(&c)?;
 let fundamental_loaded=c.fundamental_policy.as_ref().map(fundamental::Settings::load).transpose()?;
 if let Some(settings)=&c.package_allocation{settings.validate()?;if c.packages.is_none(){return Err("LIVE_ALLOCATION_PACKAGE_SETTINGS".into())}}
 if !c.bank_name.starts_with("bank")||c.bank_name.contains(['/', '\\'])||c.predecessor_bank.as_ref().is_some_and(|p|p==&c.bank_name){return Err("LIVE_BANK_NAME".into())}
 if !c.root.is_absolute()||!c.credentials.is_absolute()||!c.owner_lock.is_absolute()||!validate_owner_capital_control(&c)?||c.activation.maximum_market_cost!=c.policy["maximumMarketCost"].as_str().unwrap_or("")||c.policy["quantity"].as_u64()!=Some(c.activation.maximum_quantity as u64)||c.policy["entryStopAt"]!=iso(c.activation.entry_until_ms){return Err("LIVE_CONTROL_POLICY_BINDING".into())}
 if let Some(settings)=&c.packages{settings.load(&c.scope)?;} if let Some(cfg)=&c.execution_model{cfg.validate()?;}
 private_dir(&c.root)?;private_dir(&c.root.join("receipts"))?;private_dir(&c.root.join("package-receipts"))?;
 let lease=Arc::new(ExclusiveLock::acquire(&c.owner_lock)?);
 if args[1]=="recover-unadmitted"{recover_unadmitted(&c)?;println!("Original SDK no-send evidence verified; STOP preserved; no network");return Ok(())}
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
  if fs::read(&path)?!=original_control_bytes{return Err("LIVE_CONTROL_CHANGED_DURING_STARTUP".into())}
  let authority=Arc::new(ProductionAuthority::new(c.activation.clone(),c.scope.clone(),vec![stop.clone(),global_stop.clone()],path.clone(),std::env::current_exe()?,lease.clone())?);
  if fs::read(&path)?!=original_control_bytes{return Err("LIVE_CONTROL_CHANGED_DURING_STARTUP".into())}
  let maker_review=c.maker_policy.as_ref().map(|settings|moloch_live_runtime::maker_policy::Reviewed::load(settings.clone(),&bank)).transpose()?;
  if let Some(review)=&maker_review{review.validate_model(c.execution_model.as_ref().ok_or("LIVE_MAKER_MODEL_REQUIRED")?)?;authority.require_maker_review(review.sha256())?;}
  let mut log=DispatchLog::open(&c.root.join("dispatch.ndjson"))?;reverify_aborted_records(&c,&mut log)?;log.require_resolved_attempts()?;
  for qid in log.aborted_records().keys().copied().collect::<Vec<_>>(){retire_proved_unadmitted(&mut bank,&log,qid)?;}
  // End the startup reader before constructing the ONE continuing worker. The
  // existing-origin baseline borrows this same transport's real quiet queue;
  // it cannot create an independent account client or infer unknown absence.
  drop(read);let mut t=transport(&c,Box::new(SharedAuthority(authority.clone())),true)?;
  // Recover a committed generation BEFORE base review installation or any
  // dispatch. Never reinstall an older base review over a newer durable cohort.
  let resumed=if c.fundamental_reload&&c.root.join("fundamental.ndjson").exists(){
   let store=Cohorts::open(&c.root,&bank,&log,lease.clone())?;
   if let Some((active,loaded))=moloch_live_runtime::fundamental_reload::Active::recover(&store,&original_control_bytes)?{
    authority.begin_fundamental_generation();authority.install_fundamental_generation(active)?;
    Some(FundamentalRuntime{cohorts:store,authority:loaded.into_authority()})
   }else{drop(store);None}
  }else{None};
  let mut fundamental=if let Some(resumed)=resumed{Some(resumed)}else if let Some(loaded)=fundamental_loaded{
   let(cohorts,review_authority,stale,existing)=open_worker_fundamental(&c,loaded,&mut bank,&mut log,lease.clone(),&t)?;
   authority.require_fundamental_exchanges(&cohorts.managed_exchanges())?;atomic_json(&c.root.join("fundamental-policy.json"),&json!({"at":iso(now()),"managedExchanges":cohorts.managed_exchanges(),"existingOriginExchanges":existing,"staleReviews":stale,"initializationDoesNotSubmit":true}))?;Some(FundamentalRuntime{cohorts,authority:review_authority})}else{
   if c.root.join("fundamental.initialized.json").exists()||c.root.join("fundamental.ndjson").exists(){return Err("LIVE_FUNDAMENTAL_SETTINGS_REQUIRED_FOR_EXISTING_COHORT".into())}None
  };
  install_managed_entry_exclusions(&c,&path,&mut bank,&mut log,fundamental.as_ref())?;
  install_human_market_breadth(&c,&path,&mut bank)?;
  // A durable official acknowledgment can be replayed after a crash before ledger commit.
  let mut startup_quotes=moloch_live_runtime::startup_ack::Index::new(&bank)?;
  for (qid,v) in log.acknowledgments.clone(){let r:Receipt=serde_json::from_value(v)?;let q=startup_quotes.quote(&bank,qid)?.clone();let e=if let Some(recovery)=log.recovered.get(&qid){feedback::recovered_acknowledgment(&q,recovery["actualOrderId"].as_str().ok_or("LIVE_RECOVERY_ACTUAL_ID")?,&c.scope.tournament_id,&r,&serde_json::from_value(recovery["fills"].clone())?,&iso(now()))?}else{feedback::acknowledgment(&q,&r,&iso(now()))?};if let Some(f)=fundamental.as_ref(){if !startup_quotes.closed_legacy(&bank,&log,qid,&e)?{f.cohorts.validate_restored_acknowledgment(&bank,&log,qid,&e)?;}}authority.bind_owned(e["actualOrderId"].as_str().ok_or("LIVE_ACTUAL_ID")?.into());if !startup_quotes.validate_closed_duplicate(&bank,&log,qid,&e)?&&restore_ack(&mut bank,&e)?{startup_quotes=moloch_live_runtime::startup_ack::Index::new(&bank)?;}}
  // Always recover original liabilities even when new package scanning is off.
  // Recovery never recreates a Staged capability or retries a physical send.
  let mut packages=moloch_live_runtime::package_dispatch::PackageDispatchLog::open(&c.root.join("package-dispatch"))?;
  recover_packages(&mut t,&c,&mut bank,&log,&authority,&mut packages)?;
  let state=t.admission_state()?;if state["state"]["unknown"].as_object().is_some_and(|m|!m.is_empty()){return Err("LIVE_QUEUE_UNKNOWN_ACCEPTANCE".into())}
  let mut owner_capital_last_check=0;advance_owner_capital(&mut t,&c,&path,&mut bank,&log,&packages,lease.as_ref(),&mut owner_capital_last_check)?;
  ensure_entry_window(&c,&mut bank,&authority)?;
  install_profitable_depth_mode(&c,&path,&mut bank,&authority,&t,&log)?;
  let fully_managed=bank.engine().report()?.get("inheritedInventory").is_some();
  let empty_positions=json!({"positions":[]});
  // Verified adoption updates the native managed-market limit while preserving
  // the original fresh-entry limit. REST coverage must use that replayed limit.
  let mut universe=if c.all_markets{
   let budget=moloch_live_runtime::throughput_discovery::ReadBudget::from_bank(&bank,&c.scope)?;
   Some(Universe::new(&c.scope.exchanges,if fully_managed{&empty_positions}else{seed.get("excludedPositionsAtTransfer").ok_or("LIVE_UNIVERSE_SEED")?},budget.maximum_managed_markets())?.with_discovery_limit(budget.maximum_discovery_reads())?.with_tournament(&c.scope.tournament_id)?)
  }else{None};
  // Previously owned holdings are filtered from the financial feed as well as discovery.
  let pairs:Value=if let Some(u)=&universe{u.pairs(&c.scope.exchanges)}else{c.scope.exchanges.iter().map(|(e,m)|json!({"exchangeId":e,"marketId":m})).collect::<Vec<_>>().into()};
  if let Err(error)=moloch_live_runtime::execution_distribution::initialize_observer(&bank,c.execution_distribution.clone()){let _=atomic_json(&c.root.join("execution-distribution.json"),&json!({"at":iso(now()),"state":"unavailable","reason":error,"executionAuthority":false}));}
  if let Err(error)=moloch_live_runtime::execution_distribution::initialize_acquisition_observer(&bank,&log,&c.scope,c.fundamental_acquisition.clone(),now()){let _=atomic_json(&c.root.join("fundamental-acquisition.json"),&json!({"at":iso(now()),"state":"unavailable","reason":error,"executionAuthority":false}));}
  if let Err(error)=moloch_live_runtime::execution_edge::initialize_observer(&bank,c.execution_model.clone()){let _=atomic_json(&c.root.join("execution-model.json"),&json!({"at":iso(now()),"state":"unavailable","reason":error,"executionAuthority":false}));}
  let start=now();let mut source_state=RestSource::new(&pairs,&c.scope.tournament_id,&iso(start))?;source_state.seed_trade_facts(bank.source_facts())?;
  let recovery_budget_path=c.root.join("stream-recovery-budget.json");let mut recovery_budget=recovery_budget(&recovery_budget_path,now())?;
  if !recovery_budget_path.exists(){atomic_json(&recovery_budget_path,&serde_json::to_value(&recovery_budget)?)?;}
  let (connected,joined_at)=connect_with_budget(&mut t,&c,&mut bank,&log,&authority,&mut source_state,&mut recovery_budget)?;require_stream(&connected)?;let mut stream=Some(connected);
  authority.bind_stream(stream.as_ref().unwrap().failure_signal(),stream.as_ref().unwrap().deadline_ms);authority.bind_account_stream(stream.as_ref().unwrap().account_signal())?;
  let mut relationship_census=None;let mut preflight_cursor=None;let mut package_last_scan=0;let mut package_certificate_cursor=0;
  let graph_rotation_path=c.root.join("relationship-graph-rotation.json");
  let(mut graph_rotation,mut graph_rotation_state_error)=match moloch_live_runtime::graph_discovery::Rotation::load(&graph_rotation_path,&c.scope){Ok(state)=>(state,None),Err(reason)=>(moloch_live_runtime::graph_discovery::Rotation::new(&c.scope),Some(reason))};
  let mut package_allocation_fairness=c.package_allocation.as_ref().map(|v|moloch_live_runtime::package_allocation::Fairness::new(v.maximum_wait)).transpose()?;
  let mut reconciliation_schedule=moloch_live_runtime::scheduling::ReconciliationSchedule::new(10_000)?;
  let pending_version=|log:&DispatchLog|moloch_live_runtime::scheduling::PendingVersion{acknowledged:log.acknowledgments.len() as u64,terminal:log.closed.len() as u64};
  let breadth_path=c.root.join(format!("breadth-{}-{}.jsonl",std::process::id(),now()));
  let mut breadth=Recorder::new_rotating(&breadth_path).ok();
  if let Some(r)=&mut breadth{r.observe("stream_reset",&json!({"localBoundary":true,"reason":"confirmed_subscription"}),&joined_at)}
  if let Some(u)=&mut universe{u.observe("stream_reset",&json!({"localBoundary":true}),&joined_at)?;}
  let mut windows:std::collections::BTreeMap<String,u64>=c.scope.exchanges.keys().map(|e|(e.clone(),start.saturating_sub(60000))).collect();
  let reload_channel=if c.fundamental_reload{Some(moloch_live_runtime::fundamental_reload::Channel::bind(&c.root,lease.clone())?)}else{None};
  let mut cycles=0u64;let mut reconciliation_cache=Reconciler::default();let mut collateral_last_check=0u64;let mut tartarus_snapshot_requests=SnapshotRequestState::default();
  let paper5_runtime=c.paper5.as_ref().and_then(|settings|match moloch_live_runtime::paper5_runtime::Runtime::load(settings){Ok(runtime)=>Some(runtime),Err(error)=>{let _=atomic_json(&c.root.join("paper5-proposals.json"),&json!({"at":iso(now()),"state":"unavailable","reason":error.to_string(),"executionAuthority":false}));None}});let mut paper5_last_evaluation=0u64;let mut paper5_last_opportunity=0u64;
  let mut owned_age_reads=moloch_live_runtime::owned_age_reads::Scheduler::new(&c.scope)?;
  loop{
   let cycle_result=(||->Result<()>{
   if stop.exists()||global_stop.exists(){return Err("LIVE_EXECUTION_STOP".into())}
   ensure_entry_window(&c,&mut bank,&authority)?;
   drain_stream(stream.as_ref().ok_or("LIVE_STREAM_REQUIRED")?,&mut bank,&mut source_state,&mut breadth,&mut universe)?;
   let mut package_blocked=recover_packages(&mut t,&c,&mut bank,&log,&authority,&mut packages)?;
   let mut reconciled=reconcile(&mut t,&c,&mut bank,&mut log,&mut reconciliation_cache,stream.as_ref().ok_or("LIVE_STREAM_REQUIRED")?,&mut source_state,&mut breadth,&mut universe)?;
   reconciliation_schedule.completed(now(),pending_version(&log),reconciled.blocks_new_submissions)?;
   advance_owner_capital(&mut t,&c,&path,&mut bank,&log,&packages,lease.as_ref(),&mut owner_capital_last_check)?;ensure_entry_window(&c,&mut bank,&authority)?;
   let mut collateral_blocked=collateral_accounting(&mut t,&c,&mut bank,&log,&packages,lease.as_ref(),&mut collateral_last_check)?;
   service_fundamental_reload(reload_channel.as_ref(),&c,&path,&original_control_bytes,&mut t,&mut bank,&mut log,&packages,&authority,lease.as_ref(),fundamental.as_mut(),stream.as_ref().ok_or("LIVE_STREAM_REQUIRED")?,&mut source_state,&mut breadth,&mut universe)?;
   accept(&mut bank,&json!({"type":"tick","receivedAt":iso(now())}))?;
   let mut required=bank.engine().required_exchanges();if let Some(f)=fundamental.as_ref(){required.extend(f.cohorts.managed_exchanges());}
   let selected=if let Some(u)=&mut universe{u.select_at(&required,now())?}else{c.scope.exchanges.keys().cloned().collect()};
   let selected=if universe.is_some(){
    let read_budget=moloch_live_runtime::throughput_discovery::ReadBudget::from_bank(&bank,&c.scope)?;
    let pending=log.acknowledgments.keys().filter(|id|!log.closed.contains(id)).copied().collect();
    let urgent=urgent_source_exchanges(&bank.engine().order_records(),&pending)?;
    let order=read_budget.order(&selected,&required,&urgent);
    let _=atomic_json(&c.root.join("discovery-schedule.json"),&json!({"at":iso(now()),"pid":std::process::id(),"nativeHead":bank.durable_head(),"maximumDiscoveryReads":read_budget.maximum_discovery_reads(),"maximumManagedMarkets":read_budget.maximum_managed_markets(),"requiredScopeCount":required.len(),"urgentOwned":urgent,"selectedScopes":order.scopes,"diagnostic":order.diagnostic,"readSchedulingOnly":true,"monetaryCapsChanged":false}));
    order.scopes
   }else{selected};
   for (selected_index,ex) in selected.iter().enumerate(){require_stream(stream.as_ref().ok_or("LIVE_STREAM_REQUIRED")?)?;let market=&c.scope.exchanges[ex];let from=windows[ex];let source_started=now();let next=source(&c,&mut bank,&mut source_state,ex,market,from,&mut LivePreflightReads{t:&mut t,c:&c,authority:&authority,stream:stream.as_ref().ok_or("LIVE_STREAM_REQUIRED")?,breadth:&mut breadth,universe:&mut universe})?;owned_age_reads.record_source_read(ex,source_started,now())?;windows.insert(ex.clone(),next);drain_stream(stream.as_ref().ok_or("LIVE_STREAM_REQUIRED")?,&mut bank,&mut source_state,&mut breadth,&mut universe)?;
    if account_reconciliation_due(stream.as_ref().ok_or("LIVE_STREAM_REQUIRED")?)||reconciliation_schedule.due(now(),pending_version(&log))?.is_some(){
     reconciled=reconcile(&mut t,&c,&mut bank,&mut log,&mut reconciliation_cache,stream.as_ref().ok_or("LIVE_STREAM_REQUIRED")?,&mut source_state,&mut breadth,&mut universe)?;
     reconciliation_schedule.completed(now(),pending_version(&log),reconciled.blocks_new_submissions)?;
     collateral_blocked=collateral_accounting(&mut t,&c,&mut bank,&log,&packages,lease.as_ref(),&mut collateral_last_check)?;
    }
    ensure_entry_window(&c,&mut bank,&authority)?;
    service_fundamental_reload(reload_channel.as_ref(),&c,&path,&original_control_bytes,&mut t,&mut bank,&mut log,&packages,&authority,lease.as_ref(),fundamental.as_mut(),stream.as_ref().ok_or("LIVE_STREAM_REQUIRED")?,&mut source_state,&mut breadth,&mut universe)?;
    if owner_capital_ready(&c,&bank)?&&!package_blocked&&!collateral_blocked&&reconciliation_schedule.permits_submission(now(),pending_version(&log))?{
     package_opportunity(&mut t,&c,&mut bank,&mut log,&authority,&mut packages,&mut relationship_census,&mut source_state,stream.as_ref().ok_or("LIVE_STREAM_REQUIRED")?,&mut breadth,&mut universe,&mut windows,&mut package_last_scan,&mut package_certificate_cursor,&mut package_allocation_fairness,&fundamental.as_ref().map(|f|f.cohorts.managed_exchanges()).unwrap_or_default())?;
    }
    package_blocked=recover_packages(&mut t,&c,&mut bank,&log,&authority,&mut packages)?;
    collateral_blocked=collateral_accounting(&mut t,&c,&mut bank,&log,&packages,lease.as_ref(),&mut collateral_last_check)?;
    // A new owned lot may age after its selected scope was already visited.
    // Nominate a bounded real source refresh without waiting for another full
    // breadth pass or requiring a maximum-age SELL draft to exist first.
    // This grants reads only; unchanged dispatch obtains its own exact facts.
    let managed=fundamental.as_ref().map(|f|f.cohorts.managed_exchanges()).unwrap_or_default();
    if let Some(hint)=owned_age_reads.nominate(&bank,&c.scope,&managed,ex,now())?{
     require_stream(stream.as_ref().ok_or("LIVE_STREAM_REQUIRED")?)?;
     let target=hint.exchange_id().to_owned();let market=hint.market_id().to_owned();
     let started=now();let from=*windows.get(&target).ok_or("LIVE_OWNED_REFRESH_WINDOW")?;
     let next=source(&c,&mut bank,&mut source_state,&target,&market,from,&mut LivePreflightReads{t:&mut t,c:&c,authority:&authority,stream:stream.as_ref().ok_or("LIVE_STREAM_REQUIRED")?,breadth:&mut breadth,universe:&mut universe})?;
     owned_age_reads.complete_read(&hint,started,now())?;windows.insert(target.clone(),next);
     let execution_clock=if hint.reason()=="new_lot_maximum_age"{Some(hint.oldest_execution_ms())}else{None};
     let _=atomic_json(&c.root.join("owned-age-refresh.json"),&json!({"at":iso(now()),"pid":std::process::id(),"exchangeId":target,"marketId":market,"reason":hint.reason(),"nativeQid":hint.native_qid(),"oldestExecutionMs":execution_clock,"dueAtMs":hint.due_at_ms(),"sourceStartedAt":iso(started),"nativeHead":bank.durable_head(),"readOnlyNomination":true,"financialPolicyChanged":false}));
     drain_stream(stream.as_ref().ok_or("LIVE_STREAM_REQUIRED")?,&mut bank,&mut source_state,&mut breadth,&mut universe)?;
     if account_reconciliation_due(stream.as_ref().ok_or("LIVE_STREAM_REQUIRED")?)||reconciliation_schedule.due(now(),pending_version(&log))?.is_some(){
      reconciled=reconcile(&mut t,&c,&mut bank,&mut log,&mut reconciliation_cache,stream.as_ref().ok_or("LIVE_STREAM_REQUIRED")?,&mut source_state,&mut breadth,&mut universe)?;
      reconciliation_schedule.completed(now(),pending_version(&log),reconciled.blocks_new_submissions)?;
     }
     ensure_entry_window(&c,&mut bank,&authority)?;
    }
    // Package preflight consumes real account time; never reuse an expired
    // ordinary reconciliation permission after those paced reads.
    let permit_new=!package_blocked&&!collateral_blocked&&reconciliation_schedule.permits_submission(now(),pending_version(&log))?;
    dispatch(&mut t,&c,&mut bank,&mut log,&authority,&mut source_state,permit_new,&mut preflight_cursor,stream.as_ref().ok_or("LIVE_STREAM_REQUIRED")?,&mut breadth,&mut universe,&windows,maker_review.as_ref(),fundamental.as_mut())?;
    if owner_capital_ready(&c,&bank)?&&!package_blocked&&!collateral_blocked&&reconciliation_schedule.permits_submission(now(),pending_version(&log))?{
     paper5_opportunity(&mut t,&c,&mut bank,&mut log,&authority,&mut packages,paper5_runtime.as_ref(),&mut source_state,stream.as_ref().ok_or("LIVE_STREAM_REQUIRED")?,&mut breadth,&mut universe,&mut windows,&mut paper5_last_opportunity,&fundamental.as_ref().map(|f|f.cohorts.managed_exchanges()).unwrap_or_default())?;
    }
    // Optional bounded inference runs after completed dispatch/accounting, with
    // no live permit held. Existing original receipts supply all observations.
    if let Some(runtime)=paper5_runtime.as_ref(){let at=now();if at.saturating_sub(paper5_last_evaluation)>=60_000{paper5_last_evaluation=at;let diagnostic=runtime.evaluate(&bank,&c.scope,&log,&paper5_nodes(&c.scope),&Default::default(),&Default::default(),at);if let Ok(value)=serde_json::to_value(diagnostic){let _=atomic_json(&c.root.join("paper5-proposals.json"),&value);}}}
    let _=atomic_json(&c.root.join("progress.json"),&json!({"at":iso(now()),"pid":std::process::id(),"phase":"trading","completedReadScopes":selected_index+1,"selectedReadScopes":selected.len(),"maximumRealtimeFramesPerDrain":MAX_REALTIME_FRAMES_PER_DRAIN,"realtimeHealthy":stream.as_ref().is_some_and(|s|s.healthy()),"actualOrders":log.acknowledgments.len(),"terminalOrders":log.closed.len(),"actualFills":bank.engine().fills().len(),"profitabilityEstablished":false}));
   }
   cycles+=1; if let Some(model)=moloch_live_runtime::execution_edge::observer_diagnostic(){let _=atomic_json(&c.root.join("execution-model.json"),&json!({"at":iso(now()),"model":model,"executionAuthority":false}));}
   atomic_json(&c.root.join("status.json"),&json!({"at":iso(now()),"pid":std::process::id(),"mode":"actual-contest-trading","runId":c.activation.run_id,"bank":c.bank_name,"cycles":cycles,"realtimeHealthy":stream.as_ref().is_some_and(|s|s.healthy()),"universe":universe.as_ref().map(Universe::status),"selectedReadScopes":selected,"ownedReconciliation":reconciled,"sourceScopes":source_state.scopes(),"breadthCapture":{"path":breadth_path,"state":breadth.as_ref().map(Recorder::status)},"report":bank.engine().report()?,"queue":t.admission_state()?,"actualOrders":log.acknowledgments.len(),"terminalOrders":log.closed.len(),"entryAnchor":iso(c.activation.entry_until_ms),"entryUntil":iso(authority.entry_until_ms()),"profitabilityEstablished":false}))?;
   service_tartarus_snapshot(&c,&path,&original_control_bytes,&bank,&t,&log,&packages,lease.as_ref(),&authority,fundamental.as_ref(),&mut tartarus_snapshot_requests);
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
   if let Some(model)=moloch_live_runtime::execution_distribution::acquisition_observer_diagnostic(&bank,&log,now()){
    // Keep each cold-start cohort's latest bounded view; a restart must not
    // overwrite the preceding cohort or pretend its pending orders were zero.
    if let Some(cohort)=model["cohortSha256"].as_str(){let _=atomic_json(&c.root.join(format!("fundamental-acquisition-{cohort}.json")),&model);}
   }
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
#[cfg(test)]mod defensive_cancel_worker_tests{
 use super::*;
 const T:&str="11111111-1111-4111-8111-111111111111";
 struct F{root:PathBuf,c:Control,bank:Session,log:DispatchLog,source:RestSource,qid:u64,at:u64}
 impl Drop for F{fn drop(&mut self){let _=fs::remove_dir_all(&self.root);}}
 fn put(bank:&mut Session,e:&Value){let n=bank.last_source_sequence("defense-test")+1;bank.accept("defense-test",n,e).unwrap();}
 fn book(at:u64,seq:u64,weak:bool)->Value{json!({"type":"book","receivedAt":iso(at),"book":{"tournamentId":T,"marketId":"201","exchangeId":"101","sequence":seq.to_string(),"sourceAt":iso(at),"requestStartedAt":iso(at),"receivedAt":iso(at),"transport":"official_sig_realtime","bids":[{"price":"0.400000","quantity":1000}],"asks":[{"price":"0.450000","quantity":if weak{100000}else{1000}}]}})}
 impl F{
  fn new()->Self{let at=now();let root=std::env::temp_dir().join(format!("worker-defense-{}-{}",std::process::id(),SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));private_dir(&root).unwrap();
   let policy=json!({"quantity":4,"minimumQuoteAgeMs":60000,"lifetimeMs":600000,"entryStopAt":iso(at+3600000)});let seed=json!({"runId":"worker-defense","availableCash":"1000.000000","accountSnapshotHash":"a".repeat(64),"observedAt":iso(at-6000)});
   let mut bank=Session::open_live(&root.join("bank"),&policy,&seed).unwrap();put(&mut bank,&book(at-5000,1,false));let q=bank.engine().order_records().into_iter().find(|q|q["side"]=="YES").unwrap();let qid=q["id"].as_u64().unwrap();let mut log=DispatchLog::open(&root.join("dispatch.ndjson")).unwrap();
   let r=Receipt{endpoint:"/orders".into(),request_started_at:iso(at-2000),received_at:iso(at-1900),status:200,data:json!({"orderId":"991","exchangeId":"101","side":"yes","action":"buy","quantity":q["quantity"],"price":q["price"]})};
   for row in [json!({"kind":"intent","qid":qid,"quote":q,"fundamental":null,"at":iso(at-2100)}),json!({"kind":"attempt","qid":qid,"at":iso(at-2100)}),json!({"kind":"ack","qid":qid,"receipt":r})]{log.append(row).unwrap();}put(&mut bank,&feedback::acknowledgment(&q,&r,&iso(at-1900)).unwrap());put(&mut bank,&book(at-1000,2,true));
   let c=serde_json::from_value(json!({"activation":{"runId":"worker-defense","entryUntilMs":at+3600000,"maximumCommitted":"40.000000","maximumMarketCost":"10.000000","maximumQuantity":4,"tournamentId":T,"tournamentSlug":"cup","binarySha256":"a".repeat(64)},"scope":{"tournamentId":T,"tournamentSlug":"cup","exchanges":{"101":"201"}},"policy":policy,"root":root,"credentials":"/unused","ownerLock":root.join("owner.lock"),"accountScope":"offline-defense"})).unwrap();let source=RestSource::new(&json!([{"exchangeId":"101","marketId":"201"}]),T,&iso(at-1000)).unwrap();Self{root,c,bank,log,source,qid,at}
  }
  fn q(&self)->Value{quote(&self.bank,self.qid).unwrap()}
 }
 struct Reads{at:u64,q:Value,calls:usize,allowed:bool,fill:Option<u64>,improve:bool,gap:bool,read:Vec<Value>,returned:Vec<Value>}
 impl Reads{fn new(f:&F)->Self{Self{at:f.at,q:f.q(),calls:0,allowed:true,fill:None,improve:false,gap:false,read:vec![],returned:vec![]}}}
 impl OrdinaryPreflightReads for Reads{
  fn now_ms(&self)->u64{self.at+self.calls as u64}
  fn guarded(&mut self)->Result<bool>{Ok(self.allowed)}
  fn service_before_read(&mut self,bank:&mut Session,_source:&mut RestSource)->Result<()>{self.calls+=1;if self.calls==2{let at=self.now_ms();if let Some(n)=self.fill{put(bank,&json!({"type":"actual_fill","source":"official_sig_api_v1","receiptHash":"c".repeat(64),"orderId":self.q["id"],"actualOrderId":"991","marketId":"201","exchangeId":"101","side":"YES","action":"buy","quantity":n,"price":self.q["price"],"fillId":"881","fee":"0.000000","executedAt":iso(at),"receivedAt":iso(at)}));}if self.improve{put(bank,&book(at,3,false));}if self.gap{put(bank,&json!({"type":"gap","exchangeId":"101","receivedAt":iso(at)}));}}Ok(())}
  fn read(&mut self,ep:Endpoint)->Result<Receipt>{self.read.push(serde_json::to_value(&ep)?);let at=iso(self.now_ms());let(path,data)=match ep{Endpoint::Tournament=>("/tournaments/cup".into(),json!({"id":T,"slug":"cup","status":"active"})),Endpoint::Positions=>("/tournaments/cup/portfolio/positions".into(),json!({"positions":[]})),Endpoint::Orders{..}=>(format!("/orders?status=open&limit=200&tournamentId={T}"),json!({"data":[],"coverage":{"complete":true},"pagination":{"hasMore":false}})),Endpoint::Account=>(format!("/account?tournamentId={T}"),json!({"balance":1000,"availableAdvance":0,"buyingPower":1000})),_=>return Err("DEFENSE_EXTRA_REQUEST".into())};let r=Receipt{endpoint:path,request_started_at:at.clone(),received_at:at,status:200,data};self.returned.push(serde_json::to_value(&r)?);Ok(r)}
 }
 #[test]fn default_off_never_selects_or_reads_even_with_a_real_below_edge_owned_order(){let mut f=F::new();assert!(!f.c.defensive_cancel);assert!(defensive_cancel_candidate(&f.c,&f.bank,&f.log,&Default::default(),None,f.at).unwrap().is_none());let mut reads=Reads::new(&f);assert!(defensive_cancel_preflight(&f.c,&mut f.bank,&f.log,&mut f.source,&Default::default(),f.qid,&mut reads).unwrap().is_none());assert!(reads.read.is_empty());f.c.defensive_cancel=true;assert!(defensive_cancel_candidate(&f.c,&f.bank,&f.log,&Default::default(),None,f.at).unwrap().is_some());}
 #[test]fn real_partial_fill_during_four_original_reads_preserves_exact_remaining_quantity(){let mut f=F::new();f.c.defensive_cancel=true;let q=f.q();let mut reads=Reads::new(&f);reads.fill=Some(1);let(decision,facts)=defensive_cancel_preflight(&f.c,&mut f.bank,&f.log,&mut f.source,&Default::default(),f.qid,&mut reads).unwrap().unwrap();assert_eq!(reads.calls,4);assert_eq!(reads.read,serde_json::to_value([Endpoint::Tournament,Endpoint::Positions,Endpoint::Orders{status:"open".into()},Endpoint::Account]).unwrap().as_array().unwrap().clone());assert_eq!(reads.returned,serde_json::to_value([facts.account,facts.positions,facts.open_orders,facts.cash_balance]).unwrap().as_array().unwrap().clone());assert_eq!(decision.evidence().filled_quantity,1);assert_eq!(decision.remaining_quantity(),3);assert_eq!(f.q()["filled"],1);assert_eq!(f.q()["price"],q["price"]);assert_eq!(f.q()["expiresAt"],q["expiresAt"]);assert!(!f.log.cancel_attempted(f.qid));}
 #[test]fn filled_improved_or_gapped_during_packet_defers_without_a_cancel_attempt(){for case in ["filled","improved","gap"]{let mut f=F::new();f.c.defensive_cancel=true;let mut reads=Reads::new(&f);match case{"filled"=>reads.fill=Some(4),"improved"=>reads.improve=true,_=>reads.gap=true};assert!(defensive_cancel_preflight(&f.c,&mut f.bank,&f.log,&mut f.source,&Default::default(),f.qid,&mut reads).unwrap().is_none());assert_eq!(reads.read.len(),4);assert!(!f.log.cancel_attempted(f.qid)&&!f.log.cancel_acknowledged.contains(&f.qid)&&!f.log.closed.contains(&f.qid));}}
 #[test]fn unknown_guard_and_expired_signal_do_not_buy_a_packet(){for expired in [false,true]{let mut f=F::new();f.c.defensive_cancel=true;let mut reads=Reads::new(&f);if expired{reads.at=f.at+9000}else{reads.allowed=false}assert!(defensive_cancel_preflight(&f.c,&mut f.bank,&f.log,&mut f.source,&Default::default(),f.qid,&mut reads).unwrap().is_none());assert!(reads.read.is_empty());assert!(!f.log.cancel_attempted(f.qid));}assert_eq!(defensive_cancel_window(10000,7000,2500),Some(10000));assert_eq!(defensive_cancel_window(10000,7500,2500),None);assert_eq!(defensive_cancel_window(u64::MAX,u64::MAX-1,2500),None);}
 #[test]fn unmatched_attempt_blocks_both_cancel_lanes_after_real_log_restart(){let mut f=F::new();f.c.defensive_cancel=true;assert!(cancel_may_start(&f.log,f.qid));f.log.append(json!({"kind":"cancel_attempt","qid":f.qid,"at":iso(f.at)})).unwrap();let other=DispatchLog::open(&f.root.join("dispatch.ndjson")).unwrap();assert!(!cancel_may_start(&other,f.qid));assert!(defensive_cancel_candidate(&f.c,&f.bank,&other,&Default::default(),None,f.at).unwrap().is_none());assert!(!other.cancel_acknowledged.contains(&f.qid)&&!other.closed.contains(&f.qid));assert!(!other.never_attempted(f.qid));}
 #[test]fn cancel_ack_keeps_reservation_until_actual_racing_fill_and_complete_terminal(){let mut f=F::new();let q=f.q();f.log.append(json!({"kind":"cancel_attempt","qid":f.qid,"at":iso(f.at)})).unwrap();let before=f.bank.engine().report().unwrap();let head=f.bank.durable_head();fs::create_dir_all(f.c.root.join("receipts")).unwrap();let ack=Receipt{endpoint:"/orders/991".into(),request_started_at:iso(f.at),received_at:iso(f.at+1),status:200,data:json!({"orderId":"991","tournamentId":T})};record_cancel_ack(&f.c,&mut f.log,f.qid,&ack).unwrap();assert_eq!(head,f.bank.durable_head());assert_eq!(before,f.bank.engine().report().unwrap());assert!(f.log.cancel_acknowledged.contains(&f.qid)&&!f.log.closed.contains(&f.qid));assert_eq!(f.q()["retired"],false);
  let order=Receipt{endpoint:"/orders/991".into(),request_started_at:iso(f.at+2),received_at:iso(f.at+3),status:200,data:json!({"id":"991","exchangeId":"101","tournamentId":T,"side":"yes","action":"buy","priceLimit":0.4,"quantity":4,"quantityFilled":1,"open":false,"terminalReasonCode":"Canceled"})};let fills=Receipt{endpoint:"/orders/991/fills?limit=200".into(),request_started_at:iso(f.at+4),received_at:iso(f.at+5),status:200,data:json!({"orderId":"991","exchangeId":"101","tournamentId":T,"coverage":{"complete":true,"projectedThroughSequence":100},"pagination":{"hasMore":false,"nextCursor":null},"totalQuantityFilled":1,"data":[{"id":"882","quantity":1,"side":"yes","price":0.4,"filledAt":iso(f.at+1)}]})};for e in feedback::fills(&q,"991",T,&fills,&iso(f.at+6)).unwrap(){put(&mut f.bank,&e);}assert_eq!(f.q()["filled"],1);assert_eq!(f.q()["retired"],false);let reserved=decimal_units(f.bank.engine().report().unwrap()["reservedCash"].as_str().unwrap()).unwrap();let e=feedback::terminal(&q,"991",T,&order,&fills,&iso(f.at+6)).unwrap().unwrap();put(&mut f.bank,&e);f.log.append(json!({"kind":"closed","qid":f.qid,"orderReceiptHash":feedback::digest(&order).unwrap(),"fillReceiptHash":feedback::digest(&fills).unwrap()})).unwrap();assert!(f.log.closed.contains(&f.qid));assert_eq!(f.q()["retired"],true);assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::Yes),1);assert!(decimal_units(f.bank.engine().report().unwrap()["reservedCash"].as_str().unwrap()).unwrap()<reserved);
 }
}
#[cfg(test)]mod ordinary_side_edge_worker_tests{
 use super::*;
 use moloch_live_runtime::ordinary_side_edge::Mode;
 struct Temp(PathBuf);impl Drop for Temp{fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}
 const T:&str="11111111-1111-4111-8111-111111111111";
 fn fixture()->(Temp,Control,Session,DispatchLog,Value,Receipt){fixture_with_hedge(false)}
 fn fixture_with_hedge(retain:bool)->(Temp,Control,Session,DispatchLog,Value,Receipt){
  let at=now();let root=Temp(std::env::temp_dir().join(format!("ordinary-side-edge-{}-{}",std::process::id(),SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos())));private_dir(&root.0).unwrap();
  let policy=json!({"quantity":2,"entryStopAt":iso(at+600000),"retainProfitableHedge":retain,"maximumHedgeOvershoot":1});let seed=json!({"runId":"side-edge-fixture","availableCash":"1000.000000","accountSnapshotHash":"a".repeat(64),"observedAt":iso(at-5000)});
  let mut bank=Session::open_live(&root.0.join("bank"),&policy,&seed).unwrap();let stamp=iso(at-4000);
  bank.accept("original-fixture-book",1,&json!({"type":"book","receivedAt":stamp,"book":{"tournamentId":T,"marketId":"201","exchangeId":"101","sequence":"1","sourceAt":stamp,"requestStartedAt":stamp,"receivedAt":stamp,"bids":[{"price":"0.400000","quantity":10}],"asks":[{"price":"0.410000","quantity":990}]}})).unwrap();
  let q=bank.engine().order_records().into_iter().find(|q|q["action"]=="buy"&&q["side"]=="YES"&&q["retired"]==false).unwrap();
  let c:Control=serde_json::from_value(json!({"activation":{"runId":"side-edge-fixture","entryUntilMs":at+600000,"maximumCommitted":"40.000000","maximumMarketCost":"10.000000","maximumQuantity":2,"tournamentId":T,"tournamentSlug":"cup","binarySha256":"a".repeat(64)},"scope":{"tournamentId":T,"tournamentSlug":"cup","exchanges":{"101":"201"}},"policy":policy,"root":root.0,"credentials":"/unused","ownerLock":root.0.join("owner.lock"),"accountScope":"offline-side-edge"})).unwrap();
  let r=Receipt{endpoint:format!("/markets/201/orderbook?tournamentId={T}&depth=20"),request_started_at:stamp.clone(),received_at:stamp.clone(),status:200,data:json!({"contexts":[{"type":"tournament","tournament":{"id":T},"orderbook":{"exchanges":[{"exchangeId":"101","asOf":{"sequence":1,"at":stamp},"bids":[{"price":0.4,"quantity":10}],"asks":[{"price":0.41,"quantity":990}]}]}}]})};
  let log=DispatchLog::open(&root.0.join("dispatch.ndjson")).unwrap();(root,c,bank,log,q,r)
 }
 #[test]fn old_control_disabled_does_not_assess_or_write_a_diagnostic(){
  let(root,c,bank,log,q,mut r)=fixture();assert_eq!(c.ordinary_side_edge,Mode::Disabled);r.data=Value::Null;let head=bank.durable_head();
  assert!(ordinary_side_edge_allows(&c,&bank,&log,&q,&r,false,now()));assert!(!root.0.join("ordinary-side-edge.json").exists());assert_eq!(bank.durable_head(),head);assert!(log.intents.is_empty()&&log.attempted.is_empty());
  let mut value=serde_json::to_value(c).unwrap();value["ordinarySideEdge"]=json!("enabled");assert!(serde_json::from_value::<Control>(value).is_err());
 }
 #[test]fn early_weak_side_removed_and_strong_side_gets_same_pass_without_http_or_budget(){
  let(_root,c,bank,log,_,_)=fixture();let at=now();let quotes:Vec<_>=bank.engine().order_records().into_iter().filter(|q|q["retired"]==false&&q["action"]=="buy").collect();assert_eq!(quotes.len(),2);
  let mut physical:Vec<_>=quotes.iter().enumerate().map(|(i,q)|(format!("101:{}:buy",q["side"].as_str().unwrap()),i,false)).collect();
  let head=bank.durable_head();let report=bank.engine().report().unwrap();let descriptor=log.descriptor_identity().unwrap();let mut budget=moloch_live_runtime::scheduling::PreflightBudget::default();
  let diagnostic=early_native_side_filter(Mode::Enforce,&bank,&c.scope,&log,&Default::default(),&quotes,&mut physical,at);
  assert_eq!(physical.len(),1);assert_eq!(quotes[physical[0].1]["side"],"NO");assert!(diagnostic.iter().any(|a|a["vetoedThisPass"]==true));
  let Some((SharedPreflight::Physical(i),_))=shared_preflight_rotation(&[],&[],&physical,None)else{panic!("strong side must retain this pass")};assert_eq!(quotes[i]["side"],"NO");assert!(budget.try_start());
  assert_eq!(head,bank.durable_head());assert_eq!(report,bank.engine().report().unwrap());assert_eq!(descriptor,log.descriptor_identity().unwrap());assert!(log.intents.is_empty()&&log.attempted.is_empty());
 }
 #[test]fn early_disabled_shadow_urgent_managed_and_unavailable_do_not_remove_candidates(){
  let(_root,c,bank,log,q,_)=fixture();let physical=vec![("101:YES:buy".into(),0,false)];
  for mode in [Mode::Disabled,Mode::Shadow]{let mut p=physical.clone();let d=early_native_side_filter(mode,&bank,&c.scope,&log,&Default::default(),std::slice::from_ref(&q),&mut p,now());assert_eq!(p,physical);if mode==Mode::Disabled{assert!(d.is_empty());}}
  for (managed,urgent,at) in [(std::collections::BTreeSet::from(["101".into()]),false,now()),(Default::default(),true,now()),(Default::default(),false,now()+20000)]{
   let mut p=vec![("101:YES:buy".into(),0,urgent)];let before=p.clone();early_native_side_filter(Mode::Enforce,&bank,&c.scope,&log,&managed,std::slice::from_ref(&q),&mut p,at);assert_eq!(p,before);
  }
 }
 #[test]fn actual_native_quote_shadow_records_but_enforce_defers_without_a_financial_event(){
  let(root,mut c,bank,log,q,r)=fixture();let head=bank.durable_head();let report=bank.engine().report().unwrap();let quote_before=q.clone();
  c.ordinary_side_edge=Mode::Shadow;assert!(ordinary_side_edge_allows(&c,&bank,&log,&q,&r,false,now()));let d=load(&root.0.join("ordinary-side-edge.json")).unwrap();assert_eq!(d["assessment"]["class"],"below_minimum");assert_eq!(d["deferred"],false);assert_eq!(d["assessment"]["evidence"]["sideEdgeMicros"],100);
  c.ordinary_side_edge=Mode::Enforce;assert!(!ordinary_side_edge_allows(&c,&bank,&log,&q,&r,false,now()));let d=load(&root.0.join("ordinary-side-edge.json")).unwrap();assert_eq!(d["deferred"],true);assert_eq!(d["physicalAttempted"],false);
  assert_eq!(q,quote_before);assert_eq!(quote(&bank,q["id"].as_u64().unwrap()).unwrap(),q);assert_eq!(bank.durable_head(),head);assert_eq!(bank.engine().report().unwrap(),report);assert!(log.intents.is_empty()&&log.attempted.is_empty());
 }
 #[test]fn unavailable_is_a_defer_only_in_enforce_and_managed_cohorts_bypass(){
  let(root,mut c,bank,log,q,mut r)=fixture();r.endpoint.push_str("&forged=1");c.ordinary_side_edge=Mode::Enforce;assert!(!ordinary_side_edge_allows(&c,&bank,&log,&q,&r,false,now()));let d=load(&root.0.join("ordinary-side-edge.json")).unwrap();assert_eq!(d["assessment"]["class"],"unavailable");
  c.ordinary_side_edge=Mode::Shadow;assert!(ordinary_side_edge_allows(&c,&bank,&log,&q,&r,false,now()));fs::remove_file(root.0.join("ordinary-side-edge.json")).unwrap();c.ordinary_side_edge=Mode::Enforce;assert!(ordinary_side_edge_allows(&c,&bank,&log,&q,&r,true,now()));assert!(!root.0.join("ordinary-side-edge.json").exists());assert!(log.intents.is_empty()&&log.attempted.is_empty());
 }
 #[test]fn genuine_opposite_owned_fills_bypass_only_when_the_entire_buy_is_covered(){
  for filled in [1u64,2]{let(root,mut c,mut bank,log,q,book)=fixture_with_hedge(true);c.ordinary_side_edge=Mode::Enforce;
   let mate=bank.engine().order_records().into_iter().find(|q|q["action"]=="buy"&&q["side"]=="NO"&&q["retired"]==false).unwrap();let at=now();
   let ack=Receipt{endpoint:"/orders".into(),request_started_at:iso(at-2000),received_at:iso(at-1000),status:200,data:json!({"orderId":"9001","exchangeId":"101","side":"no","action":"buy","quantity":2,"price":0.590})};
   let event=feedback::acknowledgment(&mate,&ack,&iso(at-1000)).unwrap();bank.accept("original-owned-fixture",1,&event).unwrap();
   let tape=Receipt{endpoint:"/orders/9001/fills?limit=200".into(),request_started_at:iso(at-100),received_at:iso(at),status:200,data:json!({"orderId":"9001","tournamentId":T,"exchangeId":"101","coverage":{"complete":true},"pagination":{"hasMore":false,"nextCursor":null,"limit":200},"totalQuantityFilled":-(filled as i64),"data":[{"id":"8001","side":"no","quantity":-(filled as i64),"price":0.590,"filledAt":iso(at-500)}]})};
   let events=feedback::fills(&mate,"9001",T,&tape,&iso(at)).unwrap();assert_eq!(events.len(),1);bank.accept("original-owned-fixture",2,&events[0]).unwrap();
   assert_eq!(bank.engine().wallet.quantity("101",moloch_replay::Side::No),filled);let current=quote(&bank,q["id"].as_u64().unwrap()).unwrap();assert_eq!(current["quantity"],2);assert!(!current["cancelEffectiveAt"].is_u64());let head=bank.durable_head();
   assert_eq!(ordinary_side_edge_allows(&c,&bank,&log,&current,&book,false,now()),filled==2);assert_eq!(unreviewed_inventory_reduction(&c,&bank,&log,&current).unwrap(),filled==2);let mut changed=current.clone();changed["quantity"]=json!(1);assert!(!unreviewed_inventory_reduction(&c,&bank,&log,&changed).unwrap());assert_eq!(bank.durable_head(),head);
   if filled==1{assert_eq!(load(&root.0.join("ordinary-side-edge.json")).unwrap()["assessment"]["class"],"below_minimum");}else{assert!(!root.0.join("ordinary-side-edge.json").exists());}
  }
 }
}
#[cfg(test)]mod cooperative_preflight_tests{
 use super::*;
 // Real Session-generated quotes and original typed receipt fixtures; no
 // network, extra client or fabricated native qid.
 struct Temp(PathBuf);impl Drop for Temp{fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}
 const T:&str="11111111-1111-4111-8111-111111111111";
 fn fixture()->(Temp,Control,Session,DispatchLog,RestSource,Value,Receipt){fixture_at(now())}
 fn fixture_at(at:u64)->(Temp,Control,Session,DispatchLog,RestSource,Value,Receipt){
  let root=Temp(std::env::temp_dir().join(format!("cooperative-preflight-{}-{}",std::process::id(),SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos())));private_dir(&root.0).unwrap();
  let policy=json!({"quantity":2,"entryStopAt":iso(at+600000)});let seed=json!({"runId":"cooperative-preflight","availableCash":"1000.000000","accountSnapshotHash":"a".repeat(64),"observedAt":iso(at-5000)});let stamp=iso(at-4000);
  let book=Receipt{endpoint:format!("/markets/201/orderbook?tournamentId={T}&depth=20"),request_started_at:stamp.clone(),received_at:stamp.clone(),status:200,data:json!({"contexts":[{"type":"tournament","tournament":{"id":T},"orderbook":{"exchanges":[{"exchangeId":"101","asOf":{"sequence":1,"at":stamp},"bids":[{"price":0.4,"quantity":10}],"asks":[{"price":0.41,"quantity":990}]}]}}]})};
  let mut bank=Session::open_live(&root.0.join("bank"),&policy,&seed).unwrap();bank.accept("original-fixture-book",1,&json!({"type":"book","receivedAt":stamp,"book":{"tournamentId":T,"marketId":"201","exchangeId":"101","sequence":"1","sourceAt":stamp,"requestStartedAt":stamp,"receivedAt":stamp,"bids":[{"price":"0.400000","quantity":10}],"asks":[{"price":"0.410000","quantity":990}]}})).unwrap();
  let mut source=RestSource::new(&json!([{"marketId":"201","exchangeId":"101"},{"marketId":"202","exchangeId":"102"}]),T,&iso(at-4001)).unwrap();
  // Restore the real source adapter's initial book+complete empty-tape state.
  // Its initial blocked book is not admitted as a new current bank decision.
  let wrap=json!({"kind":"book","source":"official_sig_api_v1","status":200,"marketId":"201","exchangeId":"101","tournamentId":T,"requestStartedAt":book.request_started_at,"receivedAt":book.received_at,"endpoint":book.endpoint,"data":book.data});source.accept(&wrap).unwrap();
  let tape=json!({"kind":"trades","source":"official_sig_api_v1","status":200,"marketId":"201","exchangeId":"101","tournamentId":T,"requestStartedAt":stamp,"receivedAt":stamp,"endpoint":format!("/exchanges/101/trades?tournamentId={T}&from={}&to={}",iso(at-4002),stamp),"data":{"marketId":"201","exchangeId":"101","from":iso(at-4002),"to":stamp,"data":[],"pagination":{"hasMore":false,"nextCursor":null},"coverage":{"complete":true,"projectedThroughSequence":1}}});accept_source_events(&mut bank,&source.accept(&tape).unwrap()).unwrap();
  let q=bank.engine().order_records().into_iter().find(|q|q["action"]=="buy"&&q["side"]=="YES"&&q["exchangeId"]=="101"&&q["retired"]==false&&q.get("cancelEffectiveAt").is_none_or(Value::is_null)).unwrap();
  let c:Control=serde_json::from_value(json!({"activation":{"runId":"cooperative-preflight","entryUntilMs":at+600000,"maximumCommitted":"40.000000","maximumMarketCost":"10.000000","maximumQuantity":2,"tournamentId":T,"tournamentSlug":"cup","binarySha256":"a".repeat(64)},"scope":{"tournamentId":T,"tournamentSlug":"cup","exchanges":{"101":"201","102":"202"}},"policy":policy,"root":root.0,"credentials":"/unused","ownerLock":root.0.join("owner.lock"),"accountScope":"offline-cooperative-preflight"})).unwrap();
  let log=DispatchLog::open(&root.0.join("dispatch.ndjson")).unwrap();(root,c,bank,log,source,q,book)
 }
 fn periodic_events(path:&Path)->Vec<Value>{fs::read_to_string(path).unwrap().lines().map(|line|serde_json::from_str::<Value>(line).unwrap()["event"].clone()).collect()}
 fn fundamental_fixture(c:&Control,bank:&Session,log:&DispatchLog,at:u64,revoke:bool)->FundamentalRuntime{
  // Synthetic, source-bound review for the real Session quote above. The
  // terminal objective makes this a genuine eligible new-entry cohort.
  let node=json!({"market_id":"201","root":{"node_type":"contract","node_id":"301","contract_id":"301","contract_type":"Election Outcome","contract_details":{"contractType":"Election Outcome","resolutionType":"Party Winner","winnerName":"Republican Party","electionDate":"2026-11-03","officeLevel":"Federal","usState":"Synthetic","raceId":1,"stageId":2,"raceName":"Synthetic fixture","raceStage":"General"},"settled_with":null,"settlement_date":iso(at+300000),"title":"Synthetic Republican Party Winner fixture"}});
  let originals=[b"synthetic forecast, not a publisher".to_vec(),b"synthetic official ballot".to_vec(),serde_json::to_vec(&node).unwrap()];
  let hashes=originals.iter().map(|b|format!("{:x}",Sha256::digest(b))).collect::<Vec<_>>();
  let sources=["forecast","official_ballot","official_contract_node"].iter().enumerate().map(|(i,kind)|json!({"kind":kind,"url":format!("https://fixture.invalid/{kind}"),"sha256":hashes[i],"publishedAtMs":at-100,"observedAtMs":at-50,"maximumAgeMs":180000})).collect::<Vec<_>>();
  let review:fundamental::ReviewInput=serde_json::from_value(json!({"version":2,"reviewerId":"test-operator","assumptionLabel":"Synthetic worker fixture; party mapping and probability are assumptions, not calibration","runId":c.activation.run_id,"bankManifestSha256":bank.manifest_hash(),"tournamentId":T,"marketId":"201","exchangeId":"101","contractId":"301","side":"YES","officialContractRootSha256":fundamental::official_contract_root_sha256(&node).unwrap(),"ordinaryProbability":"0.950000","exceptionalProbabilityAllowance":"0.010000","minimumExceptionalPayout":"0.000000","marginPerShare":"0.010000","maximumPrincipal":"4.000000","maximumQuantity":2,"reviewedAtMs":at,"expiresAtMs":at+180000,"holdUntilMs":at+600000,"payoutMappingIsOperatorAssumption":true,"revokeExisting":revoke,"sources":sources,"valuationObjective":{"kind":"terminalSettlement","timingSourceSha256":hashes[2],"timingJsonPointer":"/root/settlement_date","scheduledSettlementAtMs":at+300000,"delayPolicy":"holdForActualSettlement"}})).unwrap();
  let trusted:std::collections::BTreeSet<String>=[fundamental::review_sha256(&review).unwrap()].into();
  let bytes=originals.iter().enumerate().map(|(i,b)|fundamental::SourceBytes{sha256:&hashes[i],bytes:b}).collect::<Vec<_>>();let reviewed=fundamental::Reviewed::verify(review,&bytes,&trusted,at).unwrap();
  let owner=Arc::new(ExclusiveLock::acquire(&c.owner_lock).unwrap());let mut cohorts=Cohorts::initialize(&c.root,bank,log,owner).unwrap();cohorts.install(reviewed,bank,log,at).unwrap();
  let authority_bytes=serde_json::to_vec(&json!({"version":1,"trustedReviewSha256":trusted})).unwrap();let authority_path=c.root.join("review-authority.json");fs::write(&authority_path,&authority_bytes).unwrap();
  FundamentalRuntime{cohorts,authority:ReviewAuthority::load(&authority_path,&format!("{:x}",Sha256::digest(&authority_bytes))).unwrap()}
 }
 #[test]fn fundamental_preintent_catches_actual_bind_refusal_before_dispatch(){
  use std::cell::Cell;
  let at=now()-1000;let(root,c,bank,log,_source,q,_book)=fixture_at(at);let mut f=fundamental_fixture(&c,&bank,&log,at,false);let qid=q["id"].as_u64().unwrap();
  assert!(f.eligible(&bank,&log,&c.scope,qid,at).unwrap());let n=f.cohorts.planned_entry_quantity(&bank,&log,&c.scope,qid,f.authority.trusted(),at).unwrap().unwrap();let(_,request,permit)=physical_order(&c,&q).unwrap();
  let stale=at+6001; // Original book at at-4000 is now 10001ms old.
  assert_eq!(f.cohorts.bind_quantity(&bank,&log,&c.scope,qid,n,&request,f.authority.trusted(),stale).err().expect("actual bind must refuse stale source").to_string(),"FUNDAMENTAL_NOT_ELIGIBLE");
  let head=bank.durable_head();let financial=bank.engine().report().unwrap();let dispatch=fs::read(root.0.join("dispatch.ndjson")).unwrap();let sidecar=fs::read(root.0.join("fundamental.ndjson")).unwrap();let clocks=Cell::new(0);let queues=Cell::new(0);
  let result=fundamental_preintent(&mut f,&bank,&log,&c.scope,&q,n,&request,&permit,||{clocks.set(clocks.get()+1);stale},||{queues.set(queues.get()+1);recovery_queue_clear(&json!({"state":{"unknown":{},"active":null,"manual_review":false}}))}).unwrap();
  assert!(matches!(result,FundamentalPreintent::Deferred(reason)if reason=="FUNDAMENTAL_NOT_ELIGIBLE"));assert_eq!(clocks.get(),2,"bind then defer proof; physical mint is not reached");assert_eq!(queues.get(),1);
  assert_eq!(bank.durable_head(),head);assert_eq!(bank.engine().report().unwrap(),financial);assert_eq!(quote(&bank,qid).unwrap(),q);assert_eq!(fs::read(root.0.join("dispatch.ndjson")).unwrap(),dispatch);assert_eq!(fs::read(root.0.join("fundamental.ndjson")).unwrap(),sidecar);
  assert!(f.cohorts.durable_tag(qid).is_none());assert!(log.intents.is_empty()&&log.attempted.is_empty()&&log.acknowledgments.is_empty());assert!(bank.engine().live_order_bindings().is_empty()&&bank.engine().fills().is_empty());
 }
 #[test]fn fundamental_preintent_real_bind_mints_when_fresh_and_preserves_binding_if_mint_expires(){
  use std::cell::Cell;
  for expire_mint in [false,true]{
   let at=now()-1000;let(root,c,bank,log,_source,q,_book)=fixture_at(at);let mut f=fundamental_fixture(&c,&bank,&log,at,false);let qid=q["id"].as_u64().unwrap();let n=q["quantity"].as_u64().unwrap();let(_,request,permit)=physical_order(&c,&q).unwrap();assert!(f.eligible(&bank,&log,&c.scope,qid,at).unwrap());
   let head=bank.durable_head();let financial=bank.engine().report().unwrap();let dispatch=fs::read(root.0.join("dispatch.ndjson")).unwrap();let clocks=Cell::new(0);let queues=Cell::new(0);
   let result=fundamental_preintent(&mut f,&bank,&log,&c.scope,&q,n,&request,&permit,||{let n=clocks.get();clocks.set(n+1);if expire_mint&&n>0{at+6001}else{at}},||{queues.set(queues.get()+1);recovery_queue_clear(&json!({"state":{"unknown":{},"active":null,"manual_review":false}}))}).unwrap();
   match result{
    FundamentalPreintent::Ready(decision)=>{assert!(!expire_mint);assert_eq!(decision.planned_quantity(),n);decision.validate_frozen(&q,&financial,qid,at).unwrap();decision.authorize(&c.scope,&request,&permit,at).unwrap();assert_eq!(clocks.get(),2);assert_eq!(queues.get(),0);},
    FundamentalPreintent::Deferred(reason)=>{assert!(expire_mint);assert_eq!(reason,"FUNDAMENTAL_PHYSICAL_BOOK_STALE");assert_eq!(clocks.get(),3);assert_eq!(queues.get(),1);}
   }
   assert!(f.cohorts.durable_tag(qid).is_some(),"successful bind stays original, without rearm or replacement");assert_eq!(bank.durable_head(),head);assert_eq!(bank.engine().report().unwrap(),financial);assert_eq!(fs::read(root.0.join("dispatch.ndjson")).unwrap(),dispatch);assert!(log.intents.is_empty()&&log.attempted.is_empty()&&log.acknowledgments.is_empty());assert!(bank.engine().live_order_bindings().is_empty()&&bank.engine().fills().is_empty());
  }
 }
 #[test]fn fundamental_preintent_real_refusal_never_hides_current_book_queue_or_original_operations(){
  for case in ["fresh_ineligible","unknown","active","manual","queue_io","intent","attempt","ack","binding"]{
   let at=now()-1000;let(root,c,mut bank,mut log,_source,q,_book)=fixture_at(at);let mut f=fundamental_fixture(&c,&bank,&log,at,case=="fresh_ineligible");let qid=q["id"].as_u64().unwrap();let n=q["quantity"].as_u64().unwrap();let(_,request,permit)=physical_order(&c,&q).unwrap();
   if matches!(case,"intent"|"attempt"|"ack"){log.append(json!({"kind":"intent","qid":qid,"quote":q})).unwrap();}
   if matches!(case,"attempt"|"ack"){log.append(json!({"kind":"attempt","qid":qid})).unwrap();}
   if case=="ack"{log.append(json!({"kind":"ack","qid":qid,"receipt":{"data":{"orderId":"902"}}})).unwrap();}
   if case=="binding"{let ack_at=moloch_live_runtime::daily_windows::clock(bank.engine().report().unwrap()["at"].as_str().unwrap()).unwrap().max(at+1)+1;bank.accept("original-fixture-ack",1,&json!({"type":"actual_order_ack","source":"official_sig_api_v1","receiptHash":"a".repeat(64),"orderId":qid,"actualOrderId":"902","exchangeId":"101","marketId":"201","side":q["side"],"action":q["action"],"quantity":q["quantity"],"price":q["price"],"receivedAt":iso(ack_at),"processedAt":iso(ack_at)})).unwrap();}
   let head=bank.durable_head();let financial=bank.engine().report().unwrap();let dispatch=fs::read(root.0.join("dispatch.ndjson")).unwrap();let sidecar=fs::read(root.0.join("fundamental.ndjson")).unwrap();let mut queue=json!({"state":{"unknown":{},"active":null,"manual_review":false}});
   match case{"unknown"=>queue["state"]["unknown"]=json!({"original":"uncertain"}),"active"=>queue["state"]["active"]=json!({"original":"ongoing"}),"manual"=>queue["state"]["manual_review"]=json!(true),_=>{}}
   let error=fundamental_preintent(&mut f,&bank,&log,&c.scope,&q,n,&request,&permit,||if case=="fresh_ineligible"{at}else{at+6001},||{if case=="queue_io"{return Err(std::io::Error::other("fixture quiet snapshot failure").into())}recovery_queue_clear(&queue)}).err().expect(case);
   if case=="fresh_ineligible"{assert_eq!(error.to_string(),"LIVE_FUNDAMENTAL_STALE_DEFER_CURRENT_BOOK");}if case=="queue_io"{assert_eq!(error.to_string(),"fixture quiet snapshot failure");}
   assert_eq!(bank.durable_head(),head,"{case}");assert_eq!(bank.engine().report().unwrap(),financial,"{case}");assert_eq!(fs::read(root.0.join("dispatch.ndjson")).unwrap(),dispatch,"{case}");assert_eq!(fs::read(root.0.join("fundamental.ndjson")).unwrap(),sidecar,"{case}");assert!(f.cohorts.durable_tag(qid).is_none(),"{case}");
  }
 }
 #[test]fn fundamental_preintent_stale_defer_requires_real_freshness_loss_and_preserves_prefix(){
  let at=now()-1000;let(root,c,bank,log,_source,q,_book)=fixture_at(at);let stale:Box<dyn std::error::Error>="FUNDAMENTAL_PHYSICAL_BOOK_STALE".into();let head=bank.durable_head();let bytes=fs::read(root.0.join("dispatch.ndjson")).unwrap();let financial=bank.engine().report().unwrap();
  assert!(fundamental_stale_preintent(&bank,&log,&c.scope,&q,stale.as_ref(),false,at+60000).unwrap());
  // bind_quantity assesses before physical_decision. It can report this
  // refusal before a binding exists; the production guard must cover both.
  let denied:Box<dyn std::error::Error>="FUNDAMENTAL_NOT_ELIGIBLE".into();
  assert!(fundamental_stale_preintent(&bank,&log,&c.scope,&q,denied.as_ref(),false,at+60000).unwrap());
  assert_eq!(fundamental_stale_preintent(&bank,&log,&c.scope,&q,denied.as_ref(),false,at).unwrap_err().to_string(),"LIVE_FUNDAMENTAL_STALE_DEFER_CURRENT_BOOK");
  assert_eq!(fundamental_stale_preintent(&bank,&log,&c.scope,&q,denied.as_ref(),true,at+60000).unwrap_err().to_string(),"LIVE_FUNDAMENTAL_STALE_DEFER_UNKNOWN_ACCEPTANCE");
  let exit_denied:Box<dyn std::error::Error>="FUNDAMENTAL_PHYSICAL_EXIT_NOT_ELIGIBLE".into();
  assert!(fundamental_stale_preintent(&bank,&log,&c.scope,&q,exit_denied.as_ref(),false,at+60000).unwrap());
  assert_eq!(fundamental_stale_preintent(&bank,&log,&c.scope,&q,exit_denied.as_ref(),false,at).unwrap_err().to_string(),"LIVE_FUNDAMENTAL_STALE_DEFER_CURRENT_BOOK");
  assert_eq!(fundamental_stale_preintent(&bank,&log,&c.scope,&q,exit_denied.as_ref(),true,at+60000).unwrap_err().to_string(),"LIVE_FUNDAMENTAL_STALE_DEFER_UNKNOWN_ACCEPTANCE");
  assert!(!fundamental_stale_preintent(&bank,&log,&c.scope,&q,&std::io::Error::other("unrelated failure"),false,at+60000).unwrap());
  assert_eq!(fundamental_stale_preintent(&bank,&log,&c.scope,&q,stale.as_ref(),false,at).unwrap_err().to_string(),"LIVE_FUNDAMENTAL_STALE_DEFER_CURRENT_BOOK");
  assert_eq!(fundamental_stale_preintent(&bank,&log,&c.scope,&q,stale.as_ref(),true,at+60000).unwrap_err().to_string(),"LIVE_FUNDAMENTAL_STALE_DEFER_UNKNOWN_ACCEPTANCE");
  let mut changed=q.clone();changed["price"]=json!("0.005000");assert_eq!(fundamental_stale_preintent(&bank,&log,&c.scope,&changed,stale.as_ref(),false,at+60000).unwrap_err().to_string(),"LIVE_FUNDAMENTAL_STALE_DEFER_QUOTE_CHANGED");
  assert_eq!(fundamental_stale_preintent(&bank,&log,&c.scope,&changed,exit_denied.as_ref(),false,at+60000).unwrap_err().to_string(),"LIVE_FUNDAMENTAL_STALE_DEFER_QUOTE_CHANGED");
  assert_eq!(bank.durable_head(),head);assert_eq!(bank.engine().report().unwrap(),financial);assert_eq!(fs::read(root.0.join("dispatch.ndjson")).unwrap(),bytes);
 }
 #[test]fn fundamental_preintent_stale_defer_never_hides_an_original_operation(){
  for case in ["intent","attempt","ack","binding"]{let at=now()-1000;let(_root,c,mut bank,mut log,_source,q,_book)=fixture_at(at);let qid=q["id"].as_u64().unwrap();let stale:Box<dyn std::error::Error>="FUNDAMENTAL_PHYSICAL_BOOK_STALE".into();
   if case!="binding"{log.append(json!({"kind":"intent","qid":qid,"quote":q})).unwrap();}
   if matches!(case,"attempt"|"ack"){log.append(json!({"kind":"attempt","qid":qid})).unwrap();}
   if case=="ack"{log.append(json!({"kind":"ack","qid":qid,"receipt":{"data":{"orderId":"902"}}})).unwrap();}
   if case=="binding"{accept(&mut bank,&json!({"type":"actual_order_ack","source":"official_sig_api_v1","receiptHash":"a".repeat(64),"orderId":qid,"actualOrderId":"902","exchangeId":"101","marketId":"201","side":q["side"],"action":q["action"],"quantity":q["quantity"],"price":q["price"],"receivedAt":iso(now())})).unwrap();}
   let head=bank.durable_head();let descriptor=log.descriptor_identity().unwrap();assert!(fundamental_stale_preintent(&bank,&log,&c.scope,&q,stale.as_ref(),false,at+60000).is_err(),"{case}");
   let denied:Box<dyn std::error::Error>="FUNDAMENTAL_NOT_ELIGIBLE".into();assert!(fundamental_stale_preintent(&bank,&log,&c.scope,&q,denied.as_ref(),false,at+60000).is_err(),"{case}");
   let exit_denied:Box<dyn std::error::Error>="FUNDAMENTAL_PHYSICAL_EXIT_NOT_ELIGIBLE".into();assert!(fundamental_stale_preintent(&bank,&log,&c.scope,&q,exit_denied.as_ref(),false,at+60000).is_err(),"{case}");
   assert_eq!(bank.durable_head(),head);assert_eq!(log.descriptor_identity().unwrap(),descriptor);
  }
 }
 struct PeriodicReads{at:u64,tape:Receipt,book:Receipt,journal:PathBuf,initial_sequence:u64,services:usize,fail_service:Option<usize>,book_failure:bool,queued_gap:bool,trace:Vec<&'static str>}
 impl OrdinaryPreflightReads for PeriodicReads{
  fn now_ms(&self)->u64{self.at}
  fn guarded(&mut self)->Result<bool>{panic!("source refresh must not request execution authority")}
  fn read(&mut self,endpoint:Endpoint)->Result<Receipt>{
   match endpoint{Endpoint::Trades{exchange_id,from,to}=>{assert_eq!(exchange_id,"101");assert_eq!(from,self.tape.data["from"].as_str().unwrap());assert_eq!(to,self.tape.data["to"].as_str().unwrap());},_=>panic!("only public Trades retains a blocking read")}
   self.trace.push("tape_return");Ok(self.tape.clone())
  }
  fn service_before_read(&mut self,bank:&mut Session,_source:&mut RestSource)->Result<()>{
   self.services+=1;self.trace.push("service");
   if self.services>=2{assert!(bank.last_source_sequence("moloch-live")>self.initial_sequence);assert!(periodic_events(&self.journal).iter().any(|e|e["type"]=="coverage"&&e["to"]==self.tape.data["to"]));}
   if self.fail_service==Some(self.services){return Err(std::io::Error::other("fixture periodic service failure").into())}Ok(())
  }
  fn read_serviced(&mut self,bank:&mut Session,source:&mut RestSource,endpoint:Endpoint)->Result<Receipt>{
   assert!(matches!(endpoint,Endpoint::OrderBook{market_id,depth:20}if market_id=="201"));assert_eq!(self.services,2);self.trace.push("book_wait");
   // Model a queued-frame pump while the SAME exclusive GET is waiting. The
   // helper's separate real loopback test establishes its actual SDK boundary.
   self.service_before_read(bank,source)?;
   if self.queued_gap{let at=iso(self.at+100);let payload=json!({"marketId":"201","exchangeId":"101","tournamentId":T,"at":at});accept_source_events(bank,&source.broadcast("book_dirty",&payload,&at)?)?;}
   self.trace.push("book_return");
   if self.book_failure{return Err("FIXTURE_PERIODIC_BOOK_FAILURE".into())}Ok(self.book.clone())
  }
 }
 fn periodic_reads(at:u64,mut book:Receipt,bank:&Session,c:&Control)->PeriodicReads{
  let stamp=iso(at);book.request_started_at=stamp.clone();book.received_at=stamp.clone();book.data["contexts"][0]["orderbook"]["exchanges"][0]["asOf"]=json!({"sequence":2,"at":stamp});
  let tape=Receipt{endpoint:format!("/exchanges/101/trades?tournamentId={T}&from={}&to={stamp}",iso(at-4002)),request_started_at:stamp.clone(),received_at:stamp.clone(),status:200,data:json!({"marketId":"201","exchangeId":"101","from":iso(at-4002),"to":stamp,"data":[],"pagination":{"hasMore":false,"nextCursor":null},"coverage":{"complete":true,"projectedThroughSequence":2}})};
  PeriodicReads{at,tape,book,journal:c.root.join("bank/events.jsonl"),initial_sequence:bank.last_source_sequence("moloch-live"),services:0,fail_service:None,book_failure:false,queued_gap:false,trace:vec![]}
 }
 #[test]fn periodic_source_services_book_wait_only_after_original_tape_is_committed(){
  let at=now()-1000;let(_root,c,mut bank,log,mut state,_,book)=fixture_at(at);let mut reads=periodic_reads(at,book,&bank,&c);let cash=bank.engine().wallet.cash;let fills=bank.engine().fills().to_vec();
  assert_eq!(source(&c,&mut bank,&mut state,"101","201",at-4002,&mut reads).unwrap(),at-2000);
  assert_eq!(reads.trace,vec!["service","tape_return","service","book_wait","service","book_return"]);
  assert!(periodic_events(&reads.journal).iter().any(|e|e["type"]=="book"&&e["book"]["sequence"]=="2"&&e["book"]["sourceAt"]==iso(at)&&e["book"]["receivedAt"]==reads.book.received_at));
  assert_eq!(bank.engine().wallet.cash,cash);assert_eq!(bank.engine().fills(),fills.as_slice());assert!(log.attempted.is_empty());
 }
 #[test]fn periodic_source_rejects_bad_original_tape_before_book_service_or_request(){
  let at=now()-1000;let(_root,c,mut bank,_,mut state,_,book)=fixture_at(at);let mut reads=periodic_reads(at,book,&bank,&c);reads.tape.status=503;let head=bank.durable_head();
  assert!(source(&c,&mut bank,&mut state,"101","201",at-4002,&mut reads).is_err());assert_eq!(reads.trace,vec!["service","tape_return"]);assert_eq!(bank.durable_head(),head);
 }
 #[test]fn periodic_source_service_failure_preserves_an_already_committed_tape(){
  for failed in [1,2,3]{let at=now()-1000;let(_root,c,mut bank,_,mut state,_,book)=fixture_at(at);let mut reads=periodic_reads(at,book,&bank,&c);reads.fail_service=Some(failed);let before=bank.last_source_sequence("moloch-live");
   let error=source(&c,&mut bank,&mut state,"101","201",at-4002,&mut reads).unwrap_err();assert!(error.downcast_ref::<std::io::Error>().is_some());assert_eq!(reads.services,failed);
   assert_eq!(bank.last_source_sequence("moloch-live")>before,failed>=2);assert!(!reads.trace.contains(&"book_return"));assert!(bank.engine().fills().is_empty());
  }
 }
 #[test]fn periodic_source_book_failure_keeps_tape_without_a_trailing_callback(){
  let at=now()-1000;let(_root,c,mut bank,_,mut state,_,book)=fixture_at(at);let mut reads=periodic_reads(at,book,&bank,&c);reads.book_failure=true;let before=bank.last_source_sequence("moloch-live");
  assert_eq!(source(&c,&mut bank,&mut state,"101","201",at-4002,&mut reads).unwrap_err().to_string(),"FIXTURE_PERIODIC_BOOK_FAILURE");assert_eq!(reads.services,3);assert_eq!(reads.trace.last(),Some(&"book_return"));assert!(bank.last_source_sequence("moloch-live")>before);assert!(bank.engine().fills().is_empty());
 }
 #[test]fn periodic_source_pending_book_cannot_clear_a_newer_genuine_stream_gap(){
  let at=now()-1000;let(_root,c,mut bank,log,mut state,_,book)=fixture_at(at);let mut reads=periodic_reads(at,book,&bank,&c);reads.queued_gap=true;let cash=bank.engine().wallet.cash;
  source(&c,&mut bank,&mut state,"101","201",at-4002,&mut reads).unwrap();
  assert!(state.scopes().as_array().unwrap().iter().any(|s|s["exchangeId"]=="101"&&s["blocked"]==true));assert!(periodic_events(&reads.journal).iter().any(|e|e["type"]=="gap"&&e["receivedAt"]==iso(at+100)));
  assert_eq!(bank.engine().wallet.cash,cash);assert!(bank.engine().fills().is_empty());assert!(log.attempted.is_empty());
 }
 struct Reads{rest_only:bool,calls:usize,fail_at:Option<usize>,invalidate_at:Option<usize>,book:Receipt,trace:Vec<&'static str>,endpoints:Vec<Value>,returned:Vec<Value>}
 impl Reads{fn new(book:Receipt)->Self{Self{rest_only:false,calls:0,fail_at:None,invalidate_at:None,book,trace:vec![],endpoints:vec![],returned:vec![]}}}
 impl OrdinaryPreflightReads for Reads{
  fn now_ms(&self)->u64{now()}
  fn guarded(&mut self)->Result<bool>{Ok(true)}
  fn service_before_read(&mut self,bank:&mut Session,source:&mut RestSource)->Result<()>{
   self.calls+=1;self.trace.push("service");if self.fail_at==Some(self.calls){return Err("LIVE_PREFLIGHT_QUOTE_INVALIDATED".into())}
   if self.rest_only{return Ok(())}
   let change=self.invalidate_at==Some(self.calls);let(bid,ask)=if change{("0.405000","0.415000")}else{("0.400000","0.410000")};let at=iso(now());
   // Use the same original broadcast normalizer as drain_stream, keeping its
   // revision/book state coherent with the real Session. A later REST response
   // cannot roll native state back to this fixture's older sequence1 book.
   let payload=json!({"markets":[{"marketId":"201","batch":{"delivery":{"model":"best-effort-authoritative-resync","revision":self.calls,"previousRevision":self.calls-1,"sourceSequenceFrom":self.calls+1,"sourceSequenceThrough":self.calls+1,"correlationId":format!("preflight-{}",self.calls)},"books":[{"exchangeId":"101","asOf":{"sequence":self.calls+1,"at":at},"nextExpiryAt":null,"bids":[{"price":bid,"quantity":10}],"asks":[{"price":ask,"quantity":990}]}],"trades":[],"bookDirty":[],"marketSettled":[]}}]});
   accept_source_events(bank,&source.broadcast("markets_batch",&payload,&at)?)
  }
  fn read(&mut self,ep:Endpoint)->Result<Receipt>{
   assert_eq!(self.calls,self.endpoints.len()+1,"one service must immediately precede each GET");self.trace.push("get");self.endpoints.push(serde_json::to_value(&ep)?);let at=iso(now());
   let(path,data)=match ep{
    Endpoint::Tournament=>("/tournaments/cup".into(),json!({"status":"active"})),
    Endpoint::Positions=>("/tournaments/cup/portfolio/positions".into(),json!({"positions":[],"summary":{"totalMarketValue":0,"totalCostBasis":0,"totalUnrealizedPnl":0}})),
    Endpoint::Orders{status}=>(format!("/orders?status={status}&limit=200&tournamentId={T}"),json!({"data":[],"coverage":{"complete":true},"pagination":{"hasMore":false,"nextCursor":null,"limit":200}})),
    Endpoint::Account=>(format!("/account?tournamentId={T}"),json!({"balance":1000})),
    Endpoint::Market{market_id}=>(format!("/markets/{market_id}?tournamentId={T}"),json!({"id":market_id,"status":"open"})),
    Endpoint::Nodes{market_id}=>(format!("/markets/{market_id}/nodes?tournamentId={T}"),json!({"market_id":market_id,"root":{"node_type":"contract","contract_id":"301"}})),
    Endpoint::OrderBook{..}=>{let mut data=self.book.data.clone();if self.rest_only{let b=&mut data["contexts"][0]["orderbook"]["exchanges"][0];b["asOf"]=json!({"sequence":self.calls+1,"at":at});b["bids"][0]["quantity"]=json!(990);b["asks"][0]["quantity"]=json!(10);}(self.book.endpoint.clone(),data)},
    _=>return Err("FIXTURE_UNEXPECTED_READ".into())
   };let r=Receipt{endpoint:path,request_started_at:at.clone(),received_at:at,status:200,data};self.returned.push(serde_json::to_value(&r)?);Ok(r)
  }
 }
 fn original_read_order(f:&SnapshotSet)->Value{serde_json::to_value([&f.account,&f.positions,&f.open_orders,&f.cash_balance,&f.market,&f.contract,&f.book]).unwrap()}
 #[test]fn stable_native_buy_completes_exact_seven_original_receipts_despite_head_progress(){
  let(_root,c,mut bank,log,mut source,q,book)=fixture();let head=bank.durable_head();let before=bank.engine().report().unwrap();let mut reads=Reads::new(book);
  let(current,facts)=ordinary_preflight(&c,&mut bank,&log,&mut source,&q,&mut reads).unwrap().expect("benign source progress must not veto every entry");
  let expected=[Endpoint::Tournament,Endpoint::Positions,Endpoint::Orders{status:"open".into()},Endpoint::Account,Endpoint::Market{market_id:"201".into()},Endpoint::Nodes{market_id:"201".into()},Endpoint::OrderBook{market_id:"201".into(),depth:20}];
  assert_eq!(reads.calls,7);assert_eq!(reads.endpoints,expected.iter().map(|e|serde_json::to_value(e).unwrap()).collect::<Vec<_>>());assert_eq!(reads.trace,["service","get"].repeat(7));assert_eq!(original_read_order(&facts),json!(reads.returned));assert_eq!(current,q);assert_eq!(quote(&bank,q["id"].as_u64().unwrap()).unwrap(),q);assert_ne!(head,bank.durable_head());
  for key in ["cash","ownedCost","realized","fills"]{assert_eq!(before[key],bank.engine().report().unwrap()[key]);}assert!(log.intents.is_empty()&&log.attempted.is_empty());
 }
 #[test]fn refreshed_weak_no_skips_account_packet_and_strong_yes_remains_available(){
  use moloch_live_runtime::ordinary_side_edge::Mode;
  let(root,mut c,mut bank,log,mut source,yes,book)=fixture_at(now()-15000);c.ordinary_side_edge=Mode::Enforce;
  let no=bank.engine().order_records().into_iter().find(|q|q["side"]=="NO"&&q["retired"]==false).unwrap();
  let before=bank.engine().report().unwrap();let mut reads=Reads::new(book);reads.rest_only=true;
  assert_eq!(moloch_live_runtime::quote_priority::from_native(&bank,&c.scope,&[no.clone()],reads.now_ms(),None).unwrap().assessments[0].decision.class,moloch_live_runtime::quote_priority::Class::NeedsRefresh);
  assert!(ordinary_preflight(&c,&mut bank,&log,&mut source,&no,&mut reads).unwrap().is_none());
  assert_eq!(reads.endpoints.len(),1);assert_eq!(reads.endpoints[0],serde_json::to_value(Endpoint::OrderBook{market_id:"201".into(),depth:20}).unwrap());
  let diagnostic=load(&root.0.join("ordinary-side-edge-refreshed.json")).unwrap();assert_eq!(diagnostic["class"],"below_minimum");assert_eq!(diagnostic["vetoedThisPass"],true);
  assert_eq!(quote(&bank,no["id"].as_u64().unwrap()).unwrap(),no);
  // Same pass, no reservation/intent/attempt was consumed by the weak mate.
  let calls=reads.endpoints.len();assert!(ordinary_preflight(&c,&mut bank,&log,&mut source,&yes,&mut reads).unwrap().is_some());assert_eq!(reads.endpoints.len()-calls,7);
  let after=bank.engine().report().unwrap();for k in ["cash","reservedCash","ownedCost","realized","fills"]{assert_eq!(before[k],after[k],"{k}");}assert!(log.intents.is_empty()&&log.attempted.is_empty());
 }
 #[test]fn refreshed_negative_disabled_and_shadow_preserve_entire_original_packet(){
  use moloch_live_runtime::ordinary_side_edge::Mode;
  for mode in [Mode::Disabled,Mode::Shadow]{let(root,mut c,mut bank,log,mut source,_yes,book)=fixture_at(now()-15000);c.ordinary_side_edge=mode;let no=bank.engine().order_records().into_iter().find(|q|q["side"]=="NO"&&q["retired"]==false).unwrap();let mut reads=Reads::new(book);reads.rest_only=true;
   assert!(ordinary_preflight(&c,&mut bank,&log,&mut source,&no,&mut reads).unwrap().is_some());assert_eq!(reads.endpoints.len(),8);assert!(log.intents.is_empty()&&log.attempted.is_empty());
   if mode==Mode::Disabled{assert!(!root.0.join("ordinary-side-edge-refreshed.json").exists());}else{assert_eq!(load(&root.0.join("ordinary-side-edge-refreshed.json")).unwrap()["vetoedThisPass"],false);}
  }
 }
 #[test]fn refreshed_genuine_strong_no_ws_book_does_not_veto_account_packet(){
  use moloch_live_runtime::ordinary_side_edge::Mode;
  let(root,mut c,mut bank,log,mut source,_yes,book)=fixture_at(now()-15000);c.ordinary_side_edge=Mode::Enforce;let no=bank.engine().order_records().into_iter().find(|q|q["side"]=="NO"&&q["retired"]==false).unwrap();let mut reads=Reads::new(book);
  // The hook admits a genuine newer WS book; the returned REST book is an
  // older source sequence. RestSource::broadcast pins the normalized receive
  // clock. Bid10/ask990 gives YES .400100 and NO .599900: this NO is strong,
  // so an actual fresh source update must not suppress its independent packet.
  assert!(ordinary_preflight(&c,&mut bank,&log,&mut source,&no,&mut reads).unwrap().is_some());assert_eq!(reads.endpoints.len(),8);
  let diagnostic=load(&root.0.join("ordinary-side-edge-refreshed.json")).unwrap();assert_eq!(diagnostic["class"],"meets_minimum");assert_eq!(diagnostic["vetoedThisPass"],false);assert!(log.intents.is_empty()&&log.attempted.is_empty());
 }
 #[test]fn refreshed_negative_never_swallows_stream_or_durable_errors(){
  use moloch_live_runtime::ordinary_side_edge::Mode;
  let(_root,mut c,mut bank,log,mut source,_yes,book)=fixture_at(now()-15000);c.ordinary_side_edge=Mode::Enforce;let no=bank.engine().order_records().into_iter().find(|q|q["side"]=="NO"&&q["retired"]==false).unwrap();let mut reads=Reads::new(book);reads.fail_at=Some(1);
  assert_eq!(ordinary_preflight(&c,&mut bank,&log,&mut source,&no,&mut reads).err().unwrap().to_string(),"LIVE_PREFLIGHT_QUOTE_INVALIDATED");assert!(reads.endpoints.is_empty());assert!(log.intents.is_empty());
 }
 #[test]fn hook_failure_before_first_or_later_get_is_fatal_even_with_invalidation_text(){
  for at in [1usize,3]{let(_root,c,mut bank,log,mut source,q,book)=fixture();let mut reads=Reads::new(book);reads.fail_at=Some(at);let error=ordinary_preflight(&c,&mut bank,&log,&mut source,&q,&mut reads).err().expect("untyped stream/durable error must propagate");assert_eq!(error.to_string(),"LIVE_PREFLIGHT_QUOTE_INVALIDATED");assert!(!error.is::<PreflightQuoteInvalidated>());assert_eq!(reads.endpoints.len(),at-1);assert_eq!(reads.calls,at);assert_eq!(reads.trace.last(),Some(&"service"));assert!(log.intents.is_empty()&&log.attempted.is_empty());}
 }
 #[test]fn changed_quote_is_expected_defer_before_next_get_without_new_intent(){
  let(_root,c,mut bank,log,mut source,_q,book)=fixture();let q=bank.engine().order_records().into_iter().find(|q|q["exchangeId"]=="101"&&q["retired"]==false).unwrap();assert_eq!(q["side"],"NO");let mut reads=Reads::new(book);reads.invalidate_at=Some(1);let frozen=q.clone();
  assert!(ordinary_preflight(&c,&mut bank,&log,&mut source,&q,&mut reads).unwrap().is_none());assert_eq!(reads.calls,1);assert!(reads.endpoints.is_empty());let current=quote(&bank,q["id"].as_u64().unwrap()).unwrap();assert!(current["cancelEffectiveAt"].is_u64());for key in ["id","price","quantity","createdAt","activationAt","expiresAt"]{assert_eq!(current[key],frozen[key]);}assert!(log.intents.is_empty()&&log.attempted.is_empty());
 }
 #[test]fn unreserved_read_target_still_collects_all_original_facts_after_source_changes_drafts(){
  let(_root,c,mut bank,log,mut source,_q,book)=fixture();let mut reads=Reads::new(book);reads.invalidate_at=Some(1);let facts=preflight_snapshots(&c,&mut bank,&mut source,"201",None,&mut reads).unwrap();assert_eq!(reads.calls,7);assert_eq!(reads.trace,["service","get"].repeat(7));assert_eq!(original_read_order(&facts),json!(reads.returned));assert!(log.intents.is_empty()&&log.attempted.is_empty());
 }
}
#[cfg(test)]mod discovery_urgent_tests{
 use super::*;
 #[test]fn physical_liabilities_and_risk_exits_precede_discovery_without_promoting_flat_drafts(){
  let q=|id,ex:&str|json!({"id":id,"exchangeId":ex,"retired":false});
  let mut age=q(3,"103");age["inventoryExitReason"]=json!("maximumAge");
  let mut aon=q(4,"104");aon["executionStyle"]=json!("inventoryAON");
  let mut retired=q(5,"105");retired["retired"]=json!(true);
  let orders=vec![q(1,"101"),q(2,"102"),age,aon,retired];
  assert_eq!(urgent_source_exchanges(&orders,&[1,5].into()).unwrap(),["101".into(),"103".into(),"104".into()].into());
 }
}
#[cfg(test)]mod collateral_accounting_config_tests{
 use super::*;
 fn control()->Control{serde_json::from_value(json!({"activation":{"runId":"test","entryUntilMs":9000000,"maximumCommitted":"1000.000000","maximumMarketCost":"100.000000","maximumQuantity":2,"tournamentId":"cup","tournamentSlug":"cup","binarySha256":"a".repeat(64)},"scope":{"tournamentId":"cup","tournamentSlug":"cup","exchanges":{}},"policy":{},"root":"/fixture","credentials":"/unused","ownerLock":"/unused.lock","accountScope":"fixture"})).unwrap()}
 #[test]fn acquisition_shadow_is_explicit_independent_and_absent_in_legacy_control(){let mut c=control();assert!(c.fundamental_acquisition.is_none());assert!(serde_json::to_value(&c).unwrap().get("fundamentalAcquisition").is_none());c.fundamental_acquisition=Some(moloch_live_runtime::execution_distribution::AcquisitionConfig{cohort_id:"worker-fixture".into(),maximum_pending:8,retained_completed:16});c.fundamental_acquisition.as_ref().unwrap().validate().unwrap();assert!(c.execution_distribution.is_none());assert!(c.execution_model.is_none());let v=serde_json::to_value(&c).unwrap();assert_eq!(v["fundamentalAcquisition"]["cohort_id"],"worker-fixture");}
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
 #[test]fn queued_frames_keep_exact_clocks_rows_cursors_and_restart_state(){
  let a=BankTemp::new();let b=BankTemp::new();let policy=json!({});let mut serial=Session::open(&a.0,None,&policy).unwrap();let mut batched=Session::open(&b.0,None,&policy).unwrap();let mut pending=QueuedSourceBatch::default();
  for(i,event)in source_ticks(65).iter().enumerate(){let at=1_760_000_001_000+i as u64;let mut prepared=event.clone();prepared["processedAt"]=json!(iso(at));accept_prepared_source_events(&mut serial,&[prepared]).unwrap();pending.push_at(&mut batched,&[event.clone()],at).unwrap();assert!(pending.prepared.len()<32);}
  pending.finish(&mut batched,Ok(())).unwrap();assert_eq!(batched.last_source_sequence("moloch-live"),65);assert!(pending.prepared.is_empty());
  for name in ["events.jsonl","head.json","report.json"]{assert_eq!(fs::read(a.0.join(name)).unwrap(),fs::read(b.0.join(name)).unwrap(),"{name}")}
  let report=batched.engine().report().unwrap();drop(batched);let restored=Session::open(&b.0,None,&policy).unwrap();assert_eq!(restored.engine().report().unwrap(),report);
 }
 #[test]fn queued_valid_prefix_is_durable_before_source_or_stream_failure(){
  for reason in ["source_failed","stream_failed","unexpected_rejoin"]{let temp=BankTemp::new();let policy=json!({});let mut bank=Session::open(&temp.0,None,&policy).unwrap();let mut pending=QueuedSourceBatch::default();for(i,event)in source_ticks(5).iter().enumerate(){pending.push_at(&mut bank,&[event.clone()],1_760_000_001_000+i as u64).unwrap();}assert_eq!(bank.last_source_sequence("moloch-live"),0);
   assert_eq!(pending.finish(&mut bank,Err(reason.into())).unwrap_err().to_string(),reason);assert_eq!(bank.last_source_sequence("moloch-live"),5);drop(bank);assert_eq!(Session::open(&temp.0,None,&policy).unwrap().last_source_sequence("moloch-live"),5);
  }
 }
 #[test]fn queued_native_failure_propagates_without_replaying_or_appending_invalid_batch(){
  let temp=BankTemp::new();let mut bank=Session::open(&temp.0,None,&json!({})).unwrap();let mut pending=QueuedSourceBatch::default();let mut events=source_ticks(32);events[17]["type"]=json!("unsupported");let result=pending.push_at(&mut bank,&events,1_760_000_001_000);assert!(result.is_err());assert_eq!(bank.last_source_sequence("moloch-live"),0);assert_eq!(fs::metadata(temp.0.join("events.jsonl")).unwrap().len(),0);
 }
 #[test]fn queued_large_events_flush_before_aggregate_byte_limit_and_preserve_individual_large_event(){
  let temp=BankTemp::new();let mut bank=Session::open(&temp.0,None,&json!({})).unwrap();let mut pending=QueuedSourceBatch::default();
  let mut events=source_ticks(18);for e in &mut events{e["padding"]=json!("x".repeat(512*1024));}pending.push_at(&mut bank,&events,1_760_000_001_000).unwrap();assert!(bank.last_source_sequence("moloch-live")>0);pending.finish(&mut bank,Ok(())).unwrap();assert_eq!(bank.last_source_sequence("moloch-live"),18);
  let large=json!({"type":"tick","receivedAt":iso(1_760_000_002_000),"padding":"y".repeat(5*1024*1024)});pending.push_at(&mut bank,&[large],1_760_000_002_000).unwrap();assert!(pending.prepared.is_empty());assert_eq!(bank.last_source_sequence("moloch-live"),19);
 }
 #[test]fn expired_profit_hints_bypass_only_age_and_keep_unknown_intent_partial_guards(){
  let mut q=expired_hint_quote(7,"1012");q.as_object_mut().unwrap().remove("inventoryExitReason");
  let calls=std::cell::Cell::new(0);
  let(hints,_)=prepare_expired_exit_hints(&[q.clone()],20000,|_|true,|_,_|Ok(300),|_|Ok(true),|_,_|{calls.set(calls.get()+1);Ok(false)},||Ok(false),|_,u|Ok(hint_safety(u))).unwrap();
  assert_eq!(hints.len(),1);assert_eq!(calls.get(),0);assert_eq!(hints["1012:NO:sell:profit"].1.kind(),moloch_live_runtime::exit_refresh::ReadKind::Profit);
  for case in ["intent","attempt","unknown","pending","partial","held"]{
   let mut old=q.clone();if case=="partial"{old["filled"]=json!(1)}
   let(hints,_)=prepare_expired_exit_hints(&[old],20000,|_|case!="attempt",|_,_|Ok(if case=="held"{0}else{300}),|_|Ok(true),|_,_|Ok(false),||Ok(case=="unknown"),|_,u|Ok(moloch_live_runtime::exit_refresh::Safety{has_intent:case=="intent",same_exchange_pending_physical_order:case=="pending",..hint_safety(u)})).unwrap();assert!(hints.is_empty(),"{case}");
  }
  let managed=["1012".to_owned()].into();assert!(!profit_hint_scope_allowed(&q,&managed,&Default::default()));assert!(!profit_hint_scope_allowed(&q,&Default::default(),&managed));
 }
 #[test]fn active_probabilistic_lease_blocks_all_profit_scopes_but_not_existing_age_path(){
  let scope=baal_transport_service::Scope{tournament_id:"cup".into(),tournament_slug:"cup".into(),exchanges:[("1012".into(),"525".into()),("1013".into(),"526".into())].into()};
  let lease=json!({"phase":"unknown","probabilistic":{"qualificationSha256":"a".repeat(64)},"legs":[{"exchangeId":"1012"}]});
  let protected=profit_refresh_protected(&scope,&json!([lease.clone()])).unwrap();assert_eq!(protected.len(),2,"unexposed held component cannot be inferred absent");
  let mut q=expired_hint_quote(1,"1013");q.as_object_mut().unwrap().remove("inventoryExitReason");assert!(!profit_hint_scope_allowed(&q,&Default::default(),&protected));
  q["inventoryExitReason"]=json!("maximumAge");assert!(profit_hint_scope_allowed(&q,&["1013".into()].into(),&protected),"extra profit exclusion does not alter original maximum-age checks");
  for field in ["released","no_execution"]{let mut ended=lease.clone();if field=="released"{ended["protectionReleased"]=json!(true)}else{ended["phase"]=json!("no_execution")};assert!(profit_refresh_protected(&scope,&json!([ended])).unwrap().is_empty());}
 }
 #[test]fn profit_history_cannot_mask_a_current_due_risk_hint(){
  let age=expired_hint_quote(1,"1012");let mut profit=age.clone();profit["id"]=json!(2);profit.as_object_mut().unwrap().remove("inventoryExitReason");
  let(hints,_)=prepare_expired_exit_hints(&[age,profit],20000,|_|true,|_,_|Ok(300),|_|Ok(true),|_,_|Ok(true),||Ok(false),|_,u|Ok(hint_safety(u))).unwrap();assert_eq!(hints.len(),2);assert_eq!(hints["1012:NO:sell"].1.kind(),moloch_live_runtime::exit_refresh::ReadKind::MaximumAge);
 }
 #[test]fn covered_profit_preempts_speculation_but_never_a_risk_exit_even_out_of_order(){
  let entries=vec![fundamental::EntryHint{exchange_id:"966".into(),market_id:"277".into(),contract_id:"278".into(),side:"YES".into(),review_sha256:"a".repeat(64)}];let profit_keys=vec!["948:NO:sell:profit".into()];
  let covered=[4usize].into();let physical=vec![("948:NO:sell".into(),4,false),("999:YES:sell".into(),5,true)];
  assert_eq!(shared_preflight_with_profit(&[],&entries,&profit_keys,&physical,&covered,None).unwrap().0,SharedPreflight::Physical(5));
  assert_eq!(shared_preflight_with_profit(&[],&entries,&profit_keys,&physical[..1],&covered,None).unwrap().0,SharedPreflight::Physical(4));
  let mut cursor=None;let mut seen=Vec::new();for _ in 0..4{let(choice,key)=shared_preflight_with_profit(&[],&entries,&profit_keys,&[],&Default::default(),cursor.as_deref()).unwrap();seen.push(choice);cursor=Some(key)}
  assert_eq!(seen.iter().filter(|s|**s==SharedPreflight::ProfitRead(0)).count(),2);assert_eq!(seen.iter().filter(|s|**s==SharedPreflight::EntryRead(0)).count(),2);
 }
 #[test]fn remaining_fifo_deadline_does_not_inherit_historical_quote_age(){
  let policy=json!({"inventoryAgeClock":"execution","maximumInventoryAgeMs":60000});let at=100000;
  let young=json!({"1012":{"NO":[{"quantity":3,"executedAt":iso(50000)}]}});
  assert!(!remaining_exit_age_due(&policy,&young,&Value::Null,"1012","NO",at).unwrap());
  let due=json!({"1012":{"NO":[{"quantity":1,"executedAt":iso(40000)},{"quantity":2,"executedAt":iso(50000)}]}});
  assert!(remaining_exit_age_due(&policy,&due,&Value::Null,"1012","NO",at).unwrap());
  let future=json!({"1012":{"NO":[{"quantity":1,"executedAt":iso(at+1)}]}});
  assert!(remaining_exit_age_due(&policy,&future,&Value::Null,"1012","NO",at).is_err());
  assert!(remaining_exit_age_due(&json!({"inventoryAgeClock":"receipt"}),&young,&Value::Null,"1012","NO",at).unwrap());
 }
 #[test]fn not_due_remaining_lots_skip_queue_and_fingerprint_without_altering_original_hint(){
  use std::cell::Cell;let orders=vec![expired_hint_quote(1,"1012")];let original=orders.clone();let touched=Cell::new(0);
  let(hints,unknown)=prepare_expired_exit_hints(&orders,20000,|_|true,|_,_|Ok(3),|_|Ok(true),|_,_|Ok(false),||{touched.set(touched.get()+1);Ok(false)},|_,u|{touched.set(touched.get()+1);Ok(hint_safety(u))}).unwrap();
  assert!(hints.is_empty());assert!(!unknown);assert_eq!(touched.get(),0);assert_eq!(orders,original);
 }
 #[test]fn long_expired_history_coalesces_by_scope_without_reviving_attempted_or_cancelled_hints(){
  let mut orders=(1..=10_002u64).map(|id|json!({"id":id,"action":"sell","inventoryExitReason":"maximumAge","executionStyle":"inventoryAON","filled":0,"expiresAt":100,"exchangeId":format!("ex{}",id%3),"side":"NO","retired":true})).collect::<Vec<_>>();
  let mut cancelled=orders.last().unwrap().clone();cancelled["id"]=json!(10_003);cancelled["cancelEffectiveAt"]=json!(101);orders.push(cancelled);
  let hints=latest_refresh_hints(&orders,200,|id|id!=10_002).unwrap();assert_eq!(hints.len(),3);let mut ids=hints.values().map(|q|q["id"].as_u64().unwrap()).collect::<Vec<_>>();ids.sort();assert_eq!(ids,vec![9_999,10_000,10_001]);
  assert!(latest_refresh_hints(&orders,99,|_|true).unwrap().is_empty());
 }
 fn expired_hint_quote(id:u64,ex:&str)->Value{json!({"id":id,"marketId":"525","exchangeId":ex,"side":"NO","action":"sell","executionStyle":"inventoryAON","inventoryExitReason":"maximumAge","filled":0,"quantity":60,"price":"0.685000","retired":true,"createdAt":0,"activationAt":3000,"expiresAt":13000})}
 fn hint_safety(unknown:bool)->moloch_live_runtime::exit_refresh::Safety{moloch_live_runtime::exit_refresh::Safety{never_attempted:true,has_intent:false,unresolved_attempts:false,global_unknown:unknown,stop:false,stream_healthy:true,owned_quantity:300,reserved_sell_quantity:0,same_exchange_pending_physical_order:false,lifecycle_fingerprint:[1;32],native_head_fingerprint:[2;32]}}
 #[test]fn flat_expired_history_performs_no_queue_status_or_full_fingerprint(){
  use std::cell::Cell;let orders:Vec<_>=(1..=10000).map(|id|expired_hint_quote(id,&format!("{}",1010+id%3))).collect();let owned=Cell::new(0);let policies=Cell::new(0);let queues=Cell::new(0);let fingerprints=Cell::new(0);
  let(hints,unknown)=prepare_expired_exit_hints(&orders,20000,|_|true,|_,_|{owned.set(owned.get()+1);Ok(0)},|_|{policies.set(policies.get()+1);Ok(true)},|_,_|Ok(true),||{queues.set(queues.get()+1);Ok(false)},|_,u|{fingerprints.set(fingerprints.get()+1);Ok(hint_safety(u))}).unwrap();
  assert!(hints.is_empty()&&!unknown);assert_eq!(owned.get(),3,"latest coalescing precedes ownership lookups");assert_eq!((policies.get(),queues.get(),fingerprints.get()),(0,0,0));
 }
 #[test]fn forecast_hold_and_current_same_side_sell_skip_expensive_refresh_work(){
  use std::cell::Cell;for live_sell in [false,true]{let mut orders=vec![expired_hint_quote(1,"1012")];if live_sell{let mut q=expired_hint_quote(2,"1012");q["retired"]=json!(false);q["createdAt"]=json!(18000);q["activationAt"]=json!(21000);q["expiresAt"]=json!(31000);orders.push(q);}let policies=Cell::new(0);let queues=Cell::new(0);let fingerprints=Cell::new(0);
   let(hints,_)=prepare_expired_exit_hints(&orders,20000,|_|true,|_,_|Ok(300),|_|{policies.set(policies.get()+1);Ok(false)},|_,_|Ok(true),||{queues.set(queues.get()+1);Ok(false)},|_,u|{fingerprints.set(fingerprints.get()+1);Ok(hint_safety(u))}).unwrap();assert!(hints.is_empty());assert_eq!((queues.get(),fingerprints.get()),(0,0));assert_eq!(policies.get(),if live_sell{0}else{1});
  }
 }
 #[test]fn owned_expired_candidates_share_one_real_queue_observation_and_keep_original_typed_guards(){
  use std::cell::Cell;let orders=vec![expired_hint_quote(1,"1012"),expired_hint_quote(2,"1012"),expired_hint_quote(3,"1013")];for unknown in [false,true]{let queues=Cell::new(0);let fingerprints=Cell::new(0);
   let(hints,checked_unknown)=prepare_expired_exit_hints(&orders,20000,|_|true,|_,_|Ok(300),|_|Ok(true),|_,_|Ok(true),||{queues.set(queues.get()+1);Ok(unknown)},|_,u|{assert_eq!(u,unknown);fingerprints.set(fingerprints.get()+1);Ok(hint_safety(u))}).unwrap();assert_eq!(checked_unknown,unknown);assert_eq!((queues.get(),fingerprints.get()),(1,2));assert_eq!(hints.len(),if unknown{0}else{2});if !unknown{assert_eq!(hints["1012:NO:sell"].1.old_qid(),2);assert_eq!(hints["1012:NO:sell"].0,orders[1]);assert_eq!(hints["1013:NO:sell"].1.old_qid(),3);}
  }
 }
 #[test]fn attempted_history_does_not_mask_prior_eligible_hint_and_other_live_scope_does_not_suppress_it(){
  use std::cell::Cell;let mut current=expired_hint_quote(4,"1013");current["retired"]=json!(false);current["expiresAt"]=json!(30000);let orders=vec![expired_hint_quote(1,"1012"),expired_hint_quote(2,"1012"),expired_hint_quote(3,"1013"),current];let queues=Cell::new(0);let fingerprints=Cell::new(0);
  let(hints,_)=prepare_expired_exit_hints(&orders,20000,|id|id!=2,|_,_|Ok(300),|_|Ok(true),|_,_|Ok(true),||{queues.set(queues.get()+1);Ok(false)},|_,u|{fingerprints.set(fingerprints.get()+1);Ok(hint_safety(u))}).unwrap();assert_eq!((queues.get(),fingerprints.get()),(1,1));assert_eq!(hints.len(),1);assert_eq!(hints["1012:NO:sell"].1.old_qid(),1);
 }
 #[test]fn refresh_prefilter_preserves_authoritative_queue_and_safety_failures(){
  let orders=vec![expired_hint_quote(1,"1012")];assert_eq!(prepare_expired_exit_hints(&orders,20000,|_|true,|_,_|Ok(300),|_|Ok(true),|_,_|Ok(true),||Err("queue_durability_failure".into()),|_,u|Ok(hint_safety(u))).err().unwrap().to_string(),"queue_durability_failure");
  assert_eq!(prepare_expired_exit_hints(&orders,20000,|_|true,|_,_|Ok(300),|_|Ok(true),|_,_|Ok(true),||Ok(false),|_,_|Err("safety_source_failure".into())).err().unwrap().to_string(),"safety_source_failure");
  let(hints,_)=prepare_expired_exit_hints(&orders,20000,|_|true,|_,_|Ok(300),|_|Ok(true),|_,_|Ok(true),||Ok(false),|_,u|Ok(moloch_live_runtime::exit_refresh::Safety{same_exchange_pending_physical_order:true,..hint_safety(u)})).unwrap();assert!(hints.is_empty());
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

#[cfg(test)]mod passive_reserved_exit_tests{
 use super::*;
 mod fixture{include!(concat!(env!("CARGO_MANIFEST_DIR"),"/tests/support/fundamental_passive_fixture.rs"));}
 use fixture::F;
 fn setup()->(F,FundamentalRuntime,Control,RestSource,Value){setup_at(now()-12000)}
 fn setup_at(base:u64)->(F,FundamentalRuntime,Control,RestSource,Value){
  let mut f=F::new_at(Some(2),"0.990000",0,"0.000000",true,base);let q=f.reserve_at(base+7001);
  let authority_path=f.root.0.join("retry-authority.json");let bytes=serde_json::to_vec(&json!({"version":1,"trustedReviewSha256":f.trust})).unwrap();fs::write(&authority_path,&bytes).unwrap();
  let runtime=FundamentalRuntime{cohorts:f.store.take().unwrap(),authority:ReviewAuthority::load(&authority_path,&fixture::sha(&bytes)).unwrap()};
  let c:Control=serde_json::from_value(json!({"activation":{"runId":"passive-inherited-fixture","entryUntilMs":base+3600000,"maximumCommitted":"1000.000000","maximumMarketCost":"100.000000","maximumQuantity":3,"tournamentId":fixture::T,"tournamentSlug":"cup","binarySha256":"a".repeat(64)},"scope":f.scope,"policy":f.policy,"root":f.root.0,"credentials":"/unused","ownerLock":f.root.0.join("owner.lock"),"accountScope":"synthetic-passive-inherited-account"})).unwrap();
  let stamp=iso(base+4000);let mut source=RestSource::new(&json!([{"marketId":"201","exchangeId":"101"}]),fixture::T,&iso(base+3999)).unwrap();
  source.accept(&json!({"source":"official_sig_api_v1","kind":"book","marketId":"201","exchangeId":"101","tournamentId":fixture::T,"status":200,"endpoint":format!("/markets/201/orderbook?tournamentId={}&depth=20",fixture::T),"requestStartedAt":stamp,"receivedAt":stamp,"data":{"contexts":[{"type":"tournament","tournament":{"id":fixture::T},"orderbook":{"exchanges":[{"exchangeId":"101","asOf":{"sequence":2,"at":stamp},"bids":[{"price":0.3,"quantity":100}],"asks":[{"price":0.32,"quantity":100}]}]}}]}})).unwrap();
  let tape=json!({"source":"official_sig_api_v1","kind":"trades","marketId":"201","exchangeId":"101","tournamentId":fixture::T,"status":200,"endpoint":format!("/exchanges/101/trades?tournamentId={}&from={}&to={}",fixture::T,iso(base+3998),stamp),"requestStartedAt":stamp,"receivedAt":stamp,"data":{"marketId":"201","exchangeId":"101","from":iso(base+3998),"to":stamp,"data":[],"pagination":{"hasMore":false,"nextCursor":null},"coverage":{"complete":true,"projectedThroughSequence":2}}});
  accept_source_events(&mut f.bank,&source.accept(&tape).unwrap()).unwrap();
  assert_eq!(quote(&f.bank,q["id"].as_u64().unwrap()).unwrap(),q);(f,runtime,c,source,q)
 }
 #[test]fn fundamental_preintent_real_sell_mints_fresh_and_defers_only_when_book_expires_between_bind_and_mint(){
  use std::cell::Cell;
  for expire_mint in [false,true]{
   let base=now()-12000;let(f,mut runtime,c,_source,q)=setup_at(base);let id=q["id"].as_u64().unwrap();let n=q["quantity"].as_u64().unwrap();let fresh=base+11001;let stale=base+14001;
   assert_eq!(q["action"],"sell");assert_eq!(q["side"],"NO");assert!(runtime.eligible(&f.bank,&f.log,&c.scope,id,fresh).unwrap());assert!(runtime.cohorts.durable_tag(id).is_none());
   let request=f.spec(&q);let permit=Permit{run_id:c.activation.run_id.clone(),intent_id:id.to_string(),owner:"Codex".into(),expires_at_ms:stale+1000,maximum_quantity:2,maximum_cost:"1.970000".into()};
   runtime.cohorts.bind_quantity(&f.bank,&f.log,&c.scope,id,n,&request,runtime.authority.trusted(),fresh).unwrap();let sidecar=fs::read(f.root.0.join("fundamental.ndjson")).unwrap();
   let head=f.bank.durable_head();let financial=f.bank.engine().report().unwrap();let dispatch=fs::read(f.root.0.join("dispatch.ndjson")).unwrap();let clocks=Cell::new(0);let queues=Cell::new(0);
   let result=fundamental_preintent(&mut runtime,&f.bank,&f.log,&c.scope,&q,n,&request,&permit,||{let i=clocks.get();clocks.set(i+1);if expire_mint&&i>0{stale}else{fresh}},||{queues.set(queues.get()+1);recovery_queue_clear(&json!({"state":{"unknown":{},"active":null,"manual_review":false}}))}).unwrap();
   match result{
    FundamentalPreintent::Ready(decision)=>{assert!(!expire_mint);assert_eq!(clocks.get(),2);assert_eq!(queues.get(),0);decision.validate_frozen(&q,&financial,id,fresh).unwrap();decision.authorize(&c.scope,&request,&permit,fresh).unwrap();},
    FundamentalPreintent::Deferred(reason)=>{assert!(expire_mint);assert_eq!(reason,"FUNDAMENTAL_PHYSICAL_EXIT_NOT_ELIGIBLE");assert_eq!(clocks.get(),3);assert_eq!(queues.get(),1);
     let error=runtime.cohorts.physical_decision(&f.bank,&f.log,&c.scope,id,&request,&permit,&runtime.authority,stale).err().expect("genuine SELL check_bound must reject the expired original book");assert_eq!(error.to_string(),reason);
    }
   }
   assert!(runtime.cohorts.durable_tag(id).is_some(),"the original successful bind stays; no rearm or replacement");assert_eq!(fs::read(f.root.0.join("fundamental.ndjson")).unwrap(),sidecar,"the original binding is neither changed nor appended twice");
   assert_eq!(f.bank.durable_head(),head);assert_eq!(f.bank.engine().report().unwrap(),financial);assert_eq!(quote(&f.bank,id).unwrap(),q);assert_eq!(fs::read(f.root.0.join("dispatch.ndjson")).unwrap(),dispatch);assert!(f.log.intents.is_empty()&&f.log.attempted.is_empty()&&f.log.acknowledgments.is_empty());assert!(f.bank.engine().live_order_bindings().is_empty()&&f.bank.engine().fills().is_empty());
  }
 }
 struct Reads{services:usize,endpoints:Vec<Value>,returned:Vec<Value>,fail_at:Option<usize>,retirement:Option<Value>,stale:bool}
 impl Reads{fn new()->Self{Self{services:0,endpoints:vec![],returned:vec![],fail_at:None,retirement:None,stale:false}}}
 impl OrdinaryPreflightReads for Reads{
  fn now_ms(&self)->u64{now()}
  fn guarded(&mut self)->Result<bool>{Ok(true)}
  fn service_before_read(&mut self,bank:&mut Session,_source:&mut RestSource)->Result<()>{self.services+=1;if self.fail_at==Some(self.services){return Err("FIXTURE_DURABLE_SERVICE_FAILURE".into())}if self.services==2{if let Some(mut e)=self.retirement.take(){e["receivedAt"]=json!(iso(now()));accept(bank,&e)?;}}Ok(())}
  fn read(&mut self,ep:Endpoint)->Result<Receipt>{
   assert_eq!(self.services,self.endpoints.len()+1);self.endpoints.push(serde_json::to_value(&ep)?);let at=now();let stamp=iso(if self.stale{at-10001}else{at});let t=fixture::T;
   let(endpoint,data)=match ep{
    Endpoint::Tournament=>("/tournaments/cup".into(),json!({"status":"active"})),
    Endpoint::Positions=>("/tournaments/cup/portfolio/positions".into(),json!({"positions":[{"exchangeId":"101","marketId":"201","quantity":-3,"costBasis":1.9}]})),
    Endpoint::Orders{status}=>(format!("/orders?status={status}&limit=200&tournamentId={t}"),json!({"data":[],"coverage":{"complete":true},"pagination":{"hasMore":false,"nextCursor":null,"limit":200}})),
    Endpoint::Account=>(format!("/account?tournamentId={t}"),json!({"balance":1000})),
    Endpoint::Market{market_id}=>(format!("/markets/{market_id}?tournamentId={t}"),json!({"id":market_id,"status":"open"})),
    Endpoint::Nodes{market_id}=>(format!("/markets/{market_id}/nodes?tournamentId={t}"),json!({"market_id":market_id,"root":{"node_type":"contract","contract_id":"301"}})),
    Endpoint::OrderBook{market_id,..}=>(format!("/markets/{market_id}/orderbook?tournamentId={t}&depth=20"),json!({"contexts":[{"type":"tournament","tournament":{"id":t},"orderbook":{"exchanges":[{"exchangeId":"101","asOf":{"sequence":3,"at":stamp},"bids":[{"price":0.3,"quantity":100}],"asks":[{"price":0.32,"quantity":100}]}]}}]})),
    _=>return Err("FIXTURE_UNEXPECTED_PASSIVE_READ".into())
   };let r=Receipt{endpoint,request_started_at:stamp.clone(),received_at:stamp,status:200,data};self.returned.push(serde_json::to_value(&r)?);Ok(r)
  }
 }
 #[test]fn passive_reserved_same_qid_reads_seven_and_reuses_only_unchanged_predispatch_binding(){
  for prebound in [false,true]{let(mut f,mut runtime,c,mut source,q)=setup();let id=q["id"].as_u64().unwrap();let at=now();let request=f.spec(&q);
   assert!(existing_target_busy(&f.bank,&f.log,"101").unwrap());
   let priority=moloch_live_runtime::quote_priority::from_native(&f.bank,&c.scope,std::slice::from_ref(&q),at,None).unwrap();assert_eq!(priority.assessments[0].decision.class,moloch_live_runtime::quote_priority::Class::Deferred);assert!(priority.ordered_indices.is_empty());
   assert!(existing_exit_read_hint(&f.bank,&f.log,&c.scope,&runtime,"101",at).unwrap().is_some());
   if prebound{runtime.cohorts.bind(&f.bank,&f.log,&c.scope,id,&request,runtime.authority.trusted(),at).unwrap();}
   let before=f.bank.engine().report().unwrap();let orders=f.bank.engine().order_records();let dispatch=fs::read(f.root.0.join("dispatch.ndjson")).unwrap();let mut reads=Reads::new();
   let(facts,until)=passive_reserved_preflight(&c,&mut f.bank,&f.log,&mut source,&runtime,&q,&mut reads).unwrap().expect("untouched passive limit must survive ordinary Deferred ranking");
   let expected=[Endpoint::Tournament,Endpoint::Positions,Endpoint::Orders{status:"open".into()},Endpoint::Account,Endpoint::Market{market_id:"201".into()},Endpoint::Nodes{market_id:"201".into()},Endpoint::OrderBook{market_id:"201".into(),depth:20}];
   assert_eq!(reads.endpoints,expected.iter().map(|ep|serde_json::to_value(ep).unwrap()).collect::<Vec<_>>());assert_eq!(reads.services,7);assert_eq!(json!(reads.returned),serde_json::to_value([&facts.account,&facts.positions,&facts.open_orders,&facts.cash_balance,&facts.market,&facts.contract,&facts.book]).unwrap());
   assert_eq!(existing_reservation_stage(&quote(&f.bank,id).unwrap(),&q,now(),until).unwrap(),None);assert!(runtime.eligible(&f.bank,&f.log,&c.scope,id,now()).unwrap());
   runtime.cohorts.bind(&f.bank,&f.log,&c.scope,id,&request,runtime.authority.trusted(),now()).unwrap();let bound=fs::read(f.root.0.join("fundamental.ndjson")).unwrap();runtime.cohorts.bind(&f.bank,&f.log,&c.scope,id,&request,runtime.authority.trusted(),now()).unwrap();assert_eq!(fs::read(f.root.0.join("fundamental.ndjson")).unwrap(),bound,"an existing binding is not appended twice");
   let permit=Permit{run_id:c.activation.run_id.clone(),intent_id:id.to_string(),owner:"Codex".into(),expires_at_ms:now()+1000,maximum_quantity:2,maximum_cost:"1.970000".into()};let token=runtime.cohorts.physical_decision(&f.bank,&f.log,&c.scope,id,&request,&permit,&runtime.authority,now()).unwrap();token.authorize(&c.scope,&request,&permit,now()).unwrap();
   assert_eq!(f.bank.engine().order_records(),orders);for key in ["cash","ownedCost","realized","fees","positions"]{assert_eq!(before[key],f.bank.engine().report().unwrap()[key],"{key}");}assert_eq!(fs::read(f.root.0.join("dispatch.ndjson")).unwrap(),dispatch);assert!(f.log.intents.is_empty()&&f.log.attempted.is_empty()&&f.bank.engine().live_order_bindings().is_empty()&&f.bank.engine().fills().is_empty());
   f.log.append(json!({"kind":"intent","qid":id,"quote":q,"spec":request,"fundamental":runtime.cohorts.durable_tag(id).unwrap()})).unwrap();let one_intent=fs::read(f.root.0.join("dispatch.ndjson")).unwrap();let mut no_reads=Reads::new();assert!(passive_reserved_preflight(&c,&mut f.bank,&f.log,&mut source,&runtime,&q,&mut no_reads).unwrap().is_none());assert!(no_reads.endpoints.is_empty());assert_eq!(fs::read(f.root.0.join("dispatch.ndjson")).unwrap(),one_intent);
  }
 }
 #[test]fn passive_reserved_stale_book_retry_is_priority_but_still_requires_seven_fresh_facts(){
  let(mut f,mut runtime,c,mut source,q)=setup_at(now()-16000);let id=q["id"].as_u64().unwrap();let at=now();
  let books=moloch_live_runtime::package_admission::native_books(&f.bank,&c.scope,at,10000).unwrap().0;assert!(!books.contains_key("101"));
  let raw_hint=runtime.cohorts.existing_exit_hint(&f.bank,&f.log,&c.scope,"101",runtime.authority.trusted(),at).unwrap().unwrap();assert_eq!(raw_hint.reason,"reviewed_inventory_book_refresh");assert!(!runtime.eligible(&f.bank,&f.log,&c.scope,id,at).unwrap());
  let head=f.bank.durable_head();let before=f.bank.engine().report().unwrap();let orders=f.bank.engine().order_records();let dispatch=fs::read(f.root.0.join("dispatch.ndjson")).unwrap();
  let hint=existing_exit_read_hint(&f.bank,&f.log,&c.scope,&runtime,"101",at).unwrap().unwrap();assert_eq!(hint.reason,"reviewed_passive_reservation_retry");assert_eq!((hint.exchange_id.as_str(),hint.market_id.as_str(),hint.side.as_str()),("101","201","NO"));assert_eq!(f.bank.durable_head(),head,"nomination changes no source clock or financial state");
  let mut stale=Reads::new();stale.stale=true;assert!(passive_reserved_preflight(&c,&mut f.bank,&f.log,&mut source,&runtime,&q,&mut stale).unwrap().is_none());assert_eq!(stale.endpoints.len(),7);assert_eq!(f.bank.durable_head(),head,"stale final packet is not admitted");
  let mut fresh=Reads::new();let(facts,until)=passive_reserved_preflight(&c,&mut f.bank,&f.log,&mut source,&runtime,&q,&mut fresh).unwrap().unwrap();assert_eq!(fresh.endpoints.len(),7);assert_eq!(fresh.services,7);assert_eq!(json!(fresh.returned),serde_json::to_value([&facts.account,&facts.positions,&facts.open_orders,&facts.cash_balance,&facts.market,&facts.contract,&facts.book]).unwrap());
  assert_eq!(existing_reservation_stage(&quote(&f.bank,id).unwrap(),&q,now(),until).unwrap(),None);assert!(runtime.eligible(&f.bank,&f.log,&c.scope,id,now()).unwrap());let request=f.spec(&q);runtime.cohorts.bind(&f.bank,&f.log,&c.scope,id,&request,runtime.authority.trusted(),now()).unwrap();
  let permit=Permit{run_id:c.activation.run_id.clone(),intent_id:id.to_string(),owner:"Codex".into(),expires_at_ms:now()+1000,maximum_quantity:2,maximum_cost:"1.970000".into()};let token=runtime.cohorts.physical_decision(&f.bank,&f.log,&c.scope,id,&request,&permit,&runtime.authority,now()).unwrap();token.authorize(&c.scope,&request,&permit,now()).unwrap();
  assert_eq!(quote(&f.bank,id).unwrap(),q);assert_eq!(f.bank.engine().order_records(),orders);for key in ["cash","ownedCost","realized","fees","positions"]{assert_eq!(before[key],f.bank.engine().report().unwrap()[key],"{key}");}assert_eq!(fs::read(f.root.0.join("dispatch.ndjson")).unwrap(),dispatch);assert!(f.log.intents.is_empty()&&f.log.attempted.is_empty()&&f.bank.engine().live_order_bindings().is_empty()&&f.bank.engine().fills().is_empty());
 }
 #[test]fn passive_reserved_retry_priority_never_promotes_revoked_expired_or_dispatched_quotes(){
  for case in ["revoked","expired","attempt","ack"]{let(mut f,mut runtime,c,_source,q)=setup_at(now()-16000);let id=q["id"].as_u64().unwrap();let mut at=now();
   match case{
    "revoked"=>{let mut review=f.review.clone();review.reviewed_at_ms=at;review.revoke_existing=true;let checked=f.verify(review,at);runtime.cohorts.install(checked,&f.bank,&f.log,at).unwrap();let path=f.root.0.join("retry-authority.json");let bytes=serde_json::to_vec(&json!({"version":1,"trustedReviewSha256":f.trust})).unwrap();fs::write(&path,&bytes).unwrap();runtime.authority=ReviewAuthority::load(&path,&fixture::sha(&bytes)).unwrap();},
    "expired"=>at=q["expiresAt"].as_u64().unwrap(),
    _=>{f.log.append(json!({"kind":"intent","qid":id})).unwrap();f.log.append(json!({"kind":"attempt","qid":id})).unwrap();if case=="ack"{f.log.append(json!({"kind":"ack","qid":id,"receipt":{"orderId":"901"}})).unwrap();}}
   }
   let head=f.bank.durable_head();let original=quote(&f.bank,id).unwrap();let result=existing_exit_read_hint(&f.bank,&f.log,&c.scope,&runtime,"101",at);
   if case=="attempt"{assert!(result.is_err(),"unknown attempt remains fatal");}else{let hint=result.unwrap();assert!(hint.as_ref().is_none_or(|h|h.reason!="reviewed_passive_reservation_retry"),"{case}");if case=="revoked"{assert_eq!(hint.unwrap().reason,"reviewed_source_revocation");}}
   assert_eq!(f.bank.durable_head(),head);assert_eq!(quote(&f.bank,id).unwrap(),original);assert_eq!(f.bank.engine().order_records().iter().filter(|q|q["retired"]==false&&q["exchangeId"]=="101").count(),1);
  }
 }
 #[test]fn passive_reserved_attempt_unknown_ack_native_binding_and_cancel_never_retry(){
  for case in ["attempt","unknown","ack","binding","cancel"]{let(mut f,_runtime,c,_source,q)=setup();let id=q["id"].as_u64().unwrap();
   match case{
    "attempt"|"ack"=>{f.log.append(json!({"kind":"intent","qid":id})).unwrap();f.log.append(json!({"kind":"attempt","qid":id})).unwrap();if case=="ack"{f.log.append(json!({"kind":"ack","qid":id,"receipt":{"orderId":"901"}})).unwrap();}},
    "unknown"=>{f.log.append(json!({"kind":"intent","qid":999})).unwrap();f.log.append(json!({"kind":"attempt","qid":999})).unwrap();},
    "binding"=>{let at=now();let e=json!({"type":"actual_order_ack","source":"official_sig_api_v1","receiptHash":"a".repeat(64),"orderId":id,"actualOrderId":"901","exchangeId":"101","marketId":"201","side":"NO","action":"sell","quantity":q["quantity"],"price":q["price"],"receivedAt":iso(at),"processedAt":iso(at)});f.event(e);},
    _=>f.log.append(json!({"kind":"cancel_attempt","qid":id})).unwrap()
   }
   let head=f.bank.durable_head();let bytes=fs::read(f.root.0.join("dispatch.ndjson")).unwrap();let nomination=passive_reserved_exit(&f.bank,&f.log,&c.scope,"101",now());assert!(nomination.is_err()||nomination.unwrap().is_none(),"{case}");assert_eq!(f.bank.durable_head(),head);assert_eq!(fs::read(f.root.0.join("dispatch.ndjson")).unwrap(),bytes);
  }
 }
 #[test]fn passive_reserved_stale_packet_changed_quote_and_service_failure_cannot_submit(){
  for case in ["stale","changed","service"]{let(mut f,runtime,c,mut source,q)=setup();let id=q["id"].as_u64().unwrap();let before=f.bank.engine().wallet.cash;let mut reads=Reads::new();
   if case=="stale"{reads.stale=true;}else if case=="service"{reads.fail_at=Some(2);}else{let proof=f.log.proof(id);f.log.append(json!({"kind":"not_sent","qid":id,"proof":proof})).unwrap();reads.retirement=Some(json!({"type":"unsubmitted_intent_retired","source":"moloch_live_dispatcher","orderId":id,"confirmedNetworkAttempts":0,"receiptHash":hash(&proof),"deferPlanningForPackage":true}));}
   let result=passive_reserved_preflight(&c,&mut f.bank,&f.log,&mut source,&runtime,&q,&mut reads);
   if case=="service"{assert_eq!(result.err().expect("durability error is fatal").to_string(),"FIXTURE_DURABLE_SERVICE_FAILURE");}else{assert!(result.unwrap().is_none());}
   assert_eq!(reads.endpoints.len(),if case=="stale"{7}else{1});assert_eq!(f.bank.engine().wallet.cash,before);assert!(f.log.intents.is_empty()&&f.log.attempted.is_empty()&&f.bank.engine().live_order_bindings().is_empty()&&f.bank.engine().fills().is_empty());
  }
 }
 #[test]fn passive_reserved_current_review_revocation_and_native_expiry_never_authorize(){
  let(mut f,mut runtime,c,mut source,q)=setup();let mut review=f.review.clone();review.reviewed_at_ms=now();review.revoke_existing=true;let checked=f.verify(review,now());runtime.cohorts.install(checked,&f.bank,&f.log,now()).unwrap();
  let path=f.root.0.join("retry-authority.json");let bytes=serde_json::to_vec(&json!({"version":1,"trustedReviewSha256":f.trust})).unwrap();fs::write(&path,&bytes).unwrap();runtime.authority=ReviewAuthority::load(&path,&fixture::sha(&bytes)).unwrap();
  let mut reads=Reads::new();assert!(passive_reserved_preflight(&c,&mut f.bank,&f.log,&mut source,&runtime,&q,&mut reads).unwrap().is_none());assert_eq!(reads.endpoints.len(),7);assert!(f.log.intents.is_empty()&&f.log.attempted.is_empty());
  assert!(passive_reserved_exit(&f.bank,&f.log,&c.scope,"101",q["expiresAt"].as_u64().unwrap()).unwrap().is_none());
 }
}
#[cfg(test)]mod reviewed_existing_worker_tests{
 use super::*;
 struct Temp(PathBuf);impl Temp{fn new()->Self{static NEXT:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);Self(std::env::temp_dir().join(format!("reviewed-existing-worker-{}-{}-{}",std::process::id(),now(),NEXT.fetch_add(1,std::sync::atomic::Ordering::Relaxed))))}}impl Drop for Temp{fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}
 const TID:&str="11111111-1111-1111-1111-111111111111";
 fn scope()->Scope{Scope{tournament_id:TID.into(),tournament_slug:"cup".into(),exchanges:[("101".into(),"201".into())].into()}}
 fn fixture()->(Temp,Session,DispatchLog,Value,Value,Value){fixture_at(now()-7500)}
 fn fixture_at(at:u64)->(Temp,Session,DispatchLog,Value,Value,Value){
  let root=Temp::new();private_dir(&root.0).unwrap();
  let policy=json!({"initialCash":"1000.000000","maximumCommitted":"1000.000000","maximumMarketCost":"100.000000","maximumMarkets":3,"quantity":2,"entryStopAt":iso(at+3600000),"inventoryExitMode":"fixedGain","inventoryMinimumGain":"0.005000","maximumInventoryAgeMs":60000,"inventoryAgeClock":"execution"});
  let seed=json!({"runId":"existing-worker-fixture","availableCash":"1000.000000","accountSnapshotHash":"a".repeat(64),"observedAt":iso(at-1000)});
  let mut bank=Session::open_live(&root.0.join("bank"),&policy,&seed).unwrap();
  let adoption=json!({"type":"actual_position_adoption","source":"official_sig_api_v1","authority":"human_moloch_inherited_management_20261015","complete":true,"receiptHash":"d".repeat(64),"requestStartedAt":iso(at-1),"receivedAt":iso(at),"processedAt":iso(at),"tournamentId":TID,"openingCost":"1.900000","effectiveMaximumMarkets":4,"positions":[{"exchangeId":"101","marketId":"201","quantity":-3,"lotPrincipal":"1.900000","reportedCostBasis":"1.900000","reportedMinusLotPrincipal":"0.000000","basisClassification":"exact","lots":[{"lotId":"501","quantity":1,"side":"NO","entryPrice":"0.900000","openedAt":iso(at-86400000)},{"lotId":"502","quantity":2,"side":"NO","entryPrice":"0.500000","openedAt":iso(at-43200000)}]}]});
  bank.accept("original-adoption",1,&adoption).unwrap();let at=at+1000;
  bank.accept("original-book",1,&json!({"type":"book","receivedAt":iso(at),"book":{"marketId":"201","exchangeId":"101","tournamentId":TID,"sequence":"1","sourceAt":iso(at),"requestStartedAt":iso(at),"receivedAt":iso(at),"bids":[{"price":"0.300000","quantity":100}],"asks":[{"price":"0.320000","quantity":100}]}})).unwrap();
  let log=DispatchLog::open(&root.0.join("dispatch.ndjson")).unwrap();(root,bank,log,adoption,policy,seed)
 }
 fn negative_profit_receipt(side:&str,bid:i128,at:u64,sequence:u64)->Receipt{
  let(yes_bid,yes_ask)=if side=="YES"{(bid,bid+5000)}else{(1_000_000-bid-5000,1_000_000-bid)};
  Receipt{endpoint:format!("/markets/201/orderbook?tournamentId={TID}&depth=20"),status:200,request_started_at:iso(at),received_at:iso(at),data:json!({"contexts":[{"type":"tournament","tournament":{"id":TID},"orderbook":{"exchanges":[{"exchangeId":"101","asOf":{"sequence":sequence,"at":iso(at)},"bids":[{"price":money(yes_bid),"quantity":100}],"asks":[{"price":money(yes_ask),"quantity":100}]}]}}]})}
 }
 fn negative_profit_fixture(side:&str)->(Temp,Session,DispatchLog,Control,RestSource,Value,moloch_live_runtime::exit_refresh::ReadHint){
  let temp=Temp::new();private_dir(&temp.0).unwrap();let old=now()-20000;
  let policy=json!({"initialCash":"1000.000000","quantity":5,"maximumMarkets":3,"entriesEnabled":false,"minimumQuoteAgeMs":10000,"inventoryTakerAfterMs":0,"inventoryMinimumGain":"0.005000","maximumInventoryAgeMs":60000,"inventoryAgeClock":"execution","inventoryExitMode":"fixedGain","inventoryExitDepthBps":5000});
  let seed=json!({"runId":"existing-worker-fixture","tournamentId":TID,"availableCash":"1000.000000","accountSnapshotHash":"a".repeat(64),"observedAt":iso(old-1000)});let mut bank=Session::open_live(&temp.0.join("bank"),&policy,&seed).unwrap();
  bank.accept("negative-original-adoption",1,&json!({"type":"actual_position_adoption","source":"official_sig_api_v1","authority":"human_moloch_inherited_management_20261015","complete":true,"receiptHash":"d".repeat(64),"requestStartedAt":iso(old-1),"receivedAt":iso(old),"tournamentId":TID,"openingCost":"2.500000","effectiveMaximumMarkets":4,"positions":[{"marketId":"201","exchangeId":"101","quantity":if side=="NO"{-5}else{5},"lotPrincipal":"2.500000","reportedCostBasis":"2.500000","reportedMinusLotPrincipal":"0.000000","basisClassification":"exact","lots":[{"lotId":"501","quantity":5,"side":side,"entryPrice":"0.500000","openedAt":iso(old-1000)}]}]})).unwrap();
  let c=control(&temp.0);let initial=negative_profit_receipt(side,505000,old+1000,1);let wrap=|r:&Receipt|json!({"kind":"book","source":"official_sig_api_v1","status":r.status,"marketId":"201","exchangeId":"101","tournamentId":TID,"requestStartedAt":r.request_started_at,"receivedAt":r.received_at,"endpoint":r.endpoint,"data":r.data});
  let events=moloch_replay::source::receipt_events(&wrap(&initial)).unwrap();for(i,e)in events.iter().enumerate(){bank.accept("negative-original-book",i as u64+1,e).unwrap();}
  let oldq=bank.engine().order_records().into_iter().find(|q|q["action"]=="sell"&&q["retired"]==false).unwrap();assert_eq!(moloch_live_runtime::exit_refresh::ReadKind::from_quote(&oldq),Some(moloch_live_runtime::exit_refresh::ReadKind::Profit));
  let safety=moloch_live_runtime::exit_refresh::Safety{never_attempted:true,has_intent:false,unresolved_attempts:false,global_unknown:false,stop:false,stream_healthy:true,owned_quantity:5,reserved_sell_quantity:0,same_exchange_pending_physical_order:false,lifecycle_fingerprint:[1;32],native_head_fingerprint:[1;32]};let hint=moloch_live_runtime::exit_refresh::ReadHint::from_expired_kind(&oldq,safety,now(),moloch_live_runtime::exit_refresh::ReadKind::Profit).unwrap();
  let log=DispatchLog::open(&temp.0.join("dispatch.ndjson")).unwrap();let mut source=RestSource::new(&json!([{"marketId":"201","exchangeId":"101"}]),TID,&iso(old)).unwrap();source.accept(&wrap(&initial)).unwrap();
  accept_source_events(&mut bank,&source.accept(&json!({"kind":"trades","source":"official_sig_api_v1","status":200,"marketId":"201","exchangeId":"101","tournamentId":TID,"requestStartedAt":iso(old+1000),"receivedAt":iso(old+1000),"endpoint":format!("/exchanges/101/trades?tournamentId={TID}&from={}&to={}",iso(old-1),iso(old+1000)),"data":{"marketId":"201","exchangeId":"101","from":iso(old-1),"to":iso(old+1000),"data":[],"pagination":{"hasMore":false,"nextCursor":null},"coverage":{"complete":true,"projectedThroughSequence":1}}})).unwrap()).unwrap();
  (temp,bank,log,c,source,oldq,hint)
 }
 #[test]fn live_profit_too_short_for_frozen_preflight_still_nominates_one_read_packet(){
  fn hint_safety(unknown:bool)->moloch_live_runtime::exit_refresh::Safety{moloch_live_runtime::exit_refresh::Safety{never_attempted:true,has_intent:false,unresolved_attempts:false,global_unknown:unknown,stop:false,stream_healthy:true,owned_quantity:300,reserved_sell_quantity:0,same_exchange_pending_physical_order:false,lifecycle_fingerprint:[1;32],native_head_fingerprint:[2;32]}}
  let q=json!({"id":2,"marketId":"525","exchangeId":"1012","side":"NO","action":"sell","executionStyle":"inventoryAON","filled":0,"quantity":60,"price":"0.685000","retired":false,"createdAt":18000,"activationAt":21000,"expiresAt":31000});
  assert!(!physical_preflight_ready(&q,true,false,28500));
  let mut history=q.clone();history["id"]=json!(1);history["retired"]=json!(true);history["createdAt"]=json!(0);history["activationAt"]=json!(3000);history["expiresAt"]=json!(13000);
  let(hints,_)=prepare_expired_exit_hints(&[history,q.clone()],28500,|_|true,|_,_|Ok(300),|_|Ok(true),|_,_|Ok(false),||Ok(false),|_,u|Ok(hint_safety(u))).unwrap();
  assert_eq!(hints.len(),1);assert_eq!(hints["1012:NO:sell:profit"].1.old_qid(),2);assert_eq!(hints["1012:NO:sell:profit"].0,q);
  for case in ["intent","unknown","physical","other_sell","age"]{
   let mut orders=vec![q.clone()];if case=="other_sell"{let mut sibling=q.clone();sibling["id"]=json!(3);orders.push(sibling)}
   if case=="age"{orders[0]["inventoryExitReason"]=json!("maximumAge")}
   let(hints,_)=prepare_expired_exit_hints(&orders,28500,|_|true,|_,_|Ok(300),|_|Ok(true),|_,_|Ok(true),||Ok(case=="unknown"),|_,u|Ok(moloch_live_runtime::exit_refresh::Safety{has_intent:case=="intent",same_exchange_pending_physical_order:case=="physical",..hint_safety(u)})).unwrap();assert!(hints.is_empty(),"{case}");
  }
 }
 #[test]fn post_book_replan_nomination_is_unused_inherited_only_and_never_mutates_bank(){
  for side in ["YES","NO"]{
   let(_temp,bank,mut log,c,_source,old,hint)=negative_profit_fixture(side);let id=old["id"].as_u64().unwrap();let head=bank.durable_head();let descriptor=log.descriptor_identity().unwrap();
   assert!(unused_inherited_profit_draft(&bank,&log,&c.scope,&old).unwrap());
   assert_eq!(profit_replan_draft(&bank,&log,&c.scope,&hint,id,now(),now()).unwrap(),Some(old.clone()));
   let created=old["createdAt"].as_u64().unwrap();assert!(profit_replan_draft(&bank,&log,&c.scope,&hint,0,created,created+4000).unwrap().is_none(),"a still fresh in-flow quote does not justify retirement");
   assert_eq!(bank.durable_head(),head);assert_eq!(log.descriptor_identity().unwrap(),descriptor);
   log.intents.insert(id,json!({"quote":old}));assert!(profit_replan_draft(&bank,&log,&c.scope,&hint,id,now(),now()).unwrap().is_none(),"zero attempts with an intent is still occupied");log.intents.remove(&id);
   let mut changed=old.clone();changed["filled"]=json!(1);assert!(!unused_inherited_profit_draft(&bank,&log,&c.scope,&changed).unwrap());changed=old.clone();changed["reviewedInventoryExit"]=json!({"protected":true});assert!(!unused_inherited_profit_draft(&bank,&log,&c.scope,&changed).unwrap());
  }
 }
 #[test]fn native_ack_blocks_unused_profit_retirement_even_if_dispatch_attempt_is_absent(){
  let(_temp,mut bank,log,c,_source,old,hint)=negative_profit_fixture("NO");let id=old["id"].as_u64().unwrap();
  accept(&mut bank,&json!({"type":"actual_order_ack","source":"official_sig_api_v1","receiptHash":"a".repeat(64),"orderId":id,"actualOrderId":"901","exchangeId":"101","marketId":"201","side":"NO","action":"sell","quantity":old["quantity"],"price":old["price"],"receivedAt":iso(now())})).unwrap();
  assert!(log.never_attempted(id));assert!(profit_replan_draft(&bank,&log,&c.scope,&hint,id,now(),now()).unwrap().is_none());assert_eq!(bank.engine().live_order_bindings().get(&id).map(String::as_str),Some("901"));
 }
 #[test]fn negative_profit_preparation_uses_actual_yes_no_admission_and_never_mutates_lifecycle(){
  for side in ["YES","NO"]{let(temp,mut bank,log,c,mut source,old,hint)=negative_profit_fixture(side);let r=negative_profit_receipt(side,490000,now(),2);let events=accept_fresh_book(&mut bank,&c,&old,&r,&mut source).unwrap();let head=bank.durable_head();let report=bank.engine().report().unwrap();let bytes=fs::read(temp.0.join("bank/events.jsonl")).unwrap();let descriptor=log.descriptor_identity().unwrap();
   assert!(negative_profit_preparation(&bank,&c.scope,&log,&old,&hint,&r,&events,&head,false,now()),"{side}");
   assert_eq!(bank.durable_head(),head);assert_eq!(bank.engine().report().unwrap(),report);assert_eq!(fs::read(temp.0.join("bank/events.jsonl")).unwrap(),bytes);assert_eq!(log.descriptor_identity().unwrap(),descriptor);assert!(log.intents.is_empty()&&log.attempted.is_empty());
  }
 }
 #[test]fn negative_profit_preparation_uses_final_actual_recovery_state(){
  for side in ["YES","NO"]{for recovered in [false,true]{
   let(_temp,mut bank,log,c,_source,old,hint)=negative_profit_fixture(side);
   let started=now()-2;let mut source=RestSource::new(&json!([{"marketId":"201","exchangeId":"101"}]),TID,&iso(started)).unwrap();
   if recovered{let through=now();accept_source_events(&mut bank,&source.accept(&json!({"kind":"trades","source":"official_sig_api_v1","status":200,"marketId":"201","exchangeId":"101","tournamentId":TID,"requestStartedAt":iso(through),"receivedAt":iso(through),"endpoint":format!("/exchanges/101/trades?tournamentId={TID}&from={}&to={}",iso(started-1),iso(through)),"data":{"marketId":"201","exchangeId":"101","from":iso(started-1),"to":iso(through),"data":[],"pagination":{"hasMore":false,"nextCursor":null},"coverage":{"complete":true,"projectedThroughSequence":2}}})).unwrap()).unwrap();}
   let r=negative_profit_receipt(side,490000,now(),2);let mut events=accept_fresh_book(&mut bank,&c,&old,&r,&mut source).unwrap();
   assert!(events.iter().any(|e|e["type"]=="book"&&e["book"]["invalid"]==true),"real blocked source must first admit invalid book");
   assert_eq!(events.iter().any(|e|e["type"]=="book"&&e["book"]["invalid"]!=true),recovered,"only complete recovery emits actual valid replacement");
   assert_eq!(negative_profit_preparation(&bank,&c.scope,&log,&old,&hint,&r,&events,&bank.durable_head(),false,now()),recovered,"{side}/{recovered}");
   if recovered{let invalid=events.iter().find(|e|e["type"]=="book"&&e["book"]["invalid"]==true).unwrap().clone();accept(&mut bank,&invalid).unwrap();events.push(invalid);assert!(!negative_profit_preparation(&bank,&c.scope,&log,&old,&hint,&r,&events,&bank.durable_head(),false,now()),"final invalidation must deny even after valid replacement");}
  }}
 }
 #[test]fn profit_preparation_equality_and_positive_bounds_keep_full_final_path(){
  for(side,bid)in [("YES",505000),("NO",505000),("YES",510000),("NO",510000)]{let(_temp,mut bank,log,c,mut source,old,hint)=negative_profit_fixture(side);let r=negative_profit_receipt(side,bid,now(),2);let events=accept_fresh_book(&mut bank,&c,&old,&r,&mut source).unwrap();assert!(!negative_profit_preparation(&bank,&c.scope,&log,&old,&hint,&r,&events,&bank.durable_head(),false,now()),"{side}/{bid}");}
 }
 #[test]fn negative_profit_preparation_management_is_target_specific_and_native_guarded(){
  for kind in ["ordinary_entry_exclusions_v1","managed_inventory_planning_v1"]{
   let(_temp,mut bank,log,c,mut source,old,hint)=negative_profit_fixture("NO");let r=negative_profit_receipt("NO",490000,now(),2);let events=accept_fresh_book(&mut bank,&c,&old,&r,&mut source).unwrap();
   for target in ["102","101"]{
    let stamp=iso(now());let mut e=json!({"type":kind,"source":"moloch_live_dispatcher","receivedAt":stamp,"processedAt":stamp,"bankScope":{"runId":"existing-worker-fixture","bankManifestHash":bank.manifest_hash(),"tournamentId":TID},"bankHead":bank.durable_head(),"expectedReportSha256":hash(&bank.engine().report().unwrap()),"managedExchangeIds":[target],"approvedReviewSetSha256":"a".repeat(64),"controlSha256":"b".repeat(64)});
    if kind=="ordinary_entry_exclusions_v1"{e["expectedPreviousExclusionSha256"]=bank.engine().ordinary_entry_exclusion_record().map_or(Value::Null,|v|v["sha256"].clone());}
    else{e["generationSha256"]=json!("c".repeat(64));e["expectedPreviousModeSha256"]=bank.engine().managed_inventory_planning_record().map_or(Value::Null,|v|v["sha256"].clone());}
    accept(&mut bank,&e).unwrap();
    assert_eq!(negative_profit_preparation(&bank,&c.scope,&log,&old,&hint,&r,&events,&bank.durable_head(),false,now()),target!="101","{kind}/{target}: unrelated manager must not mask target; caller false must not bypass target-native management");
   }
  }
 }
 #[test]fn negative_profit_preparation_never_vetoes_uncertain_changed_or_reviewed_context(){
  let(_temp,mut bank,mut log,c,mut source,old,hint)=negative_profit_fixture("NO");let r=negative_profit_receipt("NO",490000,now(),2);let events=accept_fresh_book(&mut bank,&c,&old,&r,&mut source).unwrap();let head=bank.durable_head();let at=now();
  assert!(!negative_profit_preparation(&bank,&c.scope,&log,&old,&hint,&r,&[],&head,false,at));
  assert!(!negative_profit_preparation(&bank,&c.scope,&log,&old,&hint,&r,&events,&head,true,at));
  assert!(!negative_profit_preparation(&bank,&c.scope,&log,&old,&hint,&r,&events,&head,false,at+10001));
  assert!(!negative_profit_preparation(&bank,&c.scope,&log,&old,&hint,&r,&events,&head,false,moloch_live_runtime::daily_windows::clock(&r.received_at).unwrap()-1));
  for field in ["endpoint","status","data"]{let mut changed=r.clone();match field{"endpoint"=>changed.endpoint.push_str("&wrong=1"),"status"=>changed.status=503,_=>changed.data=Value::Null};assert!(!negative_profit_preparation(&bank,&c.scope,&log,&old,&hint,&changed,&events,&head,false,at));}
  let mut wrong=c.scope.clone();wrong.exchanges.insert("101".into(),"999".into());assert!(!negative_profit_preparation(&bank,&wrong,&log,&old,&hint,&r,&events,&head,false,at));
  let mut reviewed=old.clone();reviewed["reviewedInventoryExit"]=json!({"version":1});assert!(!negative_profit_preparation(&bank,&c.scope,&log,&reviewed,&hint,&r,&events,&head,false,at));
  let mut partial=old.clone();partial["filled"]=json!(1);assert!(!negative_profit_preparation(&bank,&c.scope,&log,&partial,&hint,&r,&events,&head,false,at));
  let mut gapped=events.clone();gapped.push(json!({"type":"gap","exchangeId":"101"}));assert!(!negative_profit_preparation(&bank,&c.scope,&log,&old,&hint,&r,&gapped,&head,false,at));
  let mut absent=old.clone();absent["side"]=json!("YES");assert!(!negative_profit_preparation(&bank,&c.scope,&log,&absent,&hint,&r,&events,&head,false,at));
  accept(&mut bank,&json!({"type":"tick","receivedAt":iso(now())})).unwrap();assert!(!negative_profit_preparation(&bank,&c.scope,&log,&old,&hint,&r,&events,&head,false,now()));
  accept(&mut bank,&json!({"type":"gap","exchangeId":"101","receivedAt":iso(now())})).unwrap();assert!(!negative_profit_preparation(&bank,&c.scope,&log,&old,&hint,&r,&events,&bank.durable_head(),false,now()));
  log.append(json!({"kind":"intent","qid":hint.old_qid(),"quote":old})).unwrap();assert!(!negative_profit_preparation(&bank,&c.scope,&log,&old,&hint,&r,&events,&bank.durable_head(),false,now()));
 }
 #[test]fn real_adopted_fifo_lots_have_no_new_entry_deadline_and_restart_preserves_that(){
  let at=now()-20000;let(root,bank,_log,_,policy,seed)=fixture_at(at);let before=bank.durable_head();let report=bank.engine().report().unwrap();
  assert_eq!(bank.engine().wallet.quantity("101",moloch_replay::Side::No),3);
  assert!(!remaining_exit_age_due(bank.engine().policy(),&report["positions"],&report["inheritedInventory"],"101","NO",at+100000).unwrap());
  // Missing positive native provenance never turns an unclocked lot into a safe skip.
  assert!(remaining_exit_age_due(bank.engine().policy(),&report["positions"],&Value::Null,"101","NO",at+100000).unwrap());
  assert_eq!(bank.durable_head(),before);drop(bank);
  let restored=Session::open_live(&root.0.join("bank"),&policy,&seed).unwrap();let r=restored.engine().report().unwrap();
  assert!(!remaining_exit_age_due(restored.engine().policy(),&r["positions"],&r["inheritedInventory"],"101","NO",at+100000).unwrap());assert_eq!(restored.durable_head(),before);
 }
 fn control(root:&Path)->Control{serde_json::from_value(json!({"activation":{"runId":"existing-worker-fixture","entryUntilMs":now()+3600000,"maximumCommitted":"1000.000000","maximumMarketCost":"100.000000","maximumQuantity":2,"tournamentId":TID,"tournamentSlug":"cup","binarySha256":"a".repeat(64)},"scope":scope(),"policy":{},"root":root,"credentials":"/unused","ownerLock":root.join("owner.lock"),"accountScope":"synthetic-existing-worker"})).unwrap()}
 fn receipt(data:Value,at:u64)->Receipt{Receipt{endpoint:"synthetic original receipt".into(),request_started_at:iso(at-1),received_at:iso(at),status:200,data}}
 struct PrefilterReads{at:u64,allow:bool,book_price:i128,endpoints:Vec<Endpoint>,guards:usize,book_reads:u64}
 impl OrdinaryPreflightReads for PrefilterReads{
  fn now_ms(&self)->u64{self.at}
  fn guarded(&mut self)->Result<bool>{self.guards+=1;Ok(self.allow)}
  fn read(&mut self,ep:Endpoint)->Result<Receipt>{
   self.endpoints.push(ep.clone());let mut r=receipt(match &ep{
    Endpoint::Tournament=>json!({"status":"active"}),
    Endpoint::Orders{..}=>json!({"coverage":{"complete":true},"pagination":{"hasMore":false},"data":[]}),
    Endpoint::OrderBook{..}=>{self.book_reads+=1;json!({"contexts":[{"type":"tournament","tournament":{"id":TID},"orderbook":{"exchanges":[{"exchangeId":"101","asOf":{"sequence":1+self.book_reads,"at":iso(self.at-1)},"bids":[{"price":money(self.book_price),"quantity":100}],"asks":[{"price":money(self.book_price+20000),"quantity":100}]}]}}]})},
    _=>json!({})},self.at);
   if matches!(ep,Endpoint::OrderBook{..}){r.endpoint=format!("/markets/201/orderbook?tournamentId={TID}&depth=20");}Ok(r)
  }
 }
 fn prefilter_fixture()->(Temp,Session,DispatchLog,Control,RestSource,Value){
  let old=now()-20000;let(root,mut bank,log,_,_,_)=fixture_at(old);let c=control(&root.0);
  let mut source=RestSource::new(&json!([{"marketId":"201","exchangeId":"101"}]),TID,&iso(old)).unwrap();
  // Seed the same ORIGINAL old book before its original tape coverage. This
  // models an already recovered source, not a new reconnect invalidation.
  source.accept(&json!({"kind":"book","source":"official_sig_api_v1","status":200,"marketId":"201","exchangeId":"101","tournamentId":TID,"requestStartedAt":iso(old+1000),"receivedAt":iso(old+1000),"endpoint":format!("/markets/201/orderbook?tournamentId={TID}&depth=20"),"data":{"contexts":[{"type":"tournament","tournament":{"id":TID},"orderbook":{"exchanges":[{"exchangeId":"101","asOf":{"sequence":1,"at":iso(old+1000)},"bids":[{"price":"0.300000","quantity":100}],"asks":[{"price":"0.320000","quantity":100}]}]}}]}})).unwrap();
  let r=json!({"kind":"trades","source":"official_sig_api_v1","status":200,"marketId":"201","exchangeId":"101","tournamentId":TID,"requestStartedAt":iso(old+1000),"receivedAt":iso(old+1000),"endpoint":format!("/exchanges/101/trades?tournamentId={TID}&from={}&to={}",iso(old-1),iso(old+1000)),"data":{"marketId":"201","exchangeId":"101","from":iso(old-1),"to":iso(old+1000),"data":[],"pagination":{"hasMore":false,"nextCursor":null},"coverage":{"complete":true,"projectedThroughSequence":1}}});
  for(i,e)in source.accept(&r).unwrap().iter().enumerate(){bank.accept("prefilter-original-coverage",i as u64+1,e).unwrap();}
  let q=bank.engine().order_records().into_iter().find(|q|q["action"]=="sell"&&q["retired"]==false).unwrap();
  assert_eq!(moloch_live_runtime::quote_priority::from_native(&bank,&c.scope,&[q.clone()],now(),None).unwrap().assessments[0].decision.class,moloch_live_runtime::quote_priority::Class::NeedsRefresh);
  (root,bank,log,c,source,q)
 }
 struct BulkReads{inner:PrefilterReads,scope:Scope,mode:&'static str,delay:u64,deny_after:usize}
 impl OrdinaryPreflightReads for BulkReads{
  fn now_ms(&self)->u64{self.inner.at}
  fn guarded(&mut self)->Result<bool>{self.inner.guards+=1;Ok(self.inner.allow&&self.inner.guards<self.deny_after)}
  fn read(&mut self,ep:Endpoint)->Result<Receipt>{
   if let Endpoint::Prices{exchange_ids}=&ep{
    self.inner.endpoints.push(ep.clone());if self.mode=="read_error"{return Err("FIXTURE_BULK_READ_ERROR".into())}
    let started=self.inner.at;self.inner.at+=self.delay;
    let rows:Vec<_>=exchange_ids.iter().map(|ex|{let bid=if self.mode=="passing"&&ex=="101"{0.095}else{0.4};json!({"exchangeId":ex,"marketId":self.scope.exchanges[ex],"option":if self.mode=="null"{Value::Null}else{json!("YES")},"latestPrice":bid,"bestBid":bid,"bestAsk":if ex=="102"{0.45}else{bid+0.02},"spread":if ex=="102"{0.05}else{0.02}})}).collect();
    let data=if self.mode=="missing"{json!({"data":[],"missingIds":exchange_ids})}else{json!({"data":rows,"missingIds":[]})};
    return Ok(Receipt{endpoint:if self.mode=="wrong_scope"{"/exchanges/prices?foreign".into()}else{format!("/exchanges/prices?ids={}&tournamentId={}",exchange_ids.join(","),self.scope.tournament_id)},request_started_at:iso(started),received_at:iso(self.inner.at),status:200,data})
   }self.inner.read(ep)
  }
 }
 fn bulk_fixture()->(Temp,Session,DispatchLog,Control,RestSource,Vec<Value>,Vec<(String,usize,bool)>,moloch_live_runtime::quote_priority::Selection){
  let(root,mut bank,log,mut c,source,q)=prefilter_fixture();c.bulk_negative_screen=true;c.scope.exchanges.insert("102".into(),"202".into());let at=now()-15000;
  bank.accept("bulk-fixture-book",1,&json!({"type":"book","receivedAt":iso(at),"book":{"marketId":"202","exchangeId":"102","tournamentId":TID,"sequence":"1","sourceAt":iso(at),"requestStartedAt":iso(at),"receivedAt":iso(at),"bids":[{"price":"0.400000","quantity":100}],"asks":[{"price":"0.450000","quantity":100}]}})).unwrap();
  let other=bank.engine().order_records().into_iter().find(|q|q["exchangeId"]=="102"&&q["action"]=="buy"&&q["retired"]==false).unwrap();
  let quotes=vec![q,other];let physical=quotes.iter().enumerate().map(|(i,q)|(format!("{}:{}:{}",q["exchangeId"].as_str().unwrap(),q["side"].as_str().unwrap(),q["action"].as_str().unwrap()),i,false)).collect();
  let selection=moloch_live_runtime::quote_priority::from_native(&bank,&c.scope,&quotes,now(),None).unwrap();(root,bank,log,c,source,quotes,physical,selection)
 }
 fn bulk_reads(c:&Control,mode:&'static str)->BulkReads{BulkReads{inner:PrefilterReads{at:now(),allow:true,book_price:95000,endpoints:vec![],guards:0,book_reads:0},scope:c.scope.clone(),mode,delay:0,deny_after:usize::MAX}}
 #[test]fn bulk_default_disabled_and_single_scope_never_add_an_optional_read(){let(_r,_b,_l,mut c,_s,q,p,a)=bulk_fixture();assert!(!control(Path::new("/fixture")).bulk_negative_screen);c.bulk_negative_screen=false;let mut reads=bulk_reads(&c,"far");assert!(bulk_negative_vetoes(&c,&q,&p,&a,&Default::default(),0,&mut reads).unwrap().is_empty());assert!(reads.inner.endpoints.is_empty());c.bulk_negative_screen=true;assert!(bulk_negative_vetoes(&c,&q,&p[..1],&a,&Default::default(),0,&mut reads).unwrap().is_empty());assert!(reads.inner.endpoints.is_empty());}
 #[test]fn bulk_urgent_reviewed_and_managed_choices_bypass_the_extra_read(){for kind in ["urgent","reviewed","managed"]{let(_r,_b,_l,c,_s,mut q,mut p,a)=bulk_fixture();let mut managed=std::collections::BTreeSet::new();match kind{"urgent"=>p[0].2=true,"reviewed"=>q[0]["reviewedInventoryExit"]=json!({"source":"actualfixture"}),_=>{managed.insert("101".into());}}let mut reads=bulk_reads(&c,"far");assert!(bulk_negative_vetoes(&c,&q,&p,&a,&managed,0,&mut reads).unwrap().is_empty());assert!(reads.inner.endpoints.is_empty(),"{kind}");}}
 #[test]fn one_original_bulk_veto_bound_to_real_native_quote_changes_no_bank_or_intent(){let(_r,b,l,c,_s,q,p,a)=bulk_fixture();let before=b.engine().report().unwrap();let mut reads=bulk_reads(&c,"far");let vetoes=bulk_negative_vetoes(&c,&q,&p,&a,&Default::default(),0,&mut reads).unwrap();assert_eq!(reads.inner.endpoints.len(),1);assert!(matches!(&reads.inner.endpoints[0],Endpoint::Prices{exchange_ids}if exchange_ids.len()==2));assert!(vetoes.iter().any(|(i,_)|*i==0));let veto=&vetoes.iter().find(|(i,_)|*i==0).unwrap().1;assert!(veto.matches_quote(&c.scope,&q[0],reads.now_ms()).unwrap());let mut changed=q[0].clone();changed["price"]=json!("0.900000");assert!(!veto.matches_quote(&c.scope,&changed,reads.now_ms()).unwrap());assert_eq!(b.engine().report().unwrap(),before);assert!(l.intents.is_empty()&&l.attempted.is_empty());}
 #[test]fn original_negative_screen_rotates_to_another_real_native_scope_in_the_same_pass(){let(_r,b,l,c,_s,q,mut p,a)=bulk_fixture();let before=b.engine().report().unwrap();let mut reads=bulk_reads(&c,"far");let vetoes=bulk_negative_vetoes(&c,&q,&p,&a,&Default::default(),0,&mut reads).unwrap();assert!(apply_bulk_vetoes(&c.scope,&q,&mut p,&vetoes,0,reads.now_ms()).unwrap());assert_eq!(p.len(),1);assert_eq!(p[0].1,1);let cursor=format!("physical:{}",format!("{}:{}:{}",q[0]["exchangeId"].as_str().unwrap(),q[0]["side"].as_str().unwrap(),q[0]["action"].as_str().unwrap()));let next=shared_preflight_rotation(&[],&[],&p,Some(&cursor)).unwrap();assert!(matches!(next.0,SharedPreflight::Physical(1)));assert_eq!(reads.inner.endpoints.len(),1);assert_eq!(b.engine().report().unwrap(),before);assert!(l.intents.is_empty()&&l.attempted.is_empty());}
 #[test]fn bulk_veto_expiry_at_rotation_preserves_both_original_candidates(){let(_r,_b,_l,c,_s,q,mut p,a)=bulk_fixture();let mut reads=bulk_reads(&c,"far");let vetoes=bulk_negative_vetoes(&c,&q,&p,&a,&Default::default(),0,&mut reads).unwrap();assert!(!vetoes.is_empty());assert!(!apply_bulk_vetoes(&c.scope,&q,&mut p,&vetoes,0,reads.now_ms()+1500).unwrap());assert_eq!(p.len(),2);}
 #[test]fn bulk_late_missing_null_or_wrong_endpoint_is_never_positive_authority(){for mode in ["late","missing","null","wrong_scope"]{let(_r,b,l,c,_s,q,p,a)=bulk_fixture();let before=b.engine().report().unwrap();let mut reads=bulk_reads(&c,mode);if mode=="late"{reads.delay=1500;}assert!(bulk_negative_vetoes(&c,&q,&p,&a,&Default::default(),0,&mut reads).unwrap().is_empty(),"{mode}");assert_eq!(reads.inner.endpoints.len(),1);assert_eq!(b.engine().report().unwrap(),before);assert!(l.attempted.is_empty());}}
 #[test]fn passing_bulk_still_reads_the_entire_original_seven_facts_and_prefilter_book(){let(_r,mut b,l,c,mut s,q,p,a)=bulk_fixture();let mut reads=bulk_reads(&c,"passing");let vetoes=bulk_negative_vetoes(&c,&q,&p,&a,&Default::default(),0,&mut reads).unwrap();assert!(!vetoes.iter().any(|(i,_)|*i==0));let(current,facts)=ordinary_preflight(&c,&mut b,&l,&mut s,&q[0],&mut reads).unwrap().unwrap();assert_eq!(current["id"],q[0]["id"]);assert_eq!(reads.inner.endpoints.len(),9);assert!(matches!(reads.inner.endpoints[0],Endpoint::Prices{..}));assert_eq!(reads.inner.book_reads,2);assert_eq!(facts.book.data["contexts"][0]["orderbook"]["exchanges"][0]["asOf"]["sequence"],3);assert!(l.intents.is_empty());}
 #[test]fn denied_or_failed_optional_read_preserves_existing_stop_and_read_errors(){for kind in ["before","after","read_error"]{let(_r,_b,_l,c,_s,q,p,a)=bulk_fixture();let mut reads=bulk_reads(&c,kind);if kind=="before"{reads.inner.allow=false;}if kind=="after"{reads.deny_after=2;}let result=bulk_negative_vetoes(&c,&q,&p,&a,&Default::default(),0,&mut reads);if kind=="read_error"{assert_eq!(result.err().unwrap().to_string(),"FIXTURE_BULK_READ_ERROR");}else{assert!(result.unwrap().is_empty());}assert_eq!(reads.inner.endpoints.len(),usize::from(kind!="before"));}}
 #[test]fn fresh_ordinary_choice_does_not_pay_bulk_cost_for_other_stale_candidates(){let(_r,_b,_l,c,_s,q,p,mut a)=bulk_fixture();a.assessments[0].decision.class=moloch_live_runtime::quote_priority::Class::Competitive;let mut reads=bulk_reads(&c,"far");assert!(bulk_negative_vetoes(&c,&q,&p,&a,&Default::default(),0,&mut reads).unwrap().is_empty());assert!(reads.inner.endpoints.is_empty());}
 #[test]fn foreign_scope_before_optional_read_fails_without_any_request(){let(_r,_b,_l,c,_s,mut q,p,a)=bulk_fixture();q[1]["marketId"]=json!("999");let mut reads=bulk_reads(&c,"far");assert!(bulk_negative_vetoes(&c,&q,&p,&a,&Default::default(),0,&mut reads).is_err());assert!(reads.inner.endpoints.is_empty());}
 #[test]fn stale_far_native_sell_reads_only_original_book_and_never_an_account_packet(){
  let(_root,mut bank,log,c,mut source,q)=prefilter_fixture();let before=bank.engine().report().unwrap();let mut r=PrefilterReads{at:now(),allow:true,book_price:300000,endpoints:vec![],guards:0,book_reads:0};
  assert!(ordinary_preflight(&c,&mut bank,&log,&mut source,&q,&mut r).unwrap().is_none());assert_eq!(r.endpoints.len(),1);assert!(matches!(r.endpoints[0],Endpoint::OrderBook{..}));
  let current=quote(&bank,q["id"].as_u64().unwrap()).unwrap();assert_eq!(current["price"],q["price"]);assert_eq!(current["expiresAt"],q["expiresAt"]);assert_eq!(current["quantity"],q["quantity"]);assert!(log.intents.is_empty());assert!(log.attempted.is_empty());
  let after=bank.engine().report().unwrap();for key in ["cash","ownedCost","realized","fills"]{assert_eq!(before[key],after[key],"{key}");}
 }
 #[test]fn refreshed_competitive_native_sell_still_requires_seven_new_independent_facts(){
  let(_root,mut bank,log,c,mut source,q)=prefilter_fixture();let price=decimal_units(q["price"].as_str().unwrap()).unwrap();
  let mut r=PrefilterReads{at:now(),allow:true,book_price:1_000_000-price,endpoints:vec![],guards:0,book_reads:0};
  let(current,facts)=ordinary_preflight(&c,&mut bank,&log,&mut source,&q,&mut r).unwrap().unwrap();assert_eq!(r.endpoints.len(),8);assert!(matches!(r.endpoints[0],Endpoint::OrderBook{..}));
  let expected=[Endpoint::Tournament,Endpoint::Positions,Endpoint::Orders{status:"open".into()},Endpoint::Account,Endpoint::Market{market_id:"201".into()},Endpoint::Nodes{market_id:"201".into()},Endpoint::OrderBook{market_id:"201".into(),depth:20}];
  assert_eq!(serde_json::to_value(&r.endpoints[1..]).unwrap(),serde_json::to_value(expected).unwrap());assert_eq!(r.book_reads,2);assert_eq!(facts.book.data["contexts"][0]["orderbook"]["exchanges"][0]["asOf"]["sequence"],3);assert_eq!(current["price"],q["price"]);assert_eq!(current["expiresAt"],q["expiresAt"]);assert!(log.attempted.is_empty());
 }
 #[test]fn unknown_queue_or_expired_native_quote_cannot_use_book_prefilter_for_a_retry(){
  for expired in [false,true]{let(_root,mut bank,log,c,mut source,q)=prefilter_fixture();let head=bank.durable_head();let mut r=PrefilterReads{at:if expired{q["expiresAt"].as_u64().unwrap()}else{now()},allow:false,book_price:300000,endpoints:vec![],guards:0,book_reads:0};
   assert!(ordinary_preflight(&c,&mut bank,&log,&mut source,&q,&mut r).unwrap().is_none());assert!(r.endpoints.is_empty());assert_eq!(r.guards,usize::from(!expired));assert_eq!(bank.durable_head(),head);assert!(log.attempted.is_empty());}
 }
 #[test]fn reviewed_read_only_book_screen_can_decline_without_account_reads_and_preserves_unknown_guard(){
  for allow in [true,false]{let(_root,mut bank,_log,c,mut source,q)=prefilter_fixture();let mut r=PrefilterReads{at:now(),allow,book_price:300000,endpoints:vec![],guards:0,book_reads:0};let mut evaluated=false;
   assert!(!reviewed_book_prefilter(&c,&mut bank,&mut source,&q,&mut r,|bank,at|{evaluated=true;let current=quote(bank,q["id"].as_u64().unwrap())?;let s=moloch_live_runtime::quote_priority::from_native(bank,&c.scope,&[current],at,None)?;assert_eq!(s.assessments[0].decision.reason,"sell_floor_above_current_ask");Ok(false)}).unwrap());assert_eq!(evaluated,allow);assert_eq!(r.endpoints.len(),usize::from(allow));}
 }
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
 fn entry(ex:&str)->fundamental::EntryHint{fundamental::EntryHint{exchange_id:ex.into(),market_id:"277".into(),contract_id:"278".into(),side:"YES".into(),review_sha256:"a".repeat(64)}}
 #[test]fn one_cursor_services_multiple_read_hints_and_ready_physical_quotes(){
  let entries=vec![entry("1042"),entry("966")];let existing=vec![fundamental::ExistingExitHint{exchange_id:"1068".into(),market_id:"379".into(),contract_id:"380".into(),side:"YES".into(),reason:"reviewed_inventory_book_refresh".into()}];let physical=vec![("1030:NO:buy".into(),7,false),("1070:YES:buy".into(),8,false)];let mut cursor=None;let mut served=Vec::new();
  for _ in 0..10{let(choice,key)=shared_preflight_rotation(&existing,&entries,&physical,cursor.as_deref()).unwrap();served.push(choice);cursor=Some(key);}
  assert_eq!(served[..5],served[5..]);for expected in [SharedPreflight::ExistingRead(0),SharedPreflight::EntryRead(0),SharedPreflight::EntryRead(1),SharedPreflight::Physical(7),SharedPreflight::Physical(8)]{assert_eq!(served.iter().filter(|s|**s==expected).count(),2,"{expected:?}");}
 }
 #[test]fn perpetually_unqualified_nc_read_cannot_starve_ready_ordinary_quote(){
  let entries=vec![entry("966")];let physical=vec![("1040:NO:buy".into(),2,false)];let mut cursor=None;let mut ordinary=0;let mut reads=0;
  // A failed qualification does not remove the hint or reset the cursor.
  for _ in 0..8{let(choice,key)=shared_preflight_rotation(&[],&entries,&physical,cursor.as_deref()).unwrap();cursor=Some(key);match choice{SharedPreflight::Physical(2)=>ordinary+=1,SharedPreflight::EntryRead(0)=>reads+=1,_=>panic!("unexpected selection")}}
  assert_eq!((ordinary,reads),(4,4));
 }
 #[test]fn no_ready_physical_quote_still_performs_fundamental_refresh(){let entries=vec![entry("966")];assert_eq!(shared_preflight_rotation(&[],&entries,&[],Some("966:YES:buy")),Some((SharedPreflight::EntryRead(0),"966:YES:buy".into())));assert!(shared_preflight_rotation(&[],&[],&[],None).is_none());}
 #[test]fn nonmarketable_profit_sells_without_covered_proof_share_cursor_with_entry(){
  let entries=vec![entry("966")];let q=json!({"action":"sell","executionStyle":"inventoryAON"});let original=q.clone();
  let physical=vec![("1010:NO:sell".into(),4,physical_exit_urgent(&q,false,false))];let mut cursor=None;let mut selected=Vec::new();
  for _ in 0..6{let(choice,key)=shared_preflight_rotation(&[],&entries,&physical,cursor.as_deref()).unwrap();selected.push(choice);cursor=Some(key);}
  assert_eq!(selected,vec![SharedPreflight::Physical(4),SharedPreflight::EntryRead(0),SharedPreflight::Physical(4),SharedPreflight::EntryRead(0),SharedPreflight::Physical(4),SharedPreflight::EntryRead(0)]);assert_eq!(q,original);
 }
 #[test]fn current_fifo_deadline_or_actual_reviewed_exit_still_preempts_profitable_rotation(){
  let entries=vec![entry("966")];let profit=json!({"action":"sell","executionStyle":"inventoryAON"});let mut age=profit.clone();age["inventoryExitReason"]=json!("maximumAge");
  assert!(!physical_exit_urgent(&profit,true,false),"available depth/style alone grants no urgency");
  assert!(!physical_exit_urgent(&age,false,false),"historical age label without current due remaining lot cannot preempt");
  for(q,due,reviewed)in [(&age,true,false),(&profit,false,true)]{
   let physical=vec![("1010:NO:sell".into(),4,physical_exit_urgent(q,due,reviewed))];
   assert_eq!(shared_preflight_rotation(&[],&entries,&physical,Some("1010:NO:sell")).unwrap().0,SharedPreflight::Physical(4));
  }
  let buy=json!({"action":"buy","inventoryExitReason":"maximumAge"});assert!(!physical_exit_urgent(&buy,true,true));
 }
 #[test]fn real_same_key_quote_supersedes_read_hint_and_urgent_exit_preempts_union(){let entries=vec![entry("966")];let physical=vec![("966:YES:buy".into(),9,false)];assert_eq!(shared_preflight_rotation(&[],&entries,&physical,None).unwrap().0,SharedPreflight::Physical(9));let physical=vec![("1000:NO:buy".into(),3,false),("1076:YES:sell".into(),4,true)];assert_eq!(shared_preflight_rotation(&[],&entries,&physical,None).unwrap().0,SharedPreflight::Physical(4));}
 #[test]fn actual_native_activation_cancel_and_expiry_guard_determine_union_membership(){let(_root,bank,log,_,_,_)=fixture();let mut q=bank.engine().order_records()[0].clone();let at=now();q["action"]=json!("buy");q["retired"]=json!(false);q["activationAt"]=json!(at);q["expiresAt"]=json!(at+10000);q["cancelEffectiveAt"]=Value::Null;assert!(physical_preflight_ready(&q,true,false,at));q["activationAt"]=json!(at+1);assert!(!physical_preflight_ready(&q,true,false,at));q["activationAt"]=json!(at);q["expiresAt"]=json!(at+5000);assert!(!physical_preflight_ready(&q,true,false,at));q["expiresAt"]=json!(at+10000);q["cancelEffectiveAt"]=json!(at+100);assert!(!physical_preflight_ready(&q,true,false,at));q["cancelEffectiveAt"]=Value::Null;assert!(!physical_preflight_ready(&q,false,false,at));assert!(log.attempted.is_empty());}
}

#[cfg(test)]mod package_hook_tests{
 use super::*;
 #[test]fn paper5_scan_is_bounded_without_renewing_a_clock_or_allowing_backwards_time(){let mut last=0;assert!(paper5_due(&mut last,1000).unwrap());assert!(!paper5_due(&mut last,60999).unwrap());assert_eq!(last,1000);assert!(paper5_due(&mut last,61000).unwrap());assert!(paper5_due(&mut last,60999).is_err());assert_eq!(last,61000);}
 #[test]fn optional_paper5_ineligibility_does_not_hide_io_integrity_or_unknown_errors(){for code in ["P5_EXECUTION_RESEARCH_MODEL","P5_EXECUTION_NOT_CONFIGURED","SEMANTIC_CURRENT_STALE","SURFACE_CURRENT_NATIVE_BOOK_REQUIRED","PROB_RESERVATION_WINDOW","PACKAGE_PREFLIGHT_BOOK_CLOCK"]{let e:Box<dyn std::error::Error>=code.into();assert!(paper5_expected_defer(e.as_ref()),"{code}");}for code in ["DISPATCH_CHANGED","SOURCE_REF_CONFLICT","LIVE_EXECUTION_STOP","PACKAGE_QUEUE_UNKNOWN","arbitrary source failure"]{let e:Box<dyn std::error::Error>=code.into();assert!(!paper5_expected_defer(e.as_ref()),"{code}");}assert!(!paper5_expected_defer(&std::io::Error::from(std::io::ErrorKind::NotFound)));}
 struct Temp(PathBuf);impl Temp{fn new()->Self{static NEXT:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);let serial=NEXT.fetch_add(1,std::sync::atomic::Ordering::Relaxed);Self(std::env::temp_dir().join(format!("moloch-package-hook-{}-{}-{serial}",std::process::id(),SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos())))}}impl Drop for Temp{fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}
 fn setup(path:&Path,n:usize)->(Session,DispatchLog,Value,Value){
  setup_tournament(path,n,"cup")
 }
 fn setup_tournament(path:&Path,n:usize,tournament:&str)->(Session,DispatchLog,Value,Value){
  private_dir(path).unwrap();let at=now().saturating_sub(7500);let policy=json!({"initialCash":"1000.000000","maximumCommitted":"100.000000","maximumMarketCost":"50.000000","quantity":12,"maximumMarkets":3,"entryStopAt":iso(at+60_000)});let seed=json!({"runId":"package-hook-fixture","availableCash":"1000.000000","accountSnapshotHash":"a".repeat(64),"observedAt":iso(at)});
  let mut bank=Session::open_live(&path.join("bank"),&policy,&seed).unwrap();
  for i in 0..n{let ex=(101+i).to_string();let stamp=iso(at+i as u64*2500);bank.accept("official-book",i as u64+1,&json!({"type":"book","receivedAt":stamp,"book":{"tournamentId":tournament,"marketId":ex,"exchangeId":ex,"sequence":"1","sourceAt":stamp,"requestStartedAt":stamp,"receivedAt":stamp,"bids":[{"price":"0.400000","quantity":100}],"asks":[{"price":"0.450000","quantity":100}]}})).unwrap();}
  (bank,DispatchLog::open(&path.join("dispatch.ndjson")).unwrap(),policy,seed)
 }
 const INTER_READ_TOURNAMENT:&str="bda92870-621e-47b0-bc3c-3602c5c26f55";
 struct InterReadFixture{
  bank:Session,log:DispatchLog,responses:std::collections::VecDeque<Receipt>,
  hooks:usize,fail_hook:Option<usize>,reads:Vec<String>,trace:std::rc::Rc<std::cell::RefCell<Vec<&'static str>>>,
 }
 impl reconciliation::Backend for InterReadFixture{
  fn now_ms(&self)->u64{now()}
  fn read(&mut self,ep:Endpoint)->Result<Receipt>{
   let Self{bank,responses,hooks,fail_hook,reads,trace,..}=self;let fail_hook=*fail_hook;let trace=trace.clone();
   reconciliation_read_with(bank,ep,|bank|{
    *hooks+=1;trace.borrow_mut().push("service");if fail_hook==Some(*hooks){return Err("INJECTED_STREAM_FAILURE".into())}
    let at=iso(now());accept_source_events(bank,&[json!({"type":"book","receivedAt":at,"book":{"tournamentId":INTER_READ_TOURNAMENT,"marketId":"101","exchangeId":"101","sequence":(*hooks+1).to_string(),"sourceAt":at,"requestStartedAt":at,"receivedAt":at,"transport":"official_sig_realtime","bids":[{"price":"0.400000","quantity":100}],"asks":[{"price":"0.450000","quantity":100}]}})])
   },|ep|{
    trace.borrow_mut().push("read");let expected=match ep{Endpoint::Orders{..}=>"aggregate".into(),Endpoint::OwnedOrder{order_id}=>format!("/orders/{order_id}"),Endpoint::OwnedFills{order_id}=>format!("/orders/{order_id}/fills?limit=200"),_=>return Err("INJECTED_UNEXPECTED_READ".into())};reads.push(expected.clone());
    let original=responses.pop_front().ok_or("INJECTED_MISSING_RECEIPT")?;if expected!="aggregate"{assert_eq!(original.endpoint,expected)}Ok(original)
   })
  }
  fn quote(&self,id:u64)->Result<Value>{quote(&self.bank,id)}
  fn apply(&mut self,e:&Value)->Result<()>{self.trace.borrow_mut().push("apply");accept(&mut self.bank,e)}
  fn close(&mut self,id:u64,e:&Value,order:&Receipt,fills:&Receipt)->Result<()>{
   self.trace.borrow_mut().push("close");assert!(!self.bank.engine().fills().is_empty());assert!(!self.log.closed.contains(&id));
   accept(&mut self.bank,e)?;self.log.append(json!({"kind":"closed","qid":id,"orderReceiptHash":feedback::digest(order)?,"fillReceiptHash":feedback::digest(fills)?}))
  }
 }
 fn inter_read_fixture(temp:&Temp,two:bool,fail_hook:Option<usize>)->(InterReadFixture,Vec<moloch_live_runtime::reconciliation_plan::OwnedOrder>,Value,Value,String){
  let(mut bank,mut log,policy,seed)=setup_tournament(&temp.0,1,INTER_READ_TOURNAMENT);let selected:Vec<_>=bank.engine().order_records().into_iter().take(if two{2}else{1}).collect();assert_eq!(selected.len(),if two{2}else{1});
  for q in &selected{let id=q["id"].as_u64().unwrap();let actual=format!("9{id}");let at=q["activationAt"].as_u64().unwrap();let side=if q["side"]=="YES"{"yes"}else{"no"};
   let ack=Receipt{endpoint:"/orders".into(),status:200,request_started_at:iso(at),received_at:iso(at+1),data:json!({"orderId":actual,"exchangeId":q["exchangeId"],"side":side,"action":q["action"],"quantity":q["quantity"],"price":serde_json::from_str::<Value>(q["price"].as_str().unwrap()).unwrap()})};
   for event in [json!({"kind":"intent","qid":id,"quote":q}),json!({"kind":"attempt","qid":id,"at":iso(at)}),json!({"kind":"ack","qid":id,"receipt":ack})]{log.append(event).unwrap()}
   accept(&mut bank,&feedback::acknowledgment(q,&ack,&iso(now())).unwrap()).unwrap();
  }
  let owned=selected.iter().map(|q|reconciliation::owned_from_quote(&quote(&bank,q["id"].as_u64().unwrap()).unwrap(),&format!("9{}",q["id"].as_u64().unwrap()),false,false).unwrap()).collect();
  let first=&selected[0];let id=first["id"].as_u64().unwrap();let actual=format!("9{id}");let at=first["activationAt"].as_u64().unwrap();let side=if first["side"]=="YES"{"yes"}else{"no"};let price=serde_json::from_str::<Value>(first["price"].as_str().unwrap()).unwrap();
  let order=Receipt{endpoint:format!("/orders/{actual}"),status:200,request_started_at:iso(at+3),received_at:iso(at+4),data:json!({"id":actual,"exchangeId":"101","tournamentId":INTER_READ_TOURNAMENT,"side":side,"action":"buy","priceLimit":price,"quantity":first["quantity"],"quantityFilled":2,"open":false,"terminalReasonCode":"Canceled"})};
  let fills=Receipt{endpoint:format!("/orders/{actual}/fills?limit=200"),status:200,request_started_at:iso(at+5),received_at:iso(at+6),data:json!({"orderId":actual,"exchangeId":"101","tournamentId":INTER_READ_TOURNAMENT,"totalQuantityFilled":if side=="no"{-2}else{2},"coverage":{"complete":true,"projectedThroughSequence":100},"pagination":{"limit":200,"hasMore":false,"nextCursor":null},"data":[{"id":(9000+id).to_string(),"quantity":if side=="no"{-2}else{2},"side":side,"price":price,"filledAt":iso(at+2)}]})};let digest=feedback::digest(&fills).unwrap();
  let mut responses=std::collections::VecDeque::new();if two{responses.push_back(Receipt{endpoint:format!("/orders?status=open&limit=200&tournamentId={INTER_READ_TOURNAMENT}"),status:200,request_started_at:iso(at+3),received_at:iso(at+4),data:json!({"data":[],"coverage":{"complete":true,"projectedThroughSequence":100},"pagination":{"limit":200,"hasMore":false,"nextCursor":null}})});}responses.push_back(order);responses.push_back(fills);
  (InterReadFixture{bank,log,responses,hooks:0,fail_hook,reads:Vec::new(),trace:Default::default()},owned,policy,seed,digest)
 }
 #[test]fn reconcile_services_source_between_exact_reads_then_applies_original_partial_tape_and_terminal_without_hook(){
  let temp=Temp::new();let(mut live,owned,policy,seed,digest)=inter_read_fixture(&temp,false,None);let scope=Scope{tournament_id:INTER_READ_TOURNAMENT.into(),tournament_slug:"cup".into(),exchanges:[("101".into(),"101".into())].into()};let mut cache=Reconciler::default();
  let stats=cache.cycle(&scope,&owned,false,&mut live).unwrap();assert_eq!(stats.exact_order_reads,1);assert_eq!(stats.complete_fill_reads,1);assert_eq!(live.hooks,2);assert_eq!(*live.trace.borrow(),vec!["service","read","service","read","apply","close"]);
  assert_eq!(live.bank.engine().fills().len(),1);assert_eq!(live.bank.engine().fills()[0]["quantity"],2);assert_eq!(live.bank.engine().fills()[0]["receiptHash"],digest);assert_eq!(live.bank.engine().fills()[0]["executedAt"],iso(quote(&live.bank,owned[0].logical_id).unwrap()["activationAt"].as_u64().unwrap()+2));assert!(live.log.closed.contains(&owned[0].logical_id));
  let head=live.bank.durable_head();let report=live.bank.engine().report().unwrap();let journal=fs::read(temp.0.join("bank/events.jsonl")).unwrap();let hooks=live.hooks;assert_eq!(cache.cycle(&scope,&[],false,&mut live).unwrap().pending_orders,0);assert_eq!(live.hooks,hooks);drop(live);
  let bank=Session::open_live(&temp.0.join("bank"),&policy,&seed).unwrap();let log=DispatchLog::open(&temp.0.join("dispatch.ndjson")).unwrap();assert_eq!(bank.durable_head(),head);assert_eq!(bank.engine().report().unwrap(),report);assert_eq!(fs::read(temp.0.join("bank/events.jsonl")).unwrap(),journal);assert_eq!(bank.engine().fills().len(),1);assert!(log.closed.contains(&owned[0].logical_id));
 }
 #[test]fn reconcile_stream_failure_before_get_keeps_actual_binding_and_does_not_fetch_or_clear_tapes(){
  for failed in [1,2]{let temp=Temp::new();let(mut live,owned,_,_,_)=inter_read_fixture(&temp,false,Some(failed));let scope=Scope{tournament_id:INTER_READ_TOURNAMENT.into(),tournament_slug:"cup".into(),exchanges:[("101".into(),"101".into())].into()};let error=Reconciler::default().cycle(&scope,&owned,false,&mut live).unwrap_err();assert_eq!(error.to_string(),"INJECTED_STREAM_FAILURE");assert_eq!(live.reads.len(),failed-1);assert!(live.bank.engine().fills().is_empty());assert!(live.log.closed.is_empty());assert!(live.bank.engine().live_order_bindings().contains_key(&owned[0].logical_id));assert!(!live.log.never_attempted(owned[0].logical_id));assert_eq!(quote(&live.bank,owned[0].logical_id).unwrap()["retired"],false);}
 }
 #[test]fn later_reconcile_hook_failure_cannot_erase_already_durable_partial_fill_or_terminal(){
  let temp=Temp::new();let(mut live,owned,_,_,digest)=inter_read_fixture(&temp,true,Some(4));let scope=Scope{tournament_id:INTER_READ_TOURNAMENT.into(),tournament_slug:"cup".into(),exchanges:[("101".into(),"101".into())].into()};let error=Reconciler::default().cycle(&scope,&owned,false,&mut live).unwrap_err();assert_eq!(error.to_string(),"INJECTED_STREAM_FAILURE");assert_eq!(live.reads.len(),3);assert_eq!(live.bank.engine().fills().len(),1);assert_eq!(live.bank.engine().fills()[0]["receiptHash"],digest);assert!(live.log.closed.contains(&owned[0].logical_id));assert!(!live.log.closed.contains(&owned[1].logical_id));assert!(!live.log.never_attempted(owned[1].logical_id));assert_eq!(*live.trace.borrow(),vec!["service","read","service","read","service","read","apply","close","service"]);
 }
 fn stale_fixture(path:&Path,n:usize)->(Control,Session,DispatchLog,Value,Value){
  private_dir(path).unwrap();let at=now()-65_000;let policy=json!({"initialCash":"1000.000000","maximumCommitted":"100.000000","maximumMarketCost":"50.000000","quantity":12,"maximumMarkets":3,"entryStopAt":iso(at+3_600_000),"lifetimeMs":600000});let seed=json!({"runId":"stale-worker-fixture","availableCash":"1000.000000","accountSnapshotHash":"a".repeat(64),"observedAt":iso(at)});let mut bank=Session::open_live(&path.join("bank"),&policy,&seed).unwrap();
  let exchanges=(0..n).map(|i|((101+i).to_string(),(101+i).to_string())).collect::<std::collections::BTreeMap<_,_>>();for(i,ex)in exchanges.keys().enumerate(){let stamp=iso(at+i as u64*2500);bank.accept("stale-original-book",i as u64+1,&json!({"type":"book","receivedAt":stamp,"book":{"tournamentId":"cup","marketId":ex,"exchangeId":ex,"sequence":"1","sourceAt":stamp,"requestStartedAt":stamp,"receivedAt":stamp,"bids":[{"price":"0.400000","quantity":100}],"asks":[{"price":"0.450000","quantity":100}]}})).unwrap();}
  let c:Control=serde_json::from_value(json!({"activation":{"runId":"stale-worker-fixture","entryUntilMs":at+3_600_000,"maximumCommitted":"100.000000","maximumMarketCost":"50.000000","maximumQuantity":12,"tournamentId":"cup","tournamentSlug":"cup","binarySha256":"a".repeat(64)},"scope":{"tournamentId":"cup","tournamentSlug":"cup","exchanges":exchanges},"policy":policy,"root":path,"credentials":path.join("unused-fixture-path"),"ownerLock":path.join("unused-fixture-owner"),"accountScope":"no-account-client"})).unwrap();(c,bank,DispatchLog::open(&path.join("dispatch.ndjson")).unwrap(),policy,seed)
 }
 #[test]fn stale_two_draft_batch_uses_each_current_original_proof_and_restarts_without_new_cash_or_orders(){
  let temp=Temp::new();let(c,mut bank,mut log,policy,seed)=stale_fixture(&temp.0,1);let original=bank.engine().order_records();assert_eq!(original.len(),2);let before=bank.engine().report().unwrap();assert!(bank.engine().wallet.reserved().unwrap()>0);
  assert_eq!(retire_stale_entry_drafts(&c,&mut bank,&mut log,&Default::default()).unwrap(),2);let report=bank.engine().report().unwrap();for key in ["cash","ownedCost","realized","fees","positions"]{assert_eq!(report[key],before[key],"{key}");}assert_eq!(bank.engine().wallet.reserved().unwrap(),0);for old in &original{let mut expected=old.clone();expected["retired"]=json!(true);assert_eq!(quote(&bank,old["id"].as_u64().unwrap()).unwrap(),expected);}assert!(log.intents.is_empty()&&log.attempted.is_empty()&&log.acknowledgments.is_empty()&&log.closed.is_empty());assert!(bank.engine().fills().is_empty());
  let rows=fs::read(temp.0.join("dispatch.ndjson")).unwrap();let events=std::str::from_utf8(&rows).unwrap().lines().map(|s|serde_json::from_str::<Value>(s).unwrap()).collect::<Vec<_>>();assert_eq!(events.len(),2);for row in events{assert_eq!(row["event"]["kind"],"not_sent");assert_eq!(row["event"]["proof"]["networkAttempts"],0);assert_eq!(row["event"]["proof"]["qid"],row["event"]["qid"]);assert_eq!(row["event"]["proof"]["sequence"].as_u64().unwrap()+1,row["sequence"].as_u64().unwrap());assert_eq!(row["event"]["proof"]["chain"],row["previous"]);}
  let journal=fs::read(temp.0.join("bank/events.jsonl")).unwrap();assert_eq!(retire_stale_entry_drafts(&c,&mut bank,&mut log,&Default::default()).unwrap(),0);assert_eq!(fs::read(temp.0.join("dispatch.ndjson")).unwrap(),rows);assert_eq!(fs::read(temp.0.join("bank/events.jsonl")).unwrap(),journal);drop(log);drop(bank);
  let mut bank=Session::open_live(&temp.0.join("bank"),&policy,&seed).unwrap();let mut log=DispatchLog::open(&temp.0.join("dispatch.ndjson")).unwrap();assert_eq!(bank.engine().report().unwrap(),report);assert_eq!(retire_stale_entry_drafts(&c,&mut bank,&mut log,&Default::default()).unwrap(),0);assert_eq!(fs::read(temp.0.join("bank/events.jsonl")).unwrap(),journal);assert_eq!(fs::read(temp.0.join("dispatch.ndjson")).unwrap(),rows);
 }
 #[test]fn stale_cleanup_preserves_actual_pending_mate_while_another_flat_exchange_yields(){
  let temp=Temp::new();let(c,mut bank,mut log,_,_)=stale_fixture(&temp.0,2);let q=bank.engine().order_records().into_iter().find(|q|q["exchangeId"]=="101"&&q["side"]=="YES").unwrap();let id=q["id"].as_u64().unwrap();let mut event=ack(&q);event["receivedAt"]=json!(iso(q["activationAt"].as_u64().unwrap()));for row in [json!({"kind":"intent","qid":id,"quote":q}),json!({"kind":"attempt","qid":id}),json!({"kind":"ack","qid":id,"receipt":{"data":{"orderId":event["actualOrderId"]}}})]{log.append(row).unwrap();}bank.accept("stale-retained-actual-ack",1,&event).unwrap();let protected=bank.engine().order_records().into_iter().filter(|q|q["exchangeId"]=="101").collect::<Vec<_>>();let positions=bank.engine().wallet.positions();let cash=bank.engine().wallet.cash;let actual=bank.engine().live_order_bindings().clone();
  assert_eq!(retire_stale_entry_drafts(&c,&mut bank,&mut log,&Default::default()).unwrap(),2);assert_eq!(bank.engine().order_records().into_iter().filter(|q|q["exchangeId"]=="101").collect::<Vec<_>>(),protected);assert!(bank.engine().order_records().iter().filter(|q|q["exchangeId"]=="102").all(|q|q["retired"]==true));assert_eq!(bank.engine().live_order_bindings(),&actual);assert_eq!(bank.engine().wallet.positions(),positions);assert_eq!(bank.engine().wallet.cash,cash);assert!(!log.closed.contains(&id));assert!(bank.engine().fills().is_empty());
 }
 #[test]fn fresh_unqualified_batch_releases_only_native_reservation_and_replays_without_new_funds(){
  let temp=Temp::new();let(c,mut bank,mut log,policy,seed)=stale_fixture(&temp.0,1);let at=now();let stamp=iso(at);accept(&mut bank,&json!({"type":"book","receivedAt":stamp,"book":{"tournamentId":"cup","marketId":"101","exchangeId":"101","sequence":"2","sourceAt":stamp,"requestStartedAt":stamp,"receivedAt":stamp,"bids":[{"price":"0.400000","quantity":100}],"asks":[{"price":"0.450000","quantity":100}]}})).unwrap();let before=bank.engine().report().unwrap();assert_eq!(retire_stale_entry_drafts(&c,&mut bank,&mut log,&Default::default()).unwrap(),0);
  assert_eq!(retire_unqualified_entry_drafts(&c,&mut bank,&mut log,&Default::default()).unwrap(),2);let after=bank.engine().report().unwrap();for key in ["cash","ownedCost","realized","fees","positions"]{assert_eq!(after[key],before[key],"{key}");}assert_eq!(after["reservedCash"],"0.000000");assert!(log.intents.is_empty()&&log.attempted.is_empty()&&log.acknowledgments.is_empty());assert!(bank.engine().fills().is_empty());drop(log);drop(bank);
  let mut bank=Session::open_live(&temp.0.join("bank"),&policy,&seed).unwrap();let mut log=DispatchLog::open(&temp.0.join("dispatch.ndjson")).unwrap();assert_eq!(bank.engine().report().unwrap(),after);assert_eq!(retire_unqualified_entry_drafts(&c,&mut bank,&mut log,&Default::default()).unwrap(),0);accept(&mut bank,&json!({"type":"tick","receivedAt":iso(now())})).unwrap();assert!(bank.engine().wallet.reserved().unwrap()>0);assert_eq!(retire_unqualified_entry_drafts(&c,&mut bank,&mut log,&Default::default()).unwrap(),2);assert_eq!(bank.engine().wallet.reserved().unwrap(),0);assert_eq!(bank.engine().wallet.cash,1_000_000_000);assert!(bank.engine().fills().is_empty());
 }
 #[test]fn unqualified_cleanup_does_not_retire_intended_mate_or_acknowledged_actual_order(){
  for acknowledged in [false,true]{let temp=Temp::new();let(c,mut bank,mut log,_,_)=stale_fixture(&temp.0,2);let q=bank.engine().order_records().into_iter().find(|q|q["exchangeId"]=="101"&&q["side"]=="YES").unwrap();let id=q["id"].as_u64().unwrap();log.append(json!({"kind":"intent","qid":id,"quote":q})).unwrap();if acknowledged{let mut e=ack(&q);e["receivedAt"]=json!(iso(q["activationAt"].as_u64().unwrap()));log.append(json!({"kind":"attempt","qid":id})).unwrap();log.append(json!({"kind":"ack","qid":id,"receipt":{"data":{"orderId":e["actualOrderId"]}}})).unwrap();bank.accept("quarantine-actual-ack",1,&e).unwrap();}let held=bank.engine().order_records().into_iter().filter(|q|q["exchangeId"]=="101").collect::<Vec<_>>();let cash=bank.engine().wallet.cash;
   assert_eq!(retire_unqualified_entry_drafts(&c,&mut bank,&mut log,&Default::default()).unwrap(),2);assert_eq!(bank.engine().order_records().into_iter().filter(|q|q["exchangeId"]=="101").collect::<Vec<_>>(),held);assert_eq!(bank.engine().wallet.cash,cash);assert!(!log.closed.contains(&id));assert!(bank.engine().fills().is_empty());
  }
 }
 #[test]fn paper5_draft_selection_is_readonly_then_yields_only_positive_original_unsent_components(){let temp=Temp::new();let(mut bank,mut log,_,_)=setup(&temp.0,2);let exchanges=std::collections::BTreeSet::from(["101".into()]);let head=bank.durable_head();let report=bank.engine().report().unwrap();let drafts=paper5_component_drafts(&bank,&log,&exchanges).unwrap().unwrap();assert!(!drafts.is_empty());assert!(drafts.iter().all(|q|q["exchangeId"]=="101"&&log.never_attempted(q["id"].as_u64().unwrap())));assert_eq!(bank.durable_head(),head);assert_eq!(bank.engine().report().unwrap(),report);for q in drafts{retire_for_package(&mut bank,&mut log,&q).unwrap();}assert!(paper5_component_drafts(&bank,&log,&exchanges).unwrap().unwrap().is_empty());assert!(bank.engine().order_records().iter().any(|q|q["exchangeId"]=="102"&&q["retired"]==false));assert_eq!(bank.engine().wallet.positions(),json!({}));assert!(bank.engine().fills().is_empty());}
 #[test]fn paper5_keeps_original_physical_and_unknown_liabilities_without_cancel_or_replacement(){for case in ["ack","native_binding","unknown_other"]{let temp=Temp::new();let(mut bank,mut log,_,_)=setup(&temp.0,1);let q=bank.engine().order_records()[0].clone();let id=q["id"].as_u64().unwrap();match case{"ack"=>{log.acknowledgments.insert(id,json!({"data":{"orderId":"901"}}));},"native_binding"=>{restore_ack(&mut bank,&ack(&q)).unwrap();},_=>{log.append(json!({"kind":"intent","qid":999})).unwrap();log.append(json!({"kind":"attempt","qid":999})).unwrap();}}let head=bank.durable_head();let rows=fs::read(temp.0.join("dispatch.ndjson")).unwrap();let selected=paper5_component_drafts(&bank,&log,&["101".into()].into());if case=="unknown_other"{assert!(selected.is_err());}else{assert!(selected.unwrap().is_none());}assert_eq!(bank.durable_head(),head);assert_eq!(fs::read(temp.0.join("dispatch.ndjson")).unwrap(),rows);assert!(bank.engine().fills().is_empty());}}
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
 #[cfg(target_os="linux")]
 fn proved_abort_fixture(temp:&Temp,bank:&mut Session,log:&mut DispatchLog)->(u64,Config,Value){
  let q=bank.engine().order_records()[0].clone();let id=q["id"].as_u64().unwrap();let at=now();let n=q["quantity"].as_u64().unwrap() as u32;let price=q["price"].as_str().unwrap();let tid="cup";
  let order=Order{idempotency_key:format!("package-hook-fixture:{id}"),exchange_id:q["exchangeId"].as_str().unwrap().into(),side:if q["side"]=="YES"{"yes"}else{"no"}.into(),action:q["action"].as_str().unwrap().into(),quantity:n,price:price.into(),expiration_date:iso(q["expiresAt"].as_u64().unwrap()),tournament_id:tid.into()};let request=spec(&order).unwrap();
  let permit=Permit{run_id:"package-hook-fixture".into(),intent_id:id.to_string(),owner:"Codex".into(),expires_at_ms:at-1,maximum_quantity:n,maximum_cost:money(decimal_units(price).unwrap()*n as i128)};let error=api::TransportError{code:"PERMIT_EXPIRED_OR_INVALID".into(),status:None,may_have_executed:false,retry_after_ms:0,order_id:None,api_code:None};
  log.append(json!({"kind":"intent","qid":id,"quote":q,"spec":request,"permit":permit})).unwrap();record_original_attempt(bank,log,id,None,at).unwrap();log.append(json!({"kind":"error","qid":id,"at":iso(at),"error":error})).unwrap();let dir=temp.0.join("original-sdk-queue");fs::create_dir(&dir).unwrap();{let mut queue=api::admission::AccountAdmission::open(&dir,"synthetic-refresh-account","fixture").unwrap();queue.command(json!({"op":"status","scope":"synthetic-refresh-account","now":at-1000})).unwrap();}
  let config=Config{queue_dir:dir,account_scope:"synthetic-refresh-account".into(),scope:Scope{tournament_id:tid.into(),tournament_slug:"cup".into(),exchanges:[(q["exchangeId"].as_str().unwrap().into(),q["marketId"].as_str().unwrap().into())].into()},maximum_pages:2,writes_enabled:false};let proof=api::verify_frozen_ordinary_pre_admission(&config,&request,&permit,at,&error).unwrap();let evidence=proof.evidence().clone();log.append_aborted_before_admission(id,&proof,&iso(now())).unwrap();retire_proved_unadmitted(bank,log,id).unwrap();(id,config,evidence)
 }
 #[cfg(target_os="linux")]
 #[test]fn refresh_liability_uses_original_sdk_proof_and_requires_restart_reverification(){
  let temp=Temp::new();let(mut bank,mut log,_,_)=setup(&temp.0,1);let(id,config,evidence)=proved_abort_fixture(&temp,&mut bank,&mut log);let report=bank.engine().report().unwrap();let head=bank.durable_head();let bytes=fs::read(temp.0.join("dispatch.ndjson")).unwrap();assert_eq!(quote(&bank,id).unwrap()["retired"],true);
  assert!(!refresh_physical_pending(&log,id,bank.engine().live_order_bindings().contains_key(&id),bank.engine().fills().iter().any(|f|f["orderId"]==id)).unwrap());assert!(!log.never_attempted(id)&&!log.closed.contains(&id)&&!log.acknowledgments.contains_key(&id));
  drop(log);let mut log=DispatchLog::open(&temp.0.join("dispatch.ndjson")).unwrap();assert!(refresh_physical_pending(&log,id,false,false).unwrap(),"serialized abort is not an opaque capability");assert!(log.require_resolved_attempts().is_err());let(request,permit,at,error)=log.pre_admission_originals(id).unwrap();let proof=api::verify_retained_ordinary_pre_admission(&config,&request,&permit,at,&error,&evidence).unwrap();log.verify_aborted_before_admission(id,&proof).unwrap();assert!(!refresh_physical_pending(&log,id,false,false).unwrap());log.require_resolved_attempts().unwrap();
  log.append(json!({"kind":"intent","qid":999})).unwrap();log.append(json!({"kind":"attempt","qid":999})).unwrap();assert!(refresh_physical_pending(&log,999,false,false).unwrap());assert!(log.require_resolved_attempts().is_err());assert_eq!(bank.durable_head(),head);assert_eq!(bank.engine().report().unwrap(),report);assert!(fs::read(temp.0.join("dispatch.ndjson")).unwrap().starts_with(&bytes));assert!(bank.engine().fills().is_empty());assert!(!log.never_attempted(id));
 }
 #[cfg(target_os="linux")]
 #[test]fn contradicted_verified_abort_blocks_native_binding_actual_fill_or_ack_even_if_marked_closed(){
  let temp=Temp::new();let(mut bank,mut log,_,_)=setup(&temp.0,1);let(id,_,_)=proved_abort_fixture(&temp,&mut bank,&mut log);for closed in [false,true]{if closed{log.closed.insert(id);}for(binding,fill)in [(true,false),(false,true),(true,true)]{assert_eq!(refresh_physical_pending(&log,id,binding,fill).unwrap_err().to_string(),"LIVE_ABORT_PROOF_CONTRADICTS_ACTUAL_EXECUTION");}log.acknowledgments.insert(id,json!({"orderId":"contradictory-test-only"}));assert!(refresh_physical_pending(&log,id,false,false).is_err());log.acknowledgments.remove(&id);}assert!(!log.never_attempted(id));
 }
 #[test]fn ordinary_actual_evidence_requires_terminal_closure_without_creating_a_retry(){
  let temp=Temp::new();let(_bank,mut log,_,_)=setup(&temp.0,1);assert!(!refresh_physical_pending(&log,1,false,false).unwrap());log.append(json!({"kind":"intent","qid":1})).unwrap();log.append(json!({"kind":"attempt","qid":1})).unwrap();assert!(refresh_physical_pending(&log,1,false,false).unwrap());assert!(refresh_physical_pending(&log,1,true,false).unwrap());assert!(refresh_physical_pending(&log,1,false,true).unwrap());log.append(json!({"kind":"ack","qid":1,"receipt":{"orderId":"901"}})).unwrap();assert!(refresh_physical_pending(&log,1,false,false).unwrap());log.append(json!({"kind":"closed","qid":1})).unwrap();assert!(!refresh_physical_pending(&log,1,true,true).unwrap());assert!(!log.never_attempted(1));
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
 #[test]fn account_realtime_control_is_optional_default_off_and_only_explicitly_enabled(){let mut c:Control=serde_json::from_value(json!({"activation":{"runId":"fixture","entryUntilMs":100000,"maximumCommitted":"100.000000","maximumMarketCost":"50.000000","maximumQuantity":12,"tournamentId":"cup","tournamentSlug":"cup","binarySha256":"a".repeat(64)},"scope":{"tournamentId":"cup","tournamentSlug":"cup","exchanges":{"101":"101"}},"policy":{},"root":"/fixture","credentials":"/unused","ownerLock":"/fixture/owner.lock","accountScope":"fixture"})).unwrap();assert!(!c.account_realtime);assert!(serde_json::to_value(&c).unwrap().get("accountRealtime").is_none());c.account_realtime=true;assert_eq!(serde_json::to_value(&c).unwrap()["accountRealtime"],true);}
 #[test]fn account_preintent_defer_is_typed_and_cannot_hide_an_original_attempt(){
  let temp=Temp::new();let(bank,mut log,_,_)=setup(&temp.0,1);let q=bank.engine().order_records()[0].clone();let id=q["id"].as_u64().unwrap();let binary=temp.0.join("checked-binary");fs::write(&binary,b"offline fixture").unwrap();let control_path=temp.0.join("checked-control");fs::write(&control_path,b"unchanged").unwrap();
  let c:Control=serde_json::from_value(json!({"activation":{"runId":"package-hook-fixture","entryUntilMs":now()+60000,"maximumCommitted":"100.000000","maximumMarketCost":"50.000000","maximumQuantity":12,"tournamentId":"cup","tournamentSlug":"cup","binarySha256":format!("{:x}",Sha256::digest(b"offline fixture"))},"scope":{"tournamentId":"cup","tournamentSlug":"cup","exchanges":{"101":"101"}},"policy":{},"root":temp.0,"credentials":"/unused","ownerLock":temp.0.join("execution-owner.lock"),"accountScope":"fixture"})).unwrap();
  let authority=ProductionAuthority::new(c.activation.clone(),c.scope.clone(),vec![],control_path,binary,Arc::new(ExclusiveLock::acquire(&c.owner_lock).unwrap())).unwrap();let signal=Arc::new(moloch_replay::realtime::AccountSignal::default());signal.invalidate_for_resync().unwrap();let stamp=iso(signal.last_received_ms());let r=Receipt{endpoint:"original-account-fact".into(),request_started_at:stamp.clone(),received_at:stamp,status:200,data:json!({})};let error=match moloch_live_runtime::account_realtime::capture(&Some(signal),&[&r]){Err(e)=>e,Ok(_)=>panic!("old fact accepted")};
  let head=bank.durable_head();let bytes=fs::read(temp.0.join("dispatch.ndjson")).unwrap();assert!(defer_account_preintent(&c,&bank,&log,&authority,id,false,error.as_ref()).unwrap());assert_eq!(bank.durable_head(),head);assert_eq!(fs::read(temp.0.join("dispatch.ndjson")).unwrap(),bytes);assert!(!defer_account_preintent(&c,&bank,&log,&authority,id,false,&std::io::Error::other("LIVE_ACCOUNT_REALTIME_CHANGED")).unwrap());
  log.append(json!({"kind":"intent","qid":id,"quote":q})).unwrap();log.append(json!({"kind":"attempt","qid":id,"at":iso(now())})).unwrap();assert!(defer_account_preintent(&c,&bank,&log,&authority,id,false,error.as_ref()).is_err());
 }
 fn genuine_reviewed_entry_event(temp:&Temp)->(Session,DispatchLog,Value,Value,Value){
  let(mut bank,mut log,policy,seed)=setup(&temp.0,1);for q in bank.engine().order_records(){retire_for_package(&mut bank,&mut log,&q).unwrap();}
  let at=now();let book=bank.engine().live_package_books().into_iter().find(|b|b["exchangeId"]=="101").unwrap();
  let event=json!({"type":"reviewed_fundamental_entry_reserved_v1","source":"moloch_live_dispatcher","receivedAt":iso(at),"processedAt":iso(at),"bankScope":{"runId":seed["runId"],"bankManifestHash":bank.manifest_hash(),"tournamentId":"cup"},"bankHead":bank.durable_head(),"expectedReportSha256":hash(&bank.engine().report().unwrap()),"marketId":"101","exchangeId":"101","side":"YES","originSha256":"a".repeat(64),"reviewSha256":"b".repeat(64),"bookSha256":hash(&book),"reviewCurrentUntilMs":at+55_000,"reviewMaximumQuantity":2,"reviewMaximumPrincipal":"1.000000","expectedPayout":"0.600000","marginPerShare":"0.010000","limitPrice":"0.400000","expiresAt":iso(at+50_000)});
  (bank,log,event,policy,seed)
 }
 fn genuine_reviewed_entry_reservation(temp:&Temp)->(Session,DispatchLog,Value,Value,Value){
  let(mut bank,log,event,policy,seed)=genuine_reviewed_entry_event(temp);accept(&mut bank,&event).unwrap();let q=bank.engine().order_records().into_iter().find(|q|q["reviewedFundamentalEntry"].is_object()).unwrap();(bank,log,q,policy,seed)
 }
 #[test]fn reviewed_entry_immediate_native_preview_succeeds_without_artificial_gap_wait(){
  let temp=Temp::new();let(bank,log,event,_,_)=genuine_reviewed_entry_event(&temp);let head=bank.durable_head();let report=bank.engine().report().unwrap();let rows=fs::read(temp.0.join("bank/events.jsonl")).unwrap();
  let mut preview=bank.engine().clone();preview.advance(&event).unwrap();assert_eq!(reviewed_entry_budget_wait("",false,now(),2500,3000,now()+10_000).unwrap(),None);
  assert_eq!(bank.durable_head(),head);assert_eq!(bank.engine().report().unwrap(),report);assert_eq!(fs::read(temp.0.join("bank/events.jsonl")).unwrap(),rows);assert!(log.intents.is_empty()&&log.attempted.is_empty());
 }
 #[test]fn reviewed_entry_actual_budget_denial_allows_one_wait_then_fresh_preview(){
  let temp=Temp::new();let(mut bank,mut log,mut event,_,_)=genuine_reviewed_entry_event(&temp);let at=now();let mut book=bank.engine().live_package_books()[0].clone();book["sequence"]=json!("100");for key in ["sourceAt","requestStartedAt","receivedAt"]{book[key]=json!(iso(at));}
  accept(&mut bank,&json!({"type":"book","receivedAt":iso(at),"book":book})).unwrap();
  // Genuine fresh source created ordinary drafts and consumed a native write.
  let drafts:Vec<_>=bank.engine().order_records().into_iter().filter(|q|q["retired"]==false).collect();assert!(!drafts.is_empty());for q in drafts{retire_for_package(&mut bank,&mut log,&q).unwrap();}
  let current=now();event["receivedAt"]=json!(iso(current));event["processedAt"]=json!(iso(current));event["bankHead"]=bank.durable_head();event["expectedReportSha256"]=json!(hash(&bank.engine().report().unwrap()));event["bookSha256"]=json!(hash(&bank.engine().live_package_books()[0]));
  let report=bank.engine().report().unwrap();let head=bank.durable_head();let mut denied=bank.engine().clone();assert_eq!(denied.advance(&event),Err("REVIEWED_ENTRY_WRITE_BUDGET"));
  let ready=reviewed_entry_budget_wait("REVIEWED_ENTRY_WRITE_BUDGET",false,current,2500,3000,current+10_000).unwrap().unwrap();assert_eq!(ready,current+2500);
  // Same original source bytes, genuinely later processing; no native accept.
  let mut retry=event.clone();retry["receivedAt"]=json!(iso(ready));retry["processedAt"]=json!(iso(ready));let mut allowed=bank.engine().clone();allowed.advance(&retry).unwrap();
  assert_eq!(reviewed_entry_budget_wait("REVIEWED_ENTRY_WRITE_BUDGET",true,ready,2500,3000,current+10_000).unwrap(),None);assert_eq!(bank.durable_head(),head);assert_eq!(bank.engine().report().unwrap(),report);assert!(log.intents.is_empty()&&log.attempted.is_empty());
 }
 #[test]fn reviewed_entry_budget_retry_does_not_wait_on_other_errors_or_expired_original_facts(){
  for reason in ["REVIEWED_ENTRY_BOOK","REVIEWED_ENTRY_FUNDING","STOPPED_OR_OWNER_LOST","FUNDAMENTAL_ENTRY_PLAN_CHANGED"]{assert_eq!(reviewed_entry_budget_wait(reason,false,1000,2500,3000,10_000).unwrap(),None);}
  assert_eq!(reviewed_entry_budget_wait("REVIEWED_ENTRY_WRITE_BUDGET",false,1000,2500,3000,6500).unwrap(),None);assert_eq!(reviewed_entry_budget_wait("REVIEWED_ENTRY_WRITE_BUDGET",false,6500,2500,3000,6500).unwrap(),None);assert!(reviewed_entry_budget_wait("REVIEWED_ENTRY_WRITE_BUDGET",false,u64::MAX,2500,3000,u64::MAX).is_err());
 }
 #[test]fn reviewed_entry_activation_outlives_consumed_planning_book_but_grants_no_freshness(){
  let temp=Temp::new();let(bank,log,q,_,_)=genuine_reviewed_entry_reservation(&temp);let active=q["activationAt"].as_u64().unwrap();let old=bank.engine().live_package_books()[0].clone();let source_ms=moloch_live_runtime::daily_windows::clock(old["sourceAt"].as_str().unwrap()).unwrap();assert!(active>=source_ms+bank.engine().policy()["bookAgeMs"].as_u64().unwrap());
  let head=bank.durable_head();let report=bank.engine().report().unwrap();let original=fs::read(temp.0.join("bank/events.jsonl")).unwrap();assert_eq!(reviewed_entry_activation_stage(&q,&q,active-1).unwrap(),Some(active));assert_eq!(reviewed_entry_activation_stage(&q,&q,active).unwrap(),None);
  assert!(moloch_live_runtime::package_admission::native_books(&bank,&Scope{tournament_id:"cup".into(),tournament_slug:"cup".into(),exchanges:[("101".into(),"101".into())].into()},active,bank.engine().policy()["bookAgeMs"].as_u64().unwrap()).unwrap().0.is_empty());assert!(log.attempted.is_empty());assert_eq!(bank.durable_head(),head);assert_eq!(bank.engine().report().unwrap(),report);assert_eq!(fs::read(temp.0.join("bank/events.jsonl")).unwrap(),original);
 }
 #[test]fn reviewed_entry_activation_still_rejects_expiry_review_boundary_and_changed_quote(){
  let temp=Temp::new();let(bank,_log,q,_,_)=genuine_reviewed_entry_reservation(&temp);let active=q["activationAt"].as_u64().unwrap();assert!(reviewed_entry_activation_stage(&q,&q,q["expiresAt"].as_u64().unwrap()).is_err());
  for field in ["reviewCurrentUntilMs"]{let mut ended=q.clone();ended["reviewedFundamentalEntry"][field]=json!(active);assert!(reviewed_entry_activation_stage(&ended,&ended,active).is_err());}
  for(field,value)in [("quantity",json!(1)),("price",json!("0.405000")),("retired",json!(true)),("filled",json!(1)),("cancelEffectiveAt",json!(active)),("reviewedFundamentalEntry",Value::Null)]{let mut changed=q.clone();changed[field]=value;assert!(reviewed_entry_activation_stage(&changed,&q,active).is_err(),"{field}");}
  let id=q["id"].as_u64().unwrap();assert!(!bank.engine().live_order_bindings().contains_key(&id));assert!(bank.engine().fills().is_empty());
 }
 #[test]fn reviewed_entry_activation_restart_preserves_original_reservation_deadline(){
  let temp=Temp::new();let(bank,log,q,policy,seed)=genuine_reviewed_entry_reservation(&temp);let id=q["id"].as_u64().unwrap();let head=bank.durable_head();drop(bank);drop(log);let bank=Session::open_live(&temp.0.join("bank"),&policy,&seed).unwrap();let current=quote(&bank,id).unwrap();assert_eq!(current,q);assert_eq!(reviewed_entry_activation_stage(&current,&q,q["activationAt"].as_u64().unwrap()).unwrap(),None);assert_eq!(bank.durable_head(),head);
 }
 fn closed_startup_fixture(temp:&Temp,partial:bool)->(Session,DispatchLog,Value,Value,Value){
  let(mut bank,mut log,policy,seed)=setup(&temp.0,1);let q=bank.engine().order_records()[0].clone();let id=q["id"].as_u64().unwrap();let stamp=iso(now());
  let receipt=Receipt{endpoint:"/orders".into(),status:200,request_started_at:stamp.clone(),received_at:stamp.clone(),data:json!({"orderId":format!("9{id}"),"exchangeId":q["exchangeId"],"side":if q["side"]=="YES"{"yes"}else{"no"},"action":q["action"],"quantity":q["quantity"],"price":serde_json::from_str::<Value>(q["price"].as_str().unwrap()).unwrap()})};let mut e=feedback::acknowledgment(&q,&receipt,&stamp).unwrap();
  for row in [json!({"kind":"intent","qid":id,"quote":q}),json!({"kind":"attempt","qid":id,"at":stamp}),json!({"kind":"ack","qid":id,"receipt":receipt})]{log.append(row).unwrap();}
  restore_ack(&mut bank,&e).unwrap();
  if partial{let mut fill=e.clone();fill["type"]=json!("actual_fill");fill["fillId"]=json!("80001");fill["quantity"]=json!(1);fill["fee"]=json!("0.000000");fill["executedAt"]=e["receivedAt"].clone();accept(&mut bank,&fill).unwrap();}
  let mut terminal=e.clone();terminal["type"]=json!("actual_order_terminal");terminal["status"]=json!("canceled");terminal["filledQuantity"]=json!(if partial{1}else{0});terminal["fillsComplete"]=json!(true);accept(&mut bank,&terminal).unwrap();log.append(json!({"kind":"closed","qid":id})).unwrap();
  e["processedAt"]=json!(iso(now()));(bank,log,e,policy,seed)
 }
 #[test]fn startup_closed_duplicate_matches_native_and_never_writes_even_after_restart(){
  for partial in [false,true]{let temp=Temp::new();let(bank,log,e,policy,seed)=closed_startup_fixture(&temp,partial);let id=e["orderId"].as_u64().unwrap();let index=moloch_live_runtime::startup_ack::Index::new(&bank).unwrap();let mut native=bank.engine().clone();native.advance(&e).unwrap();
   let head=bank.durable_head();let report=bank.engine().report().unwrap();let bytes=fs::read(temp.0.join("bank/events.jsonl")).unwrap();assert!(index.validate_closed_duplicate(&bank,&log,id,&e).unwrap());assert_eq!(bank.durable_head(),head);assert_eq!(bank.engine().report().unwrap(),report);assert_eq!(fs::read(temp.0.join("bank/events.jsonl")).unwrap(),bytes);drop(bank);drop(log);
   let bank=Session::open_live(&temp.0.join("bank"),&policy,&seed).unwrap();let log=DispatchLog::open(&temp.0.join("dispatch.ndjson")).unwrap();let index=moloch_live_runtime::startup_ack::Index::new(&bank).unwrap();assert!(index.validate_closed_duplicate(&bank,&log,id,&e).unwrap());assert_eq!(bank.durable_head(),head);assert_eq!(bank.engine().report().unwrap(),report);
  }
 }
 #[test]fn startup_closed_duplicate_rejects_the_same_changed_native_ack_fields(){
  let temp=Temp::new();let(bank,log,e,_,_)=closed_startup_fixture(&temp,false);let id=e["orderId"].as_u64().unwrap();let index=moloch_live_runtime::startup_ack::Index::new(&bank).unwrap();
  for(field,value)in [("actualOrderId",json!("999999")),("quantity",json!(1)),("quantity",json!(0)),("exchangeId",json!("999")),("marketId",json!("999")),("side",json!(if e["side"]=="YES"{"NO"}else{"YES"})),("action",json!("sell")),("price",json!("0.995000")),("receiptHash",json!("invalid")),("source",json!("foreign")),("type",json!("actual_order_terminal")),("orderId",json!(999999)),("receivedAt",json!("bad")),("processedAt",json!("1969-12-31T23:59:59.999999999Z")),("deferPlanningForPackage",json!(false))]{let mut bad=e.clone();bad[field]=value;let mut native=bank.engine().clone();assert!(native.advance(&bad).is_err(),"native accepted {field}");assert!(index.validate_closed_duplicate(&bank,&log,id,&bad).is_err(),"index accepted {field}");}
 }
 #[test]fn startup_closed_duplicate_uses_exact_nanosecond_native_clock_boundaries(){
  let temp=Temp::new();let(bank,log,mut e,_,_)=closed_startup_fixture(&temp,false);let id=e["orderId"].as_u64().unwrap();let index=moloch_live_runtime::startup_ack::Index::new(&bank).unwrap();let at=bank.engine().report().unwrap()["at"].clone();e["receivedAt"]=at.clone();e["processedAt"]=at.clone();let mut native=bank.engine().clone();native.advance(&e).unwrap();assert!(index.validate_closed_duplicate(&bank,&log,id,&e).unwrap());
  let stamp=chrono::DateTime::parse_from_rfc3339(at.as_str().unwrap()).unwrap();e["processedAt"]=json!((stamp-chrono::Duration::nanoseconds(1)).to_rfc3339());let mut native=bank.engine().clone();assert!(native.advance(&e).is_err());assert!(index.validate_closed_duplicate(&bank,&log,id,&e).is_err());
  e["processedAt"]=at;e["receivedAt"]=json!((stamp+chrono::Duration::nanoseconds(1)).to_rfc3339());let mut native=bank.engine().clone();assert!(native.advance(&e).is_err());assert!(index.validate_closed_duplicate(&bank,&log,id,&e).is_err());
 }
 #[test]fn startup_index_is_invalidated_by_real_acceptance_and_rebuilt_for_next_ack(){
  let temp=Temp::new();let(mut bank,log,_,_)=setup(&temp.0,1);let index=moloch_live_runtime::startup_ack::Index::new(&bank).unwrap();let e=ack(&bank.engine().order_records()[0]);let id=e["orderId"].as_u64().unwrap();assert!(!index.validate_closed_duplicate(&bank,&log,id,&e).unwrap());assert!(restore_ack(&mut bank,&e).unwrap());assert!(index.quote(&bank,id).is_err());let index=moloch_live_runtime::startup_ack::Index::new(&bank).unwrap();assert!(!index.validate_closed_duplicate(&bank,&log,id,&e).unwrap());assert!(!restore_ack(&mut bank,&e).unwrap());
 }
 #[test]fn startup_closed_fastpath_never_adopts_fundamental_or_unclosed_orders(){
  let temp=Temp::new();let(bank,mut log,e,_,_)=closed_startup_fixture(&temp,false);let id=e["orderId"].as_u64().unwrap();let index=moloch_live_runtime::startup_ack::Index::new(&bank).unwrap();log.closed.remove(&id);assert!(!index.validate_closed_duplicate(&bank,&log,id,&e).unwrap());log.closed.insert(id);log.intents.get_mut(&id).unwrap()["fundamental"]=json!({"bindingSha256":"a".repeat(64)});assert!(!index.validate_closed_duplicate(&bank,&log,id,&e).unwrap());
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

#[cfg(test)]mod owner_capital_startup_tests{
 use super::*;
 #[test]fn expired_funding_is_a_buy_only_defer_and_integrity_errors_remain_fatal(){for code in ["CAPITAL_COMMIT_EXPIRED","CAPITAL_MANDATE_EXPIRED","CAPITAL_ORIGINAL_SOURCE_CLOCK"]{assert!(expected_owner_capital_defer(code));}for code in ["CAPITAL_BOUNDARY_CHANGED","CAPITAL_COMMIT_REFERENCE","LIVE_OWNER_CAPITAL_CONTROL_CHANGED","io failure"]{assert!(!expected_owner_capital_defer(code));}}
 #[test]fn legacy_controls_do_not_change_cap_binding_and_opt_in_requires_verified_approval(){
  let mut c:Control=serde_json::from_value(json!({"activation":{"runId":"test","entryUntilMs":100000,"maximumCommitted":"40000.000000","maximumMarketCost":"10000.000000","maximumQuantity":2,"tournamentId":"cup","tournamentSlug":"cup","binarySha256":"a".repeat(64)},"scope":{"tournamentId":"cup","tournamentSlug":"cup","exchanges":{}},"policy":{"initialCash":"40000.000000","maximumCommitted":"40000.000000","maximumMarketCost":"10000.000000","quantity":2,"entryStopAt":iso(100000)},"root":"/fixture","credentials":"/unused","ownerLock":"/unused.lock","accountScope":"fixture"})).unwrap();assert!(c.owner_capital_allocation.is_none());assert!(validate_owner_capital_control(&c).unwrap());c.activation.maximum_committed="95579.915000".into();assert!(!validate_owner_capital_control(&c).unwrap());
 }
 #[test]fn allocation_uses_four_original_shared_reads_in_order_without_realtime_or_writes(){let mut reads=Vec::new();let _=owner_capital_receipts_with(|endpoint|{reads.push(serde_json::to_value(&endpoint)?);Ok(Receipt{endpoint:format!("original-{}",reads.len()),request_started_at:iso(0),received_at:iso(1),status:200,data:json!({})})}).unwrap();assert_eq!(reads,[Endpoint::Account,Endpoint::Collateral,Endpoint::Positions,Endpoint::Orders{status:"open".into()}].iter().map(|e|serde_json::to_value(e).unwrap()).collect::<Vec<_>>());let mut reads=0;assert!(owner_capital_receipts_with(|_|{reads+=1;Err("original read failed".into())}).is_err());assert_eq!(reads,1);}
 #[test]fn pending_original_bank_withholds_buys_but_not_owned_sell_lane(){
  let root=std::env::temp_dir().join(format!("capital-buy-veto-{}-{}",std::process::id(),now()));let policy=json!({"initialCash":"40000.000000","maximumCommitted":"40000.000000","maximumMarketCost":"10000.000000","quantity":2,"entryStopAt":iso(100000)});let seed=json!({"runId":"test","availableCash":"70850.430000","accountSnapshotHash":"a".repeat(64),"observedAt":iso(0)});let bank=Session::open_live(&root,&policy,&seed).unwrap();let scope=Scope{tournament_id:"cup".into(),tournament_slug:"cup".into(),exchanges:[("101".into(),"101".into())].into()};let scope_sha=format!("{:x}",Sha256::digest(serde_json::to_vec(&scope).unwrap()));let input=json!({"authorizationId":"human_moloch_full_account_20261016","allocationId":"human-all-cash","controlSha256":"b".repeat(64),"bankManifestHash":bank.manifest_hash(),"runId":"test","accountScope":"fixture","tournamentId":"cup","tournamentSlug":"cup","profileId":"profile","scopeSha256":scope_sha,"delta":"55579.915000","maximumExtra":"55579.915000","maximumEffectiveInitial":"95579.915000","maximumCommitted":"95579.915000","approvedAt":"2026-10-16T09:43:45Z","validUntil":"2026-11-03T00:00:00Z"});let c:Control=serde_json::from_value(json!({"ownerCapitalAllocation":{"mandate":input,"approvedMandateSha256":hash(&input),"previousDailyMandateSha256":"c".repeat(64)},"activation":{"runId":"test","entryUntilMs":100000,"maximumCommitted":"95579.915000","maximumMarketCost":"10000.000000","maximumQuantity":2,"tournamentId":"cup","tournamentSlug":"cup","binarySha256":"a".repeat(64)},"dailyEntryWindows":{"authorizationId":"human_moloch_full_account_20261016","approvedAt":"2026-10-16T09:43:45Z","anchorMs":100000,"campaignEndMs":100000+20*86400000u64},"scope":scope,"policy":policy,"root":"/fixture","credentials":"/unused","ownerLock":"/unused.lock","accountScope":"fixture"})).unwrap();assert!(validate_owner_capital_control(&c).unwrap());assert!(!owner_capital_ready(&c,&bank).unwrap());assert!(!owner_capital_buys_allowed(&c,&bank,&json!("buy")).unwrap());assert!(owner_capital_buys_allowed(&c,&bank,&json!("sell")).unwrap());assert_eq!(bank.engine().wallet.initial,40_000_000_000);assert_eq!(bank.engine().policy()["maximumCommitted"],"40000.000000");drop(bank);fs::remove_dir_all(root).unwrap();
 }
}

#[cfg(test)]
mod profitable_depth_control_tests{
 use super::*;
 #[test]fn prospective_mode_is_opt_in_and_omitted_from_old_control_serialization(){
  let original=json!({"activation":{"runId":"fixture","entryUntilMs":100000,"maximumCommitted":"40.000000","maximumMarketCost":"10.000000","maximumQuantity":2,"tournamentId":"cup","tournamentSlug":"cup","binarySha256":"a".repeat(64)},"scope":{"tournamentId":"cup","tournamentSlug":"cup","exchanges":{}},"policy":{},"root":"/fixture","credentials":"/unused","ownerLock":"/fixture/owner.lock","accountScope":"offline"});
  let c:Control=serde_json::from_value(original.clone()).unwrap();assert!(!c.profitable_depth_mode);assert!(serde_json::to_value(c).unwrap().get("profitableDepthMode").is_none());
  let mut enabled=original.clone();enabled["profitableDepthMode"]=json!(true);let c:Control=serde_json::from_value(enabled).unwrap();assert!(c.profitable_depth_mode);assert_eq!(serde_json::to_value(c).unwrap()["profitableDepthMode"],true);
  let mut malformed=original;malformed["profitableDepthMode"]=json!("true");assert!(serde_json::from_value::<Control>(malformed).is_err());
 }
}

#[cfg(test)]mod fundamental_reload_control_tests{
 use super::*;
 #[test]fn legacy_control_keeps_reload_off_and_serialization_omits_it(){let raw=json!({"activation":{"runId":"test","entryUntilMs":9000000,"maximumCommitted":"1000.000000","maximumMarketCost":"100.000000","maximumQuantity":2,"tournamentId":"cup","tournamentSlug":"cup","binarySha256":"a".repeat(64)},"scope":{"tournamentId":"cup","tournamentSlug":"cup","exchanges":{}},"policy":{},"root":"/fixture","credentials":"/unused","ownerLock":"/unused.lock","accountScope":"fixture"});let control:Control=serde_json::from_value(raw.clone()).unwrap();assert!(!control.fundamental_reload);assert!(serde_json::to_value(control).unwrap().get("fundamentalReload").is_none());let mut enabled=raw;enabled["fundamentalReload"]=json!(true);assert!(serde_json::from_value::<Control>(enabled).unwrap().fundamental_reload);}
}
