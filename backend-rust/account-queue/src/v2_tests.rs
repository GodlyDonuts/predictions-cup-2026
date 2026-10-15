use super::*;

fn command(op:&str,now:u64,write:bool)->Command{Command{op:op.into(),scope:"fixture-scope".into(),now,request:format!("request-{now}"),write,binding:if write{Some(binding(now))}else{None},authorized:true,permit_valid:true,lease_valid:true,..Default::default()}}
fn binding(i:u64)->Binding{Binding{intent:format!("intent-{i:012}"),key:format!("key-{i:012}"),payload_hash:hash(&i)}}
fn populated(n:u64)->State{let mut s=State::new("fixture-scope".into());for i in 0..n{let b=binding(i);s.keys.insert(b.key.clone(),b.clone());s.intents.insert(b.intent.clone(),b);}s}
fn equivalent(s:&mut State,c:Command)->Result<Value>{
    let mut oracle=s.clone();let expected=oracle.apply(&c);
    let (d,result)=prepare(s,&Transition::Command(c.clone()));
    assert_eq!(result.as_ref().err(),expected.as_ref().err());
    assert_eq!(d.is_some(),expected.is_ok()||oracle.manual_review);
    if let Some(d)=d{validate_delta(s,&d).unwrap();install(s,&d);assert_eq!(*s,oracle);}
    if c.op!="status"{assert_eq!(result,expected);}
    expected
}
fn finish(s:&mut State,c:&Command,now:u64,outcome:&str,status:u16){
    let mut n=command("network",c.now,c.write);n.request=c.request.clone();n.binding=c.binding.clone();equivalent(s,n).unwrap();
    let mut done=command("complete",now,c.write);done.request=c.request.clone();done.outcome=outcome.into();done.status=status;done.binding=None;equivalent(s,done).unwrap();
}

