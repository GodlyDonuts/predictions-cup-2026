//! Private first-logical-invocation evidence, never an official venue receipt.
//! A permanent intent AND key ticket precedes guards/admission. The SDK's
//! monotonic in-process stage is the no-HTTP evidence; queue absence is not.
//! Source hashes detect changes, not an attacker able to rewrite trusted SDK
//! files and recompute all hashes. These files require the worker's private ACL.
use crate::{admission::ImmutableBinding,multi_leg::{self,Basket},Scope,Permit,RequestSpec,TransportError,Result,need,fail,digest};
use serde_json::{Value,json};
use std::{fs::{self,File,OpenOptions},io::{Read,Write},path::{Path,PathBuf},sync::atomic::{AtomicU64,Ordering}};
static SERIAL:AtomicU64=AtomicU64::new(0);
const LIMIT:u64=1024*1024;
#[cfg(test)]thread_local!{static FAIL_SYNC_PHASE:std::cell::Cell<u8>=const{std::cell::Cell::new(0)};}
#[cfg(test)]pub(crate) fn fail_next_sync(phase:u8){FAIL_SYNC_PHASE.with(|f|f.set(phase));}


/// Constructible only from an SDK trace and its original durable sources.
/// This is local evidence; it grants neither account health nor retry authority.
pub struct Proof{evidence:Value}
impl Proof{pub fn evidence(&self)->&Value{&self.evidence}}
pub(crate) struct Ticket{root:PathBuf,ticket:Value,key:Value,binding:ImmutableBinding,eligible:bool,admit_invoked:bool}
fn bytes(v:&Value)->Result<Vec<u8>>{serde_json::to_vec(v).map_err(|_|fail("FIRST_CALL_SERIALIZATION"))}
fn hash(v:&Value)->Result<String>{Ok(digest(&bytes(v)?))}
fn seal(mut v:Value)->Result<Value>{v["sha256"]=json!(hash(&v)?);Ok(v)}
fn check(v:&Value)->Result<()>{let mut body=v.clone();let h=body.as_object_mut().ok_or_else(||fail("FIRST_CALL_SOURCE_INVALID"))?.remove("sha256").ok_or_else(||fail("FIRST_CALL_SOURCE_HASH"))?;need(h==hash(&body)?,"FIRST_CALL_SOURCE_HASH")}
fn name(scope:&str,value:&str,kind:&str)->String{format!("first-call-{kind}-{}.json",digest(&serde_json::to_vec(&json!([scope,value])).expect("strings serialize")))}
fn safe_root(root:&Path)->Result<PathBuf>{
 for p in root.ancestors(){need(!fs::symlink_metadata(p).map_err(|_|fail("FIRST_CALL_ROOT_UNAVAILABLE"))?.file_type().is_symlink(),"FIRST_CALL_SYMLINK")?;}
 let p=fs::canonicalize(root).map_err(|_|fail("FIRST_CALL_ROOT_UNAVAILABLE"))?;need(p.is_dir(),"FIRST_CALL_ROOT_UNAVAILABLE")?;Ok(p)
}
fn sync_dir(root:&Path)->Result<()>{#[cfg(unix)]File::open(root).and_then(|f|f.sync_all()).map_err(|_|fail("FIRST_CALL_DURABILITY"))?;#[cfg(not(unix))]let _=root;Ok(())}
fn write_new(root:&Path,path:&Path,v:&Value)->Result<()>{
 let data=bytes(v)?;need(data.len() as u64<=LIMIT,"FIRST_CALL_SOURCE_TOO_LARGE")?;
 let mut o=OpenOptions::new();o.write(true).create_new(true);
 #[cfg(unix)]{use std::os::unix::fs::OpenOptionsExt;o.mode(0o600);}
 let mut f=o.open(path).map_err(|_|fail("FIRST_CALL_ALREADY_INVOKED_OR_UNAVAILABLE"))?;
 f.write_all(&data).map_err(|_|fail("FIRST_CALL_DURABILITY"))?;
 #[cfg(test)]{
  let filename=path.file_name().and_then(|s|s.to_str()).unwrap_or("");
  let phase=if filename.starts_with("first-call-intent-"){1}else if filename.starts_with("first-call-key-"){2}else{3};
  if FAIL_SYNC_PHASE.with(|v|if v.get()==phase{v.set(0);true}else{false}){return Err(fail("FIRST_CALL_DURABILITY"));}
 }
 f.sync_all().map_err(|_|fail("FIRST_CALL_DURABILITY"))?;sync_dir(root)
}
fn read(root:&Path,path:&Path)->Result<Value>{
 need(path.parent()==Some(root),"FIRST_CALL_SOURCE_PATH")?;
 let before=fs::symlink_metadata(path).map_err(|_|fail("FIRST_CALL_SOURCE_UNAVAILABLE"))?;
 need(before.is_file()&&!before.file_type().is_symlink()&&before.len()<=LIMIT,"FIRST_CALL_SOURCE_TYPE_OR_SIZE")?;
 let mut o=OpenOptions::new();o.read(true);
 #[cfg(windows)]{use std::os::windows::fs::OpenOptionsExt;o.share_mode(1);}
 let mut f=o.open(path).map_err(|_|fail("FIRST_CALL_SOURCE_UNAVAILABLE"))?;
 let opened=f.metadata().map_err(|_|fail("FIRST_CALL_SOURCE_UNAVAILABLE"))?;
 let mut data=Vec::new();(&mut f).take(LIMIT+1).read_to_end(&mut data).map_err(|_|fail("FIRST_CALL_SOURCE_UNAVAILABLE"))?;
 let after=fs::symlink_metadata(path).map_err(|_|fail("FIRST_CALL_SOURCE_UNAVAILABLE"))?;
 need(data.len() as u64==before.len()&&before.len()==opened.len()&&before.len()==after.len()&&before.modified().ok()==after.modified().ok()&&!after.file_type().is_symlink(),"FIRST_CALL_SOURCE_CHANGED")?;
 #[cfg(unix)]{use std::os::unix::fs::MetadataExt;need(before.ino()==opened.ino()&&before.dev()==opened.dev()&&before.ino()==after.ino()&&before.dev()==after.dev(),"FIRST_CALL_SOURCE_REPLACED")?;}
 let value:Value=serde_json::from_slice(&data).map_err(|_|fail("FIRST_CALL_SOURCE_INVALID"))?;
 need(bytes(&value)?==data,"FIRST_CALL_SOURCE_NONCANONICAL")?;check(&value)?;Ok(value)
}
pub(crate) fn binding(scope:&Scope,spec:&RequestSpec,basket:&Basket,permit:&Permit)->Result<ImmutableBinding>{Ok(ImmutableBinding{intent:format!("{}:{}",permit.run_id,permit.intent_id),key:basket.idempotency_key.clone(),payload_hash:hash(&json!({"scope":scope,"spec":spec}))?})}
pub(crate) fn legacy_request_path(root:&Path,spec:&RequestSpec,basket:&Basket,permit:&Permit)->Result<PathBuf>{Ok(root.join(format!("basket-request-{}.json",hash(&json!({"request":spec,"basket":basket,"runId":permit.run_id,"intentId":permit.intent_id,"owner":permit.owner}))?)))}
impl Ticket{
 pub(crate) fn begin(root:&Path,account:&str,scope:&Scope,basket:&Basket,permit:&Permit,spec:&RequestSpec,now:u64)->Result<Self>{
  let root=safe_root(root)?;let binding=binding(scope,spec,basket,permit)?;
  let ticket=seal(json!({"schemaVersion":1,"kind":"sdk_first_logical_invocation","accountScope":account,"root":root,"scope":scope,"basket":basket,"permit":permit,"request":spec,"requestSha256":digest(&serde_json::to_vec(spec).map_err(|_|fail("FIRST_CALL_SERIALIZATION"))?),"binding":binding,"createdAtMs":now,"invocationId":format!("{}-{now}-{}",std::process::id(),SERIAL.fetch_add(1,Ordering::SeqCst))}))?;
  let key=seal(json!({"schemaVersion":1,"kind":"sdk_first_logical_key","accountScope":account,"key":binding.key,"ticketSha256":ticket["sha256"],"intent":binding.intent}))?;
  write_new(&root,&root.join(name(account,&binding.intent,"intent")),&ticket)?;
  write_new(&root,&root.join(name(account,&binding.key,"key")),&key)?;
  Ok(Self{root,ticket,key,binding,eligible:false,admit_invoked:false})
 }
 pub(crate) fn qualify(&mut self){self.eligible=true;}
 pub(crate) fn disqualify(&mut self){self.admit_invoked=true;}
 pub(crate) fn can_seal(&self)->bool{self.eligible&&!self.admit_invoked}
 pub(crate) fn finish(self,error:&TransportError,now:u64)->Result<Proof>{
  need(self.can_seal()&&!error.may_have_executed&&error.status.is_none()&&error.order_id.is_none()&&error.api_code.is_none(),"FIRST_CALL_NO_HTTP_UNPROVEN")?;
  need(now>=self.ticket["createdAtMs"].as_u64().ok_or_else(||fail("FIRST_CALL_CLOCK"))?,"FIRST_CALL_CLOCK")?;
  let account=self.ticket["accountScope"].as_str().ok_or_else(||fail("FIRST_CALL_SCOPE"))?;
  need(safe_root(&self.root)?==self.root&&read(&self.root,&self.root.join(name(account,&self.binding.intent,"intent")))?==self.ticket&&read(&self.root,&self.root.join(name(account,&self.binding.key,"key")))?==self.key,"FIRST_CALL_SOURCE_CHANGED")?;
  let terminal=seal(json!({"schemaVersion":1,"kind":"sdk_before_first_admit_no_http","accountScope":account,"binding":self.binding,"requestSha256":self.ticket["requestSha256"],"ticketSha256":self.ticket["sha256"],"keyIndexSha256":self.key["sha256"],"invocationId":self.ticket["invocationId"],"startedAtMs":self.ticket["createdAtMs"],"completedAtMs":now,"stage":"before_first_admit_invocation","admitInvocations":0,"httpInvocations":0,"queueHealth":"not_established","originalError":error}))?;
  write_new(&self.root,&self.root.join(name(account,&self.binding.intent,"terminal")),&terminal)?;
  Ok(Proof{evidence:json!({"kind":"sdk_before_first_admit_no_http","ticket":self.ticket,"keyIndex":self.key,"terminal":terminal})})
 }
}
pub(crate) fn recover(root:&Path,account:&str,scope:&Scope,binding:&ImmutableBinding,request_sha256:&str)->Result<Proof>{
 let root=safe_root(root)?;
 let ticket=read(&root,&root.join(name(account,&binding.intent,"intent")))?;
 let key=read(&root,&root.join(name(account,&binding.key,"key")))?;
 let terminal=read(&root,&root.join(name(account,&binding.intent,"terminal")))?;
 need(ticket["schemaVersion"]==1&&ticket["kind"]=="sdk_first_logical_invocation"&&ticket["root"]==json!(root)&&ticket["accountScope"]==account&&ticket["scope"]==serde_json::to_value(scope).map_err(|_|fail("FIRST_CALL_SCOPE"))?&&ticket["binding"]==serde_json::to_value(binding).map_err(|_|fail("FIRST_CALL_BINDING"))?&&ticket["requestSha256"]==request_sha256,"FIRST_CALL_TICKET_BINDING")?;
 let basket:Basket=serde_json::from_value(ticket["basket"].clone()).map_err(|_|fail("FIRST_CALL_BASKET"))?;
 let permit:Permit=serde_json::from_value(ticket["permit"].clone()).map_err(|_|fail("FIRST_CALL_PERMIT"))?;
 let spec=multi_leg::request_spec(&basket,scope,&permit)?;
 need(ticket["request"]==serde_json::to_value(&spec).map_err(|_|fail("FIRST_CALL_REQUEST"))?&&digest(&serde_json::to_vec(&spec).map_err(|_|fail("FIRST_CALL_REQUEST"))?)==request_sha256&&self::binding(scope,&spec,&basket,&permit)?==*binding,"FIRST_CALL_REQUEST_BINDING")?;
 need(key["schemaVersion"]==1&&key["kind"]=="sdk_first_logical_key"&&key["accountScope"]==account&&key["key"]==binding.key&&key["intent"]==binding.intent&&key["ticketSha256"]==ticket["sha256"],"FIRST_CALL_KEY_BINDING")?;
 need(terminal["schemaVersion"]==1&&terminal["kind"]=="sdk_before_first_admit_no_http"&&terminal["accountScope"]==account&&terminal["binding"]==ticket["binding"]&&terminal["requestSha256"]==request_sha256&&terminal["ticketSha256"]==ticket["sha256"]&&terminal["keyIndexSha256"]==key["sha256"]&&terminal["invocationId"]==ticket["invocationId"]&&terminal["startedAtMs"]==ticket["createdAtMs"]&&terminal["stage"]=="before_first_admit_invocation"&&terminal["admitInvocations"]==0&&terminal["httpInvocations"]==0&&terminal["queueHealth"]=="not_established","FIRST_CALL_TERMINAL_BINDING")?;
 let error=&terminal["originalError"];
 need(error["mayHaveExecuted"]==false&&error.get("status")==Some(&Value::Null)&&error.get("orderId")==Some(&Value::Null)&&error.get("apiCode")==Some(&Value::Null)&&error["code"].as_str().is_some_and(|s|!s.is_empty())&&error["retryAfterMs"].as_u64().is_some(),"FIRST_CALL_TERMINAL_ERROR")?;
 need(terminal["completedAtMs"].as_u64().zip(terminal["startedAtMs"].as_u64()).is_some_and(|(end,start)|end>=start),"FIRST_CALL_CLOCK")?;
 need(read(&root,&root.join(name(account,&binding.intent,"intent")))?==ticket&&read(&root,&root.join(name(account,&binding.key,"key")))?==key&&read(&root,&root.join(name(account,&binding.intent,"terminal")))?==terminal,"FIRST_CALL_SOURCE_CHANGED")?;
 Ok(Proof{evidence:json!({"kind":"sdk_before_first_admit_no_http","ticket":ticket,"keyIndex":key,"terminal":terminal})})
}
