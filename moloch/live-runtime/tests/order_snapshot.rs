use moloch_live_runtime::order_snapshot::{snapshot,quote};
use moloch_replay::session::{Session,BatchEntry};
use serde_json::{Value,json};
use std::{path::PathBuf,sync::Arc};
const NOW:u64=1_792_022_400_000;
fn iso(at:u64)->String{chrono::DateTime::from_timestamp_millis(at as i64).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis,true)}
struct Temp(PathBuf);impl Temp{fn new()->Self{let p=std::env::temp_dir().join(format!("order-snapshot-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));Self(p)}}impl Drop for Temp{fn drop(&mut self){let _=std::fs::remove_dir_all(&self.0);}}
fn policy()->Value{json!({"quantity":5,"maximumMarkets":3,"entryStopAt":iso(NOW+600000)})}
fn seed()->Value{json!({"runId":"order-cache-fixture","availableCash":"1000.000000","accountSnapshotHash":"a".repeat(64),"observedAt":iso(NOW-1000)})}
fn book()->Value{json!({"type":"book","receivedAt":iso(NOW),"book":{"tournamentId":"cup","marketId":"101","exchangeId":"101","sequence":"1","sourceAt":iso(NOW),"requestStartedAt":iso(NOW),"receivedAt":iso(NOW),"bids":[{"price":"0.400000","quantity":100}],"asks":[{"price":"0.450000","quantity":100}]}})}
fn fixture(path:&std::path::Path)->Session{let mut b=Session::open_live(path,&policy(),&seed()).unwrap();assert!(b.accept("original-books",1,&book()).unwrap());b}
#[test]fn same_exact_head_reuses_one_render_and_single_quotes_equal_original_native_rows(){
 let t=Temp::new();let b=fixture(&t.0);let head=b.durable_head();let journal=std::fs::read(t.0.join("events.jsonl")).unwrap();let s=snapshot(&b);let again=snapshot(&b);assert!(Arc::ptr_eq(&s,&again));assert_eq!(s.records(),b.engine().order_records());assert!(!s.records().is_empty());
 for q in s.records(){assert_eq!(quote(&b,q["id"].as_u64().unwrap()).as_ref(),Some(q));}assert!(quote(&b,u64::MAX).is_none());assert!(s.validate_current(&b));assert_eq!(b.durable_head(),head);assert_eq!(std::fs::read(t.0.join("events.jsonl")).unwrap(),journal);
}
#[test]fn every_committed_head_change_misses_even_when_order_rows_did_not_change(){
 let t=Temp::new();let mut b=fixture(&t.0);let before=snapshot(&b);let tick=json!({"type":"tick","receivedAt":iso(NOW+1)});assert!(b.accept("ticks",1,&tick).unwrap());let after=snapshot(&b);assert!(!Arc::ptr_eq(&before,&after));assert!(!before.validate_current(&b));assert_eq!(after.records(),b.engine().order_records());
 assert!(!b.accept("ticks",1,&tick).unwrap());assert!(Arc::ptr_eq(&after,&snapshot(&b)));let mut conflict=tick.clone();conflict["receivedAt"]=json!(iso(NOW+2));assert!(b.accept("ticks",1,&conflict).is_err());assert!(Arc::ptr_eq(&after,&snapshot(&b)));
}
#[test]fn independent_identical_prefix_banks_and_restart_never_share_cached_session_identity(){
 let a=Temp::new();let z=Temp::new();let b=fixture(&a.0);let other=fixture(&z.0);let first=snapshot(&b);let second=snapshot(&other);assert!(!Arc::ptr_eq(&first,&second));assert!(!first.validate_current(&other));assert_eq!(first.records(),second.records());assert!(!Arc::ptr_eq(&second,&snapshot(&b)));drop(b);
 let restored=Session::open_live(&a.0,&policy(),&seed()).unwrap();let s=snapshot(&restored);assert_eq!(s.records(),restored.engine().order_records());assert!(s.validate_current(&restored));
}
#[test]fn actual_ack_fill_and_retirement_never_reuse_old_zero_fill_or_funded_quote(){
 let t=Temp::new();let mut b=fixture(&t.0);let before=snapshot(&b);let q=before.records().iter().find(|q|q["side"]=="YES").unwrap().clone();let id=q["id"].as_u64().unwrap();let n=q["quantity"].as_u64().unwrap();let at=q["activationAt"].as_u64().unwrap();
 let mut e=json!({"type":"actual_order_ack","source":"official_sig_api_v1","receiptHash":"b".repeat(64),"orderId":id,"actualOrderId":"901","tournamentId":"cup","marketId":q["marketId"],"exchangeId":q["exchangeId"],"side":q["side"],"action":"buy","quantity":n,"price":q["price"],"receivedAt":iso(at+1)});
 b.accept("owned-original",1,&e).unwrap();let ack=snapshot(&b);assert!(!Arc::ptr_eq(&before,&ack));assert_eq!(quote(&b,id),b.engine().order_records().into_iter().find(|q|q["id"]==id));
 e["type"]=json!("actual_fill");e["fillId"]=json!("1001");e["fee"]=json!("0.000000");e["executedAt"]=json!(iso(at+2));e["receivedAt"]=json!(iso(at+3));b.accept("owned-original",2,&e).unwrap();let filled=snapshot(&b);assert!(!Arc::ptr_eq(&ack,&filled));assert_eq!(filled.quote(id).unwrap()["filled"],n);assert_eq!(before.quote(id).unwrap()["filled"],0);assert!(!before.validate_current(&b));assert_eq!(filled.records(),b.engine().order_records());
 assert_eq!(b.engine().wallet.quantity("101",moloch_replay::Side::Yes),n);
}
#[test]fn genuine_never_sent_retirement_invalidates_original_funded_row(){
 let t=Temp::new();let mut b=fixture(&t.0);let before=snapshot(&b);let q=before.records().iter().find(|q|q["retired"]==false).unwrap().clone();let id=q["id"].as_u64().unwrap();
 b.accept("retire",1,&json!({"type":"unsubmitted_intent_retired","source":"moloch_live_dispatcher","receiptHash":"c".repeat(64),"confirmedNetworkAttempts":0,"orderId":id,"receivedAt":iso(NOW+1)})).unwrap();
 assert_eq!(quote(&b,id).unwrap()["retired"],true);assert_eq!(before.quote(id).unwrap()["retired"],false);assert!(!before.validate_current(&b));assert_eq!(snapshot(&b).records(),b.engine().order_records());
}

#[test]fn accepted_batch_invalidates_once_at_actual_final_head_without_intermediate_state_alias(){
 let t=Temp::new();let mut b=fixture(&t.0);let before=snapshot(&b);let events=[json!({"type":"tick","receivedAt":iso(NOW+1)}),json!({"type":"tick","receivedAt":iso(NOW+2)})];let entries=events.iter().enumerate().map(|(i,e)|BatchEntry{source_id:"batch-original",sequence:i as u64+1,event:e}).collect::<Vec<_>>();assert_eq!(b.accept_batch(&entries).unwrap().accepted,vec![true,true]);let after=snapshot(&b);assert!(!Arc::ptr_eq(&before,&after));assert!(after.validate_current(&b));assert_eq!(after.records(),b.engine().order_records());assert!(Arc::ptr_eq(&after,&snapshot(&b)));
}
