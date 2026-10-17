//! Finite OFFLINE render-only comparison; no Session restoration or account I/O.
//! Copy this file to live-runtime/examples/render_bench.rs and run, for example:
//! cargo run --release --locked --offline --example render_bench --
//!   --report ORIGINAL_REPORT --engine-source EXACT_GIT_LF_ENGINE --loops 11
//! Optional --archive ORIGINAL_ZIP verifies its raw pin as well. Extract the report
//! separately without changing its bytes. The source must be the exact Git LF blob.
//! Input parsing, pins, seal, typed reconstruction and full-row parity precede timing.
//! OLD renders once per declared query; NEW renders once then uses the SAME scans.
//! This is not a recovered Engine, Cohorts.check, physical_pending, complete cache,
//! full-worker benchmark, live measurement, or proof of any execution authority.
//! The default 11 queries are a laboratory knob, NOT an observed live call count.

use moloch_replay::{Side, amount, money};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{alloc::{GlobalAlloc, Layout, System}, collections::{BTreeMap, BTreeSet},
    hint::black_box, path::Path, sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed},
    time::Instant};

const REPORT_SHA: &str = "3f95693ad82bba9e03863e12bffa2402d4c0b68a6e7afecb0388a76aa505f178";
const ARCHIVE_SHA: &str = "9d32ccb0986eb9a1430a396df03bd2c8df7abce6475f302a0344c16afa3aa5e2";
const ENGINE_LF_SHA: &str = "888e21a66be9a3fb9effa618bdb03f2475f8049dc4200c8a6baf5e0fe9ed0a72";
const RENDERER_SHA: &str = "943c7a5421fda00d360b2ae98bc6a311ef7a7152f305d297f0dd693a411f00b7";
const REPORT_SEAL: &str = "914186a4c2f24a3057355f7c5668506e3c21104e0a6cc307e776418b263f2ba4";
const EXPECTED_ROWS: usize = 16_601;
const WARMUPS: usize = 2;
const ROUNDS: usize = 7;

struct CountingAllocator;
static COUNT: AtomicBool = AtomicBool::new(false);
static CALLS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if COUNT.load(Relaxed) { CALLS.fetch_add(1, Relaxed); BYTES.fetch_add(l.size() as u64, Relaxed); }
        p
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(l) };
        if COUNT.load(Relaxed) { CALLS.fetch_add(1, Relaxed); BYTES.fetch_add(l.size() as u64, Relaxed); }
        p
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        let out = unsafe { System.realloc(p, l, n) };
        if COUNT.load(Relaxed) { CALLS.fetch_add(1, Relaxed); BYTES.fetch_add(n as u64, Relaxed); }
        out
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) { unsafe { System.dealloc(p, l) }; }
}
fn sha(raw: &[u8]) -> String { format!("{:x}", Sha256::digest(raw)) }
fn read_pinned(path: &Path, maximum: u64, expected: &str) -> Vec<u8> {
    let m = std::fs::symlink_metadata(path).expect("INPUT_METADATA");
    assert!(m.file_type().is_file() && !m.file_type().is_symlink() && m.len() <= maximum, "INPUT_BOUND_OR_TYPE");
    let raw = std::fs::read(path).expect("INPUT_READ");
    assert_eq!(raw.len() as u64, m.len(), "INPUT_LENGTH_CHANGED");
    assert_eq!(sha(&raw), expected, "INPUT_PIN"); raw
}
fn text<'a>(v: &'a Value, k: &str) -> &'a str { v[k].as_str().expect(k) }
fn uint(v: &Value, k: &str) -> u64 { v[k].as_u64().expect(k) }
fn integer(v: &Value, k: &str) -> i64 { v[k].as_i64().expect(k) }
fn boolean(v: &Value, k: &str) -> bool { v[k].as_bool().expect(k) }

