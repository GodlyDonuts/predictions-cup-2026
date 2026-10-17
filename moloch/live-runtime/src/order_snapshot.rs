//! Read-only order rendering cache, never financial/report/receipt authority.
//! A Session exposes no mutable Engine. Every successful state publication changes
//! durable_head; failed semantic admission leaves it unchanged, and I/O failure
//! retains the existing poisoned-session stop contract. Moving a Session misses.
use moloch_replay::session::Session;
use serde_json::Value;
use std::{cell::RefCell,collections::BTreeMap,sync::Arc};
pub const MAXIMUM_RECORDS:usize=100_000;
pub const MAXIMUM_ENCODED_BYTES:usize=64*1024*1024;
#[derive(PartialEq,Eq)]
struct Key{session:usize,manifest:Value,head:Value}
impl Key{fn current(bank:&Session)->Self{Self{session:bank as *const Session as usize,manifest:bank.manifest_hash().clone(),head:bank.durable_head()}}}
/// Immutable exact native rows. Holding this object does NOT make it current;
/// use snapshot(bank) again after any admission, or validate_current explicitly.
pub struct Snapshot{key:Key,records:Arc<Vec<Value>>,index:Option<BTreeMap<u64,usize>>}
impl Snapshot{
 pub fn records(&self)->&[Value]{self.records.as_slice()}
 pub fn quote(&self,qid:u64)->Option<&Value>{match &self.index{Some(index)=>index.get(&qid).map(|i|&self.records[*i]),None=>self.records.iter().find(|q|q["id"].as_u64()==Some(qid))}}
 pub fn validate_current(&self,bank:&Session)->bool{self.key==Key::current(bank)}
}
thread_local!{static CURRENT:RefCell<Option<Arc<Snapshot>>>=const{RefCell::new(None)};}
#[cfg(test)]thread_local!{static RENDERS:std::cell::Cell<usize>=const{std::cell::Cell::new(0)};}
// Conservative serialized JSON upper bound without serializing all order rows
// again. String bytes may each need six escaped bytes. This bounds encoded
// retention, not a claim that total allocator overhead equals JSON size.
fn encoded_bound(v:&Value)->Option<usize>{
 match v{
  Value::Null=>Some(4),Value::Bool(_)=>Some(5),Value::Number(n)=>Some(n.to_string().len()),
  Value::String(s)=>s.len().checked_mul(6)?.checked_add(2),
  Value::Array(a)=>a.iter().try_fold(2usize,|n,v|n.checked_add(encoded_bound(v)?)?.checked_add(1)),
  Value::Object(o)=>o.iter().try_fold(2usize,|n,(k,v)|n.checked_add(k.len().checked_mul(6)?)?.checked_add(4)?.checked_add(encoded_bound(v)?)),
 }
}
fn bounded(records:&[Value],max_records:usize,max_bytes:usize)->bool{
 records.len()<=max_records&&records.iter().try_fold(2usize,|n,v|n.checked_add(encoded_bound(v)?)?.checked_add(1)).is_some_and(|n|n<=max_bytes)
}
fn first_indices(records:&[Value])->BTreeMap<u64,usize>{
 let mut index=BTreeMap::new();for(i,q)in records.iter().enumerate(){if let Some(id)=q["id"].as_u64(){index.entry(id).or_insert(i);}}index
}
pub fn snapshot(bank:&Session)->Arc<Snapshot>{
 let key=Key::current(bank);
 if let Some(hit)=CURRENT.with(|slot|slot.borrow().as_ref().filter(|s|s.key==key).cloned()){return hit}
 // At most ONE retained cache entry; drop it before rendering a replacement.
 CURRENT.with(|slot|*slot.borrow_mut()=None);
 #[cfg(test)]RENDERS.with(|n|n.set(n.get()+1));
 let records=Arc::new(bank.engine().order_records());let cacheable=bounded(&records,MAXIMUM_RECORDS,MAXIMUM_ENCODED_BYTES);
 let index=cacheable.then(||first_indices(&records));let rendered=Arc::new(Snapshot{key,records,index});
 if cacheable{CURRENT.with(|slot|*slot.borrow_mut()=Some(rendered.clone()));}rendered
}
/// Same first-match semantics as the previous native rendering/find call.
pub fn quote(bank:&Session,qid:u64)->Option<Value>{snapshot(bank).quote(qid).cloned()}
#[cfg(test)]mod tests{
 use super::*;use serde_json::json;
 #[test]fn duplicate_and_malformed_ids_preserve_original_first_match(){
  let rows=vec![json!({"id":1,"retired":true}),json!({"id":1,"retired":false}),json!({"id":"1"}),json!({}),json!({"id":2})];
  let i=first_indices(&rows);assert_eq!(i.len(),2);assert_eq!(i[&1],0);assert_eq!(i[&2],4);
 }
 #[test]fn both_bounds_are_conservative_and_excess_is_uncached_not_financial_error(){
  let rows=vec![json!({"id":1,"s":"\n\"\\"})];let raw=serde_json::to_vec(&rows).unwrap().len();let upper=2+encoded_bound(&rows[0]).unwrap()+1;
  assert!(upper>=raw);assert!(bounded(&rows,1,upper));assert!(!bounded(&rows,0,upper));assert!(!bounded(&rows,1,upper-1));
 }
 #[test]fn genuine_session_full_scans_render_once_until_actual_native_head_changes(){
  use moloch_replay::session::Session;
  let root=std::env::temp_dir().join(format!("order-snapshot-render-count-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
  let at=1_792_022_400_000i64;
  let iso=|ms|chrono::DateTime::from_timestamp_millis(ms).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis,true);
  let policy=json!({"quantity":5,"maximumMarkets":3,"entryStopAt":iso(at+600000)});
  let seed=json!({"runId":"order-cache-render-count","availableCash":"1000.000000","accountSnapshotHash":"a".repeat(64),"observedAt":iso(at-1000)});
  let mut bank=Session::open_live(&root,&policy,&seed).unwrap();
  let book=json!({"type":"book","receivedAt":iso(at),"book":{"tournamentId":"cup","marketId":"101","exchangeId":"101","sequence":"1","sourceAt":iso(at),"requestStartedAt":iso(at),"receivedAt":iso(at),"bids":[{"price":"0.400000","quantity":100}],"asks":[{"price":"0.450000","quantity":100}]}});
  bank.accept("original-books",1,&book).unwrap();
  CURRENT.with(|slot|*slot.borrow_mut()=None);RENDERS.with(|n|n.set(0));
  let original=bank.engine().order_records();assert!(!original.is_empty());
  let before=bank.durable_head();let journal=std::fs::read(root.join("events.jsonl")).unwrap();
  let first=snapshot(&bank);
  for q in &original{
   let current=snapshot(&bank);assert!(Arc::ptr_eq(&first,&current));
   // Retain the exact full-scan/first-match comparisons used by fundamental
   // retirement, pending-liability and cancellation checks; not index shortcuts.
   assert_eq!(current.records().iter().find(|r|r["id"]==q["id"]),original.iter().find(|r|r["id"]==q["id"]));
   assert_eq!(current.records().iter().any(|r|r["exchangeId"]==q["exchangeId"]&&r["retired"]==false),original.iter().any(|r|r["exchangeId"]==q["exchangeId"]&&r["retired"]==false));
  }
  assert_eq!(RENDERS.with(|n|n.get()),1);assert_eq!(bank.durable_head(),before);assert_eq!(std::fs::read(root.join("events.jsonl")).unwrap(),journal);
  let tick=json!({"type":"tick","receivedAt":iso(at+1)});bank.accept("ticks",1,&tick).unwrap();
  let after=snapshot(&bank);assert!(!first.validate_current(&bank));assert!(!Arc::ptr_eq(&first,&after));assert_eq!(after.records(),bank.engine().order_records());assert_eq!(RENDERS.with(|n|n.get()),2);
  assert!(!bank.accept("ticks",1,&tick).unwrap());assert!(Arc::ptr_eq(&after,&snapshot(&bank)));
  let mut conflict=tick.clone();conflict["receivedAt"]=json!(iso(at+2));assert!(bank.accept("ticks",1,&conflict).is_err());assert!(Arc::ptr_eq(&after,&snapshot(&bank)));assert_eq!(RENDERS.with(|n|n.get()),2);
  drop(bank);CURRENT.with(|slot|*slot.borrow_mut()=None);std::fs::remove_dir_all(root).unwrap();
 }
}
