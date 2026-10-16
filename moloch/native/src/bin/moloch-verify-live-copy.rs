//! Offline COPY only: Session replay, no account client or upgrade approval.
use moloch_replay::session::{Session,engine_identity};
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::{collections::BTreeMap,fs::{self,File},io::{Read,BufReader},path::Path,time::Instant};
type Result<T> = std::result::Result<T,Box<dyn std::error::Error>>;
fn hash(v:&Value)->Result<String>{Ok(format!("{:x}",Sha256::digest(serde_json::to_vec(v)?)))}
fn file_hash(p:&Path)->Result<(u64,String)>{
 let m=fs::symlink_metadata(p)?;if !m.is_file()||m.file_type().is_symlink(){return Err("COPY_FILE_TYPE".into())}
 #[cfg(unix)] {use std::os::unix::fs::MetadataExt;if m.nlink()!=1{return Err("COPY_HARDLINK".into())}}
 let mut f=BufReader::new(File::open(p)?);let mut h=Sha256::new();let mut bytes=0u64;let mut b=[0u8;65536];
 loop{let n=f.read(&mut b)?;if n==0{break}h.update(&b[..n]);bytes+=n as u64;}Ok((bytes,format!("{:x}",h.finalize())))
}
fn read_json(p:&Path)->Result<Value>{if file_hash(p)?.0>256*1024*1024{return Err("COPY_METADATA_LIMIT".into())}Ok(serde_json::from_slice(&fs::read(p)?)?)}
fn sealed(p:&Path)->Result<Value>{let v=read_json(p)?;let mut body=v.clone();let expected=body.as_object_mut().ok_or("COPY_OBJECT")?.remove("hash").ok_or("COPY_HASH")?;if expected.as_str()!=Some(hash(&body)?.as_str()){return Err("COPY_SEAL".into())}Ok(v)}
fn check(root:&Path)->Result<Value>{
 let start=Instant::now();if !root.is_absolute(){return Err("COPY_ABSOLUTE_PATH_REQUIRED".into())}
 for p in root.ancestors(){if fs::symlink_metadata(p)?.file_type().is_symlink(){return Err("COPY_SYMLINK".into())}}
 let root=fs::canonicalize(root)?;let parts:Vec<_>=root.components().map(|c|c.as_os_str().to_string_lossy().to_lowercase()).collect();
 if parts.windows(2).any(|xs|xs==["shared","live-production-v1"]){return Err("PRODUCTION_BANK_REFUSED".into())}
 let marker_path=root.join(".moloch-offline-prefix-copy.json");let marker=read_json(&marker_path)?;
 #[cfg(unix)] {use std::os::unix::fs::{MetadataExt,PermissionsExt};let uid=fs::metadata("/proc/self")?.uid();let m=fs::metadata(&marker_path)?;let d=fs::metadata(&root)?;if m.uid()!=uid||d.uid()!=uid||m.permissions().mode()&0o077!=0||d.permissions().mode()&0o022!=0{return Err("COPY_OWNER_OR_PERMISSIONS".into())}}
 #[cfg(not(unix))] {return Err("COPY_UNIX_OWNERSHIP_REQUIRED".into())}
 if marker["kind"]!="moloch_offline_prefix_copy_v1"||marker["copyOnly"]!=true||marker["accountRequests"]!=0{return Err("COPY_MARKER".into())}
 // Reject every pre-existing symlink, including writer.lock, before Session opens it.
 for e in fs::read_dir(&root)?{let p=e?.path();let m=fs::symlink_metadata(&p)?;if m.file_type().is_symlink(){return Err("COPY_CHILD_SYMLINK".into())}if m.is_file(){file_hash(&p)?;}}
 let mut before=BTreeMap::new();for name in ["manifest.json","head.json","report.json","events.jsonl",".moloch-offline-prefix-copy.json"]{before.insert(name.to_owned(),file_hash(&root.join(name))?);}
 let bridge=root.join("engine-upgrade-v1.json");if bridge.exists(){before.insert("engine-upgrade-v1.json".into(),file_hash(&bridge)?);}
 let manifest=sealed(&root.join("manifest.json"))?;let head=sealed(&root.join("head.json"))?;let report=sealed(&root.join("report.json"))?;
 if marker["originalHead"]!=head||marker["reportSha256"]!=before["report.json"].1||marker["manifestSha256"]!=before["manifest.json"].1||marker["prefixBytes"]!=before["events.jsonl"].0||marker["prefixSha256"]!=before["events.jsonl"].1{return Err("COPY_BOUNDARY".into())}
 let bridge_hash=before.get("engine-upgrade-v1.json").map(|x|json!(x.1)).unwrap_or(Value::Null);
 if marker["bridgeSha256"]!=bridge_hash||(bridge_hash.is_null()&&manifest["engineIdentity"]!=engine_identity()){return Err("COPY_BRIDGE_BOUNDARY".into())}
 if manifest["mode"]!="rust_live_session"||manifest["seed"]["kind"]!="actual_account_allocation"||head["manifestHash"]!=manifest["hash"]||report["manifestHash"]!=manifest["hash"]||head["sequence"]!=report["sequence"]||head["previous"]!=report["previous"]{return Err("COPY_SCOPE".into())}
 let session=Session::open_live(&root,&manifest["policy"],&manifest["seed"]["evidence"])?;
 let actual=session.engine().report()?;let orders=json!(session.engine().order_records());let fills=json!(session.engine().fills());
 let rebuilt=sealed(&root.join("report.json"))?;
 if session.durable_head()!=head||actual!=report["report"]||orders!=report["orders"]||fills!=report["fills"]||rebuilt!=report||report["contestOrders"].as_u64()!=Some(session.engine().live_order_bindings().len() as u64){return Err("COPY_FINANCIAL_PARITY".into())}
 drop(session);
 for (name,original) in &before{if file_hash(&root.join(name))?!=*original{return Err("COPY_SOURCE_MUTATED".into())}}
 Ok(json!({"schemaVersion":1,"mode":"offline_copy_live_prefix_equivalence","engineIdentity":engine_identity(),"sequence":head["sequence"],"previous":head["previous"],"prefixBytes":before["events.jsonl"].0,"prefixSha256":before["events.jsonl"].1,"financialReportSha256":hash(&actual)?,"orderRecordsSha256":hash(&orders)?,"fillsSha256":hash(&fills)?,"sourceCursorsSha256":hash(&report["sourceCursors"])?,"elapsedMs":start.elapsed().as_millis(),"financialParity":true,"sourceMutated":false,"originalSourceUnchanged":true,"executionAuthorized":false,"accountRequests":0,"copyOnly":true,"manifestSha256":before["manifest.json"].1,"headSha256":before["head.json"].1,"reportSha256":before["report.json"].1,"bridgeSha256":bridge_hash}))
}
fn main()->Result<()>{let args:Vec<_>=std::env::args().collect();if args.len()!=2{return Err("usage: moloch-verify-live-copy ABSOLUTE_FROZEN_COPY".into())}println!("{}",check(Path::new(&args[1]))?);Ok(())}
#[cfg(all(test,unix))]
mod tests{
 use super::*;use std::{os::unix::fs::PermissionsExt,time::{SystemTime,UNIX_EPOCH}};
 struct Fixture(std::path::PathBuf);impl Drop for Fixture{fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}
 fn fixture()->Fixture{
  let p=std::env::temp_dir().join(format!("moloch-copy-{}-{}",std::process::id(),SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));fs::create_dir(&p).unwrap();fs::set_permissions(&p,fs::Permissions::from_mode(0o700)).unwrap();
  let seed=json!({"runId":"copy-fixture","availableCash":"1000.000000","accountSnapshotHash":"a".repeat(64),"observedAt":"2026-10-13T00:00:00Z"});let mut s=Session::open_live(&p,&json!({"quantity":1}),&seed).unwrap();
  let golden:Value=serde_json::from_str(include_str!("../../tests/reference/golden.json")).unwrap();s.accept("books",1,&golden[0]["steps"][0]["event"]).unwrap();drop(s);
  let m=json!({"kind":"moloch_offline_prefix_copy_v1","copyOnly":true,"accountRequests":0,"originalHead":sealed(&p.join("head.json")).unwrap(),"manifestSha256":file_hash(&p.join("manifest.json")).unwrap().1,"reportSha256":file_hash(&p.join("report.json")).unwrap().1,"prefixBytes":file_hash(&p.join("events.jsonl")).unwrap().0,"prefixSha256":file_hash(&p.join("events.jsonl")).unwrap().1,"bridgeSha256":null});
  let mp=p.join(".moloch-offline-prefix-copy.json");fs::write(&mp,serde_json::to_vec(&m).unwrap()).unwrap();fs::set_permissions(mp,fs::Permissions::from_mode(0o600)).unwrap();Fixture(p)
 }
 #[test]fn genuine_session_copy_parity_and_unchanged_bytes(){let f=fixture();let p=check(&f.0).unwrap();assert_eq!(p["financialParity"],true);assert_eq!(p["sequence"],1);assert_eq!(p["copyOnly"],true);assert_eq!(p["accountRequests"],0);assert_eq!(check(&f.0).unwrap()["prefixSha256"],p["prefixSha256"]);}
 #[test]fn changed_journal_and_marker_are_not_accepted(){let f=fixture();fs::write(f.0.join("events.jsonl"),b"{}\n").unwrap();assert!(check(&f.0).unwrap_err().to_string().contains("COPY_BOUNDARY"));let g=fixture();fs::set_permissions(g.0.join(".moloch-offline-prefix-copy.json"),fs::Permissions::from_mode(0o644)).unwrap();assert!(check(&g.0).unwrap_err().to_string().contains("COPY_OWNER"));}
 #[test]fn hardlinked_lock_and_symlink_refused(){let f=fixture();fs::rename(f.0.join("writer.lock"),f.0.join("original.lock")).unwrap();fs::hard_link(f.0.join("original.lock"),f.0.join("writer.lock")).unwrap();assert!(check(&f.0).unwrap_err().to_string().contains("COPY_HARDLINK"));let g=fixture();std::os::unix::fs::symlink(g.0.join("manifest.json"),g.0.join("foreign-link")).unwrap();assert!(check(&g.0).unwrap_err().to_string().contains("COPY_CHILD_SYMLINK"));}
 #[test]fn production_path_and_relative_refused(){assert!(check(Path::new("relative")).unwrap_err().to_string().contains("ABSOLUTE"));let f=fixture();let p=f.0.join("shared/live-production-v1");fs::create_dir_all(&p).unwrap();assert!(check(&p).unwrap_err().to_string().contains("PRODUCTION_BANK_REFUSED"));}
}
