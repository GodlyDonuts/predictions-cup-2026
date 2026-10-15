use super::*;

fn fixture()->PathBuf{let p=std::env::temp_dir().join(format!("sdk-v2-adapter-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));fs::create_dir_all(&p).unwrap();p}
fn binding(n:u64)->Binding{Binding{intent:format!("intent-{n}"),key:format!("key-{n}"),payload_hash:hash(&n)}}
fn command(op:&str,now:u64,write:bool,n:u64)->Value{json!({"op":op,"scope":"adapter","now":now,"request":format!("request-{n}"),"write":write,"binding":if write{serde_json::to_value(binding(n)).unwrap()}else{Value::Null},"authorized":true,"lease_valid":true,"permit_valid":true,"outcome":"success","status":200})}

#[test]fn typed_summary_rejects_missing_or_inconsistent_global_unknown_fields(){
    let dir=fixture();let a=AccountAdmission::open(&dir,"adapter","token").unwrap();let raw=serde_json::to_value(a.summary().unwrap()).unwrap();
    for key in ["summarySchemaVersion","globalUnknownCount","hasUnknown"]{let mut v=raw.clone();v.as_object_mut().unwrap().remove(key);assert!(AdmissionSummary::parse(v,"adapter").is_err(),"{key}");}
    for key in ["active","read_profile"]{let mut v=raw.clone();v["state"].as_object_mut().unwrap().remove(key);assert!(AdmissionSummary::parse(v,"adapter").is_err(),"{key}");}
    for (key,value) in [("hasUnknown",json!(true)),("globalUnknownCount",json!(1)),("summarySchemaVersion",json!(2))]{let mut v=raw.clone();v[key]=value;assert!(AdmissionSummary::parse(v,"adapter").is_err());}
    assert!(AdmissionSummary::parse(raw,"other-account").is_err());drop(a);fs::remove_dir_all(dir).unwrap();
}
#[test]fn new_queue_and_v1_generation_remain_v1_without_auto_migration(){
    let dir=fixture();{let mut a=AccountAdmission::open(&dir,"adapter","first").unwrap();assert!(!a.is_v2());a.command(command("status",1000,false,0)).unwrap();a.command_with_maintenance(command("compact",1000,false,0)).unwrap();assert_eq!(a.summary().unwrap().schema_version,1);}
    let a=AccountAdmission::open(&dir,"adapter","again").unwrap();assert!(!a.is_v2());let m:Manifest=serde_json::from_slice(&fs::read(dir.join("account-queue-manifest.json")).unwrap()).unwrap();assert_eq!(m.schema_version,1);drop(a);fs::remove_dir_all(dir).unwrap();
}
#[test]fn bounded_summary_and_lookup_do_not_append_or_serialize_v1_maps(){
    let dir=fixture();let mut a=AccountAdmission::open(&dir,"adapter","token").unwrap();let mut s=a.v1().state.clone();for n in 0..10000{let b=binding(n);s.keys.insert(b.key.clone(),b.clone());s.intents.insert(b.intent.clone(),b);}a.v1_mut().persist(s).unwrap();let sequence=a.v1().sequence;let bytes=a.v1().active_bytes;
    for _ in 0..5{let summary=a.summary().unwrap();let lookup=a.lookup_intent("intent-9999").unwrap();assert_eq!(summary.immutable_bindings,10000);assert_eq!(lookup.binding.as_ref().unwrap().key,"key-9999");assert!(serde_json::to_vec(&summary).unwrap().len()<2000);}
    assert_eq!(a.v1().sequence,sequence);assert_eq!(a.v1().active_bytes,bytes);drop(a);fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
mod unix {
    use super::*;
    fn migrated(setup:impl FnOnce(&mut AccountAdmission))->PathBuf{
        let dir=fixture();let mut a=AccountAdmission::open(&dir,"adapter","old").unwrap();a.configure_reads(100,600).unwrap();setup(&mut a);
        let Backend::V1(old)=a.store else{panic!("v1")};let p=v2::MigrationProof::capture(&old).unwrap();let v=v2::V2Store::migrate_v1(old,&p).unwrap();drop(v);dir
    }
    fn open(dir:&Path)->AccountAdmission{AccountAdmission::open(dir,"adapter","new").unwrap()}
    fn unknowns(a:&mut AccountAdmission,n:u64){let mut s=a.v1().state.clone();for i in 0..n{let b=binding(i);s.keys.insert(b.key.clone(),b.clone());s.intents.insert(b.intent.clone(),b.clone());s.unknown.insert(b.intent.clone(),b);}a.v1_mut().persist(s).unwrap();}
    fn fill_tail(a:&mut AccountAdmission){for now in 1..130{a.command(json!({"op":"summary","scope":"adapter","now":now})).unwrap();}}
    fn lower_cap(a:&mut AccountAdmission){let bytes=a.summary().unwrap().active_journal_bytes;assert!(bytes>70*1024);match &mut a.store{Backend::V2(s)=>s.maximum_bytes=bytes+10,_=>panic!("v2")}}

    #[test]fn explicit_v2_detection_refuses_legacy_status_and_old_v1_reader(){
        let dir=migrated(|_|{});let mut a=open(&dir);assert!(a.is_v2());assert_eq!(a.summary().unwrap().schema_version,2);assert_eq!(a.command(command("status",1000,false,0)).unwrap_err(),"V2_TYPED_SUMMARY_REQUIRED");
        let full=a.command(json!({"op":"diagnostic_status","scope":"adapter","now":1000})).unwrap();assert!(full["state"]["unknown"].is_object());drop(a);assert_eq!(Store::open(&dir,"adapter","old-binary").err().unwrap(),"MANIFEST_SCOPE_OR_PATH_FAILURE");fs::remove_dir_all(dir).unwrap();
    }
    #[test]fn global_unknown_remains_blocking_when_requested_lookup_is_empty(){
        let dir=migrated(|a|unknowns(a,2));let mut a=open(&dir);let summary=a.summary().unwrap();assert_eq!(summary.global_unknown_count,2);assert!(summary.has_unknown);let exact=a.lookup_intent("unrelated-new-intent").unwrap();assert!(exact.binding.is_none()&&exact.unknown_binding.is_none());assert!(exact.has_unknown);assert_eq!(exact.global_unknown_count,2);
        assert_eq!(a.command(command("admit",1000,true,99)).unwrap_err(),"UNKNOWN_ACCEPTANCE_RECONCILE_FIRST");let mut r=command("reconcile",1001,true,1);r["reconciled"]=json!(true);a.command(r).unwrap();assert!(a.lookup_intent("intent-1").unwrap().unknown_binding.is_none());assert!(a.lookup_intent("intent-1").unwrap().binding.is_some());assert_eq!(a.summary().unwrap().global_unknown_count,1);assert_eq!(a.command(command("admit",4000,true,99)).unwrap_err(),"UNKNOWN_ACCEPTANCE_RECONCILE_FIRST");drop(a);fs::remove_dir_all(dir).unwrap();
    }
    #[test]fn active_write_restart_preserves_binding_unknown_rate_and_profile(){
        let dir=migrated(|_|{});{let mut a=open(&dir);assert_eq!(a.command(command("admit",1000,true,7)).unwrap()["admitted"],true);a.command(command("network",1001,true,7)).unwrap();}
        let mut a=open(&dir);let s=a.summary().unwrap();assert_eq!(s.global_unknown_count,1);assert!(s.state.active.is_none());assert_eq!(s.state.writes,vec![1000]);assert!(s.state.next>=3501);assert_eq!(s.state.read_profile,Some(ReadBudget{cap:100,gap_ms:600}));let exact=a.lookup_intent("intent-7").unwrap();assert_eq!(exact.binding,exact.unknown_binding);assert_eq!(a.command(command("admit",5000,true,9)).unwrap_err(),"UNKNOWN_ACCEPTANCE_RECONCILE_FIRST");drop(a);fs::remove_dir_all(dir).unwrap();
    }
    #[test]fn migrated_profile_is_immutable_and_headroom_cannot_be_reseeded(){
        let dir=migrated(|_|{});let mut a=open(&dir);let sequence=a.summary().unwrap().sequence;a.configure_reads(100,600).unwrap();assert_eq!(a.summary().unwrap().sequence,sequence);assert_eq!(a.configure_reads(60,1000).unwrap_err(),"READ_PROFILE_IMMUTABLE");assert_eq!(a.seed_legacy_headroom(1000,61001,false).unwrap_err(),"MIGRATION_BOUNDARY_REQUIRES_FRESH_QUEUE");drop(a);fs::remove_dir_all(dir).unwrap();
        let dir=fixture();let old=Store::open(&dir,"adapter","old").unwrap();let p=v2::MigrationProof::capture(&old).unwrap();drop(v2::V2Store::migrate_v1(old,&p).unwrap());let mut a=open(&dir);assert_eq!(a.configure_reads(100,600).unwrap_err(),"READ_PROFILE_REQUIRES_FRESH_QUEUE");drop(a);fs::remove_dir_all(dir).unwrap();
    }
    #[test]fn rate_429_survives_restart_and_v2_read_admission_uses_migrated_profile(){
        let dir=migrated(|_|{});{let mut a=open(&dir);a.command(command("admit",1000,false,1)).unwrap();a.command(command("network",1001,false,1)).unwrap();let mut done=command("complete",1002,false,1);done["status"]=json!(429);done["outcome"]=json!("rejected");done["retry_after"]=json!("90");a.command(done).unwrap();}
        let mut a=open(&dir);assert_eq!(a.summary().unwrap().state.cooldown,91002);assert_eq!(a.command(command("admit",2000,false,2)).unwrap()["wait_until"],91002);assert_eq!(a.command(command("admit",91002,false,2)).unwrap()["next"],91602);drop(a);fs::remove_dir_all(dir).unwrap();
    }
    #[test]fn cap_maintenance_replays_only_local_network_marker_and_keeps_state(){
        let dir=migrated(|_|{});let mut a=open(&dir);fill_tail(&mut a);a.command(command("admit",1000,true,7)).unwrap();let before=a.summary().unwrap();lower_cap(&mut a);assert_eq!(a.command_with_maintenance(command("network",1001,true,7)).unwrap()["network_permitted"],true);let after=a.summary().unwrap();assert_eq!(after.immutable_bindings,before.immutable_bindings);assert_eq!(after.state.writes,before.state.writes);assert_eq!(after.state.read_profile,before.state.read_profile);assert!(after.state.active.unwrap().network_started);drop(a);let a=open(&dir);assert_eq!(a.summary().unwrap().global_unknown_count,1);drop(a);fs::remove_dir_all(dir).unwrap();
    }
    #[test]fn failed_outer_directory_pin_prevents_local_replay_and_poisons_v2(){
        let dir=migrated(|_|{});let mut a=open(&dir);fill_tail(&mut a);let before=a.summary().unwrap();lower_cap(&mut a);assert_eq!(a.command_with_maintenance_sync(command("admit",1000,true,7),|_|Err("injected".into())).unwrap_err(),"COMPACTION_DURABILITY_FAILURE");assert_eq!(a.command(command("admit",1001,true,7)).unwrap_err(),"DURABILITY_FAILURE");drop(a);let a=open(&dir);assert_eq!(a.summary().unwrap().immutable_bindings,before.immutable_bindings);assert!(a.lookup_intent("intent-7").unwrap().binding.is_none());drop(a);fs::remove_dir_all(dir).unwrap();
    }
    #[test]fn malformed_selected_v2_never_falls_back_to_v1(){
        for mode in 0..4{let dir=migrated(|_|{});let path=dir.join("account-queue-manifest.json");let m:Manifest=serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();match mode{0=>fs::remove_file(dir.join("account-queue-generation.guard")).unwrap(),1=>fs::remove_file(dir.join(m.checkpoint)).unwrap(),2=>fs::remove_file(dir.join(m.journal)).unwrap(),_=>fs::remove_file(path).unwrap()};assert!(AccountAdmission::open(&dir,"adapter","bad").is_err());assert!(dir.join("account-queue.jsonl").exists());fs::remove_dir_all(dir).unwrap();}
    }
    #[test]fn normal_v2_summary_lookup_are_bounded_and_do_not_append(){
        let dir=migrated(|a|{let mut s=a.v1().state.clone();for n in 0..10000{let b=binding(n);s.keys.insert(b.key.clone(),b.clone());s.intents.insert(b.intent.clone(),b);}a.v1_mut().persist(s).unwrap();});let a=open(&dir);let sequence=a.summary().unwrap().sequence;let bytes=a.summary().unwrap().active_journal_bytes;for _ in 0..5{let s=a.summary().unwrap();assert_eq!(s.immutable_bindings,10000);assert!(serde_json::to_vec(&s).unwrap().len()<2000);assert_eq!(a.lookup_intent("intent-9999").unwrap().binding.unwrap().key,"key-9999");assert_eq!(s.sequence,sequence);assert_eq!(s.active_journal_bytes,bytes);}drop(a);fs::remove_dir_all(dir).unwrap();
    }

    use std::{net::TcpListener,sync::{Arc,atomic::{AtomicBool,AtomicU64,Ordering}},thread,time::Duration};
    struct FakeClock(Arc<AtomicU64>);
    impl crate::Clock for FakeClock{fn now_ms(&self)->u64{self.0.load(Ordering::SeqCst)}fn sleep_ms(&mut self,n:u64)->crate::Result<()>{self.0.fetch_add(n,Ordering::SeqCst);Ok(())}}
    struct Credentials(Arc<AtomicU64>);
    impl crate::CredentialProvider for Credentials{fn load(&self)->crate::Result<zeroize::Zeroizing<String>>{self.0.fetch_add(1,Ordering::SeqCst);Ok(zeroize::Zeroizing::new("OFFLINE_FIXTURE_ONLY".into()))}}
    struct Authority;
    impl crate::Authority for Authority{fn can_read(&self,_:&crate::Scope)->bool{true}fn authorize(&self,_:&crate::Scope,_:&crate::RequestSpec,_:&crate::Permit,_:bool,_:u64)->crate::Result<()>{Ok(())}fn owns_order(&self,_:&str)->bool{true}fn reconcile_unknown(&self,_:&crate::Scope,_:&Value,_:u64)->crate::Result<()>{Ok(())}}
    struct Server{base:String,count:Arc<AtomicU64>,running:Arc<AtomicBool>,worker:Option<thread::JoinHandle<()>>}
    impl Server{fn new(bodies:Vec<Value>)->Self{
        let socket=TcpListener::bind("127.0.0.1:0").unwrap();socket.set_nonblocking(true).unwrap();let base=format!("http://{}/api/v1",socket.local_addr().unwrap());let count=Arc::new(AtomicU64::new(0));let seen=count.clone();let running=Arc::new(AtomicBool::new(true));let alive=running.clone();
        let worker=thread::spawn(move||{let mut bodies=bodies.into_iter();while alive.load(Ordering::SeqCst){let (mut stream,_)=match socket.accept(){Ok(s)=>s,Err(e)if e.kind()==std::io::ErrorKind::WouldBlock=>{thread::sleep(Duration::from_millis(2));continue},Err(_)=>break};stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();let mut raw=Vec::new();let mut buffer=[0u8;4096];let end=loop{let n=stream.read(&mut buffer).unwrap_or(0);if n==0{break None}raw.extend_from_slice(&buffer[..n]);if let Some(i)=raw.windows(4).position(|w|w==b"\r\n\r\n"){break Some(i+4)}if raw.len()>65536{break None}};let Some(end)=end else{continue};let head=String::from_utf8_lossy(&raw[..end]);let length=head.lines().find_map(|s|s.to_ascii_lowercase().strip_prefix("content-length:").and_then(|n|n.trim().parse::<usize>().ok())).unwrap_or(0);while raw.len()<end+length{let n=stream.read(&mut buffer).unwrap_or(0);if n==0{break}raw.extend_from_slice(&buffer[..n]);}seen.fetch_add(1,Ordering::SeqCst);let Some(body)=bodies.next() else{continue};let body=body.to_string();let response=format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());let _=stream.write_all(response.as_bytes());}});
        Self{base,count,running,worker:Some(worker)}
    }}
    impl Drop for Server{fn drop(&mut self){self.running.store(false,Ordering::SeqCst);if let Some(w)=self.worker.take(){w.join().unwrap();}}}
    const TID:&str="bda92870-621e-47b0-bc3c-3602c5c26f55";
    fn transport(dir:&Path,server:&Server,loads:Arc<AtomicU64>)->crate::Transport{
        let config=crate::Config{queue_dir:dir.into(),account_scope:"adapter".into(),scope:crate::Scope{tournament_id:TID.into(),tournament_slug:"midterm-elections".into(),exchanges:BTreeMap::from([("994".into(),"305".into())])},maximum_pages:5,writes_enabled:true};
        crate::Transport::new_for_mock(config,Box::new(Credentials(loads)),Box::new(FakeClock(Arc::new(AtomicU64::new(1000)))),Box::new(Authority),server.base.clone()).unwrap()
    }
    fn order()->crate::Order{crate::Order{idempotency_key:"fixture-key".into(),exchange_id:"994".into(),side:"no".into(),action:"buy".into(),quantity:1,price:"0.120000".into(),expiration_date:"2030-01-01T00:00:00Z".into(),tournament_id:TID.into()}}
    fn permit()->crate::Permit{crate::Permit{run_id:"fixture".into(),intent_id:"new".into(),owner:"Codex".into(),expires_at_ms:10000000,maximum_quantity:1,maximum_cost:"1.000000".into()}}

    #[test]fn transport_v2_normal_read_uses_typed_snapshot_without_legacy_status(){
        let dir=migrated(|_|{});let body=json!({"id":TID,"slug":"midterm-elections","myBalance":100000.123456,"status":"active","isPendingEnrolment":false});let server=Server::new(vec![body]);let loads=Arc::new(AtomicU64::new(0));let mut t=transport(&dir,&server,loads.clone());
        assert_eq!(t.admission_state().unwrap_err().code,"V2_TYPED_SUMMARY_REQUIRED");assert!(t.request(crate::Endpoint::Tournament).is_ok());let summary=t.admission_summary().unwrap();assert_eq!(summary.state.reads,vec![1000]);assert!(!summary.has_unknown);assert!(summary.state.active.is_none());assert_eq!(server.count.load(Ordering::SeqCst),1);assert_eq!(loads.load(Ordering::SeqCst),1);assert!(t.admission_diagnostic_state().unwrap()["state"]["intents"].is_object());drop(t);fs::remove_dir_all(dir).unwrap();
    }
    #[test]fn transport_global_unknown_blocks_unrelated_write_then_exact_recovery_keeps_key(){
        let dir=migrated(|a|unknowns(a,1));let ack=json!({"orderId":42,"exchangeId":"994","open":true,"remainingQuantity":1,"quantity":1,"quantityTraded":0,"totalCost":0,"fillPrice":null,"price":0.12,"side":"no","action":"buy","all":{}});let server=Server::new(vec![ack]);let loads=Arc::new(AtomicU64::new(0));let mut t=transport(&dir,&server,loads.clone());
        let lookup=t.admission_lookup_intent("fixture:new").unwrap();assert!(lookup.binding.is_none()&&lookup.has_unknown);assert_eq!(t.place(order(),permit()).err().unwrap().code,"UNKNOWN_ACCEPTANCE_RECONCILE_FIRST");assert_eq!(server.count.load(Ordering::SeqCst),0);assert_eq!(loads.load(Ordering::SeqCst),0);
        t.reconcile_unknown(serde_json::to_value(binding(0)).unwrap()).unwrap();let lookup=t.admission_lookup_intent("intent-0").unwrap();assert!(lookup.binding.is_some()&&lookup.unknown_binding.is_none());assert!(!t.admission_summary().unwrap().has_unknown);
        assert!(t.place(order(),permit()).is_ok());assert_eq!(t.place(order(),permit()).err().unwrap().code,"KNOWN_INTENT_RECONCILE_FIRST");assert_eq!(server.count.load(Ordering::SeqCst),1);assert_eq!(loads.load(Ordering::SeqCst),1);drop(t);fs::remove_dir_all(dir).unwrap();
    }
}
