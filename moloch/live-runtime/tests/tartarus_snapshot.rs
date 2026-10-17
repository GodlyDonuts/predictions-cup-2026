#![cfg(unix)]
//! Original held Session/SDK/log fixtures; no network method is invoked.
use moloch_live_runtime::{tartarus_snapshot::*,collateral_feedback::{Boundary,RetainedReceipt,ReceiptStore},dispatch::DispatchLog,package_dispatch::PackageDispatchLog,feedback};
use baal_transport_service::{Receipt,Scope,Config,Transport,DisabledCredentials,SystemClock,ReadOnlyAuthority,admission::AccountAdmission};
use moloch_replay::session::{Session,ExclusiveLock,atomic_json};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{fs,path::{Path,PathBuf}};
const T:&str="11111111-1111-4111-8111-111111111111";
const P:&str="22222222-2222-4222-8222-222222222222";
const NOW:i64=1_792_022_400_000;
const ZERO:&str="0000000000000000000000000000000000000000000000000000000000000000";
fn iso(at:i64)->String{chrono::DateTime::from_timestamp_millis(at).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis,true)}
fn sha(b:&[u8])->String{format!("{:x}",Sha256::digest(b))}
fn retain(root:&Path,r:Receipt)->RetainedReceipt{fs::create_dir_all(root.join("receipts")).unwrap();atomic_json(&root.join("receipts").join(format!("{}.json",feedback::digest(&r).unwrap())),&serde_json::to_value(&r).unwrap()).unwrap();RetainedReceipt::open(root,&r,ReceiptStore::Ordinary).unwrap()}
fn receipt(endpoint:String,data:Value)->Receipt{Receipt{endpoint,data,status:200,request_started_at:iso(NOW-2000),received_at:iso(NOW-1000)}}
// Optional interop handoff only. The external destination is never used as a
// fixture root and is NEVER deleted (even on failure). No official/live data.
fn export_synthetic_interop(packet:&Path){
 let Some(output)=std::env::var_os("TARTARUS_PACKET_FIXTURE_OUTPUT")else{return};
 let output=PathBuf::from(output);assert!(output.is_absolute(),"fixture output must be absolute");
 assert!(!output.exists(),"fixture output must be create-new");fs::create_dir(&output).unwrap();
 use std::{io::Write,os::unix::{fs::{MetadataExt,OpenOptionsExt,PermissionsExt}}};
 fs::set_permissions(&output,fs::Permissions::from_mode(0o700)).unwrap();
 let names=["account-original.json","positions-original.json","open-orders-original.json","collateral-original.json","control.json","manifest-original.json","head-original.json","report-original.json","ordinary-dispatch-original.json","package-dispatch-original.json","metadata.json"];
 let metadata:Value=serde_json::from_slice(&fs::read(packet.join("metadata.json")).unwrap()).unwrap();
 assert_eq!(metadata["generationSha256"],ZERO);assert_eq!(metadata["executionAuthority"],false);
 assert!(!packet.join("generation-request.json").exists());
 let mut files=serde_json::Map::new();let mut total=0u64;
 for name in names{
  let source=packet.join(name);let before=fs::symlink_metadata(&source).unwrap();assert!(before.is_file()&&!before.file_type().is_symlink()&&before.nlink()==1&&before.len()<=64*1024*1024);
  total=total.checked_add(before.len()).unwrap();assert!(total<=128*1024*1024);
  let bytes=fs::read(&source).unwrap();let after=fs::symlink_metadata(&source).unwrap();
  assert_eq!((before.dev(),before.ino(),before.len()),(after.dev(),after.ino(),after.len()));assert_eq!(bytes.len()as u64,before.len());
  let mut file=fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(output.join(name)).unwrap();file.write_all(&bytes).unwrap();file.sync_all().unwrap();
  assert_eq!(fs::read(output.join(name)).unwrap(),bytes);files.insert(name.into(),json!({"sha256":sha(&bytes),"bytes":bytes.len()}));
 }
 let expected=json!({"expected_metadata_sha256":files.get("metadata.json").unwrap()["sha256"],"expected_control_sha256":files.get("control.json").unwrap()["sha256"],"expected_generation_sha256":ZERO,"expected_report_sha256":files.get("report-original.json").unwrap()["sha256"],"expected_head_sha256":files.get("head-original.json").unwrap()["sha256"]});
 let artifact=json!({"schemaVersion":1,"evidenceClass":"synthetic_rust_fixture","synthetic":true,"executionAuthority":false,"financialEvidence":false,"expectedPins":expected,"files":files,"producerModuleSha256":sha(include_bytes!("../src/tartarus_snapshot.rs")),"producerTestSha256":sha(include_bytes!("tartarus_snapshot.rs"))});
 let mut raw=serde_json::to_vec(&artifact).unwrap();raw.push(b'\n');let mut file=fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(output.join("interop-expected-pins.json")).unwrap();file.write_all(&raw).unwrap();file.sync_all().unwrap();fs::File::open(&output).unwrap().sync_all().unwrap();
 println!("TARTARUS_SYNTHETIC_PACKET_FIXTURE={}",json!({"outputDirectory":output,"expectedPinsArtifactSha256":sha(&raw),"expectedPins":expected,"synthetic":true,"executionAuthority":false}));
}
struct F{root:PathBuf,owner_path:PathBuf,owner:ExclusiveLock,t:Transport,log:DispatchLog,packages:PackageDispatchLog,bank:Session,control_sha:String,account:RetainedReceipt,positions:RetainedReceipt,open:RetainedReceipt,collateral:RetainedReceipt}
impl Drop for F{fn drop(&mut self){let _=fs::remove_dir_all(&self.root);}}
impl F{
 fn new(unknown:bool)->Self{
  let root=std::env::temp_dir().join(format!("tartarus-held-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));fs::create_dir_all(&root).unwrap();
  let owner_path=root.join("owner.lock");let owner=ExclusiveLock::acquire(&owner_path).unwrap();
  let scope=Scope{tournament_id:T.into(),tournament_slug:"cup".into(),exchanges:[("101".into(),"201".into())].into()};
  {let mut q=AccountAdmission::open(&root.join("account-queue"),"snapshot-account","fixture").unwrap();if unknown{q.command(json!({"op":"admit","scope":"snapshot-account","now":100,"request":"prior-request","write":true,"binding":{"intent":"prior-intent","key":"prior-key","payload_hash":"a".repeat(64)},"authorized":true,"lease_valid":true,"permit_valid":true})).unwrap();}else{q.command(json!({"op":"status","scope":"snapshot-account","now":1000})).unwrap();}}
  let t=Transport::new_for_mock(Config{queue_dir:root.join("account-queue"),account_scope:"snapshot-account".into(),scope:scope.clone(),maximum_pages:20,writes_enabled:false},Box::new(DisabledCredentials),Box::new(SystemClock),Box::new(ReadOnlyAuthority),"http://127.0.0.1:1/api/v1".into()).unwrap();
  let log=DispatchLog::open(&root.join("dispatch.ndjson")).unwrap();let packages=PackageDispatchLog::open(&root.join("package-dispatch")).unwrap();
  let policy=json!({"quantity":5,"maximumMarkets":1,"entryStopAt":iso(NOW+600000)});let seed=json!({"runId":"snapshot-fixture","availableCash":"1000.000000","accountSnapshotHash":"a".repeat(64),"observedAt":iso(NOW-3000)});
  let mut bank=Session::open_live(&root.join("bank"),&policy,&seed).unwrap();
  bank.accept("books",1,&json!({"type":"book","receivedAt":iso(NOW),"book":{"tournamentId":T,"marketId":"201","exchangeId":"101","sequence":"1","sourceAt":iso(NOW),"requestStartedAt":iso(NOW),"receivedAt":iso(NOW),"bids":[{"price":"0.400000","quantity":100}],"asks":[{"price":"0.450000","quantity":100}]}})).unwrap();
  atomic_json(&root.join("control.json"),&json!({"scope":scope,"accountScope":"snapshot-account","activation":{"runId":"snapshot-fixture"},"bankName":"bank"})).unwrap();let control_sha=sha(&fs::read(root.join("control.json")).unwrap());
  let account=retain(&root,receipt(format!("/account?tournamentId={T}"),json!({"id":P,"balance":1000,"availableAdvance":0,"buyingPower":1000})));
  let positions=retain(&root,receipt("/tournaments/cup/portfolio/positions".into(),json!({"positions":[]})));
  let open=retain(&root,receipt(format!("/orders?status=open&limit=200&tournamentId={T}"),json!({"data":[],"coverage":{"complete":true},"pagination":{"hasMore":false,"nextCursor":null}})));
  let collateral=retain(&root,receipt(format!("/portfolio/collateral?tournamentId={T}"),json!({"availableAdvance":0,"data":[]})));
  Self{root,owner_path,owner,t,log,packages,bank,control_sha,account,positions,open,collateral}
 }
 fn boundary(&self)->Boundary<'_>{Boundary{root:&self.root,owner_lock_path:&self.owner_path,owner_lease:&self.owner,transport:&self.t,ordinary:&self.log,packages:&self.packages}}
 fn capture(&self,collateral:bool)->Result<Packet>{let sources=[SourceFile{label:"control",root:&self.root,relative:Path::new("control.json"),sha256:&self.control_sha}];capture(&self.bank,&self.boundary(),Receipts{account:&self.account,positions:&self.positions,open_orders:&self.open,collateral:collateral.then_some(&self.collateral)},Pins{files:&sources,generation_sha256:ZERO},NOW+5000)}
 fn financial_bytes(&self)->Vec<Vec<u8>>{["bank/events.jsonl","bank/head.json","bank/report.json","dispatch.ndjson","package-dispatch/package-dispatch.ndjson"].iter().map(|p|fs::read(self.root.join(p)).unwrap()).collect()}
}
#[test]fn exact_original_bytes_and_complete_unused_drafts_export_without_mutation(){
 let f=F::new(false);let head=f.bank.durable_head();let before=f.financial_bytes();let packet=f.capture(true).unwrap();let m=packet.metadata();
 assert_eq!(m["status"],"available_as_evidence");assert_eq!(m["capturedAtMs"],NOW+5000);assert_eq!(m["sourceObservationMs"],NOW-1000);assert_eq!(m["terminalDebtMicros"],Value::Null);assert_eq!(m["observedCollateralDebtMicros"],"0");assert_eq!(m["executionAuthority"],false);
 let ids:Vec<_>=f.bank.engine().order_records().into_iter().filter(|q|q["retired"]==false).map(|q|q["id"].clone()).collect();assert!(!ids.is_empty());assert_eq!(m["neverSubmittedDraftIds"],json!(ids));
 assert_eq!(m["nativeBooks"][0]["sourceAt"],iso(NOW));let path=packet.write_new(&f.bank,&f.boundary(),"frozen-001").unwrap();
 for(label,source)in [("report-original","bank/report.json"),("head-original","bank/head.json"),("ordinary-dispatch-original","dispatch.ndjson")]{assert_eq!(fs::read(path.join(format!("{label}.json"))).unwrap(),fs::read(f.root.join(source)).unwrap());}
 assert!(packet.write_new(&f.bank,&f.boundary(),"frozen-001").is_err());assert!(packet.write_new(&f.bank,&f.boundary(),"../elsewhere").is_err());assert_eq!(f.bank.durable_head(),head);assert_eq!(f.financial_bytes(),before);
 assert!(f.bank.engine().managed_inventory_planning_record().is_none());export_synthetic_interop(&path);
}
#[test]fn missing_collateral_preserves_unknown_debt_but_still_exports_present_evidence(){let f=F::new(false);let p=f.capture(false).unwrap();assert!(p.metadata()["observedCollateralDebtMicros"].is_null());assert!(p.metadata()["terminalDebtMicros"].is_null());assert_eq!(p.metadata()["economicFreshnessQualified"],false);}
#[test]fn actual_unknown_sdk_request_refuses_without_financial_mutation(){let f=F::new(true);let before=f.financial_bytes();assert!(f.capture(true).is_err());assert_eq!(f.financial_bytes(),before);}
#[test]fn even_unattempted_durable_intent_is_not_classified_as_unused(){let mut f=F::new(false);let q=f.bank.engine().order_records()[0]["id"].as_u64().unwrap();f.log.append(json!({"kind":"intent","qid":q})).unwrap();let before=f.financial_bytes();assert!(f.capture(true).is_err());assert_eq!(f.financial_bytes(),before);}
#[test]fn actual_head_change_invalidates_packet_and_retains_original_snapshot(){let mut f=F::new(false);let p=f.capture(true).unwrap();f.bank.accept("tick",1,&json!({"type":"tick","receivedAt":iso(NOW+100)})).unwrap();let before=f.financial_bytes();assert!(p.validate_current(&f.bank,&f.boundary()).is_err());assert!(p.write_new(&f.bank,&f.boundary(),"stale").is_err());assert_eq!(f.financial_bytes(),before);}
#[test]fn same_length_original_receipt_tamper_and_descriptor_replacement_are_rejected(){
 let f=F::new(false);let p=f.capture(true).unwrap();let path=f.root.join("receipts").join(format!("{}.json",feedback::digest(f.account.receipt()).unwrap()));let raw=fs::read(&path).unwrap();let changed=String::from_utf8(raw.clone()).unwrap().replace("1000","1001");assert_eq!(changed.len(),raw.len());fs::write(&path,changed).unwrap();assert!(p.validate_current(&f.bank,&f.boundary()).is_err());
 fs::write(&path,&raw).unwrap();let p=f.capture(true).unwrap();fs::rename(&path,path.with_extension("retained")).unwrap();fs::write(&path,&raw).unwrap();assert!(p.validate_current(&f.bank,&f.boundary()).is_err());
}
#[test]fn persisted_report_or_wrong_control_pin_cannot_stand_in_for_session(){let f=F::new(false);let sources=[SourceFile{label:"control",root:&f.root,relative:Path::new("control.json"),sha256:&"f".repeat(64)}];assert!(capture(&f.bank,&f.boundary(),Receipts{account:&f.account,positions:&f.positions,open_orders:&f.open,collateral:None},Pins{files:&sources,generation_sha256:ZERO},NOW+5000).is_err());let p=f.root.join("bank/report.json");let raw=fs::read_to_string(&p).unwrap();fs::write(p,raw.replace("1000.000000","1001.000000")).unwrap();assert!(f.capture(true).is_err());}
#[test]fn wrong_owner_descriptor_and_unbound_generation_request_are_rejected(){let f=F::new(false);let alternate=f.root.join("alternate.lock");let owner=ExclusiveLock::acquire(&alternate).unwrap();let mut boundary=f.boundary();boundary.owner_lock_path=&alternate;assert!(f.capture(true).unwrap().validate_current(&f.bank,&boundary).is_err());drop(owner);let sources=[SourceFile{label:"control",root:&f.root,relative:Path::new("control.json"),sha256:&f.control_sha}];assert!(capture(&f.bank,&f.boundary(),Receipts{account:&f.account,positions:&f.positions,open_orders:&f.open,collateral:None},Pins{files:&sources,generation_sha256:&"a".repeat(64)},NOW+5000).is_err());}
#[test]fn original_future_receipt_and_parent_traversal_never_become_snapshot_evidence(){
 let f=F::new(false);let mut r=f.account.receipt().clone();r.request_started_at=iso(NOW+6000);r.received_at=iso(NOW+6001);let future=retain(&f.root,r);
 let sources=[SourceFile{label:"control",root:&f.root,relative:Path::new("control.json"),sha256:&f.control_sha}];
 assert!(capture(&f.bank,&f.boundary(),Receipts{account:&future,positions:&f.positions,open_orders:&f.open,collateral:None},Pins{files:&sources,generation_sha256:ZERO},NOW+5000).is_err());
 let unsafe_path=[SourceFile{label:"control",root:&f.root,relative:Path::new("../control.json"),sha256:&f.control_sha}];
 assert!(capture(&f.bank,&f.boundary(),Receipts{account:&f.account,positions:&f.positions,open_orders:&f.open,collateral:None},Pins{files:&unsafe_path,generation_sha256:ZERO},NOW+5000).is_err());
}
