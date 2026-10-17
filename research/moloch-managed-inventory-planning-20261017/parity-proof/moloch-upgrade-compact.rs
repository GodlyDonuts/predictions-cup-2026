use std::{fs,path::Path,time::Instant};
use serde_json::{Value,json};
use moloch_replay::session::{Session,propose_live_engine_upgrade,engine_identity};
fn load(p:&Path)->Result<Value,Box<dyn std::error::Error>>{Ok(serde_json::from_slice(&fs::read(p)?)?)}
fn main()->Result<(),Box<dyn std::error::Error>>{
 let a:Vec<String>=std::env::args().collect();
 if a.len()==4&&a[1]=="parity"{
  let root=Path::new(&a[2]);let before=load(Path::new(&a[3]))?;let manifest=load(&root.join("manifest.json"))?;let head=load(&root.join("head.json"))?;
  let bytes=fs::metadata(root.join("events.jsonl"))?.len();let start=Instant::now();
  let bank=Session::open_live(root,&manifest["policy"],&manifest["seed"]["evidence"])?;
  let elapsed=start.elapsed().as_secs_f64();let after=load(&root.join("report.json"))?;
  if before!=after||head!=bank.durable_head()||fs::metadata(root.join("events.jsonl"))?.len()!=bytes{return Err("FULL_ORIGINAL_FINANCIAL_PREFIX_PARITY_FAILED".into())}
  println!("{}",json!({"financialPrefixParityPassed":true,"finalPublicStateExact":true,"nativeEngineIdentity":engine_identity(),"sequence":bank.durable_head()["sequence"],"journalBytes":bytes,"fills":bank.engine().fills().len(),"restoreSeconds":elapsed,"networkRequests":0,"productionAuthorization":false}));return Ok(());
 }
 if a.len()!=5{return Err("BANK OLD_BINARY_SHA APPROVED_UTC PROPOSAL | parity CLONE ORIGINAL_REPORT".into())}
 let proposal=propose_live_engine_upgrade(Path::new(&a[1]),&a[2],&a[3])?;
 use std::io::Write;let mut f=fs::OpenOptions::new().write(true).create_new(true).open(&a[4])?;serde_json::to_writer(&mut f,&proposal)?;f.write_all(b"\n")?;f.sync_all()?;
 println!("{}",json!({"proposedOnly":true,"nativeEngineIdentity":engine_identity(),"upgrades":proposal["upgrades"].as_array().ok_or("UPGRADES")?.len()}));Ok(())
}
