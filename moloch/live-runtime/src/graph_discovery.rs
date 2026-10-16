//! Bounded read-only graph-root scheduling. No relationship or order authority.
//! Current reports concern observed roots only, never exhaustive graph coverage.
//! Initial equal-age priorities are managed fundamental, owned, then other scoped
//! markets. Least-recently selected roots outrank priority, preventing starvation.
//! The private serialized cursor carries scheduling only, no receipts/certificates;
//! every selection is intersected with the actual current explicit Scope. Failed
//! graph responses still rotate; cursor durability precedes physical graph reads.
//! Existing package-specific certified captures remain separate and unchanged.
use baal_transport_service::Scope;
use serde::{Serialize,Deserialize};
use std::{collections::{BTreeMap,BTreeSet},path::Path,fs};
pub type Result<T>=std::result::Result<T,&'static str>;
const MAX_MARKETS:usize=16384;
fn numeric(s:&str)->bool{!s.is_empty()&&!s.starts_with('0')&&s.bytes().all(|b|b.is_ascii_digit())&&s.parse::<u64>().is_ok_and(|n|n<=9_007_199_254_740_991)}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Rotation{version:u32,tournament_id:String,round:u64,last_served:BTreeMap<String,u64>}
impl Rotation{
 pub fn new(scope:&Scope)->Self{Self{version:1,tournament_id:scope.tournament_id.clone(),round:0,last_served:BTreeMap::new()}}
 pub fn restore(value:serde_json::Value,scope:&Scope)->Result<Self>{let s:Self=serde_json::from_value(value).map_err(|_|"GRAPH_ROTATION_SCHEMA")?;if s.version!=1||s.tournament_id!=scope.tournament_id||s.last_served.len()>MAX_MARKETS||s.last_served.iter().any(|(id,n)|!numeric(id)||*n==0||*n>s.round){return Err("GRAPH_ROTATION_BINDING")}Ok(s)}
 pub fn load(path:&Path,scope:&Scope)->Result<Self>{let m=match fs::symlink_metadata(path){Ok(m)=>m,Err(e)if e.kind()==std::io::ErrorKind::NotFound=>return Ok(Self::new(scope)),Err(_)=>return Err("GRAPH_ROTATION_READ")};if !m.is_file()||m.file_type().is_symlink()||m.len()>1024*1024{return Err("GRAPH_ROTATION_FILE")}let bytes=fs::read(path).map_err(|_|"GRAPH_ROTATION_READ")?;if bytes.len()>1024*1024{return Err("GRAPH_ROTATION_FILE")}Self::restore(serde_json::from_slice(&bytes).map_err(|_|"GRAPH_ROTATION_SCHEMA")?,scope)}
 /// Call only when the existing census is due. Persist the returned scheduler
 /// state before requesting these roots; failed graph reads still rotate, so a
 /// forbidden root cannot starve the rest. Priority affects equal-age ties only.
 pub fn next(&mut self,scope:&Scope,fundamental:&BTreeSet<String>,owned:&BTreeSet<String>)->Result<Vec<String>>{
  if self.tournament_id!=scope.tournament_id||scope.exchanges.len()>MAX_MARKETS||scope.exchanges.iter().any(|(e,m)|!numeric(e)||!numeric(m)){return Err("GRAPH_ROTATION_SCOPE")}
  let known:BTreeSet<String>=scope.exchanges.values().cloned().collect();self.last_served.retain(|m,_|known.contains(m));
  if known.is_empty(){return Ok(vec![])}let round=self.round.checked_add(1).ok_or("GRAPH_ROTATION_ROUND_OVERFLOW")?;
  let fundamental:BTreeSet<String>=fundamental.iter().filter_map(|e|scope.exchanges.get(e).cloned()).collect();let owned:BTreeSet<String>=owned.iter().filter_map(|e|scope.exchanges.get(e).cloned()).collect();
  let mut roots:Vec<String>=known.into_iter().collect();roots.sort_by_key(|m|(self.last_served.get(m).copied().unwrap_or(0),if fundamental.contains(m){0}else if owned.contains(m){1}else{2},m.parse::<u64>().unwrap()));roots.truncate(3);
  for m in &roots{self.last_served.insert(m.clone(),round);}self.round=round;Ok(roots)
 }
}
