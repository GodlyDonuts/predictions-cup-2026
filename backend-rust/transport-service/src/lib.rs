pub mod admission;
pub mod credentials;
pub mod prices;

use std::{collections::{BTreeMap,BTreeSet}, path::PathBuf, time::{Duration,SystemTime,UNIX_EPOCH}, io::Read};
use chrono::{DateTime,Utc,SecondsFormat};
use reqwest::{blocking::Client,header::{AUTHORIZATION,HeaderValue},Method,redirect::Policy};
use serde::{Serialize,Deserialize};
use serde_json::{Value,json};
use sha2::{Sha256,Digest};
use zeroize::Zeroizing;
pub use credentials::{DisabledCredentials,WindowsDpapiCredentials};

pub const OFFICIAL_API:&str="https://sig.thesuper.market/api/v1";
const MAX_RESPONSE:u64=8*1024*1024;
pub type Result<T>=std::result::Result<T,TransportError>;
#[derive(Debug,Clone,Serialize)]
#[serde(rename_all="camelCase")]
pub struct TransportError {pub code:String,pub status:Option<u16>,pub may_have_executed:bool,pub retry_after_ms:u64,pub order_id:Option<String>,pub api_code:Option<String>}
impl std::fmt::Display for TransportError {fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result{write!(f,"{}",self.code)}}
impl std::error::Error for TransportError {}
fn fail(code:&str)->TransportError {TransportError{code:code.into(),status:None,may_have_executed:false,retry_after_ms:0,order_id:None,api_code:None}}
fn need(ok:bool,code:&str)->Result<()> {if ok{Ok(())}else{Err(fail(code))}}
fn numeric(s:&str)->bool{!s.is_empty()&&!s.starts_with('0')&&s.bytes().all(|b|b.is_ascii_digit())&&s.parse::<u64>().is_ok_and(|n|n<=9_007_199_254_740_991)}
fn digest(bytes:&[u8])->String{format!("{:x}",Sha256::digest(bytes))}
fn utc(ms:u64)->Result<String>{DateTime::<Utc>::from_timestamp_millis(i64::try_from(ms).map_err(|_|fail("CLOCK"))?).map(|d|d.to_rfc3339_opts(SecondsFormat::Millis,true)).ok_or_else(||fail("CLOCK"))}
pub fn decimal_units(text:&str)->Result<i128>{
 need(text.len()<=128,"DECIMAL_OVERFLOW")?;let negative=text.starts_with('-');let text=if negative{&text[1..]}else{text};let exponent_parts=text.split(['e','E']).collect::<Vec<_>>();need(exponent_parts.len()<=2,"INVALID_EXACT_DECIMAL")?;
 let exponent=if exponent_parts.len()==2{exponent_parts[1].parse::<i32>().map_err(|_|fail("INVALID_EXACT_DECIMAL"))?}else{0};
 let parts=exponent_parts[0].split('.').collect::<Vec<_>>();need(parts.len()<=2&&!parts[0].is_empty()&&parts[0].bytes().all(|b|b.is_ascii_digit()),"INVALID_EXACT_DECIMAL")?;
 let fraction=parts.get(1).copied().unwrap_or("");need((parts.len()==1||!fraction.is_empty())&&fraction.bytes().all(|b|b.is_ascii_digit()),"INVALID_EXACT_DECIMAL")?;
 let coefficient=format!("{}{fraction}",parts[0]).parse::<i128>().map_err(|_|fail("DECIMAL_OVERFLOW"))?;if coefficient==0{return Ok(0);}
 let scale=exponent.checked_add(6).and_then(|n|n.checked_sub(fraction.len() as i32)).ok_or_else(||fail("DECIMAL_OVERFLOW"))?;
 let n=if scale>=0{let factor=10i128.checked_pow(scale as u32).ok_or_else(||fail("DECIMAL_OVERFLOW"))?;coefficient.checked_mul(factor).ok_or_else(||fail("DECIMAL_OVERFLOW"))?}else{let factor=10i128.checked_pow(scale.checked_neg().ok_or_else(||fail("DECIMAL_OVERFLOW"))? as u32).ok_or_else(||fail("INVALID_EXACT_DECIMAL"))?;need(coefficient%factor==0,"INVALID_EXACT_DECIMAL")?;coefficient/factor};
 Ok(if negative{-n}else{n})
}
fn money_value(v:&Value)->Result<i128>{need(v.is_number(),"INVALID_MONEY_NUMBER")?;decimal_units(&v.to_string())}
pub trait CredentialProvider:Send {fn load(&self)->Result<Zeroizing<String>>;}
pub trait Clock:Send {fn now_ms(&self)->u64;fn sleep_ms(&mut self,ms:u64)->Result<()>;}
pub struct SystemClock;
impl Clock for SystemClock {fn now_ms(&self)->u64{SystemTime::now().duration_since(UNIX_EPOCH).map(|d|d.as_millis() as u64).unwrap_or(0)}fn sleep_ms(&mut self,ms:u64)->Result<()>{std::thread::sleep(Duration::from_millis(ms));Ok(())}}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Scope {pub tournament_id:String,pub tournament_slug:String,pub exchanges:BTreeMap<String,String>}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Config {pub queue_dir:PathBuf,pub account_scope:String,pub scope:Scope,pub maximum_pages:usize,#[serde(default)]pub writes_enabled:bool}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct RequestSpec {pub method:String,pub endpoint:String,pub body:Option<Value>,pub body_json:Option<String>}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Permit {pub run_id:String,pub intent_id:String,pub owner:String,pub expires_at_ms:u64,pub maximum_quantity:u32,pub maximum_cost:String}
pub trait Authority:Send {
 fn can_read(&self,scope:&Scope)->bool;
 fn authorize(&self,scope:&Scope,spec:&RequestSpec,permit:&Permit,network:bool,now:u64)->Result<()>;
 fn owns_order(&self,id:&str)->bool;
 fn reconcile_unknown(&self,_scope:&Scope,_binding:&Value,_now:u64)->Result<()> {Err(fail("SCOPED_RECONCILIATION_REQUIRED"))}
}
pub struct ReadOnlyAuthority;
impl Authority for ReadOnlyAuthority {fn can_read(&self,_:&Scope)->bool{true}fn authorize(&self,_:&Scope,_:&RequestSpec,_:&Permit,_:bool,_:u64)->Result<()>{Err(fail("WRITES_DISABLED"))}fn owns_order(&self,_:&str)->bool{false}}
#[derive(Clone,Serialize,Deserialize)]
#[serde(tag="kind",rename_all="snake_case",deny_unknown_fields)]
pub enum Endpoint {
 Tournament,Positions,Orders{status:String},OrderBook{market_id:String,depth:u16},Market{market_id:String},Nodes{market_id:String},Prices{exchange_ids:Vec<String>},OwnedOrder{order_id:String},OwnedFills{order_id:String},History{exchange_id:String,to:String},PortfolioFills,Pnl,Trades{exchange_id:String,from:String,to:String},
}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Order {pub idempotency_key:String,pub exchange_id:String,pub side:String,pub action:String,pub quantity:u32,pub price:String,pub expiration_date:String,pub tournament_id:String}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Receipt {pub endpoint:String,pub request_started_at:String,pub received_at:String,pub status:u16,pub data:Value}
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub struct AccountReconciliation {pub tournament:Receipt,pub positions:Receipt,pub orders:Receipt,pub exact_cash:String,pub started_at:String,pub completed_at:String,pub adopts_holdings:bool}
pub struct Transport {config:Config,credentials:Box<dyn CredentialProvider>,clock:Box<dyn Clock>,authority:Box<dyn Authority>,client:Client,base:String,queue:admission::AccountAdmission,request_sequence:u64,poisoned:bool,full_account_observation:bool}
impl Transport {
 /// Observation never expands order placement or owned-order permissions.
 pub fn with_full_account_observation(mut self)->Self{self.full_account_observation=true;self}
 pub fn new_with_read_budget(config:Config,credentials:Box<dyn CredentialProvider>,clock:Box<dyn Clock>,authority:Box<dyn Authority>,cap:usize,gap_ms:u64)->Result<Self>{let mut t=Self::build(config,credentials,clock,authority,OFFICIAL_API.into())?;t.queue.configure_reads(cap,gap_ms).map_err(|e|fail(&e))?;Ok(t)}
 pub fn new(config:Config,credentials:Box<dyn CredentialProvider>,clock:Box<dyn Clock>,authority:Box<dyn Authority>)->Result<Self>{Self::build(config,credentials,clock,authority,OFFICIAL_API.into())}
 #[cfg(any(test,feature="offline-fixtures"))]
 pub fn new_for_mock(config:Config,credentials:Box<dyn CredentialProvider>,clock:Box<dyn Clock>,authority:Box<dyn Authority>,base:String)->Result<Self>{let url=reqwest::Url::parse(&base).map_err(|_|fail("MOCK_URL"))?;need(url.scheme()=="http"&&matches!(url.host_str(),Some("127.0.0.1"|"localhost"|"::1"))&&url.path()=="/api/v1"&&url.query().is_none()&&url.fragment().is_none(),"LOOPBACK_FIXTURE_ONLY")?;Self::build(config,credentials,clock,authority,base)}
 fn build(config:Config,credentials:Box<dyn CredentialProvider>,clock:Box<dyn Clock>,authority:Box<dyn Authority>,base:String)->Result<Self>{
  need(config.queue_dir.is_absolute()&&!config.account_scope.is_empty()&&config.maximum_pages>0&&config.maximum_pages<=1000,"INVALID_CONFIGURATION")?;
  let scope=&config.scope;need(scope.tournament_id.len()==36&&scope.tournament_id.bytes().all(|b|b.is_ascii_hexdigit()||b==b'-')&&!scope.tournament_slug.is_empty()&&scope.tournament_slug.bytes().all(|b|b.is_ascii_lowercase()||b.is_ascii_digit()||b==b'-')&&!scope.exchanges.is_empty()&&scope.exchanges.iter().all(|(e,m)|numeric(e)&&numeric(m)),"INVALID_SCOPE")?;
  let token=format!("{}-{}",std::process::id(),clock.now_ms());let queue=admission::AccountAdmission::open(&config.queue_dir,&config.account_scope,&token).map_err(|e|fail(&e))?;
  let client=Client::builder().no_proxy().redirect(Policy::none()).timeout(Duration::from_secs(20)).connect_timeout(Duration::from_secs(10)).build().map_err(|_|fail("HTTP_CLIENT_UNAVAILABLE"))?;
  Ok(Self{config,credentials,clock,authority,client,base,queue,request_sequence:0,poisoned:false,full_account_observation:false})
 }
 fn command(&mut self,mut c:Value)->Result<Value>{c["scope"]=json!(self.config.account_scope);c["now"]=json!(self.clock.now_ms());self.queue.command(c).map_err(|e|fail(&e))}
 fn guard(&self,spec:&RequestSpec,permit:Option<&Permit>,network:bool)->Result<()> {
  need(!self.poisoned&&self.authority.can_read(&self.config.scope),"STOPPED_OR_OWNER_LOST")?;
  if spec.method!="GET"&&!(spec.method=="POST"&&spec.endpoint=="/realtime/token"&&spec.body.is_none()&&spec.body_json.is_none()){need(self.config.writes_enabled,"WRITES_DISABLED")?;let p=permit.ok_or_else(||fail("PERMIT_REQUIRED"))?;need(p.owner=="Codex"&&p.expires_at_ms>self.clock.now_ms()&&!p.run_id.is_empty()&&!p.intent_id.is_empty(),"PERMIT_EXPIRED_OR_INVALID")?;self.authority.authorize(&self.config.scope,spec,p,network,self.clock.now_ms())?;need(self.authority.can_read(&self.config.scope),"STOPPED_OR_OWNER_LOST")?;}
  Ok(())
 }
 fn wait(&mut self,until:u64,spec:&RequestSpec,p:Option<&Permit>)->Result<()> {while self.clock.now_ms()<until{self.guard(spec,p,false)?;let remaining=until-self.clock.now_ms();self.clock.sleep_ms(remaining.min(200))?;}self.guard(spec,p,false)}
 fn perform(&mut self,spec:RequestSpec,permit:Option<&Permit>,validator:impl Fn(&Self,&Value)->Result<()>)->Result<Receipt>{
  self.guard(&spec,permit,false)?;self.request_sequence+=1;let request=format!("request-{}-{}",self.clock.now_ms(),self.request_sequence);let write=spec.method!="GET"&&spec.endpoint!="/realtime/token";
  let binding=if write{let p=permit.unwrap();let key=spec.body.as_ref().and_then(|b|b.get("idempotencyKey")).and_then(Value::as_str).map(str::to_string).unwrap_or_else(||format!("{}:{}:cancel",p.run_id,p.intent_id));Some(json!({"intent":format!("{}:{}",p.run_id,p.intent_id),"key":key,"payload_hash":digest(serde_json::to_string(&json!({"scope":self.config.scope,"spec":spec})).unwrap().as_bytes())}))}else{None};
  loop{self.guard(&spec,permit,false)?;let c=json!({"op":"admit","request":request,"write":write,"binding":binding,"authorized":write,"lease_valid":write,"permit_valid":write});let r=self.command(c)?;if r["admitted"]==true{break;}let until=r["wait_until"].as_u64().ok_or_else(||fail("QUEUE_RESPONSE"))?;self.wait(until,&spec,permit)?;}
  let network_guard=self.guard(&spec,permit,true);if let Err(error)=network_guard{let _=self.command(json!({"op":"complete","request":request,"outcome":"not_sent"}));return Err(error);}
  // Authentication is loaded only after scoped admission, never at process startup.
  let key=match self.credentials.load(){Ok(k)=>k,Err(e)=>{let _=self.command(json!({"op":"complete","request":request,"outcome":"not_sent"}));return Err(e)}};
  let encoded=Zeroizing::new(format!("Bearer {}",key.as_str()));let mut header=match HeaderValue::from_str(encoded.as_str()){Ok(h)=>h,Err(_)=>{let _=self.command(json!({"op":"complete","request":request,"outcome":"not_sent"}));return Err(fail("CREDENTIAL_FORMAT"));}};header.set_sensitive(true);
  self.command(json!({"op":"network","request":request,"write":write,"binding":binding,"authorized":write,"lease_valid":write,"permit_valid":write}))?;
  let started=self.clock.now_ms();let final_guard=self.guard(&spec,permit,true);
  let mut observed_status=0u16;let mut observed_retry=String::new();
  let call=final_guard.and_then(|_|{
   let method=Method::from_bytes(spec.method.as_bytes()).map_err(|_|fail("HTTP_METHOD"))?;let mut builder=self.client.request(method,self.base.clone()+&spec.endpoint).header(AUTHORIZATION,header);
   if let Some(body)=&spec.body_json{builder=builder.header("Content-Type","application/json").body(body.clone());}
   let mut response=builder.send().map_err(|_|fail("CONNECTION_UNCONFIRMED"))?;let status=response.status().as_u16();let retry=response.headers().get("Retry-After").and_then(|v|v.to_str().ok()).unwrap_or("").to_string();observed_status=status;observed_retry=retry.clone();let mut bytes=Vec::new();response.by_ref().take(MAX_RESPONSE+1).read_to_end(&mut bytes).map_err(|_|fail("BODY_UNCONFIRMED"))?;need(bytes.len() as u64<=MAX_RESPONSE,"RESPONSE_TOO_LARGE")?;let data:Value=serde_json::from_slice(&bytes).map_err(|_|fail("INVALID_RESPONSE_JSON"))?;Ok((status,retry,data))
  });
  let (outcome,status,retry,api_code)=match &call{Ok((status,retry,data))=>{let code=data.pointer("/error/code").and_then(Value::as_str).unwrap_or("");let unknown=write&&(*status>=500||*status==409&&["REQUEST_IN_FLIGHT","ORDER_STATUS_UNKNOWN"].contains(&code));(if unknown{"unknown"}else if (200..300).contains(status){"success"}else{"rejected"},*status,retry.clone(),code.to_string())},Err(_)=>(if write{"unknown"}else{"rejected"},observed_status,observed_retry,String::new())};
  let returned_order_id=call.as_ref().ok().and_then(|(_,_,d)|id_value(&d["orderId"]));
  let validated=call.and_then(|(status,retry,data)|{if !(200..300).contains(&status){let mut e=fail("OFFICIAL_REQUEST_REJECTED");e.status=Some(status);e.may_have_executed=outcome=="unknown";let _=retry;return Err(e);}validator(self,&data)?;Ok((status,data))});
  let final_outcome=if write&&validated.is_err()&&status<300{"unknown"}else{outcome};
  let completion=self.command(json!({"op":"complete","request":request,"outcome":final_outcome,"status":status,"retry_after":retry,"api_code":if api_code=="REQUEST_IN_FLIGHT"{"REQUEST_IN_FLIGHT"}else{""}}));
  if completion.is_err(){self.poisoned=true;let mut e=fail("COMPLETION_DURABILITY_UNCERTAIN");e.may_have_executed=write;return Err(e);}
  let retry_ms=completion.as_ref().ok().and_then(|v|v["next"].as_u64()).unwrap_or(0).saturating_sub(self.clock.now_ms());
  match validated{Ok((status,data))=>Ok(Receipt{endpoint:spec.endpoint,request_started_at:utc(started)?,received_at:utc(self.clock.now_ms())?,status,data}),Err(mut e)=>{if status!=0{e.status=Some(status)}e.may_have_executed|=write&&final_outcome=="unknown";e.order_id=returned_order_id;e.retry_after_ms=retry_ms;e.api_code=safe_api_code(&api_code);Err(e)}}
 }
 pub fn compact(&mut self)->Result<Value>{need(self.authority.can_read(&self.config.scope),"STOPPED_OR_OWNER_LOST")?;self.command(json!({"op":"compact"}))}
 /// Short-lived authentication material stays in memory; callers must not log it.
 /// Charged to account reads; 24 order writes leave headroom for token issuance.
 pub fn realtime_token(&mut self)->Result<Value>{
  let r=self.perform(RequestSpec{method:"POST".into(),endpoint:"/realtime/token".into(),body:None,body_json:None},None,|s,v|{
   for k in ["token","anonKey","supabaseUrl"]{need(v[k].as_str().is_some_and(|x|!x.is_empty()&&x.len()<=32768&&!x.contains(['\r','\n'])),"REALTIME_AUTH_FORMAT")?;}
   need(parse_clock(&v["expiresAt"]).is_some_and(|t|t>s.clock.now_ms()+60000),"REALTIME_AUTH_EXPIRY")
  })?;Ok(r.data)
 }
 pub fn admission_state(&mut self)->Result<Value>{self.command(json!({"op":"status"}))}
 pub fn reserve_legacy_migration_boundary(&mut self,legacy_next:u64,manual_review:bool)->Result<()> {
  // Parent must first prove the old account owner/requests are stopped. This
  // conservative quiet minute prevents an empty Rust history from forgetting
  // the old rolling budget. It does not authorize any HTTP or order activation.
  let now=self.clock.now_ms();let until=now.checked_add(60_001).ok_or_else(||fail("MIGRATION_CLOCK"))?.max(legacy_next);
  self.queue.seed_legacy_headroom(now,until,manual_review).map_err(|e|fail(&e))
 }
 fn scoped_market(&self,id:&str)->Result<()> {need(numeric(id)&&self.config.scope.exchanges.values().any(|m|m==id),"MARKET_SCOPE_MISMATCH")}
 fn owned(&self,id:&str)->Result<()> {need(numeric(id)&&self.authority.owns_order(id),"ORDER_NOT_OWNED")}
 fn path(&self,endpoint:&Endpoint,cursor:Option<&str>)->Result<String>{
  let tid=&self.config.scope.tournament_id;let slug=&self.config.scope.tournament_slug;
  let suffix=if let Some(c)=cursor{need(!c.is_empty()&&c.len()<=4096&&!c.chars().any(char::is_control),"INVALID_CURSOR")?;format!("&cursor={}",percent(c))}else{String::new()};
  Ok(match endpoint {
   Endpoint::Tournament=>format!("/tournaments/{slug}"),Endpoint::Positions=>format!("/tournaments/{slug}/portfolio/positions"),
   Endpoint::Orders{status}=>{need(["all","open","closed","expired"].contains(&status.as_str()),"ORDER_STATUS")?;format!("/orders?status={status}&limit=200&tournamentId={tid}{suffix}")},
   Endpoint::OrderBook{market_id,depth}=>{self.scoped_market(market_id)?;need(*depth==20||*depth==200,"BOOK_DEPTH")?;format!("/markets/{market_id}/orderbook?tournamentId={tid}&depth={depth}")},
   Endpoint::Market{market_id}=>{self.scoped_market(market_id)?;format!("/markets/{market_id}?tournamentId={tid}")},
   Endpoint::Nodes{market_id}=>{self.scoped_market(market_id)?;format!("/markets/{market_id}/nodes?tournamentId={tid}")},
   Endpoint::Prices{exchange_ids}=>{need(!exchange_ids.is_empty()&&exchange_ids.len()<=100&&exchange_ids.iter().collect::<BTreeSet<_>>().len()==exchange_ids.len()&&exchange_ids.iter().all(|e|self.config.scope.exchanges.contains_key(e)),"PRICE_SCOPE")?;format!("/exchanges/prices?ids={}&tournamentId={tid}",exchange_ids.join(","))},
   Endpoint::OwnedOrder{order_id}=>{self.owned(order_id)?;format!("/orders/{order_id}")},
   Endpoint::OwnedFills{order_id}=>{self.owned(order_id)?;format!("/orders/{order_id}/fills?limit=200{suffix}")},
   Endpoint::History{exchange_id,to}=>{need(self.config.scope.exchanges.contains_key(exchange_id),"EXCHANGE_SCOPE")?;let t=DateTime::parse_from_rfc3339(to).map_err(|_|fail("HISTORY_CLOCK"))?;need(t.timestamp_millis()>=0&&t.timestamp_millis()%60000==0&&t.timestamp_millis() as u64<=self.clock.now_ms(),"HISTORY_CLOCK")?;format!("/exchanges/{exchange_id}/price-history?tournamentId={tid}&resolution=1m&limit=500&to={}",percent(to))},
   Endpoint::Trades{exchange_id,from,to}=>{need(self.config.scope.exchanges.contains_key(exchange_id),"EXCHANGE_SCOPE")?;let start=parse_clock(&json!(from)).ok_or_else(||fail("TRADE_WINDOW"))?;let end=parse_clock(&json!(to)).ok_or_else(||fail("TRADE_WINDOW"))?;need(start<end&&end<=self.clock.now_ms(),"TRADE_WINDOW")?;format!("/exchanges/{exchange_id}/trades?tournamentId={tid}&from={}&to={}&limit=200{suffix}",percent(from),percent(to))},
   Endpoint::PortfolioFills=>format!("/tournaments/{slug}/portfolio/fills?limit=100{suffix}"),Endpoint::Pnl=>format!("/tournaments/{slug}/portfolio/pnl?period=day"),
  })
 }
 fn coverage(&self,value:&Value,required:bool)->Result<()> {if value.is_null()&&!required{return Ok(());}need(value.is_object()&&value["complete"]==true&&(value["projectedThroughSequence"].is_null()||value["projectedThroughSequence"].as_u64().is_some()),"INCOMPLETE_COVERAGE")}
 fn validate_order(&self,v:&Value)->Result<()> {
  let id=id_value(&v["id"]);need(id.is_some()&&v["tournamentId"]==self.config.scope.tournament_id&&v["exchangeId"].as_str().is_some_and(|e|self.config.scope.exchanges.contains_key(e)||self.full_account_observation&&numeric(e)),"RESPONSE_SCOPE_MISMATCH")?;
  need(v["side"].as_str().is_some_and(|s|["yes","no"].contains(&s))&&v["action"].as_str().is_some_and(|s|["buy","sell"].contains(&s))&&v["quantity"].as_u64().is_some()&&v["open"].is_boolean()&&parse_clock(&v["createdAt"]).is_some()&&(v["expirationDate"].is_null()||parse_clock(&v["expirationDate"]).is_some()),"INVALID_OFFICIAL_ORDER")?;
  if !v["priceLimit"].is_null(){let p=prices::official_tick_units(&v["priceLimit"])?;need((0..=1_000_000).contains(&p),"INVALID_ORDER_PRICE")?;}Ok(())
 }
 fn validate(&self,endpoint:&Endpoint,v:&Value)->Result<()> {
  need(v.is_object(),"INVALID_RESPONSE_OBJECT")?;match endpoint {
   Endpoint::Tournament=>{need(v["id"]==self.config.scope.tournament_id&&v["slug"]==self.config.scope.tournament_slug&&v["isPendingEnrolment"].is_boolean()&&v["status"].is_string()&&money_value(&v["myBalance"])? >=0,"TOURNAMENT_SCOPE_MISMATCH")?;},
   Endpoint::Positions=>{let rows=v["positions"].as_array().ok_or_else(||fail("INCOMPLETE_POSITIONS"))?;need(v.get("pagination").is_none(),"POSITION_PAGINATION_UNDOCUMENTED")?;for field in ["totalMarketValue","totalCostBasis","totalUnrealizedPnl"]{need(v["summary"][field].as_f64().is_some_and(|n|n.is_finite()),"INVALID_REPORTED_POSITION_SUMMARY")?;}let mut seen=BTreeSet::new();for row in rows{let e=row["exchangeId"].as_str().ok_or_else(||fail("POSITION_SCOPE"))?;need((self.config.scope.exchanges.get(e).is_some_and(|m|row["marketId"]==*m)||self.full_account_observation&&!self.config.scope.exchanges.contains_key(e)&&numeric(e)&&row["marketId"].as_str().is_some_and(numeric))&&seen.insert(e)&&row["quantity"].as_i64().is_some()&&row["lots"].is_array(),"POSITION_SCOPE")?;}},
   Endpoint::Orders{..}=>{self.coverage(&v["coverage"],false)?;for row in v["data"].as_array().ok_or_else(||fail("INCOMPLETE_ORDERS"))?{self.validate_order(row)?;}pagination(v)?;},
   Endpoint::OwnedOrder{order_id}=>{self.validate_order(v)?;need(id_value(&v["id"]).as_ref()==Some(order_id),"ORDER_ID_MISMATCH")?;},
   Endpoint::OwnedFills{order_id}=>{need(id_value(&v["orderId"]).as_ref()==Some(order_id)&&v["tournamentId"]==self.config.scope.tournament_id&&v["exchangeId"].as_str().is_some_and(|e|self.config.scope.exchanges.contains_key(e)),"FILL_SCOPE")?;self.coverage(&v["coverage"],true)?;pagination(v)?;money_value(&v["totalQuantityFilled"])?;for row in v["data"].as_array().ok_or_else(||fail("FILL_ARRAY"))?{need(id_value(&row["id"]).is_some()&&parse_clock(&row["filledAt"]).is_some()&&row["quantity"].as_i64().is_some_and(|q|q>0&&row["side"]=="yes"||q<0&&row["side"]=="no"),"INVALID_FILL")?;if !row["price"].is_null(){let p=prices::official_tick_units(&row["price"])?;need((0..=1_000_000).contains(&p),"INVALID_FILL_PRICE")?;}}},
   Endpoint::OrderBook{market_id,..}|Endpoint::Market{market_id}=>{
    let contexts=v["contexts"].as_array().ok_or_else(||fail("MARKET_CONTEXT"))?;need(contexts.len()==1&&contexts[0]["type"]=="tournament"&&contexts[0]["tournament"]["id"]==self.config.scope.tournament_id,"MARKET_CONTEXT")?;
    let rows=if matches!(endpoint,Endpoint::OrderBook{..}){contexts[0]["orderbook"]["exchanges"].as_array()}else{need(v["id"]==*market_id,"MARKET_ID")?;v["exchanges"].as_array()}.ok_or_else(||fail("MARKET_EXCHANGES"))?;
    let expected=self.config.scope.exchanges.iter().filter(|(_,m)|*m==market_id).map(|(e,_)|e.as_str()).collect::<BTreeSet<_>>();let mut actual=BTreeSet::new();for row in rows{let e=row["exchangeId"].as_str().or_else(||row["id"].as_str()).ok_or_else(||fail("BOOK_EXCHANGE"))?;need(expected.contains(e)&&actual.insert(e),"BOOK_EXCHANGE_SCOPE")?;if matches!(endpoint,Endpoint::OrderBook{..}){for side in ["bids","asks"]{for level in row[side].as_array().ok_or_else(||fail("BOOK_LEVELS"))?{let p=prices::official_tick_units(&level["price"])?;need((0..=1_000_000).contains(&p)&&money_value(&level["quantity"])? >0,"BOOK_LEVEL")?;}}}}
    need(actual==expected,"INCOMPLETE_BOOK_EXCHANGES")?;
   },
   Endpoint::Nodes{market_id}=>need(v["market_id"]==*market_id&&v["contexts"].as_array().is_some_and(|a|a.len()==1&&a[0]["type"]=="tournament"&&a[0]["tournament"]["id"]==self.config.scope.tournament_id)&&v["root"]["node_type"]=="contract"&&(v["root"]["contract_type"].is_string()||v["root"]["contract_type"].is_null()),"ROOT_SCOPE")?,
   Endpoint::Prices{exchange_ids}=>{let rows=v["data"].as_array().ok_or_else(||fail("PRICE_ARRAY"))?;let mut seen=BTreeSet::new();for row in rows{let e=row["exchangeId"].as_str().ok_or_else(||fail("PRICE_SCOPE"))?;need(exchange_ids.iter().any(|x|x==e)&&seen.insert(e.to_string())&&self.config.scope.exchanges.get(e).is_some_and(|m|row["marketId"]==*m),"PRICE_SCOPE")?;for field in ["bestBid","bestAsk","latestPrice","spread"]{if !row[field].is_null(){let p=money_value(&row[field])?;need((0..=1_000_000).contains(&p),"PRICE_NUMBER")?;}}}for id in v["missingIds"].as_array().ok_or_else(||fail("MISSING_IDS"))?{let e=id.as_str().ok_or_else(||fail("MISSING_IDS"))?;need(exchange_ids.iter().any(|x|x==e)&&seen.insert(e.into()),"MISSING_IDS")?;}need(seen.len()==exchange_ids.len(),"INCOMPLETE_PRICE_RESPONSE")?;},
   Endpoint::History{exchange_id,to}=>{need(v["exchangeId"]==*exchange_id&&self.config.scope.exchanges.get(exchange_id).is_some_and(|m|v["marketId"]==*m)&&v["resolution"]=="1m","HISTORY_SCOPE")?;let end=parse_clock(&v["to"]).ok_or_else(||fail("HISTORY_CLOCK"))?;let start=parse_clock(&v["from"]).ok_or_else(||fail("HISTORY_CLOCK"))?;let requested=DateTime::parse_from_rfc3339(to).map_err(|_|fail("HISTORY_CLOCK"))?.timestamp_millis() as u64;need(start<end&&end<=requested&&end<=self.clock.now_ms()&&end%60000==0,"HISTORY_CLOCK")?;self.coverage(&v["coverage"],false)?;let rows=v["candles"].as_array().ok_or_else(||fail("HISTORY_CANDLES"))?;need(rows.len()<=500,"HISTORY_CANDLES")?;let mut seen=BTreeSet::new();for row in rows{let time=parse_clock(&row["time"]).ok_or_else(||fail("CANDLE_CLOCK"))?;need(time>=start&&time.checked_add(60000).is_some_and(|t|t<=end)&&time%60000==0&&seen.insert(time)&&money_value(&row["volume"])? >=0&&row["tradeCount"].as_u64().is_some(),"CANDLE_CLOCK")?;for field in ["open","high","low","close","vwap"]{if !row[field].is_null(){let p=money_value(&row[field])?;need((0..=1_000_000).contains(&p),"CANDLE_PRICE")?;}}}},
   Endpoint::Trades{exchange_id,from,to}=>{need(v["exchangeId"]==*exchange_id&&self.config.scope.exchanges.get(exchange_id).is_some_and(|m|v["marketId"]==*m)&&parse_clock(&v["from"])==parse_clock(&json!(from))&&parse_clock(&v["to"])==parse_clock(&json!(to)),"TRADE_WINDOW_SCOPE")?;self.coverage(&v["coverage"],true)?;pagination(v)?;let start=parse_clock(&v["from"]).ok_or_else(||fail("TRADE_WINDOW"))?;let end=parse_clock(&v["to"]).ok_or_else(||fail("TRADE_WINDOW"))?;for row in v["data"].as_array().ok_or_else(||fail("TRADE_ARRAY"))?{need(row["id"].as_str().is_some_and(numeric)&&parse_clock(&row["createdAt"]).is_some_and(|t|t>=start&&t<end)&&row["size"].as_u64().is_some_and(|n|n>0)&&row["side"].as_str().is_some_and(|s|s=="YES"||s=="NO"),"INVALID_TRADE")?;prices::official_tick_units(&row["price"])?;}},
   Endpoint::PortfolioFills=>{self.coverage(&v["coverage"],true)?;pagination(v)?;for row in v["data"].as_array().ok_or_else(||fail("PORTFOLIO_FILL_ARRAY"))?{let e=row["exchangeId"].as_str().ok_or_else(||fail("FILL_SCOPE"))?;need((self.config.scope.exchanges.get(e).is_some_and(|m|row["marketId"]==*m)||self.full_account_observation&&!self.config.scope.exchanges.contains_key(e)&&numeric(e)&&row["marketId"].as_str().is_some_and(numeric))&&id_value(&row["id"]).is_some()&&(row["orderId"].is_null()||id_value(&row["orderId"]).is_some())&&parse_clock(&row["filledAt"]).is_some()&&row["quantity"].as_i64().is_some_and(|q|q>0&&row["side"]=="yes"||q<0&&row["side"]=="no"),"INVALID_PORTFOLIO_FILL")?;if !row["price"].is_null(){let p=prices::official_tick_units(&row["price"])?;need((0..=1_000_000).contains(&p),"INVALID_FILL_PRICE")?;}}},
   Endpoint::Pnl=>{need(v["period"]=="day"&&parse_clock(&v["periodEnd"]).is_some()&&(v["periodStart"].is_null()||parse_clock(&v["periodStart"]).is_some()),"PNL_SCOPE")?;for field in ["unrealizedPnl","totalAccountValue","totalHoldingsValue","totalCostBasis"]{money_value(&v[field])?;}if !v["periodPnl"].is_null(){money_value(&v["periodPnl"])?;}},
  }Ok(())
 }
 pub fn request(&mut self,endpoint:Endpoint)->Result<Receipt>{
  if !matches!(endpoint,Endpoint::Orders{..}|Endpoint::OwnedFills{..}|Endpoint::PortfolioFills|Endpoint::Trades{..}){let p=self.path(&endpoint,None)?;return self.perform(RequestSpec{method:"GET".into(),endpoint:p,body:None,body_json:None},None,|s,v|s.validate(&endpoint,v));}
  let mut cursor=None;let mut cursors=BTreeSet::new();let mut identities=BTreeSet::new();let mut merged=vec![];let mut first_time=None;let mut first_totals=None;let mut minimum_projection=Some(u64::MAX);
  for _ in 0..self.config.maximum_pages{let path=self.path(&endpoint,cursor.as_deref())?;let mut receipt=self.perform(RequestSpec{method:"GET".into(),endpoint:path,body:None,body_json:None},None,|s,v|s.validate(&endpoint,v))?;
   minimum_projection=minimum_projection.zip(receipt.data["coverage"]["projectedThroughSequence"].as_u64()).map(|(a,b)|a.min(b));
   if first_time.is_none(){first_time=Some(receipt.request_started_at.clone());first_totals=Some((receipt.data["totalQuantityFilled"].clone(),receipt.data["avgFillPrice"].clone()));}
   if matches!(endpoint,Endpoint::OwnedFills{..}){need(first_totals.as_ref()==Some(&(receipt.data["totalQuantityFilled"].clone(),receipt.data["avgFillPrice"].clone())),"INCONSISTENT_FILL_TOTALS")?;}
   let rows=receipt.data["data"].as_array().unwrap();for row in rows{let key=if matches!(endpoint,Endpoint::Orders{..}){id_value(&row["id"]).unwrap()}else{serde_json::to_string(&json!([row["id"],row["filledAt"],row["side"],row["quantity"],row["price"]])).unwrap()};need(identities.insert(key),"DUPLICATE_PAGINATED_ROW")?;merged.push(row.clone());}
   let pg=&receipt.data["pagination"];if pg["hasMore"]==false{need(pg["nextCursor"].is_null(),"INVALID_FINAL_CURSOR")?;if receipt.data["coverage"].is_object(){receipt.data["coverage"]["projectedThroughSequence"]=minimum_projection.map_or(Value::Null,|n|json!(n));}receipt.data["data"]=Value::Array(merged);receipt.data["pagination"]=json!({"limit":receipt.data["pagination"]["limit"],"hasMore":false,"nextCursor":null});receipt.request_started_at=first_time.unwrap();receipt.endpoint=self.path(&endpoint,None)?;return Ok(receipt);}
   let next=pg["nextCursor"].as_str().ok_or_else(||fail("MISSING_CURSOR"))?;need(!rows.is_empty()&&cursors.insert(next.to_string()),"REPEATED_CURSOR")?;cursor=Some(next.to_string());
  }Err(fail("PAGINATION_LIMIT_EXCEEDED"))
 }
 pub fn place(&mut self,order:Order,permit:Permit)->Result<Receipt>{
  need(order.tournament_id==self.config.scope.tournament_id&&self.config.scope.exchanges.contains_key(&order.exchange_id),"ORDER_SCOPE_MISMATCH")?;need(["yes","no"].contains(&order.side.as_str())&&["buy","sell"].contains(&order.action.as_str())&&order.quantity>0&&order.quantity<=2_147_483_647&&order.quantity<=permit.maximum_quantity&&!order.idempotency_key.is_empty()&&order.idempotency_key.len()<=255&&!order.idempotency_key.chars().any(char::is_control),"INVALID_ORDER")?;
  let price=decimal_units(&order.price)?;need((5000..=995000).contains(&price)&&price%5000==0&&price.checked_mul(order.quantity as i128).is_some_and(|c|c<=decimal_units(&permit.maximum_cost).unwrap_or(-1)),"ORDER_TICK_OR_COST")?;
  let expiry=DateTime::parse_from_rfc3339(&order.expiration_date).map_err(|_|fail("ORDER_EXPIRY"))?.timestamp_millis();need(expiry>0&&expiry as u64>self.clock.now_ms(),"ORDER_EXPIRY")?;
  let wire_price=format!("{}.{:06}",price/1_000_000,price%1_000_000);
  let body=json!({"idempotencyKey":order.idempotency_key,"exchangeId":order.exchange_id,"side":order.side,"action":order.action,"quantity":order.quantity,"price":serde_json::Number::from_string_unchecked(wire_price),"expirationDate":order.expiration_date,"tournamentId":order.tournament_id});let body_json=serde_json::to_string(&body).unwrap();
  self.perform(RequestSpec{method:"POST".into(),endpoint:"/orders".into(),body:Some(body),body_json:Some(body_json)},Some(&permit),|_,v|{
   need(v["exchangeId"]==order.exchange_id&&v["side"]==order.side&&v["action"]==order.action&&v["open"].is_boolean()&&(v["orderId"].is_null()||id_value(&v["orderId"]).is_some()),"ORDER_ACK_SCOPE_OR_CANONICAL_CHANGE")?;
   let quantity=v["quantity"].as_u64().ok_or_else(||fail("ORDER_ACK_QUANTITY"))?;need(quantity>0&&quantity<=order.quantity as u64&&prices::official_tick_units(&v["price"])?==price&&money_value(&v["quantityTraded"])? >=0&&money_value(&v["quantityTraded"])? <=quantity as i128*1_000_000&&money_value(&v["remainingQuantity"])? >=0&&money_value(&v["quantityTraded"])?+money_value(&v["remainingQuantity"])?==quantity as i128*1_000_000&&v["all"].is_object(),"ORDER_ACK_ECONOMICS")?;need(v["totalCost"].as_f64().is_some_and(|n|n.is_finite()&&n>=0.0),"INVALID_ACK_REPORTED_NOTIONAL")?;Ok(())
  })
 }
 pub fn cancel(&mut self,order_id:String,permit:Permit)->Result<Receipt>{self.owned(&order_id)?;self.perform(RequestSpec{method:"DELETE".into(),endpoint:format!("/orders/{order_id}"),body:None,body_json:None},Some(&permit),|s,v|need(id_value(&v["orderId"]).as_ref()==Some(&order_id)&&v["tournamentId"]==s.config.scope.tournament_id,"CANCEL_ACK_SCOPE"))}
 pub fn reconcile(&mut self)->Result<AccountReconciliation>{let started=utc(self.clock.now_ms())?;let tournament=self.request(Endpoint::Tournament)?;let positions=self.request(Endpoint::Positions)?;let orders=self.request(Endpoint::Orders{status:"all".into()})?;let cash=tournament.data["myBalance"].to_string();decimal_units(&cash)?;Ok(AccountReconciliation{tournament,positions,orders,exact_cash:cash,started_at:started,completed_at:utc(self.clock.now_ms())?,adopts_holdings:false})}
 pub fn reconcile_unknown(&mut self,binding:Value)->Result<()> {self.authority.reconcile_unknown(&self.config.scope,&binding,self.clock.now_ms())?;self.command(json!({"op":"reconcile","binding":binding,"reconciled":true,"lease_valid":true,"permit_valid":true}))?;Ok(())}
}
fn percent(text:&str)->String {text.as_bytes().iter().map(|b|if b.is_ascii_alphanumeric()||b"-_.~".contains(b){(*b as char).to_string()}else{format!("%{b:02X}")}).collect()}
fn parse_clock(v:&Value)->Option<u64>{v.as_str().and_then(|s|DateTime::parse_from_rfc3339(s).ok()).and_then(|d|u64::try_from(d.timestamp_millis()).ok())}
fn id_value(v:&Value)->Option<String>{v.as_str().filter(|s|numeric(s)).map(str::to_string).or_else(||v.as_u64().filter(|n|*n>0&&*n<=9_007_199_254_740_991).map(|n|n.to_string()))}
fn pagination(v:&Value)->Result<()> {let p=&v["pagination"];need(p.is_object()&&p["hasMore"].is_boolean()&&p["limit"].as_u64().is_some_and(|n|(1..=200).contains(&n))&&(p["nextCursor"].is_null()||p["nextCursor"].is_string()),"INCOMPLETE_PAGINATION")}
fn safe_api_code(code:&str)->Option<String>{["MISSING_API_KEY","INVALID_API_KEY","API_KEY_REVOKED","API_KEY_EXPIRED","UNAUTHORIZED","INSUFFICIENT_SCOPES","ACCOUNT_BANNED","RESIDENCE_UPDATE_REQUIRED","FORBIDDEN","TERMS_NOT_ACKNOWLEDGED","RATE_LIMITED","INVALID_JSON","INVALID_ID","INVALID_CURSOR","VALIDATION_ERROR","ORDER_CONFLICT","NOT_FOUND","CONFLICT","REQUEST_IN_FLIGHT","TERMINAL_ENGINE_SCOPE","RELATIONSHIP_VIOLATION","DATABASE_ERROR","INTERNAL_ERROR","ORDER_STATUS_UNKNOWN","TX_CONFLICT","SERVICE_UNAVAILABLE","IDEMPOTENCY_KEY_REUSED","IDEMPOTENCY_CONFLICT"].contains(&code).then(||code.into())}
#[cfg(test)]mod tests;
