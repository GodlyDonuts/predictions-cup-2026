//! Read offsets are an optimization. Only the held Session's committed source
//! membership can authorize the returned original event.
use super::{Session,Result,Value,Path,fs,hash};
#[cfg(unix)]
mod unix{
 use super::*;
 use std::{cell::RefCell,collections::BTreeMap,io::{Read,BufRead,BufReader,Seek,SeekFrom},os::unix::fs::MetadataExt,path::PathBuf};
 const ROW:u64=8*1024*1024;
 const ENTRIES:usize=100_000;
 struct Index{session:usize,manifest:Value,path:PathBuf,device:u64,inode:u64,length:u64,modified:(i64,i64,i64,i64),head:Value,offset:u64,rows:u64,locations:BTreeMap<(String,u64),(u64,u64)>}
 thread_local!{static INDEX:RefCell<Option<Index>>=const{RefCell::new(None)};}
 fn stamp(m:&fs::Metadata)->(i64,i64,i64,i64){(m.mtime(),m.mtime_nsec(),m.ctime(),m.ctime_nsec())}
 fn verified_event(bank:&Session,row:&Value,qid:u64,kind:&str)->Result<(String,u64,Value)>{
  let e=&row["event"];if e["type"]!=kind||e["orderId"]!=qid{return Err("LIVE_COHORT_INDEX_WRONG_EVENT".into())}
  let r=&e["sourceRef"];let source=r["id"].as_str().ok_or("LIVE_COHORT_SOURCE_REF")?;let seq=r["sequence"].as_u64().filter(|n|*n>0).ok_or("LIVE_COHORT_SOURCE_REF")?;
  let mut original=e.clone();original.as_object_mut().ok_or("LIVE_COHORT_EVENT")?.remove("sourceRef");let digest=hash(&original);
  if r["hash"]!=digest||bank.committed_source_hash(source,seq)!=Some(digest.as_str()){return Err("LIVE_COHORT_COMMITTED_SOURCE_MISMATCH".into())}
  Ok((source.into(),seq,e.clone()))
 }
 pub(super)fn locate(bank:&Session,path:&Path,qid:u64,kind:&str)->Result<(String,u64,Value)>{
  let metadata=fs::symlink_metadata(path)?;if !metadata.is_file()||metadata.file_type().is_symlink(){return Err("LIVE_COHORT_ORIGINAL_JOURNAL_REQUIRED".into())}
  let file=fs::File::open(path)?;let held=file.metadata()?;
  if metadata.dev()!=held.dev()||metadata.ino()!=held.ino(){return Err("LIVE_COHORT_JOURNAL_CHANGED".into())}
  let session=bank as *const Session as usize;let manifest=bank.manifest_hash().clone();let head=bank.durable_head();let key=(kind.to_owned(),qid);
  // Take ownership so any failure discards a partial/uncertain derived index.
  let mut index=INDEX.with(|slot|slot.borrow_mut().take()).filter(|i|i.session==session&&i.manifest==manifest&&i.path==path).unwrap_or_else(||Index{session,manifest,path:path.to_path_buf(),device:held.dev(),inode:held.ino(),length:held.len(),modified:stamp(&held),head:head.clone(),offset:0,rows:0,locations:BTreeMap::new()});
  if index.device!=held.dev()||index.inode!=held.ino()||held.len()<index.length
    ||(head==index.head&&(held.len()!=index.length||stamp(&held)!=index.modified))
    ||(head!=index.head&&(held.len()<=index.length||head["sequence"].as_u64()<=index.head["sequence"].as_u64())){return Err("LIVE_COHORT_JOURNAL_CHANGED".into())}
  let mut reader=BufReader::new(file);let mut line=Vec::new();
  if !index.locations.contains_key(&key){
   reader.seek(SeekFrom::Start(index.offset))?;
   loop{
    line.clear();let n=reader.by_ref().take(ROW+1).read_until(b'\n',&mut line)?;if n==0{break}
    if line.len() as u64>ROW||line.last()!=Some(&b'\n'){return Err("LIVE_COHORT_JOURNAL_ROW_BOUND_OR_TORN".into())}
    let row:Value=serde_json::from_slice(&line)?;let e=&row["event"];let offset=index.offset;index.offset=index.offset.checked_add(n as u64).ok_or("LIVE_COHORT_OFFSET_RANGE")?;index.rows+=1;
    if let(Some(event),Some(id))=(e["type"].as_str(),e["orderId"].as_u64()){
     if matches!(event,"unsubmitted_intent_retired"|"actual_order_terminal"){
      if index.locations.len()>=ENTRIES{return super::super::committed_cohort_event_uncached(bank,path,qid,kind)}
      index.locations.entry((event.into(),id)).or_insert((offset,n as u64));
     }
    }
    if index.locations.contains_key(&key){break}
   }
  }
  let result=if let Some(&(offset,length))=index.locations.get(&key){
   if length>ROW{return Err("LIVE_COHORT_JOURNAL_ROW_BOUND_OR_TORN".into())}
   reader.seek(SeekFrom::Start(offset))?;line.clear();reader.by_ref().take(length).read_to_end(&mut line)?;
   if line.len() as u64!=length||line.last()!=Some(&b'\n'){return Err("LIVE_COHORT_JOURNAL_ROW_BOUND_OR_TORN".into())}
   verified_event(bank,&serde_json::from_slice(&line)?,qid,kind)
  }else{Err("LIVE_COHORT_ORIGINAL_EVENT_MISSING".into())};
  let after=reader.get_ref().metadata()?;
  if after.dev()!=held.dev()||after.ino()!=held.ino()||after.len()!=held.len()||stamp(&after)!=stamp(&held)||bank.durable_head()!=head{return Err("LIVE_COHORT_JOURNAL_CHANGED".into())}
  // Revalidate the selected original row on every hit, then retain offsets.
  if result.is_ok()||!index.locations.contains_key(&key){index.length=held.len();index.modified=stamp(&held);index.head=head;INDEX.with(|slot|*slot.borrow_mut()=Some(index));}
  result
 }
 #[cfg(test)]pub(super)fn stats()->(u64,u64){INDEX.with(|slot|slot.borrow().as_ref().map(|i|(i.offset,i.rows)).unwrap_or_default())}
}
pub(super)fn locate(bank:&Session,path:&Path,qid:u64,kind:&str)->Result<(String,u64,Value)>{
 #[cfg(unix)]{unix::locate(bank,path,qid,kind)}
 #[cfg(not(unix))]{super::committed_cohort_event_uncached(bank,path,qid,kind)}
}
#[cfg(all(test,unix))]pub(super)fn stats()->(u64,u64){unix::stats()}
