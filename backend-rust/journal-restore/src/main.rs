mod canonical;
use serde::Serialize;use serde_json::value::RawValue;use sha2::{Digest,Sha256};
use std::{collections::HashMap,fs::{File,OpenOptions},io::{BufRead,BufReader,Read,Write},path::Path,time::Instant};
const LIMIT:u64=16*1024*1024;
#[derive(Serialize)]#[serde(rename_all="camelCase")]struct Restore{
 schema_version:u8,mode:&'static str,engine_hash:&'static str,source_bytes:u64,source_hash:String,records:u64,tail_hash:String,last_body:Option<Box<RawValue>>,verify_ms:f64,
 #[serde(skip_serializing_if="Option::is_none")] projection:Option<Projection>,
}
#[derive(Serialize)]#[serde(rename_all="camelCase")]struct Projection{after_sequence:u64,after_hash:String,records:u64,bytes:u64,hash:String}
struct ProjectionRequest<'a>{after_sequence:u64,after_hash:&'a str,out:&'a Path}
fn need(v:bool,c:&str)->Result<(),String>{if v{Ok(())}else{Err(c.to_owned())}}
fn restore(file:&Path,end:u64,expected:&str,projection:Option<ProjectionRequest>)->Result<Restore,String>{
 let started=Instant::now();need(expected.len()==64&&expected.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b)),"JOURNAL_PREFIX_BINDING")?;
 let input=File::open(file).map_err(|_|"JOURNAL_OPEN_FAILED")?;need(end<=input.metadata().map_err(|_|"JOURNAL_METADATA_FAILED")?.len(),"JOURNAL_PREFIX_TRUNCATED")?;
 let mut reader=BufReader::with_capacity(1024*1024,input.take(end));let mut prefix=Sha256::new();let mut bytes=0u64;let mut records=0u64;let mut previous=String::new();let mut last_line=Vec::new();let mut line=Vec::new();let mut last_end=0;
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
 need(bytes==end,"JOURNAL_PREFIX_TRUNCATED")?;let source_hash=format!("{:x}",prefix.finalize());need(source_hash==expected,"JOURNAL_PREFIX_MISMATCH")?;
 need(frontier_seen,"JOURNAL_PROJECTION_FRONTIER_TRUNCATED")?;
 if let Some(output)=projected.as_mut(){output.sync_all().map_err(|_|"JOURNAL_PROJECTION_SYNC_FAILED")?;}
 let projection=projection.map(|p|Projection{after_sequence:p.after_sequence,after_hash:p.after_hash.to_owned(),records:projected_records,bytes:projected_bytes,hash:format!("{:x}",projected_hash.finalize())});
 let last_body=if records==0{None}else{last_line.truncate(last_end);last_line.push(b'}');let body=String::from_utf8(last_line).map_err(|_|"JOURNAL_UTF8_INVALID")?;Some(RawValue::from_string(body).map_err(|_|"JOURNAL_BODY_INVALID")?)};
 Ok(Restore{schema_version:1,mode:if projection.is_some(){"canonical_js_journal_projection_v1"}else{"canonical_js_journal_restore_v1"},engine_hash:env!("JOURNAL_ENGINE_HASH"),source_bytes:bytes,source_hash,records,tail_hash:previous,last_body,verify_ms:started.elapsed().as_secs_f64()*1000.0,projection})
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
fn main(){if let Err(e)=run(){eprintln!("{e}");std::process::exit(1);}}
