//! Offline comparison of original linear historical-ID lookups and the exact
//! existing Snapshot index. Excludes Session recovery, rendering and authority.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::time::Instant;
#[path = "order_snapshot_instrumented.rs"]
mod snapshot;
fn sha(b: &[u8]) -> String { format!("{:x}", Sha256::digest(b)) }
fn median(mut v: Vec<u128>) -> u128 { v.sort_unstable(); v[v.len()/2] }
fn linear<'a>(rows: &'a [Value], ids: &[u64]) -> Vec<Option<&'a Value>> {
    ids.iter().map(|id| rows.iter().find(|q| q["id"] == *id)).collect()
}
fn indexed<'a>(s: &'a snapshot::Snapshot, ids: &[u64]) -> Vec<Option<&'a Value>> {
    ids.iter().map(|id| s.quote(*id)).collect()
}
fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert_eq!(args.len(), 3, "INPUT INPUT_SHA256");
    let raw = std::fs::read(&args[1]).unwrap();
    assert!(raw.len() <= 32*1024*1024);
    assert_eq!(sha(&raw), args[2]);
    let input: Value = serde_json::from_slice(&raw).unwrap();
    let rows = input["orders"].as_array().unwrap();
    let ids: Vec<u64> = input["originalAcknowledgedQids"].as_array().unwrap()
        .iter().map(|v| v.as_u64().unwrap()).collect();
    assert!(rows.len() >= 10_000 && ids.len() >= 1_000);
    let cached = snapshot::benchmark_only_from_original_rows(rows.clone());
    let expected = linear(rows, &ids);
    assert!(expected.iter().all(Option::is_some));
    assert_eq!(expected, indexed(&cached, &ids));
    // First-match and missing-ID behavior of the actual production index.
    let mut diagnostic = rows.clone();
    let mut duplicate = diagnostic[0].clone();
    duplicate["retired"] = json!("synthetic later duplicate must not win");
    diagnostic.push(duplicate);
    let diagnostic_index = snapshot::benchmark_only_from_original_rows(diagnostic.clone());
    let mut diagnostic_ids = ids.clone(); diagnostic_ids.push(u64::MAX);
    assert_eq!(linear(&diagnostic, &diagnostic_ids), indexed(&diagnostic_index, &diagnostic_ids));
    let mut old = Vec::new(); let mut new = Vec::new(); let mut cold = Vec::new();
    for round in 0..9 {
        let mut a = || {
            let start = Instant::now(); let result = linear(rows, &ids);
            std::hint::black_box(&result); let ns = start.elapsed().as_nanos();
            assert_eq!(result, expected); ns
        };
        let mut b = || {
            let start = Instant::now(); let result = indexed(&cached, &ids);
            std::hint::black_box(&result); let ns = start.elapsed().as_nanos();
            assert_eq!(result, expected); ns
        };
        let (o, n) = if round % 2 == 0 { (a(), b()) } else { let n = b(); (a(), n) };
        // Native order rendering and this setup clone are outside both timers.
        let owned = rows.clone(); let start = Instant::now();
        let built = snapshot::benchmark_only_from_original_rows(owned);
        let result = indexed(&built, &ids); std::hint::black_box(&result);
        let c = start.elapsed().as_nanos(); assert_eq!(result, expected);
        if round > 1 { old.push(o); new.push(n); cold.push(c); }
    }
    println!("{}", serde_json::to_string_pretty(&json!({
        "inputSha256": args[2], "orders": rows.len(), "originalAcknowledgedQids": ids.len(),
        "oldLinearLookupMedianNs": median(old), "existingHotSnapshotLookupMedianNs": median(new),
        "existingColdSnapshotIndexAndLookupMedianNs": median(cold), "rounds": 7, "warmups": 2,
        "exactOrderedOriginalRowParity": true, "missingAndLaterDuplicateFirstMatchParity": true,
        "resultSha256": sha(&serde_json::to_vec(&expected).unwrap()),
        "orderSnapshotOriginalSourceSha256": input["orderSnapshotOriginalSourceSha256"],
        "harnessSha256": sha(include_bytes!("bench.rs")),
        "instrumentation": "Original order_snapshot.rs unchanged prefix plus benchmark-only constructor; checks actual retention bound and invokes original first_indices/quote",
        "limitations": ["Original ACK ID lookup workload, not independent native binding-map authentication.",
          "Excludes order rendering, financial Session recovery, file pins, full liability policy, network and complete worker timing.",
          "Cold measurement includes exact conservative cache-bound check and first-index construction; setup Vec clone excluded.",
          "Reduced-priority live-host CPU sharing and cache noise remain."],
        "executionAuthority": false, "wholeWorkerSpeedupMeasured": false, "profitabilityEstablished": false
    })).unwrap());
}
