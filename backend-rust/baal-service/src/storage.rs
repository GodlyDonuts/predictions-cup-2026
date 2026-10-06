use baal_journal_append::Journal;
use baal_portfolio_engine::js_stringify;
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::{fs::{self,File,OpenOptions},io::{BufRead,BufReader,Read,Write},path::{Path,PathBuf}};
pub type Result<T>=std::result::Result<T,String>;
pub fn sha(bytes:&[u8])->String{format!("{:x}",Sha256::digest(bytes))}
pub fn read_json(path:&Path)->Result<Value>{serde_json::from_slice(&fs::read(path).map_err(|_|"SERVICE_READ_FAILED")?).map_err(|_|"SERVICE_JSON_INVALID".into())}
pub fn atomic_json(path:&Path,value:&Value)->Result<()> {let temporary=path.with_extension(format!("{}.{}.tmp",std::process::id(),chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)));let mut f=OpenOptions::new().write(true).create_new(true).open(&temporary).map_err(|_|"SERVICE_RECORDING_FAILED")?;f.write_all(serde_json::to_string(value).map_err(|_|"SERVICE_JSON_INVALID")?.as_bytes()).and_then(|_|f.sync_all()).map_err(|_|"SERVICE_RECORDING_FAILED")?;fs::rename(&temporary,path).map_err(|_|"SERVICE_RECORDING_FAILED".into())}
pub struct OwnerLock { path:PathBuf, token:String, file:Option<File> }
impl OwnerLock {
 pub fn acquire(path:&Path)->Result<Self>{let token=format!("{}:{}",std::process::id(),chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0));let mut options=OpenOptions::new();options.read(true).write(true).create_new(true);
  #[cfg(windows)]{use std::os::windows::fs::OpenOptionsExt;options.share_mode(1);}
  let mut file=options.open(path).map_err(|_|"SERVICE_OWNER_EXISTS_OR_UNKNOWN")?;file.write_all(token.as_bytes()).and_then(|_|file.sync_all()).map_err(|_|"SERVICE_OWNER_RECORDING_FAILED")?;Ok(Self{path:path.to_owned(),token,file:Some(file)})}
 pub fn check(&self)->Result<()>{if fs::read_to_string(&self.path).ok().as_deref()==Some(&self.token){Ok(())}else{Err("SERVICE_OWNER_CHANGED".into())}}
 pub fn token(&self)->&str{&self.token}
}
impl Drop for OwnerLock {fn drop(&mut self){let owned=self.check().is_ok();drop(self.file.take());if owned&&self.check().is_ok(){let _=fs::remove_file(&self.path);}}}
pub struct OwnedJournal { journal:Journal, checkpoint:PathBuf, owner:String, stops:Vec<PathBuf> }
impl OwnedJournal {
 pub fn open(path:&Path,owner:&str,stops:&[PathBuf])->Result<Self>{let checkpoint=path.with_extension("checkpoint.json");let known=if checkpoint.exists(){read_json(&checkpoint)?}else{if path.exists(){return Err("SERVICE_UNKNOWN_JOURNAL_RECONCILE_REQUIRED".into())}json!({"sourceBytes":0,"sourceHash":sha(b""),"sequence":0,"tailHash":""})};let request=json!({"path":path,"ownerToken":owner,"generation":1,"stopPaths":stops,"canOperate":true,"stopped":false,"expectedBytes":known["sourceBytes"],"expectedSourceHash":known["sourceHash"],"expectedSequence":known["sequence"],"expectedTailHash":known["tailHash"],"createNew":!path.exists()});let journal=Journal::open(&request)?;let out=Self{journal,checkpoint,owner:owner.into(),stops:stops.to_vec()};atomic_json(&out.checkpoint,&out.journal.status())?;Ok(out)}
 pub fn status(&self)->Value{self.journal.status()}
 pub fn last_state(&self)->Result<Option<Value>>{let st=self.status();let Some(body)=st["lastBody"].as_str()else{return Ok(None)};let parsed:Value=serde_json::from_str(body).map_err(|_|"SERVICE_LAST_STATE_INVALID")?;Ok(Some(parsed["state"].clone()))}
 pub fn commit(&mut self,state:Value,event:Value,at:&str)->Result<Value>{let prior=self.status();let body=format!("{{\"sequence\":{},\"previous\":{},\"at\":{},\"event\":{},\"state\":{}}}",prior["sequence"].as_u64().ok_or("SERVICE_JOURNAL_HEAD")?+1,serde_json::to_string(&prior["tailHash"]).unwrap(),serde_json::to_string(at).unwrap(),js_stringify(&event),js_stringify(&state));let row=json!({"ownerToken":self.owner,"generation":1,"canOperate":!self.stops.iter().any(|p|p.exists()),"stopped":false,"expectedSequence":prior["sequence"],"expectedTailHash":prior["tailHash"],"expectedBytes":prior["sourceBytes"],"expectedSourceHash":prior["sourceHash"],"bodyJson":body});let next=self.journal.commit(&row)?;atomic_json(&self.checkpoint,&self.journal.status())?;Ok(next)}
}
pub fn journal_rows(path:&Path,mut use_row:impl FnMut(Value)->Result<()>)->Result<()> {let mut r=BufReader::new(File::open(path).map_err(|_|"SERVICE_READ_FAILED")?);let mut line=Vec::new();loop{line.clear();let n=(&mut r).take(16*1024*1024+2).read_until(b'\n',&mut line).map_err(|_|"SERVICE_READ_FAILED")?;if n==0{break}if n>16*1024*1024+1||line.last()!=Some(&b'\n'){return Err("SERVICE_PARTIAL_OR_OVERSIZE".into())}if n==1{continue}use_row(serde_json::from_slice(&line).map_err(|_|"SERVICE_JSON_INVALID")?)?;}Ok(())}
