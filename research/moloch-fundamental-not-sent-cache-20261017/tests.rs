use super::*;
use std::sync::Mutex;

struct F{dir:PathBuf,path:PathBuf,log:DispatchLog,identity:Value,cache:Mutex<NotSentRowsCache>}
impl Drop for F{fn drop(&mut self){let _=fs::remove_dir_all(&self.dir);}}
impl F{
    fn new()->Self{
        static NEXT:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);
        let dir=std::env::temp_dir().join(format!("not-sent-original-cache-{}-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),NEXT.fetch_add(1,std::sync::atomic::Ordering::Relaxed)));
        fs::create_dir(&dir).unwrap();let path=dir.join("dispatch.ndjson");let log=DispatchLog::open(&path).unwrap();let identity=identity(&File::open(&path).unwrap()).unwrap();
        Self{dir,path,log,identity,cache:Default::default()}
    }
    fn not_sent(&mut self,id:u64,padding:usize){
        self.log.append(json!({"kind":"intent","qid":id})).unwrap();let proof=self.log.proof(id);
        self.log.append(json!({"kind":"not_sent","qid":id,"proof":proof,"diagnostic":"x".repeat(padding)})).unwrap();
    }
    fn read(&self,wanted:&[u64])->Result<(BTreeMap<u64,Value>,FilePin)>{original_not_sent_rows(&self.cache,&self.path,&self.identity,&self.log,&wanted.iter().copied().collect())}
    fn parses(&self)->u64{self.cache.lock().unwrap().parses}
    #[cfg(target_os="linux")]
    fn sdk_abort(&mut self,id:u64)->PathBuf{
        use baal_transport_service::{Config,Permit,RequestSpec,TransportError,admission::AccountAdmission};
        let scope=Scope{tournament_id:"11111111-1111-4111-8111-111111111111".into(),tournament_slug:"cup".into(),exchanges:[("1009".into(),"319".into())].into()};
        let spec=RequestSpec{method:"POST".into(),endpoint:"/orders".into(),body:Some(json!({"idempotencyKey":format!("cache:{id}"),"exchangeId":"1009","side":"yes","action":"buy","quantity":1,"price":0.5,"expirationDate":"2030-01-01T00:00:00.000Z","tournamentId":scope.tournament_id})),body_json:None};
        let permit=Permit{run_id:"cache".into(),intent_id:id.to_string(),owner:"Codex".into(),expires_at_ms:1999,maximum_quantity:1,maximum_cost:"0.500000".into()};
        let error=TransportError{code:"PERMIT_EXPIRED_OR_INVALID".into(),status:None,may_have_executed:false,retry_after_ms:0,order_id:None,api_code:None};
        self.log.append(json!({"kind":"intent","qid":id,"spec":spec,"permit":permit})).unwrap();
        self.log.append(json!({"kind":"attempt","qid":id,"at":crate::daily_windows::iso(2000).unwrap(),"captureAtMs":2000})).unwrap();
        self.log.append(json!({"kind":"error","qid":id,"at":crate::daily_windows::iso(2001).unwrap(),"error":error})).unwrap();
        let dir=self.dir.join("original-sdk");fs::create_dir(&dir).unwrap();{let mut q=AccountAdmission::open(&dir,"not-sent-cache-fixture","original").unwrap();q.command(json!({"op":"status","scope":"not-sent-cache-fixture","now":1000})).unwrap();}
        let config=Config{queue_dir:dir.clone(),account_scope:"not-sent-cache-fixture".into(),scope,maximum_pages:2,writes_enabled:false};
        let proof=baal_transport_service::verify_frozen_ordinary_pre_admission(&config,&spec,&permit,2000,&error).unwrap();
        let archive=dir.join(proof.evidence()["sourceArchive"].as_str().unwrap());self.log.append_aborted_before_admission(id,&proof,&crate::daily_windows::iso(2002).unwrap()).unwrap();archive
    }
}

#[test]fn warm_rows_equal_original_parser_and_return_full_validated_held_pin(){
    let mut f=F::new();f.not_sent(1,0);let before=fs::read(&f.path).unwrap();
    let(expected,_)=original_not_sent_rows_uncached(&f.path,&f.identity,&f.log,&[1].into()).unwrap();
    let(a,p)=f.read(&[1]).unwrap();let(b,q)=f.read(&[1]).unwrap();assert_eq!(a,expected);assert_eq!(b,expected);assert_eq!(f.parses(),1);
    assert_eq!(a[&1]["event"]["kind"],"not_sent");assert_eq!(p.sha,q.sha);assert_eq!(q.identity,f.identity);p.validate().unwrap();q.validate().unwrap();assert_eq!(before,fs::read(&f.path).unwrap());
}

