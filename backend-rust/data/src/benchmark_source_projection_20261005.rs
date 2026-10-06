// data/benchmark-source-projection-20261005.mjs — JS chain verify vs native
// source projection on the coupled frontier files.
use baal_service_runtime::journal::{journal_lines, JournalReadOptions};
use baal_service_runtime::json_text::{js_stringify, sha256};
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

fn sha(b: &[u8]) -> String { sha256(b) }
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
    let runtime_dir = localappdata()?.join("SigTradingCup").join("paper-runtime");
    let binding = read_json(Path::new("data/coupled-rust-benchmark-20261005.json"))?["binding"].clone();
    let mut results: Vec<V> = Vec::new();
    for name in ["anonymous-trades.jsonl", "market-prices.jsonl"] {
        let file = runtime_dir.join(name);
        let end_byte = std::fs::metadata(&file).map_err(|e| e.to_string())?.len();
        let start = perf_ms();
        let mut prefix = Sha256::new();
        let mut sequence = 0i64;
        let mut previous = String::new();
        for line in journal_lines(&file, &JournalReadOptions { end_byte: Some(end_byte), ..Default::default() })? {
            prefix.update(line.as_bytes());
            prefix.update(b"\n");
            if line.is_empty() { continue; }
            let row: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
            let mut body = row.clone();
            let hash = row["hash"].as_str().unwrap_or("").to_string();
            if let V::Object(m) = &mut body { m.shift_remove("hash"); }
            sequence += 1;
            if baal_service_runtime::json_text::js_int(&row["sequence"]) != Some(sequence)
                || row["previous"].as_str().unwrap_or("") != previous
                || sha(js_stringify(&body).as_bytes()) != hash {
                return Err("JS_CHAIN".into());
            }
            previous = hash;
        }
        let source_hash = format!("{:x}", prefix.finalize());
        let js_ms = perf_ms() - start;
        let native_start = perf_ms();
        let projected = baal_collector::native_source_projection::project_native_source(
            &runtime_dir, &binding, &file,
            &json!({"sequence": sequence, "hash": previous}),
            &runtime_dir.join("rust-source-benchmark-20261005").join(name),
            None, &|| true, Some(end_byte))?;
        let rust_ms = perf_ms() - native_start;
        if projected["sourceHash"].as_str() != Some(source_hash.as_str())
            || projected["projection"]["records"].as_i64() != Some(0)
            || projected["records"].as_i64() != Some(sequence)
            || projected["tailHash"].as_str() != Some(previous.as_str()) {
            return Err("PARITY".into());
        }
        results.push(json!({"source": name, "sourceBytes": end_byte, "sourceHash": source_hash,
            "records": sequence, "tailHash": previous, "jsMs": js_ms, "rustMs": rust_ms,
            "rustInternalVerifyMs": projected["verifyMs"], "speedup": js_ms / rust_ms,
            "suffixBytes": 0, "sourceParity": true}));
    }
    let evidence = json!({"schemaVersion": 1, "observedAt": now_iso(),
        "mode": "actual_coupled_source_frontier_same_work_v1", "results": results,
        "binding": binding, "networkRequests": 0, "competitionOrders": 0,
        "profitabilityEstablished": false});
    std::fs::write("data/source-projection-benchmark-20261005.json",
        format!("{}\n", serde_json::to_string_pretty(&evidence).map_err(|e| e.to_string())?))
        .map_err(|e| e.to_string())?;
    Ok(evidence)
}
