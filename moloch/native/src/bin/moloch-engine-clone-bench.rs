//! Offline baseline/candidate harness. Synthetic data only; no account/session IO.
#[path="../../tests/support/engine_clone_fixture.rs"]mod fixture;
use fixture::*;
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::{hint::black_box,time::Instant};
fn nanos(t:std::time::Duration)->u64{t.as_nanos().min(u64::MAX as u128) as u64}
fn run()->Result<Value>{
 let args:Vec<_>=std::env::args().collect();let n=match args.as_slice(){[_]=>100,[_,flag,value]if flag=="--iterations"=>value.parse::<usize>()?,_=>return Err("usage: moloch-engine-clone-bench [--iterations 4..2000]".into())};
 if !(4..=2000).contains(&n){return Err("BENCH_ITERATION_BOUND".into())}
 let start=Instant::now();let engine=warm()?;let warm_ns=nanos(start.elapsed());let warm_hash=fingerprint(&engine)?;
 let start=Instant::now();for _ in 0..n{drop(black_box(engine.clone()));}let clone_ns=nanos(start.elapsed());
 let mut measurements=vec![];
 for kind in ["tick","book","gap_and_resync"]{
  let events=workload(kind,n);let fixture_hash=format!("{:x}",Sha256::digest(serde_json::to_vec(&events)?));
  let mut e=engine.clone();let mut checkpoints=vec![json!({"events":0,"financialStateSha256":fingerprint(&e)?})];
  let mut elapsed=0u64;
  for (i,event) in events.iter().enumerate(){
   let at=Instant::now();e.advance(black_box(event))?;elapsed=elapsed.saturating_add(nanos(at.elapsed()));
   let k=i+1;if [n/4,n/2,3*n/4,n].contains(&k){checkpoints.push(json!({"events":k,"financialStateSha256":fingerprint(&e)?}));}
  }
  let rollback=rollback_probe(&mut e,WARM_END_MS+n as u64+1)?;
  measurements.push(json!({"kind":kind,"events":n,"fixtureSha256":fixture_hash,"advanceElapsedNs":elapsed,"meanAdvanceNs":elapsed/n as u64,"prefixes":checkpoints,"rollback":rollback,"finalOrders":e.order_records().len(),"finalFills":e.fills().len()}));
 }
 Ok(json!({"schemaVersion":1,"synthetic":true,"accountRequests":0,"mode":"in_memory_clone_and_advance","engineIdentity":moloch_replay::session::engine_identity(),"markets":MARKETS,"levelsPerSide":DEPTH,"iterations":n,"warmElapsedNs":warm_ns,"warmFinancialStateSha256":warm_hash,"warmOrders":engine.order_records().len(),"warmFills":engine.fills().len(),"cloneElapsedNs":clone_ns,"meanCloneNs":clone_ns/n as u64,"cloneUnchanged":fingerprint(&engine)?==warm_hash,"workloads":measurements,"timingExcludesFixtureConstructionAndPrefixHashing":true,"sessionFsyncOrStartupReplayMeasured":false,"profitabilityEstablished":false}))
}
fn main(){match run(){Ok(v)=>println!("{v}"),Err(e)=>{eprintln!("{e}");std::process::exit(1)}}}