// Verify the original OUTER raw-token seal; never reserialize financial numbers.
fn sealed_report(raw: &[u8]) -> Value {
    assert!(raw.ends_with(b"\n") && !raw.ends_with(b"\r\n"), "ORIGINAL_LF");
    let body = &raw[..raw.len()-1];
    assert_eq!(body.first(), Some(&b'{')); assert_eq!(body.last(), Some(&b'}'));
    let mut at = 1; let mut names = BTreeSet::new(); let mut hash_span = None;
    while at < body.len()-1 {
        let start = at;
        let mut key_stream = serde_json::Deserializer::from_slice(&body[at..]).into_iter::<String>();
        let key = key_stream.next().expect("OUTER_KEY").expect("OUTER_KEY_JSON");
        at += key_stream.byte_offset(); assert!(names.insert(key.clone()), "DUPLICATE_OUTER_KEY");
        assert_eq!(body[at], b':', "OUTER_COLON"); at += 1;
        let mut value_stream = serde_json::Deserializer::from_slice(&body[at..]).into_iter::<Value>();
        value_stream.next().expect("OUTER_VALUE").expect("OUTER_VALUE_JSON");
        at += value_stream.byte_offset();
        if key == "hash" { hash_span = Some((start, at)); }
        if at == body.len()-1 { break; }
        assert_eq!(body[at], b',', "OUTER_SEPARATOR"); at += 1;
    }
    assert_eq!(at, body.len()-1, "OUTER_TRAILING");
    let (start, end) = hash_span.expect("OUTER_HASH");
    let mut unsigned = Vec::with_capacity(body.len());
    if start > 1 { unsigned.extend_from_slice(&body[..start-1]); unsigned.extend_from_slice(&body[end..]); }
    else { unsigned.extend_from_slice(&body[..start]); unsigned.extend_from_slice(&body[end+1..]); }
    assert_eq!(sha(&unsigned), REPORT_SEAL, "ORIGINAL_RAW_SEAL");
    let v: Value = serde_json::from_slice(raw).expect("REPORT_JSON");
    assert_eq!(v["hash"], REPORT_SEAL); assert_eq!(v["sequence"], 523602); v
}

