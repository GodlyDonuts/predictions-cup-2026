use moloch_replay::{engine::Engine,session::{BatchEntry,Session,SESSION_BATCH_MAX_BYTES,SESSION_BATCH_MAX_EVENTS}};
use serde_json::{Value,json};
use std::{fs,io::Write,path::{Path,PathBuf},sync::atomic::{AtomicU64,Ordering},time::{SystemTime,UNIX_EPOCH}};
struct Temp(PathBuf);
impl Temp{fn new()->Self{static N:AtomicU64=AtomicU64::new(0);let p=std::env::temp_dir().join(format!("moloch-session-batch-{}-{}-{}",std::process::id(),SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(),N.fetch_add(1,Ordering::SeqCst)));fs::create_dir(&p).unwrap();Self(p)}}
impl Drop for Temp{fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}
fn corpus()->Value{serde_json::from_str(include_str!("reference/golden.json")).unwrap()}
fn ticks(n:usize)->Vec<Value>{(0..n).map(|i|json!({"type":"tick","receivedAt":format!("2026-10-15T00:00:{i:02}Z")})).collect()}
fn inputs<'a>(source:&'a str,events:&'a[Value],first:u64)->Vec<BatchEntry<'a>>{events.iter().enumerate().map(|(i,event)|BatchEntry{source_id:source,sequence:first+i as u64,event}).collect()}
fn bytes(root:&Path)->Vec<Vec<u8>>{["events.jsonl","head.json","report.json"].iter().map(|name|fs::read(root.join(name)).unwrap()).collect()}
fn parity(a:&Path,b:&Path){assert_eq!(bytes(a),bytes(b));}
#[test]fn golden_financial_fee_gap_and_individual_journal_bytes_match_serial(){
 for scenario in corpus().as_array().unwrap().iter().take(3){let t=Temp::new();let a=t.0.join("serial");let b=t.0.join("batch");let policy=&scenario["policy"];let events=scenario["steps"].as_array().unwrap().iter().map(|s|s["event"].clone()).collect::<Vec<_>>();
  let mut serial=Session::open(&a,None,policy).unwrap();let mut batch=Session::open(&b,None,policy).unwrap();
  for(i,event)in events.iter().enumerate(){assert!(serial.accept("fixture",i as u64+1,event).unwrap());}
  for(i,chunk)in events.chunks(SESSION_BATCH_MAX_EVENTS).enumerate(){let r=batch.accept_batch(&inputs("fixture",chunk,(i*SESSION_BATCH_MAX_EVENTS+1)as u64)).unwrap();assert!(r.accepted.iter().all(|v|*v));assert_eq!(r.appended_rows,chunk.len());}
  let actual=batch.engine().report().unwrap();assert_eq!(serial.engine().report().unwrap(),actual);for(field,expected)in scenario["steps"].as_array().unwrap().last().unwrap()["expected"].as_object().unwrap(){assert_eq!(&actual[field],expected,"reference field {field}");}parity(&a,&b);drop(batch);let mut restored=Session::open(&b,None,policy).unwrap();assert_eq!(restored.engine().report().unwrap(),serial.engine().report().unwrap());assert!(restored.accept_batch(&inputs("fixture",&events,1)).unwrap().accepted.iter().all(|v|!*v));parity(&a,&b);
 }
}
#[test]fn historical_and_in_batch_duplicates_are_exactly_once_and_conflict_is_atomic(){
 let t=Temp::new();let root=t.0.join("bank");let mut s=Session::open(&root,None,&json!({})).unwrap();let events=ticks(3);
 let r=s.accept_batch(&[BatchEntry{source_id:"a",sequence:1,event:&events[0]},BatchEntry{source_id:"a",sequence:1,event:&events[0]},BatchEntry{source_id:"b",sequence:1,event:&events[1]}]).unwrap();assert_eq!(r.accepted,vec![true,false,true]);assert_eq!(s.last_source_sequence("a"),1);assert_eq!(s.last_source_sequence("b"),1);
 let before=bytes(&root);assert!(s.accept_batch(&[BatchEntry{source_id:"a",sequence:2,event:&events[2]},BatchEntry{source_id:"b",sequence:1,event:&events[0]}]).is_err());assert_eq!(bytes(&root),before);assert_eq!(s.last_source_sequence("a"),1);assert!(s.accept("a",2,&events[2]).unwrap());
}
#[test]fn invalid_late_event_gap_or_reserved_reference_changes_nothing_and_does_not_poison(){
 for case in 0..4{let t=Temp::new();let root=t.0.join("bank");let mut s=Session::open(&root,None,&json!({})).unwrap();let before=bytes(&root);let good=ticks(2);let mut bad=good[1].clone();match case{0=>bad["type"]=json!("unsupported"),1=>bad["receivedAt"]=json!("2026-10-14T00:00:00Z"),2=>bad["sourceRef"]=json!({}),_=>{}}
  let entries=[BatchEntry{source_id:"a",sequence:1,event:&good[0]},BatchEntry{source_id:"a",sequence:if case==3{3}else{2},event:&bad}];assert!(s.accept_batch(&entries).is_err());assert_eq!(bytes(&root),before);assert_eq!(s.last_source_sequence("a"),0);assert!(s.accept_batch(&inputs("a",&good,1)).is_ok());
 }
}
#[test]fn count_raw_bytes_row_bytes_and_nested_shape_are_bounded_before_io(){
 let t=Temp::new();let root=t.0.join("bank");let mut s=Session::open(&root,None,&json!({})).unwrap();let before=bytes(&root);let many=ticks(SESSION_BATCH_MAX_EVENTS+1);assert!(s.accept_batch(&inputs("a",&many,1)).is_err());
 for row_overhead in [false,true]{let mut e=ticks(1).remove(0);e["padding"]=json!("x".repeat(if row_overhead{SESSION_BATCH_MAX_BYTES-100}else{SESSION_BATCH_MAX_BYTES+1}));if row_overhead{assert!(serde_json::to_vec(&e).unwrap().len()<SESSION_BATCH_MAX_BYTES);}assert!(s.accept_batch(&inputs("a",&[e],1)).is_err());}
 let mut nested=json!(0);for _ in 0..66{nested=json!({"child":nested});}let mut e=ticks(1).remove(0);e["ignored"]=nested;assert!(s.accept_batch(&inputs("a",&[e],1)).is_err());assert_eq!(bytes(&root),before);assert!(s.accept_batch(&[]).unwrap().accepted.is_empty());
}
#[test]fn duplicate_only_batch_does_not_touch_projection_paths(){let t=Temp::new();let root=t.0.join("bank");let mut s=Session::open(&root,None,&json!({})).unwrap();let e=ticks(1);s.accept_batch(&inputs("a",&e,1)).unwrap();fs::rename(root.join("head.json"),root.join("saved-head")).unwrap();fs::create_dir(root.join("head.json")).unwrap();let r=s.accept_batch(&inputs("a",&e,1)).unwrap();assert_eq!(r.accepted,vec![false]);assert_eq!(r.appended_rows,0);assert_eq!(r.encoded_journal_bytes,0);assert!(s.accept_batch(&[]).is_ok());}
#[test]fn failed_head_or_report_publication_poison_both_apis_then_recover_complete_prefix(){
 for failing in ["head.json","report.json"]{let t=Temp::new();let root=t.0.join("bank");let mut s=Session::open(&root,None,&json!({})).unwrap();let events=ticks(3);let mut expected=Engine::new(&json!({})).unwrap();for e in &events{expected.advance(e).unwrap();}
  fs::rename(root.join(failing),root.join("saved-projection")).unwrap();fs::create_dir(root.join(failing)).unwrap();assert!(s.accept_batch(&inputs("a",&events,1)).is_err());assert!(s.accept_batch(&[]).unwrap_err().to_string().contains("POISONED"));assert!(s.accept("a",1,&events[0]).unwrap_err().to_string().contains("POISONED"));drop(s);
  fs::remove_dir(root.join(failing)).unwrap();fs::rename(root.join("saved-projection"),root.join(failing)).unwrap();let mut recovered=Session::open(&root,None,&json!({})).unwrap();assert_eq!(recovered.engine().report().unwrap(),expected.report().unwrap());assert_eq!(recovered.last_source_sequence("a"),3);assert_eq!(recovered.accept_batch(&inputs("a",&events,1)).unwrap().accepted,vec![false;3]);
 }
}
#[test]fn any_complete_serial_prefix_ahead_of_old_head_is_recovered_without_batch_atomicity_claim(){
 let t=Temp::new();let full=t.0.join("full");let events=ticks(3);let mut source=Session::open(&full,None,&json!({})).unwrap();source.accept_batch(&inputs("a",&events,1)).unwrap();let all=fs::read(full.join("events.jsonl")).unwrap();let lines=all.split_inclusive(|c|*c==b'\n').collect::<Vec<_>>();
 for n in 1..=3{let root=t.0.join(format!("prefix-{n}"));drop(Session::open(&root,None,&json!({})).unwrap());let mut f=fs::OpenOptions::new().append(true).open(root.join("events.jsonl")).unwrap();for line in &lines[..n]{f.write_all(line).unwrap();}f.sync_all().unwrap();drop(f);let recovered=Session::open(&root,None,&json!({})).unwrap();assert_eq!(recovered.last_source_sequence("a"),n as u64);let mut expected=Engine::new(&json!({})).unwrap();for e in &events[..n]{expected.advance(e).unwrap();}assert_eq!(recovered.engine().report().unwrap(),expected.report().unwrap());}
}
#[test]fn partial_final_row_fails_closed_and_is_not_automatically_repaired(){let t=Temp::new();let root=t.0.join("bank");let mut s=Session::open(&root,None,&json!({})).unwrap();s.accept_batch(&inputs("a",&ticks(1),1)).unwrap();drop(s);let path=root.join("events.jsonl");let mut file=fs::OpenOptions::new().append(true).open(&path).unwrap();file.write_all(b"{\"sequence\":2").unwrap();file.sync_all().unwrap();drop(file);let before=fs::read(&path).unwrap();assert!(Session::open(&root,None,&json!({})).is_err());assert_eq!(fs::read(path).unwrap(),before);}
#[test]fn actual_ack_partial_fill_and_terminal_match_serial_without_synthetic_fill_credit(){
 let t=Temp::new();let policy=json!({"quantity":2});let seed=json!({"runId":"offline-fixture","availableCash":"1000.000000","accountSnapshotHash":"a".repeat(64),"observedAt":"2026-10-13T00:00:00Z"});let a=t.0.join("serial");let b=t.0.join("batch");let mut serial=Session::open_live(&a,&policy,&seed).unwrap();let mut batch=Session::open_live(&b,&policy,&seed).unwrap();let book=corpus()[0]["steps"][0]["event"].clone();serial.accept("book",1,&book).unwrap();batch.accept("book",1,&book).unwrap();let q=serial.engine().order_records().into_iter().find(|q|q["side"]=="YES").unwrap();
 let base=|kind:&str,at:&str|json!({"type":kind,"source":"official_sig_api_v1","receiptHash":"b".repeat(64),"orderId":q["id"],"actualOrderId":"201","exchangeId":q["exchangeId"],"marketId":q["marketId"],"side":q["side"],"action":"buy","quantity":q["quantity"],"price":q["price"],"receivedAt":at});
 let ack=base("actual_order_ack","2026-10-13T00:00:01Z");let mut fill=base("actual_fill","2026-10-13T00:00:05Z");fill["fillId"]=json!("301");fill["quantity"]=json!(1);fill["fee"]=json!("0.000000");fill["executedAt"]=json!("2026-10-13T00:00:04Z");let mut terminal=base("actual_order_terminal","2026-10-13T00:00:06Z");terminal["status"]=json!("canceled");terminal["fillsComplete"]=json!(true);terminal["filledQuantity"]=json!(1);let events=vec![ack,fill,terminal];
 for(i,e)in events.iter().enumerate(){serial.accept("owned",i as u64+1,e).unwrap();}batch.accept_batch(&inputs("owned",&events,1)).unwrap();parity(&a,&b);assert_eq!(batch.engine().report().unwrap()["fills"],1);assert_eq!(batch.engine().report().unwrap()["ownedCost"],"0.450000");assert_eq!(batch.engine().live_order_bindings().len(),1);
}