#[test] fn sparse_projection_preserves_global_unknown_gate(){
    let mut s=populated(3);for i in 0..2{let b=binding(i);s.unknown.insert(b.intent.clone(),b);}
    assert_eq!(projection(&s,Some(&command("admit",1000,true))).unknown.len(),1);
    assert_eq!(equivalent(&mut s,command("admit",1000,true)).unwrap_err(),"UNKNOWN_ACCEPTANCE_RECONCILE_FIRST");
    let mut r=command("reconcile",1001,true);r.binding=Some(binding(1));r.reconciled=true;
    equivalent(&mut s,r).unwrap();assert_eq!(s.unknown.len(),1);assert!(s.unknown.contains_key(&binding(0).intent));
    assert_eq!(equivalent(&mut s,command("admit",4000,true)).unwrap_err(),"UNKNOWN_ACCEPTANCE_RECONCILE_FIRST");
}
#[test] fn sparse_projection_checks_both_global_binding_indexes(){
    let mut s=populated(4);let mut c=command("admit",1000,true);let mut b=binding(1);b.payload_hash="b".repeat(64);c.binding=Some(b);
    assert_eq!(equivalent(&mut s,c).unwrap_err(),"LOGICAL_REQUEST_CHANGED");
    let mut c=command("admit",1000,true);let mut b=binding(99);b.key=binding(2).key;c.binding=Some(b);
    assert_eq!(equivalent(&mut s,c).unwrap_err(),"IDEMPOTENCY_PAYLOAD_CHANGED");
    let mut c=command("admit",1000,true);c.binding=Some(binding(3));assert_eq!(equivalent(&mut s,c).unwrap_err(),"KNOWN_INTENT_RECONCILE_FIRST");
}
#[test] fn deterministic_state_response_parity_over_mixed_requests(){
    for seed in 0..8{
        let mut s=populated(12);let mut now=1000;let mut rng=seed+1u64;
        for i in 0..180{
            rng=rng.wrapping_mul(6364136223846793005).wrapping_add(1);let write=rng&1==0;
            let mut c=command("admit",now,write);c.binding=if write{Some(binding(100+i+seed*1000))}else{None};
            if i%17==0{let mut bad=c.clone();bad.scope="wrong".into();equivalent(&mut s,bad).unwrap_err();}
            if i%13==0 && write{let mut bad=c.clone();bad.authorized=false;equivalent(&mut s,bad).unwrap_err();}
            let mut r=equivalent(&mut s,c.clone()).unwrap();
            if r["admitted"]==false{now=r["wait_until"].as_u64().unwrap();c.now=now;r=equivalent(&mut s,c.clone()).unwrap();}
            assert_eq!(r["admitted"],true);
            let mut wrong=command("network",now,false);wrong.request="wrong".into();equivalent(&mut s,wrong).unwrap_err();
            let unknown=write && i%19==0;finish(&mut s,&c,now+5,if unknown{"unknown"}else{"success"},if i%23==0{503}else{200});
            if unknown{let mut rc=command("reconcile",now+6,true);rc.reconciled=true;rc.binding=c.binding.clone();equivalent(&mut s,rc).unwrap();}
            now=s.next.max(s.cooldown);
        }
    }
}
#[test] fn rejected_clock_pruning_and_manual_poison_match_v1(){
    let mut s=State::new("fixture-scope".into());let c=command("admit",1000,false);equivalent(&mut s,c.clone()).unwrap();finish(&mut s,&c,1100,"success",200);
    let before=s.clone();equivalent(&mut s,command("bad-op",90000,false)).unwrap_err();assert_eq!(s,before);
    let mut wrong=command("status",90000,false);wrong.scope="wrong".into();equivalent(&mut s,wrong).unwrap_err();assert_eq!(s,before);
    equivalent(&mut s,command("status",MAX_TIME+1,false)).unwrap_err();assert_eq!(s,before);
    assert_eq!(equivalent(&mut s,command("status",1099,false)).unwrap_err(),"CLOCK_REGRESSION");assert!(s.manual_review);
    equivalent(&mut s,command("admit",90000,false)).unwrap_err();assert_eq!(s.last_clock,90000);assert!(s.reads.is_empty());
    equivalent(&mut s,command("status",90001,false)).unwrap();
}
#[test] fn all_completion_errors_backoffs_and_retry_overflow_match(){
    for status in [200,429,503]{for outcome in ["success","unknown","rejected","not_sent","invalid"]{
        let mut s=populated(3);let c=command("admit",1000,true);equivalent(&mut s,c.clone()).unwrap();
        let mut done=command("complete",1001,true);done.request=c.request.clone();done.status=status;done.outcome=outcome.into();
        let _=equivalent(&mut s,done.clone());
        if s.active.is_some(){let mut n=command("network",1002,true);n.request=c.request.clone();n.binding=c.binding.clone();equivalent(&mut s,n).unwrap();done.now=1003;let _=equivalent(&mut s,done);}
    }}
    for retry in ["1.25","Thu, 01 Jan 1970 00:02:00 GMT","bad","99999999999999999999999"]{
        let mut s=State::new("fixture-scope".into());let c=command("admit",1000,false);equivalent(&mut s,c.clone()).unwrap();let mut n=command("network",1000,false);n.request=c.request.clone();equivalent(&mut s,n).unwrap();
        let mut done=command("complete",1001,false);done.request=c.request;done.status=429;done.outcome="rejected".into();done.retry_after=retry.into();equivalent(&mut s,done).unwrap();
    }
    let mut s=State::new("fixture-scope".into());let c=command("admit",1000,false);equivalent(&mut s,c.clone()).unwrap();let mut n=command("network",1000,false);n.request=c.request.clone();equivalent(&mut s,n).unwrap();let mut d=command("complete",MAX_TIME,false);d.request=c.request;d.outcome="rejected".into();d.api_code="REQUEST_IN_FLIGHT".into();equivalent(&mut s,d).unwrap_err();
}
#[test] fn read_profile_and_recovery_match_without_full_map_copy(){
    for write in [false,true]{for started in [false,true]{let mut s=populated(100);s.read_profile=Some(ReadProfile{cap:100,gap_ms:600});let c=command("admit",1000,write);equivalent(&mut s,c.clone()).unwrap();if started{let mut n=command("network",1001,write);n.binding=c.binding;n.request=c.request;equivalent(&mut s,n).unwrap();}
        let mut oracle=s.clone();oracle.recover();let (d,_)=prepare(&s,&Transition::Recovery);let d=d.unwrap();validate_delta(&s,&d).unwrap();install(&mut s,&d);assert_eq!(s,oracle);
    }}
}
#[test] fn normal_delta_size_independent_of_permanent_map_cardinality(){
    let c=command("admit",100000,true);let t=Transition::Command(c);
    let a=prepare(&populated(1),&t).0.unwrap();let b=prepare(&populated(10000),&t).0.unwrap();
    assert_eq!(a,b);assert!(serde_json::to_vec(&a).unwrap().len()<2000);
    assert!(projection(&populated(10000),None).intents.is_empty());
}
#[test] fn bounded_line_rejects_oversize_torn_and_empty(){
    assert!(bounded_line(&mut BufReader::new(&b"abc"[..])).is_err());
    assert!(bounded_line(&mut BufReader::new(&vec![b'a';MAX_ROW_BYTES as usize+1][..])).is_err());
    assert!(bounded_line(&mut BufReader::new(&b""[..])).unwrap().is_none());
}
#[test] fn checkpoint_stream_roundtrip_counts_and_tamper_detection(){
    let dir=fixture();let mut s=populated(1200);let b=binding(30);s.unknown.insert(b.intent.clone(),b);let h=header(&s);
    let path=dir.join("stream.jsonl");let digest=write_checkpoint(&path,&s,h).unwrap();let (actual,_)=read_checkpoint(&path,&s.scope,&digest).unwrap();assert_eq!(actual,s);
    let mut raw=fs::read(&path).unwrap();let n=raw.len();raw[n/2]=b'X';fs::write(&path,raw).unwrap();assert!(read_checkpoint(&path,&s.scope,&digest).is_err());fs::remove_dir_all(dir).unwrap();
}
#[test] fn malformed_binding_maps_never_migrate_or_checkpoint(){
    let mut s=populated(3);s.keys.remove(&binding(0).key);assert!(validate_maps(&s).is_err());
    let dir=fixture();assert!(write_checkpoint(&dir.join("bad.jsonl"),&s,header(&s)).is_err());fs::remove_dir_all(dir).unwrap();
}
fn fixture()->PathBuf{let p=std::env::temp_dir().join(format!("moloch-queue-v2-test-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));fs::create_dir_all(&p).unwrap();p}
fn header(s:&State)->Header{Header{schema_version:2,anchor:Anchor{sequence:0,previous:String::new(),source_journal:"account-queue.jsonl".into(),source_bytes:0,source_sha256:hash(&0u64),prior_checkpoint_hash:None},operational:Operational::capture(s),migration:MigrationProof{schema_version:1,scope:s.scope.clone(),sequence:0,previous:String::new(),state_sha256:hash(s),source_journal:"account-queue.jsonl".into(),source_bytes:0,source_sha256:hash(&0u64),checkpoint_hash:None,manifest_sha256:None},bindings:s.intents.len(),unknowns:s.unknown.len()}}

#[cfg(unix)]
mod durable {
    use super::*;
    fn migrated()->(PathBuf,V2Store){let dir=fixture();let mut old=Store::open(&dir,"fixture-scope","token").unwrap();old.command(command("status",10,false)).unwrap();let proof=MigrationProof::capture(&old).unwrap();let s=V2Store::migrate_v1(old,&proof).unwrap();(dir,s)}
    #[test] fn explicit_migration_preserves_exact_state_head_and_all_old_files(){
        let dir=fixture();let mut old=Store::open(&dir,"fixture-scope","token").unwrap();let state=populated(2000);old.persist(state).unwrap();old.command(command("compact",0,false)).unwrap();old.command(command("status",100,false)).unwrap();
        let proof=MigrationProof::capture(&old).unwrap();let expected=old.state.clone();let files:Vec<_>=fs::read_dir(&dir).unwrap().map(|e|e.unwrap().path()).filter(|p|p.file_name().unwrap()!="offline-account-queue.lock" && p.file_name().unwrap()!="account-queue-manifest.json").map(|p|{let d=source_digest(&p).unwrap();(p,d)}).collect();
        let mut s=V2Store::migrate_v1(old,&proof).unwrap();assert_eq!(s.state,expected);assert_eq!(s.sequence,proof.sequence);assert_eq!(s.previous,proof.previous);
        for (p,d) in files{assert_eq!(source_digest(&p).unwrap(),d);}
        let r=s.command(command("summary",100,false)).unwrap();assert_eq!(r["immutable_bindings"],2000);assert_eq!(r["globalUnknownCount"],0);assert_eq!(r["hasUnknown"],false);assert!(r["state"].get("intents").is_none());
        let mut q=command("lookup_intent",101,false);q.request=binding(33).intent;let r=s.command(q).unwrap();assert_eq!(r["binding"],serde_json::to_value(binding(33)).unwrap());
        drop(s);assert_eq!(Store::open(&dir,"fixture-scope","old").err().unwrap(),"MANIFEST_SCOPE_OR_PATH_FAILURE");
        let s=V2Store::open(&dir,"fixture-scope","new").unwrap();assert_eq!(s.state.intents.len(),2000);assert_eq!(s.migration,proof);drop(s);fs::remove_dir_all(dir).unwrap();
    }
    #[test] fn stale_or_modified_migration_proof_does_not_switch(){
        for field in 0..4{let dir=fixture();let mut old=Store::open(&dir,"fixture-scope","token").unwrap();old.command(command("status",10,false)).unwrap();let mut p=MigrationProof::capture(&old).unwrap();match field{0=>p.sequence+=1,1=>p.state_sha256=hash(&42),2=>p.source_bytes+=1,_=>p.source_sha256=hash(&42)};
            assert_eq!(V2Store::migrate_v1(old,&p).err().unwrap(),"V2_MIGRATION_SOURCE_PROOF_MISMATCH");assert!(!dir.join("account-queue-manifest.json").exists());let s=Store::open(&dir,"fixture-scope","old").unwrap();assert_eq!(s.sequence,1);drop(s);fs::remove_dir_all(dir).unwrap();}
    }
    #[test] fn crash_after_durable_write_fence_restores_unknown_before_admission(){
        let (dir,mut s)=migrated();let c=command("admit",1000,true);s.command(c.clone()).unwrap();let mut n=command("network",1001,true);n.request=c.request;n.binding=c.binding.clone();s.command(n).unwrap();let pre=s.sequence;drop(s);
        let mut s=V2Store::open(&dir,"fixture-scope","recovered").unwrap();assert_eq!(s.sequence,pre+1);assert!(s.state.active.is_none());assert_eq!(s.state.unknown.get(&c.binding.as_ref().unwrap().intent),c.binding.as_ref());
        let r=s.command(command("summary",4000,false)).unwrap();assert_eq!(r["globalUnknownCount"],1);assert_eq!(r["hasUnknown"],true);assert_eq!(s.command(command("admit",4001,true)).unwrap_err(),"UNKNOWN_ACCEPTANCE_RECONCILE_FIRST");drop(s);fs::remove_dir_all(dir).unwrap();
    }
    #[test] fn identical_actual_store_results_across_v1_v2_and_restarts(){
        let (dir,mut s)=migrated();let mut oracle=s.state.clone();let mut now=1000;
        for i in 0..100{let write=i%3==0;let c=command("admit",now,write);let expected=equivalent(&mut oracle,c.clone()).unwrap();assert_eq!(s.command(c.clone()).unwrap(),expected);let mut n=command("network",now,write);n.request=c.request.clone();n.binding=c.binding.clone();assert_eq!(s.command(n.clone()),equivalent(&mut oracle,n));let mut d=command("complete",now+1,write);d.request=c.request;d.outcome="success".into();assert_eq!(s.command(d.clone()),equivalent(&mut oracle,d));now=oracle.next.max(oracle.cooldown);assert_eq!(s.state,oracle);if i%17==0{s.command(command("compact",now,false)).unwrap();drop(s);s=V2Store::open(&dir,"fixture-scope","again").unwrap();assert_eq!(s.state,oracle);}}
        assert_eq!(s.command(command("status",now,false)).unwrap(),equivalent(&mut oracle,command("status",now,false)).unwrap());drop(s);fs::remove_dir_all(dir).unwrap();
    }
    #[test] fn migration_faults_select_only_v1_or_complete_v2_and_preserve_sources(){
        for stage in ["checkpoint_synced","journal_synced","manifest_synced","generation_pinned","manifest_switched","manifest_pinned"]{
            let dir=fixture();let mut old=Store::open(&dir,"fixture-scope","token").unwrap();old.command(command("status",10,false)).unwrap();old.command(command("compact",10,false)).unwrap();let p=MigrationProof::capture(&old).unwrap();let source=source_digest(&dir.join(&old.journal_name)).unwrap();let path=dir.join(&old.journal_name);
            let error=V2Store::migrate_with_hook(old,&p,|s|if s==stage{Err("INJECTED".into())}else{Ok(())},pin_directory).err().unwrap();assert_eq!(error,"INJECTED");assert_eq!(source_digest(&path).unwrap(),source);
            if stage.starts_with("manifest_swi")||stage=="manifest_pinned"{let s=V2Store::open(&dir,"fixture-scope","new").unwrap();assert_eq!(s.sequence,p.sequence);drop(s);}else{let s=Store::open(&dir,"fixture-scope","old").unwrap();assert_eq!(s.sequence,p.sequence);drop(s);}fs::remove_dir_all(dir).unwrap();
        }
    }
    #[test] fn first_generation_interruption_cannot_fall_back_to_original_history(){
        let dir=fixture();let old=Store::open(&dir,"fixture-scope","token").unwrap();let p=MigrationProof::capture(&old).unwrap();assert!(V2Store::migrate_with_hook(old,&p,|s|if s=="generation_pinned"{Err("INJECTED".into())}else{Ok(())},pin_directory).is_err());assert!(Store::open(&dir,"fixture-scope","old").is_err());assert!(V2Store::open(&dir,"fixture-scope","new").is_err());assert!(dir.join("account-queue.jsonl").exists());fs::remove_dir_all(dir).unwrap();
    }
    #[test] fn directory_pin_failure_poison_never_allows_append(){
        for failure in [1,2]{let (dir,mut s)=migrated();let mut count=0;let error=s.compact_with_hook(|_|Ok(()),|p|{count+=1;if count==failure{Err("INJECTED_DIR_FAILURE".into())}else{pin_directory(p)}}).unwrap_err();assert_eq!(error,"INJECTED_DIR_FAILURE");assert!(s.poisoned);assert_eq!(s.command(command("admit",1000,false)).unwrap_err(),"DURABILITY_FAILURE");drop(s);let s=V2Store::open(&dir,"fixture-scope","new").unwrap();drop(s);fs::remove_dir_all(dir).unwrap();}
    }
    #[test] fn append_fault_durable_prefix_replays_or_conservatively_recovers(){
        for stage in ["before_append","append_synced"]{let (dir,mut s)=migrated();let t=Transition::Command(command("admit",1000,true));let d=prepare(&s.state,&t).0.unwrap();let before=s.state.clone();assert_eq!(s.persist_with_hook(t,d,|point|if point==stage{Err("INJECTED".into())}else{Ok(())}).unwrap_err(),"INJECTED");assert_eq!(s.state,before);assert!(s.poisoned);drop(s);let s=V2Store::open(&dir,"fixture-scope","new").unwrap();assert_eq!(s.state.unknown.len(),if stage=="append_synced"{1}else{0});drop(s);fs::remove_dir_all(dir).unwrap();}
    }
    #[test] fn partial_append_is_poisoned_and_restart_refuses_torn_tail(){
        let (dir,mut s)=migrated();let path=dir.join(&s.journal_name);let t=Transition::Command(command("admit",1000,true));let d=prepare(&s.state,&t).0.unwrap();let before=s.state.clone();
        assert_eq!(s.persist_with_hook(t,d,|point|{if point=="before_append"{let mut f=OpenOptions::new().append(true).open(&path).unwrap();f.write_all(b"{\"schema_version\":2,\"sequence\":").unwrap();f.sync_all().unwrap();return Err("INJECTED_PARTIAL_WRITE".into());}Ok(())}).unwrap_err(),"INJECTED_PARTIAL_WRITE");
        assert!(s.poisoned);assert_eq!(s.state,before);assert_eq!(s.command(command("admit",1001,true)).unwrap_err(),"DURABILITY_FAILURE");drop(s);
        assert!(V2Store::open(&dir,"fixture-scope","new").is_err());assert!(dir.join("account-queue.jsonl").exists());fs::remove_dir_all(dir).unwrap();
    }
    #[test] fn source_mutation_during_checkpoint_prevents_manifest_selection(){
        let (dir,mut s)=migrated();let source=dir.join(&s.journal_name);let before=fs::read(dir.join("account-queue-manifest.json")).unwrap();
        assert_eq!(s.compact_with_hook(|point|{if point=="checkpoint_synced"{OpenOptions::new().append(true).open(&source).unwrap().write_all(b"invalid\n").unwrap();}Ok(())},pin_directory).unwrap_err(),"SOURCE_JOURNAL_CHANGED");assert!(s.poisoned);assert_eq!(fs::read(dir.join("account-queue-manifest.json")).unwrap(),before);drop(s);fs::remove_dir_all(dir).unwrap();
    }
    #[test] fn cap_refusal_is_nonmutating_and_preserves_recovery_reserve(){
        let (dir,mut s)=migrated();s.maximum_bytes=MAX_ROW_BYTES;let before=s.state.clone();assert_eq!(s.command(command("admit",1000,true)).unwrap_err(),"EXPLICIT_COMPACTION_REQUIRED");assert_eq!(s.state,before);assert!(!s.poisoned);s.maximum_bytes=MAX_ACTIVE_JOURNAL_BYTES;s.command(command("admit",1000,true)).unwrap();let bytes=s.active_bytes;assert!(bytes+MAX_ROW_BYTES<=s.maximum_bytes);drop(s);let s=V2Store::open(&dir,"fixture-scope","new").unwrap();assert_eq!(s.state.unknown.len(),1);drop(s);fs::remove_dir_all(dir).unwrap();
    }
    #[test] fn corruption_torn_tail_missing_manifest_or_tail_never_falls_back(){
        for mode in 0..4{let (dir,mut s)=migrated();s.command(command("status",100,false)).unwrap();let path=dir.join(&s.journal_name);drop(s);match mode{0=>{let mut file=OpenOptions::new().append(true).open(path).unwrap();file.write_all(b"{\"sequence\":").unwrap();},1=>{let raw=fs::read_to_string(&path).unwrap().replace("fixture-scope","wrong-scope");fs::write(path,raw).unwrap();},2=>{fs::remove_file(dir.join("account-queue-manifest.json")).unwrap();},_=>{fs::remove_file(path).unwrap();}}assert!(V2Store::open(&dir,"fixture-scope","new").is_err());assert!(Store::open(&dir,"fixture-scope","old").is_err());fs::remove_dir_all(dir).unwrap();}
    }
    #[test] fn lost_lease_and_lease_change_mid_checkpoint_never_continue(){
        let (dir,mut s)=migrated();let lock=s.lock_path.clone();assert_eq!(s.compact_with_hook(|p|{if p=="checkpoint_synced"{fs::write(&lock,b"replacement").unwrap();}Ok(())},pin_directory).unwrap_err(),"SCOPED_LEASE_LOST");assert!(s.poisoned);drop(s);assert_eq!(fs::read(&lock).unwrap(),b"replacement");fs::remove_dir_all(dir).unwrap();
    }
}