#[test]fn large_unrelated_rank_intent_is_verified_without_authorizing_its_retirement(){
    let mut f=F::new();f.not_sent(1,0);
    f.log.append(json!({"kind":"intent","qid":2,"reviewedRankCampaign":{"evidence":"x".repeat(2*1024*1024+100)}})).unwrap();
    f.log.append(json!({"kind":"attempt","qid":2})).unwrap();
    let before=fs::read(&f.path).unwrap();let(rows,pin)=f.read(&[1]).unwrap();
    assert_eq!(rows.len(),1);pin.validate().unwrap();assert_eq!(fs::read(&f.path).unwrap(),before);
    assert!(f.read(&[2]).is_err());
    let mut changed=before;let at=changed.windows(8).position(|w|w==b"xxxxxxxx").unwrap();changed[at]=b'y';
    fs::write(&f.path,changed).unwrap();assert!(f.read(&[1]).is_err());
}

#[test]fn dispatch_rows_above_supported_eight_mib_remain_rejected(){
    let mut f=F::new();f.not_sent(1,0);
    f.log.append(json!({"kind":"intent","qid":2,"evidence":"x".repeat(MAX_DISPATCH_ROW)})).unwrap();
    assert!(f.read(&[1]).err().unwrap().to_string().contains("DISPATCH_ROW_BOUND"));
}

#[test]fn wanted_set_and_real_append_head_invalidate_without_rejecting_new_valid_rows(){
    let mut f=F::new();f.not_sent(1,0);f.not_sent(2,0);f.read(&[1]).unwrap();f.read(&[2]).unwrap();let(all,old)=f.read(&[1,2]).unwrap();f.read(&[1]).unwrap();assert_eq!(f.parses(),3);
    f.log.append(json!({"kind":"source","qid":90,"original":"ordinary append"})).unwrap();let(next,p)=f.read(&[1,2]).unwrap();assert_eq!(all,next);assert_eq!(f.parses(),4);assert!(old.validate().is_err());p.validate().unwrap();f.read(&[1,2]).unwrap();assert_eq!(f.parses(),4);
}

#[test]fn unknown_attempt_or_ack_cannot_use_warm_rows_even_with_same_file_descriptor(){
    let mut f=F::new();f.not_sent(1,0);f.read(&[1]).unwrap();let head=f.log.descriptor_identity().unwrap();
    f.log.attempted.insert(1);assert!(f.read(&[1]).err().unwrap().to_string().contains("ABANDON_ATTEMPTED"));f.log.attempted.remove(&1);
    f.log.acknowledgments.insert(1,json!({"orderId":"known-actual"}));assert!(f.read(&[1]).err().unwrap().to_string().contains("ABANDON_ATTEMPTED"));f.log.acknowledgments.remove(&1);
    assert_eq!(head,f.log.descriptor_identity().unwrap());assert_eq!(f.parses(),1);f.log.append(json!({"kind":"attempt","qid":1})).unwrap();assert!(f.read(&[1]).is_err());assert_eq!(f.parses(),1);
}

#[test]fn same_length_edit_and_restored_mtime_do_not_authorize_cached_proof(){
    let mut f=F::new();f.not_sent(1,20);f.read(&[1]).unwrap();let modified=fs::metadata(&f.path).unwrap().modified().unwrap();let mut raw=fs::read(&f.path).unwrap();
    let needle=b"xxxxxxxx";let at=raw.windows(needle.len()).position(|w|w==needle).unwrap();raw[at]=b'y';fs::write(&f.path,raw).unwrap();File::open(&f.path).unwrap().set_times(std::fs::FileTimes::new().set_modified(modified)).unwrap();
    assert_eq!(fs::metadata(&f.path).unwrap().modified().unwrap(),modified);assert!(f.read(&[1]).is_err());assert_eq!(f.parses(),1);
}

