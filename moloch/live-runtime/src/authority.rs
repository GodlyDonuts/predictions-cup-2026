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
#[derive(Clone)]struct OrderReady {spec:RequestSpec,permit:Permit,quote:Value,ledger:Value,snapshots:SnapshotSet,owned:u64,expected_signed:i128,market_cost:i128,expected_cash:i128,entry_until:u64}
#[derive(Clone)]struct CancelReady {spec:RequestSpec,permit:Permit,snapshots:CancelSnapshotSet}
#[derive(Clone)]enum Ready {Order(OrderReady),Cancel(CancelReady)}
pub struct ProductionAuthority {package_ready:Mutex<Option<crate::package_authority::Prepared>>,daily:Mutex<Option<(DailyEntryWindows,u64)>>,activation:Activation,scope:Scope,stops:Vec<PathBuf>,control:PathBuf,control_hash:String,binary:PathBuf,_lease:Arc<ExclusiveLock>,ready:Mutex<Option<Ready>>,owned:Mutex<std::collections::BTreeSet<String>>,recovery:Mutex<Option<String>>,stream:Mutex<Option<(Arc<std::sync::atomic::AtomicBool>,u64)>>}
fn hash_bytes(bytes:&[u8])->String{format!("{:x}",Sha256::digest(bytes))}
fn clock(s:&str)->Option<u64>{chrono::DateTime::parse_from_rfc3339(s).ok().and_then(|d|u64::try_from(d.timestamp_millis()).ok())}
fn denied(code:&str)->TransportError{TransportError{code:code.into(),status:None,may_have_executed:false,retry_after_ms:0,order_id:None,api_code:None}}
fn fresh(r:&Receipt,now:u64,max_age:u64)->bool{r.status==200&&clock(&r.request_started_at).is_some_and(|t|t<=now&&now-t<=max_age)&&clock(&r.received_at).is_some_and(|t|t<=now&&now-t<=max_age)}
impl ProductionAuthority {
 pub fn new(activation:Activation,scope:Scope,stops:Vec<PathBuf>,control:PathBuf,binary:PathBuf,lease:Arc<ExclusiveLock>)->Result<Self>{
  if activation.run_id.is_empty()||activation.maximum_quantity==0||activation.maximum_quantity>10000||decimal_units(&activation.maximum_committed)?<=0||decimal_units(&activation.maximum_market_cost)?<=0||scope.tournament_id!=activation.tournament_id||scope.tournament_slug!=activation.tournament_slug||hash_bytes(&fs::read(&binary)?)!=activation.binary_sha256{return Err("LIVE_ACTIVATION_BINDING".into())}
  let control_hash=hash_bytes(&fs::read(&control)?);Ok(Self{package_ready:Mutex::new(None),daily:Mutex::new(None),activation,scope,stops,control,control_hash,binary,_lease:lease,ready:Mutex::new(None),owned:Mutex::new(Default::default()),recovery:Mutex::new(None),stream:Mutex::new(None)})
 }
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
 pub fn clear_ready(&self){*self.ready.lock().unwrap()=None;*self.package_ready.lock().unwrap()=None;}
 pub fn bind_stream(&self,failed:Arc<std::sync::atomic::AtomicBool>,deadline:u64){*self.stream.lock().unwrap()=Some((failed,deadline));}
 pub fn prepare(&self,bank:&Session,qid:u64,spec:RequestSpec,permit:Permit,snapshots:SnapshotSet)->Result<()> {
  self.clear_ready();
  if spec.method=="DELETE"{return Err("LIVE_CANCEL_REQUIRES_ACCOUNT_FACTS".into())}
  if !bank.engine().is_live(){return Err("PAPER_BANK_CANNOT_AUTHORIZE_WRITES".into())}
  if bank.engine().is_package_order(qid){return Err("PACKAGE_REQUIRES_BASKET_AUTHORITY".into())}
  if spec.method=="POST"&&bank.engine().package_blocks_submissions(){return Err("PACKAGE_UNRESOLVED".into())}
  let quote=bank.engine().order_records().into_iter().find(|q|q["id"].as_u64()==Some(qid)&&q["retired"]==false).ok_or("LIVE_INTENT_NOT_FUNDED")?;
  let owned=bank.engine().wallet.quantity(quote["exchangeId"].as_str().ok_or("LIVE_INTENT_SCOPE")?,if quote["side"]=="YES"{moloch_replay::Side::Yes}else{moloch_replay::Side::No});
  let ex=quote["exchangeId"].as_str().ok_or("LIVE_INTENT_SCOPE")?;
  let expected_signed=bank.engine().wallet.quantity(ex,moloch_replay::Side::Yes) as i128-bank.engine().wallet.quantity(ex,moloch_replay::Side::No) as i128;
  let market_cost=bank.engine().wallet.market_cost(ex)+bank.engine().order_records().iter().filter(|q|q["exchangeId"]==ex&&q["action"]=="buy"&&q["retired"]==false).map(|q|decimal_units(q["price"].as_str().unwrap_or("invalid")).map(|p|p*(q["quantity"].as_u64().unwrap_or(0)-q["filled"].as_u64().unwrap_or(0)) as i128)).collect::<std::result::Result<Vec<_>,_>>()?.iter().sum::<i128>();
  let expected_cash=decimal_units(bank.live_account_seed().ok_or("LIVE_ACCOUNT_SEED")?["availableCash"].as_str().ok_or("LIVE_ACCOUNT_SEED")?)?+bank.engine().wallet.cash-bank.engine().wallet.initial+bank.engine().opening_inventory_cost();
  *self.ready.lock().unwrap()=Some(Ready::Order(OrderReady{spec,permit,quote,ledger:bank.engine().report()?,snapshots,owned,expected_signed,market_cost,expected_cash,entry_until:self.entry_until_ms()}));Ok(())
 }
 pub fn prepare_cancel(&self,bank:&Session,qid:u64,spec:RequestSpec,permit:Permit,snapshots:CancelSnapshotSet)->Result<()> {
  self.clear_ready();
  if !bank.engine().is_live()||bank.live_account_seed().is_none_or(|s|s["runId"]!=self.activation.run_id){return Err("LIVE_CANCEL_BANK_SCOPE".into())}
  if bank.engine().is_package_order(qid){return Err("PACKAGE_REQUIRES_BASKET_AUTHORITY".into())}
  let actual=bank.engine().live_order_bindings().get(&qid).ok_or("LIVE_CANCEL_BINDING")?;
  if bank.engine().order_records().iter().all(|q|q["id"].as_u64()!=Some(qid)||q["retired"]!=false)||spec.method!="DELETE"||spec.endpoint!=format!("/orders/{actual}")||spec.body.is_some()||spec.body_json.is_some()||permit.intent_id!=format!("{qid}-cancel")||permit.run_id!=self.activation.run_id {return Err("LIVE_CANCEL_REQUEST_BINDING".into())}
  *self.ready.lock().unwrap()=Some(Ready::Cancel(CancelReady{spec,permit,snapshots}));Ok(())
 }
 pub fn prepare_package(&self,bank:&Session,admission:&crate::package_admission::Admission,permit:Permit,facts:crate::package_authority::PackageSnapshotSet,log:&crate::dispatch::DispatchLog,at:u64)->Result<()> {
  self.clear_ready();log.require_resolved_attempts()?;
  let orders=bank.engine().order_records();
  for qid in log.acknowledgments.keys().filter(|qid|!log.closed.contains(qid)){
   let q=orders.iter().find(|q|q["id"].as_u64()==Some(*qid)).ok_or("PACKAGE_PRIOR_ACK_QUOTE_MISSING")?;
   if admission.basket().legs.iter().any(|l|q["exchangeId"]==l.exchange_id){return Err("PACKAGE_PRIOR_COMPONENT_ACK_UNRECONCILED".into())}
  }
  let prepared=crate::package_authority::Prepared::new(bank,&self.activation,&self.scope,admission,permit,facts,at,self.entry_until_ms())?;
  *self.package_ready.lock().unwrap()=Some(prepared);Ok(())
 }
 pub fn prepare_portfolio(&self,bank:&Session,plan:&crate::package_portfolio::Plan,permit:Permit,facts:crate::package_authority::PackageSnapshotSet,log:&crate::dispatch::DispatchLog,at:u64)->Result<()> {
  use baal_transport_service::{multi_leg,multi_leg_portfolio as wire};
  self.clear_ready();log.require_resolved_attempts()?;
  let orders=bank.engine().order_records();let component=plan.proof()["portfolio"]["componentExchanges"].as_array().ok_or("PORTFOLIO_COMPONENTS")?;
  for qid in log.acknowledgments.keys().filter(|id|!log.closed.contains(id)){let q=orders.iter().find(|q|q["id"]==*qid).ok_or("PORTFOLIO_PRIOR_QUOTE")?;if component.iter().any(|ex|*ex==q["exchangeId"]){return Err("PORTFOLIO_PRIOR_ACK_UNRECONCILED".into())}}
  let basket=wire::Basket{idempotency_key:format!("{}:{}",self.activation.run_id,plan.package_id()),tournament_id:self.scope.tournament_id.clone(),maximum_gross_buy_cost:plan.proof()["grossFunding"].as_str().ok_or("PORTFOLIO_FUNDING")?.into(),minimum_gross_sell_proceeds:plan.proof()["minimumCashProceeds"].as_str().ok_or("PORTFOLIO_PROCEEDS")?.into(),relationship_constraint:None,legs:plan.legs().iter().map(|l|Ok(wire::Leg{exchange_id:l.exchange_id.clone(),side:if l.side=="YES"{multi_leg::Side::Yes}else{multi_leg::Side::No},action:if l.action=="buy"{wire::Action::Buy}else{wire::Action::Sell},quantity:u32::try_from(l.quantity)?,limit_price:l.limit_price.clone()})).collect::<Result<_>>()?};
  let prepared=crate::package_authority::Prepared::new_portfolio(bank,&self.activation,&self.scope,plan,&basket,permit,facts,at,self.entry_until_ms())?;*self.package_ready.lock().unwrap()=Some(prepared);Ok(())
 }
 fn check(&self,scope:&Scope,spec:&RequestSpec,p:&Permit,network:bool,now:u64)->Result<()> {
  if !self.can_read(scope)||p.owner!="Codex"||p.run_id!=self.activation.run_id||p.expires_at_ms<=now{return Err("LIVE_OWNER_OR_ACTIVATION".into())}
  if network&&hash_bytes(&fs::read(&self.binary)?)!=self.activation.binary_sha256{return Err("LIVE_BINARY_CHANGED".into())}
  if spec.endpoint=="/orders/multi-leg"{
   if self.stream.lock().unwrap().as_ref().is_none_or(|(failed,deadline)|*deadline<=now||failed.load(std::sync::atomic::Ordering::SeqCst)){return Err("LIVE_VERIFIED_STREAM_REQUIRED".into())}
   let end=self.entry_until_ms();
   if self.daily.lock().unwrap().as_ref().is_some_and(|(cfg,bound)|cfg.end_at(now).ok()!=Some(*bound)){return Err("LIVE_ENTRY_OR_ACCOUNT_CASH".into())}
   let locked=self.package_ready.lock().unwrap();let prepared=locked.as_ref().ok_or("PACKAGE_FRESH_FACTS_REQUIRED")?;if prepared.requires_entry()&&now>=end{return Err("LIVE_ENTRY_OR_ACCOUNT_CASH".into())}
   return prepared.authorize(spec,p,now,&self.owned.lock().unwrap(),end);
  }
  let locked=self.ready.lock().unwrap();let prepared=locked.as_ref().ok_or("LIVE_FRESH_FACTS_REQUIRED")?;
  let f=match prepared {Ready::Order(f)=>f,Ready::Cancel(f)=>{
   if serde_json::to_value(spec)?!=serde_json::to_value(&f.spec)?||serde_json::to_value(p)?!=serde_json::to_value(&f.permit)?{return Err("LIVE_REQUEST_CHANGED".into())}
   self.check_cancel_facts(scope,&f.snapshots,now)?;
   let id=spec.endpoint.strip_prefix("/orders/").ok_or("LIVE_CANCEL_PATH")?;
   if spec.method!="DELETE"||!self.owns_order(id){return Err("LIVE_CANCEL_UNOWNED".into())}return Ok(())
  }};
  if serde_json::to_value(spec)?!=serde_json::to_value(&f.spec)?||serde_json::to_value(p)?!=serde_json::to_value(&f.permit)?{return Err("LIVE_REQUEST_CHANGED".into())}
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
  if body["action"]=="buy"{let opposite=if body["side"]=="yes"{&b["asks"]}else{&b["bids"]};let level=opposite.as_array().and_then(|a|a.first()).ok_or("LIVE_EMPTY_BOOK")?;let yes=baal_transport_service::prices::official_tick_units(&level["price"])?;let ask=if body["side"]=="yes"{yes}else{1_000_000-yes};if price>=ask{return Err("LIVE_ENTRY_MUST_BE_PASSIVE".into())}}
  if body["tournamentId"]!=scope.tournament_id||body["exchangeId"]!=ex||body["side"].as_str()!=f.quote["side"].as_str().map(|s|if s=="YES"{"yes"}else{"no"})||body["action"]!=f.quote["action"]||n!=f.quote["quantity"].as_u64().unwrap_or(0)||price!=decimal_units(f.quote["price"].as_str().ok_or("LIVE_ORDER_PRICE")?)?||n>p.maximum_quantity as u64||price*n as i128>decimal_units(&p.maximum_cost)?{return Err("LIVE_ORDER_INTENT_MISMATCH".into())}
  if body["action"]=="buy" {if now>=self.entry_until_ms()||f.entry_until!=self.entry_until_ms()||self.daily.lock().unwrap().as_ref().is_some_and(|(cfg,end)|cfg.end_at(now).ok()!=Some(*end)||f.snapshots.account.data["endDate"].as_str().and_then(clock).is_none_or(|t|t<*end))||cash_economics.gross_cash_units.ok_or("LIVE_ACCOUNT_CASH_REQUIRED")?<price*n as i128{return Err("LIVE_ENTRY_OR_ACCOUNT_CASH".into())}}
  else if body["action"]!="sell"||n>f.owned{return Err("LIVE_SELL_NOT_COVERED".into())}
  Ok(())
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
 fn can_read(&self,scope:&Scope)->bool{scope.tournament_id==self.scope.tournament_id&&scope.tournament_slug==self.scope.tournament_slug&&scope.exchanges==self.scope.exchanges&&!self.stops.iter().any(|p|p.exists())&&fs::read(&self.control).is_ok_and(|b|hash_bytes(&b)==self.control_hash)}
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
  let root=std::env::temp_dir().join(format!("live-authority-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));fs::create_dir(&root).unwrap();
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
 #[test]fn owned_cancel_authorizes_with_only_four_facts_without_stream_or_entry_window(){let(root,mut a,b,s,r,p,f,n)=fixture();a.activation.entry_until_ms=n-1;a.prepare_cancel(&b,qid(&p),r.clone(),p.clone(),f).unwrap();a.authorize(&s,&r,&p,false,n).unwrap();a.authorize(&s,&r,&p,true,n).unwrap();drop(b);drop(a);fs::remove_dir_all(root).unwrap();}
 #[test]fn all_cancel_receipts_require_fresh_causal_clocks_and_complete_account_scope(){for case in ["account","cash","positions","orders","unordered","tournament","cash_scope","positions_scope","orders_scope","coverage","pagination","inactive"]{let(root,a,b,s,r,p,mut f,n)=fixture();match case{"account"=>f.account.request_started_at=crate::daily_windows::iso(n-20001).unwrap(),"cash"=>f.cash_balance.request_started_at=crate::daily_windows::iso(n-20001).unwrap(),"positions"=>f.positions.request_started_at=crate::daily_windows::iso(n-20001).unwrap(),"orders"=>f.open_orders.request_started_at=crate::daily_windows::iso(n-20001).unwrap(),"unordered"=>f.cash_balance.request_started_at=crate::daily_windows::iso(n).unwrap(),"tournament"=>f.account.data["id"]=json!("other"),"cash_scope"=>f.cash_balance.endpoint="/account?tournamentId=other".into(),"positions_scope"=>f.positions.endpoint="/tournaments/other/portfolio/positions".into(),"orders_scope"=>f.open_orders.endpoint="/orders?status=open&tournamentId=cup".into(),"coverage"=>f.open_orders.data["coverage"]["complete"]=json!(false),"pagination"=>f.open_orders.data["pagination"]["hasMore"]=json!(true),_=>f.account.data["status"]=json!("ended")};if case=="unordered"{f.cash_balance.received_at=crate::daily_windows::iso(n-1).unwrap();}a.prepare_cancel(&b,qid(&p),r.clone(),p.clone(),f).unwrap();assert!(a.authorize(&s,&r,&p,true,n).is_err(),"{case}");drop(b);drop(a);fs::remove_dir_all(root).unwrap();}}
 #[test]fn cancel_still_checks_control_binary_stop_run_owner_and_exact_request(){for case in ["control","binary","stop","run","owner","request","permit","unowned"]{let(root,a,b,s,mut r,mut p,f,n)=fixture();a.prepare_cancel(&b,qid(&p),r.clone(),p.clone(),f).unwrap();match case{"control"=>fs::write(root.join("control"),b"changed").unwrap(),"binary"=>fs::write(root.join("binary"),b"changed").unwrap(),"stop"=>fs::write(root.join("STOP"),b"stop").unwrap(),"run"=>p.run_id="other".into(),"owner"=>p.owner="other".into(),"request"=>r.endpoint="/orders/202".into(),"permit"=>p.maximum_quantity+=1,_=>{a.owned.lock().unwrap().clear();}}assert!(a.authorize(&s,&r,&p,true,n).is_err(),"{case}");drop(b);drop(a);fs::remove_dir_all(root).unwrap();}}
 #[test]fn cancellation_facts_cannot_authorize_a_buy_or_mismatched_native_binding(){for case in ["post","actual","body","intent"]{let(root,a,b,_,mut r,mut p,f,_)=fixture();let id=qid(&p);match case{"post"=>{r.method="POST".into();r.endpoint="/orders".into();},"actual"=>r.endpoint="/orders/202".into(),"body"=>r.body=Some(json!({})),_=>p.intent_id="999-cancel".into()};assert!(a.prepare_cancel(&b,id,r,p,f).is_err(),"{case}");assert!(a.ready.lock().unwrap().is_none());drop(b);drop(a);fs::remove_dir_all(root).unwrap();}}
 #[test]fn ordinary_buy_keeps_all_three_contract_and_book_freshness_gates(){for case in ["market","contract","book"]{let(root,a,b,s,r,p,mut f,n)=super::tests::fixture();match case{"market"=>f.market.status=503,"contract"=>f.contract.status=503,_=>f.book.status=503};a.prepare(&b,p.intent_id.parse().unwrap(),r.clone(),p.clone(),f).unwrap();assert_eq!(a.authorize(&s,&r,&p,true,n).unwrap_err().code,"LIVE_CONTRACT_OR_BOOK_STALE");drop(b);drop(a);fs::remove_dir_all(root).unwrap();}}
 #[test]fn changing_fact_variants_clears_previously_prepared_authority(){let(root,a,b,s,r,p,f,n)=fixture();a.prepare_cancel(&b,qid(&p),r.clone(),p.clone(),f.clone()).unwrap();let mut buy=r.clone();buy.method="POST".into();assert!(a.authorize(&s,&buy,&p,true,n).is_err());a.clear_ready();assert!(a.authorize(&s,&r,&p,true,n).is_err());drop(b);drop(a);fs::remove_dir_all(root).unwrap();}
}
