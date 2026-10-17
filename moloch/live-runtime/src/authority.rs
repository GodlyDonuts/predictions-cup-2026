use baal_transport_service::{Authority,Permit,RequestSpec,Scope,Receipt,TransportError,decimal_units};
use moloch_replay::session::{Session,ExclusiveLock};
use serde::{Serialize,Deserialize};
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::{fs,path::PathBuf,sync::{Arc,Mutex}};
use crate::feedback::Result;
use crate::daily_windows::DailyEntryWindows;
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Activation {pub run_id:String,pub entry_until_ms:u64,pub maximum_committed:String,pub maximum_market_cost:String,pub maximum_quantity:u32,pub tournament_id:String,pub tournament_slug:String,pub binary_sha256:String}
#[derive(Clone)]pub struct SnapshotSet {pub account:Receipt,pub cash_balance:Receipt,pub positions:Receipt,pub open_orders:Receipt,pub market:Receipt,pub contract:Receipt,pub book:Receipt}
/// Cancellation never consumes a contract/book observation. Keep its actual
/// four official account receipts distinct rather than inventing unused facts.
#[derive(Clone)]pub struct CancelSnapshotSet {pub account:Receipt,pub cash_balance:Receipt,pub positions:Receipt,pub open_orders:Receipt}
/// Independent OS-owner files authenticate campaign approval. A nomination's
/// own JSON fields cannot create this capability. Both files remain pinned at
/// physical authorization so replacing or revoking approval invalidates it.
pub struct RankOwnerPins{files:Vec<(PathBuf,String,(u64,u64,u64))>,valid_until_ms:u64}
fn rank_owner_file(path:&std::path::Path)->Result<(Vec<u8>,(u64,u64,u64))>{
 #[cfg(target_os="linux")]{
  use std::{io::Read,os::unix::fs::MetadataExt};
  if !path.is_absolute(){return Err("RANK_OWNER_ABSOLUTE_PATH".into())}
  for p in path.ancestors(){if fs::symlink_metadata(p)?.file_type().is_symlink(){return Err("RANK_OWNER_SYMLINK".into())}}
  let mut file=fs::File::open(path)?;let m=file.metadata()?;
  if !m.is_file()||m.len()>8*1024*1024||m.nlink()!=1||m.uid()!=unsafe{libc::geteuid()}||m.mode()&0o077!=0{return Err("RANK_OWNER_PRIVATE_FILE_REQUIRED".into())}
  let mut raw=Vec::new();Read::by_ref(&mut file).take(8*1024*1024+1).read_to_end(&mut raw)?;let after=file.metadata()?;let named=fs::symlink_metadata(path)?;
  let id=|m:&fs::Metadata|(m.dev(),m.ino(),m.len());
  if raw.len()as u64!=m.len()||id(&m)!=id(&after)||id(&m)!=id(&named)||named.file_type().is_symlink()||m.mtime_nsec()!=after.mtime_nsec()||m.ctime_nsec()!=after.ctime_nsec(){return Err("RANK_OWNER_FILE_CHANGED".into())}
  Ok((raw,id(&m)))
 }
 #[cfg(not(target_os="linux"))]{let _=path;Err("RANK_OWNER_LINUX_REQUIRED".into())}
}
impl RankOwnerPins{
 pub fn capture(files:&[(PathBuf,String)],valid_until_ms:u64)->Result<Self>{
  if files.len()!=2||files[0].0==files[1].0{return Err("RANK_SEPARATE_NOMINATION_AND_OWNER_APPROVAL".into())}
  let mut pins=Vec::new();for(path,sha)in files{let(raw,id)=rank_owner_file(path)?;if hash_bytes(&raw)!=*sha{return Err("RANK_OWNER_FILE_HASH".into())}pins.push((path.clone(),sha.clone(),id));}let result=Self{files:pins,valid_until_ms};result.validate()?;Ok(result)
 }
 pub fn validate(&self)->Result<()>{let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis();if now>=u128::from(self.valid_until_ms){return Err("RANK_OWNER_APPROVAL_EXPIRED".into())}for(path,sha,id)in &self.files{let(raw,current)=rank_owner_file(path)?;if current!=*id||hash_bytes(&raw)!=*sha{return Err("RANK_OWNER_APPROVAL_REVOKED_OR_CHANGED".into())}}Ok(())}
}
#[derive(Clone)]struct OrderReady {account_stamp:Option<crate::account_realtime::Stamp>,spec:RequestSpec,permit:Permit,quote:Value,ledger:Value,snapshots:SnapshotSet,owned:u64,expected_signed:i128,market_cost:i128,expected_cash:i128,entry_until:u64,maker:Option<Arc<crate::maker_policy::Decision>>,fundamental:Option<Arc<crate::fundamental_policy::PhysicalDecision>>,rank:Option<Arc<crate::rank_campaign::PhysicalDecision>>,rank_owner:Option<Arc<RankOwnerPins>>}
#[derive(Clone)]struct CancelReady {account_stamp:Option<crate::account_realtime::Stamp>,spec:RequestSpec,permit:Permit,snapshots:CancelSnapshotSet}
#[derive(Clone)]enum Ready {Order(OrderReady),Cancel(CancelReady)}
pub struct ProductionAuthority {fundamental_generation:Mutex<Option<crate::fundamental_reload::Active>>,fundamental_transition:std::sync::atomic::AtomicBool,account_required:std::sync::atomic::AtomicBool,account_stream:Mutex<Option<Arc<moloch_replay::realtime::AccountSignal>>>,package_account_stamp:Mutex<Option<crate::account_realtime::Stamp>>,fundamental_exchanges:Mutex<Option<std::collections::BTreeSet<String>>>,maker_review:Mutex<Option<String>>,package_ready:Mutex<Option<crate::package_authority::Prepared>>,daily:Mutex<Option<(DailyEntryWindows,u64)>>,activation:Activation,scope:Scope,stops:Vec<PathBuf>,control:PathBuf,control_hash:String,binary:PathBuf,_lease:Arc<ExclusiveLock>,ready:Mutex<Option<Ready>>,owned:Mutex<std::collections::BTreeSet<String>>,recovery:Mutex<Option<String>>,stream:Mutex<Option<(Arc<std::sync::atomic::AtomicBool>,u64)>>}
/// Actual attributed cash only. Borrowed principal remains outside unlevered
/// Wallet sizing; until original fill economics reconcile it cannot authorize
/// a replacement physical order. Cancellation keeps its separate authority.
pub fn reconciled_expected_cash(bank:&Session)->Result<i128>{
 let base=decimal_units(bank.live_account_seed().ok_or_else(||denied("LIVE_ACCOUNT_SEED"))?["availableCash"].as_str().ok_or_else(||denied("LIVE_ACCOUNT_SEED"))?)?.checked_add(bank.engine().wallet.cash).and_then(|n|n.checked_sub(bank.engine().wallet.initial)).and_then(|n|n.checked_add(bank.engine().opening_inventory_cost())).ok_or_else(||denied("LIVE_ACCOUNT_CASH_RANGE"))?;
 if bank.engine().collateral_snapshot_receipts().is_none(){return Ok(base)}
 let c=bank.engine().collateral_report().map_err(|_|denied("LIVE_COLLATERAL_LEDGER"))?;
 if c["pendingNativeFillEconomics"]!=false{return Err("LIVE_COLLATERAL_FILL_RECONCILIATION_REQUIRED".into())}
 let adjustment=decimal_units(c["collateralCashAdjustment"].as_str().ok_or_else(||denied("LIVE_COLLATERAL_LEDGER"))?)?;
 Ok(base.checked_add(adjustment).ok_or("LIVE_ACCOUNT_CASH_RANGE")?)
}
fn hash_bytes(bytes:&[u8])->String{format!("{:x}",Sha256::digest(bytes))}
fn clock(s:&str)->Option<u64>{chrono::DateTime::parse_from_rfc3339(s).ok().and_then(|d|u64::try_from(d.timestamp_millis()).ok())}
fn denied(code:&str)->TransportError{TransportError{code:code.into(),status:None,may_have_executed:false,retry_after_ms:0,order_id:None,api_code:None}}
fn fresh(r:&Receipt,now:u64,max_age:u64)->bool{r.status==200&&clock(&r.request_started_at).is_some_and(|t|t<=now&&now-t<=max_age)&&clock(&r.received_at).is_some_and(|t|t<=now&&now-t<=max_age)}
impl ProductionAuthority {
 pub fn new(activation:Activation,scope:Scope,stops:Vec<PathBuf>,control:PathBuf,binary:PathBuf,lease:Arc<ExclusiveLock>)->Result<Self>{
  if activation.run_id.is_empty()||activation.maximum_quantity==0||activation.maximum_quantity>10000||decimal_units(&activation.maximum_committed)?<=0||decimal_units(&activation.maximum_market_cost)?<=0||scope.tournament_id!=activation.tournament_id||scope.tournament_slug!=activation.tournament_slug||hash_bytes(&fs::read(&binary)?)!=activation.binary_sha256{return Err("LIVE_ACTIVATION_BINDING".into())}
  let control_hash=hash_bytes(&fs::read(&control)?);Ok(Self{fundamental_generation:Mutex::new(None),fundamental_transition:std::sync::atomic::AtomicBool::new(false),account_required:std::sync::atomic::AtomicBool::new(false),account_stream:Mutex::new(None),package_account_stamp:Mutex::new(None),fundamental_exchanges:Mutex::new(None),maker_review:Mutex::new(None),package_ready:Mutex::new(None),daily:Mutex::new(None),activation,scope,stops,control,control_hash,binary,_lease:lease,ready:Mutex::new(None),owned:Mutex::new(Default::default()),recovery:Mutex::new(None),stream:Mutex::new(None)})
 }
 pub fn require_maker_review(&self,sha:&str)->Result<()>{
  if sha.len()!=64||!sha.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b)){return Err("LIVE_MAKER_REVIEW_HASH".into())}
  let mut required=self.maker_review.lock().unwrap();if required.as_ref().is_some_and(|old|old!=sha){return Err("LIVE_MAKER_REVIEW_CHANGED".into())}*required=Some(sha.into());drop(required);self.clear_ready();Ok(())
 }
 /// Immutable scoped routing requirement; registering even an empty set seals
 /// the choice for this authority. It cannot be removed to bypass a held review.
 pub fn require_fundamental_exchanges(&self,exchanges:&std::collections::BTreeSet<String>)->Result<()>{
  if exchanges.len()>self.scope.exchanges.len()||exchanges.iter().any(|e|!self.scope.exchanges.contains_key(e)){return Err("LIVE_FUNDAMENTAL_EXCHANGE_SCOPE".into())}
  let mut required=self.fundamental_exchanges.lock().unwrap();
  if required.as_ref().is_some_and(|old|old!=exchanges){return Err("LIVE_FUNDAMENTAL_EXCHANGES_CHANGED".into())}
  *required=Some(exchanges.clone());drop(required);self.clear_ready();Ok(())
 }
 /// Only the opaque capability reconstructed from this original locked cohort
 /// journal can change routing. Financial/base-Control authority stays sealed.
 pub fn begin_fundamental_generation(&self){self.fundamental_transition.store(true,std::sync::atomic::Ordering::SeqCst);self.clear_ready();}
 pub fn reject_uncommitted_fundamental_generation(&self){self.clear_ready();self.fundamental_transition.store(false,std::sync::atomic::Ordering::SeqCst);}
 pub fn install_fundamental_generation(&self,active:crate::fundamental_reload::Active)->Result<()>{
  active.validate()?;
  if active.exchanges().len()>32||active.exchanges().iter().any(|e|!self.scope.exchanges.contains_key(e)){return Err("LIVE_FUNDAMENTAL_EXCHANGE_SCOPE".into())}
  let mut generation=self.fundamental_generation.lock().unwrap();
  if generation.as_ref().is_some_and(|old|old.generation_sha()!=active.previous_sha()&&old.generation_sha()!=active.generation_sha()){return Err("LIVE_FUNDAMENTAL_GENERATION_CHANGED".into())}
  let mut exchanges=self.fundamental_exchanges.lock().unwrap();
  if exchanges.as_ref().is_some_and(|old|!old.is_subset(active.exchanges())){return Err("LIVE_FUNDAMENTAL_GENERATION_REMOVAL".into())}
  *exchanges=Some(active.exchanges().clone());*generation=Some(active);drop(exchanges);drop(generation);
  self.clear_ready();self.fundamental_transition.store(false,std::sync::atomic::Ordering::SeqCst);Ok(())
 }
 fn fundamental_required(&self,exchange:&str)->bool{self.fundamental_exchanges.lock().unwrap().as_ref().is_some_and(|e|e.contains(exchange))}
 pub fn entry_until_ms(&self)->u64{self.daily.lock().unwrap().as_ref().map_or(self.activation.entry_until_ms,|(_,end)|*end)}
 pub fn bind_entry_window(&self,bank:&Session,cfg:&DailyEntryWindows,at:u64)->Result<()>{
  cfg.validate(&self.activation,at)?;
  let end=cfg.end_at(at)?;
  if bank.engine().policy()["maximumCommitted"]!=self.activation.maximum_committed||bank.engine().policy()["maximumMarketCost"]!=self.activation.maximum_market_cost||bank.engine().policy()["quantity"].as_u64()!=Some(self.activation.maximum_quantity as u64)||!bank.engine().is_live()||bank.live_account_seed().is_none_or(|s|s["runId"]!=self.activation.run_id)||bank.engine().policy()["entryStopAt"]!=crate::daily_windows::iso(end)? {return Err("DAILY_DURABLE_BANK_BINDING".into())}
  if end>cfg.anchor_ms&&bank.engine().report()?["dailyEntryWindows"]!=json!({"mandate":cfg.mandate(&self.activation),"entryStopAt":crate::daily_windows::iso(end)?}){return Err("DAILY_DURABLE_MANDATE_REQUIRED".into())}
  self.clear_ready();
  let mut bound=self.daily.lock().unwrap();
  if let Some((old,old_end))=&*bound{if old.mandate(&self.activation)!=cfg.mandate(&self.activation)||end<*old_end{return Err("DAILY_MANDATE_CHANGED".into())}}
  *bound=Some((cfg.clone(),end));Ok(())
 }
 pub fn bind_owned(&self,id:String){self.owned.lock().unwrap().insert(id);}
 fn validate_generation_sources(&self)->Result<()>{if let Some(g)=self.fundamental_generation.lock().unwrap().as_ref(){g.validate_full()?;}Ok(())}
 pub fn clear_ready(&self){*self.ready.lock().unwrap()=None;*self.package_ready.lock().unwrap()=None;*self.package_account_stamp.lock().unwrap()=None;}
 pub fn bind_account_stream(&self,signal:Option<Arc<moloch_replay::realtime::AccountSignal>>)->Result<()>{
  if signal.is_none()&&self.account_required.load(std::sync::atomic::Ordering::SeqCst){return Err("LIVE_ACCOUNT_REALTIME_CANNOT_DISABLE".into())}
  if signal.is_some(){self.account_required.store(true,std::sync::atomic::Ordering::SeqCst);}
  self.clear_ready();*self.account_stream.lock().unwrap()=signal;Ok(())
 }
 fn account_stamp(&self,receipts:&[&Receipt])->Result<Option<crate::account_realtime::Stamp>>{crate::account_realtime::capture(&self.account_stream.lock().unwrap().clone(),receipts)}
 fn validate_account_stamp(&self,stamp:&Option<crate::account_realtime::Stamp>)->Result<()>{crate::account_realtime::validate(stamp,&self.account_stream.lock().unwrap().clone())}
 pub fn bind_stream(&self,failed:Arc<std::sync::atomic::AtomicBool>,deadline:u64){*self.stream.lock().unwrap()=Some((failed,deadline));}
 pub fn prepare(&self,bank:&Session,qid:u64,spec:RequestSpec,permit:Permit,snapshots:SnapshotSet)->Result<()> {
  self.prepare_with_maker(bank,qid,spec,permit,snapshots,None)
 }
 pub fn prepare_with_maker(&self,bank:&Session,qid:u64,spec:RequestSpec,permit:Permit,snapshots:SnapshotSet,maker:Option<Arc<crate::maker_policy::Decision>>)->Result<()> {
  self.prepare_with_policies(bank,qid,spec,permit,snapshots,maker,None)
 }
 pub fn prepare_with_policies(&self,bank:&Session,qid:u64,spec:RequestSpec,permit:Permit,snapshots:SnapshotSet,maker:Option<Arc<crate::maker_policy::Decision>>,fundamental:Option<Arc<crate::fundamental_policy::PhysicalDecision>>)->Result<()> {
  self.prepare_with_decisions(bank,qid,spec,permit,snapshots,maker,fundamental,None,None)
 }
 pub fn prepare_with_rank(&self,bank:&Session,qid:u64,spec:RequestSpec,permit:Permit,snapshots:SnapshotSet,rank:Arc<crate::rank_campaign::PhysicalDecision>,rank_owner:Arc<RankOwnerPins>)->Result<()> {
  self.prepare_with_decisions(bank,qid,spec,permit,snapshots,None,None,Some(rank),Some(rank_owner))
 }
 fn prepare_with_decisions(&self,bank:&Session,qid:u64,spec:RequestSpec,permit:Permit,snapshots:SnapshotSet,maker:Option<Arc<crate::maker_policy::Decision>>,fundamental:Option<Arc<crate::fundamental_policy::PhysicalDecision>>,rank:Option<Arc<crate::rank_campaign::PhysicalDecision>>,rank_owner:Option<Arc<RankOwnerPins>>)->Result<()> {
  self.validate_generation_sources()?;
  self.clear_ready();
  let account_stamp=self.account_stamp(&[&snapshots.account,&snapshots.cash_balance,&snapshots.positions,&snapshots.open_orders])?;
  if spec.method=="DELETE"{return Err("LIVE_CANCEL_REQUIRES_ACCOUNT_FACTS".into())}
  if !bank.engine().is_live(){return Err("PAPER_BANK_CANNOT_AUTHORIZE_WRITES".into())}
  if bank.engine().is_package_order(qid){return Err("PACKAGE_REQUIRES_BASKET_AUTHORITY".into())}
  if spec.method=="POST"&&bank.engine().package_blocks_submissions(){return Err("PACKAGE_UNRESOLVED".into())}
  let orders=crate::order_snapshot::snapshot(bank);
  let quote=orders.records().iter().find(|q|q["id"].as_u64()==Some(qid)&&q["retired"]==false).cloned().ok_or("LIVE_INTENT_NOT_FUNDED")?;
  let exchange=quote["exchangeId"].as_str().ok_or("LIVE_INTENT_SCOPE")?;
  if usize::from(maker.is_some())+usize::from(fundamental.is_some())+usize::from(rank.is_some())>1{return Err("LIVE_EXCLUSIVE_ORDER_OBJECTIVE".into())}
  if rank.is_some()!=rank_owner.is_some(){return Err("LIVE_RANK_OWNER_APPROVAL_REQUIRED".into())}if let Some(pins)=&rank_owner{pins.validate()?;}
  if quote.get("reviewedRankCampaign").is_some_and(|v|!v.is_null())!=rank.is_some(){return Err("LIVE_RANK_DECISION_REQUIRED".into())}
  if rank.as_ref().is_some_and(|proof|self.fundamental_required(exchange)||proof.native_head()!=&bank.durable_head()){return Err("LIVE_RANK_SCOPE_OR_NATIVE_HEAD".into())}
  if self.fundamental_required(exchange)&&fundamental.is_none(){return Err("LIVE_FUNDAMENTAL_DECISION_REQUIRED".into())}
  if fundamental.as_ref().is_some_and(|proof|proof.native_head()!=&bank.durable_head()){return Err("LIVE_FUNDAMENTAL_NATIVE_HEAD_CHANGED".into())}
  if quote["action"]=="buy"{if let Some(required)=self.maker_review.lock().unwrap().as_ref(){if rank.is_none()&&fundamental.is_none()&&maker.as_ref().is_none_or(|m|m.review_sha256()!=required){return Err("LIVE_MAKER_DECISION_REQUIRED".into())}}}
  let owned=bank.engine().wallet.quantity(quote["exchangeId"].as_str().ok_or("LIVE_INTENT_SCOPE")?,if quote["side"]=="YES"{moloch_replay::Side::Yes}else{moloch_replay::Side::No});
  let ex=quote["exchangeId"].as_str().ok_or("LIVE_INTENT_SCOPE")?;
  let expected_signed=bank.engine().wallet.quantity(ex,moloch_replay::Side::Yes) as i128-bank.engine().wallet.quantity(ex,moloch_replay::Side::No) as i128;
  let market_cost=bank.engine().wallet.market_cost(ex)+orders.records().iter().filter(|q|q["exchangeId"]==ex&&q["action"]=="buy"&&q["retired"]==false).map(|q|decimal_units(q["price"].as_str().unwrap_or("invalid")).map(|p|p*(q["quantity"].as_u64().unwrap_or(0)-q["filled"].as_u64().unwrap_or(0)) as i128)).collect::<std::result::Result<Vec<_>,_>>()?.iter().sum::<i128>();
  let expected_cash=reconciled_expected_cash(bank)?;
  *self.ready.lock().unwrap()=Some(Ready::Order(OrderReady{account_stamp,spec,permit,quote,ledger:bank.engine().report()?,snapshots,owned,expected_signed,market_cost,expected_cash,entry_until:self.entry_until_ms(),maker,fundamental,rank,rank_owner}));Ok(())
 }
 pub fn prepare_cancel(&self,bank:&Session,qid:u64,spec:RequestSpec,permit:Permit,snapshots:CancelSnapshotSet)->Result<()> {
  self.validate_generation_sources()?;
  self.clear_ready();
  let account_stamp=self.account_stamp(&[&snapshots.account,&snapshots.cash_balance,&snapshots.positions,&snapshots.open_orders])?;
  if !bank.engine().is_live()||bank.live_account_seed().is_none_or(|s|s["runId"]!=self.activation.run_id){return Err("LIVE_CANCEL_BANK_SCOPE".into())}
  if bank.engine().is_package_order(qid){return Err("PACKAGE_REQUIRES_BASKET_AUTHORITY".into())}
  let actual=bank.engine().live_order_bindings().get(&qid).ok_or("LIVE_CANCEL_BINDING")?;
  if bank.engine().order_records().iter().all(|q|q["id"].as_u64()!=Some(qid)||q["retired"]!=false)||spec.method!="DELETE"||spec.endpoint!=format!("/orders/{actual}")||spec.body.is_some()||spec.body_json.is_some()||permit.intent_id!=format!("{qid}-cancel")||permit.run_id!=self.activation.run_id {return Err("LIVE_CANCEL_REQUEST_BINDING".into())}
  *self.ready.lock().unwrap()=Some(Ready::Cancel(CancelReady{account_stamp,spec,permit,snapshots}));Ok(())
 }
 pub fn prepare_package(&self,bank:&Session,admission:&crate::package_admission::Admission,permit:Permit,facts:crate::package_authority::PackageSnapshotSet,log:&crate::dispatch::DispatchLog,at:u64)->Result<()> {
  self.validate_generation_sources()?;
  self.clear_ready();let account_stamp=self.account_stamp(&[&facts.account,&facts.cash_balance,&facts.positions,&facts.open_orders])?;log.require_resolved_attempts()?;
  if admission.basket().legs.iter().any(|leg|self.fundamental_required(&leg.exchange_id)){return Err("LIVE_FUNDAMENTAL_PACKAGE_COMPONENT_DENIED".into())}
  let orders=bank.engine().order_records();
  for qid in log.acknowledgments.keys().filter(|qid|!log.closed.contains(qid)){
   let q=orders.iter().find(|q|q["id"].as_u64()==Some(*qid)).ok_or("PACKAGE_PRIOR_ACK_QUOTE_MISSING")?;
   if admission.basket().legs.iter().any(|l|q["exchangeId"]==l.exchange_id){return Err("PACKAGE_PRIOR_COMPONENT_ACK_UNRECONCILED".into())}
  }
  let prepared=crate::package_authority::Prepared::new(bank,&self.activation,&self.scope,admission,permit,facts,at,self.entry_until_ms())?;
  *self.package_ready.lock().unwrap()=Some(prepared);*self.package_account_stamp.lock().unwrap()=account_stamp;Ok(())
 }
 pub fn prepare_portfolio(&self,bank:&Session,plan:&crate::package_portfolio::Plan,permit:Permit,facts:crate::package_authority::PackageSnapshotSet,log:&crate::dispatch::DispatchLog,at:u64)->Result<()> {
  self.validate_generation_sources()?;
  use baal_transport_service::{multi_leg,multi_leg_portfolio as wire};
  self.clear_ready();let account_stamp=self.account_stamp(&[&facts.account,&facts.cash_balance,&facts.positions,&facts.open_orders])?;log.require_resolved_attempts()?;
  let orders=bank.engine().order_records();let component=plan.proof()["portfolio"]["componentExchanges"].as_array().ok_or("PORTFOLIO_COMPONENTS")?;
  for exchange in component{if self.fundamental_required(exchange.as_str().ok_or("PORTFOLIO_COMPONENTS")?){return Err("LIVE_FUNDAMENTAL_PACKAGE_COMPONENT_DENIED".into())}}
  for qid in log.acknowledgments.keys().filter(|id|!log.closed.contains(id)){let q=orders.iter().find(|q|q["id"]==*qid).ok_or("PORTFOLIO_PRIOR_QUOTE")?;if component.iter().any(|ex|*ex==q["exchangeId"]){return Err("PORTFOLIO_PRIOR_ACK_UNRECONCILED".into())}}
  let basket=wire::Basket{idempotency_key:format!("{}:{}",self.activation.run_id,plan.package_id()),tournament_id:self.scope.tournament_id.clone(),maximum_gross_buy_cost:plan.proof()["grossFunding"].as_str().ok_or("PORTFOLIO_FUNDING")?.into(),minimum_gross_sell_proceeds:plan.proof()["minimumCashProceeds"].as_str().ok_or("PORTFOLIO_PROCEEDS")?.into(),relationship_constraint:None,legs:plan.legs().iter().map(|l|Ok(wire::Leg{exchange_id:l.exchange_id.clone(),side:if l.side=="YES"{multi_leg::Side::Yes}else{multi_leg::Side::No},action:if l.action=="buy"{wire::Action::Buy}else{wire::Action::Sell},quantity:u32::try_from(l.quantity)?,limit_price:l.limit_price.clone()})).collect::<Result<_>>()?};
  let prepared=crate::package_authority::Prepared::new_portfolio(bank,&self.activation,&self.scope,plan,&basket,permit,facts,at,self.entry_until_ms())?;*self.package_ready.lock().unwrap()=Some(prepared);*self.package_account_stamp.lock().unwrap()=account_stamp;Ok(())
 }
 /// Separate probabilistic lane. Only the original opaque qualified factory
 /// reservation can populate the existing physical SDK/STOP/stream authority.
 pub fn prepare_probabilistic(&self,bank:&Session,plan:&crate::probabilistic_portfolio::Plan,reservation:&crate::probabilistic_portfolio::Reservation,basket:&baal_transport_service::multi_leg_portfolio::Basket,permit:Permit,facts:crate::package_authority::PackageSnapshotSet,log:&crate::dispatch::DispatchLog,at:u64)->Result<()> {
  self.validate_generation_sources()?;
  self.clear_ready();let account_stamp=self.account_stamp(&[&facts.account,&facts.cash_balance,&facts.positions,&facts.open_orders])?;log.require_resolved_attempts()?;reservation.validate_plan(plan)?;
  let qualification=&reservation.event()["qualification"];let components=qualification["contracts"].as_array().ok_or("PROB_AUTH_COMPONENTS")?;let mut exchanges=std::collections::BTreeSet::new();
  for c in components{let ex=c["exchangeId"].as_str().ok_or("PROB_AUTH_COMPONENTS")?;if !exchanges.insert(ex)||self.fundamental_required(ex){return Err("LIVE_FUNDAMENTAL_PACKAGE_COMPONENT_DENIED".into())}}
  if hash_bytes(&serde_json::to_vec(&log.descriptor_identity()?)?)!=qualification["dispatchSha256"].as_str().ok_or("PROB_AUTH_DISPATCH_BINDING")?{return Err("PROB_AUTH_ORIGINAL_DISPATCH_CHANGED".into())}
  let orders=bank.engine().order_records();
  for qid in log.acknowledgments.keys().filter(|qid|!log.closed.contains(qid)){let q=orders.iter().find(|q|q["id"]==*qid).ok_or("PROB_AUTH_PRIOR_ACK_QUOTE_MISSING")?;if q["exchangeId"].as_str().is_some_and(|ex|exchanges.contains(ex)){return Err("PROB_AUTH_PRIOR_ACK_UNRECONCILED".into())}}
  for qid in bank.engine().live_order_bindings().keys(){let q=orders.iter().find(|q|q["id"]==*qid).ok_or("PROB_AUTH_PRIOR_NATIVE_QUOTE_MISSING")?;if q["exchangeId"].as_str().is_some_and(|ex|exchanges.contains(ex))&&!log.closed.contains(qid){return Err("PROB_AUTH_PRIOR_BINDING_UNRECONCILED".into())}}
  let prepared=crate::package_authority::Prepared::new_probabilistic(bank,&self.activation,&self.scope,plan,reservation,basket,permit,facts,at,self.entry_until_ms())?;*self.package_ready.lock().unwrap()=Some(prepared);*self.package_account_stamp.lock().unwrap()=account_stamp;Ok(())
 }
 fn check(&self,scope:&Scope,spec:&RequestSpec,p:&Permit,network:bool,now:u64)->Result<()> {
  if self.fundamental_transition.load(std::sync::atomic::Ordering::SeqCst){return Err("LIVE_FUNDAMENTAL_GENERATION_PENDING".into())}
  if !self.can_read(scope)||p.owner!="Codex"||p.run_id!=self.activation.run_id||p.expires_at_ms<=now{return Err("LIVE_OWNER_OR_ACTIVATION".into())}
  if network&&hash_bytes(&fs::read(&self.binary)?)!=self.activation.binary_sha256{return Err("LIVE_BINARY_CHANGED".into())}
  if spec.endpoint=="/orders/multi-leg"{
   let has_managed=self.fundamental_exchanges.lock().unwrap().as_ref().is_some_and(|set|!set.is_empty());
   if has_managed{
    for leg in spec.body.as_ref().ok_or("LIVE_PACKAGE_BODY")?["legs"].as_array().ok_or("LIVE_PACKAGE_LEGS")?{if self.fundamental_required(leg["exchangeId"].as_str().ok_or("LIVE_PACKAGE_LEG_SCOPE")?){return Err("LIVE_FUNDAMENTAL_PACKAGE_COMPONENT_DENIED".into())}}
   }
   if self.stream.lock().unwrap().as_ref().is_none_or(|(failed,deadline)|*deadline<=now||failed.load(std::sync::atomic::Ordering::SeqCst)){return Err("LIVE_VERIFIED_STREAM_REQUIRED".into())}
   let end=self.entry_until_ms();
   if self.daily.lock().unwrap().as_ref().is_some_and(|(cfg,bound)|cfg.end_at(now).ok()!=Some(*bound)){return Err("LIVE_ENTRY_OR_ACCOUNT_CASH".into())}
   let locked=self.package_ready.lock().unwrap();let prepared=locked.as_ref().ok_or("PACKAGE_FRESH_FACTS_REQUIRED")?;if prepared.requires_entry()&&now>=end{return Err("LIVE_ENTRY_OR_ACCOUNT_CASH".into())}
   self.validate_account_stamp(&self.package_account_stamp.lock().unwrap())?;
   prepared.authorize(spec,p,now,&self.owned.lock().unwrap(),end)?;
   return self.validate_account_stamp(&self.package_account_stamp.lock().unwrap());
  }
  let locked=self.ready.lock().unwrap();let prepared=locked.as_ref().ok_or("LIVE_FRESH_FACTS_REQUIRED")?;
  let f=match prepared {Ready::Order(f)=>{self.validate_account_stamp(&f.account_stamp)?;f},Ready::Cancel(f)=>{
   self.validate_account_stamp(&f.account_stamp)?;
   if serde_json::to_value(spec)?!=serde_json::to_value(&f.spec)?||serde_json::to_value(p)?!=serde_json::to_value(&f.permit)?{return Err("LIVE_REQUEST_CHANGED".into())}
   self.check_cancel_facts(scope,&f.snapshots,now)?;
   let id=spec.endpoint.strip_prefix("/orders/").ok_or("LIVE_CANCEL_PATH")?;
   if spec.method!="DELETE"||!self.owns_order(id){return Err("LIVE_CANCEL_UNOWNED".into())}self.validate_account_stamp(&f.account_stamp)?;return Ok(())
  }};
  if serde_json::to_value(spec)?!=serde_json::to_value(&f.spec)?||serde_json::to_value(p)?!=serde_json::to_value(&f.permit)?{return Err("LIVE_REQUEST_CHANGED".into())}
  if self.fundamental_required(f.quote["exchangeId"].as_str().ok_or("LIVE_INTENT_SCOPE")?)&&f.fundamental.is_none(){return Err("LIVE_FUNDAMENTAL_DECISION_REQUIRED".into())}
  if f.quote.get("reviewedRankCampaign").is_some_and(|v|!v.is_null())!=f.rank.is_some(){return Err("LIVE_RANK_DECISION_REQUIRED".into())}
  if f.rank.is_some()!=f.rank_owner.is_some(){return Err("LIVE_RANK_OWNER_APPROVAL_REQUIRED".into())}if let Some(pins)=&f.rank_owner{pins.validate()?;}
  if f.rank.is_some()&&(f.maker.is_some()||f.fundamental.is_some()||self.fundamental_required(f.quote["exchangeId"].as_str().ok_or("LIVE_INTENT_SCOPE")?)){return Err("LIVE_RANK_SCOPE_OR_OBJECTIVE".into())}
  if f.quote["action"]=="buy"{if let Some(required)=self.maker_review.lock().unwrap().as_ref(){if f.rank.is_none()&&f.fundamental.is_none()&&f.maker.as_ref().is_none_or(|m|m.review_sha256()!=required){return Err("LIVE_MAKER_DECISION_REQUIRED".into())}}}
  if let Some(maker)=&f.maker{maker.validate_frozen(&f.quote,&f.ledger,f.quote["id"].as_u64().ok_or("LIVE_QUOTE_ID")?,now)?;maker.authorize(scope,spec,p,now)?;}
  if let Some(fundamental)=&f.fundamental{fundamental.validate_contract(&f.snapshots.contract)?;fundamental.validate_frozen(&f.quote,&f.ledger,f.quote["id"].as_u64().ok_or("LIVE_QUOTE_ID")?,now)?;fundamental.authorize(scope,spec,p,now)?;}
  if let Some(rank)=&f.rank{rank.validate_contract(scope,&f.snapshots.contract)?;rank.validate_frozen(&f.quote,&f.ledger,f.quote["id"].as_u64().ok_or("LIVE_QUOTE_ID")?,now)?;rank.authorize(scope,spec,p,now)?;}
  for r in [&f.snapshots.account,&f.snapshots.cash_balance,&f.snapshots.positions,&f.snapshots.open_orders]{if !fresh(r,now,20000){return Err("LIVE_ACCOUNT_STALE".into())}}
  if f.snapshots.account.data["id"]!=scope.tournament_id||f.snapshots.account.data["slug"]!=scope.tournament_slug||f.snapshots.account.data["status"]!="active"||f.snapshots.open_orders.data["pagination"]["hasMore"]!=false{return Err("LIVE_ACCOUNT_SCOPE".into())}
  if spec.method=="DELETE"{return Err("LIVE_CANCEL_REQUIRES_ACCOUNT_FACTS".into())}
  if self.stream.lock().unwrap().as_ref().is_none_or(|(failed,deadline)|*deadline<=now||failed.load(std::sync::atomic::Ordering::SeqCst)){return Err("LIVE_VERIFIED_STREAM_REQUIRED".into())}
  for row in f.snapshots.open_orders.data["data"].as_array().ok_or("LIVE_ORDER_OBSERVATION")?{let id=row["id"].as_u64().map(|n|n.to_string()).or_else(||row["id"].as_str().map(str::to_owned)).ok_or("LIVE_OPEN_ORDER_ID")?;if !self.owns_order(&id){return Err("LIVE_FOREIGN_OPEN_ORDER".into())}}
  let ex=f.quote["exchangeId"].as_str().ok_or("LIVE_INTENT_SCOPE")?;let market=f.quote["marketId"].as_str().ok_or("LIVE_INTENT_SCOPE")?;
  if scope.exchanges.get(ex).is_none_or(|m|m!=market)||f.quote["quantity"].as_u64().is_none_or(|n|n==0||n>self.activation.maximum_quantity as u64){return Err("LIVE_INTENT_LIMIT".into())}
  let mut matches=f.snapshots.positions.data["positions"].as_array().ok_or("LIVE_POSITIONS_REQUIRED")?.iter().filter(|row|row["exchangeId"]==ex);
  let actual_signed=if let Some(row)=matches.next(){row["quantity"].as_i64().ok_or("LIVE_POSITION_QUANTITY")? as i128}else{0};
  if matches.next().is_some()||actual_signed!=f.expected_signed{return Err("LIVE_POSITION_ATTRIBUTION_MISMATCH".into())}
  if f.snapshots.cash_balance.endpoint!=format!("/account?tournamentId={}",scope.tournament_id){return Err("LIVE_ACCOUNT_CASH_SCOPE".into())}
  let cash_economics=baal_transport_service::relationships::parse_buying_power(&f.snapshots.cash_balance.data,scope)?;
  if cash_economics.gross_cash_units!=Some(f.expected_cash){return Err("LIVE_ACCOUNT_CASH_RECONCILIATION_REQUIRED".into())}
  let reserved=decimal_units(f.ledger["reservedCash"].as_str().ok_or("LIVE_LEDGER")?)?;let cost=decimal_units(f.ledger["ownedCost"].as_str().ok_or("LIVE_LEDGER")?)?;let cash=decimal_units(f.ledger["cash"].as_str().ok_or("LIVE_LEDGER")?)?;
  if reserved>cash||reserved+cost>decimal_units(&self.activation.maximum_committed)?||f.market_cost>decimal_units(&self.activation.maximum_market_cost)?{return Err("LIVE_CAPITAL_LIMIT".into())}
  if spec.method!="POST"||spec.endpoint!="/orders"{return Err("LIVE_OPERATION_NOT_ALLOWED".into())}
  for r in [&f.snapshots.market,&f.snapshots.contract,&f.snapshots.book]{if !fresh(r,now,10000){return Err("LIVE_CONTRACT_OR_BOOK_STALE".into())}}
  if f.snapshots.market.data["id"]!=market||f.snapshots.market.data["status"]!="open"||f.snapshots.market.data["contexts"][0]["status"]!="open"||f.snapshots.market.data["isComposite"]!=false||f.snapshots.market.data["isMultiOutcome"]!=false||f.snapshots.contract.data["market_id"]!=market||f.snapshots.contract.data["root"].is_null(){return Err("LIVE_CONTRACT_NOT_SUPPORTED".into())}
  let body=spec.body.as_ref().ok_or("LIVE_ORDER_BODY")?;let n=body["quantity"].as_u64().ok_or("LIVE_ORDER_QUANTITY")?;let price=decimal_units(&body["price"].to_string())?;
  let expiry=clock(body["expirationDate"].as_str().ok_or("LIVE_EXPIRY")?).ok_or("LIVE_EXPIRY")?;
  if expiry<=now||Some(expiry)!=f.quote["expiresAt"].as_u64()||f.quote["cancelEffectiveAt"].as_u64().is_some_and(|t|t<=now)||f.quote["activationAt"].as_u64().is_none_or(|t|t>now)||body["idempotencyKey"].as_str()!=Some(format!("{}:{}",self.activation.run_id,f.quote["id"]).as_str()){return Err("LIVE_INTENT_CLOCK_OR_KEY".into())}
  let b=f.snapshots.book.data["contexts"][0]["orderbook"]["exchanges"].as_array().ok_or("LIVE_BOOK_SCOPE")?.iter().find(|b|b["exchangeId"]==ex).ok_or("LIVE_BOOK_SCOPE")?;
  if !b["asOf"]["sequence"].is_u64()||clock(b["asOf"]["at"].as_str().ok_or("LIVE_BOOK_CLOCK")?).is_none_or(|t|t>now||now-t>10000){return Err("LIVE_BOOK_STALE".into())}
  if body["action"]=="buy"{let opposite=if body["side"]=="yes"{&b["asks"]}else{&b["bids"]};let level=opposite.as_array().and_then(|a|a.first()).ok_or("LIVE_EMPTY_BOOK")?;let yes=baal_transport_service::prices::official_tick_units(&level["price"])?;let ask=if body["side"]=="yes"{yes}else{1_000_000-yes};if price>=ask&&f.rank.is_none()&&!f.fundamental.as_ref().is_some_and(|proof|proof.allows_marketable_reviewed_buy()){return Err("LIVE_ENTRY_MUST_BE_PASSIVE".into())}}
  let native_quantity=f.quote["quantity"].as_u64().ok_or("LIVE_ORDER_QUANTITY")?;
  // A fixed slice is permitted only by an opaque policy token that already
  // authorized this exact spec/permit; ordinary requests remain full native.
  let quantity_matches=if f.fundamental.is_some(){n>0&&n<=native_quantity}else{n==native_quantity};
  if body["tournamentId"]!=scope.tournament_id||body["exchangeId"]!=ex||body["side"].as_str()!=f.quote["side"].as_str().map(|s|if s=="YES"{"yes"}else{"no"})||body["action"]!=f.quote["action"]||!quantity_matches||price!=decimal_units(f.quote["price"].as_str().ok_or("LIVE_ORDER_PRICE")?)?||n>p.maximum_quantity as u64||price*n as i128>decimal_units(&p.maximum_cost)?{return Err("LIVE_ORDER_INTENT_MISMATCH".into())}
  if body["action"]=="buy" {if now>=self.entry_until_ms()||f.entry_until!=self.entry_until_ms()||self.daily.lock().unwrap().as_ref().is_some_and(|(cfg,end)|cfg.end_at(now).ok()!=Some(*end)||f.snapshots.account.data["endDate"].as_str().and_then(clock).is_none_or(|t|t<*end))||cash_economics.gross_cash_units.ok_or("LIVE_ACCOUNT_CASH_REQUIRED")?<price*n as i128{return Err("LIVE_ENTRY_OR_ACCOUNT_CASH".into())}}
  else if body["action"]!="sell"||n>f.owned{return Err("LIVE_SELL_NOT_COVERED".into())}
  if let Some(pins)=&f.rank_owner{pins.validate()?;}self.validate_account_stamp(&f.account_stamp)

 }
 fn check_cancel_facts(&self,s:&Scope,f:&CancelSnapshotSet,now:u64)->Result<()> {
  for r in [&f.account,&f.cash_balance,&f.positions,&f.open_orders]{
   if !fresh(r,now,20000)||clock(&r.request_started_at).zip(clock(&r.received_at)).is_none_or(|(start,end)|start>end){return Err("LIVE_ACCOUNT_STALE".into())}
  }
  if f.account.endpoint!=format!("/tournaments/{}",s.tournament_slug)||f.account.data["id"]!=s.tournament_id||f.account.data["slug"]!=s.tournament_slug||f.account.data["status"]!="active"||f.cash_balance.endpoint!=format!("/account?tournamentId={}",s.tournament_id)||f.positions.endpoint!=format!("/tournaments/{}/portfolio/positions",s.tournament_slug)||f.positions.data["positions"].as_array().is_none()||f.positions.data.get("pagination").is_some()||f.open_orders.endpoint!=format!("/orders?status=open&limit=200&tournamentId={}",s.tournament_id)||f.open_orders.data["coverage"]["complete"]!=true||f.open_orders.data["pagination"]["hasMore"]!=false||f.open_orders.data["data"].as_array().is_none(){return Err("LIVE_ACCOUNT_SCOPE".into())}
  // Validate account economics shape, without demanding a trade cost basis or
  // available cash to remove an existing owned order. Credit never funds a BUY.
  baal_transport_service::relationships::parse_buying_power(&f.cash_balance.data,s)?;Ok(())
 }
}
impl Authority for ProductionAuthority {
 fn can_read(&self,scope:&Scope)->bool{scope.tournament_id==self.scope.tournament_id&&scope.tournament_slug==self.scope.tournament_slug&&scope.exchanges==self.scope.exchanges&&!self.stops.iter().any(|p|p.exists())&&self.fundamental_generation.lock().unwrap().as_ref().is_none_or(|g|g.validate().is_ok())&&fs::read(&self.control).is_ok_and(|b|hash_bytes(&b)==self.control_hash)}
 fn authorize(&self,scope:&Scope,spec:&RequestSpec,p:&Permit,network:bool,now:u64)->std::result::Result<(),TransportError>{self.check(scope,spec,p,network,now).map_err(|e|denied(&e.to_string()))}
 fn owns_order(&self,id:&str)->bool{self.owned.lock().unwrap().contains(id)}
 fn reconcile_unknown(&self,scope:&Scope,binding:&Value,_now:u64)->std::result::Result<(),TransportError>{if self.can_read(scope)&&self.recovery.lock().unwrap().as_ref()==Some(&hash_bytes(&serde_json::to_vec(binding).unwrap())){Ok(())}else{Err(denied("LIVE_RECONCILIATION_PROOF_REQUIRED"))}}
}
#[derive(Clone)]
pub struct SharedAuthority(pub Arc<ProductionAuthority>);
impl Authority for SharedAuthority {
 fn can_read(&self,s:&Scope)->bool{self.0.can_read(s)}
 fn authorize(&self,s:&Scope,r:&RequestSpec,p:&Permit,n:bool,t:u64)->std::result::Result<(),TransportError>{self.0.authorize(s,r,p,n,t)}
 fn owns_order(&self,id:&str)->bool{self.0.owns_order(id)}
 fn reconcile_unknown(&self,s:&Scope,b:&Value,n:u64)->std::result::Result<(),TransportError>{self.0.reconcile_unknown(s,b,n)}
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn daily_binding_requires_durable_event_and_replay_keeps_original_manifest(){
  let(root,a,original,_,spec,permit,facts,n)=fixture();
  let anchor=a.activation.entry_until_ms;let at=anchor+1;
  let cfg=DailyEntryWindows{authorization_id:"human-daily-test".into(),approved_at:crate::daily_windows::iso(n).unwrap(),anchor_ms:anchor,campaign_end_ms:anchor+2*crate::daily_windows::DAY_MS};
  let policy=json!({"quantity":a.activation.maximum_quantity,"maximumCommitted":a.activation.maximum_committed,"maximumMarketCost":a.activation.maximum_market_cost,"entryStopAt":crate::daily_windows::iso(anchor).unwrap()});
  let seed=json!({"runId":"fixture","availableCash":"1000.000000","accountSnapshotHash":"a".repeat(64),"observedAt":"2026-10-13T00:00:00Z"});
  let path=root.join("daily-bank");let mut bank=Session::open_live(&path,&policy,&seed).unwrap();
  assert!(a.bind_entry_window(&bank,&cfg,at).is_err());assert_eq!(a.entry_until_ms(),anchor);
  let end=cfg.end_at(at).unwrap();let e=json!({"type":"live_entry_window","source":"moloch_live_dispatcher","receivedAt":crate::daily_windows::iso(at).unwrap(),"processedAt":crate::daily_windows::iso(at).unwrap(),"previousEntryStopAt":crate::daily_windows::iso(anchor).unwrap(),"entryStopAt":crate::daily_windows::iso(end).unwrap(),"mandate":cfg.mandate(&a.activation)});
  bank.accept("daily",1,&e).unwrap();a.prepare(&original,permit.intent_id.parse().unwrap(),spec,permit,facts).unwrap();assert!(a.ready.lock().unwrap().is_some());a.bind_entry_window(&bank,&cfg,at).unwrap();assert!(a.ready.lock().unwrap().is_none());assert_eq!(a.entry_until_ms(),end);
  drop(bank);let bank=Session::open_live(&path,&policy,&seed).unwrap();a.bind_entry_window(&bank,&cfg,at).unwrap();assert_eq!(bank.engine().policy()["entryStopAt"],crate::daily_windows::iso(end).unwrap());assert!(a.bind_entry_window(&bank,&cfg,end).is_err());
  drop(bank);drop(original);drop(a);fs::remove_dir_all(root).unwrap();
 }
 #[test]fn queued_buy_is_denied_at_exact_cutoff_and_without_official_campaign_end(){
  for daily in [false,true]{let(root,mut a,b,s,r,p,f,n)=fixture();
   a.activation.entry_until_ms=if daily{n-1}else{n+1};
   if daily{let cfg=DailyEntryWindows{authorization_id:"test-daily".into(),approved_at:crate::daily_windows::iso(n-1).unwrap(),anchor_ms:n-1,campaign_end_ms:n+86400000};let end=cfg.end_at(n).unwrap();*a.daily.lock().unwrap()=Some((cfg,end));}
   a.prepare(&b,p.intent_id.parse().unwrap(),r.clone(),p.clone(),f).unwrap();
   if !daily{a.authorize(&s,&r,&p,true,n).unwrap();}
   let error=a.authorize(&s,&r,&p,true,if daily{n}else{n+1}).unwrap_err();assert_eq!(error.code,"LIVE_ENTRY_OR_ACCOUNT_CASH");
   drop(b);drop(a);fs::remove_dir_all(root).unwrap();
  }
 }
 pub(super) fn fixture()->(PathBuf,ProductionAuthority,Session,Scope,RequestSpec,Permit,SnapshotSet,u64){fixture_with_cash("1000.000000")}
 fn fixture_with_cash(exact:&str)->(PathBuf,ProductionAuthority,Session,Scope,RequestSpec,Permit,SnapshotSet,u64){
  static NEXT_FIXTURE:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);
  let serial=NEXT_FIXTURE.fetch_add(1,std::sync::atomic::Ordering::Relaxed);
  let root=std::env::temp_dir().join(format!("live-authority-{}-{}-{serial}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));fs::create_dir(&root).unwrap();
  let binary=root.join("binary");fs::write(&binary,b"fixture executable").unwrap();let control=root.join("control");fs::write(&control,b"immutable").unwrap();
  let lease=Arc::new(ExclusiveLock::acquire(&root.join("owner.lock")).unwrap());let scope=Scope{tournament_id:"cup".into(),tournament_slug:"cup".into(),exchanges:[("101".into(),"101".into())].into()};
  let now=clock("2026-10-13T00:00:04Z").unwrap();let activation=Activation{run_id:"fixture".into(),entry_until_ms:now+3600000,maximum_committed:"40.000000".into(),maximum_market_cost:"10.000000".into(),maximum_quantity:1,tournament_id:"cup".into(),tournament_slug:"cup".into(),binary_sha256:hash_bytes(b"fixture executable")};
  let auth=ProductionAuthority::new(activation,scope.clone(),vec![root.join("STOP")],control,binary,lease).unwrap();
  auth.bind_stream(Arc::new(std::sync::atomic::AtomicBool::new(false)),now+3600000);
  let policy=json!({"quantity":1});let seed=json!({"runId":"fixture","availableCash":exact,"accountSnapshotHash":"a".repeat(64),"observedAt":"2026-10-13T00:00:00Z"});let mut bank=Session::open_live(&root.join("bank"),&policy,&seed).unwrap();
  let corpus:Value=serde_json::from_str(include_str!("../../native/tests/reference/golden.json")).unwrap();bank.accept("market",1,&corpus[0]["steps"][0]["event"]).unwrap();
  let q=bank.engine().order_records().into_iter().find(|q|q["side"]=="YES").unwrap();let expiry=chrono::DateTime::from_timestamp_millis(q["expiresAt"].as_u64().unwrap() as i64).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis,true);
  let body=json!({"idempotencyKey":format!("fixture:{}",q["id"]),"tournamentId":"cup","exchangeId":"101","side":"yes","action":"buy","quantity":1,"price":0.45,"expirationDate":expiry});
  let spec=RequestSpec{method:"POST".into(),endpoint:"/orders".into(),body_json:Some(serde_json::to_string(&body).unwrap()),body:Some(body)};
  let permit=Permit{run_id:"fixture".into(),intent_id:q["id"].to_string(),owner:"Codex".into(),expires_at_ms:now+10000,maximum_quantity:1,maximum_cost:"0.450000".into()};
  let r=|v|Receipt{endpoint:"/fixture".into(),request_started_at:"2026-10-13T00:00:03Z".into(),received_at:"2026-10-13T00:00:04Z".into(),status:200,data:v};
  let facts=SnapshotSet{cash_balance:Receipt{endpoint:"/account?tournamentId=cup".into(),..r(json!({"balance":serde_json::from_str::<Value>(exact).unwrap(),"availableAdvance":0,"buyingPower":serde_json::from_str::<Value>(exact).unwrap()}))},account:r(json!({"id":"cup","slug":"cup","status":"active","myBalance":1000})),positions:r(json!({"positions":[]})),open_orders:r(json!({"data":[],"pagination":{"hasMore":false},"coverage":{"complete":true}})),market:r(json!({"id":"101","status":"open","isComposite":false,"isMultiOutcome":false,"contexts":[{"status":"open"}]})),contract:r(json!({"market_id":"101","root":{"node_type":"contract"}})),book:r(json!({"contexts":[{"orderbook":{"exchanges":[{"exchangeId":"101","asOf":{"sequence":1,"at":"2026-10-13T00:00:04Z"},"bids":[{"price":0.45,"quantity":1000}],"asks":[{"price":0.455,"quantity":1000}]}]}}]}))};
  (root,auth,bank,scope,spec,permit,facts,now)
 }
 #[test]fn real_live_bank_and_fresh_funded_passive_intent_authorize(){let (root,a,b,s,r,p,f,n)=fixture();a.prepare(&b,p.intent_id.parse().unwrap(),r.clone(),p.clone(),f).unwrap();a.authorize(&s,&r,&p,true,n).unwrap();drop(b);drop(a);fs::remove_dir_all(root).unwrap();}
 #[test]fn stale_cash_foreign_holdings_and_foreign_orders_cannot_authorize(){for case in 0..4{let (root,a,b,s,r,p,mut f,n)=fixture();match case{0=>f.account.request_started_at="2026-10-12T00:00:00Z".into(),1=>f.positions.data["positions"]=json!([{"exchangeId":"101","quantity":1}]),2=>f.open_orders.data["data"]=json!([{"id":99}]),_=>f.cash_balance.data=json!({"balance":0,"availableAdvance":0,"buyingPower":0})}a.prepare(&b,p.intent_id.parse().unwrap(),r.clone(),p.clone(),f).unwrap();assert!(a.authorize(&s,&r,&p,true,n).is_err());drop(b);drop(a);fs::remove_dir_all(root).unwrap();}}
 #[test]fn changed_control_binary_stop_and_altered_payload_fail_closed(){for case in 0..4{let (root,a,b,s,mut r,p,f,n)=fixture();a.prepare(&b,p.intent_id.parse().unwrap(),r.clone(),p.clone(),f).unwrap();match case{0=>fs::write(root.join("control"),b"changed").unwrap(),1=>fs::write(root.join("binary"),b"changed").unwrap(),2=>fs::write(root.join("STOP"),b"stop").unwrap(),_=>r.body.as_mut().unwrap()["quantity"]=json!(2)}assert!(a.authorize(&s,&r,&p,true,n).is_err());drop(b);drop(a);fs::remove_dir_all(root).unwrap();}}
 #[test]fn stream_failure_between_preparation_and_network_is_fenced(){
 let (root,a,b,s,r,p,f,n)=fixture();let failed=Arc::new(std::sync::atomic::AtomicBool::new(false));a.bind_stream(failed.clone(),n+60000);a.prepare(&b,p.intent_id.parse().unwrap(),r.clone(),p.clone(),f).unwrap();a.authorize(&s,&r,&p,false,n).unwrap();failed.store(true,std::sync::atomic::Ordering::SeqCst);assert!(a.authorize(&s,&r,&p,true,n).is_err());drop(b);drop(a);fs::remove_dir_all(root).unwrap();
 }
 #[test]fn exact_half_cent_cash_authorizes_despite_rounded_tournament_display(){
  let (root,a,b,s,r,p,mut f,n)=fixture_with_cash("1000.005000");f.account.data["myBalance"]=json!(1000.01);
  a.prepare(&b,p.intent_id.parse().unwrap(),r.clone(),p.clone(),f).unwrap();a.authorize(&s,&r,&p,true,n).unwrap();drop(b);drop(a);fs::remove_dir_all(root).unwrap();
 }
 #[test]fn exact_cash_mismatch_is_rejected_even_with_same_rounded_display(){
  let (root,a,b,s,r,p,mut f,n)=fixture_with_cash("1000.005000");f.account.data["myBalance"]=json!(1000.01);f.cash_balance.data=json!({"balance":1000.006,"availableAdvance":0,"buyingPower":1000.006});
  a.prepare(&b,p.intent_id.parse().unwrap(),r.clone(),p.clone(),f).unwrap();assert_eq!(a.authorize(&s,&r,&p,true,n).unwrap_err().code,"LIVE_ACCOUNT_CASH_RECONCILIATION_REQUIRED");drop(b);drop(a);fs::remove_dir_all(root).unwrap();
 }
 #[test]fn exact_cash_requires_fresh_scoped_nonnull_gross_cash_not_credit(){
  for case in 0..4{let (root,a,b,s,r,p,mut f,n)=fixture();match case{0=>f.cash_balance.endpoint="/account?tournamentId=other".into(),1=>f.cash_balance.request_started_at="2026-10-12T00:00:00Z".into(),2=>f.cash_balance.data=json!({"balance":null,"availableAdvance":0,"buyingPower":null}),_=>f.cash_balance.data=json!({"balance":999,"availableAdvance":1,"buyingPower":1000})};a.prepare(&b,p.intent_id.parse().unwrap(),r.clone(),p.clone(),f).unwrap();assert!(a.authorize(&s,&r,&p,true,n).is_err());drop(b);drop(a);fs::remove_dir_all(root).unwrap();}
 }

}

