//! Private account broadcasts invalidate REST context; they never book economics.
use baal_transport_service::{Receipt,Scope};
use moloch_replay::realtime::{AccountOptions,AccountSignal};
use serde_json::json;
use sha2::{Digest,Sha256};
use std::{fs::{self,OpenOptions},io::Write,path::Path,sync::Arc};
pub type Result<T> = std::result::Result<T,Box<dyn std::error::Error>>;
#[derive(Debug)]
pub struct AccountGenerationChanged(&'static str);
impl std::fmt::Display for AccountGenerationChanged{fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result{f.write_str(self.0)}}
impl std::error::Error for AccountGenerationChanged{}
#[derive(Clone)]
pub struct Stamp { signal:Arc<AccountSignal>, generation:u64 }
impl Stamp {
    pub fn validate(&self,current:&Option<Arc<AccountSignal>>)->Result<()>{
        if current.as_ref().is_none_or(|s|!Arc::ptr_eq(s,&self.signal))||self.signal.generation()!=self.generation{return Err(Box::new(AccountGenerationChanged("LIVE_ACCOUNT_REALTIME_CHANGED")))}Ok(())
    }
}
/// A packet acquired before/during an observed account mutation is not rescued
/// by merely capturing the newest generation at the end of prepare.
pub fn capture(signal:&Option<Arc<AccountSignal>>,receipts:&[&Receipt])->Result<Option<Stamp>>{
    let Some(signal)=signal else{return Ok(None)};let generation=signal.generation();
    let last=signal.last_received_ms();
    if receipts.is_empty(){return Err("LIVE_ACCOUNT_REALTIME_EMPTY_FACTS".into())}
    let starts=receipts.iter().map(|r|crate::daily_windows::clock(&r.request_started_at)).collect::<Result<Vec<_>>>()?;
    if generation==0||starts.iter().any(|start|*start<=last)||signal.generation()!=generation{return Err(Box::new(AccountGenerationChanged("LIVE_ACCOUNT_REALTIME_REQUIRES_FRESH_FACTS")))}
    Ok(Some(Stamp{signal:signal.clone(),generation}))
}
pub fn validate(stamp:&Option<Stamp>,signal:&Option<Arc<AccountSignal>>)->Result<()>{match(stamp,signal){(None,None)=>Ok(()),(Some(stamp),_)=>stamp.validate(signal),_=>Err(Box::new(AccountGenerationChanged("LIVE_ACCOUNT_REALTIME_CHANGED")))}}
/// Create one bounded, private, create-new journal for this connection. The
/// sink receives ONLY validated account broadcasts, never authentication frames.
pub fn options(scope:&Scope,root:&Path)->Result<AccountOptions>{
    let directory=root.join("account-realtime");
    match fs::create_dir(&directory){Ok(())=>{},Err(e)if e.kind()==std::io::ErrorKind::AlreadyExists=>{},Err(e)=>return Err(e.into())}
    let metadata=fs::symlink_metadata(&directory)?;if !metadata.is_dir()||metadata.file_type().is_symlink(){return Err("ACCOUNT_REALTIME_CAPTURE_DIRECTORY".into())}
    #[cfg(unix)]{use std::os::unix::fs::PermissionsExt;fs::set_permissions(&directory,fs::Permissions::from_mode(0o700))?;}
    let nonce=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos();
    let path=directory.join(format!("account-{}-{nonce}.ndjson",std::process::id()));
    let mut open=OpenOptions::new();open.write(true).create_new(true);
    #[cfg(unix)]{use std::os::unix::fs::OpenOptionsExt;open.mode(0o600);}
    let mut file=open.open(path)?;let mut bytes=0usize;
    Ok(AccountOptions{exchanges:scope.exchanges.clone(),capture:Box::new(move|event,raw,payload,received|{
        let row=json!({"kind":"official_account_realtime_invalidation","event":event,"receivedAt":received,"bodyText":raw,"bodySha256":format!("{:x}",Sha256::digest(raw.as_bytes())),"payload":payload,"financialAuthority":false});
        let mut line=serde_json::to_vec(&row).map_err(|_|"STREAM_ACCOUNT_CAPTURE")?;line.push(b'\n');
        bytes=bytes.checked_add(line.len()).filter(|n|*n<=64*1024*1024).ok_or("STREAM_ACCOUNT_CAPTURE")?;
        file.write_all(&line).and_then(|_|file.flush()).map_err(|_|"STREAM_ACCOUNT_CAPTURE")?;Ok(())
    })})
}
/// Only exact original physical IDs can become priority hints. This does not
/// create ownership or authorize retry/adoption from an account broadcast.
pub fn prioritize<T>(items:&mut [T],exchange:impl Fn(&T)->&str,signal:&AccountSignal){
    let dirty:std::collections::BTreeSet<_>=signal.dirty_exchanges().into_iter().collect();
    items.sort_by_key(|item|!dirty.contains(exchange(item)));
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn disabled_is_byte_independent_and_needs_no_account_packet(){assert!(capture(&None,&[]).unwrap().is_none());}
 #[test]fn unrelated_signal_cannot_replace_a_frozen_stamp(){let original=Arc::new(AccountSignal::default());let stamp=Stamp{signal:original.clone(),generation:0};assert!(stamp.validate(&Some(original)).is_ok());assert!(stamp.validate(&Some(Arc::new(AccountSignal::default()))).is_err());assert!(stamp.validate(&None).is_err());}
 #[test]fn not_joined_account_stream_cannot_mint_a_stamp(){assert!(capture(&Some(Arc::new(AccountSignal::default())),&[]).is_err());}
 fn receipt(at:u64)->Receipt{Receipt{endpoint:"original-account-fact".into(),status:200,request_started_at:crate::daily_windows::iso(at).unwrap(),received_at:crate::daily_windows::iso(at).unwrap(),data:json!({})}}
 #[test]fn event_during_packet_cannot_be_hidden_by_capturing_new_generation_at_end(){let signal=Arc::new(AccountSignal::default());signal.invalidate_for_resync().unwrap();let old=receipt(signal.last_received_ms());let error=match capture(&Some(signal.clone()),&[&old]){Err(e)=>e,Ok(_)=>panic!("accepted old facts")};assert!(error.is::<AccountGenerationChanged>());let later=receipt(signal.last_received_ms()+1);let stamp=capture(&Some(signal.clone()),&[&later]).unwrap().unwrap();stamp.validate(&Some(signal.clone())).unwrap();signal.invalidate_for_resync().unwrap();assert!(stamp.validate(&Some(signal)).unwrap_err().is::<AccountGenerationChanged>());}
 #[test]fn malformed_original_clock_is_not_a_typed_account_change_defer(){let signal=Arc::new(AccountSignal::default());signal.invalidate_for_resync().unwrap();let mut bad=receipt(signal.last_received_ms()+1);bad.request_started_at="malformed".into();let error=match capture(&Some(signal),&[&bad]){Err(e)=>e,Ok(_)=>panic!("accepted malformed clock")};assert!(!error.is::<AccountGenerationChanged>());}
 #[test]fn original_private_journal_is_create_new_separate_and_preserves_raw_bytes(){let root=std::env::temp_dir().join(format!("account-private-capture-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));fs::create_dir(&root).unwrap();let scope=Scope{tournament_id:"cup".into(),tournament_slug:"cup".into(),exchanges:[("101".into(),"201".into())].into()};let mut first=options(&scope,&root).unwrap();let second=options(&scope,&root).unwrap();let raw="{ \"type\": \"original account data\" }";(first.capture)("account_batch",raw,&json!({"fills":[]}),"2026-10-16T22:00:00Z").unwrap();drop(first);drop(second);let files:Vec<_>=fs::read_dir(root.join("account-realtime")).unwrap().map(|r|r.unwrap().path()).collect();assert_eq!(files.len(),2);let bytes=files.iter().map(|p|fs::read(p).unwrap()).find(|b|!b.is_empty()).unwrap();let row:serde_json::Value=serde_json::from_slice(&bytes).unwrap();assert_eq!(row["bodyText"],raw);assert_eq!(row["bodySha256"],format!("{:x}",Sha256::digest(raw.as_bytes())));assert_eq!(row["financialAuthority"],false);assert!(!root.join("breadth.jsonl").exists());fs::remove_dir_all(root).unwrap();}

}