#[test]fn replaced_path_truncation_and_symlink_cannot_be_rescued_by_warm_cache(){
    for mode in 0..3{let mut f=F::new();f.not_sent(1,0);f.read(&[1]).unwrap();match mode{
        0=>{let original=f.path.with_extension("original");fs::rename(&f.path,&original).unwrap();fs::copy(original,&f.path).unwrap();},
        1=>{OpenOptions::new().write(true).open(&f.path).unwrap().set_len(10).unwrap();},
        _=>{let original=f.path.with_extension("original");fs::rename(&f.path,&original).unwrap();std::os::unix::fs::symlink(original,&f.path).unwrap();}
    }assert!(f.read(&[1]).is_err(),"mode {mode}");}
}

#[test]fn physical_pin_still_rehashes_if_in_memory_version_is_artificially_replaced(){
    let mut f=F::new();f.not_sent(1,8);let(_,mut pin)=f.read(&[1]).unwrap();let mut raw=fs::read(&f.path).unwrap();let at=raw.windows(8).position(|w|w==b"xxxxxxxx").unwrap();raw[at]=b'y';fs::write(&f.path,raw).unwrap();
    // Ordinary changes fail the immutable cached version. Physical validation
    // additionally retains its full-byte digest even in this memory-corruption
    // fixture; normal callers cannot rewrite a private pin's version.
    assert!(f.read(&[1]).is_err());
    pin.version=file_version(&pin.file).unwrap();pin.validate_metadata().unwrap();
    assert!(pin.validate().err().unwrap().to_string().contains("PINNED_FILE_CHANGED"));assert_eq!(f.parses(),1);
}

#[test]fn restoring_mtime_cannot_hide_same_length_edit_from_cached_ctime(){
    let mut f=F::new();f.not_sent(1,8);f.read(&[1]).unwrap();
    let old=fs::metadata(&f.path).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(2));
    let mut raw=fs::read(&f.path).unwrap();let at=raw.windows(8).position(|w|w==b"xxxxxxxx").unwrap();raw[at]=b'y';fs::write(&f.path,raw).unwrap();
    File::open(&f.path).unwrap().set_times(std::fs::FileTimes::new().set_modified(old)).unwrap();
    assert_eq!(fs::metadata(&f.path).unwrap().modified().unwrap(),old);
    assert!(f.read(&[1]).is_err());assert_eq!(f.parses(),1);
}

#[test]fn poisoned_optimization_lock_falls_back_but_original_chain_failure_still_fails(){
    let mut f=F::new();f.not_sent(1,8);let(expected,_)=f.read(&[1]).unwrap();
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(||{let _held=f.cache.lock().unwrap();panic!("synthetic optimization-only lock poison")})).is_err());
    let(actual,p)=f.read(&[1]).unwrap();assert_eq!(actual,expected);p.validate().unwrap();
    let mut raw=fs::read(&f.path).unwrap();let at=raw.windows(8).position(|w|w==b"xxxxxxxx").unwrap();raw[at]=b'y';fs::write(&f.path,raw).unwrap();assert!(f.read(&[1]).is_err());
}

#[test]fn missing_duplicate_or_wrong_positive_proof_preserves_original_error_on_miss(){
    for mode in 0..3{let mut f=F::new();f.not_sent(1,0);match mode{
        0=>{assert!(f.read(&[2]).err().unwrap().to_string().contains("POSITIVE_NOT_SENT_REQUIRED"));},
        1=>{let proof=f.log.proof(1);f.log.append(json!({"kind":"not_sent","qid":1,"proof":proof})).unwrap();assert!(f.read(&[1]).err().unwrap().to_string().contains("POSITIVE_NOT_SENT"));},
        _=>{f.log.append(json!({"kind":"intent","qid":2})).unwrap();f.log.append(json!({"kind":"not_sent","qid":2,"proof":{"qid":2,"networkAttempts":0}})).unwrap();assert!(f.read(&[2]).err().unwrap().to_string().contains("POSITIVE_NOT_SENT"));}
    }}
}

