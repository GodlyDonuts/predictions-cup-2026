//! One offline invocation replays all immutable funded checkpoints in-process.
//! No actor, API client, append handle, ownership lease or daemon is constructed.
use serde::Deserialize;
use serde_json::{Value as V,json};
use baal_portfolio_engine::{digest,canonical_hash,import_history};
use baal_service::PrefixBinding;
mod overlay_fast;
use sha2::{Digest,Sha256};
use std::{fs,io::Read,path::{Path,PathBuf},time::Instant};
type Result<T>=std::result::Result<T,String>;
fn need(ok:bool,c:&str)->Result<()>{if ok{Ok(())}else{Err(format!("RESTORE_REPLAY_{c}"))}}
fn hex(v:&str)->bool{v.len()==64&&v.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))}
fn hash(v:&[u8])->String{format!("{:x}",Sha256::digest(v))}
#[derive(Clone,Deserialize)]#[serde(rename_all="camelCase",deny_unknown_fields)]pub struct FilePin{pub path:PathBuf,pub sha256:String}
#[derive(Deserialize)]#[serde(rename_all="camelCase",deny_unknown_fields)]pub struct OverlayBinding{pub opening:FilePin,pub operator_journal:PrefixBinding,pub base_journal:PrefixBinding,pub operator_code:FilePin}
#[derive(Deserialize)]#[serde(rename_all="camelCase",deny_unknown_fields)]pub struct Config{
 pub bank:FilePin,pub bank_hash:String,pub journal:PrefixBinding,pub public_source:PrefixBinding,
 pub report_at:String,pub output:PathBuf,#[serde(default)]pub overlay:Option<OverlayBinding>,
 #[serde(default)]pub archived_report:Option<FilePin>,pub source_pins:Vec<FilePin>,
}
fn file(pin:&FilePin)->Result<Vec<u8>>{need(pin.path.is_absolute()&&hex(&pin.sha256),"ABSOLUTE_FILE_PIN")?;let bytes=fs::read(&pin.path).map_err(|_|"RESTORE_REPLAY_FILE_READ")?;need(hash(&bytes)==pin.sha256,"FILE_HASH")?;Ok(bytes)}
fn prefix(pin:&PrefixBinding)->Result<()>{need(pin.path.is_absolute()&&pin.bytes>0&&hex(&pin.sha256),"SEALED_SOURCE_PREFIX")?;Ok(())}
fn verify_bytes(pin:&PrefixBinding)->Result<()>{let f=fs::File::open(&pin.path).map_err(|_|"RESTORE_REPLAY_SOURCE_READ")?;need(f.metadata().map_err(|_|"RESTORE_REPLAY_SOURCE_METADATA")?.len()>=pin.bytes,"SOURCE_TRUNCATED_AFTER_REPLAY")?;let mut reader=f.take(pin.bytes);let mut sha=Sha256::new();let mut b=[0u8;65536];let mut n=0;loop{let count=reader.read(&mut b).map_err(|_|"RESTORE_REPLAY_SOURCE_READ")?;if count==0{break}n+=count as u64;sha.update(&b[..count]);}need(n==pin.bytes&&format!("{:x}",sha.finalize())==pin.sha256,"SOURCE_INTERIOR_CHANGED_AFTER_REPLAY")}
#[cfg(windows)]fn identity(path:&Path)->Result<String>{use std::os::windows::io::AsRawHandle;#[repr(C)]struct Info{attrs:u32,created:[u32;2],access:[u32;2],written:[u32;2],volume:u32,size_hi:u32,size_lo:u32,links:u32,index_hi:u32,index_lo:u32}#[link(name="kernel32")]unsafe extern "system"{fn GetFileInformationByHandle(handle:*mut std::ffi::c_void,info:*mut Info)->i32;}let f=fs::File::open(path).map_err(|_|"RESTORE_REPLAY_SOURCE_ID")?;let mut v=std::mem::MaybeUninit::<Info>::uninit();need(unsafe{GetFileInformationByHandle(f.as_raw_handle(),v.as_mut_ptr())}!=0,"SOURCE_ID")?;let v=unsafe{v.assume_init()};Ok(format!("{}:{}:{}",v.volume,v.index_hi,v.index_lo))}
#[cfg(not(windows))]fn identity(path:&Path)->Result<String>{use std::os::unix::fs::MetadataExt;let m=fs::metadata(path).map_err(|_|"RESTORE_REPLAY_SOURCE_ID")?;Ok(format!("{}:{}",m.dev(),m.ino()))}
pub fn compare_archived(actual:&V,archived:&V,expected_bank:&str)->Result<()>{need(archived["bankHash"]==expected_bank&&canonical_hash(&actual["wallets"])==canonical_hash(&archived["wallets"]),"ARCHIVED_EXACT_WALLETS")}
pub fn run(cfg:&Config)->Result<V>{
 let began=Instant::now();need(cfg.output.is_absolute()&&!cfg.output.exists()&&hex(&cfg.bank_hash)&&!cfg.source_pins.is_empty(),"OFFLINE_NEW_OUTPUT_AND_PINS")?;
 chrono::DateTime::parse_from_rfc3339(&cfg.report_at).map_err(|_|"RESTORE_REPLAY_REPORT_CLOCK")?;
 let bank_bytes=file(&cfg.bank)?;let bank:V=serde_json::from_slice(&bank_bytes).map_err(|_|"RESTORE_REPLAY_BANK_JSON")?;need(digest(&bank)==cfg.bank_hash,"ORIGINAL_BANK_HASH")?;
 for pin in &cfg.source_pins{file(pin)?;}prefix(&cfg.journal)?;prefix(&cfg.public_source)?;
 let mut identity_pins=vec![&cfg.journal,&cfg.public_source];if let Some(o)=&cfg.overlay{file(&o.opening)?;file(&o.operator_code)?;prefix(&o.operator_journal)?;prefix(&o.base_journal)?;identity_pins.push(&o.operator_journal);identity_pins.push(&o.base_journal);need(cfg.journal.path==o.operator_journal.path&&cfg.journal.bytes==o.operator_journal.bytes&&cfg.journal.sha256==o.operator_journal.sha256,"OVERLAY_AUTHORITATIVE_JOURNAL")?;}
 let identities=identity_pins.iter().map(|p|identity(&p.path)).collect::<Result<Vec<_>>>()?;
 let (state,report,evidence)=if let Some(o)=&cfg.overlay{
  let (engine,evidence)=overlay_fast::restore(bank.clone(),o,&cfg.public_source)?;
  (engine.state.clone(),engine.report(&cfg.report_at)?,evidence)
 }else{
  let(engine,evidence)=import_history(bank.clone(),&cfg.journal.path,cfg.journal.bytes,&cfg.journal.sha256,&cfg.public_source.path,cfg.public_source.bytes,&cfg.public_source.sha256)?;
  (engine.state().clone(),engine.report(&cfg.report_at)?,serde_json::to_value(evidence).map_err(|_|"RESTORE_REPLAY_EVIDENCE")?)
 };
 for(i,pin)in identity_pins.iter().enumerate(){need(identity(&pin.path)?==identities[i],"SOURCE_REPLACED_DURING_REPLAY")?;verify_bytes(pin)?;}
 if let Some(o)=&cfg.overlay{file(&o.opening)?;file(&o.operator_code)?;}
 need(file(&cfg.bank)?==bank_bytes,"BANK_CHANGED_DURING_REPLAY")?;for pin in &cfg.source_pins{file(pin)?;}
 let archived_verified=if let Some(pin)=&cfg.archived_report{let r:V=serde_json::from_slice(&file(pin)?).map_err(|_|"RESTORE_REPLAY_ARCHIVED_REPORT")?;compare_archived(&report,&r,&cfg.bank_hash)?;true}else{false};
 let executable=std::env::current_exe().map_err(|_|"RESTORE_REPLAY_BINARY")?;
 Ok(json!({"schemaVersion":1,"mode":"offline_native_funded_settlement_restore_replay_v1","elapsedMs":began.elapsed().as_secs_f64()*1000.,"binarySha256":hash(&fs::read(executable).map_err(|_|"RESTORE_REPLAY_BINARY")?),"originalBankFileSha256":cfg.bank.sha256,"originalBankHash":cfg.bank_hash,"originalCheckpointPrefix":{"bytes":cfg.journal.bytes,"sha256":cfg.journal.sha256},"originalPublicPrefix":{"bytes":cfg.public_source.bytes,"sha256":cfg.public_source.sha256},"sourcePins":cfg.source_pins.iter().map(|p|json!({"file":p.path.file_name().unwrap().to_string_lossy(),"sha256":p.sha256})).collect::<Vec<_>>(),"stateHash":digest(&state),"state":state,"report":report,"evidence":evidence,"archivedWalletParityVerified":archived_verified,"perBookProcessesSpawned":0,"originalFilesModified":false,"officialHoldingsAdopted":false,"networkRequests":0,"competitionOrders":0,"executionAuthority":false,"profitabilityEstablished":false}))
}
#[cfg(test)]mod tests{use super::*;
 #[test]fn archived_cash_cost_reservations_and_fills_cannot_be_erased(){let r=json!({"wallets":{"1000":{"cash":"99.645000","cost":"0.355000","positions":[{"marketId":"356","quantity":1}],"pending":[{"limit":"0.400000"}],"recentFills":[{"price":"0.355000"}]}}});let mut archived=r.clone();archived["bankHash"]=json!("B");assert!(compare_archived(&r,&archived,"B").is_ok());for key in ["cash","cost","positions","pending","recentFills"]{let mut bad=r.clone();bad["wallets"]["1000"][key]=V::Null;assert!(compare_archived(&bad,&archived,"B").is_err());}}
 #[test]fn wrong_bank_or_wallet_canonical_values_fail(){let r=json!({"wallets":{"3000":{"cash":"99.985000","realizedPnl":"-0.015000"}}});let mut a=r.clone();a["bankHash"]=json!("A");assert!(compare_archived(&r,&a,"B").is_err());let mut b=r.clone();b["wallets"]["3000"]["realizedPnl"]=json!(0);assert!(compare_archived(&b,&a,"A").is_err());}
 #[test]fn final_prefix_rehash_catches_same_file_same_size_interior_rewrite(){let p=std::env::temp_dir().join(format!("baal-restore-prefix-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));fs::write(&p,b"a\n").unwrap();let pin=PrefixBinding{path:p.clone(),bytes:2,sha256:hash(b"a\n")};let id=identity(&p).unwrap();assert!(verify_bytes(&pin).is_ok());fs::write(&p,b"b\n").unwrap();assert_eq!(identity(&p).unwrap(),id);assert!(verify_bytes(&pin).is_err());fs::remove_file(p).unwrap();}
}
