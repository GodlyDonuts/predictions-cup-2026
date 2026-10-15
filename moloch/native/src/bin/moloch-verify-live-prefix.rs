//! Read-only replay against a sealed live-bank prefix. No writer lease,
//! account client, identity approval, migration or source mutation is created.
use moloch_replay::{engine::Engine,journal::Journal};
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::{collections::BTreeMap,fs::{self,File},io::{BufRead,BufReader,Read},path::Path,time::Instant};
type Result<T>=std::result::Result<T,Box<dyn std::error::Error>>;
fn hash(v:&Value)->Result<String>{Ok(format!("{:x}",Sha256::digest(serde_json::to_vec(v)?)))}
fn sealed(path:&Path)->Result<Value>{
 let m=fs::symlink_metadata(path)?;if m.file_type().is_symlink()||!m.is_file()||m.len()>256*1024*1024{return Err("PREFIX_METADATA_FILE".into())}
 let v:Value=serde_json::from_slice(&fs::read(path)?)?;let mut body=v.clone();let expected=body.as_object_mut().ok_or("PREFIX_OBJECT")?.remove("hash").ok_or("PREFIX_HASH")?;
 if expected.as_str()!=Some(hash(&body)?.as_str()){return Err("PREFIX_METADATA_HASH".into())}Ok(v)
}
pub fn check(root:&Path)->Result<Value>{
 if fs::symlink_metadata(root)?.file_type().is_symlink(){return Err("PREFIX_DIRECTORY_SYMLINK".into())}
 let manifest=sealed(&root.join("manifest.json"))?;let head=sealed(&root.join("head.json"))?;let report=sealed(&root.join("report.json"))?;
 if manifest["mode"]!="rust_live_session"||head["manifestHash"]!=manifest["hash"]||report["manifestHash"]!=manifest["hash"]||head["sequence"]!=report["sequence"]||head["previous"]!=report["previous"]{return Err("PREFIX_SOURCE_SCOPE".into())}
 let p=root.join("events.jsonl");let meta=fs::symlink_metadata(&p)?;if meta.file_type().is_symlink()||!meta.is_file(){return Err("PREFIX_JOURNAL_FILE".into())}
 let start=Instant::now();let mut engine=Engine::new_live(&manifest["policy"])?;let mut chain=Journal::default();let mut cursors=BTreeMap::<String,u64>::new();let mut digest=Sha256::new();let mut bytes=0u64;let mut input=BufReader::new(File::open(p)?);
 loop{
  let mut line=Vec::new();let n=input.by_ref().take(16*1024*1024+1).read_until(b'\n',&mut line)?;if n==0{break}
  if n>16*1024*1024||line.last()!=Some(&b'\n'){return Err("PREFIX_ROW_BOUNDARY".into())}
  bytes=bytes.checked_add(n as u64).ok_or("PREFIX_LENGTH")?;digest.update(&line);let event=chain.accept(&serde_json::from_slice(&line)?)?;
  let source=&event["sourceRef"];let id=source["id"].as_str().filter(|s|!s.is_empty()&&s.len()<=256&&!s.contains(['\n','\r'])).ok_or("PREFIX_SOURCE_ID")?;let seq=source["sequence"].as_u64().ok_or("PREFIX_SOURCE_SEQUENCE")?;
  let mut original=event.clone();original.as_object_mut().ok_or("PREFIX_EVENT")?.remove("sourceRef");
  if source["hash"].as_str()!=Some(hash(&original)?.as_str())||seq!=cursors.get(id).copied().unwrap_or(0).checked_add(1).ok_or("PREFIX_SOURCE_SEQUENCE")?{return Err("PREFIX_SOURCE_CHAIN".into())}
  cursors.insert(id.into(),seq);engine.advance(&event)?;
 }
 if head["sequence"].as_u64()!=Some(chain.sequence)||head["previous"].as_str()!=Some(chain.previous.as_str()){return Err("PREFIX_HEAD_CHAIN".into())}
 let actual=engine.report()?;
 if report["report"]!=actual||report["orders"]!=json!(engine.order_records())||report["fills"]!=json!(engine.fills())||report["sourceCursors"]!=json!(cursors)||report["contestOrders"].as_u64()!=Some(engine.live_order_bindings().len() as u64){return Err("PREFIX_FINANCIAL_PARITY".into())}
 Ok(json!({"schemaVersion":1,"mode":"read_only_live_prefix_equivalence","engineIdentity":moloch_replay::session::engine_identity(),"sequence":chain.sequence,"previous":chain.previous,"prefixBytes":bytes,"prefixSha256":format!("{:x}",digest.finalize()),"financialReportSha256":hash(&actual)?,"orderRecordsSha256":hash(&json!(engine.order_records()))?,"fillsSha256":hash(&json!(engine.fills()))?,"sourceCursorsSha256":hash(&json!(cursors))?,"elapsedMs":start.elapsed().as_millis(),"financialParity":true,"sourceMutated":false,"executionAuthorized":false,"accountRequests":0}))
}
fn main()->Result<()>{let args:Vec<_>=std::env::args().collect();if args.len()!=2{return Err("usage: moloch-verify-live-prefix FROZEN_BANK_DIRECTORY".into())}println!("{}",check(Path::new(&args[1]))?);Ok(())}
