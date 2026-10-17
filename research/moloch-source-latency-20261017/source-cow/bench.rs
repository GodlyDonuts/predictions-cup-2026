//! Offline old/new RestSource comparison. See README.md for fixed-source setup.
pub use moloch_replay::{Result, amount, money, price};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::time::Instant;

#[path = "old_source.rs"]
mod old_source;
#[path = "new_source.rs"]
mod new_source;

struct CountingAllocator;
static COUNT: AtomicBool = AtomicBool::new(false);
static CALLS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if COUNT.load(Relaxed) { CALLS.fetch_add(1, Relaxed); BYTES.fetch_add(layout.size() as u64, Relaxed); }
        p
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(layout) };
        if COUNT.load(Relaxed) { CALLS.fetch_add(1, Relaxed); BYTES.fetch_add(layout.size() as u64, Relaxed); }
        p
    }
    unsafe fn realloc(&self, p: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let out = unsafe { System.realloc(p, layout, size) };
        if COUNT.load(Relaxed) { CALLS.fetch_add(1, Relaxed); BYTES.fetch_add(size as u64, Relaxed); }
        out
    }
    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) { unsafe { System.dealloc(p, layout) }; }
}

const ROUNDS: usize = 20;
const WARMUPS: usize = 2;
const MAX_INPUT_BYTES: u64 = 256 * 1024 * 1024;
fn sha(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }
fn text<'a>(v: &'a Value, key: &str) -> &'a str { v[key].as_str().expect(key) }
fn array<'a>(v: &'a Value, key: &str, max: usize) -> &'a Vec<Value> {
    v[key].as_array().filter(|a| a.len() <= max).expect(key)
}
fn key(v: &Value) -> (String, String) {
    (text(v, "exchangeId").into(), text(v, "id").into())
}
fn fact_projection_bytes(seeds: &[Value]) -> usize {
    let mut facts = BTreeMap::new();
    for e in seeds {
        let executed = chrono::DateTime::parse_from_rfc3339(text(e, "executedAt")).unwrap().timestamp_nanos_opt().unwrap();
        facts.entry(format!("{}:{}", text(e, "exchangeId"), text(e, "id"))).or_insert_with(||
            json!({"market":e["marketId"],"price":e["yesPrice"],"quantity":e["quantity"],"executed":executed,"stamp":e["executedAt"],"namespace":e["idNamespace"]}));
    }
    serde_json::to_vec(&facts).unwrap().len()
}
fn check_hash(bytes: &[u8], expected: &str) {
    assert!(expected.len() == 64 && expected.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)), "SHA_FORMAT");
    assert_eq!(sha(bytes), expected, "SOURCE_SHA_MISMATCH");
}

trait Adapter: Clone {
    fn make(input: &Value) -> Self;
    fn step(&mut self, step: &Value) -> Result<Vec<Value>>;
}
macro_rules! adapter {
    ($module:ident) => {
        impl Adapter for $module::RestSource {
            fn make(input: &Value) -> Self {
                let mut source = Self::new(&input["scopes"], text(input, "tournamentId"), text(input, "startedAt")).expect("SOURCE_INIT");
                source.seed_trade_facts(array(input, "seedEvents", 500_000).clone()).expect("SOURCE_SEED");
                source
            }
            fn step(&mut self, step: &Value) -> Result<Vec<Value>> {
                match text(step, "kind") {
                    "broadcast" => self.broadcast(text(step, "event"), &step["payload"], text(step, "receivedAt")),
                    "receipt" => self.accept(&step["receipt"]),
                    _ => panic!("STEP_KIND"),
                }
            }
        }
    };
}
adapter!(old_source);
adapter!(new_source);