// Render fields have native types; no Value.clone stand-in for full rendering.
// This adapter rejects variants absent from THIS capture. It does not reconstruct
// private engine state, financial lots, permissions, source refs or authority.
struct Order {
    id: u64, market: String, exchange: String, side: Side, buy: bool,
    quantity: u64, filled: u64, limit: i128, created: i64, activation: i64,
    expires: i64, cancel: Option<i64>, retired: bool, credit: u64,
    barrier: Option<(u128, i64)>, same: u64, better: u64, visible_same: u64,
    visible_better: u64, touch_remainder: u64, aon: bool, age_reason: bool,
    through: bool, participation: u64, conditional: bool, touch_bps: u64,
}
struct RenderEngine {
    orders: Vec<Order>, reviewed_inventory_exits: BTreeMap<u64, Value>,
    reviewed_fundamental_entries: BTreeMap<u64, Value>,
}
fn label(side: Side) -> &'static str { match side { Side::Yes => "YES", Side::No => "NO" } }
impl RenderEngine {
    fn from_rows(rows: &[Value]) -> Self {
        let allowed: BTreeSet<&str> = ["id","marketId","exchangeId","side","action","quantity","filled","price","createdAt","activationAt","expiresAt","retired","participationCredit","barrier","cancelEffectiveAt","executionStyle","inventoryExitReason","reviewedInventoryExit","reviewedFundamentalEntry"].into();
        let mut engine = Self { orders: Vec::new(), reviewed_inventory_exits: BTreeMap::new(), reviewed_fundamental_entries: BTreeMap::new() };
        let mut ids = BTreeSet::new();
        for r in rows {
            assert!(r.as_object().expect("ORDER_OBJECT").keys().all(|k| allowed.contains(k.as_str())), "UNSUPPORTED_ORDER_VARIANT");
            let id = uint(r,"id"); assert!(ids.insert(id), "DUPLICATE_NATIVE_ID");
            let side = match text(r,"side") { "YES" => Side::Yes, "NO" => Side::No, _ => panic!("SIDE") };
            let buy = match text(r,"action") { "buy" => true, "sell" => false, _ => panic!("ACTION") };
            let barrier = if r["barrier"].is_null() { None } else {
                let b = r["barrier"].as_object().expect("BARRIER"); assert_eq!(b.len(),2);
                Some((text(&r["barrier"],"sequence").parse::<u128>().expect("BARRIER_SEQUENCE"),integer(&r["barrier"],"endAt")))
            };
            let cancel = r.get("cancelEffectiveAt").map(|n|n.as_i64().expect("CANCEL_CLOCK"));
            let aon = r.get("executionStyle").is_some_and(|v|{assert_eq!(v,"inventoryAON"); true});
            let age_reason = r.get("inventoryExitReason").is_some_and(|v|{assert_eq!(v,"maximumAge"); true});
            for (key,map) in [("reviewedInventoryExit", &mut engine.reviewed_inventory_exits),("reviewedFundamentalEntry", &mut engine.reviewed_fundamental_entries)] {
                if let Some(v) = r.get(key) { assert!(v.is_object(), "UNSUPPORTED_NATIVE_REVIEW_MARKER"); map.insert(id,v.clone()); }
            }
            let limit = amount(text(r,"price")).expect("NATIVE_PRICE"); assert!((0..=1_000_000).contains(&limit));
            let quantity = uint(r,"quantity"); let filled = uint(r,"filled"); assert!(filled <= quantity);
            engine.orders.push(Order { id, market:text(r,"marketId").into(), exchange:text(r,"exchangeId").into(), side,buy,quantity,filled,limit,
                created:integer(r,"createdAt"),activation:integer(r,"activationAt"),expires:integer(r,"expiresAt"),cancel,retired:boolean(r,"retired"),credit:uint(r,"participationCredit"),barrier,
                same:0,better:0,visible_same:0,visible_better:0,touch_remainder:0,aon,age_reason,through:false,participation:100,conditional:false,touch_bps:0 });
        } engine
    }
    // BEGIN VERBATIM NATIVE RENDERER (engine.rs Git LF888e21a6...,86619d82).
    pub fn order_records(&self) -> Vec<Value> {
        self.orders.iter().map(|q|{let mut v=json!({"id":q.id,"marketId":q.market,"exchangeId":q.exchange,"side":label(q.side),"action":if q.buy{"buy"}else{"sell"},"quantity":q.quantity,"filled":q.filled,"price":money(q.limit),"createdAt":q.created,"activationAt":q.activation,"expiresAt":q.expires,"retired":q.retired,"participationCredit":q.credit,"barrier":q.barrier.map(|(sequence,end)|json!({"sequence":sequence.to_string(),"endAt":end}))});
        if let Some(proof)=self.reviewed_inventory_exits.get(&q.id){v["reviewedInventoryExit"]=proof.clone();}
        if let Some(proof)=self.reviewed_fundamental_entries.get(&q.id){v["reviewedFundamentalEntry"]=proof.clone();}
        if let Some(at)=q.cancel{v["cancelEffectiveAt"]=json!(at)}if q.aon{v["executionStyle"]=json!("inventoryAON")}if q.age_reason{v["inventoryExitReason"]=json!("maximumAge")}if q.participation!=100{v["participationBps"]=json!(q.participation)}if q.through{v["throughFillCapacity"]=json!(true)}if q.conditional{v["fillModel"]=json!("conditionalFifo");v["touchAttributionBps"]=json!(q.touch_bps);v["queueAhead"]=json!((q.same+q.better).to_string());v["queueSame"]=json!(q.same.to_string());v["queueBetter"]=json!(q.better.to_string());v["queueVisibleSame"]=json!(q.visible_same.to_string());v["queueVisibleBetter"]=json!(q.visible_better.to_string());v["touchRemainder"]=json!(q.touch_remainder)}v}).collect()
    }
    // END VERBATIM NATIVE RENDERER.
}

