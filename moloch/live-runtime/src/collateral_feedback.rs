//! Sole-worker admission for actual collateral ledger transitions. This module
//! creates no client and performs no network request. Callers supply the actual
//! worker Transport, held owner lease and original configured dispatch logs.
//! Receipt hashes prove integrity/membership, not independent authentication.
use crate::{dispatch::DispatchLog, feedback, package_dispatch::PackageDispatchLog};
use baal_transport_service::{Receipt, Transport};
use moloch_replay::session::{ExclusiveLock, Session};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::{BTreeMap, BTreeSet}, fs::{self, File}, io::{BufRead, BufReader, Read, Seek, SeekFrom}, path::{Path, PathBuf}};
pub type Result<T> = feedback::Result<T>;
const MAX_RECEIPT:u64=8*1024*1024;
const MAX_JOURNAL:u64=256*1024*1024;
const SOURCE:&str="moloch-collateral-owner";
fn need(ok:bool,why:&'static str)->Result<()>{if ok{Ok(())}else{Err(why.into())}}
fn hash<T:serde::Serialize>(v:&T)->Result<String>{Ok(raw_hash(&serde_json::to_vec(v)?))}
fn raw_hash(b:&[u8])->String{format!("{:x}",Sha256::digest(b))}
fn clock(s:&str)->Result<i64>{Ok(chrono::DateTime::parse_from_rfc3339(s)?.timestamp_millis())}
fn id(v:&Value)->Result<String>{let s=v.as_str().map(str::to_owned).or_else(||v.as_u64().map(|n|n.to_string())).ok_or("COLLATERAL_ID")?;need(!s.is_empty()&&!s.starts_with('0')&&s.bytes().all(|b|b.is_ascii_digit()),"COLLATERAL_ID")?;Ok(s)}
fn metadata(m:&fs::Metadata)->Result<Value>{
 #[cfg(unix)]{use std::os::unix::fs::MetadataExt;need(m.is_file(),"COLLATERAL_REGULAR_FILE")?;Ok(json!({"schemaVersion":1,"device":m.dev(),"inode":m.ino(),"bytes":m.len()}))}
 #[cfg(not(unix))]{let _=m;Err("COLLATERAL_DESCRIPTOR_UNSUPPORTED".into())}
}
fn no_links(path:&Path)->Result<()>{for p in path.ancestors(){need(!fs::symlink_metadata(p)?.file_type().is_symlink(),"COLLATERAL_SYMLINK")?;}Ok(())}
fn pin(path:&Path,held:&Value)->Result<()> {
 no_links(path)?;let named=metadata(&fs::metadata(path)?)?;
 for k in ["schemaVersion","device","inode","bytes"]{need(named[k]==held[k],"COLLATERAL_DESCRIPTOR_CHANGED")?;}Ok(())
}
fn bounded(file:&File,maximum:u64)->Result<Vec<u8>>{let mut f=file.try_clone()?;f.seek(SeekFrom::Start(0))?;let mut b=Vec::new();f.take(maximum+1).read_to_end(&mut b)?;need(b.len() as u64<=maximum,"COLLATERAL_SOURCE_SIZE")?;Ok(b)}

/// Only these two original worker receipt stores are accepted. No evidence-
/// selected arbitrary path or JSON constructor is available.
pub enum ReceiptStore { Ordinary, Package }
pub struct RetainedReceipt {file:File,path:PathBuf,root:PathBuf,identity:Value,raw_sha256:String,receipt:Receipt}
impl RetainedReceipt {
 /// Digest must come from an already admitted bank fill or the original held
 /// dispatch record, never an API-selected file path. The filename and decoded
 /// original Receipt must agree exactly.
 pub fn from_recorded_digest(root:&Path,digest:&str,store:ReceiptStore)->Result<Self>{
  need(digest.len()==64&&digest.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b)),"COLLATERAL_SOURCE_DIGEST")?;
  let path=match &store{ReceiptStore::Ordinary=>root.join("receipts").join(format!("{digest}.json")),ReceiptStore::Package=>root.join("package-receipts").join(format!("receipt-{digest}.json"))};no_links(&path)?;let file=File::open(&path)?;let raw=bounded(&file,MAX_RECEIPT)?;let receipt:Receipt=serde_json::from_slice(&raw)?;
  need(feedback::digest(&receipt)?==digest,"COLLATERAL_SOURCE_DIGEST")?;Self::open(root,&receipt,store)
 }
 pub fn receipt(&self)->&Receipt{&self.receipt}
 pub(crate) fn verify_root(&self,root:&Path)->Result<()>{need(self.root==root.canonicalize()?,"COLLATERAL_RECEIPT_OTHER_ROOT")?;self.verify()}
 pub fn open(root:&Path,receipt:&Receipt,store:ReceiptStore)->Result<Self>{
  no_links(root)?;let root=root.canonicalize()?;let digest=feedback::digest(receipt)?;
  let path=match store{ReceiptStore::Ordinary=>root.join("receipts").join(format!("{digest}.json")),ReceiptStore::Package=>root.join("package-receipts").join(format!("receipt-{digest}.json"))};
  no_links(&path)?;let file=File::open(&path)?;let identity=metadata(&file.metadata()?)?;pin(&path,&identity)?;
  let raw=bounded(&file,MAX_RECEIPT)?;let value:Value=serde_json::from_slice(&raw)?;
  need(value==serde_json::to_value(receipt)?,"COLLATERAL_ORIGINAL_RECEIPT_CHANGED")?;
  let r=Self{file,path,root,identity,raw_sha256:raw_hash(&raw),receipt:receipt.clone()};r.verify()?;Ok(r)
 }
 pub(crate) fn verify(&self)->Result<()>{pin(&self.path,&self.identity)?;need(metadata(&self.file.metadata()?)?==self.identity&&raw_hash(&bounded(&self.file,MAX_RECEIPT)?)==self.raw_sha256,"COLLATERAL_ORIGINAL_RECEIPT_CHANGED")?;pin(&self.path,&self.identity)}
 pub(crate) fn evidence(&self)->Result<Value>{Ok(json!({"receiptSha256":feedback::digest(&self.receipt)?,"retainedBytesSha256":self.raw_sha256,"descriptor":self.identity}))}
}
#[derive(Clone,Copy)]pub struct Snapshot<'a>{pub account:&'a RetainedReceipt,pub collateral:&'a RetainedReceipt}
impl Snapshot<'_>{fn value(&self)->Value{json!({"account":self.account.receipt,"collateral":self.collateral.receipt})}}
pub enum Operation<'a>{
 Baseline{after:Snapshot<'a>},
 Checkpoint{before:Snapshot<'a>,after:Snapshot<'a>},
 Execution{before:Snapshot<'a>,after:Snapshot<'a>,acknowledgments:&'a [&'a RetainedReceipt],fill_tapes:&'a [&'a RetainedReceipt]},
 OrdinaryCashSync{before:Snapshot<'a>,after:Snapshot<'a>,acknowledgments:&'a [&'a RetainedReceipt],fill_tapes:&'a [&'a RetainedReceipt]},
 Settlement{before:Snapshot<'a>,after:Snapshot<'a>,settlements:&'a RetainedReceipt,settlement_ids:&'a [String]},
}
/// Paths come from the worker's immutable control, never from receipt/evidence
/// JSON. These must borrow its ORIGINAL held objects; opening a copy is not a
/// new source of execution authority. Transport rechecks its actual authority.
pub struct Boundary<'a>{pub root:&'a Path,pub owner_lock_path:&'a Path,pub owner_lease:&'a ExclusiveLock,pub transport:&'a Transport,pub ordinary:&'a DispatchLog,pub packages:&'a PackageDispatchLog}
#[derive(Clone)]
struct Journal {sha256:String,acks:BTreeSet<String>,attempted:BTreeSet<u64>,closed:BTreeSet<u64>,acknowledged:BTreeSet<u64>,aborted:BTreeMap<u64,Value>,local_final_guard:BTreeMap<u64,Value>,expired_exposure:BTreeMap<u64,Value>}
// Only parsing is reused. Every invocation independently SHA-verifies every
// original byte, so same-size edits and restored timestamps cannot hit cache.
struct VerifiedJournal{path:PathBuf,held:Value,package:bool,journal:Journal}
thread_local!{static VERIFIED_JOURNALS:std::cell::RefCell<Vec<VerifiedJournal>>=const{std::cell::RefCell::new(Vec::new())};}
#[cfg(test)]thread_local!{static JOURNAL_PARSE_COUNT:std::cell::Cell<usize>=const{std::cell::Cell::new(0)};}
fn exact_expired_resolution(log:&DispatchLog,rows:&BTreeMap<u64,Value>,qid:u64)->bool{log.exposure_resolved(qid)&&rows.contains_key(&qid)&&rows.get(&qid)==log.exposure_records().get(&qid)}
#[cfg(test)]pub(crate) fn test_exact_expired_resolution(log:&DispatchLog,rows:&BTreeMap<u64,Value>,qid:u64)->bool{exact_expired_resolution(log,rows,qid)}
/// Stream the original retained chain with bounded rows; compare its actual
/// head to the held object before and after scanning. Never repairs a tail.
fn journal(path:&Path,held:&Value,package:bool)->Result<Journal>{
 pin(path,held)?;need(held["bytes"].as_u64().is_some_and(|n|n<=MAX_JOURNAL),"COLLATERAL_JOURNAL_SIZE")?;
 let mut f=File::open(path)?;let opened=metadata(&f.metadata()?)?;for k in ["schemaVersion","device","inode","bytes"]{need(opened[k]==held[k],"COLLATERAL_DESCRIPTOR_CHANGED")?;}pin(path,&opened)?;
 let mut raw_digest=Sha256::new();let mut raw_size=0u64;let mut chunk=[0u8;65536];loop{let n=f.read(&mut chunk)?;if n==0{break}raw_size=raw_size.checked_add(n as u64).ok_or("COLLATERAL_RANGE")?;need(raw_size<=MAX_JOURNAL,"COLLATERAL_JOURNAL_SIZE")?;raw_digest.update(&chunk[..n]);}
 need(held["bytes"]==raw_size,"COLLATERAL_JOURNAL_HEAD")?;pin(path,held)?;let raw_sha=format!("{:x}",raw_digest.finalize());
 if let Some(checked)=VERIFIED_JOURNALS.with(|c|c.borrow().iter().find(|c|c.path==path&&c.held==*held&&c.package==package&&c.journal.sha256==raw_sha).map(|c|c.journal.clone())){return Ok(checked)}
 f.seek(SeekFrom::Start(0))?;
 #[cfg(test)]JOURNAL_PARSE_COUNT.with(|c|c.set(c.get()+1));
 let mut r=BufReader::new(f);let(mut seq,mut previous)=(0u64,"0".repeat(64));let mut digest=Sha256::new();let mut size=0u64;
 let mut out=Journal{sha256:String::new(),acks:BTreeSet::new(),attempted:BTreeSet::new(),closed:BTreeSet::new(),acknowledged:BTreeSet::new(),aborted:BTreeMap::new(),local_final_guard:BTreeMap::new(),expired_exposure:BTreeMap::new()};
 loop{let mut row=Vec::new();let n=Read::take(&mut r,MAX_RECEIPT+1).read_until(b'\n',&mut row)?;if n==0{break}need(n as u64<=MAX_RECEIPT&&row.last()==Some(&b'\n'),"COLLATERAL_JOURNAL_TAIL_OR_ROW")?;size=size.checked_add(n as u64).ok_or("COLLATERAL_RANGE")?;need(size<=MAX_JOURNAL,"COLLATERAL_JOURNAL_SIZE")?;digest.update(&row);
  let mut v:Value=serde_json::from_slice(&row)?;let claimed=v.as_object_mut().ok_or("COLLATERAL_JOURNAL_ROW")?.remove("hash").ok_or("COLLATERAL_JOURNAL_HASH")?;
  need(v["sequence"]==seq+1&&v["previous"]==previous&&claimed==hash(&v)?,"COLLATERAL_JOURNAL_CHAIN")?;seq+=1;previous=claimed.as_str().ok_or("COLLATERAL_JOURNAL_HASH")?.into();let e=&v["event"];
  if e["kind"]=="ack"{let receipt:Receipt=serde_json::from_value(e["receipt"].clone())?;out.acks.insert(feedback::digest(&receipt)?);}
  if !package{let q=e["qid"].as_u64().ok_or("COLLATERAL_JOURNAL_QID")?;match e["kind"].as_str(){Some("attempt")=>{out.attempted.insert(q);},Some("ack"|"recovered_ack")=>{out.acknowledged.insert(q);},Some("closed")=>{out.closed.insert(q);},Some("aborted_before_admission")=>{out.aborted.insert(q,e.clone());},Some("local_final_guard_no_http")=>{out.local_final_guard.insert(q,e.clone());},Some("expired_exposure_reconciled")=>{out.expired_exposure.insert(q,e.clone());},_=>{}}}
 }
 need(seq==held["sequence"].as_u64().ok_or("COLLATERAL_JOURNAL_HEAD")?&&held["previous"]==previous&&held["bytes"]==size,"COLLATERAL_JOURNAL_HEAD")?;
 pin(path,held)?;out.sha256=format!("{:x}",digest.finalize());need(out.sha256==raw_sha,"COLLATERAL_JOURNAL_CHANGED_DURING_READ")?;
 VERIFIED_JOURNALS.with(|c|{let mut c=c.borrow_mut();if c.len()==4{c.remove(0);}c.push(VerifiedJournal{path:path.into(),held:held.clone(),package,journal:out.clone()});});Ok(out)
}
fn capture(bank:&Session,b:&Boundary<'_>,at:i64)->Result<Value>{capture_checked(bank,b,at,false)}
/// Allocation only: original positively verified no-network terminal audit may
/// coexist with closed physical history. Existing collateral semantics unchanged.
pub(crate) fn allocation_boundary(bank:&Session,b:&Boundary<'_>,at:i64)->Result<Value>{capture_checked(bank,b,at,true)}
fn capture_checked(bank:&Session,b:&Boundary<'_>,at:i64,allow_proved:bool)->Result<Value>{
 no_links(b.root)?;let root=b.root.canonicalize()?;let queue=b.transport.package_queue_quiet()?;
 need(queue.state.scope==b.transport.account_scope()&&queue.state.last_clock<=u64::try_from(at)?&&queue.global_unknown_count==0&&!queue.has_unknown&&queue.state.active.is_none()&&!queue.state.manual_review,"COLLATERAL_QUEUE_BOUNDARY")?;
 let lease=b.owner_lease.descriptor_identity()?;pin(b.owner_lock_path,&lease)?;let lease_file=File::open(b.owner_lock_path)?;let lease_bytes=bounded(&lease_file,4096)?;let lease_json:Value=serde_json::from_slice(&lease_bytes)?;
 need(lease_json["kind"]=="moloch_rust_os_lock"&&lease_json["pid"]==std::process::id(),"COLLATERAL_OWNER_LEASE")?;pin(b.owner_lock_path,&lease)?;
 let ordinary=b.ordinary.descriptor_identity()?;let packages=b.packages.descriptor_identity()?;
 let o=journal(&root.join("dispatch.ndjson"),&ordinary,false)?;let p=journal(&root.join("package-dispatch/package-dispatch.ndjson"),&packages,true)?;
 b.ordinary.require_resolved_attempts()?;need(o.attempted==b.ordinary.attempted&&o.acknowledged==b.ordinary.acknowledgments.keys().copied().collect()&&o.closed==b.ordinary.closed,"COLLATERAL_DISPATCH_PROJECTION")?;
 let mut allocation_aborted=BTreeSet::new();let mut allocation_expired=BTreeSet::new();
 if allow_proved{need(o.aborted==*b.ordinary.aborted_records(),"CAPITAL_ABORT_PROJECTION")?;need(o.local_final_guard==*b.ordinary.local_final_guard_records(),"CAPITAL_LOCAL_GUARD_PROJECTION")?;for q in o.attempted.difference(&o.closed){
  let original=bank.engine().order_records().into_iter().find(|r|r["id"]==*q).ok_or("CAPITAL_ABORT_NATIVE_QUOTE")?;
  need(original["retired"]==true&&original["filled"]==0&&!b.ordinary.acknowledgments.contains_key(q)&&!bank.engine().live_order_bindings().contains_key(q)&&!bank.engine().fills().iter().any(|f|f["orderId"]==*q),"CAPITAL_UNVERIFIED_ABORT")?;
  if b.ordinary.proved_no_network(*q)&&(o.aborted.contains_key(q)||o.local_final_guard.contains_key(q)){allocation_aborted.insert(*q);}else{need(exact_expired_resolution(b.ordinary,&o.expired_exposure,*q),"CAPITAL_UNVERIFIED_EXPIRED_EXPOSURE")?;allocation_expired.insert(*q);}
 }}
 need(o.attempted.iter().all(|q|o.closed.contains(q)||allocation_aborted.contains(q)||allocation_expired.contains(q))&&!b.packages.unresolved()&&!bank.engine().package_blocks_new_submissions(),"COLLATERAL_UNRESOLVED_EXECUTION")?;
 let pending:Vec<_>=bank.engine().order_records().into_iter().filter(|q|q["retired"]==false).map(|q|q["id"].as_u64().ok_or("COLLATERAL_QUOTE_ID")).collect::<std::result::Result<_,_>>()?;
 need(pending.iter().all(|q|!o.attempted.contains(q)&&b.ordinary.never_attempted(*q)&&!bank.engine().live_order_bindings().contains_key(q)),"COLLATERAL_UNSUBMITTED_PROOF")?;
 need(ordinary==b.ordinary.descriptor_identity()?&&packages==b.packages.descriptor_identity()?,"COLLATERAL_DISPATCH_CHANGED")?;
 let seed=bank.live_account_seed().ok_or("COLLATERAL_LIVE_BANK")?;
 let captured=json!({"root":root,"scope":b.transport.scope(),"accountScope":b.transport.account_scope(),"runId":seed["runId"],"manifestHash":bank.manifest_hash(),"reportHash":hash(&bank.engine().report()?)?,"nativeHead":bank.durable_head(),"ordinaryDescriptor":ordinary,"packageDescriptor":packages,"ordinarySha256":o.sha256,"packageSha256":p.sha256,"originalAckReceiptHashes":o.acks.union(&p.acks).collect::<Vec<_>>(),"queue":queue,"leaseDescriptor":lease,"ownerLeaseSha256":raw_hash(&lease_bytes),"unsubmitted":pending});
 if allow_proved{let mut c=captured;c["provedNoNetworkOrderIds"]=json!(allocation_aborted);if !allocation_expired.is_empty(){c["expiredExposureOrderIds"]=json!(allocation_expired);}Ok(c)}else{Ok(captured)}
}
fn receipt_clocks(r:&Receipt,at:i64)->Result<()> {let start=clock(&r.request_started_at)?;let end=clock(&r.received_at)?;need(r.status==200&&start>=0&&start<=end&&end<=at,"COLLATERAL_RECEIPT_CLOCK_OR_STATUS")}
#[cfg(all(test,unix))]mod verified_journal_tests{
 use super::*;use std::io::Write;
 struct Temp(PathBuf);impl Drop for Temp{fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}
 fn fixture()->(Temp,PathBuf,DispatchLog){
  let p=std::env::temp_dir().join(format!("verified-dispatch-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));fs::create_dir(&p).unwrap();let path=p.join("dispatch.ndjson");let log=DispatchLog::open(&path).unwrap();VERIFIED_JOURNALS.with(|c|c.borrow_mut().clear());JOURNAL_PARSE_COUNT.with(|c|c.set(0));(Temp(p),path,log)
 }
 fn append(log:&mut DispatchLog,qid:u64){let proof=log.proof(qid);log.append(json!({"kind":"not_sent","qid":qid,"proof":proof})).unwrap();}
 #[test]fn unchanged_bytes_reuse_parsing_but_new_original_head_is_fully_validated(){
  let(_t,path,mut log)=fixture();append(&mut log,1);let first=log.descriptor_identity().unwrap();let a=journal(&path,&first,false).unwrap();let b=journal(&path,&first,false).unwrap();assert_eq!(a.sha256,b.sha256);assert_eq!(JOURNAL_PARSE_COUNT.with(|c|c.get()),1);
  append(&mut log,2);assert!(journal(&path,&first,false).is_err());let next=log.descriptor_identity().unwrap();let changed=journal(&path,&next,false).unwrap();assert_ne!(changed.sha256,a.sha256);assert_eq!(JOURNAL_PARSE_COUNT.with(|c|c.get()),2);assert_eq!(changed.attempted,log.attempted);assert_eq!(changed.closed,log.closed);
 }
 #[test]fn same_size_corruption_with_restored_mtime_never_reuses_validated_history(){
  let(_t,path,mut log)=fixture();append(&mut log,1);let held=log.descriptor_identity().unwrap();let original=fs::read(&path).unwrap();let modified=fs::metadata(&path).unwrap().modified().unwrap();journal(&path,&held,false).unwrap();let mut changed=original.clone();let marker=b"\"qid\":1";let at=changed.windows(marker.len()).position(|s|s==marker).unwrap()+marker.len()-1;changed[at]=b'9';
  let mut f=fs::OpenOptions::new().write(true).open(&path).unwrap();f.write_all(&changed).unwrap();f.sync_all().unwrap();f.set_times(fs::FileTimes::new().set_modified(modified)).unwrap();assert_eq!(fs::metadata(&path).unwrap().len(),original.len()as u64);assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(),modified);
  assert!(journal(&path,&held,false).err().expect("changed original chain must fail").to_string().contains("COLLATERAL_JOURNAL_CHAIN"));assert_eq!(JOURNAL_PARSE_COUNT.with(|c|c.get()),2);
 }
}
fn sources<'a>(op:&Operation<'a>)->Vec<&'a RetainedReceipt>{let mut v=Vec::new();match op{Operation::Baseline{after}=>v.extend([after.account,after.collateral]),Operation::Checkpoint{before,after}=>v.extend([before.account,before.collateral,after.account,after.collateral]),Operation::Execution{before,after,acknowledgments,fill_tapes}|Operation::OrdinaryCashSync{before,after,acknowledgments,fill_tapes}=>{v.extend([before.account,before.collateral,after.account,after.collateral]);v.extend_from_slice(acknowledgments);v.extend_from_slice(fill_tapes);},Operation::Settlement{before,after,settlements,..}=>v.extend([before.account,before.collateral,after.account,after.collateral,*settlements])}v}
fn same_fill(native:&Value,event:&Value)->Result<bool>{
 for(n,e)in [("orderId","orderId"),("actualOrderId","actualOrderId"),("sourceId","fillId"),("exchangeId","exchangeId"),("marketId","marketId"),("side","side"),("action","action"),("quantity","quantity"),("price","price"),("fee","fee")]{if native[n]!=event[e]{return Ok(false)}}
 Ok(chrono::DateTime::parse_from_rfc3339(native["executedAt"].as_str().ok_or("COLLATERAL_FILL_CLOCK")?)?==chrono::DateTime::parse_from_rfc3339(event["executedAt"].as_str().ok_or("COLLATERAL_FILL_CLOCK")?)?)
}
fn verify_execution(bank:&Session,b:&Boundary<'_>,acks:&[&RetainedReceipt],tapes:&[&RetainedReceipt],after:Snapshot<'_>,captured:&Value,processed:&str)->Result<()> {
 need(!acks.is_empty()&&acks.len()<=16&&!tapes.is_empty()&&tapes.len()<=256,"COLLATERAL_EXECUTION_EVIDENCE_COUNT")?;
 let mut ids=BTreeSet::new();for r in acks{let digest=feedback::digest(&r.receipt)?;need(captured["originalAckReceiptHashes"].as_array().is_some_and(|a|a.iter().any(|x|x==&digest)),"COLLATERAL_ACK_NOT_ORIGINAL_DISPATCH")?;
  let rows:Vec<&Value>=match r.receipt.endpoint.as_str(){"/orders"=>vec![&r.receipt.data],"/orders/multi-leg"=>r.receipt.data["results"].as_array().ok_or("COLLATERAL_ACK_ROWS")?.iter().map(|v|&v["data"]).collect(),_=>return Err("COLLATERAL_ACK_ENDPOINT".into())};for raw in rows{need(ids.insert(id(&raw["orderId"])?),"COLLATERAL_DUPLICATE_ACK")?;}}
 let records=bank.engine().order_records();let native:Vec<_>=bank.engine().fills().iter().filter(|f|f["actualOrderId"].as_str().is_some_and(|id|ids.contains(id))).collect();need(!native.is_empty(),"COLLATERAL_NO_ACTUAL_FILLS")?;
 let after_start=clock(&after.account.receipt.request_started_at)?.min(clock(&after.collateral.receipt.request_started_at)?);let mut proven=BTreeSet::new();
 for tape in tapes{let r=&tape.receipt;let actual=id(&r.data["orderId"])?;need(ids.contains(&actual)&&r.endpoint==format!("/orders/{actual}/fills?limit=200&tournamentId={}",b.transport.scope().tournament_id)&&clock(&r.received_at)?<=after_start,"COLLATERAL_FILL_TAPE_SCOPE_OR_CLOCK")?;
  let qid=*bank.engine().live_order_bindings().iter().find(|(_,v)|**v==actual).ok_or("COLLATERAL_UNOWNED_ACTUAL_ID")?.0;let q=records.iter().find(|q|q["id"]==qid).ok_or("COLLATERAL_QUOTE_ID")?;
  let events=feedback::fills(q,&actual,&b.transport.scope().tournament_id,r,processed)?;let digest=feedback::digest(r)?;
  for e in events{let mut matched=false;for(i,f)in native.iter().enumerate(){if same_fill(f,&e)?{matched=true;if f["receiptHash"]==digest{proven.insert(i);}}}need(matched,"COLLATERAL_TAPE_NOT_NATIVE_FILL")?;}
 }
 need(proven.len()==native.len(),"COLLATERAL_ORIGINAL_NATIVE_FILL_SOURCE_MISSING")
}
/// Not deserializable/cloneable and exposes no mutable event. A prepared
/// transition is consumed by commit, which rechecks all original sources.
pub struct Transition<'a>{event:Value,boundary:Value,receipts:Vec<&'a RetainedReceipt>,processed:String}
pub struct Committed {pub source_sequence:u64,pub event_sha256:String,pub durable_head:Value,pub report:Value}
pub fn prepare<'a>(bank:&Session,b:&Boundary<'_>,op:Operation<'a>,processed:&str)->Result<Transition<'a>>{
 let at=clock(processed)?;let captured=capture(bank,b,at)?;let receipts=sources(&op);let root=b.root.canonicalize()?;
 for r in &receipts{need(r.root==root,"COLLATERAL_RECEIPT_OTHER_ROOT")?;r.verify()?;receipt_clocks(&r.receipt,at)?;}
 if let Operation::Baseline{after}=&op{let balance=&after.account.receipt.data["balance"];let text=balance.as_str().map(str::to_owned).unwrap_or_else(||balance.to_string());need(baal_transport_service::decimal_units(&text)?==crate::authority::reconciled_expected_cash(bank)?,"COLLATERAL_BASELINE_CASH_MISMATCH")?;}
 let(kind,before,after)=match &op{Operation::Baseline{after}=>("actual_collateral_baseline",None,*after),Operation::Checkpoint{before,after}=>("actual_collateral_checkpoint",Some(*before),*after),Operation::Execution{before,after,..}=>("actual_collateral_execution",Some(*before),*after),Operation::OrdinaryCashSync{before,after,..}=>("actual_collateral_ordinary_cash_sync",Some(*before),*after),Operation::Settlement{before,after,..}=>("actual_collateral_settlement",Some(*before),*after)};
 let scope=b.transport.scope();let mut event=json!({"type":kind,"source":"official_sig_api_v1","receivedAt":processed,"processedAt":processed,"bankScope":{"runId":captured["runId"],"bankManifestHash":bank.manifest_hash(),"tournamentId":scope.tournament_id,"tournamentSlug":scope.tournament_slug,"profileId":after.account.receipt.data["id"]},"expectedReportHash":captured["reportHash"],"boundary":{"nativeHead":captured["nativeHead"],"ordinaryDispatchHash":captured["ordinarySha256"],"packageDispatchHash":captured["packageSha256"],"queueHeadHash":hash(&captured["queue"])?,"ownerLeaseHash":captured["ownerLeaseSha256"],"unknownCount":0,"activeRequest":null,"unsubmittedOrderIds":captured["unsubmitted"]},"after":after.value()});
 if let Some(before)=before{event["before"]=before.value();}
 match &op{Operation::Baseline{..}|Operation::Checkpoint{..}=>{},Operation::Execution{acknowledgments,fill_tapes,after,..}|Operation::OrdinaryCashSync{acknowledgments,fill_tapes,after,..}=>{verify_execution(bank,b,acknowledgments,fill_tapes,*after,&captured,processed)?;if kind=="actual_collateral_ordinary_cash_sync"{event["acknowledgments"]=json!(acknowledgments.iter().map(|r|&r.receipt).collect::<Vec<_>>());event["fillTapes"]=json!(fill_tapes.iter().map(|r|&r.receipt).collect::<Vec<_>>());}else{event["receipts"]=json!(acknowledgments.iter().map(|r|&r.receipt).collect::<Vec<_>>());}},Operation::Settlement{settlements,settlement_ids,..}=>{event["settlements"]=serde_json::to_value(&settlements.receipt)?;event["settlementIds"]=json!(settlement_ids);}}
 event["originalSourceEvidence"]=json!(receipts.iter().map(|r|r.evidence()).collect::<Result<Vec<_>>>()?);event["eventId"]=json!(format!("collateral:{}",hash(&event)?));
 let mut preview=bank.engine().clone();preview.advance(&event).map_err(|s|->Box<dyn std::error::Error>{s.into()})?;
 need(capture(bank,b,at)?==captured,"COLLATERAL_BOUNDARY_CHANGED")?;for r in &receipts{r.verify()?;}
 Ok(Transition{event,boundary:captured,receipts,processed:processed.into()})
}
impl Transition<'_>{
 /// Commit immediately in the sole worker, with no intervening network or
 /// source/planning events. The unchanged supplied processing clock is sealed.
 pub fn commit(self,bank:&mut Session,b:&Boundary<'_>)->Result<Committed>{
  need(capture(bank,b,clock(&self.processed)?)?==self.boundary,"COLLATERAL_BOUNDARY_CHANGED")?;for r in &self.receipts{r.verify()?;}
  let seq=bank.last_source_sequence(SOURCE).checked_add(1).ok_or("COLLATERAL_RANGE")?;let digest=hash(&self.event)?;
  need(bank.accept(SOURCE,seq,&self.event)?,"COLLATERAL_ALREADY_COMMITTED")?;need(bank.committed_source_hash(SOURCE,seq)==Some(digest.as_str()),"COLLATERAL_COMMIT_REFERENCE")?;
  Ok(Committed{source_sequence:seq,event_sha256:digest,durable_head:bank.durable_head(),report:bank.engine().report().map_err(|s|->Box<dyn std::error::Error>{s.into()})?})
 }
}
