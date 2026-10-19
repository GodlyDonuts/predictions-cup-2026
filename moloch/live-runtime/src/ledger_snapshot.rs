//! Exact full native report shared only at one unchanged held Session head.
//! This is a rendering/digest cache, not financial or execution authority.
use moloch_replay::session::Session;
use serde_json::Value;
use sha2::{Digest,Sha256};
use std::{cell::RefCell,sync::Arc};
type Result<T>=std::result::Result<T,Box<dyn std::error::Error>>;
const MAXIMUM_ENCODED_BYTES:usize=64*1024*1024;
#[derive(PartialEq,Eq)]
struct Key{session:usize,manifest:Value,head:Value}
impl Key{fn current(bank:&Session)->Self{Self{session:bank as *const Session as usize,manifest:bank.manifest_hash().clone(),head:bank.durable_head()}}}
pub struct Snapshot{key:Key,value:Arc<Value>,sha:String}
impl Snapshot{
 pub fn value(&self)->&Arc<Value>{&self.value}
 pub fn validate_current(&self,bank:&Session)->bool{self.key==Key::current(bank)}
 pub fn matches_value(&self,value:&Value)->Result<bool>{
  // An immutable original allocation needs no repeated serialization. Equal
  // independent values still require the original complete cryptographic hash.
  Ok(std::ptr::eq(self.value.as_ref(),value)||digest(value)?==self.sha)
 }
}
fn digest(value:&Value)->Result<String>{Ok(format!("{:x}",Sha256::digest(serde_json::to_vec(value)?)))}
thread_local!{static CURRENT:RefCell<Option<Arc<Snapshot>>>=const{RefCell::new(None)};}
pub fn snapshot(bank:&Session)->Result<Arc<Snapshot>>{
 let key=Key::current(bank);
 if let Some(hit)=CURRENT.with(|slot|slot.borrow().as_ref().filter(|s|s.key==key).cloned()){return Ok(hit)}
 CURRENT.with(|slot|*slot.borrow_mut()=None);
 let value=bank.engine().report()?;let bytes=serde_json::to_vec(&value)?;
 let cacheable=bytes.len()<=MAXIMUM_ENCODED_BYTES;
 let out=Arc::new(Snapshot{key,value:Arc::new(value),sha:format!("{:x}",Sha256::digest(&bytes))});
 if cacheable{CURRENT.with(|slot|*slot.borrow_mut()=Some(out.clone()));}Ok(out)
}
