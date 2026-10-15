//! Transfer only an entirely unused live allocation, retaining its full source bank.
use crate::{feedback::Result,dispatch::DispatchLog};
use moloch_replay::{session::Session,amount};
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::{path::Path,fs};
pub fn verify_flat_predecessor(root:&Path,bank_name:&str,new_policy:&Value)->Result<Value>{
 if !bank_name.starts_with("bank")||bank_name.contains(['/', '\\']){return Err("LIVE_PREDECESSOR_PATH".into())}
 let directory=root.join(bank_name);let manifest:Value=serde_json::from_slice(&fs::read(directory.join("manifest.json"))?)?;
 let seed=manifest["seed"]["evidence"].clone();let bank=Session::open_live(&directory,&manifest["policy"],&seed)?;
 let report=bank.engine().report()?;validate_flat(&report,&manifest["policy"],new_policy)?;
 let log=DispatchLog::open(&root.join("dispatch.ndjson"))?;log.require_resolved_attempts()?;
 if !log.attempted.is_empty()||!log.intents.is_empty()||!log.acknowledgments.is_empty(){return Err("LIVE_PREDECESSOR_DISPATCH_HISTORY".into())}
 let bytes=fs::read(directory.join("events.jsonl"))?;
 Ok(json!({"kind":"transfer_entire_unused_allocation","predecessorBank":bank_name,"predecessorManifestHash":bank.manifest_hash(),"predecessorHead":serde_json::from_slice::<Value>(&fs::read(directory.join("head.json"))?)?,"predecessorJournalSha256":format!("{:x}",Sha256::digest(bytes)),"report":report,"capitalTransferred":new_policy["initialCash"],"capitalAdded":"0.000000","historicalBankPreserved":true}))
}
fn validate_flat(report:&Value,old:&Value,new:&Value)->Result<()>{
 if old["initialCash"]!=new["initialCash"]||old["maximumCommitted"]!=new["maximumCommitted"]||old["entryStopAt"]!=new["entryStopAt"]{return Err("LIVE_TRANSFER_CAPITAL_OR_DEADLINE_CHANGED".into())}
 for key in ["ownedCost","reservedCash","realized","fees"]{if amount(report[key].as_str().ok_or("LIVE_TRANSFER_REPORT")?)?!=0{return Err("LIVE_TRANSFER_NOT_UNUSED".into())}}
 if report["executionMode"]!="live"||report["orders"]!=0||report["fills"]!=0||report["unresolvedOrders"]!=0||report["cash"]!=old["initialCash"]{return Err("LIVE_TRANSFER_NOT_UNUSED".into())}Ok(())
}
#[cfg(test)]mod tests{use super::*;#[test]fn transfer_rejects_actual_activity_and_changed_funding_or_deadline(){let p=json!({"initialCash":"40000.000000","maximumCommitted":"40000.000000","entryStopAt":"2026-10-15T14:44:34Z"});let r=json!({"executionMode":"live","cash":"40000.000000","ownedCost":"0.000000","reservedCash":"0.000000","realized":"0.000000","fees":"0.000000","orders":0,"fills":0,"unresolvedOrders":0});validate_flat(&r,&p,&p).unwrap();for key in ["orders","fills","unresolvedOrders"]{let mut changed=r.clone();changed[key]=json!(1);assert!(validate_flat(&changed,&p,&p).is_err());}let mut changed=p.clone();changed["entryStopAt"]=json!("later");assert!(validate_flat(&r,&p,&changed).is_err());let mut changed=r.clone();changed["realized"]=json!("-0.005000");assert!(validate_flat(&changed,&p,&p).is_err());}}