#[cfg(test)]mod cancellation_fact_tests{
 use super::*;
 fn fixture()->(PathBuf,ProductionAuthority,Session,Scope,RequestSpec,Permit,CancelSnapshotSet,u64){
  let(root,a,mut bank,scope,_,mut permit,full,n)=super::tests::fixture();
  let qid=permit.intent_id.parse::<u64>().unwrap();let q=bank.engine().order_records().into_iter().find(|q|q["id"]==qid).unwrap();
  let ack=json!({"type":"actual_order_ack","source":"official_sig_api_v1","receiptHash":"a".repeat(64),"orderId":qid,"actualOrderId":"201","exchangeId":q["exchangeId"],"marketId":q["marketId"],"side":q["side"],"action":q["action"],"quantity":q["quantity"],"price":q["price"],"receivedAt":crate::daily_windows::iso(n).unwrap()});bank.accept("cancel-ack",1,&ack).unwrap();a.bind_owned("201".into());*a.stream.lock().unwrap()=None;
  permit.intent_id=format!("{qid}-cancel");permit.expires_at_ms=n+60000;
  let mut facts=CancelSnapshotSet{account:full.account,cash_balance:full.cash_balance,positions:full.positions,open_orders:full.open_orders};facts.account.endpoint="/tournaments/cup".into();facts.positions.endpoint="/tournaments/cup/portfolio/positions".into();facts.open_orders.endpoint="/orders?status=open&limit=200&tournamentId=cup".into();
  let spec=RequestSpec{method:"DELETE".into(),endpoint:"/orders/201".into(),body:None,body_json:None};(root,a,bank,scope,spec,permit,facts,n)
 }
 fn qid(p:&Permit)->u64{p.intent_id.strip_suffix("-cancel").unwrap().parse().unwrap()}
 #[test]fn requiring_maker_evidence_never_blocks_original_owned_cancel_authority(){
  let(root,a,b,s,r,p,f,n)=fixture();a.require_maker_review(&"a".repeat(64)).unwrap();a.require_fundamental_exchanges(&["101".into()].into()).unwrap();a.prepare_cancel(&b,qid(&p),r.clone(),p.clone(),f).unwrap();a.authorize(&s,&r,&p,true,n).unwrap();drop(b);drop(a);fs::remove_dir_all(root).unwrap();
 }
 #[test]fn configured_maker_review_cannot_be_skipped_with_legacy_buy_prepare(){
  let(root,a,b,_,r,p,f,_)=super::tests::fixture();a.require_maker_review(&"a".repeat(64)).unwrap();assert_eq!(a.prepare(&b,p.intent_id.parse().unwrap(),r,p,f).unwrap_err().to_string(),"LIVE_MAKER_DECISION_REQUIRED");assert!(a.ready.lock().unwrap().is_none());assert!(a.require_maker_review(&"b".repeat(64)).is_err());drop(b);drop(a);fs::remove_dir_all(root).unwrap();
 }
 #[test]fn owned_cancel_authorizes_with_only_four_facts_without_stream_or_entry_window(){let(root,mut a,b,s,r,p,f,n)=fixture();a.activation.entry_until_ms=n-1;a.prepare_cancel(&b,qid(&p),r.clone(),p.clone(),f).unwrap();a.authorize(&s,&r,&p,false,n).unwrap();a.authorize(&s,&r,&p,true,n).unwrap();drop(b);drop(a);fs::remove_dir_all(root).unwrap();}
 #[test]fn all_cancel_receipts_require_fresh_causal_clocks_and_complete_account_scope(){for case in ["account","cash","positions","orders","unordered","tournament","cash_scope","positions_scope","orders_scope","coverage","pagination","inactive"]{let(root,a,b,s,r,p,mut f,n)=fixture();match case{"account"=>f.account.request_started_at=crate::daily_windows::iso(n-20001).unwrap(),"cash"=>f.cash_balance.request_started_at=crate::daily_windows::iso(n-20001).unwrap(),"positions"=>f.positions.request_started_at=crate::daily_windows::iso(n-20001).unwrap(),"orders"=>f.open_orders.request_started_at=crate::daily_windows::iso(n-20001).unwrap(),"unordered"=>f.cash_balance.request_started_at=crate::daily_windows::iso(n).unwrap(),"tournament"=>f.account.data["id"]=json!("other"),"cash_scope"=>f.cash_balance.endpoint="/account?tournamentId=other".into(),"positions_scope"=>f.positions.endpoint="/tournaments/other/portfolio/positions".into(),"orders_scope"=>f.open_orders.endpoint="/orders?status=open&tournamentId=cup".into(),"coverage"=>f.open_orders.data["coverage"]["complete"]=json!(false),"pagination"=>f.open_orders.data["pagination"]["hasMore"]=json!(true),_=>f.account.data["status"]=json!("ended")};if case=="unordered"{f.cash_balance.received_at=crate::daily_windows::iso(n-1).unwrap();}a.prepare_cancel(&b,qid(&p),r.clone(),p.clone(),f).unwrap();assert!(a.authorize(&s,&r,&p,true,n).is_err(),"{case}");drop(b);drop(a);fs::remove_dir_all(root).unwrap();}}
 #[test]fn cancel_still_checks_control_binary_stop_run_owner_and_exact_request(){for case in ["control","binary","stop","run","owner","request","permit","unowned"]{let(root,a,b,s,mut r,mut p,f,n)=fixture();a.prepare_cancel(&b,qid(&p),r.clone(),p.clone(),f).unwrap();match case{"control"=>fs::write(root.join("control"),b"changed").unwrap(),"binary"=>fs::write(root.join("binary"),b"changed").unwrap(),"stop"=>fs::write(root.join("STOP"),b"stop").unwrap(),"run"=>p.run_id="other".into(),"owner"=>p.owner="other".into(),"request"=>r.endpoint="/orders/202".into(),"permit"=>p.maximum_quantity+=1,_=>{a.owned.lock().unwrap().clear();}}assert!(a.authorize(&s,&r,&p,true,n).is_err(),"{case}");drop(b);drop(a);fs::remove_dir_all(root).unwrap();}}
 #[test]fn cancellation_facts_cannot_authorize_a_buy_or_mismatched_native_binding(){for case in ["post","actual","body","intent"]{let(root,a,b,_,mut r,mut p,f,_)=fixture();let id=qid(&p);match case{"post"=>{r.method="POST".into();r.endpoint="/orders".into();},"actual"=>r.endpoint="/orders/202".into(),"body"=>r.body=Some(json!({})),_=>p.intent_id="999-cancel".into()};assert!(a.prepare_cancel(&b,id,r,p,f).is_err(),"{case}");assert!(a.ready.lock().unwrap().is_none());drop(b);drop(a);fs::remove_dir_all(root).unwrap();}}
 #[test]fn ordinary_buy_keeps_all_three_contract_and_book_freshness_gates(){for case in ["market","contract","book"]{let(root,a,b,s,r,p,mut f,n)=super::tests::fixture();match case{"market"=>f.market.status=503,"contract"=>f.contract.status=503,_=>f.book.status=503};a.prepare(&b,p.intent_id.parse().unwrap(),r.clone(),p.clone(),f).unwrap();assert_eq!(a.authorize(&s,&r,&p,true,n).unwrap_err().code,"LIVE_CONTRACT_OR_BOOK_STALE");drop(b);drop(a);fs::remove_dir_all(root).unwrap();}}
 #[test]fn changing_fact_variants_clears_previously_prepared_authority(){let(root,a,b,s,r,p,f,n)=fixture();a.prepare_cancel(&b,qid(&p),r.clone(),p.clone(),f.clone()).unwrap();let mut buy=r.clone();buy.method="POST".into();assert!(a.authorize(&s,&buy,&p,true,n).is_err());a.clear_ready();assert!(a.authorize(&s,&r,&p,true,n).is_err());drop(b);drop(a);fs::remove_dir_all(root).unwrap();}
}
#[cfg(test)]mod fundamental_tests{
 use super::*;
 #[test]fn managed_buy_requires_real_token_through_all_legacy_prepare_entrypoints(){
  for path in 0..3{let(root,a,b,_,r,p,f,_)=super::tests::fixture();a.require_fundamental_exchanges(&["101".into()].into()).unwrap();a.require_maker_review(&"a".repeat(64)).unwrap();let id=p.intent_id.parse().unwrap();let result=match path{0=>a.prepare(&b,id,r,p,f),1=>a.prepare_with_maker(&b,id,r,p,f,None),_=>a.prepare_with_policies(&b,id,r,p,f,None,None)};assert_eq!(result.unwrap_err().to_string(),"LIVE_FUNDAMENTAL_DECISION_REQUIRED");assert!(a.ready.lock().unwrap().is_none());drop(b);drop(a);fs::remove_dir_all(root).unwrap();}
 }
 #[test]fn fundamental_managed_set_is_immutable_scoped_and_clears_old_ready(){
  let(root,a,b,s,r,p,f,n)=super::tests::fixture();a.prepare(&b,p.intent_id.parse().unwrap(),r.clone(),p.clone(),f).unwrap();a.authorize(&s,&r,&p,true,n).unwrap();let managed=["101".into()].into();a.require_fundamental_exchanges(&managed).unwrap();assert!(a.ready.lock().unwrap().is_none());a.require_fundamental_exchanges(&managed).unwrap();assert_eq!(a.require_fundamental_exchanges(&Default::default()).unwrap_err().to_string(),"LIVE_FUNDAMENTAL_EXCHANGES_CHANGED");assert_eq!(a.require_fundamental_exchanges(&["999".into()].into()).unwrap_err().to_string(),"LIVE_FUNDAMENTAL_EXCHANGE_SCOPE");drop(b);drop(a);fs::remove_dir_all(root).unwrap();
 }
 #[test]fn physical_gate_independently_rejects_managed_ready_without_token(){
  let(root,a,b,s,r,p,f,n)=super::tests::fixture();a.prepare(&b,p.intent_id.parse().unwrap(),r.clone(),p.clone(),f).unwrap();*a.fundamental_exchanges.lock().unwrap()=Some(["101".into()].into());assert_eq!(a.authorize(&s,&r,&p,true,n).unwrap_err().code,"LIVE_FUNDAMENTAL_DECISION_REQUIRED");drop(b);drop(a);fs::remove_dir_all(root).unwrap();
 }
 #[test]fn managed_sell_from_actual_owned_fill_also_requires_real_token(){
  let(root,a,mut bank,_,r,p,f,n)=super::tests::fixture();let qid=p.intent_id.parse::<u64>().unwrap();let q=bank.engine().order_records().into_iter().find(|q|q["id"]==qid).unwrap();let stamp=|at|crate::daily_windows::iso(at).unwrap();let event=|kind:&str|json!({"type":kind,"source":"official_sig_api_v1","receiptHash":"a".repeat(64),"orderId":qid,"actualOrderId":"201","marketId":q["marketId"],"exchangeId":q["exchangeId"],"side":q["side"],"action":"buy","quantity":1,"price":q["price"],"fee":"0.000000","fillId":"301","executedAt":stamp(n),"receivedAt":stamp(n)});
  bank.accept("fundamental-owned",1,&event("actual_order_ack")).unwrap();bank.accept("fundamental-owned",2,&event("actual_fill")).unwrap();let mut seq=2;for old in bank.engine().order_records(){let id=old["id"].as_u64().unwrap();if old["retired"]==false&&!bank.engine().live_order_bindings().contains_key(&id){seq+=1;bank.accept("fundamental-owned",seq,&json!({"type":"unsubmitted_intent_retired","source":"moloch_live_dispatcher","receiptHash":"b".repeat(64),"confirmedNetworkAttempts":0,"deferPlanningForPackage":true,"orderId":id,"receivedAt":stamp(n)})).unwrap();}}
  seq+=1;bank.accept("fundamental-owned",seq,&json!({"type":"tick","receivedAt":stamp(n+3000)})).unwrap();let sell=bank.engine().order_records().into_iter().find(|q|q["retired"]==false&&q["action"]=="sell").expect("owned native SELL after unsubmitted hedge retirement");a.require_fundamental_exchanges(&["101".into()].into()).unwrap();assert_eq!(a.prepare_with_policies(&bank,sell["id"].as_u64().unwrap(),r,p,f,None,None).unwrap_err().to_string(),"LIVE_FUNDAMENTAL_DECISION_REQUIRED");assert!(a.ready.lock().unwrap().is_none());drop(bank);drop(a);fs::remove_dir_all(root).unwrap();
 }
 #[test]fn package_physical_guard_cannot_consume_managed_inventory(){
  let(root,a,b,s,mut r,p,_,n)=super::tests::fixture();a.require_fundamental_exchanges(&["101".into()].into()).unwrap();r.endpoint="/orders/multi-leg".into();r.body=Some(json!({"legs":[{"exchangeId":"101","action":"sell"}]}));r.body_json=Some(serde_json::to_string(r.body.as_ref().unwrap()).unwrap());assert_eq!(a.authorize(&s,&r,&p,true,n).unwrap_err().code,"LIVE_FUNDAMENTAL_PACKAGE_COMPONENT_DENIED");drop(b);drop(a);fs::remove_dir_all(root).unwrap();
 }
}

