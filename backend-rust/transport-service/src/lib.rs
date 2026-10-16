pub mod admission;
pub mod credentials;
pub mod prices;
pub mod relationships;
pub mod multi_leg;
pub mod multi_leg_portfolio;
pub mod multi_leg_first_call;

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
 Account,Collateral,Relationships{market_id:Option<String>,exchange_id:Option<String>,relationship_type:Option<relationships::RelationshipType>,depth:u8},RelationshipGraph{market_id:String,depth:u8},RelationshipConstraints{market_id:Option<String>,relationship_id:Option<String>,violations_only:bool,min_violation:Option<String>},
 Tournament,Positions,Orders{status:String},OrderBook{market_id:String,depth:u16},Market{market_id:String},Nodes{market_id:String},Prices{exchange_ids:Vec<String>},OwnedOrder{order_id:String},OwnedFills{order_id:String},History{exchange_id:String,to:String},PortfolioFills,Settlements,Pnl,Trades{exchange_id:String,from:String,to:String},
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
struct LocalNotSentCapability {request:String,binding:admission::ImmutableBinding,minimum_clock:u64,sequence:u64,previous:String}
pub struct Transport {config:Config,credentials:Box<dyn CredentialProvider>,clock:Box<dyn Clock>,authority:Box<dyn Authority>,client:Client,base:String,queue:admission::AccountAdmission,request_sequence:u64,poisoned:bool,full_account_observation:bool,last_basket_response:Option<multi_leg::ResponseEvidence>,local_not_sent:Option<LocalNotSentCapability>,first_call_active:Option<multi_leg_first_call::Ticket>,first_call_proof:Option<(multi_leg_first_call::Proof,u64,String)>}
impl Transport {
 /// Observation never expands order placement or owned-order permissions.
 pub fn with_full_account_observation(mut self)->Self{self.local_not_sent=None;self.first_call_proof=None;self.full_account_observation=true;self}
 pub fn new_with_read_budget(config:Config,credentials:Box<dyn CredentialProvider>,clock:Box<dyn Clock>,authority:Box<dyn Authority>,cap:usize,gap_ms:u64)->Result<Self>{let mut t=Self::build(config,credentials,clock,authority,OFFICIAL_API.into())?;t.queue.configure_reads(cap,gap_ms).map_err(|e|fail(&e))?;Ok(t)}
 pub fn new(config:Config,credentials:Box<dyn CredentialProvider>,clock:Box<dyn Clock>,authority:Box<dyn Authority>)->Result<Self>{Self::build(config,credentials,clock,authority,OFFICIAL_API.into())}
 #[cfg(any(test,feature="offline-fixtures"))]
 pub fn new_for_mock(config:Config,credentials:Box<dyn CredentialProvider>,clock:Box<dyn Clock>,authority:Box<dyn Authority>,base:String)->Result<Self>{let url=reqwest::Url::parse(&base).map_err(|_|fail("MOCK_URL"))?;need(url.scheme()=="http"&&matches!(url.host_str(),Some("127.0.0.1"|"localhost"|"::1"))&&url.path()=="/api/v1"&&url.query().is_none()&&url.fragment().is_none(),"LOOPBACK_FIXTURE_ONLY")?;Self::build(config,credentials,clock,authority,base)}
 fn build(config:Config,credentials:Box<dyn CredentialProvider>,clock:Box<dyn Clock>,authority:Box<dyn Authority>,base:String)->Result<Self>{
  need(config.queue_dir.is_absolute()&&!config.account_scope.is_empty()&&config.maximum_pages>0&&config.maximum_pages<=1000,"INVALID_CONFIGURATION")?;
  let scope=&config.scope;need(scope.tournament_id.len()==36&&scope.tournament_id.bytes().all(|b|b.is_ascii_hexdigit()||b==b'-')&&!scope.tournament_slug.is_empty()&&scope.tournament_slug.bytes().all(|b|b.is_ascii_lowercase()||b.is_ascii_digit()||b==b'-')&&!scope.exchanges.is_empty()&&scope.exchanges.iter().all(|(e,m)|numeric(e)&&numeric(m)),"INVALID_SCOPE")?;
  let token=format!("{}-{}",std::process::id(),clock.now_ms());let queue=admission::AccountAdmission::open(&config.queue_dir,&config.account_scope,&token).map_err(|e|fail(&e))?;
  let client=Client::builder().no_proxy().redirect(Policy::none()).timeout(Duration::from_secs(20)).connect_timeout(Duration::from_secs(10)).build().map_err(|_|fail("HTTP_CLIENT_UNAVAILABLE"))?;
  Ok(Self{config,credentials,clock,authority,client,base,queue,request_sequence:0,poisoned:false,full_account_observation:false,last_basket_response:None,local_not_sent:None,first_call_active:None,first_call_proof:None})
 }
 fn command(&mut self,mut c:Value)->Result<Value>{self.local_not_sent=None;self.first_call_proof=None;if c["op"]=="admit"{if let Some(t)=self.first_call_active.as_mut(){t.disqualify();}}c["scope"]=json!(self.config.account_scope);c["now"]=json!(self.clock.now_ms());self.queue.command_with_maintenance(c).map_err(|e|fail(&e))}
 fn guard(&self,spec:&RequestSpec,permit:Option<&Permit>,network:bool)->Result<()> {
  need(!self.poisoned&&self.authority.can_read(&self.config.scope),"STOPPED_OR_OWNER_LOST")?;
  if spec.method!="GET"&&!(spec.method=="POST"&&spec.endpoint=="/realtime/token"&&spec.body.is_none()&&spec.body_json.is_none()){need(self.config.writes_enabled,"WRITES_DISABLED")?;let p=permit.ok_or_else(||fail("PERMIT_REQUIRED"))?;need(p.owner=="Codex"&&p.expires_at_ms>self.clock.now_ms()&&!p.run_id.is_empty()&&!p.intent_id.is_empty(),"PERMIT_EXPIRED_OR_INVALID")?;self.authority.authorize(&self.config.scope,spec,p,network,self.clock.now_ms())?;need(self.authority.can_read(&self.config.scope),"STOPPED_OR_OWNER_LOST")?;}
  Ok(())
 }
 fn wait(&mut self,until:u64,spec:&RequestSpec,p:Option<&Permit>)->Result<()> {while self.clock.now_ms()<until{self.guard(spec,p,false)?;let remaining=until-self.clock.now_ms();self.clock.sleep_ms(remaining.min(200))?;}self.guard(spec,p,false)}
 fn perform(&mut self,spec:RequestSpec,permit:Option<&Permit>,validator:impl Fn(&Self,&Value)->Result<()>)->Result<Receipt>{
  self.local_not_sent=None;self.first_call_proof=None;let minimum_clock=self.clock.now_ms();
  self.guard(&spec,permit,false)?;self.request_sequence+=1;let request=format!("request-{}-{}",self.clock.now_ms(),self.request_sequence);let write=spec.method!="GET"&&spec.endpoint!="/realtime/token";
  let binding=if write{let p=permit.unwrap();let key=spec.body.as_ref().and_then(|b|b.get("idempotencyKey")).and_then(Value::as_str).map(str::to_string).unwrap_or_else(||format!("{}:{}:cancel",p.run_id,p.intent_id));Some(json!({"intent":format!("{}:{}",p.run_id,p.intent_id),"key":key,"payload_hash":digest(serde_json::to_string(&json!({"scope":self.config.scope,"spec":spec})).unwrap().as_bytes())}))}else{None};
  // The bounded summary is authoritative for GLOBAL uncertainty. An exact
  // lookup may be empty while another intent is unknown; never infer emptiness
  // from the requested intent. Core admission independently repeats its fences.
  let queue=self.admission_summary()?;
  if write{
   let b=binding.as_ref().unwrap();let intent=b["intent"].as_str().ok_or_else(||fail("INVALID_BINDING"))?;
   let known=self.admission_lookup_intent(intent)?;
   need(known.global_unknown_count==queue.global_unknown_count&&known.has_unknown==queue.has_unknown,"QUEUE_UNKNOWN_SNAPSHOT_CHANGED")?;
  }
  // Let core admission evaluate clock/manual/active/immutable errors in their
  // original order. An SDK precheck must not hide durable clock regression.
  loop{self.guard(&spec,permit,false)?;let c=json!({"op":"admit","request":request,"write":write,"binding":binding,"authorized":write,"lease_valid":write,"permit_valid":write});let r=self.command(c)?;if r["admitted"]==true{if write&&queue.has_unknown{self.poisoned=true;return Err(fail("UNKNOWN_ACCEPTANCE_RECONCILE_FIRST"));}break;}let until=r["wait_until"].as_u64().ok_or_else(||fail("QUEUE_RESPONSE"))?;self.wait(until,&spec,permit)?;}
  let network_guard=self.guard(&spec,permit,true);if let Err(error)=network_guard{return Err(self.complete_pre_network(&spec,&request,binding.as_ref(),minimum_clock,error));}
  // Authentication is loaded only after scoped admission, never at process startup.
  let key=match self.credentials.load(){Ok(k)=>k,Err(e)=>return Err(self.complete_pre_network(&spec,&request,binding.as_ref(),minimum_clock,e))};
  let encoded=Zeroizing::new(format!("Bearer {}",key.as_str()));let mut header=match HeaderValue::from_str(encoded.as_str()){Ok(h)=>h,Err(_)=>return Err(self.complete_pre_network(&spec,&request,binding.as_ref(),minimum_clock,fail("CREDENTIAL_FORMAT")))};header.set_sensitive(true);
  self.command(json!({"op":"network","request":request,"write":write,"binding":binding,"authorized":write,"lease_valid":write,"permit_valid":write}))?;
  let started=self.clock.now_ms();let final_guard=self.guard(&spec,permit,true);
  let mut observed_status=0u16;let mut observed_retry=String::new();
  let call=final_guard.and_then(|_|{
   let method=Method::from_bytes(spec.method.as_bytes()).map_err(|_|fail("HTTP_METHOD"))?;let mut builder=self.client.request(method,self.base.clone()+&spec.endpoint).header(AUTHORIZATION,header);
   if let Some(body)=&spec.body_json{builder=builder.header("Content-Type","application/json").body(body.clone());}
   let mut response=builder.send().map_err(|_|fail("CONNECTION_UNCONFIRMED"))?;let status=response.status().as_u16();let retry=response.headers().get("Retry-After").and_then(|v|v.to_str().ok()).unwrap_or("").to_string();observed_status=status;observed_retry=retry.clone();let mut bytes=Vec::new();
   let body_read=response.by_ref().take(MAX_RESPONSE+1).read_to_end(&mut bytes);
   if write&&spec.endpoint=="/orders/multi-leg" {self.last_basket_response=Some(multi_leg::record_response(&self.config.queue_dir,&request,&spec,status,&utc(started)?,&utc(self.clock.now_ms())?,&bytes,body_read.is_ok()&&bytes.len() as u64<=MAX_RESPONSE)?);}
   body_read.map_err(|_|fail("BODY_UNCONFIRMED"))?;need(bytes.len() as u64<=MAX_RESPONSE,"RESPONSE_TOO_LARGE")?;let data:Value=serde_json::from_slice(&bytes).map_err(|_|fail("INVALID_RESPONSE_JSON"))?;Ok((status,retry,data))
  });
  let (outcome,status,retry,api_code)=match &call{Ok((status,retry,data))=>{let code=data.pointer("/error/code").and_then(Value::as_str).unwrap_or("");let basket_unknown=spec.endpoint=="/orders/multi-leg"&&!(200..300).contains(status)&&multi_leg::zero_execution(*status,data,spec.body.as_ref().and_then(|b|b["legs"].as_array()).map_or(0,Vec::len)).is_none();let unknown=write&&(basket_unknown||*status>=500||*status==409&&["REQUEST_IN_FLIGHT","ORDER_STATUS_UNKNOWN"].contains(&code));(if unknown{"unknown"}else if (200..300).contains(status){"success"}else{"rejected"},*status,retry.clone(),code.to_string())},Err(_)=>(if write{"unknown"}else{"rejected"},observed_status,observed_retry,String::new())};
  let returned_order_id=call.as_ref().ok().and_then(|(_,_,d)|id_value(&d["orderId"]));
  let validated=call.and_then(|(status,retry,data)|{if !(200..300).contains(&status){let mut e=fail("OFFICIAL_REQUEST_REJECTED");e.status=Some(status);e.may_have_executed=outcome=="unknown";let _=retry;return Err(e);}if write{self.record_write_response(&request,&spec,status,started,&data)?;}validator(self,&data)?;Ok((status,data))});
  let final_outcome=if write&&validated.is_err()&&status<300{"unknown"}else{outcome};
  let completion=self.command(json!({"op":"complete","request":request,"outcome":final_outcome,"status":status,"retry_after":retry,"api_code":if api_code=="REQUEST_IN_FLIGHT"{"REQUEST_IN_FLIGHT"}else{""}}));
  if completion.is_err(){self.poisoned=true;let mut e=fail("COMPLETION_DURABILITY_UNCERTAIN");e.may_have_executed=write;return Err(e);}
  let retry_ms=completion.as_ref().ok().and_then(|v|v["next"].as_u64()).unwrap_or(0).saturating_sub(self.clock.now_ms());
  match validated{Ok((status,data))=>Ok(Receipt{endpoint:spec.endpoint,request_started_at:utc(started)?,received_at:utc(self.clock.now_ms())?,status,data}),Err(mut e)=>{if status!=0{e.status=Some(status)}e.may_have_executed|=write&&final_outcome=="unknown";e.order_id=returned_order_id;e.retry_after_ms=retry_ms;e.api_code=safe_api_code(&api_code);Err(e)}}
 }
 /// Finish a known pre-network failure durably. Failure to record completion
 /// poisons this controller; the original error cannot hide queue uncertainty.
 fn complete_pre_network(&mut self,spec:&RequestSpec,request:&str,binding:Option<&Value>,minimum_clock:u64,error:TransportError)->TransportError{
  let completed=self.command(json!({"op":"complete","request":request,"outcome":"not_sent"}));
  if !completed.as_ref().is_ok_and(|v|v["completed"]==true&&v["manual_review"]==false){self.poisoned=true;return TransportError{code:"LOCAL_NOT_SENT_DURABILITY_UNCERTAIN".into(),..error}}
  if spec.method=="POST"&&spec.endpoint=="/orders/multi-leg"&&!error.may_have_executed&&error.status.is_none()&&error.order_id.is_none()&&error.api_code.is_none(){
   let capture=(||->Result<LocalNotSentCapability>{let summary=self.package_queue_quiet()?;need(summary.schema_version==1,"LOCAL_NOT_SENT_V1_REQUIRED")?;let binding:admission::ImmutableBinding=serde_json::from_value(binding.cloned().ok_or_else(||fail("LOCAL_NOT_SENT_BINDING"))?).map_err(|_|fail("LOCAL_NOT_SENT_BINDING"))?;Ok(LocalNotSentCapability{request:request.into(),binding,minimum_clock,sequence:summary.sequence,previous:summary.previous})})();
   match capture{Ok(capability)=>self.local_not_sent=Some(capability),Err(_)=>{self.local_not_sent=None;self.first_call_proof=None;}}
  }
  error
 }
 /// Read-only context: no account paths or credentials are exposed.
 pub fn account_scope(&self)->&str{&self.config.account_scope}
 pub fn scope(&self)->&Scope{&self.config.scope}
 /// Actual live controller lease/poison and global uncertainty fences. This
 /// neither advances clocks nor appends a status transition.
 pub fn package_queue_quiet(&self)->Result<admission::AdmissionSummary>{
  need(!self.poisoned&&self.authority.can_read(&self.config.scope),"STOPPED_OR_OWNER_LOST")?;
  let summary=self.queue.summary().map_err(|e|fail(&e))?;
  need(!summary.has_unknown&&summary.global_unknown_count==0&&summary.state.active.is_none()&&!summary.state.manual_review,"PACKAGE_QUEUE_NOT_QUIET")?;Ok(summary)
 }
 fn verify_bound_local_not_sent(&self,binding:&admission::ImmutableBinding,request:&str,minimum_clock:u64,expected:Option<(u64,&str)>)->Result<admission::VerifiedLocalNotSentV1>{
  let before=self.package_queue_quiet()?;need(before.schema_version==1,"LOCAL_NOT_SENT_V1_REQUIRED")?;
  if let Some((sequence,previous))=expected{need(before.sequence==sequence&&before.previous==previous,"LOCAL_NOT_SENT_CAPABILITY_STALE")?;}
  let known=self.queue.lookup_intent(&binding.intent).map_err(|e|fail(&e))?;
  need(known.binding.as_ref()==Some(binding)&&known.unknown_binding.is_none()&&!known.has_unknown&&known.global_unknown_count==0,"LOCAL_NOT_SENT_BINDING")?;
  let proof=if expected.is_some(){admission::verify_local_not_sent_v1(&self.config.queue_dir,&self.config.account_scope,binding,request,minimum_clock)}else{admission::verify_retained_local_not_sent_v1(&self.config.queue_dir,&self.config.account_scope,binding,request,minimum_clock)}.map_err(|e|fail(&e))?;
  let source=proof.evidence();let current=source.get("currentTailRecord").unwrap_or(&source["tailRecord"]);need(current["sequence"]==before.sequence&&current["hash"]==before.previous,"LOCAL_NOT_SENT_SOURCE_CONTROLLER_CHANGED")?;
  let after=self.package_queue_quiet()?;need(after.sequence==before.sequence&&after.previous==before.previous,"LOCAL_NOT_SENT_SOURCE_CONTROLLER_CHANGED")?;Ok(proof)
 }
 /// Consume only the most recent failed multi-leg call's durable pre-network
 /// completion. Any intervening operation/queue mutation invalidates it. No
 /// caller path, request identifier, fabricated response or retry is accepted.
 pub fn take_local_not_sent_proof(&mut self)->Result<admission::VerifiedLocalNotSentV1>{
  let cap=self.local_not_sent.take().ok_or_else(||fail("LOCAL_NOT_SENT_CAPABILITY_UNAVAILABLE"))?;
  self.verify_bound_local_not_sent(&cap.binding,&cap.request,cap.minimum_clock,Some((cap.sequence,&cap.previous)))
 }
 /// Read-only crash recovery ONLY for an independently verified durable
 /// package-dispatch not_sent record. This validates the retained original
 /// transition through this live controller's own path/scope; it grants no
 /// attempt token or retry. A missing/compacted witness stays unresolved.
 pub fn verify_retained_local_not_sent(&self,binding:&admission::ImmutableBinding,request:&str,minimum_clock:u64)->Result<admission::VerifiedLocalNotSentV1>{
  self.verify_bound_local_not_sent(binding,request,minimum_clock,None)
 }
 fn record_write_response(&self,request:&str,spec:&RequestSpec,status:u16,started:u64,data:&Value)->Result<()> {
  use std::io::Write;
  let mut options=std::fs::OpenOptions::new();options.write(true).create_new(true);
  #[cfg(unix)]{use std::os::unix::fs::OpenOptionsExt;options.mode(0o600);}
  let result=(||->std::io::Result<()>{let mut file=options.open(self.config.queue_dir.join(format!("write-response-{request}.json")))?;let value=json!({"endpoint":spec.endpoint,"requestStartedAt":utc(started).map_err(std::io::Error::other)?,"receivedAt":utc(self.clock.now_ms()).map_err(std::io::Error::other)?,"status":status,"data":data});file.write_all(&serde_json::to_vec(&value)?)?;file.sync_all()?;#[cfg(unix)]std::fs::File::open(&self.config.queue_dir)?.sync_all()?;Ok(())})();
  result.map_err(|_|fail("WRITE_RESPONSE_EVIDENCE_DURABILITY"))
 }
 pub fn compact(&mut self)->Result<Value>{self.local_not_sent=None;self.first_call_proof=None;need(self.authority.can_read(&self.config.scope),"STOPPED_OR_OWNER_LOST")?;self.command(json!({"op":"compact"}))}
 /// Short-lived authentication material stays in memory; callers must not log it.
 /// Charged to account reads; 24 order writes leave headroom for token issuance.
 pub fn realtime_token(&mut self)->Result<Value>{
  let r=self.perform(RequestSpec{method:"POST".into(),endpoint:"/realtime/token".into(),body:None,body_json:None},None,|s,v|{
   for k in ["token","anonKey","supabaseUrl"]{need(v[k].as_str().is_some_and(|x|!x.is_empty()&&x.len()<=32768&&!x.contains(['\r','\n'])),"REALTIME_AUTH_FORMAT")?;}
   need(parse_clock(&v["expiresAt"]).is_some_and(|t|t>s.clock.now_ms()+60000),"REALTIME_AUTH_EXPIRY")
  })?;Ok(r.data)
 }
 /// Legacy full-state diagnostic. Explicitly migrated v2 queues require typed
 /// consumers; returning a summary in this shape would fail old unknown guards.
 pub fn admission_state(&mut self)->Result<Value>{self.local_not_sent=None;self.first_call_proof=None;need(!self.queue.is_v2(),"V2_TYPED_SUMMARY_REQUIRED")?;self.command(json!({"op":"status"}))}
 /// Bounded snapshot of the last durable operation; querying does not advance
 /// its clock, prune rate samples or append another queue transition.
 pub fn admission_summary(&self)->Result<admission::AdmissionSummary>{self.queue.summary().map_err(|e|fail(&e))}
 pub fn admission_lookup_intent(&self,intent:&str)->Result<admission::BindingLookup>{self.queue.lookup_intent(intent).map_err(|e|fail(&e))}
 /// Explicit O(N) full-state diagnostic on either selected queue version.
 pub fn admission_diagnostic_state(&mut self)->Result<Value>{self.command(json!({"op":"diagnostic_status"}))}
 pub fn reserve_legacy_migration_boundary(&mut self,legacy_next:u64,manual_review:bool)->Result<()> {
  self.local_not_sent=None;self.first_call_proof=None;
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
   Endpoint::Account=>format!("/account?tournamentId={tid}"),
   Endpoint::Collateral=>format!("/portfolio/collateral?tournamentId={tid}"),
   Endpoint::Relationships{market_id,exchange_id,relationship_type,depth}=>{
    need(market_id.is_none()||exchange_id.is_none(),"RELATIONSHIP_FILTER_CONFLICT")?;need((1..=4).contains(depth),"RELATIONSHIP_DEPTH")?;
    let mut path=format!("/relationships?tournamentId={tid}&limit=200&depth={depth}{suffix}");
    if let Some(m)=market_id {self.scoped_market(m)?;path.push_str(&format!("&marketId={m}"));}
    if let Some(e)=exchange_id {need(self.config.scope.exchanges.contains_key(e),"RELATIONSHIP_EXCHANGE_SCOPE")?;path.push_str(&format!("&exchangeId={e}"));}
    if let Some(kind)=relationship_type {path.push_str(&format!("&type={}",kind.wire()));}path
   },
   Endpoint::RelationshipGraph{market_id,depth}=>{self.scoped_market(market_id)?;need((1..=4).contains(depth),"RELATIONSHIP_DEPTH")?;format!("/relationships/graph?marketId={market_id}&depth={depth}&tournamentId={tid}")},
   Endpoint::RelationshipConstraints{market_id,relationship_id,violations_only,min_violation}=>{
    let mut path=format!("/relationships/constraints?tournamentId={tid}&violationsOnly={violations_only}");
    if let Some(m)=market_id {self.scoped_market(m)?;path.push_str(&format!("&marketId={m}"));}
    if let Some(id)=relationship_id {need(relationships::uuid(id),"RELATIONSHIP_ID")?;path.push_str(&format!("&relationshipId={id}"));}
    if let Some(min)=min_violation {let n=decimal_units(min)?;need((0..=1_000_000).contains(&n),"RELATIONSHIP_MIN_VIOLATION")?;path.push_str(&format!("&minViolation={}",percent(min)));}path
   },
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
   Endpoint::Settlements=>format!("/tournaments/{slug}/portfolio/settlements?limit=200{suffix}"),Endpoint::PortfolioFills=>format!("/tournaments/{slug}/portfolio/fills?limit=100{suffix}"),Endpoint::Pnl=>format!("/tournaments/{slug}/portfolio/pnl?period=day"),
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
   Endpoint::Account=>{relationships::parse_buying_power(v,&self.config.scope)?;},
   Endpoint::Collateral=>{relationships::parse_collateral(v,&self.config.scope)?;},
   Endpoint::Relationships{market_id,exchange_id,relationship_type,..}=>{
    let list=relationships::parse_list(v,&self.config.scope)?;need(list.data.len()<=200,"RELATIONSHIP_PAGE_LIMIT")?;
    for row in &list.data {need(market_id.as_ref().is_none_or(|m|row.nodes.iter().any(|n|n.market_id.as_ref()==Some(m)))&&exchange_id.as_ref().is_none_or(|e|row.nodes.iter().any(|n|&n.exchange_id==e))&&relationship_type.is_none_or(|t|row.r#type==t),"RELATIONSHIP_FILTER_RESPONSE")?;}
   },
   Endpoint::RelationshipGraph{market_id,..}=>{let graph=relationships::parse_graph(v,&self.config.scope)?;need(graph.relationships.is_empty()||graph.nodes.iter().any(|n|n.market_id.as_ref()==Some(market_id)),"RELATIONSHIP_GRAPH_SEED")?;},
   Endpoint::RelationshipConstraints{relationship_id,..}=>{relationships::validate_constraints(v,&self.config.scope)?;if let Some(id)=relationship_id {need(v["data"].as_array().unwrap().iter().all(|r|r["relationshipId"]==*id),"RELATIONSHIP_CONSTRAINT_FILTER")?;}},
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
   // Primary OpenAPI TournamentSettlementEntry: nullable metadata is retained,
   // correction payouts may be signed, and shares sign is NOT outcome direction.
   // This caller/tournament account feed includes settlements outside the active
   // market universe; such cash must never disappear through a scope filter.
   Endpoint::Settlements=>{self.coverage(&v["coverage"],true)?;pagination(v)?;let rows=v["data"].as_array().ok_or_else(||fail("SETTLEMENT_ARRAY"))?;need(rows.len()<=200,"SETTLEMENT_PAGE_BOUND")?;for r in rows{
    let id=r["id"].as_str().ok_or_else(||fail("SETTLEMENT_ID"))?;need(relationships::uuid(id)||id.strip_prefix("engine-").is_some_and(|n|numeric(n)&&n.parse::<u64>().is_ok()),"SETTLEMENT_ID")?;
    need(r.get("type").is_some_and(|t|t.is_null()||t.as_str().is_some_and(|t|["win","loss","refund","correction"].contains(&t))),"SETTLEMENT_TYPE")?;
    for k in ["createdAt","marketId","marketTitle","exchangeId","settlementOption","settlementOutcome"]{need(r.get(k).is_some_and(|x|x.is_null()||x.is_string()),"SETTLEMENT_METADATA")?;}
    need(r["createdAt"].is_null()||parse_clock(&r["createdAt"]).is_some(),"SETTLEMENT_CLOCK")?;
    for k in ["marketId","exchangeId"]{need(r[k].is_null()||r[k].as_str().is_some_and(numeric),"SETTLEMENT_REFERENCE")?;}
    if let Some(ex)=r["exchangeId"].as_str(){need(self.config.scope.exchanges.contains_key(ex)||self.full_account_observation,"SETTLEMENT_FULL_ACCOUNT_REQUIRED")?;}
    if let(Some(ex),Some(m))=(r["exchangeId"].as_str(),r["marketId"].as_str()){need(self.config.scope.exchanges.get(ex).is_none_or(|expected|expected==m),"SETTLEMENT_REFERENCE_SCOPE")?;}
    need(r["payout"].is_number(),"SETTLEMENT_PAYOUT")?;money_value(&r["payout"])?;
    for k in ["shares","avgEntryPrice","costBasis","realizedPnl"]{let n=r.get(k).ok_or_else(||fail("SETTLEMENT_ECONOMICS"))?;need(n.is_null()||n.is_number(),"SETTLEMENT_ECONOMICS")?;if !n.is_null(){money_value(n)?;}}
   }},
   Endpoint::Pnl=>{need(v["period"]=="day"&&parse_clock(&v["periodEnd"]).is_some()&&(v["periodStart"].is_null()||parse_clock(&v["periodStart"]).is_some()),"PNL_SCOPE")?;for field in ["unrealizedPnl","totalAccountValue","totalHoldingsValue","totalCostBasis"]{money_value(&v[field])?;}if !v["periodPnl"].is_null(){money_value(&v["periodPnl"])?;}},
  }Ok(())
 }
 pub fn request(&mut self,endpoint:Endpoint)->Result<Receipt>{
  self.local_not_sent=None;self.first_call_proof=None;
  if !matches!(endpoint,Endpoint::Orders{..}|Endpoint::OwnedFills{..}|Endpoint::PortfolioFills|Endpoint::Settlements|Endpoint::Trades{..}|Endpoint::Relationships{..}){let p=self.path(&endpoint,None)?;return self.perform(RequestSpec{method:"GET".into(),endpoint:p,body:None,body_json:None},None,|s,v|s.validate(&endpoint,v));}
  let mut cursor=None;let mut cursors=BTreeSet::new();let mut identities=BTreeSet::new();let mut merged=vec![];let mut first_time=None;let mut first_totals=None;let mut minimum_projection=Some(u64::MAX);let mut settlement_pages=vec![];let mut settlement_bytes=0usize;
  for _ in 0..self.config.maximum_pages{let path=self.path(&endpoint,cursor.as_deref())?;let mut receipt=self.perform(RequestSpec{method:"GET".into(),endpoint:path,body:None,body_json:None},None,|s,v|s.validate(&endpoint,v))?;
   if matches!(endpoint,Endpoint::Settlements){let original=serde_json::to_value(&receipt).map_err(|_|fail("SETTLEMENT_PROVENANCE"))?;settlement_bytes=settlement_bytes.checked_add(serde_json::to_vec(&original).map_err(|_|fail("SETTLEMENT_PROVENANCE"))?.len()).ok_or_else(||fail("SETTLEMENT_AGGREGATE_BOUND"))?;need(settlement_bytes<=32*1024*1024,"SETTLEMENT_AGGREGATE_BOUND")?;settlement_pages.push(original);}
   minimum_projection=minimum_projection.zip(receipt.data["coverage"]["projectedThroughSequence"].as_u64()).map(|(a,b)|a.min(b));
   if first_time.is_none(){first_time=Some(receipt.request_started_at.clone());first_totals=Some((receipt.data["totalQuantityFilled"].clone(),receipt.data["avgFillPrice"].clone()));}
   if matches!(endpoint,Endpoint::OwnedFills{..}){need(first_totals.as_ref()==Some(&(receipt.data["totalQuantityFilled"].clone(),receipt.data["avgFillPrice"].clone())),"INCONSISTENT_FILL_TOTALS")?;}
   let rows=receipt.data["data"].as_array().unwrap();for row in rows{let key=if matches!(endpoint,Endpoint::Relationships{..}|Endpoint::Settlements){row["id"].as_str().unwrap().to_string()}else if matches!(endpoint,Endpoint::Orders{..}){id_value(&row["id"]).unwrap()}else{serde_json::to_string(&json!([row["id"],row["filledAt"],row["side"],row["quantity"],row["price"]])).unwrap()};need(identities.insert(key),"DUPLICATE_PAGINATED_ROW")?;merged.push(row.clone());}
   let pg=&receipt.data["pagination"];if pg["hasMore"]==false{need(pg["nextCursor"].is_null(),"INVALID_FINAL_CURSOR")?;if !matches!(endpoint,Endpoint::Settlements)&&receipt.data["coverage"].is_object(){receipt.data["coverage"]["projectedThroughSequence"]=minimum_projection.map_or(Value::Null,|n|json!(n));}receipt.data["data"]=Value::Array(merged);receipt.data["pagination"]=json!({"limit":receipt.data["pagination"]["limit"],"hasMore":false,"nextCursor":null});receipt.request_started_at=first_time.unwrap();receipt.endpoint=self.path(&endpoint,None)?;if matches!(endpoint,Endpoint::Settlements){receipt.data["transportProvenance"]=json!({"kind":"original_settlement_pages_v1","snapshotIsolation":"not_documented","pages":settlement_pages});}return Ok(receipt);}
   let next=pg["nextCursor"].as_str().ok_or_else(||fail("MISSING_CURSOR"))?;need(!rows.is_empty()&&cursors.insert(next.to_string()),"REPEATED_CURSOR")?;cursor=Some(next.to_string());
  }Err(fail("PAGINATION_LIMIT_EXCEEDED"))
 }
 pub fn place(&mut self,order:Order,permit:Permit)->Result<Receipt>{
  self.local_not_sent=None;self.first_call_proof=None;
  need(order.tournament_id==self.config.scope.tournament_id&&self.config.scope.exchanges.contains_key(&order.exchange_id),"ORDER_SCOPE_MISMATCH")?;need(["yes","no"].contains(&order.side.as_str())&&["buy","sell"].contains(&order.action.as_str())&&order.quantity>0&&order.quantity<=2_147_483_647&&order.quantity<=permit.maximum_quantity&&!order.idempotency_key.is_empty()&&order.idempotency_key.len()<=255&&!order.idempotency_key.chars().any(char::is_control),"INVALID_ORDER")?;
  let price=decimal_units(&order.price)?;need((5000..=995000).contains(&price)&&price%5000==0&&price.checked_mul(order.quantity as i128).is_some_and(|c|c<=decimal_units(&permit.maximum_cost).unwrap_or(-1)),"ORDER_TICK_OR_COST")?;
  let expiry=DateTime::parse_from_rfc3339(&order.expiration_date).map_err(|_|fail("ORDER_EXPIRY"))?.timestamp_millis();need(expiry>0&&expiry as u64>self.clock.now_ms(),"ORDER_EXPIRY")?;
  let wire_price=format!("{}.{:06}",price/1_000_000,price%1_000_000);
  let body=json!({"idempotencyKey":order.idempotency_key,"exchangeId":order.exchange_id,"side":order.side,"action":order.action,"quantity":order.quantity,"price":serde_json::Number::from_string_unchecked(wire_price),"expirationDate":order.expiration_date,"tournamentId":order.tournament_id});let body_json=serde_json::to_string(&body).unwrap();
  self.perform(RequestSpec{method:"POST".into(),endpoint:"/orders".into(),body:Some(body),body_json:Some(body_json)},Some(&permit),|_,v|{
   need(v["exchangeId"]==order.exchange_id&&v["side"]==order.side&&v["action"]==order.action&&v["open"].is_boolean()&&id_value(&v["orderId"]).is_some(),"ORDER_ACK_SCOPE_OR_CANONICAL_CHANGE")?;
   let quantity=v["quantity"].as_u64().ok_or_else(||fail("ORDER_ACK_QUANTITY"))?;need(quantity>0&&quantity<=order.quantity as u64&&prices::official_tick_units(&v["price"])?==price,"ORDER_ACK_ECONOMICS")?;
   // This receipt binds identity and admission size only. Accounting consumes the
   // independently fetched complete order-fill tape, never these advisory totals.
   // NO quantities may be signed; canonical remaining-size conventions differ.
   need(money_value(&v["quantityTraded"])?.unsigned_abs()<=quantity as u128*1_000_000,"ORDER_ACK_TRADED_BOUND")?;
   if let Some(n)=v.get("remainingQuantity"){need(money_value(n)?.unsigned_abs()<=quantity as u128*1_000_000,"ORDER_ACK_REMAINDER_BOUND")?;}
   if let Some(a)=v.get("all"){need(a.is_object()||a.is_null(),"ORDER_ACK_COLLATERAL_METADATA")?;}
   need(v["totalCost"].as_f64().is_some_and(|n|n.is_finite()&&n>=0.0),"INVALID_ACK_REPORTED_NOTIONAL")?;Ok(())
  })
 }
 /// Full-fill atomic BUY baskets only. The current live Authority rejects this
 /// endpoint until a separately checked package reservation/dispatch exists.
 /// A first-call ticket is permanent, even after local failure. This method
 /// never retries an existing ticket/key. Original pre-admission errors are
 /// eligible only after legacy intent/key checks independently succeeded.
 pub fn place_all_or_none(&mut self,basket:multi_leg::Basket,permit:Permit)->Result<multi_leg::Outcome>{
  self.local_not_sent=None;self.first_call_proof=None;self.first_call_active=None;self.last_basket_response=None;
  let spec=multi_leg::request(&basket,&self.config.scope,&permit)?;
  let ticket=match multi_leg_first_call::Ticket::begin(&self.config.queue_dir,&self.config.account_scope,&self.config.scope,&basket,&permit,&spec,self.clock.now_ms()){
   Ok(ticket)=>ticket,Err(error)=>{self.poisoned=true;return Err(error)}
  };
  self.first_call_active=Some(ticket);
  let eligibility=(||->Result<(u64,String)>{
   need(!self.poisoned,"STOPPED_OR_OWNER_LOST")?;
   let before=self.queue.summary().map_err(|e|fail(&e))?;need(before.schema_version==1,"FIRST_CALL_V1_REQUIRED")?;
   need(!before.has_unknown&&before.state.active.is_none()&&!before.state.manual_review,"PACKAGE_QUEUE_NOT_QUIET")?;
   let binding=multi_leg_first_call::binding(&self.config.scope,&spec,&basket,&permit)?;
   let intent=self.queue.lookup_intent(&binding.intent).map_err(|e|fail(&e))?;
   let key=self.queue.lookup_key(&binding.key).map_err(|e|fail(&e))?;
   need(intent.binding.is_none()&&intent.unknown_binding.is_none()&&!intent.has_unknown&&intent.global_unknown_count==0&&key.is_none(),"FIRST_CALL_PRIOR_BINDING")?;
   let legacy=multi_leg_first_call::legacy_request_path(&self.config.queue_dir,&spec,&basket,&permit)?;
   match std::fs::symlink_metadata(legacy){Err(e)if e.kind()==std::io::ErrorKind::NotFound=>{},_=>return Err(fail("FIRST_CALL_PRIOR_REQUEST_ARTIFACT"))}
   let after=self.queue.summary().map_err(|e|fail(&e))?;need(!after.has_unknown&&after.state.active.is_none()&&!after.state.manual_review&&before.sequence==after.sequence&&before.previous==after.previous,"FIRST_CALL_CONTROLLER_CHANGED")?;Ok((after.sequence,after.previous))
  })();
  let first_call_head=match eligibility{Ok(head)=>head,Err(error)=>{self.first_call_active=None;return Err(error)}};
  self.first_call_active.as_mut().expect("private active ticket").qualify();
  let result=(||->Result<multi_leg::Outcome>{
  self.guard(&spec,Some(&permit),false)?;
  let request_evidence=multi_leg::record_request(&self.config.queue_dir,&spec,&basket,&permit)?;
  let acknowledgment=std::cell::RefCell::new(None);
  let result=self.perform(spec,Some(&permit),|_,value|{*acknowledgment.borrow_mut()=Some(multi_leg::validate_ack(value,&basket)?);Ok(())});
  match result {
   Ok(receipt)=>match (acknowledgment.into_inner(),self.last_basket_response.clone()){
    (Some(acknowledgment),Some(evidence))=>Ok(multi_leg::Outcome::FullFillReported{receipt,acknowledgment,evidence,request_evidence}),
    _=>{self.poisoned=true;let mut error=fail("BASKET_VALIDATED_EVIDENCE_MISSING");error.may_have_executed=true;Ok(multi_leg::Outcome::Unknown{error,evidence:self.last_basket_response.clone(),request_evidence})},
   },
   Err(error)=>{
    if !error.may_have_executed {if let Some(evidence)=self.last_basket_response.clone(){if let Ok(body)=multi_leg::read_evidenced_error(&evidence){if let Some(reason)=multi_leg::zero_execution(evidence.status,&body,basket.legs.len()){return Ok(multi_leg::Outcome::NoExecution{error,reason,evidence,request_evidence});}}}return Err(error);}
    Ok(multi_leg::Outcome::Unknown{error,evidence:self.last_basket_response.clone(),request_evidence})
   }
  }
  })();
  if let Some(ticket)=self.first_call_active.take(){
   if ticket.can_seal(){if let Err(error)=&result{
    match ticket.finish(error,self.clock.now_ms()){
     Ok(proof)=>self.first_call_proof=Some((proof,first_call_head.0,first_call_head.1)),
     Err(_)=>{self.poisoned=true;return Err(TransportError{code:"FIRST_CALL_PROOF_DURABILITY_UNCERTAIN".into(),..error.clone()})}
    }
   }}
  }
  result
 }
 /// Signed all-or-none portfolios. The same physical Authority must verify
 /// covered SELL inventory and the exact full-gross request; no net cash/credit
 /// shortcut, market order, synthetic expiry, retry or new client is created.
 pub fn place_portfolio_all_or_none(&mut self,basket:multi_leg_portfolio::Basket,permit:Permit)->Result<multi_leg_portfolio::Outcome>{
  self.local_not_sent=None;self.first_call_proof=None;self.first_call_active=None;self.last_basket_response=None;
  let spec=multi_leg_portfolio::request_spec(&basket,&self.config.scope,&permit)?;
  let ticket=match multi_leg_first_call::Ticket::begin_portfolio(&self.config.queue_dir,&self.config.account_scope,&self.config.scope,&basket,&permit,&spec,self.clock.now_ms()){
   Ok(ticket)=>ticket,Err(error)=>{self.poisoned=true;return Err(error)}
  };
  self.first_call_active=Some(ticket);
  let eligibility=(||->Result<(u64,String)>{
   need(!self.poisoned,"STOPPED_OR_OWNER_LOST")?;
   let before=self.queue.summary().map_err(|e|fail(&e))?;need(before.schema_version==1,"FIRST_CALL_V1_REQUIRED")?;
   need(!before.has_unknown&&before.state.active.is_none()&&!before.state.manual_review,"PACKAGE_QUEUE_NOT_QUIET")?;
   let binding=multi_leg_first_call::binding_key(&self.config.scope,&spec,&basket.idempotency_key,&permit)?;
   let intent=self.queue.lookup_intent(&binding.intent).map_err(|e|fail(&e))?;
   let key=self.queue.lookup_key(&binding.key).map_err(|e|fail(&e))?;
   need(intent.binding.is_none()&&intent.unknown_binding.is_none()&&!intent.has_unknown&&intent.global_unknown_count==0&&key.is_none(),"FIRST_CALL_PRIOR_BINDING")?;
   let legacy=multi_leg_first_call::legacy_request_value_path(&self.config.queue_dir,&spec,&serde_json::to_value(&basket).map_err(|_|fail("PORTFOLIO_SERIALIZATION"))?,&permit)?;
   match std::fs::symlink_metadata(legacy){Err(e)if e.kind()==std::io::ErrorKind::NotFound=>{},_=>return Err(fail("FIRST_CALL_PRIOR_REQUEST_ARTIFACT"))}
   let after=self.queue.summary().map_err(|e|fail(&e))?;need(!after.has_unknown&&after.state.active.is_none()&&!after.state.manual_review&&before.sequence==after.sequence&&before.previous==after.previous,"FIRST_CALL_CONTROLLER_CHANGED")?;Ok((after.sequence,after.previous))
  })();
  let first_call_head=match eligibility{Ok(head)=>head,Err(error)=>{self.first_call_active=None;return Err(error)}};
  self.first_call_active.as_mut().expect("private active ticket").qualify();
  let result=(||->Result<multi_leg_portfolio::Outcome>{
  self.guard(&spec,Some(&permit),false)?;
  let request_evidence=multi_leg_portfolio::record_request(&self.config.queue_dir,&spec,&basket,&permit)?;
  let acknowledgment=std::cell::RefCell::new(None);
  let result=self.perform(spec,Some(&permit),|_,value|{*acknowledgment.borrow_mut()=Some(multi_leg_portfolio::validate_ack(value,&basket)?);Ok(())});
  match result {
   Ok(receipt)=>match (acknowledgment.into_inner(),self.last_basket_response.clone()){
    (Some(acknowledgment),Some(evidence))=>Ok(multi_leg_portfolio::Outcome::FullFillReported{receipt,acknowledgment,evidence,request_evidence}),
    _=>{self.poisoned=true;let mut error=fail("BASKET_VALIDATED_EVIDENCE_MISSING");error.may_have_executed=true;Ok(multi_leg_portfolio::Outcome::Unknown{error,evidence:self.last_basket_response.clone(),request_evidence})},
   },
   Err(error)=>{
    if !error.may_have_executed {if let Some(evidence)=self.last_basket_response.clone(){if let Ok(body)=multi_leg::read_evidenced_error(&evidence){if let Some(reason)=multi_leg::zero_execution(evidence.status,&body,basket.legs.len()){return Ok(multi_leg_portfolio::Outcome::NoExecution{error,reason,evidence,request_evidence});}}}return Err(error);}
    Ok(multi_leg_portfolio::Outcome::Unknown{error,evidence:self.last_basket_response.clone(),request_evidence})
   }
  }
  })();
  if let Some(ticket)=self.first_call_active.take(){
   if ticket.can_seal(){if let Err(error)=&result{
    match ticket.finish(error,self.clock.now_ms()){
     Ok(proof)=>self.first_call_proof=Some((proof,first_call_head.0,first_call_head.1)),
     Err(_)=>{self.poisoned=true;return Err(TransportError{code:"FIRST_CALL_PROOF_DURABILITY_UNCERTAIN".into(),..error.clone()})}
    }
   }}
  }
  result
 }
 /// Consume the latest original first-call failure's local no-HTTP proof.
 /// No caller paths, synthetic responses, retries or health claims are accepted.
 pub fn take_first_call_no_http_proof(&mut self)->Result<multi_leg_first_call::Proof>{
  let (original,sequence,previous)=self.first_call_proof.take().ok_or_else(||fail("FIRST_CALL_PROOF_UNAVAILABLE"))?;
  let current=self.package_queue_quiet()?;need(current.sequence==sequence&&current.previous==previous,"FIRST_CALL_CAPABILITY_STALE")?;
  let evidence=original.evidence();
  let binding:admission::ImmutableBinding=serde_json::from_value(evidence["ticket"]["binding"].clone()).map_err(|_|fail("FIRST_CALL_BINDING"))?;
  let request=evidence["ticket"]["requestSha256"].as_str().ok_or_else(||fail("FIRST_CALL_REQUEST"))?;
  let recovered=self.verify_retained_first_call_no_http(&binding,request)?;
  need(recovered.evidence()==evidence,"FIRST_CALL_SOURCE_CHANGED")?;Ok(recovered)
 }
 /// Read-only crash recovery for a separately verified durable dispatch record.
 /// The source ticket, key index and terminal must all remain retained. No
 /// attempt token is created, and current controller health is checked apart.
 pub fn verify_retained_first_call_no_http(&self,binding:&admission::ImmutableBinding,request_sha256:&str)->Result<multi_leg_first_call::Proof>{
  let before=self.package_queue_quiet()?;need(before.schema_version==1,"FIRST_CALL_V1_REQUIRED")?;
  let intent=self.queue.lookup_intent(&binding.intent).map_err(|e|fail(&e))?;
  let key=self.queue.lookup_key(&binding.key).map_err(|e|fail(&e))?;
  need(intent.binding.is_none()&&intent.unknown_binding.is_none()&&!intent.has_unknown&&key.is_none(),"FIRST_CALL_PRIOR_BINDING")?;
  let proof=multi_leg_first_call::recover(&self.config.queue_dir,&self.config.account_scope,&self.config.scope,binding,request_sha256)?;
  need(proof.evidence()["terminal"]["completedAtMs"].as_u64().is_some_and(|at|at<=self.clock.now_ms()),"FIRST_CALL_CLOCK")?;
  let after=self.package_queue_quiet()?;need(before.sequence==after.sequence&&before.previous==after.previous,"FIRST_CALL_CONTROLLER_CHANGED")?;Ok(proof)
 }
 /// Positive pre-admission proof from THIS original held SDK controller.
 /// Call immediately after the original local error, before any queue operation.
 /// Does not retry, clear poison/unknown, or authorize any physical request.
 pub fn verify_current_ordinary_pre_admission(&self,spec:&RequestSpec,p:&Permit,at:u64,error:&TransportError)->Result<OrdinaryPreAdmissionProof>{
  need(!self.poisoned&&self.authority.can_read(&self.config.scope),"UNADMITTED_CONTROLLER_STOPPED_OR_POISONED")?;
  let context=unadmitted_context(&self.config,spec,p,at,error)?;let binding:admission::ImmutableBinding=serde_json::from_value(context["binding"].clone()).map_err(|_|fail("UNADMITTED_BINDING"))?;
  let capture=self.queue.capture_current_unadmitted(&binding,at).map_err(|e|fail(&e))?;
  seal_unadmitted_proof(&self.config,spec,p,at,error,context,capture)
 }
 pub fn last_basket_response_evidence(&self)->Option<&multi_leg::ResponseEvidence>{self.last_basket_response.as_ref()}
 pub fn cancel(&mut self,order_id:String,permit:Permit)->Result<Receipt>{self.local_not_sent=None;self.first_call_proof=None;self.owned(&order_id)?;self.perform(RequestSpec{method:"DELETE".into(),endpoint:format!("/orders/{order_id}"),body:None,body_json:None},Some(&permit),|s,v|need(id_value(&v["orderId"]).as_ref()==Some(&order_id)&&v["tournamentId"]==s.config.scope.tournament_id,"CANCEL_ACK_SCOPE"))}
 pub fn reconcile(&mut self)->Result<AccountReconciliation>{self.local_not_sent=None;self.first_call_proof=None;let started=utc(self.clock.now_ms())?;let tournament=self.request(Endpoint::Tournament)?;let positions=self.request(Endpoint::Positions)?;let orders=self.request(Endpoint::Orders{status:"all".into()})?;let cash=tournament.data["myBalance"].to_string();decimal_units(&cash)?;Ok(AccountReconciliation{tournament,positions,orders,exact_cash:cash,started_at:started,completed_at:utc(self.clock.now_ms())?,adopts_holdings:false})}
 pub fn reconcile_unknown(&mut self,binding:Value)->Result<()> {
  self.local_not_sent=None;self.first_call_proof=None;
  let expected:admission::ImmutableBinding=serde_json::from_value(binding.clone()).map_err(|_|fail("INVALID_BINDING"))?;
  let summary=self.admission_summary()?;let exact=self.admission_lookup_intent(&expected.intent)?;
  need(summary.has_unknown&&summary.global_unknown_count==exact.global_unknown_count&&exact.has_unknown&&exact.unknown_binding.as_ref()==Some(&expected),"UNKNOWN_BINDING_MISMATCH")?;
  self.authority.reconcile_unknown(&self.config.scope,&binding,self.clock.now_ms())?;self.command(json!({"op":"reconcile","binding":binding,"reconciled":true,"lease_valid":true,"permit_valid":true}))?;Ok(())
 }
}
fn percent(text:&str)->String {text.as_bytes().iter().map(|b|if b.is_ascii_alphanumeric()||b"-_.~".contains(b){(*b as char).to_string()}else{format!("%{b:02X}")}).collect()}
fn parse_clock(v:&Value)->Option<u64>{v.as_str().and_then(|s|DateTime::parse_from_rfc3339(s).ok()).and_then(|d|u64::try_from(d.timestamp_millis()).ok())}
fn id_value(v:&Value)->Option<String>{v.as_str().filter(|s|numeric(s)).map(str::to_string).or_else(||v.as_u64().filter(|n|*n>0&&*n<=9_007_199_254_740_991).map(|n|n.to_string()))}
fn pagination(v:&Value)->Result<()> {let p=&v["pagination"];need(p.is_object()&&p["hasMore"].is_boolean()&&p["limit"].as_u64().is_some_and(|n|(1..=200).contains(&n))&&(p["nextCursor"].is_null()||p["nextCursor"].is_string()),"INCOMPLETE_PAGINATION")}
fn safe_api_code(code:&str)->Option<String>{["MISSING_API_KEY","INVALID_API_KEY","API_KEY_REVOKED","API_KEY_EXPIRED","UNAUTHORIZED","INSUFFICIENT_SCOPES","ACCOUNT_BANNED","RESIDENCE_UPDATE_REQUIRED","FORBIDDEN","TERMS_NOT_ACKNOWLEDGED","RATE_LIMITED","INVALID_JSON","INVALID_ID","INVALID_CURSOR","VALIDATION_ERROR","ORDER_CONFLICT","NOT_FOUND","CONFLICT","REQUEST_IN_FLIGHT","TERMINAL_ENGINE_SCOPE","RELATIONSHIP_VIOLATION","DATABASE_ERROR","INTERNAL_ERROR","ORDER_STATUS_UNKNOWN","TX_CONFLICT","SERVICE_UNAVAILABLE","IDEMPOTENCY_KEY_REUSED","IDEMPOTENCY_CONFLICT"].contains(&code).then(||code.into())}
/// Historical local no-HTTP evidence only. It grants neither queue health,
/// retry authority nor permission to erase the original attempted intent.
#[derive(Serialize)]
pub struct OrdinaryPreAdmissionProof{evidence:Value}
impl OrdinaryPreAdmissionProof{
 pub fn evidence(&self)->&Value{&self.evidence}
 pub fn validate_binding(&self,spec:&RequestSpec,permit:&Permit,attempt_at:u64,error:&TransportError)->Result<()>{
  need(self.evidence["originalRequestSha256"]==digest(&serde_json::to_vec(spec).map_err(|_|fail("UNADMITTED_SERIALIZE"))?)
   &&self.evidence["permitSha256"]==digest(&serde_json::to_vec(permit).map_err(|_|fail("UNADMITTED_SERIALIZE"))?)&&self.evidence["attemptAtMs"]==attempt_at
   &&self.evidence["originalError"]==serde_json::to_value(error).map_err(|_|fail("UNADMITTED_SERIALIZE"))?,"UNADMITTED_ORIGINAL_BINDING_CHANGED")
 }
}
fn unadmitted_context(config:&Config,spec:&RequestSpec,p:&Permit,at:u64,error:&TransportError)->Result<Value>{
 need(config.queue_dir.is_absolute()&&!config.account_scope.is_empty()&&at>0&&at<=SystemClock.now_ms()&&p.owner=="Codex"&&!p.run_id.is_empty()&&!p.intent_id.is_empty()
  &&!error.may_have_executed&&error.status.is_none()&&error.order_id.is_none()&&error.api_code.is_none()&&error.retry_after_ms==0
  &&error.code=="PERMIT_EXPIRED_OR_INVALID","UNADMITTED_ORIGINAL_ERROR_OR_PERMIT")?;
 need(spec.method=="POST"&&spec.endpoint=="/orders","UNADMITTED_ORDINARY_ONLY")?;let b=spec.body.as_ref().ok_or_else(||fail("UNADMITTED_BODY"))?;
 need(b.as_object().is_some_and(|m|m.len()==8)&&b["tournamentId"]==config.scope.tournament_id&&b["exchangeId"].as_str().is_some_and(|e|config.scope.exchanges.contains_key(e))
  &&matches!(b["side"].as_str(),Some("yes"|"no"))&&matches!(b["action"].as_str(),Some("buy"|"sell")),"UNADMITTED_ORDER_SCOPE")?;
 if let Some(raw)=&spec.body_json{need(serde_json::from_str::<Value>(raw).map_err(|_|fail("UNADMITTED_BODY"))?==*b,"UNADMITTED_BODY_CHANGED")?;}
 let key=b["idempotencyKey"].as_str().filter(|s|!s.is_empty()&&s.len()<=255&&!s.chars().any(char::is_control)).ok_or_else(||fail("UNADMITTED_KEY"))?;
 let n=b["quantity"].as_u64().filter(|n|*n>0&&*n<=p.maximum_quantity as u64&&*n<=2_147_483_647).ok_or_else(||fail("UNADMITTED_QUANTITY"))?;
 let price=decimal_units(&b["price"].as_str().map(str::to_owned).unwrap_or_else(||b["price"].to_string()))?;
 need((5000..=995000).contains(&price)&&price%5000==0&&price.checked_mul(n as i128).is_some_and(|v|decimal_units(&p.maximum_cost).is_ok_and(|cap|v<=cap)),"UNADMITTED_PRICE_OR_COST")?;
 let expiry=b["expirationDate"].as_str().ok_or_else(||fail("UNADMITTED_EXPIRY"))?;need(parse_clock(&json!(expiry)).is_some_and(|n|n>0),"UNADMITTED_EXPIRY")?;
 // SDK place always normalizes price to six decimal JSON and supplies body_json.
 // Bind BOTH the original durable spec and this actual immutable queue payload.
 let mut body=b.clone();body["price"]=json!(serde_json::Number::from_string_unchecked(format!("{}.{:06}",price/1_000_000,price%1_000_000)));
 let actual=RequestSpec{method:"POST".into(),endpoint:"/orders".into(),body_json:Some(serde_json::to_string(&body).map_err(|_|fail("UNADMITTED_SERIALIZE"))?),body:Some(body)};
 let binding=admission::ImmutableBinding{intent:format!("{}:{}",p.run_id,p.intent_id),key:key.into(),payload_hash:digest(serde_json::to_string(&json!({"scope":config.scope,"spec":actual})).map_err(|_|fail("UNADMITTED_SERIALIZE"))?.as_bytes())};
 let meta=std::fs::symlink_metadata(&config.queue_dir).map_err(|_|fail("UNADMITTED_DIRECTORY"))?;need(meta.is_dir()&&!meta.file_type().is_symlink(),"UNADMITTED_DIRECTORY")?;
 #[cfg(unix)]let directory_identity={use std::os::unix::fs::MetadataExt;json!({"device":meta.dev(),"inode":meta.ino()})};
 #[cfg(not(unix))]let directory_identity:Value={return Err(fail("UNADMITTED_DIRECTORY_IDENTITY_PLATFORM_UNAVAILABLE"));};
 Ok(json!({"schemaVersion":1,"kind":"sdk_stopped_v1_before_admit_no_http","queueScope":config.account_scope,"queueDirectory":config.queue_dir,"queueDirectoryIdentity":directory_identity,"scopeSha256":digest(&serde_json::to_vec(&config.scope).map_err(|_|fail("UNADMITTED_SERIALIZE"))?),"originalRequestSha256":digest(&serde_json::to_vec(spec).map_err(|_|fail("UNADMITTED_SERIALIZE"))?),"sdkRequestSha256":digest(&serde_json::to_vec(&actual).map_err(|_|fail("UNADMITTED_SERIALIZE"))?),"permitSha256":digest(&serde_json::to_vec(p).map_err(|_|fail("UNADMITTED_SERIALIZE"))?),"binding":binding,"attemptAtMs":at,"originalError":error}))
}
fn unadmitted_archive(config:&Config,context:&Value)->std::path::PathBuf{config.queue_dir.join(format!("ordinary-unadmitted-{}.json",digest(serde_json::to_string(context).expect("JSON").as_bytes())))}
fn unadmitted_read(path:&std::path::Path)->Result<Vec<u8>>{
 let meta=std::fs::symlink_metadata(path).map_err(|_|fail("UNADMITTED_ARCHIVE_UNAVAILABLE"))?;need(meta.is_file()&&!meta.file_type().is_symlink()&&meta.len()<=768*1024*1024,"UNADMITTED_ARCHIVE_BOUND_OR_SYMLINK")?;
 let mut file=std::fs::File::open(path).map_err(|_|fail("UNADMITTED_ARCHIVE_UNAVAILABLE"))?;let held=file.metadata().map_err(|_|fail("UNADMITTED_ARCHIVE_UNAVAILABLE"))?;let mut bytes=Vec::new();file.by_ref().take(meta.len()+1).read_to_end(&mut bytes).map_err(|_|fail("UNADMITTED_ARCHIVE_READ"))?;
 let end=std::fs::symlink_metadata(path).map_err(|_|fail("UNADMITTED_ARCHIVE_CHANGED"))?;
 #[cfg(unix)]{use std::os::unix::fs::MetadataExt;need(meta.dev()==held.dev()&&meta.ino()==held.ino()&&end.dev()==held.dev()&&end.ino()==held.ino(),"UNADMITTED_ARCHIVE_REPLACED")?;}
 need(bytes.len()as u64==meta.len()&&end.len()==meta.len()&&meta.modified().ok()==end.modified().ok(),"UNADMITTED_ARCHIVE_CHANGED")?;Ok(bytes)
}
/// Offline frozen incident capture. Acquire the actual core lease exclusively,
/// or pin a retained original lease whose Linux PID is stopped. Caller holds the original
/// worker capture lease/STOP and validates the original durable attempt/error.
/// No queue open/recovery/commands, credentials or HTTP occurs. Unknown/active,
/// a tail at/after attempt, any permanent intent/key, V2 and missing data deny.
pub fn verify_frozen_ordinary_pre_admission(config:&Config,spec:&RequestSpec,p:&Permit,at:u64,error:&TransportError)->Result<OrdinaryPreAdmissionProof>{
 let context=unadmitted_context(config,spec,p,at,error)?;let binding:admission::ImmutableBinding=serde_json::from_value(context["binding"].clone()).map_err(|_|fail("UNADMITTED_BINDING"))?;
 let capture=admission::capture_unadmitted_v1(&config.queue_dir,&config.account_scope,&binding,at).map_err(|e|fail(&e))?;
 seal_unadmitted_proof(config,spec,p,at,error,context,capture)
}
fn seal_unadmitted_proof(config:&Config,spec:&RequestSpec,p:&Permit,at:u64,error:&TransportError,context:Value,mut capture:admission::FrozenUnadmittedSource)->Result<OrdinaryPreAdmissionProof>{
 let source=capture.snapshot();let value=json!({"context":context,"source":source});let mut bytes=serde_json::to_vec(&value).map_err(|_|fail("UNADMITTED_SERIALIZE"))?;need(bytes.len()<=768*1024*1024,"UNADMITTED_ARCHIVE_BOUND")?;let path=unadmitted_archive(config,&context);
 use std::io::Write;let mut options=std::fs::OpenOptions::new();options.write(true).create_new(true);#[cfg(unix)]{use std::os::unix::fs::OpenOptionsExt;options.mode(0o600);}
 match options.open(&path){
  Ok(mut file)=>{file.write_all(&bytes).and_then(|_|file.sync_all()).map_err(|_|fail("UNADMITTED_ARCHIVE_DURABILITY"))?;},
  Err(error)if error.kind()==std::io::ErrorKind::AlreadyExists=>{
   // A crash may precede the dispatch-abort append. A new genuine capture
   // lease can differ; every economic source byte/head must remain identical.
   // Preserve the ORIGINAL archive/lease/proof rather than substituting today's
   // lease into history. The full retained verifier below checks that original.
   let original=unadmitted_read(&path)?;let retained:Value=serde_json::from_slice(&original).map_err(|_|fail("UNADMITTED_ARCHIVE_JSON"))?;
   need(retained.as_object().is_some_and(|m|m.len()==2)&&retained["context"]==context
     &&serde_json::to_vec(&retained).map_err(|_|fail("UNADMITTED_SERIALIZE"))?==original,"UNADMITTED_ARCHIVE_CANONICAL_CONTEXT")?;
   let mut old=retained["source"].clone();let mut fresh=source.clone();
   for v in [&mut old,&mut fresh]{let o=v.as_object_mut().ok_or_else(||fail("UNADMITTED_ARCHIVE_SOURCE"))?;o.remove("originalLease");o.remove("leaseKind");}
   need(old==fresh,"UNADMITTED_ARCHIVE_FRESH_SOURCE_CHANGED")?;bytes=original;
  },
  Err(_)=>return Err(fail("UNADMITTED_ARCHIVE_ALREADY_EXISTS_OR_UNAVAILABLE")),
 }
 #[cfg(unix)]std::fs::File::open(&config.queue_dir).and_then(|f|f.sync_all()).map_err(|_|fail("UNADMITTED_ARCHIVE_DURABILITY"))?;
 let mut evidence=context;evidence["sourceArchive"]=json!(path.file_name().and_then(|s|s.to_str()).ok_or_else(||fail("UNADMITTED_ARCHIVE_NAME"))?);evidence["sourceArchiveSha256"]=json!(digest(&bytes));evidence["originalHead"]=source["verifiedHead"].clone();
 let verified=verify_retained_ordinary_pre_admission(config,spec,p,at,error,&evidence)?;capture.unchanged().map_err(|e|fail(&e))?;Ok(verified)
}
/// Reverify ONLY the deterministic original immutable SDK archive, independently
/// of later current queue clock/profile/compaction. Not a queue-health claim.
pub fn verify_retained_ordinary_pre_admission(config:&Config,spec:&RequestSpec,p:&Permit,at:u64,error:&TransportError,evidence:&Value)->Result<OrdinaryPreAdmissionProof>{
 let context=unadmitted_context(config,spec,p,at,error)?;let path=unadmitted_archive(config,&context);need(evidence["sourceArchive"].as_str()==path.file_name().and_then(|s|s.to_str()),"UNADMITTED_ARCHIVE_NAME")?;
 let bytes=unadmitted_read(&path)?;need(evidence["sourceArchiveSha256"]==digest(&bytes),"UNADMITTED_ARCHIVE_HASH")?;let archive:Value=serde_json::from_slice(&bytes).map_err(|_|fail("UNADMITTED_ARCHIVE_JSON"))?;need(archive["context"]==context,"UNADMITTED_ARCHIVE_CONTEXT")?;
 let binding:admission::ImmutableBinding=serde_json::from_value(context["binding"].clone()).map_err(|_|fail("UNADMITTED_BINDING"))?;let head=admission::verify_unadmitted_snapshot_v1(&config.account_scope,&binding,at,&archive["source"]).map_err(|e|fail(&e))?;
 let mut expected=context;expected["sourceArchive"]=json!(path.file_name().and_then(|s|s.to_str()).ok_or_else(||fail("UNADMITTED_ARCHIVE_NAME"))?);expected["sourceArchiveSha256"]=json!(digest(&bytes));expected["originalHead"]=head;need(*evidence==expected,"UNADMITTED_EVIDENCE_CHANGED")?;
 Ok(OrdinaryPreAdmissionProof{evidence:expected})
}
#[cfg(test)]#[path="ordinary_pre_admission_tests.rs"]mod ordinary_pre_admission_tests;
#[cfg(test)]mod tests;
#[cfg(test)]#[path="transport_not_sent_tests.rs"]mod transport_not_sent_tests;

#[cfg(test)]#[path="multi_leg_first_call_tests.rs"]mod multi_leg_first_call_tests;

#[cfg(test)]#[path="multi_leg_portfolio_tests.rs"]mod multi_leg_portfolio_tests;

#[cfg(test)]#[path="settlements_tests.rs"]mod settlements_tests;
