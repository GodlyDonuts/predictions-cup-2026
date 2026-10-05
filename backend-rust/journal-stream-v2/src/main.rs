mod canonical;
use serde::Serialize;use serde_json::value::RawValue;use sha2::{Digest,Sha256};
use std::{collections::HashMap,fs::{File,OpenOptions},io::{BufRead,BufReader,Read,Write},path::Path,time::Instant};
const LIMIT:u64=16*1024*1024;
#[derive(Serialize)]#[serde(rename_all="camelCase")]struct Restore{
 schema_version:u8,mode:&'static str,engine_hash:&'static str,source_bytes:u64,source_hash:String,source_stamp:String,records:u64,tail_hash:String,last_body:Option<Box<RawValue>>,verify_ms:f64,
 #[serde(skip_serializing_if="Option::is_none")] projection:Option<Projection>,
}
#[derive(Serialize)]#[serde(rename_all="camelCase")]struct Projection{after_sequence:u64,after_hash:String,records:u64,bytes:u64,hash:String}
struct ProjectionRequest<'a>{after_sequence:u64,after_hash:&'a str,out:&'a Path}
fn need(v:bool,c:&str)->Result<(),String>{if v{Ok(())}else{Err(c.to_owned())}}
fn restore(file:&Path,end:u64,expected:&str,projection:Option<ProjectionRequest>)->Result<Restore,String>{
 let started=Instant::now();need(expected=="COMPUTE"||expected.len()==64&&expected.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b)),"JOURNAL_PREFIX_BINDING")?;
 let input=open_snapshot(file).map_err(|_|"JOURNAL_OPEN_FAILED")?;need(end<=input.metadata().map_err(|_|"JOURNAL_METADATA_FAILED")?.len(),"JOURNAL_PREFIX_TRUNCATED")?;
 let initial=stamp(&input)?;let mut reader=BufReader::with_capacity(1024*1024,(&input).take(end));let mut prefix=Sha256::new();let mut bytes=0u64;let mut records=0u64;let mut previous=String::new();let mut last_line=Vec::new();let mut line=Vec::new();let mut last_end=0;
 let mut projected=projection.as_ref().map(|p|OpenOptions::new().write(true).create_new(true).open(p.out)).transpose().map_err(|_|"JOURNAL_PROJECTION_OUTPUT_FAILED")?;
 let mut projected_hash=Sha256::new();let mut projected_records=0u64;let mut projected_bytes=0u64;
 let mut frontier_seen=projection.as_ref().is_none_or(|p|p.after_sequence==0&&p.after_hash.is_empty());
 loop{
  line.clear();let n=reader.by_ref().take(LIMIT+2).read_until(b'\n',&mut line).map_err(|_|"JOURNAL_READ_FAILED")?;if n==0{break;}
  need(n as u64<=LIMIT+1&&line.last()==Some(&b'\n'),"JOURNAL_PARTIAL_OR_OVERSIZE")?;bytes+=n as u64;prefix.update(&line);
  if line.len()==1{need(projection.is_none(),"JOURNAL_PROJECTION_EMPTY_RECORD")?;continue;}
  let raw=std::str::from_utf8(&line[..line.len()-1]).map_err(|_|"JOURNAL_UTF8_INVALID")?;
  need(records<9007199254740991,"JOURNAL_CHAIN_INVALID")?;
  let header=format!("{{\"sequence\":{},\"previous\":\"{}\",\"at\":",records+1,previous);
  need(raw.starts_with(&header),"JOURNAL_CHAIN_INVALID")?;
  need(raw.as_bytes().get(header.len())==Some(&b'"'),"JOURNAL_CANONICAL_ENVELOPE")?;
  let at_end=canonical::end(raw,header.len())?;let event_start=at_end+9;
  need(raw.get(at_end..event_start)==Some(",\"event\":"),"JOURNAL_CANONICAL_ENVELOPE")?;
  let event_end=canonical::end(raw,event_start)?;let state_start=event_end+9;
  need(raw.get(event_end..state_start)==Some(",\"state\":"),"JOURNAL_CANONICAL_ENVELOPE")?;
  let state_end=canonical::end(raw,state_start)?;
  need(raw.get(state_end..state_end+9)==Some(",\"hash\":\"")&&raw.get(state_end+73..)==Some("\"}"),"JOURNAL_CANONICAL_ENVELOPE")?;
  let record_hash=raw.get(state_end+9..state_end+73).ok_or("JOURNAL_HASH_INVALID")?;
  need(record_hash.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b)),"JOURNAL_HASH_INVALID")?;
  let mut h=Sha256::new();h.update(&raw.as_bytes()[..state_end]);h.update(b"}");need(format!("{:x}",h.finalize())==record_hash,"JOURNAL_HASH_INVALID")?;
  previous=record_hash.to_owned();records+=1;
  if let Some(p)=projection.as_ref(){
   if records==p.after_sequence{need(previous==p.after_hash,"JOURNAL_PROJECTION_FRONTIER_CHANGED")?;frontier_seen=true;}
   if records>p.after_sequence{let output=projected.as_mut().unwrap();output.write_all(&line).map_err(|_|"JOURNAL_PROJECTION_WRITE_FAILED")?;projected_hash.update(&line);projected_records+=1;projected_bytes+=line.len() as u64;}
  }
  last_end=state_end;std::mem::swap(&mut line,&mut last_line);
 }
 need(bytes==end,"JOURNAL_PREFIX_TRUNCATED")?;let source_hash=format!("{:x}",prefix.finalize());need(expected=="COMPUTE"||source_hash==expected,"JOURNAL_PREFIX_MISMATCH")?;verify_identity(&input,file,&initial)?;
 need(frontier_seen,"JOURNAL_PROJECTION_FRONTIER_TRUNCATED")?;
 if let Some(output)=projected.as_mut(){output.sync_all().map_err(|_|"JOURNAL_PROJECTION_SYNC_FAILED")?;}
 let projection=projection.map(|p|Projection{after_sequence:p.after_sequence,after_hash:p.after_hash.to_owned(),records:projected_records,bytes:projected_bytes,hash:format!("{:x}",projected_hash.finalize())});
 let last_body=if records==0{None}else{last_line.truncate(last_end);last_line.push(b'}');let body=String::from_utf8(last_line).map_err(|_|"JOURNAL_UTF8_INVALID")?;Some(RawValue::from_string(body).map_err(|_|"JOURNAL_BODY_INVALID")?)};
 Ok(Restore{schema_version:2,mode:if projection.is_some(){"canonical_js_journal_projection_v2"}else{"canonical_js_journal_restore_v2"},engine_hash:env!("JOURNAL_ENGINE_HASH"),source_bytes:bytes,source_hash,source_stamp:initial,records,tail_hash:previous,last_body,verify_ms:started.elapsed().as_secs_f64()*1000.0,projection})
}
fn run()->Result<(),String>{
 let args:Vec<String>=std::env::args().skip(1).collect();let mut options=HashMap::new();
 for pair in args.chunks(2){need(pair.len()==2&&["--journal","--end-byte","--expected-sha","--out","--after-sequence","--after-hash","--suffix-out"].contains(&pair[0].as_str())&&!pair[1].starts_with("--")&&options.insert(pair[0].as_str(),pair[1].as_str()).is_none(),"JOURNAL_ARGUMENTS")?;}
 let get=|k|options.get(k).copied().ok_or("JOURNAL_ARGUMENTS");let out=get("--out")?;need(!Path::new(out).exists(),"JOURNAL_OUTPUT_EXISTS")?;
 let projection=if options.keys().any(|k|["--after-sequence","--after-hash","--suffix-out"].contains(k)){
  let sequence=get("--after-sequence")?.parse::<u64>().map_err(|_|"JOURNAL_ARGUMENTS")?;let hash=get("--after-hash")?;
  need(sequence<=9007199254740991&&(sequence==0&&hash=="EMPTY"||sequence>0&&hash.len()==64&&hash.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))),"JOURNAL_PROJECTION_BINDING")?;
  Some(ProjectionRequest{after_sequence:sequence,after_hash:if sequence==0{""}else{hash},out:Path::new(get("--suffix-out")?)})
 }else{None};
 let result=restore(Path::new(get("--journal")?),get("--end-byte")?.parse().map_err(|_|"JOURNAL_ARGUMENTS")?,get("--expected-sha")?,projection)?;
 let mut output=OpenOptions::new().write(true).create_new(true).open(out).map_err(|_|"JOURNAL_OUTPUT_FAILED")?;
 output.write_all(&serde_json::to_vec(&result).map_err(|_|"JOURNAL_OUTPUT_FAILED")?).map_err(|_|"JOURNAL_OUTPUT_FAILED")?;output.sync_all().map_err(|_|"JOURNAL_OUTPUT_FAILED")?;
 println!("{}",serde_json::json!({"sourceVerified":true,"sourceBytes":result.source_bytes,"records":result.records,"verifyMs":result.verify_ms,"engineHash":result.engine_hash}));Ok(())
}
fn main(){if std::env::args().nth(1).as_deref()==Some("--worker"){worker();return;}if let Err(e)=run(){eprintln!("{e}");std::process::exit(1);}}
fn worker(){let stdin=std::io::stdin();let mut input=stdin.lock();loop{let mut bytes=Vec::new();match input.by_ref().take(65537).read_until(b'\n',&mut bytes){Ok(0)=>break,Ok(_)=>{},Err(_)=>break};if bytes.len()>65536||bytes.last()!=Some(&b'\n'){println!("{}",serde_json::json!({"ok":false,"error":"JOURNAL_WORKER_REQUEST_LIMIT"}));break;}let result=(||{let raw=std::str::from_utf8(&bytes).map_err(|_|"JOURNAL_WORKER_READ")?;let req:serde_json::Value=serde_json::from_str(&raw).map_err(|_|"JOURNAL_WORKER_REQUEST")?;let file=req["journal"].as_str().ok_or("JOURNAL_WORKER_REQUEST")?;let end=req["endByte"].as_u64().ok_or("JOURNAL_WORKER_REQUEST")?;let expected=req["expectedSha"].as_str().unwrap_or("COMPUTE");restore(Path::new(file),end,expected,None)})();match result{Ok(v)=>println!("{}",serde_json::to_string(&WorkerResult{ok:true,result:v}).expect("worker response serialization")),Err(e)=>println!("{}",serde_json::json!({"ok":false,"error":e}))};let _=std::io::stdout().flush();}}
#[cfg(windows)]fn stamp(file:&File)->Result<String,String>{use std::os::windows::io::AsRawHandle;#[repr(C)]struct Info{attrs:u32,created_low:u32,created_high:u32,access_low:u32,access_high:u32,write_low:u32,write_high:u32,volume:u32,size_high:u32,size_low:u32,links:u32,index_high:u32,index_low:u32}#[link(name="kernel32")]unsafe extern "system"{fn GetFileInformationByHandle(h:*mut std::ffi::c_void,info:*mut Info)->i32;}let mut info=std::mem::MaybeUninit::<Info>::uninit();if unsafe{GetFileInformationByHandle(file.as_raw_handle(),info.as_mut_ptr())}==0{return Err("JOURNAL_IDENTITY_FAILED".into())}let i=unsafe{info.assume_init()};Ok(format!("{}:{}:{}:{}:{}:{}:{}:{}",i.volume,i.index_high,i.index_low,i.created_high,i.created_low,i.write_high,i.write_low,((i.size_high as u64)<<32)|i.size_low as u64))}
#[cfg(unix)]fn stamp(file:&File)->Result<String,String>{use std::os::unix::fs::MetadataExt;let m=file.metadata().map_err(|_|"JOURNAL_IDENTITY_FAILED")?;Ok(format!("{}:{}:{}:{}:{}:{}",m.dev(),m.ino(),m.len(),m.mtime(),m.mtime_nsec(),m.ctime_nsec()))}