#[derive(Debug, PartialEq)]
enum Outcome { Events(Vec<Value>), Error(&'static str) }
fn run<A: Adapter>(source: &mut A, steps: &[Value]) -> Vec<Outcome> {
    steps.iter().map(|step| match source.step(step) {
        Ok(events) => Outcome::Events(events), Err(error) => Outcome::Error(error),
    }).collect()
}
fn expected(steps: &[Value], outcomes: &[Outcome]) {
    assert_eq!(steps.len(), outcomes.len());
    for (step, outcome) in steps.iter().zip(outcomes) {
        match outcome {
            Outcome::Events(_) => assert_eq!(text(step, "expect"), "ok", "EXPECTED_ERROR_NOT_OBSERVED"),
            Outcome::Error(error) => assert_eq!(text(step, "expect"), *error, "UNEXPECTED_SOURCE_ERROR"),
        }
    }
}
fn digest(outcomes: &[Outcome]) -> String {
    let rows: Vec<Value> = outcomes.iter().map(|o| match o {
        Outcome::Events(events) => json!({"events":events}), Outcome::Error(error) => json!({"error":error}),
    }).collect();
    sha(&serde_json::to_vec(&rows).unwrap())
}
fn events(outcomes: &[Outcome], kind: &str) -> Vec<Value> {
    outcomes.iter().flat_map(|o| match o { Outcome::Events(v) => v.as_slice(), _ => &[] })
        .filter(|e| e["type"] == kind).cloned().collect()
}
fn semantic_case(case: &Value, outcomes: &[Outcome], follow: &[Outcome], seeds: &BTreeSet<(String, String)>) {
    let trades = events(outcomes, "trade");
    match text(case, "name") {
        "book_only" => {
            assert!(outcomes.iter().all(|o| matches!(o, Outcome::Events(_))));
            assert!(events(outcomes, "book").iter().any(|e| e["book"]["invalid"] != true), "BOOK_CASE_HAS_NO_VALID_ADMITTED_BOOK");
            assert!(trades.is_empty(), "BOOK_CASE_CONTAINS_TRADES");
        }
        "duplicate_trade" => {
            assert!(outcomes.iter().all(|o| matches!(o, Outcome::Events(_))));
            assert!(!trades.is_empty() && trades.iter().all(|t| seeds.contains(&key(t))), "DUPLICATE_CASE_NOT_SEEDED");
        }
        "new_trade" => {
            assert!(outcomes.iter().all(|o| matches!(o, Outcome::Events(_))));
            assert!(trades.iter().any(|t| !seeds.contains(&key(t))), "NEW_CASE_HAS_NO_NEW_TRADE");
            assert!(!follow.is_empty() && follow.iter().any(|o| matches!(o, Outcome::Error("SOURCE_TRADE_FACT_CONFLICT"))), "NEW_CASE_CONFLICT_FOLLOWUP_REQUIRED");
        }
        "insert_then_conflict" => {
            assert_eq!(outcomes.len(), 1, "ROLLBACK_CASE_ONE_TRANSACTION");
            assert!(matches!(outcomes[0], Outcome::Error("SOURCE_TRADE_FACT_CONFLICT" | "SOURCE_TRADE_CLOCK_CONFLICT")), "ROLLBACK_CASE_NOT_LATE_FACT_ERROR");
            assert!(!follow.is_empty() && follow.iter().all(|o| matches!(o, Outcome::Events(_))), "ROLLBACK_FOLLOWUP_REQUIRED");
            let probe = &case["rollbackProbe"];
            assert!(!seeds.contains(&key(probe)), "ROLLBACK_PROBE_ALREADY_SEEDED");
            let step = &case["steps"][0];
            let wire: Vec<Value> = if step["kind"] == "receipt" {
                old_source::receipt_events(&step["receipt"]).expect("ROLLBACK_RECEIPT_PARSE").into_iter().filter(|t| t["type"] == "trade").collect()
            } else {
                assert_eq!(step["kind"], "broadcast");
                let mut wire = Vec::new();
                for market in step["payload"]["markets"].as_array().expect("ROLLBACK_MARKETS") {
                    for t in market["batch"]["trades"].as_array().expect("ROLLBACK_TRADES") {
                        let p = &t["price"];
                        let micros = if let Some(s) = p.as_str() { amount(s).unwrap() } else {
                            let n = p.as_f64().expect("ROLLBACK_PRICE");
                            assert!(n.is_finite() && (n * 200.0 - (n * 200.0).round()).abs() < 1e-7);
                            price((n * 200.0).round() as i128 * 5000).unwrap()
                        };
                        wire.push(json!({"exchangeId":t["exchangeId"],"id":t["id"],"yesPrice":money(micros)}));
                    }
                }
                wire
            };
            let index = wire.iter().position(|t| key(t) == key(probe)).expect("ROLLBACK_INSERTION_MISSING");
            assert!(wire[index + 1..].iter().any(|t| seeds.contains(&key(t))), "ROLLBACK_INSERTION_MUST_PRECEDE_SEEDED_CONFLICT");
            let failed_price = amount(text(&wire[index], "yesPrice")).unwrap();
            let successful = events(follow, "trade").into_iter().find(|t| key(t) == key(probe)).expect("ROLLBACK_PROBE_NOT_REACCEPTED");
            assert_ne!(amount(text(&successful, "yesPrice")).unwrap(), failed_price, "ROLLBACK_FOLLOWUP_MUST_CHANGE_REJECTED_INSERTION_PRICE");
        }
        _ => panic!("CASE_NAME"),
    }
}

#[derive(Clone, Copy)]
struct Sample { nanos: u64, allocations: u64, requested_bytes: u64 }
fn measured<A: Adapter>(base: &A, steps: &[Value], followup: &[Value], want: &[Outcome], want_follow: &[Outcome]) -> Sample {
    // Setup clone, checks, hashing, follow-up, and owner/result destruction are NOT timed/counted.
    let mut source = base.clone();
    CALLS.store(0, Relaxed); BYTES.store(0, Relaxed); COUNT.store(true, Relaxed);
    let start = Instant::now();
    let result = run(&mut source, steps);
    std::hint::black_box(&result);
    let nanos = u64::try_from(start.elapsed().as_nanos()).expect("TIME_RANGE");
    COUNT.store(false, Relaxed);
    let sample = Sample { nanos, allocations: CALLS.load(Relaxed), requested_bytes: BYTES.load(Relaxed) };
    assert_eq!(result, want, "TIMED_RESULT_CHANGED");
    assert_eq!(run(&mut source, followup), want_follow, "TIMED_FOLLOWUP_CHANGED");
    sample
}
fn summary(samples: &[Sample]) -> Value {
    let stats = |f: fn(&Sample) -> u64| {
        let mut v: Vec<u64> = samples.iter().map(f).collect(); v.sort_unstable();
        let median = ((v[(v.len()-1)/2] as u128 + v[v.len()/2] as u128)/2) as u64;
        json!({"minimum":v[0],"median":median,"maximum":v[v.len()-1]})
    };
    json!({"wallNanoseconds":stats(|s|s.nanos),"allocationCalls":stats(|s|s.allocations),"allocationRequestedBytes":stats(|s|s.requested_bytes)})
}
fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert_eq!(args.len(), 5, "usage: source-cow-bench INPUT INPUT_SHA OLD_SOURCE_SHA NEW_SOURCE_SHA");
    let path = std::path::Path::new(&args[1]);
    let meta = std::fs::symlink_metadata(path).expect("INPUT_METADATA");
    assert!(meta.is_file() && !meta.file_type().is_symlink() && meta.len() <= MAX_INPUT_BYTES, "INPUT_BOUND");
    let raw = std::fs::read(path).expect("INPUT_READ"); assert!(raw.len() as u64 <= MAX_INPUT_BYTES);
    check_hash(&raw, &args[2]);
    check_hash(include_bytes!("old_source.rs"), &args[3]);
    check_hash(include_bytes!("new_source.rs"), &args[4]);
    let input: Value = serde_json::from_slice(&raw).expect("INPUT_JSON");
    assert_eq!(input["schemaVersion"], 1);
    let seeds = array(&input, "seedEvents", 500_000);
    assert!(seeds.len() >= 10_000, "ACTUAL_HISTORY_REQUIRED");
    let mut seed_keys = BTreeSet::new();
    for e in seeds { assert_eq!(e["type"], "trade"); seed_keys.insert(key(e)); }
    assert!(input["provenance"].is_object(), "PROVENANCE_REQUIRED");
    let synthetic = &input["provenance"]["syntheticCases"];
    let declared = |name: &str| synthetic.as_array().is_some_and(|a| a.iter().any(|v| v == name))
        || synthetic.as_object().is_some_and(|o| o.get(name).is_some_and(|v| v.as_str().is_some_and(|s| !s.is_empty())));
    assert!(declared("new_trade") && declared("insert_then_conflict"), "SYNTHETIC_CASES");
    let scopes = array(&input, "scopes", 1000); assert!(!scopes.is_empty());
    let prime = array(&input, "prime", 4096);
    let cases = array(&input, "cases", 4); assert_eq!(cases.len(), 4);
    let names: BTreeSet<&str> = cases.iter().map(|c| text(c, "name")).collect();
    assert_eq!(names, BTreeSet::from(["book_only", "duplicate_trade", "new_trade", "insert_then_conflict"]));
    let mut old = old_source::RestSource::make(&input);
    let mut new = new_source::RestSource::make(&input);
    let old_prime = run(&mut old, prime); let new_prime = run(&mut new, prime);
    expected(prime, &old_prime); assert_eq!(old_prime, new_prime, "PRIME_PARITY");
    let mut known_keys = seed_keys.clone();
    for trade in events(&old_prime, "trade") { known_keys.insert(key(&trade)); }
    let mut rows = Vec::new();
    for case in cases {
        let steps = array(case, "steps", 64); assert!(!steps.is_empty());
        let follow = array(case, "followup", 64);
        let mut a = old.clone(); let mut b = new.clone();
        let want = run(&mut a, steps); let got = run(&mut b, steps);
        expected(steps, &want); assert_eq!(want, got, "CASE_PARITY");
        let want_follow = run(&mut a, follow); let got_follow = run(&mut b, follow);
        expected(follow, &want_follow); assert_eq!(want_follow, got_follow, "FOLLOWUP_PARITY");
        semantic_case(case, &want, &want_follow, &known_keys);
        if case["name"] == "insert_then_conflict" {
            assert_eq!(run(&mut old.clone(), follow), want_follow, "OLD_REJECTED_TRANSACTION_CHANGED_FOLLOWUP");
            assert_eq!(run(&mut new.clone(), follow), got_follow, "NEW_REJECTED_TRANSACTION_CHANGED_FOLLOWUP");
        }
        if case["name"] == "new_trade" {
            let baseline = run(&mut old.clone(), follow);
            assert!(!baseline.is_empty() && baseline.iter().all(|o| matches!(o, Outcome::Events(_))), "UNTOUCHED_BASE_MUST_ACCEPT_NEW_TRADE_FOLLOWUP");
            assert_eq!(run(&mut new.clone(), follow), baseline, "SUCCESSFUL_MUTATION_LEAKED_TO_BASE");
        }
        for _ in 0..WARMUPS { measured(&old, steps, follow, &want, &want_follow); measured(&new, steps, follow, &want, &want_follow); }
        let mut before = Vec::new(); let mut after = Vec::new();
        for round in 0..ROUNDS {
            if round % 2 == 0 {
                before.push(measured(&old, steps, follow, &want, &want_follow));
                after.push(measured(&new, steps, follow, &want, &want_follow));
            } else {
                after.push(measured(&new, steps, follow, &want, &want_follow));
                before.push(measured(&old, steps, follow, &want, &want_follow));
            }
        }
        rows.push(json!({"name":case["name"],"steps":steps.len(),"followupSteps":follow.len(),"emittedBooks":events(&want,"book").len(),"emittedTrades":events(&want,"trade").len(),"resultSha256":digest(&want),"followupSha256":digest(&want_follow),"old":summary(&before),"new":summary(&after),"exactParity":true}));
    }
    println!("{}", serde_json::to_string_pretty(&json!({"schemaVersion":1,"inputSha256":args[2],"oldSourceSha256":args[3],"newSourceSha256":args[4],"harnessSha256":sha(include_bytes!("bench.rs")),"seedEvents":seeds.len(),"uniqueSeedKeys":seed_keys.len(),"knownKeysAfterPrime":known_keys.len(),"factJsonProjectionBytes":fact_projection_bytes(seeds),"factProjectionIsRustHeapMeasurement":false,"scopes":scopes.len(),"primeSteps":prime.len(),"primeResultSha256":digest(&old_prime),"provenanceSha256":sha(&serde_json::to_vec(&input["provenance"]).unwrap()),"rounds":ROUNDS,"warmups":WARMUPS,"cases":rows,"allocationMetric":"alloc/alloc_zeroed requested size plus each realloc NEW requested size; includes failed requests, not peak/live/RSS","timedRegion":"steps only; excludes base clone/setup, seed/prime, checks/hashes, followup and owner/result drop","executionAuthority":false,"wholeWorkerSpeedupMeasured":false})).unwrap());
}

#[cfg(test)]
mod tests {
    use super::*;
    fn trade(id: &str, p: &str) -> Value {
        json!({"type":"trade","marketId":"1","exchangeId":"2","id":id,"yesPrice":p,"quantity":1,"executedAt":"2026-10-17T00:00:00Z","idNamespace":"sig_engine_match"})
    }
    fn input() -> Value { json!({"tournamentId":"fixture","startedAt":"2026-10-17T00:00:00Z","scopes":[{"marketId":"1","exchangeId":"2"}],"seedEvents":[trade("original","0.400000")]}) }
    #[test]
    fn seed_late_conflict_rolls_back_inserted_fact_and_cloned_owner() {
        macro_rules! check {
            ($m:ident) => {{
                let mut original = $m::RestSource::make(&input());
                let mut candidate = original.clone();
                assert_eq!(candidate.seed_trade_facts(vec![trade("new", "0.405000"), trade("original", "0.410000")]), Err("SOURCE_TRADE_FACT_CONFLICT"));
                candidate.seed_trade_facts(vec![trade("new", "0.415000")]).unwrap();
                original.seed_trade_facts(vec![trade("new", "0.420000")]).unwrap();
                assert_eq!(candidate.seed_trade_facts(vec![trade("new", "0.420000")]), Err("SOURCE_TRADE_FACT_CONFLICT"));
                assert_eq!(original.seed_trade_facts(vec![trade("new", "0.415000")]), Err("SOURCE_TRADE_FACT_CONFLICT"));
            }};
        }
        check!(old_source); check!(new_source);
    }
    #[test]
    fn rejected_broadcast_retains_original_seed_conflict_evidence() {
        let mut old = old_source::RestSource::make(&input()); let mut new = new_source::RestSource::make(&input());
        let bad = json!({"kind":"broadcast","event":"unsupported","payload":{},"receivedAt":"2026-10-17T00:00:00Z","expect":"SOURCE_STREAM_EVENT"});
        let a = run(&mut old, &[bad.clone()]); let b = run(&mut new, &[bad.clone()]);
        expected(&[bad], &a); assert_eq!(a, b);
        assert_eq!(old.seed_trade_facts(vec![trade("original", "0.405000")]), Err("SOURCE_TRADE_FACT_CONFLICT"));
        assert_eq!(new.seed_trade_facts(vec![trade("original", "0.405000")]), Err("SOURCE_TRADE_FACT_CONFLICT"));
    }
}
