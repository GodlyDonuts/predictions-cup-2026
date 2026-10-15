#[path="../src/bin/moloch-verify-live-prefix.rs"]mod verifier;
use moloch_replay::session::Session;
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::{fs,path::PathBuf,sync::atomic::{AtomicU64,Ordering}};
static NEXT:AtomicU64=AtomicU64::new(0);
struct Fixture{outer:PathBuf,bank:PathBuf}
impl Fixture{
 fn new()->Self{
  let outer=std::env::temp_dir().join(format!("moloch-prefix-fixture-{}-{}",std::process::id(),NEXT.fetch_add(1,Ordering::SeqCst)));fs::create_dir(&outer).unwrap();let bank=outer.join("bank");
  {let mut s=Session::open_live(&bank,&json!({"initialCash":"40.000000","maximumCommitted":"40.000000"}),&json!({"runId":"offline-fixture","availableCash":"40.000000","accountSnapshotHash":"0".repeat(64),"observedAt":"2026-10-15T00:00:00Z"})).unwrap();s.accept("fixture",1,&json!({"type":"tick","receivedAt":"2026-10-15T00:00:00Z"})).unwrap();s.accept("fixture",2,&json!({"type":"tick","receivedAt":"2026-10-15T00:00:01Z"})).unwrap();}
  Self{outer,bank}
 }
 fn digest(&self)->Vec<(String,String)>{let mut out:Vec<_>=fs::read_dir(&self.bank).unwrap().map(|e|{let p=e.unwrap().path();(p.file_name().unwrap().to_string_lossy().to_string(),format!("{:x}",Sha256::digest(fs::read(p).unwrap())))}).collect();out.sort();out}
}
impl Drop for Fixture{fn drop(&mut self){fs::remove_dir_all(&self.outer).unwrap();}}
#[test]fn complete_live_prefix_matches_all_financial_views_without_mutation(){let f=Fixture::new();let before=f.digest();let result=verifier::check(&f.bank).unwrap();assert_eq!(result["sequence"],2);assert_eq!(result["financialParity"],true);assert_eq!(result["executionAuthorized"],false);assert_eq!(result["accountRequests"],0);assert_eq!(f.digest(),before);}
#[test]fn truncated_or_altered_chain_is_rejected_without_repair(){let f=Fixture::new();let p=f.bank.join("events.jsonl");let mut bytes=fs::read(&p).unwrap();bytes.pop();fs::write(&p,&bytes).unwrap();let before=f.digest();assert!(verifier::check(&f.bank).is_err());assert_eq!(f.digest(),before);}
#[test]fn even_a_resealed_wrong_financial_report_fails_replay_parity(){let f=Fixture::new();let p=f.bank.join("report.json");let mut v:Value=serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();v.as_object_mut().unwrap().remove("hash");v["report"]["cash"]=json!("999.000000");let digest=format!("{:x}",Sha256::digest(serde_json::to_vec(&v).unwrap()));v["hash"]=json!(digest);fs::write(&p,serde_json::to_vec(&v).unwrap()).unwrap();let before=f.digest();assert!(verifier::check(&f.bank).is_err());assert_eq!(f.digest(),before);}
