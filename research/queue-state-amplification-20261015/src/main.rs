//! Offline fixture benchmark; no transport, credential or account code linked.
use serde::Deserialize;
use serde_json::{json, Value};
use std::{path::PathBuf, fs};

#[derive(Clone, Deserialize)]
#[serde(rename_all="camelCase", deny_unknown_fields)]
pub struct Shape {
    scope_bytes:usize, intent_bytes:usize, key_bytes:usize, request_bytes:usize,
    binding_samples:Vec<usize>, cpu_samples:usize, persist_samples:usize,
    fixture_maximum_bytes:u64, fixture_initial_bindings:usize,
    fixture_write_every_requests:u64, observed_production:Value,
}

#[allow(dead_code)]
mod current {
    // Benchmark the EXACT current Store/hash/clone/serde implementation. This
    // module's main() is never invoked, and only new private fixtures are opened.
    include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../backend-rust/account-queue/src/main.rs"));
    use super::Shape;
    use std::{hint::black_box, time::{Instant,SystemTime,UNIX_EPOCH}};
    const CLOCK:u64=1_792_054_800_000;

    fn label(prefix:char, bytes:usize, index:usize)->String {
        let suffix=format!("{index:09}");format!("{}{suffix}",prefix.to_string().repeat(bytes-suffix.len()))
    }
    fn binding(shape:&Shape,index:usize)->Binding {
        Binding { intent:label('i',shape.intent_bytes,index), key:label('k',shape.key_bytes,index),
            payload_hash:format!("{:x}",Sha256::digest(format!("synthetic-payload-{index}").as_bytes())) }
    }
    fn state(shape:&Shape,n:usize, rolling:bool)->State {
        let mut s=State::new("s".repeat(shape.scope_bytes));s.last_clock=CLOCK;s.next=CLOCK+600;
        s.read_profile=Some(ReadProfile{cap:100,gap_ms:600});
        if rolling{s.reads=(0..100).map(|i|CLOCK-59_400+i*600).collect();s.writes=(0..24).map(|i|CLOCK-57_500+i*2500).collect();}
        for i in 0..n {let b=binding(shape,i);s.intents.insert(b.intent.clone(),b.clone());s.keys.insert(b.key.clone(),b);}
        s
    }
    fn bytes(s:&State)->Vec<u8> {
        let previous="a".repeat(64);let sequence=1;let h=record_hash(sequence,&previous,s);
        let r=Record{sequence,previous,state:s.clone(),hash:h};let mut b=serde_json::to_vec(&r).unwrap();b.push(b'\n');b
    }
    fn statistics(mut xs:Vec<u128>)->Value {
        xs.sort_unstable();json!({"samples":xs.len(),"minMicros":xs[0],"p50Micros":xs[xs.len()/2],"p95Micros":xs[(xs.len()*95/100).min(xs.len()-1)],"maxMicros":xs[xs.len()-1]})
    }
    fn measure(count:usize,mut f:impl FnMut())->Value {
        f();let mut xs=Vec::new();for _ in 0..count {let at=Instant::now();f();xs.push(at.elapsed().as_micros());}statistics(xs)
    }
    fn fixture_root()->Result<PathBuf> {
        let root=std::env::temp_dir().canonicalize().map_err(|_|"BENCH_TEMP_ROOT")?;
        let path=root.join(format!("moloch-queue-benchmark-{}-{}",std::process::id(),SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir(&path).map_err(|_|"BENCH_TEMP_CREATE")?;Ok(path)
    }
    fn clean_fixture(path:&Path)->Result<()> {
        let root=std::env::temp_dir().canonicalize().map_err(|_|"BENCH_TEMP_ROOT")?;
        let actual=path.canonicalize().map_err(|_|"BENCH_TEMP_TARGET")?;
        need(actual.parent()==Some(root.as_path())&&actual.file_name().is_some_and(|s|s.to_string_lossy().starts_with("moloch-queue-benchmark-")),"BENCH_CLEANUP_BOUNDARY")?;
        fs::remove_dir_all(actual).map_err(|_|"BENCH_TEMP_CLEANUP".into())
    }
    pub fn benchmark(shape:&Shape,output:&Path)->Result<Value> {
        need((16..=128).contains(&shape.scope_bytes)&&(16..=128).contains(&shape.intent_bytes)&&(16..=128).contains(&shape.key_bytes)
            &&(16..=128).contains(&shape.request_bytes)&&!shape.binding_samples.is_empty()&&shape.binding_samples.len()<=8
            &&shape.binding_samples.iter().all(|n|*n<=10_000)&&(3..=15).contains(&shape.cpu_samples)
            &&(1..=3).contains(&shape.persist_samples)&&(1_048_576..=64*1024*1024).contains(&shape.fixture_maximum_bytes)
            &&shape.fixture_initial_bindings<=100_000&&(2..=1000).contains(&shape.fixture_write_every_requests),"BENCH_BOUNDS")?;
        let mut rows=Vec::new();
        for &n in &shape.binding_samples {
            let s=state(shape,n,true);let sparse=state(shape,n,false);let mut row=json!({"bindings":n,"stateBytes":serde_json::to_vec(&s).unwrap().len(),
                "recordBytes":bytes(&s).len(),"sparseRecordBytes":bytes(&sparse).len(),"uniquePayloadHashes":s.intents.values().map(|b|&b.payload_hash).collect::<std::collections::BTreeSet<_>>().len()});
            row["clone"]=measure(shape.cpu_samples,||{black_box(s.clone());});
            row["hash"]=measure(shape.cpu_samples,||{black_box(record_hash(1,&"a".repeat(64),&s));});
            let r=Record{sequence:1,previous:"a".repeat(64),state:s.clone(),hash:record_hash(1,&"a".repeat(64),&s)};
            row["serialize"]=measure(shape.cpu_samples,||{black_box(serde_json::to_vec(&r).unwrap());});
            row["cloneHashSerialize"]=measure(shape.cpu_samples,||{black_box(bytes(&s.clone()));});
            let dir=fixture_root()?;
            {let mut store=Store::open(&dir,&s.scope,"synthetic-benchmark")?;
                let mut xs=Vec::new();for _ in 0..shape.persist_samples {let at=Instant::now();store.persist(s.clone())?;xs.push(at.elapsed().as_micros());}
                row["persistWithFsync"]=statistics(xs);
                let at=Instant::now();let result=store.command(Command{op:"status".into(),scope:s.scope.clone(),now:CLOCK+1,..Default::default()})?;black_box(result);
                row["statusWithFsyncMicros"]=json!(at.elapsed().as_micros());row["writtenFixtureBytes"]=json!(store.active_bytes);
            }clean_fixture(&dir)?;rows.push(row);
        }
        // Valid chained v1 snapshots generated by the existing local State and
        // Store transitions, with unique deterministic payload hashes. No HTTP.
        let dir=fixture_root()?;let scope="s".repeat(shape.scope_bytes);let mut requests=0u64;
        let fixture_path=output.parent().ok_or("BENCH_OUTPUT_PARENT")?.join("queue-compression-fixture.jsonl");
        let fixture_info;
        {let mut store=Store::open_with_limit(&dir,&scope,"synthetic-compression",shape.fixture_maximum_bytes)?;
            store.persist(state(shape,shape.fixture_initial_bindings,false))?;
            let mut now=CLOCK+600;let mut next_binding=shape.fixture_initial_bindings;
            'outer: loop {
                let write=(requests+1)%shape.fixture_write_every_requests==0;
                now=now.max(store.state.time(write));
                let request=label('r',shape.request_bytes,requests as usize);
                let b=write.then(||binding(shape,next_binding));
                for op in ["admit","network","complete"] {
                    let c=Command{op:op.into(),scope:scope.clone(),now,request:request.clone(),write,binding:b.clone(),authorized:true,permit_valid:true,lease_valid:true,
                        outcome:"success".into(),status:200,..Default::default()};
                    match store.command(c){Ok(v)=>{if op=="admit"{need(v["admitted"]==true,"BENCH_SIMULATED_PACING")?;}},Err(e)if e=="EXPLICIT_COMPACTION_REQUIRED"=>break 'outer,Err(e)=>return Err(e)}
                }
                requests+=1;if write{next_binding+=1;}now=store.state.next;
            }
            let source=dir.join(&store.journal_name);let (length,digest)=source_digest(&source)?;
            let mut src=File::open(source).map_err(|_|"BENCH_FIXTURE_READ")?;
            let mut dst=OpenOptions::new().write(true).create_new(true).open(&fixture_path).map_err(|_|"BENCH_FIXTURE_CREATE")?;
            std::io::copy(&mut src,&mut dst).map_err(|_|"BENCH_FIXTURE_COPY")?;dst.sync_all().map_err(|_|"BENCH_FIXTURE_SYNC")?;
            fixture_info=json!({"path":fixture_path,"bytes":length,"sha256":digest,"records":store.sequence,"completeSyntheticRequests":requests,
                "initialBindings":shape.fixture_initial_bindings,"finalBindings":store.state.intents.len(),"finalActive":store.state.active.is_some(),
                "payloadHashesUnique":true,"actualNetworkRequests":0});
        }clean_fixture(&dir)?;
        let s0=state(shape,0,false);let s1=state(shape,1,false);let s2=state(shape,2,false);
        let per_binding=bytes(&s2).len()-bytes(&s1).len();
        Ok(json!({"schemaVersion":1,"source":"unchanged backend-rust/account-queue/src/main.rs included in private module",
            "coreSourceSha256":format!("{:x}",Sha256::digest(include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"),"/../../backend-rust/account-queue/src/main.rs")))),
            "platform":{"os":std::env::consts::OS,"architecture":std::env::consts::ARCH},"measurements":rows,
            "sizeModel":{"scopeBytes":shape.scope_bytes,"intentBytes":shape.intent_bytes,"keyBytes":shape.key_bytes,"payloadHashBytes":64,
                "sparseEmptyRecordBytes":bytes(&s0).len(),"sparseRecordBaseBytes":bytes(&s1).len()-per_binding,"perRetainedBindingBytes":per_binding,
                "meaning":"affine intercept for positive binding counts; empty maps omit two commas; fixed synthetic lengths; excludes rolling vectors, active flight, larger sequence digits and status/wait records"},
            "compressionFixture":fixture_info,"observedProduction":shape.observed_production,"accountCalls":0,"productionModified":false}))
    }

    #[cfg(test)]mod benchmark_tests {
        use super::*;
        fn shape()->Shape{serde_json::from_str(include_str!("../shape.json")).unwrap()}
        #[test]fn byte_growth_is_exactly_affine_after_first_entry_in_both_maps(){let sh=shape();let b0=bytes(&state(&sh,0,false)).len();let b1=bytes(&state(&sh,1,false)).len();let step=bytes(&state(&sh,2,false)).len()-b1;assert_eq!(b0,b1-step+2);for n in [1,2,70,98,1000]{assert_eq!(bytes(&state(&sh,n,false)).len(),b1+(n-1)*step);}}
        #[test]fn fixture_payload_hashes_are_distinct_not_artificial_zero_runs(){let s=state(&shape(),98,false);assert_eq!(s.intents.values().map(|b|b.payload_hash.clone()).collect::<std::collections::BTreeSet<_>>().len(),98);for b in s.intents.values(){assert_eq!(s.keys[&b.key],*b);}}
        #[test]fn measured_bytes_use_the_actual_chained_record_format(){let s=state(&shape(),98,true);let b=bytes(&s);let r:Record=serde_json::from_slice(&b).unwrap();assert_eq!(r.hash,record_hash(r.sequence,&r.previous,&r.state));assert_eq!(r.state,s);assert_eq!(b.last(),Some(&b'\n'));}
        #[test]fn synthetic_labels_remain_fixed_width_and_unique(){let sh=shape();assert_ne!(binding(&sh,1),binding(&sh,2));assert_eq!(binding(&sh,691200).intent.len(),sh.intent_bytes);assert_eq!(binding(&sh,691200).key.len(),sh.key_bytes);}
        #[test]fn old_core_refuses_version_two_manifest_without_falling_back_to_v1_journal(){let sh=shape();let dir=fixture_root().unwrap();{let mut s=Store::open(&dir,&"s".repeat(sh.scope_bytes),"first").unwrap();s.persist(state(&sh,70,false)).unwrap();s.command(Command{op:"compact".into(),scope:s.state.scope.clone(),now:CLOCK,..Default::default()}).unwrap();}let path=dir.join("account-queue-manifest.json");let mut v:Value=serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();v["schema_version"]=json!(2);fs::write(&path,serde_json::to_vec(&v).unwrap()).unwrap();let error=Store::open(&dir,&"s".repeat(sh.scope_bytes),"old-reader").err().unwrap();assert_eq!(error,"MANIFEST_SCOPE_OR_PATH_FAILURE");clean_fixture(&dir).unwrap();}
        #[test]fn startup_does_not_recursively_open_anchor_source_but_history_is_retained(){let sh=shape();let dir=fixture_root().unwrap();let expected=state(&sh,70,false);{let mut s=Store::open(&dir,&expected.scope,"first").unwrap();s.persist(expected.clone()).unwrap();s.command(Command{op:"compact".into(),scope:expected.scope.clone(),now:CLOCK,..Default::default()}).unwrap();}let old=dir.join("account-queue.jsonl");let retained=dir.join("retained-original-synthetic-journal.jsonl");fs::rename(&old,&retained).unwrap();{let s=Store::open(&dir,&expected.scope,"second").unwrap();assert_eq!(s.state,expected);assert!(retained.exists());}clean_fixture(&dir).unwrap();}
    }
}

fn main()->Result<(),Box<dyn std::error::Error>> {
    let args:Vec<_>=std::env::args().collect();if args.len()!=3{return Err("usage: moloch-queue-state-benchmark SHAPE.json NEW_OUTPUT.json".into())}
    let shape:Shape=serde_json::from_slice(&fs::read(&args[1])?)?;let out=PathBuf::from(&args[2]);
    if !out.is_absolute()||!out.parent().is_some_and(|p|p.is_dir())||out.exists(){return Err("output must be a NEW absolute path in an existing benchmark directory".into())}
    let result=current::benchmark(&shape,&out)?;
    use std::io::Write;let mut file=fs::OpenOptions::new().write(true).create_new(true).open(&out)?;file.write_all(&serde_json::to_vec_pretty(&result)?)?;file.sync_all()?;
    println!("{}",json!({"output":out,"accountCalls":0,"productionModified":false}));Ok(())
}
