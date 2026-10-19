use moloch_live_runtime::ledger_snapshot;
use moloch_replay::session::Session;
use serde_json::json;
use std::{fs,path::PathBuf,sync::Arc};
fn bank(name:&str)->(PathBuf,Session){
 let root=std::env::temp_dir().join(format!("ledger-snapshot-{name}-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
 let at=1_792_022_400_000i64;let iso=|ms|chrono::DateTime::from_timestamp_millis(ms).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis,true);
 let policy=json!({"quantity":5,"maximumMarkets":3,"entryStopAt":iso(at+600000)});
 let seed=json!({"runId":"ledger-cache-native","availableCash":"1000.000000","accountSnapshotHash":"a".repeat(64),"observedAt":iso(at-1000)});
 let bank=Session::open_live(&root,&policy,&seed).unwrap();(root,bank)
}
#[test]fn actual_session_full_report_is_shared_without_mutating_any_original(){
 let(root,bank)=bank("original");let head=bank.durable_head();let original=fs::read(root.join("events.jsonl")).unwrap();
 let first=ledger_snapshot::snapshot(&bank).unwrap();assert_eq!(first.value().as_ref(),&bank.engine().report().unwrap());
 for _ in 0..10{let other=ledger_snapshot::snapshot(&bank).unwrap();assert!(Arc::ptr_eq(&first,&other));assert!(Arc::ptr_eq(first.value(),other.value()));assert!(other.validate_current(&bank));}
 assert_eq!(bank.durable_head(),head);assert_eq!(fs::read(root.join("events.jsonl")).unwrap(),original);
 drop(bank);fs::remove_dir_all(root).unwrap();
}
#[test]fn independent_equal_json_hashes_but_changed_original_field_is_rejected(){
 let(root,bank)=bank("bytes");let first=ledger_snapshot::snapshot(&bank).unwrap();
 let mut independent=first.value().as_ref().clone();assert!(first.matches_value(&independent).unwrap());
 independent["cash"]=json!("0.000000");assert!(!first.matches_value(&independent).unwrap());
 let mut history=first.value().as_ref().clone();history["retainedHistoryMustNotBeDiscarded"]=json!({"text":"a".repeat(1024*1024)});
 assert!(!first.matches_value(&history).unwrap());drop(bank);fs::remove_dir_all(root).unwrap();
}
#[test]fn actual_head_change_and_distinct_held_session_invalidate_the_cache(){
 let(root,mut first_bank)=bank("head");let old=ledger_snapshot::snapshot(&first_bank).unwrap();
 let tick=json!({"type":"tick","receivedAt":"2026-10-15T00:00:00.001Z"});first_bank.accept("ticks",1,&tick).unwrap();
 assert!(!old.validate_current(&first_bank));let new=ledger_snapshot::snapshot(&first_bank).unwrap();assert!(!Arc::ptr_eq(&old,&new));
 assert_eq!(new.value().as_ref(),&first_bank.engine().report().unwrap());
 let(other_root,other)=bank("other");assert!(!new.validate_current(&other));let different=ledger_snapshot::snapshot(&other).unwrap();assert!(!Arc::ptr_eq(&new,&different));
 assert!(!first_bank.accept("ticks",1,&tick).unwrap());assert!(new.validate_current(&first_bank));
 drop(first_bank);drop(other);fs::remove_dir_all(root).unwrap();fs::remove_dir_all(other_root).unwrap();
}
