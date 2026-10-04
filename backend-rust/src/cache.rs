//! Immutable derived observations. Raw account/state blobs never enter a cache.
//! A caller must pin its original-byte SHA; the matching original prefix is
//! rehashed on every use, and appended records pass the normal canonical reader.
use crate::types::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest,Sha256};
use std::{fs,path::Path,io::Write};

#[derive(Serialize,Deserialize,PartialEq,Eq)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Scope { tournament_id:String, market_ids:Vec<String>, max_data_age_ms:i64, receipt_order:bool }
fn scope(c:&Config,receipt_order:bool)->Scope {Scope{tournament_id:c.tournament_id.clone(),market_ids:c.market_ids.clone(),max_data_age_ms:c.max_data_age_ms,receipt_order}}
#[derive(Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Cache {
    schema_version:u32,mode:String,engine_hash:String,scope:Scope,
    source_hash:String,source_bytes:u64,source_records:usize,tail_hash:String,
    first_at:i64,through:i64,cycles:usize,accepted_books:usize,invalid_books:usize,
    frames:Vec<Frame>,prefix_hashes:Vec<String>,prefix_bytes:Vec<u64>,
}
fn valid_hash(s:&str)->bool{s.len()==64&&s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))}
pub fn write(path:&Path,source:&Capture,c:&Config,receipt_order:bool)->Result<serde_json::Value,String>{
    let data=Cache{schema_version:1,mode:"original_prefix_observation_cache_v1".into(),engine_hash:env!("BAAL_ENGINE_HASH").into(),scope:scope(c,receipt_order),source_hash:source.meta.source_hash.clone(),source_bytes:source.meta.source_bytes,source_records:source.meta.source_records,tail_hash:source.tail_hash.clone(),first_at:source.meta.first_at,through:source.meta.through,cycles:source.meta.cycles,accepted_books:source.meta.accepted_books,invalid_books:source.meta.invalid_books,frames:source.frames.clone(),prefix_hashes:source.prefix_hashes.clone(),prefix_bytes:source.prefix_bytes.clone()};
    let bytes=serde_json::to_vec(&data).map_err(|_|"CACHE_SERIALIZE_FAILED")?;
    let mut file=fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|_|"CACHE_OUTPUT_EXISTS_OR_UNAVAILABLE")?;
    file.write_all(&bytes).map_err(|_|"CACHE_WRITE_FAILED")?;file.sync_all().map_err(|_|"CACHE_SYNC_FAILED")?;
    Ok(serde_json::json!({"schemaVersion":1,"mode":data.mode,"engineHash":data.engine_hash,"cacheHash":format!("{:x}",Sha256::digest(&bytes)),"cacheBytes":bytes.len(),"sourceHash":data.source_hash,"sourceBytes":data.source_bytes,"sourceRecords":data.source_records,"through":data.through,"networkRequests":0,"competitionOrders":0}))
}
pub fn read(path:&Path,expected_hash:&str,source_hash:&str,source_bytes:u64,c:&Config,receipt_order:bool)->Result<Capture,String>{
    c.validate()?;
    if !valid_hash(expected_hash)||!valid_hash(source_hash)||source_bytes==0{return Err("CACHE_BINDING_INVALID".into());}
    let size=fs::metadata(path).map_err(|_|"CACHE_OPEN_FAILED")?.len();
    if size==0||size>512*1024*1024{return Err("CACHE_SIZE_INVALID".into());}
    let bytes=fs::read(path).map_err(|_|"CACHE_OPEN_FAILED")?;
    if format!("{:x}",Sha256::digest(&bytes))!=expected_hash{return Err("CACHE_HASH_MISMATCH".into());}
    let d:Cache=serde_json::from_slice(&bytes).map_err(|_|"CACHE_JSON_INVALID")?;
    if d.schema_version!=1||d.mode!="original_prefix_observation_cache_v1"||d.engine_hash!=env!("BAAL_ENGINE_HASH")||d.scope!=scope(c,receipt_order)||d.source_hash!=source_hash||d.source_bytes!=source_bytes||!valid_hash(&d.tail_hash)||d.source_records==0||d.frames.len()!=d.source_records||d.prefix_hashes.len()!=d.source_records||d.prefix_bytes.len()!=d.source_records||d.prefix_hashes.last()!=Some(&d.source_hash)||d.prefix_bytes.last()!=Some(&source_bytes){return Err("CACHE_BINDING_MISMATCH".into());}
    // These checks catch broken cache layouts; authenticity comes from the
    // pinned cache SHA and independently matched original prefix, not this schema.
    let mut last=i64::MIN;let mut last_byte=0;
    for (i,f) in d.frames.iter().enumerate(){if f.sequence!=i+1||f.at<last||d.prefix_bytes[i]<=last_byte||!valid_hash(&d.prefix_hashes[i]){return Err("CACHE_LAYOUT_INVALID".into());}last=f.at;last_byte=d.prefix_bytes[i];}
    if d.frames[0].at!=d.first_at||last!=d.through{return Err("CACHE_CLOCK_INVALID".into());}
    Ok(Capture{meta:CaptureMeta{engine_hash:env!("BAAL_ENGINE_HASH"),source_format:"verified_original_canonical_journal_v1",source_bytes:d.source_bytes,source_records:d.source_records,source_hash:d.source_hash,first_at:d.first_at,through:d.through,cycles:d.cycles,accepted_books:d.accepted_books,invalid_books:d.invalid_books,verify_ms:0},frames:d.frames,prefix_hashes:d.prefix_hashes,prefix_bytes:d.prefix_bytes,tail_hash:d.tail_hash})
}
