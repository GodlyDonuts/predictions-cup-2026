use baal_transport_service::{Authority,Permit,RequestSpec,Scope,Receipt,TransportError,decimal_units};
use moloch_replay::session::{Session,ExclusiveLock};
use serde::{Serialize,Deserialize};
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::{fs,path::PathBuf,sync::{Arc,Mutex}};
use crate::feedback::Result;
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Activation {pub run_id:String,pub entry_until_ms:u64,pub maximum_committed:String,pub maximum_market_cost:String,pub maximum_quantity:u32,pub tournament_id:String,pub tournament_slug:String,pub binary_sha256:String}
#[derive(Clone)]pub struct SnapshotSet {pub account:Receipt,pub cash_balance:Receipt,pub positions:Receipt,pub open_orders:Receipt,pub market:Receipt,pub contract:Receipt,pub book:Receipt}
#[derive(Clone)]struct Ready {spec:RequestSpec,permit:Permit,quote:Value,ledger:Value,snapshots:SnapshotSet,owned:u64,expected_signed:i128,market_cost:i128,expected_cash:i128}
pub struct ProductionAuthority {activation:Activation,scope:Scope,stops:Vec<PathBuf>,control:PathBuf,control_hash:String,binary:PathBuf,_lease:Arc<ExclusiveLock>,ready:Mutex<Option<Ready>>,owned:Mutex<std::collections::BTreeSet<String>>,recovery:Mutex<Option<String>>,stream:Mutex<Option<(Arc<std::sync::atomic::AtomicBool>,u64)>>}
fn hash_bytes(bytes:&[u8])->String{format!("{:x}",Sha256::digest(bytes))}
fn clock(s:&str)->Option<u64>{chrono::DateTime::parse_from_rfc3339(s).ok().and_then(|d|u64::try_from(d.timestamp_millis()).ok())}
fn denied(code:&str)->TransportError{TransportError{code:code.into(),status:None,may_have_executed:false,retry_after_ms:0,order_id:None,api_code:None}}
fn fresh(r:&Receipt,now:u64,max_age:u64)->bool{r.status==200&&clock(&r.request_started_at).is_some_and(|t|t<=now&&now-t<=max_age)&&clock(&r.received_at).is_some_and(|t|t<=now&&now-t<=max_age)}
impl ProductionAuthority {
 pub fn new(activation:Activation,scope:Scope,stops:Vec<PathBuf>,control:PathBuf,binary:PathBuf,lease:Arc<ExclusiveLock>)->Result<Self>{
  if activation.run_id.is_empty()||activation.maximum_quantity==0||activation.maximum_quantity>10000||decimal_units(&activation.maximum_committed)?<=0||decimal_units(&activation.maximum_market_cost)?<=0||scope.tournament_id!=activation.tournament_id||scope.tournament_slug!=activation.tournament_slug||hash_bytes(&fs::read(&binary)?)!=activation.binary_sha256{return Err("LIVE_ACTIVATION_BINDING".into())}
  let control_hash=hash_bytes(&fs::read(&control)?);Ok(Self{activation,scope,stops,control,control_hash,binary,_lease:lease,ready:Mutex::new(None),owned:Mutex::new(Default::default()),recovery:Mutex::new(None),stream:Mutex::new(None)})
 }
 pub fn bind_owned(&self,id:String){self.owned.lock().unwrap().insert(id);}
 pub fn clear_ready(&self){*self.ready.lock().unwrap()=None;}
 pub fn bind_stream(&self,failed:Arc<std::sync::atomic::AtomicBool>,deadline:u64){*self.stream.lock().unwrap()=Some((failed,deadline));}
 pub fn prepare(&self,bank:&Session,qid:u64,spec:RequestSpec,permit:Permit,snapshots:SnapshotSet)->Result<()> {
  if !bank.engine().is_live(){return Err("PAPER_BANK_CANNOT_AUTHORIZE_WRITES".into())}
  let quote=bank.engine().order_records().into_iter().find(|q|q["id"].as_u64()==Some(qid)&&q["retired"]==false).ok_or("LIVE_INTENT_NOT_FUNDED")?;
  let owned=bank.engine().wallet.quantity(quote["exchangeId"].as_str().ok_or("LIVE_INTENT_SCOPE")?,if quote["side"]=="YES"{moloch_replay::Side::Yes}else{moloch_replay::Side::No});
  let ex=quote["exchangeId"].as_str().ok_or("LIVE_INTENT_SCOPE")?;
  let expected_signed=bank.engine().wallet.quantity(ex,moloch_replay::Side::Yes) as i128-bank.engine().wallet.quantity(ex,moloch_replay::Side::No) as i128;
  let market_cost=bank.engine().wallet.market_cost(ex)+bank.engine().order_records().iter().filter(|q|q["exchangeId"]==ex&&q["action"]=="buy"&&q["retired"]==false).map(|q|decimal_units(q["price"].as_str().unwrap_or("invalid")).map(|p|p*(q["quantity"].as_u64().unwrap_or(0)-q["filled"].as_u64().unwrap_or(0)) as i128)).collect::<std::result::Result<Vec<_>,_>>()?.iter().sum::<i128>();
  let expected_cash=decimal_units(bank.live_account_seed().ok_or("LIVE_ACCOUNT_SEED")?["availableCash"].as_str().ok_or("LIVE_ACCOUNT_SEED")?)?+bank.engine().wallet.cash-bank.engine().wallet.initial+bank.engine().opening_inventory_cost();
  *self.ready.lock().unwrap()=Some(Ready{spec,permit,quote,ledger:bank.engine().report()?,snapshots,owned,expected_signed,market_cost,expected_cash});Ok(())
 }
 fn check(&self,scope:&Scope,spec:&RequestSpec,p:&Permit,network:bool,now:u64)->Result<()> {
  if !self.can_read(scope)||p.owner!="Codex"||p.run_id!=self.activation.run_id||p.expires_at_ms<=now{return Err("LIVE_OWNER_OR_ACTIVATION".into())}
  if network&&hash_bytes(&fs::read(&self.binary)?)!=self.activation.binary_sha256{return Err("LIVE_BINARY_CHANGED".into())}
  let locked=self.ready.lock().unwrap();let f=locked.as_ref().ok_or("LIVE_FRESH_FACTS_REQUIRED")?;
  if serde_json::to_value(spec)?!=serde_json::to_value(&f.spec)?||serde_json::to_value(p)?!=serde_json::to_value(&f.permit)?{return Err("LIVE_REQUEST_CHANGED".into())}
  for r in [&f.snapshots.account,&f.snapshots.cash_balance,&f.snapshots.positions,&f.snapshots.open_orders]{if !fresh(r,now,20000){return Err("LIVE_ACCOUNT_STALE".into())}}
  if f.snapshots.account.data["id"]!=scope.tournament_id||f.snapshots.account.data["slug"]!=scope.tournament_slug||f.snapshots.account.data["status"]!="active"||f.snapshots.open_orders.data["pagination"]["hasMore"]!=false{return Err("LIVE_ACCOUNT_SCOPE".into())}
  if spec.method=="DELETE"{let id=spec.endpoint.strip_prefix("/orders/").ok_or("LIVE_CANCEL_PATH")?;if !self.owns_order(id){return Err("LIVE_CANCEL_UNOWNED".into())}return Ok(())}
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
  if body["action"]=="buy" {if now>=self.activation.entry_until_ms||cash_economics.gross_cash_units.ok_or("LIVE_ACCOUNT_CASH_REQUIRED")?<price*n as i128{return Err("LIVE_ENTRY_OR_ACCOUNT_CASH".into())}}
  else if body["action"]!="sell"||n>f.owned{return Err("LIVE_SELL_NOT_COVERED".into())}
  Ok(())
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
 fn fixture()->(PathBuf,ProductionAuthority,Session,Scope,RequestSpec,Permit,SnapshotSet,u64){fixture_with_cash("1000.000000")}
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