#[cfg(test)]mod account_realtime_tests{
 use super::*;
 #[test]fn enabling_account_watch_clears_old_prepared_authority_and_has_no_none_bypass(){let(root,a,bank,scope,spec,permit,facts,at)=super::tests::fixture();a.prepare(&bank,permit.intent_id.parse().unwrap(),spec.clone(),permit.clone(),facts.clone()).unwrap();a.authorize(&scope,&spec,&permit,true,at).unwrap();let signal=Arc::new(moloch_replay::realtime::AccountSignal::default());a.bind_account_stream(Some(signal.clone())).unwrap();assert!(a.bind_account_stream(None).is_err());assert!(a.ready.lock().unwrap().is_none());assert!(a.authorize(&scope,&spec,&permit,true,at).is_err());signal.invalidate_for_resync().unwrap();let error=a.prepare(&bank,permit.intent_id.parse().unwrap(),spec,permit,facts).unwrap_err();assert!(error.is::<crate::account_realtime::AccountGenerationChanged>());assert!(a.ready.lock().unwrap().is_none());drop(bank);drop(a);fs::remove_dir_all(root).unwrap();}
 #[test]fn changed_account_after_prepare_blocks_physical_even_when_old_ready_has_no_stamp(){let(root,a,bank,scope,spec,permit,facts,at)=super::tests::fixture();a.prepare(&bank,permit.intent_id.parse().unwrap(),spec.clone(),permit.clone(),facts).unwrap();*a.account_stream.lock().unwrap()=Some(Arc::new(moloch_replay::realtime::AccountSignal::default()));assert_eq!(a.authorize(&scope,&spec,&permit,true,at).unwrap_err().code,"LIVE_ACCOUNT_REALTIME_CHANGED");drop(bank);drop(a);fs::remove_dir_all(root).unwrap();}
}
#[cfg(all(test,target_os="linux"))]
mod rank_owner_pin_tests{
 use super::*;
 use std::os::unix::fs::PermissionsExt;
 fn fixture()->(PathBuf,Vec<(PathBuf,String)>){let root=std::env::temp_dir().join(format!("rank-owner-pin-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));fs::create_dir(&root).unwrap();fs::set_permissions(&root,fs::Permissions::from_mode(0o700)).unwrap();let files=[("nomination",b"reviewed model".as_slice()),("owner-approval",b"independent owner authorization".as_slice())].into_iter().map(|(n,b)|{let p=root.join(n);fs::write(&p,b).unwrap();fs::set_permissions(&p,fs::Permissions::from_mode(0o600)).unwrap();(p,hash_bytes(b))}).collect();(root,files)}
 fn until()->u64{(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()+60000)as u64}
 #[test]fn actual_owner_approval_replacement_revokes_existing_physical_pin(){let(root,files)=fixture();let pins=RankOwnerPins::capture(&files,until()).unwrap();pins.validate().unwrap();fs::write(&files[1].0,b"revoked approval").unwrap();assert!(pins.validate().is_err());fs::remove_dir_all(root).unwrap();}
 #[test]fn nomination_cannot_self_authorize_and_public_or_symlink_files_reject(){let(root,mut files)=fixture();assert!(RankOwnerPins::capture(&[files[0].clone(),files[0].clone()],until()).is_err());fs::set_permissions(&files[1].0,fs::Permissions::from_mode(0o644)).unwrap();assert!(RankOwnerPins::capture(&files,until()).is_err());fs::set_permissions(&files[1].0,fs::Permissions::from_mode(0o600)).unwrap();let link=root.join("link");std::os::unix::fs::symlink(&files[1].0,&link).unwrap();files[1].0=link;assert!(RankOwnerPins::capture(&files,until()).is_err());fs::remove_dir_all(root).unwrap();}
 #[test]fn expired_owner_approval_cannot_become_a_physical_capability(){let(root,files)=fixture();assert!(RankOwnerPins::capture(&files,0).is_err());fs::remove_dir_all(root).unwrap();}
}
