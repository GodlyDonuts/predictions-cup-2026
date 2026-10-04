mod canonical;
use serde::Serialize;use serde_json::value::RawValue;use sha2::{Digest,Sha256};
use std::{collections::HashMap,fs::{File,OpenOptions},io::{BufRead,BufReader,Read,Write},path::Path,time::Instant};
const LIMIT:u64=16*1024*1024;
#[derive(Serialize)]#[serde(rename_all="camelCase")]struct Restore{
 schema_version:u8,mode:&'static str,engine_hash:&'static str,source_bytes:u64,source_hash:String,records:u64,tail_hash:String,last_body:Option<Box<RawValue>>,verify_ms:f64,
}
fn need(v:bool,c:&str)->Result<(),String>{if v{Ok(())}else{Err(c.to_owned())}}
fn restore(file:&Path,end:u64,expected:&str)->Result<Restore,String>{
 let started=Instant::now();need(expected.len()==64&&expected.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b)),"JOURNAL_PREFIX_BINDING")?;
 let input=File::open(file).map_err(|_|"JOURNAL_OPEN_FAILED")?;need(end<=input.metadata().map_err(|_|"JOURNAL_METADATA_FAILED")?.len(),"JOURNAL_PREFIX_TRUNCATED")?;
 let mut reader=BufReader::with_capacity(1024*1024,input.take(end));let mut prefix=Sha256::new();let mut bytes=0u64;let mut records=0u64;let mut previous=String::new();let mut last_line=Vec::new();let mut line=Vec::new();let mut last_end=0;
 loop{
  line.clear();let n=reader.by_ref().take(LIMIT+2).read_until(b'\n',&mut line).map_err(|_|"JOURNAL_READ_FAILED")?;if n==0{break;}
  need(n as u64<=LIMIT+1&&line.last()==Some(&b'\n'),"JOURNAL_PARTIAL_OR_OVERSIZE")?;bytes+=n as u64;prefix.update(&line);
  if line.len()==1{continue;}
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
  previous=record_hash.to_owned();records+=1;last_end=state_end;std::mem::swap(&mut line,&mut last_line);
 }
 need(bytes==end,"JOURNAL_PREFIX_TRUNCATED")?;let source_hash=format!("{:x}",prefix.finalize());need(source_hash==expected,"JOURNAL_PREFIX_MISMATCH")?;
 let last_body=if records==0{None}else{last_line.truncate(last_end);last_line.push(b'}');let body=String::from_utf8(last_line).map_err(|_|"JOURNAL_UTF8_INVALID")?;Some(RawValue::from_string(body).map_err(|_|"JOURNAL_BODY_INVALID")?)};
 Ok(Restore{schema_version:1,mode:"canonical_js_journal_restore_v1",engine_hash:env!("JOURNAL_ENGINE_HASH"),source_bytes:bytes,source_hash,records,tail_hash:previous,last_body,verify_ms:started.elapsed().as_secs_f64()*1000.0})
}
fn run()->Result<(),String>{
 let args:Vec<String>=std::env::args().skip(1).collect();let mut options=HashMap::new();
 for pair in args.chunks(2){need(pair.len()==2&&["--journal","--end-byte","--expected-sha","--out"].contains(&pair[0].as_str())&&!pair[1].starts_with("--")&&options.insert(pair[0].as_str(),pair[1].as_str()).is_none(),"JOURNAL_ARGUMENTS")?;}
 let get=|k|options.get(k).copied().ok_or("JOURNAL_ARGUMENTS");let out=get("--out")?;need(!Path::new(out).exists(),"JOURNAL_OUTPUT_EXISTS")?;
 let result=restore(Path::new(get("--journal")?),get("--end-byte")?.parse().map_err(|_|"JOURNAL_ARGUMENTS")?,get("--expected-sha")?)?;
 let mut output=OpenOptions::new().write(true).create_new(true).open(out).map_err(|_|"JOURNAL_OUTPUT_FAILED")?;
 output.write_all(&serde_json::to_vec(&result).map_err(|_|"JOURNAL_OUTPUT_FAILED")?).map_err(|_|"JOURNAL_OUTPUT_FAILED")?;output.sync_all().map_err(|_|"JOURNAL_OUTPUT_FAILED")?;
 println!("{}",serde_json::json!({"sourceVerified":true,"sourceBytes":result.source_bytes,"records":result.records,"verifyMs":result.verify_ms,"engineHash":result.engine_hash}));Ok(())
}
fn main(){if let Err(e)=run(){eprintln!("{e}");std::process::exit(1);}}
