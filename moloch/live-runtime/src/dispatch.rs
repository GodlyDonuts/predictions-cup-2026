//! Physical network attempts are recorded independently of strategy projections.
use crate::feedback::Result;
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::{fs::{self,File,OpenOptions},io::{BufRead,BufReader,Write},path::Path,collections::{BTreeMap,BTreeSet}};
fn hash(v:&Value)->String{format!("{:x}",Sha256::digest(serde_json::to_vec(v).unwrap()))}
pub struct DispatchLog {file:File,sequence:u64,previous:String,pub intents:BTreeMap<u64,Value>,pub acknowledgments:BTreeMap<u64,Value>,pub recovered:BTreeMap<u64,Value>,pub attempted:BTreeSet<u64>,pub closed:BTreeSet<u64>,pub cancel_acknowledged:BTreeSet<u64>}
impl DispatchLog {
 pub fn descriptor_identity(&self)->Result<Value>{
  #[cfg(unix)]{use std::os::unix::fs::MetadataExt;let m=self.file.metadata()?;Ok(json!({"schemaVersion":1,"device":m.dev(),"inode":m.ino(),"bytes":m.len(),"sequence":self.sequence,"previous":self.previous}))}
  #[cfg(not(unix))]{Err("DISPATCH_DESCRIPTOR_IDENTITY_UNSUPPORTED".into())}
 }
 pub fn open(path:&Path)->Result<Self>{
  let mut options=OpenOptions::new();options.read(true).append(true).create(true);
  #[cfg(unix)]{use std::os::unix::fs::OpenOptionsExt;options.mode(0o600);}
  let file=options.open(path)?;file.sync_all()?;File::open(path.parent().ok_or("DISPATCH_PARENT")?)?.sync_all()?;
  let bytes=fs::read(path)?;if !bytes.is_empty()&&bytes.last()!=Some(&b'\n'){return Err("DISPATCH_TORN_TAIL".into())}
  let mut log=Self{file,sequence:0,previous:"0".repeat(64),intents:Default::default(),acknowledgments:Default::default(),recovered:Default::default(),attempted:Default::default(),closed:Default::default(),cancel_acknowledged:Default::default()};
  for line in BufReader::new(File::open(path)?).lines(){let mut row:Value=serde_json::from_str(&line?)?;let claimed=row.as_object_mut().ok_or("DISPATCH_ROW")?.remove("hash").ok_or("DISPATCH_HASH")?;if row["sequence"]!=log.sequence+1||row["previous"]!=log.previous||claimed!=hash(&row){return Err("DISPATCH_CHAIN".into())}log.apply(&row["event"])?;log.sequence+=1;log.previous=claimed.as_str().ok_or("DISPATCH_HASH")?.into();}
  Ok(log)
 }
 fn apply(&mut self,e:&Value)->Result<()> {
  let id=e["qid"].as_u64().ok_or("DISPATCH_QID")?;
  match e["kind"].as_str(){
   Some("intent")=>{if self.intents.contains_key(&id){return Err("DISPATCH_DUPLICATE_INTENT".into())}self.intents.insert(id,e.clone());},
   Some("attempt")=>{if !self.intents.contains_key(&id)||!self.attempted.insert(id){return Err("DISPATCH_REPEAT_ATTEMPT".into())}},
   Some("ack")=>{if !self.attempted.contains(&id)||self.acknowledgments.insert(id,e["receipt"].clone()).is_some(){return Err("DISPATCH_ACK_WITHOUT_ATTEMPT".into())}},
   Some("recovered_ack")=>{if !self.attempted.contains(&id)||self.acknowledgments.insert(id,e["order"].clone()).is_some()||self.recovered.insert(id,e.clone()).is_some(){return Err("DISPATCH_RECOVERY_WITHOUT_ATTEMPT".into())}},
   Some("closed")=>{if !self.acknowledgments.contains_key(&id)||!self.closed.insert(id){return Err("DISPATCH_TERMINAL_WITHOUT_ACK".into())}},
   Some("cancel_ack")=>{if !self.acknowledgments.contains_key(&id)||!self.cancel_acknowledged.insert(id){return Err("DISPATCH_CANCEL_ACK_WITHOUT_ORDER_OR_DUPLICATE".into())}},
   Some("cancel_attempt")|Some("error")|Some("source")|Some("not_sent")=>{},
   _=>return Err("DISPATCH_EVENT".into())
  }Ok(())
 }
 pub fn append(&mut self,event:Value)->Result<()>{
  let mut next=json!({"sequence":self.sequence+1,"previous":self.previous,"event":event});let digest=hash(&next);next["hash"]=json!(digest);
  // An append failure ends the process; no network call follows uncertain durability.
  let bytes=serde_json::to_vec(&next)?;self.file.write_all(&bytes)?;self.file.write_all(b"\n")?;self.file.sync_all()?;
  self.apply(&event)?;self.sequence+=1;self.previous=digest;Ok(())
 }
 pub fn require_resolved_attempts(&self)->Result<()>{if self.attempted.iter().any(|q|!self.acknowledgments.contains_key(q)){return Err("DISPATCH_UNKNOWN_ACCEPTANCE_MANUAL_RECONCILIATION".into())}Ok(())}
 pub fn never_attempted(&self,qid:u64)->bool{!self.attempted.contains(&qid)&&!self.acknowledgments.contains_key(&qid)}
 pub fn proof(&self,qid:u64)->Value{json!({"qid":qid,"chain":self.previous,"sequence":self.sequence,"networkAttempts":if self.never_attempted(qid){0}else{1}})}
}
#[cfg(test)]mod tests{
 use super::*;
 fn path()->std::path::PathBuf{let p=std::env::temp_dir().join(format!("moloch-dispatch-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));fs::create_dir(&p).unwrap();p.join("dispatch.ndjson")}
 #[test]fn unknown_attempt_survives_restart_and_cannot_be_replaced(){let p=path();{let mut l=DispatchLog::open(&p).unwrap();l.append(json!({"kind":"intent","qid":1,"spec":{"price":0.5}})).unwrap();assert!(l.never_attempted(1));l.append(json!({"kind":"attempt","qid":1})).unwrap();assert!(!l.never_attempted(1));assert!(l.require_resolved_attempts().is_err());}let l=DispatchLog::open(&p).unwrap();assert!(l.require_resolved_attempts().is_err());assert!(!l.never_attempted(1));fs::remove_dir_all(p.parent().unwrap()).unwrap();}
 #[test]fn acknowledged_orders_and_terminal_proof_survive_restart(){let p=path();{let mut l=DispatchLog::open(&p).unwrap();for e in [json!({"kind":"intent","qid":1}),json!({"kind":"attempt","qid":1}),json!({"kind":"ack","qid":1,"receipt":{"orderId":77}}),json!({"kind":"closed","qid":1})]{l.append(e).unwrap();}}let l=DispatchLog::open(&p).unwrap();assert_eq!(l.acknowledgments[&1]["orderId"],77);assert!(l.closed.contains(&1));l.require_resolved_attempts().unwrap();fs::remove_dir_all(p.parent().unwrap()).unwrap();}
 #[test]fn truncation_or_mutation_is_not_a_clean_log(){let p=path();{let mut l=DispatchLog::open(&p).unwrap();l.append(json!({"kind":"intent","qid":1})).unwrap();}let text=fs::read_to_string(&p).unwrap();fs::write(&p,text.replace("\"qid\":1","\"qid\":2")).unwrap();assert!(DispatchLog::open(&p).is_err());fs::write(&p,"{").unwrap();assert!(DispatchLog::open(&p).is_err());fs::remove_dir_all(p.parent().unwrap()).unwrap();}
 #[test]fn confirmed_cancel_survives_restart_while_terminal_reconciliation_remains_pending(){let p=path();{let mut l=DispatchLog::open(&p).unwrap();for e in [json!({"kind":"intent","qid":1}),json!({"kind":"attempt","qid":1}),json!({"kind":"ack","qid":1,"receipt":{"orderId":77}}),json!({"kind":"cancel_attempt","qid":1}),json!({"kind":"cancel_ack","qid":1,"receipt":{"orderId":77}})]{l.append(e).unwrap();}}let l=DispatchLog::open(&p).unwrap();assert!(l.cancel_acknowledged.contains(&1));assert!(!l.closed.contains(&1));l.require_resolved_attempts().unwrap();fs::remove_dir_all(p.parent().unwrap()).unwrap();}
}