enum Query { RetiredZero(u64), ActiveReviewed(String) }
impl Query {
    fn matches(&self, rows: &[Value]) -> bool {
        match self {
            Self::RetiredZero(id) => rows.iter().any(|q|q["id"]==*id && q["retired"]==true && q["filled"]==0),
            Self::ActiveReviewed(ex) => rows.iter().any(|q|q["exchangeId"]==*ex && q["retired"]==false && q.get("reviewedFundamentalEntry").is_some()),
        }
    }
}
fn scan_checks() {
    let rows = vec![json!({"id":7,"retired":false,"filled":1}),json!({"id":7,"retired":true,"filled":0}),json!({"id":"7","exchangeId":"x","retired":false,"reviewedFundamentalEntry":null})];
    assert!(Query::RetiredZero(7).matches(&rows), "ANY_MUST_NOT_BECOME_FIRST_MATCH");
    assert!(!Query::RetiredZero(8).matches(&rows), "MISSING_ID");
    assert!(Query::ActiveReviewed("x".into()).matches(&rows), "NULL_MARKER_STILL_PRESENT");
    assert!(!Query::ActiveReviewed("missing".into()).matches(&rows));
}
fn work(engine: &RenderEngine, queries: &[Query], reuse: bool) -> Vec<bool> {
    if reuse { let rows = black_box(engine).order_records(); queries.iter().map(|q|q.matches(black_box(&rows))).collect() }
    else { queries.iter().map(|q|{ let rows = black_box(engine).order_records(); q.matches(black_box(&rows)) }).collect() }
}
fn measured(engine: &RenderEngine, queries: &[Query], reuse: bool, expected: &[bool]) -> Value {
    CALLS.store(0,Relaxed); BYTES.store(0,Relaxed);
    let started = Instant::now(); COUNT.store(true,Relaxed);
    let answer = black_box(work(engine,queries,reuse));
    COUNT.store(false,Relaxed); let elapsed = started.elapsed().as_nanos();
    assert_eq!(answer,expected,"PREDICATE_PARITY");
    json!({"elapsedNs":elapsed.to_string(),"allocationRequests":CALLS.load(Relaxed),"allocationRequestedBytes":BYTES.load(Relaxed)})
}
fn median(rows: &[Value], key: &str) -> String {
    let mut values: Vec<u128> = rows.iter().map(|r| if key=="elapsedNs" {text(r,key).parse().unwrap()} else {uint(r,key) as u128}).collect();
    values.sort_unstable(); values[values.len()/2].to_string()
}
fn main() {
    let mut args = std::env::args().skip(1); let mut report = None; let mut source = None; let mut archive = None; let mut loops = 11usize;
    while let Some(key) = args.next() {
        let value = args.next().expect("ARG_VALUE");
        match key.as_str() { "--report"=>report=Some(value),"--engine-source"=>source=Some(value),"--archive"=>archive=Some(value),"--loops"=>loops=value.parse().expect("LOOPS"),_=>panic!("UNKNOWN_ARG") }
    }
    assert!((1..=128).contains(&loops),"LOOP_BOUND");
    let raw = read_pinned(Path::new(&report.expect("--report REQUIRED")),64*1024*1024,REPORT_SHA);
    let engine_raw = read_pinned(Path::new(&source.expect("--engine-source REQUIRED")),2*1024*1024,ENGINE_LF_SHA);
    let archive_verified = archive.map(|p|{read_pinned(Path::new(&p),16*1024*1024,ARCHIVE_SHA);true}).unwrap_or(false);
    let original = sealed_report(&raw); let rows = original["orders"].as_array().expect("ORDERS"); assert_eq!(rows.len(),EXPECTED_ROWS);
    let engine = RenderEngine::from_rows(rows); let rendered = engine.order_records(); assert_eq!(&rendered,rows,"COMPLETE_NATIVE_ROW_PARITY");
    let renderer_at = std::str::from_utf8(&engine_raw).unwrap().find("    pub fn order_records(&self) -> Vec<Value> {").expect("SOURCE_RENDERER");
    let tail = &engine_raw[renderer_at..]; let end = tail.windows(6).position(|w|w==b"    }\n").expect("SOURCE_RENDERER_END")+6;
    let renderer_sha = sha(&tail[..end]); assert_eq!(renderer_sha,RENDERER_SHA,"NATIVE_RENDERER_SOURCE");
    let rows_sha = sha(&serde_json::to_vec(rows).unwrap()); scan_checks();
    let queries: Vec<Query> = (0..loops).map(|i|{let q=&engine.orders[(i*997)%engine.orders.len()]; if i%2==0 {Query::RetiredZero(q.id)} else {Query::ActiveReviewed(q.exchange.clone())}}).collect();
    let expected: Vec<bool> = queries.iter().map(|q|q.matches(rows)).collect();
    for _ in 0..WARMUPS { assert_eq!(work(&engine,&queries,false),expected); assert_eq!(work(&engine,&queries,true),expected); }
    let mut old = Vec::new(); let mut new = Vec::new();
    for round in 0..ROUNDS {
        if round%2==0 {old.push(measured(&engine,&queries,false,&expected));new.push(measured(&engine,&queries,true,&expected));}
        else {new.push(measured(&engine,&queries,true,&expected));old.push(measured(&engine,&queries,false,&expected));}
    }
    // Both variants have the same counting allocator enabled during timing;
    // these walltimes include instrumentation, not uninstrumented live costs.
    println!("{}",serde_json::to_string_pretty(&json!({"schemaVersion":1,"kind":"actual_captured_orders_render_only_differential","reportRawSha256":REPORT_SHA,"reportSealSha256":REPORT_SEAL,"reportAt":original["report"]["at"],"nativeSequence":523602,"archiveSha256":ARCHIVE_SHA,"archiveRawBytesVerifiedByThisRun":archive_verified,"engineGitLfSha256":ENGINE_LF_SHA,"nativeRendererGitLfSha256":renderer_sha,"canonicalRowsSha256":rows_sha,"actualRows":rows.len(),"allRowsExactlyEqual":true,"declaredQueries":loops,"queriesAreObservedRuntimeCallCount":false,"warmupsPerVariant":WARMUPS,"roundsPerVariant":ROUNDS,"alternatingFirstVariant":true,"oldRendersPerRound":loops,"newRendersPerRound":1,"medianOld":{"elapsedNs":median(&old,"elapsedNs"),"allocationRequests":median(&old,"allocationRequests"),"allocationRequestedBytes":median(&old,"allocationRequestedBytes")},"medianNew":{"elapsedNs":median(&new,"elapsedNs"),"allocationRequests":median(&new,"allocationRequests"),"allocationRequestedBytes":median(&new,"allocationRequestedBytes")},"oldRounds":old,"newRounds":new,"queryResultsSha256":sha(&serde_json::to_vec(&expected).unwrap()),"allocationMeaning":"alloc/alloc_zeroed/realloc requested counts and bytes, not peak live bytes or system calls","omittedTimedWork":["Session restore/admission/poison/authority","physical_pending and all financial/source/dispatch proofs","order_snapshot key checks, bounded cache/index build and cache misses after mutation","stream, HTTP, disk persistence, policy inference"],"unsupportedCapturedVariantsRefused":["conditional queue fields","through-fill capacity","nondefault participation","unknown order fields","malformed/null native review metadata"],"financialReplayPerformed":false,"executionAuthority":false,"liveSpeedupClaimed":false})).unwrap());
}