fn verify_identity(input:&File,path:&Path,initial:&str)->Result<(),String>{need(stamp(input)?==initial,"JOURNAL_SOURCE_CHANGED")?;let rebound=File::open(path).map_err(|_|"JOURNAL_SOURCE_CHANGED")?;need(stamp(&rebound)?==initial,"JOURNAL_SOURCE_REPLACED")}
#[cfg(test)]mod identity_tests{use super::*;fn fixture()->(std::path::PathBuf,File,String){let p=std::env::temp_dir().join(format!("baal-v2-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));std::fs::write(&p,b"sealed").unwrap();let f=File::open(&p).unwrap();let s=stamp(&f).unwrap();(p,f,s)}#[test]fn append_and_truncation_fail_closed(){let(p,f,s)=fixture();OpenOptions::new().append(true).open(&p).unwrap().write_all(b"append").unwrap();assert_eq!(verify_identity(&f,&p,&s).unwrap_err(),"JOURNAL_SOURCE_CHANGED");drop(f);std::fs::remove_file(p).unwrap();let(p,f,s)=fixture();OpenOptions::new().write(true).open(&p).unwrap().set_len(2).unwrap();assert_eq!(verify_identity(&f,&p,&s).unwrap_err(),"JOURNAL_SOURCE_CHANGED");drop(f);std::fs::remove_file(p).unwrap();}#[test]fn same_size_replacement_detected_by_open_handle_identity(){let(p,f,s)=fixture();let other=p.with_extension("replacement");std::fs::write(&other,b"sealed").unwrap();std::fs::remove_file(&p).unwrap();std::fs::rename(&other,&p).unwrap();assert!(verify_identity(&f,&p,&s).is_err());drop(f);std::fs::remove_file(p).unwrap();}}

#[cfg(windows)]fn open_snapshot(path:&Path)->std::io::Result<File>{use std::os::windows::fs::OpenOptionsExt;OpenOptions::new().read(true).share_mode(1).open(path)}
#[cfg(unix)]fn open_snapshot(path:&Path)->std::io::Result<File>{File::open(path)}
#[cfg(all(test,windows))]mod locking_tests{use super::*;#[test]fn read_snapshot_prevents_inplace_mutation_and_replacement(){let p=std::env::temp_dir().join(format!("baal-v2-lock-{}",std::process::id()));std::fs::write(&p,b"sealed").unwrap();let f=open_snapshot(&p).unwrap();assert!(OpenOptions::new().append(true).open(&p).is_err());assert!(std::fs::remove_file(&p).is_err());drop(f);std::fs::remove_file(p).unwrap();}}

#[derive(Serialize)]struct WorkerResult{ok:bool,result:Restore}