#[test]fn count_and_payload_bounds_evict_or_fall_back_without_changing_original_rows(){
    let mut f=F::new();for id in 1..=33{f.not_sent(id,0);}for id in 1..=33{f.read(&[id]).unwrap();let c=f.cache.lock().unwrap();assert!(c.entries.len()<=NOT_SENT_CACHE_COUNT&&c.bytes<=NOT_SENT_CACHE_BYTES);}assert_eq!(f.cache.lock().unwrap().entries.len(),1);
    let mut big=F::new();for id in 1..=5{big.not_sent(id,1_800_000);}let wanted=[1,2,3,4,5];let(a,p)=big.read(&wanted).unwrap();let(b,q)=big.read(&wanted).unwrap();assert_eq!(a,b);assert_eq!(big.parses(),2);assert!(big.cache.lock().unwrap().entries.is_empty());assert_eq!(big.cache.lock().unwrap().bytes,0);p.validate().unwrap();q.validate().unwrap();
}

#[cfg(target_os="linux")]
#[test]fn genuine_sdk_proved_abort_and_mixed_wanted_set_always_reverify_original_archive(){
    let mut f=F::new();f.not_sent(1,0);f.read(&[1]).unwrap();let archive=f.sdk_abort(2);assert!(f.log.proved_no_network(2));assert!(!f.log.never_attempted(2));
    let(a,p)=f.read(&[1,2]).unwrap();let(b,q)=f.read(&[1,2]).unwrap();assert_eq!(a,b);assert_eq!(a[&2]["event"]["kind"],"aborted_before_admission");assert_eq!(f.parses(),1);p.validate().unwrap();q.validate().unwrap();
    fs::write(archive,b"{}").unwrap();assert!(f.read(&[2]).is_err());assert!(f.read(&[1,2]).is_err());assert_eq!(f.parses(),1);
}

/// Root supplies a separately copied immutable prefix and genuine wanted qids.
/// This opt-in benchmark never opens a Session, Transport or production log.
#[test]#[ignore]fn offline_original_not_sent_cold_warm_component_benchmark(){
    let path=PathBuf::from(std::env::var("MOLOCH_NOT_SENT_CACHE_OFFLINE_LOG").expect("explicit copied prefix required"));
    assert!(path.is_absolute()&&path.components().any(|c|c.as_os_str()=="offline-not-sent-cache"));assert!(!path.to_string_lossy().contains("/shared/live-production-v1/"));
    let raw=fs::read(&path).unwrap();assert_eq!(bytes_sha(&raw),std::env::var("MOLOCH_NOT_SENT_CACHE_EXPECTED_SHA256").unwrap());assert_eq!(raw.len() as u64,std::env::var("MOLOCH_NOT_SENT_CACHE_EXPECTED_BYTES").unwrap().parse::<u64>().unwrap());drop(raw);
    let wanted:BTreeSet<u64>=serde_json::from_str(&std::env::var("MOLOCH_NOT_SENT_CACHE_WANTED_JSON").unwrap()).unwrap();assert!(!wanted.is_empty());
    let log=DispatchLog::open(&path).unwrap();let identity=identity(&File::open(&path).unwrap()).unwrap();let cache=Mutex::new(NotSentRowsCache::default());let head=log.descriptor_identity().unwrap();
    let t=std::time::Instant::now();let(original,p)=original_not_sent_rows_uncached(&path,&identity,&log,&wanted).unwrap();let original_ns=t.elapsed().as_nanos();
    let t=std::time::Instant::now();let(cold,q)=original_not_sent_rows(&cache,&path,&identity,&log,&wanted).unwrap();let cold_ns=t.elapsed().as_nanos();
    let mut warm=Vec::new();for _ in 0..7{let t=std::time::Instant::now();let(rows,pin)=original_not_sent_rows(&cache,&path,&identity,&log,&wanted).unwrap();warm.push(t.elapsed().as_nanos());assert_eq!(rows,original);pin.validate().unwrap();}warm.sort_unstable();
    assert_eq!(cold,original);assert_eq!(cache.lock().unwrap().parses,1);assert_eq!(head,log.descriptor_identity().unwrap());p.validate().unwrap();q.validate().unwrap();
    println!("{}",json!({"originalBytes":head["bytes"],"originalSequence":head["sequence"],"wanted":wanted,"rowsSha256":digest(&original).unwrap(),"fullParsePasses":1,"originalUncachedNs":original_ns,"coldNs":cold_ns,"warmMedianNs":warm[3],"warmRounds":7,"warmFullFileHashRequired":false,"warmHeldVersionRequired":true,"physicalFullFileHashRequired":true,"wholeWorkerSpeedupMeasured":false,"financialAuthority":false}));
}
