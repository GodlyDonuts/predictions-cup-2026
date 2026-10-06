// data/benchmark-native-collector-20261004.mjs — JS v2 publication vs native
// backend on the same full sealed public prefix.
use baal_collector::book_publication_v2::{apply_book_publication, create_book_publication_state};
use baal_service_runtime::journal::{journal_lines, JournalReadOptions};
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};

fn ph(v: &V) -> Result<String, String> { baal_execution::core::payload_hash(v) }
fn read_json(p: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn localappdata() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).ok_or_else(|| "LOCALAPPDATA".to_string())
}
fn now_iso() -> String {
    baal_service_runtime::contract::ms_to_iso(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as f64)
}
fn perf_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64() * 1000.0
}

pub fn run() -> Result<V, String> {
    let r = localappdata()?.join("SigTradingCup").join("paper-runtime");
    let p = read_json(Path::new("data/native-collector-preservation-20261004.json"))?;
    let a = read_json(&r.join("probability-receipt-active.json"))?;
    let b = read_json(&r.join("probability-receipt-cohorts")
        .join(a["cohort"].as_str().unwrap_or("")).join("bank.json"))?;
    let file = r.join("public-books.jsonl");
    let end_byte = p["source"]["bytes"].as_u64().unwrap_or(0);
    let started = perf_ms();
    let mut state = create_book_publication_state(&b["universe"])?;
    for line in journal_lines(&file, &JournalReadOptions { end_byte: Some(end_byte), ..Default::default() })? {
        let row: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        state = apply_book_publication(&state, &row)?;
    }
    let js_ms = perf_ms() - started;
    let native_started = perf_ms();
    let native = baal_collector::native_publication::native_publication_state(&r,
        &p["config"]["nativeBackend"], &b["universe"], &file, end_byte,
        p["source"]["hash"].as_str().unwrap_or(""),
        &r.join("collector-native-benchmark-receipts"))?;
    let native_ms = perf_ms() - native_started;
    if ph(&state).ok() != ph(&native["state"]).ok() {
        return Err("Collector native complete state mismatch".into());
    }
    let hashes: serde_json::Map<String, V> =
        ["live-paper/book-publication-v2.mjs", "live-paper/book-publication-v3.mjs",
            "live-paper/native-publication.mjs"].iter().map(|f| -> Result<(String, V), String> {
            Ok((f.to_string(), json!(sha256(&std::fs::read(f).map_err(|e| e.to_string())?))))
        }).collect::<Result<_, String>>()?;
    let report = json!({"at": now_iso(), "source": p["source"], "sourceVerified": true,
        "exactCompleteStateParity": true, "stateHash": ph(&state)?,
        "javascriptVariant": "book-publication-v2 optimized incremental scope",
        "javascriptMs": js_ms, "nativeAdapterMs": native_ms, "speedup": js_ms / native_ms,
        "sourceFileHashes": V::Object(hashes), "nativeBackend": p["config"]["nativeBackend"],
        "scope": "Same full sealed public prefix, single measured run; validation only, not inference/trading latency/total startup. Separate earlier 32.66x measurement used original book-publication.mjs.",
        "accountRequests": 0, "competitionOrders": 0, "profitabilityEstablished": false});
    baal_service_runtime::atomic::atomic_json_canonical(
        Path::new("data/native-collector-benchmark-20261004.json"), &report)?;
    Ok(report)
}
