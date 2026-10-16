//! Same original report, temp files only. No Session, replay, SDK or account.
use moloch_replay::session::atomic_json;
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{fs::{self,File,OpenOptions},io::Write,path::PathBuf,time::Instant};
fn sha(bytes:&[u8])->String{format!("{:x}",Sha256::digest(bytes))}
fn counters()->Result<(u64,u64),Box<dyn std::error::Error>>{
    let text=fs::read_to_string("/proc/self/io")?;let get=|key:&str|->Result<u64,Box<dyn std::error::Error>>{Ok(text.lines().find_map(|l|l.strip_prefix(key)).ok_or("BENCH_PROC")?.trim().parse()?)};
    Ok((get("syscw:")?,get("wchar:")?))
}
fn main()->Result<(),Box<dyn std::error::Error>>{
    let args:Vec<_>=std::env::args_os().collect();if args.len()!=4{return Err("usage: moloch-atomic-json-bench ORIGINAL_REPORT EXPECTED_SHA NEW_TEMP_DIR".into())}
    let source=PathBuf::from(&args[1]);let target=PathBuf::from(&args[3]);
    let meta=fs::symlink_metadata(&source)?;if !meta.is_file()||meta.file_type().is_symlink()||meta.len()>16*1024*1024||!target.is_absolute(){return Err("BENCH_SOURCE_BOUND".into())}
    let raw=fs::read(&source)?;let input_sha=sha(&raw);if Some(input_sha.as_str())!=args[2].to_str(){return Err("BENCH_SOURCE_SHA".into())}
    let value:Value=serde_json::from_slice(&raw)?;let mut expected=serde_json::to_vec(&value)?;expected.push(b'\n');if expected!=raw{return Err("BENCH_ORIGINAL_BYTE_PARITY".into())}
    fs::create_dir(&target)?;
    let mut results=Vec::new();
    for buffered in [false,true]{let destination=target.join(if buffered{"buffered.json"}else{"baseline.json"});let before=counters()?;let start=Instant::now();
        if buffered{atomic_json(&destination,&value)?;}else{
            let pending=target.join("baseline.pending");let mut f=OpenOptions::new().write(true).create_new(true).open(&pending)?;
            serde_json::to_writer(&mut f,&value)?;f.write_all(b"\n")?;f.sync_all()?;fs::rename(&pending,&destination)?;File::open(&target)?.sync_all()?;
        }
        let elapsed=start.elapsed().as_nanos();let after=counters()?;let emitted=fs::read(&destination)?;if emitted!=raw{return Err("BENCH_OUTPUT_BYTE_PARITY".into())}
        results.push(json!({"buffered":buffered,"wallNs":elapsed,"writeSyscalls":after.0-before.0,"writtenBytes":after.1-before.1,"sha256":sha(&emitted),"byteParity":true,"fileAndDirectorySynced":true}));
    }
    println!("{}",json!({"schemaVersion":1,"sourceSha256":input_sha,"sourceBytes":raw.len(),"results":results,"networkRequests":0,"financialReplayPerformed":false,"limitations":["One sequential baseline/candidate pair; cache and reduced-priority CPU noise remain.","Only durable report serialization, not full account-loop timing or alpha."]}));Ok(())
}
