//! Exact lifecycle fingerprint reuse within one unchanged exit-read packet.
//! No decisions, account facts, timestamps or execution authority are cached.
use crate::dispatch::DispatchLog;
use moloch_replay::session::Session;
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::cell::RefCell;
type Result<T>=std::result::Result<T,Box<dyn std::error::Error>>;
#[derive(PartialEq,Eq)]
struct Key{session:usize,manifest:Value,head:Value,dispatch:Value}
thread_local!{static CURRENT:RefCell<Option<(Key,[u8;32])>>=const{RefCell::new(None)};}
#[cfg(test)]thread_local!{static RENDERS:std::cell::Cell<usize>=const{std::cell::Cell::new(0)};}
pub fn lifecycle(bank:&Session,log:&DispatchLog)->Result<[u8;32]>{
    let key=Key{session:bank as *const Session as usize,manifest:bank.manifest_hash().clone(),head:bank.durable_head(),dispatch:log.proof(0)};
    if let Some(hit)=CURRENT.with(|slot|slot.borrow().as_ref().filter(|(k,_)|k==&key).map(|(_,hash)|*hash)){return Ok(hit)}
    CURRENT.with(|slot|*slot.borrow_mut()=None);
    let original=json!({"manifest":bank.manifest_hash(),"policy":bank.engine().policy(),"economics":bank.engine().report()?,"fills":bank.engine().fills(),"dispatch":key.dispatch});
    let hash=Sha256::digest(serde_json::to_vec(&original)?).into();
    #[cfg(test)]RENDERS.with(|n|n.set(n.get()+1));
    CURRENT.with(|slot|*slot.borrow_mut()=Some((key,hash)));Ok(hash)
}
#[cfg(test)]mod tests{
    use super::*;
    use std::fs;
    fn original(bank:&Session,log:&DispatchLog)->[u8;32]{Sha256::digest(serde_json::to_vec(&json!({"manifest":bank.manifest_hash(),"policy":bank.engine().policy(),"economics":bank.engine().report().unwrap(),"fills":bank.engine().fills(),"dispatch":log.proof(0)})).unwrap()).into()}
    #[test]fn unchanged_packet_is_exact_and_real_native_or_dispatch_changes_recompute(){
        let root=std::env::temp_dir().join(format!("exit-lifecycle-cache-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));fs::create_dir_all(&root).unwrap();
        let mut bank=Session::open(&root.join("bank"),None,&json!({})).unwrap();let mut log=DispatchLog::open(&root.join("dispatch.ndjson")).unwrap();
        CURRENT.with(|slot|*slot.borrow_mut()=None);RENDERS.with(|n|n.set(0));
        let journal=fs::read(root.join("bank/events.jsonl")).unwrap();let first=original(&bank,&log);
        for _ in 0..8{assert_eq!(lifecycle(&bank,&log).unwrap(),first)}
        assert_eq!(RENDERS.with(|n|n.get()),1);assert_eq!(fs::read(root.join("bank/events.jsonl")).unwrap(),journal);
        let tick=json!({"type":"tick","receivedAt":"2026-10-15T00:00:00.001Z"});bank.accept("ticks",1,&tick).unwrap();
        assert_eq!(lifecycle(&bank,&log).unwrap(),original(&bank,&log));assert_eq!(RENDERS.with(|n|n.get()),2);
        assert!(!bank.accept("ticks",1,&tick).unwrap());lifecycle(&bank,&log).unwrap();assert_eq!(RENDERS.with(|n|n.get()),2);
        log.append(json!({"kind":"intent","qid":99})).unwrap();assert_eq!(lifecycle(&bank,&log).unwrap(),original(&bank,&log));assert_eq!(RENDERS.with(|n|n.get()),3);
        log.append(json!({"kind":"attempt","qid":99})).unwrap();assert_eq!(lifecycle(&bank,&log).unwrap(),original(&bank,&log));assert_eq!(RENDERS.with(|n|n.get()),4);
        let other=Session::open(&root.join("other"),None,&json!({})).unwrap();assert_eq!(lifecycle(&other,&log).unwrap(),original(&other,&log));assert_eq!(RENDERS.with(|n|n.get()),5);
        drop(other);drop(bank);drop(log);fs::remove_dir_all(root).unwrap();
    }
}
