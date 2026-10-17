//! Offline original_not_sent_records workload only; no Session, SDK or network.
//! Compile with the runtime's SAME arbitrary_precision serde_json and sha2 rlibs.
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::{collections::{BTreeMap,BTreeSet},error::Error,fs::{self,File},hint::black_box,
    os::unix::fs::{FileExt,MetadataExt},path::{Path,PathBuf},time::Instant};
type Result<T> = std::result::Result<T,Box<dyn Error>>;
const MAX:u64=64*1024*1024;
fn need(b:bool,s:&str)->Result<()>{if b{Ok(())}else{Err(s.to_owned().into())}}
fn sha(b:&[u8])->String{format!("{:x}",Sha256::digest(b))}
fn digest(v:&Value)->Result<String>{Ok(sha(&serde_json::to_vec(v)?))}
fn identity(f:&File)->Result<Value>{let m=f.metadata()?;Ok(json!({"device":m.dev(),"inode":m.ino()}))}
fn version(f:&File)->Result<Value>{let m=f.metadata()?;Ok(json!({"device":m.dev(),"inode":m.ino(),"bytes":m.len(),"mtime":m.mtime(),"mtimeNs":m.mtime_nsec(),"ctime":m.ctime(),"ctimeNs":m.ctime_nsec()}))}
fn path_pin(p:&Path,id:&Value)->Result<()>{need(!fs::symlink_metadata(p)?.file_type().is_symlink(),"FUNDAMENTAL_SYMLINK")?;need(identity(&File::open(p)?)?==*id,"FUNDAMENTAL_PATH_REPLACED")}
fn read_file(f:&File)->Result<Vec<u8>>{let n=f.metadata()?.len();need(n<=MAX,"FUNDAMENTAL_FILE_BOUND")?;let mut b=vec![0;usize::try_from(n)?];f.read_exact_at(&mut b,0)?;need(f.metadata()?.len()==n,"FUNDAMENTAL_FILE_CHANGED")?;Ok(b)}
struct Pin{file:File,path:PathBuf,identity:Value,bytes:u64,sha:String,version:Value}
impl Pin{
 fn new(file:File,path:PathBuf)->Result<Self>{let identity=identity(&file)?;path_pin(&path,&identity)?;let bytes=read_file(&file)?;let sha=sha(&bytes);let version=version(&file)?;Ok(Self{file,path,identity,bytes:bytes.len() as u64,sha,version})}
 fn metadata(&self)->Result<()>{path_pin(&self.path,&self.identity)?;need(version(&self.file)?==self.version,"FUNDAMENTAL_PINNED_FILE_CHANGED")}
 fn validate(&self)->Result<()>{self.metadata()?;need(self.file.metadata()?.len()==self.bytes&&sha(&read_file(&self.file)?)==self.sha,"FUNDAMENTAL_PINNED_FILE_CHANGED")?;self.metadata()}
}
// Copied normal never-attempted branch of fundamental_policy.rs730..752.
// The original proved-no-network branch is deliberately UNSUPPORTED here:
// wanted is exclusively genuine retired top-ups with original not_sent rows.
fn scan(bytes:&[u8],wanted:&BTreeSet<u64>)->Result<(u64,String,BTreeMap<u64,Value>)>{
 need(bytes.is_empty()||bytes.last()==Some(&b'\n'),"FUNDAMENTAL_ABANDON_DISPATCH_TORN")?;
 let mut sequence=0u64;let mut previous="0".repeat(64);let mut found=BTreeMap::new();
 for line in bytes.split(|b|*b==b'\n').filter(|l|!l.is_empty()){
  need(line.len()<=2*1024*1024,"FUNDAMENTAL_ABANDON_DISPATCH_ROW_BOUND")?;
  let original:Value=serde_json::from_slice(line)?;let mut row=original.clone();let claimed=row.as_object_mut().ok_or("FUNDAMENTAL_ABANDON_DISPATCH_ROW")?.remove("hash").ok_or("FUNDAMENTAL_ABANDON_DISPATCH_HASH")?;
  need(row["sequence"]==sequence+1&&row["previous"]==previous&&claimed==digest(&row)?,"FUNDAMENTAL_ABANDON_DISPATCH_CHAIN")?;
  if let Some(qid)=row["event"]["qid"].as_u64().filter(|id|wanted.contains(id)){
   need(!matches!(row["event"]["kind"].as_str(),Some("attempt"|"ack"|"recovered_ack")),"FUNDAMENTAL_ABANDON_ATTEMPTED")?;
   if row["event"]["kind"]=="not_sent"{let proof=&row["event"]["proof"];
    need(!found.contains_key(&qid)&&*proof==json!({"qid":qid,"chain":previous,"sequence":sequence,"networkAttempts":0}),"FUNDAMENTAL_ABANDON_POSITIVE_NOT_SENT")?;found.insert(qid,original);
   }
  }
  sequence+=1;previous=claimed.as_str().ok_or("FUNDAMENTAL_ABANDON_DISPATCH_HASH")?.into();
 }
 Ok((sequence,previous,found))
}
fn descriptor(f:&File,expected:&Value)->Result<Value>{let m=f.metadata()?;Ok(json!({"schemaVersion":1,"device":m.dev(),"inode":m.ino(),"bytes":m.len(),"sequence":expected["sequence"],"previous":expected["previous"]}))}
fn milliseconds(t:std::time::Duration)->f64{t.as_secs_f64()*1000.0}
fn once(path:&Path,held:&File,original_identity:&Value,expected:&Value,wanted:&BTreeSet<u64>,attempted:&BTreeSet<u64>,selected:&Value)->Result<Value>{
 let start=Instant::now();need(wanted.len()<=4096&&wanted.iter().all(|id|!attempted.contains(id)),"FUNDAMENTAL_ABANDON_ATTEMPTED")?;let before=descriptor(held,expected)?;
 let pin=Pin::new(File::open(path)?,path.to_owned())?;need(pin.identity==*original_identity,"FUNDAMENTAL_ORIGINAL_DISPATCH_REQUIRED")?;
 let pinned=Instant::now();let bytes=read_file(&pin.file)?;let read=Instant::now();
 let(sequence,previous,found)=scan(&bytes,wanted)?;let scanned=Instant::now();
 pin.validate()?;need(descriptor(held,expected)?==before&&before["sequence"]==sequence&&before["previous"]==previous,"FUNDAMENTAL_ABANDON_DISPATCH_CHANGED")?;
 need(found.len()==wanted.len(),"FUNDAMENTAL_ABANDON_POSITIVE_NOT_SENT_REQUIRED")?;
 let end=Instant::now();
 // Independent sidecar output comparison is OUTSIDE the timed interval.
 need(serde_json::to_value(&found)?==*selected,"INDEPENDENT_SELECTED_NOT_SENT_MISMATCH")?;
 need(pin.bytes==expected["bytes"].as_u64().ok_or("EXPECTED_BYTES")?&&pin.sha==expected["sha256"].as_str().ok_or("EXPECTED_SHA")?,"PINNED_ORIGINAL_CHANGED")?;
 black_box(&found);
 Ok(json!({"initialPinMs":milliseconds(pinned-start),"parseReadMs":milliseconds(read-pinned),"chainParseCloneDigestSelectionMs":milliseconds(scanned-read),"finalPinAndDescriptorMs":milliseconds(end-scanned),"totalMs":milliseconds(end-start)}))
}
fn file_path(root:&Path,v:&Value)->Result<PathBuf>{let n=v["file"].as_str().ok_or("SOURCE_FILE")?;need(!n.is_empty()&&!n.contains(['/', '\\'])&&!n.contains(".."),"SOURCE_BASENAME")?;let p=root.join(n);need(fs::symlink_metadata(&p)?.is_file()&&!fs::symlink_metadata(&p)?.file_type().is_symlink(),"SOURCE_REGULAR_FILE")?;Ok(p)}
fn pinned(path:&Path,proof:&Value)->Result<Vec<u8>>{let f=File::open(path)?;path_pin(path,&identity(&f)?)?;let b=read_file(&f)?;need(b.len() as u64==proof["bytes"].as_u64().ok_or("SOURCE_BYTES")?&&sha(&b)==proof["sha256"].as_str().ok_or("SOURCE_SHA")?,"SOURCE_PIN_MISMATCH")?;Ok(b)}
fn selected_from_fundamental(bytes:&[u8],expected:&Value)->Result<BTreeMap<u64,Value>>{
 need(bytes.last()==Some(&b'\n'),"FUNDAMENTAL_TORN")?;
 let mut sequence=0u64;let mut previous="0".repeat(64);let mut cohorts:BTreeMap<String,BTreeMap<u64,Value>>=BTreeMap::new();
 for line in bytes.split(|b|*b==b'\n').filter(|l|!l.is_empty()){
  let mut row:Value=serde_json::from_slice(line)?;let hash=row.as_object_mut().ok_or("FUNDAMENTAL_ROW")?.remove("hash").ok_or("FUNDAMENTAL_HASH")?;
  need(row["sequence"]==sequence+1&&row["previous"]==previous&&hash==digest(&row)?,"FUNDAMENTAL_CHAIN")?;
  let e=&row["event"];
  match e["kind"].as_str(){
   Some("entry"|"existing_origin")=>{let ex=if e["kind"]=="existing_origin"{e["review"]["exchangeId"].as_str()}else{e["exchangeId"].as_str()}.ok_or("COHORT_EXCHANGE")?;need(cohorts.insert(ex.into(),BTreeMap::new()).is_none()&&e["cohort"].get("retiredTopUps").is_none_or(|x|x.as_object().is_some_and(|o|o.is_empty())),"UNSUPPORTED_COHORT_REPLACEMENT")?;},
   Some("abandon_top_up_never_sent")=>{let ex=e["exchangeId"].as_str().ok_or("COHORT_EXCHANGE")?;let qid=e["qid"].as_u64().ok_or("COHORT_QID")?;let c=cohorts.get_mut(ex).ok_or("COHORT_MISSING")?;need(c.insert(qid,e["originalNotSent"].clone()).is_none(),"DUPLICATE_RETIREMENT")?;},
   Some("abandon_never_sent"|"abandon_proved_no_network"|"rearm_closed_zero")=>{let ex=e["exchangeId"].as_str().ok_or("COHORT_EXCHANGE")?;need(cohorts.remove(ex).is_some_and(|c|c.is_empty()),"UNSUPPORTED_ABANDONED_TOPUPS")?;},
   Some("review"|"review_generation"|"top_up"|"exit")=>{},_=>return Err("UNSUPPORTED_FUNDAMENTAL_EVENT".into())
  }
  sequence+=1;previous=hash.as_str().ok_or("FUNDAMENTAL_HASH")?.into();
 }
 need(expected["sequence"]==sequence&&expected["previous"]==previous,"FUNDAMENTAL_EXPECTED_HEAD")?;
 let mut out=BTreeMap::new();for rows in cohorts.into_values(){for(id,row)in rows{need(out.insert(id,row).is_none(),"DUPLICATE_COHORT_QID")?;}}Ok(out)
}
fn median(rows:&[Value],key:&str)->Result<f64>{let mut values=rows.iter().map(|r|r[key].as_f64().ok_or("TIME_FIELD")).collect::<std::result::Result<Vec<_>,_>>()?;values.sort_by(f64::total_cmp);Ok(values[values.len()/2])}
fn main()->Result<()>{
 let args=std::env::args().collect::<Vec<_>>();need(args.len()==3,"usage: not-sent-bench INPUT.json EXPECTED_INPUT_SHA256")?;
 let raw=fs::read(&args[1])?;need(raw.len()<=1024*1024&&sha(&raw)==args[2],"INPUT_PIN_MISMATCH")?;let input:Value=serde_json::from_slice(&raw)?;need(input["schemaVersion"]==1,"INPUT_SCHEMA")?;
 // Fail before measurement if linked against a non-runtime Number feature set.
 let sentinel="1.234567890123456789012345678901";need(serde_json::from_str::<Value>(sentinel)?.to_string()==sentinel,"ARBITRARY_PRECISION_SERDE_JSON_REQUIRED")?;
 let root=Path::new(&args[1]).parent().ok_or("INPUT_PARENT")?;let dispatch_path=file_path(root,&input["dispatch"])?;let fundamental_path=file_path(root,&input["fundamental"])?;
 let dispatch=pinned(&dispatch_path,&input["dispatch"])?;let fundamental=pinned(&fundamental_path,&input["fundamental"])?;
 let selected=selected_from_fundamental(&fundamental,&input["fundamental"])?;let wanted=selected.keys().copied().collect::<BTreeSet<_>>();need(!wanted.is_empty()&&wanted.len()<=4096,"WANTED_BOUND")?;
 let selected_json=serde_json::to_value(&selected)?;need(selected_json==input["expectedNotSent"]&&digest(&selected_json)?==input["expectedNotSentSha256"].as_str().ok_or("EXPECTED_SELECTION_SHA")?,"INDEPENDENT_EXPECTED_SELECTION")?;
 let(s,p,rows)=scan(&dispatch,&wanted)?;need(input["dispatch"]["sequence"]==s&&input["dispatch"]["previous"]==p&&serde_json::to_value(rows)?==selected_json,"INDEPENDENT_INITIAL_SCAN")?;
 let mut attempted=BTreeSet::new();for line in dispatch.split(|b|*b==b'\n').filter(|l|!l.is_empty()){let row:Value=serde_json::from_slice(line)?;if row["event"]["kind"]=="attempt"{attempted.insert(row["event"]["qid"].as_u64().ok_or("ATTEMPT_QID")?);}}
 let held=File::open(&dispatch_path)?;let original_identity=identity(&held)?;drop(dispatch);drop(fundamental);
 for _ in 0..2{black_box(once(&dispatch_path,&held,&original_identity,&input["dispatch"],&wanted,&attempted,&selected_json)?);}
 let mut rounds=Vec::new();for _ in 0..7{rounds.push(once(&dispatch_path,&held,&original_identity,&input["dispatch"],&wanted,&attempted,&selected_json)?);}
 let mut medians=json!({});for key in ["initialPinMs","parseReadMs","chainParseCloneDigestSelectionMs","finalPinAndDescriptorMs","totalMs"]{medians[key]=json!(median(&rounds,key)?);}
 println!("{}",serde_json::to_string_pretty(&json!({"schemaVersion":1,"sourceSha256":sha(include_bytes!("bench.rs")),"inputSha256":args[2],"dispatchSha256":input["dispatch"]["sha256"],"dispatchBytes":input["dispatch"]["bytes"],"dispatchSequence":s,"dispatchPrevious":p,"fundamentalSha256":input["fundamental"]["sha256"],"wantedQids":wanted,"selectedNotSentSha256":digest(&selected_json)?,"warmups":2,"roundCount":7,"rounds":rounds,"medianMs":medians,"actualCurrentFunctionExecuted":false,"copiedNormalNeverAttemptedBranch":true,"sdkProvedNoNetworkBranchBenchmarked":false,"sessionOpened":false,"networkRequests":0,"executionAuthority":false,"financialReplay":false,"totalLoopSpeedupEstablished":false}))?);Ok(())
}
