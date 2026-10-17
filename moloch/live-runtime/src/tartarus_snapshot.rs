//! Read-only sole-worker evidence export. No client, admission or financial event.
//! Raw hashes establish retained byte identity, not independent server truth.
use crate::{collateral_feedback::{allocation_boundary,Boundary,RetainedReceipt},feedback,order_snapshot};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{collections::BTreeSet,fs::{self,File,OpenOptions},io::{Read,Write},path::{Component,Path,PathBuf}};
pub type Result<T> = feedback::Result<T>;
const MAX_FILE:u64=64*1024*1024;
const MAX_TOTAL:usize=128*1024*1024;
const ZERO:&str="0000000000000000000000000000000000000000000000000000000000000000";
fn need(ok:bool,code:&'static str)->Result<()>{if ok{Ok(())}else{Err(code.into())}}
fn sha(raw:&[u8])->String{format!("{:x}",Sha256::digest(raw))}
fn digest(v:&Value)->Result<String>{Ok(sha(&serde_json::to_vec(v)?))}
fn hex(s:&str)->bool{s.len()==64&&s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))}
fn atom(s:&str)->bool{!s.is_empty()&&s.len()<=96&&s.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-'||b==b'_')}
fn clock(s:&str)->Result<i64>{Ok(chrono::DateTime::parse_from_rfc3339(s)?.timestamp_millis())}
fn no_links(path:&Path)->Result<()>{for p in path.ancestors(){need(!fs::symlink_metadata(p)?.file_type().is_symlink(),"TARTARUS_SYMLINK")?;}Ok(())}
fn identity(m:&fs::Metadata)->Result<Value>{
 #[cfg(unix)]{use std::os::unix::fs::MetadataExt;need(m.is_file()&&m.nlink()==1,"TARTARUS_SINGLE_REGULAR_FILE")?;Ok(json!({"device":m.dev(),"inode":m.ino(),"bytes":m.len()}))}
 #[cfg(not(unix))]{let _=m;Err("TARTARUS_DESCRIPTOR_UNSUPPORTED".into())}
}
struct Original{path:PathBuf,identity:Value,bytes:Vec<u8>,sha256:String}
impl Original{
 fn read(path:&Path,expected:Option<&str>)->Result<Self>{
  no_links(path)?;let file=File::open(path)?;let id=identity(&file.metadata()?)?;need(id["bytes"].as_u64().is_some_and(|n|n<=MAX_FILE),"TARTARUS_FILE_BOUND")?;
  let mut bytes=Vec::new();file.take(MAX_FILE+1).read_to_end(&mut bytes)?;need(bytes.len() as u64<=MAX_FILE,"TARTARUS_FILE_BOUND")?;
  let hash=sha(&bytes);if let Some(expected)=expected{need(hex(expected)&&hash==expected,"TARTARUS_ORIGINAL_HASH")?;}
  let out=Self{path:path.to_owned(),identity:id,bytes,sha256:hash};out.verify()?;Ok(out)
 }
 fn verify(&self)->Result<()>{
  no_links(&self.path)?;let file=File::open(&self.path)?;need(identity(&file.metadata()?)?==self.identity,"TARTARUS_ORIGINAL_DESCRIPTOR")?;
  let mut hash=Sha256::new();let mut n=0usize;let mut limited=file.take(MAX_FILE+1);let mut buf=[0u8;65536];loop{let k=limited.read(&mut buf)?;if k==0{break}n+=k;hash.update(&buf[..k]);}
  need(n==self.bytes.len()&&format!("{:x}",hash.finalize())==self.sha256,"TARTARUS_ORIGINAL_CHANGED")?;
  need(identity(&fs::metadata(&self.path)?)?==self.identity,"TARTARUS_ORIGINAL_DESCRIPTOR")
 }
 fn pin(&self)->Value{json!({"sha256":self.sha256,"bytes":self.bytes.len(),"descriptor":self.identity})}
 fn value(&self)->Result<Value>{Ok(serde_json::from_slice(&self.bytes)?)}
}
fn add(originals:&mut Vec<(String,Original)>,label:&str,original:Original)->Result<()>{
 let n=originals.iter().try_fold(original.bytes.len(),|n,(_,r)|n.checked_add(r.bytes.len()).ok_or("TARTARUS_TOTAL_BOUND"))?;need(n<=MAX_TOTAL,"TARTARUS_TOTAL_BOUND")?;originals.push((label.into(),original));Ok(())
}
fn sealed(original:&Original)->Result<Value>{
 let mut value=original.value()?;let claimed=value.as_object_mut().ok_or("TARTARUS_SEALED_OBJECT")?.remove("hash").ok_or("TARTARUS_SEAL")?;
 need(claimed.as_str().is_some_and(hex)&&claimed==digest(&value)?,"TARTARUS_SEAL")?;value["hash"]=claimed;
 let mut canonical=serde_json::to_vec(&value)?;canonical.push(b'\n');need(canonical==original.bytes,"TARTARUS_ORIGINAL_CANONICAL")?;Ok(value)
}
/// Immutable paths/expected raw hashes supplied by the worker's already checked
/// control/generation state, not by an evidence JSON document. `relative` cannot
/// escape its explicit source root. These pins confer no review/OS-owner approval.
pub struct SourceFile<'a>{pub label:&'a str,pub root:&'a Path,pub relative:&'a Path,pub sha256:&'a str}
pub struct Pins<'a>{pub files:&'a [SourceFile<'a>],pub generation_sha256:&'a str}
pub struct Receipts<'a>{pub account:&'a RetainedReceipt,pub positions:&'a RetainedReceipt,pub open_orders:&'a RetainedReceipt,pub collateral:Option<&'a RetainedReceipt>}
/// Opaque complete evidence packet. No Deserialize, trade permission or future
/// liability model. Exact original bytes are private; export only to fixed root.
pub struct Packet{metadata:Value,originals:Vec<(String,Original)>,boundary:Value,at:i64}
impl Packet{
 pub fn metadata(&self)->&Value{&self.metadata}
 pub fn validate_current(&self,bank:&moloch_replay::session::Session,b:&Boundary<'_>)->Result<()>{
  need(allocation_boundary(bank,b,self.at)?==self.boundary,"TARTARUS_BOUNDARY_CHANGED")?;
  for(_,original)in &self.originals{original.verify()?;}Ok(())
 }
 /// Exclusive private directory, fixed worker-root parent, metadata written last.
 /// A failed export leaves an incomplete directory for inspection, never retries
 /// or deletes financial state. It performs no Session event, SDK admission or network operation.
 pub fn write_new(&self,bank:&moloch_replay::session::Session,b:&Boundary<'_>,name:&str)->Result<PathBuf>{
  need(atom(name),"TARTARUS_PACKET_NAME")?;self.validate_current(bank,b)?;
  let parent=b.root.join("tartarus-snapshots");if !parent.exists(){fs::create_dir(&parent)?;}no_links(&parent)?;
  let path=parent.join(name);fs::create_dir(&path)?;
  #[cfg(unix)]{use std::os::unix::fs::PermissionsExt;fs::set_permissions(&path,fs::Permissions::from_mode(0o700))?;}
  for(label,original)in &self.originals{write_new(&path.join(format!("{label}.json")),&original.bytes)?;}
  self.validate_current(bank,b)?;
  let mut bytes=serde_json::to_vec(&self.metadata)?;bytes.push(b'\n');write_new(&path.join("metadata.json"),&bytes)?;
  #[cfg(unix)]{File::open(&path)?.sync_all()?;File::open(&parent)?.sync_all()?;}
  Ok(path)
 }
}
fn write_new(path:&Path,bytes:&[u8])->Result<()>{let mut options=OpenOptions::new();options.write(true).create_new(true);#[cfg(unix)]{use std::os::unix::fs::OpenOptionsExt;options.mode(0o600);}let mut file=options.open(path)?;file.write_all(bytes)?;file.sync_all()?;Ok(())}

pub fn capture(bank:&moloch_replay::session::Session,b:&Boundary<'_>,receipts:Receipts<'_>,pins:Pins<'_>,at_ms:i64)->Result<Packet>{
 need(at_ms>=0&&hex(pins.generation_sha256)&&!pins.files.is_empty()&&pins.files.len()<=32,"TARTARUS_PIN_OR_CLOCK")?;
 let before=allocation_boundary(bank,b,at_ms)?;let scope=b.transport.scope();
 let mut originals=Vec::new();let mut labels=BTreeSet::new();let mut source_pins=serde_json::Map::new();let mut control=None;
 for source in pins.files{
  need(atom(source.label)&&labels.insert(source.label.to_owned())&&!matches!(source.label,"metadata"|"report-original"|"head-original"|"manifest-original"|"ordinary-dispatch-original"|"package-dispatch-original"|"account-original"|"positions-original"|"open-orders-original"|"collateral-original"),"TARTARUS_SOURCE_LABEL")?;
  need(!source.relative.as_os_str().is_empty()&&source.relative.components().all(|p|matches!(p,Component::Normal(_))),"TARTARUS_SOURCE_RELATIVE_PATH")?;
  no_links(source.root)?;let root=source.root.canonicalize()?;let path=root.join(source.relative);let original=Original::read(&path,Some(source.sha256))?;
  if source.label=="control"{control=Some(original.value()?);}
  source_pins.insert(source.label.into(),original.pin());add(&mut originals,source.label,original)?;
 }
 let control=control.ok_or("TARTARUS_CONTROL_REQUIRED")?;
 need(control["scope"]==serde_json::to_value(scope)?&&control["accountScope"]==b.transport.account_scope()&&control["activation"]["runId"]==before["runId"],"TARTARUS_CONTROL_SCOPE")?;
 if pins.generation_sha256!=ZERO{need(source_pins.get("generation-request").is_some_and(|v|v["sha256"]==pins.generation_sha256),"TARTARUS_GENERATION_REQUEST_REQUIRED")?;}
 if let Some(record)=bank.engine().managed_inventory_planning_record(){need(record["generationSha256"]==pins.generation_sha256,"TARTARUS_NATIVE_GENERATION")?;}
 let bank_name=control["bankName"].as_str().filter(|s|atom(s)).ok_or("TARTARUS_BANK_NAME")?;
 let mut native=serde_json::Map::new();
 for (label,file)in [("manifest-original","manifest.json"),("head-original","head.json"),("report-original","report.json")]{
  let original=Original::read(&b.root.join(bank_name).join(file),None)?;let value=sealed(&original)?;native.insert(label.into(),value);source_pins.insert(label.into(),original.pin());add(&mut originals,label,original)?;
 }
 let head=&native["head-original"];let report=&native["report-original"];
 need(head==&bank.durable_head()&&native["manifest-original"]["hash"]==*bank.manifest_hash()&&report["manifestHash"]==*bank.manifest_hash()&&report["sequence"]==head["sequence"]&&report["previous"]==head["previous"]
  &&report["report"]==bank.engine().report()?&&report["orders"]==json!(order_snapshot::snapshot(bank).records())&&report["fills"]==json!(bank.engine().fills())&&report["contestOrders"].as_u64()==Some(bank.engine().live_order_bindings().len() as u64),"TARTARUS_PERSISTED_SESSION_BINDING")?;
 if let Some(t)=report["report"]["at"].as_str(){need(clock(t)?<=at_ms,"TARTARUS_NATIVE_FUTURE")?;}
 for(label,file,digest_key)in [("ordinary-dispatch-original","dispatch.ndjson","ordinarySha256"),("package-dispatch-original","package-dispatch/package-dispatch.ndjson","packageSha256")]{
  let original=Original::read(&b.root.join(file),before[digest_key].as_str())?;source_pins.insert(label.into(),original.pin());add(&mut originals,label,original)?;
 }
 let mut receipt_pins=serde_json::Map::new();let mut latest=None::<i64>;let mut oldest=None::<i64>;let mut observed_debt=None;
 let mut rs=vec![("account-original",receipts.account,format!("/account?tournamentId={}",scope.tournament_id)),("positions-original",receipts.positions,format!("/tournaments/{}/portfolio/positions",scope.tournament_slug)),("open-orders-original",receipts.open_orders,format!("/orders?status=open&limit=200&tournamentId={}",scope.tournament_id))];
 if let Some(r)=receipts.collateral{rs.push(("collateral-original",r,format!("/portfolio/collateral?tournamentId={}",scope.tournament_id)));}
 for(label,retained,endpoint)in rs{
  retained.verify_root(b.root)?;let r=retained.receipt();let start=clock(&r.request_started_at)?;let end=clock(&r.received_at)?;
  need(r.status==200&&r.endpoint==endpoint&&0<=start&&start<=end&&end<=at_ms,"TARTARUS_RECEIPT_SCOPE_OR_CLOCK")?;
  let evidence=retained.evidence()?;let digest=feedback::digest(r)?;let original=Original::read(&b.root.join("receipts").join(format!("{digest}.json")),evidence["retainedBytesSha256"].as_str())?;
  need(original.value()?==serde_json::to_value(r)?,"TARTARUS_TYPED_RECEIPT_BINDING")?;
  receipt_pins.insert(label.into(),json!({"raw":original.pin(),"receiptSha256":digest,"endpoint":r.endpoint,"requestStartedAt":r.request_started_at,"receivedAt":r.received_at,"sourceAgeAtCaptureMs":at_ms-start}));
  latest=Some(latest.map_or(end,|n|n.max(end)));oldest=Some(oldest.map_or(start,|n|n.min(start)));
  if label=="collateral-original"{let parsed=baal_transport_service::relationships::parse_collateral(&r.data,scope)?;need(parsed.components.iter().all(|c|receipts.account.receipt().data["id"]==c.profile_id),"TARTARUS_COLLATERAL_PROFILE")?;observed_debt=Some(parsed.outstanding_liability_units.to_string());}
  add(&mut originals,label,original)?;
 }
 // Complete official open-order scope may predate books/ticks. Retain its
 // original age; physical/economic currency is a separate downstream question.
 let open=&receipts.open_orders.receipt().data;
 need(open["coverage"]["complete"]==true&&open["pagination"]["hasMore"]==false&&open["pagination"]["nextCursor"].is_null()&&open["data"].as_array().is_some_and(Vec::is_empty),"TARTARUS_OPEN_ORDER_EVIDENCE")?;
 let snapshot=order_snapshot::snapshot(bank);let mut drafts=Vec::new();
 for q in snapshot.records().iter().filter(|q|q["retired"]==false){
  let id=q["id"].as_u64().ok_or("TARTARUS_DRAFT_ID")?;
  need(b.ordinary.never_attempted(id)&&!b.ordinary.intents.contains_key(&id)&&!b.ordinary.acknowledgments.contains_key(&id)&&!bank.engine().live_order_bindings().contains_key(&id)&&!bank.engine().is_package_order(id)&&q["filled"]==0&&!bank.engine().fills().iter().any(|f|f["orderId"]==id),"TARTARUS_UNCLASSIFIED_NATIVE_ORDER")?;
  drafts.push(id);
 }
 need(json!(drafts)==before["unsubmitted"],"TARTARUS_DRAFT_SET_CHANGED")?;
 let total=originals.iter().try_fold(0usize,|n,(_,r)|n.checked_add(r.bytes.len()).ok_or("TARTARUS_TOTAL_BOUND"))?;need(total<=MAX_TOTAL,"TARTARUS_TOTAL_BOUND")?;
 let metadata=json!({"schemaVersion":1,"status":"available_as_evidence","capturedAtMs":at_ms,"sourceObservationMs":latest,"oldestReceiptRequestMs":oldest,"nativeHead":head,"bankManifestHash":bank.manifest_hash(),"scope":scope,"generationSha256":pins.generation_sha256,"originalFiles":source_pins,"originalReceipts":receipt_pins,"originalMemberNaming":"<map-key>.json (raw bytes preserved, including NDJSON journals)","heldBoundary":before,"neverSubmittedDraftIds":drafts,"draftClassification":"complete_exact_never_attempted_no_intent_ack_binding_fill_or_package","nativeBooks":bank.engine().live_package_books(),"observedCollateralDebtMicros":observed_debt,"terminalDebtMicros":Value::Null,"economicFreshnessQualified":false,"terminalLiabilityModelAvailable":false,"registrySemanticsQualified":false,"executionAuthority":false,"calibratedProbabilityClaimed":false,"rawSourceServerAuthenticationClaimed":false});
 let packet=Packet{metadata,originals,boundary:before,at:at_ms};packet.validate_current(bank,b)?;Ok(packet)
}
