// data/benchmark-journal-native-20261004.mjs — complete legacy/native
// restoration benchmark on the original canonical journal.
use baal_service_runtime::journal::{journal_lines, JournalReadOptions};
use baal_service_runtime::json_text::{js_stringify, sha256};
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};

fn sha(b: &[u8]) -> String { sha256(b) }
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
    let file = r.join("journal.jsonl");
    let milestone = read_json(Path::new("data/baal-rust-milestone-20261004.json"))?;
    let source = milestone["source"].clone();
    let binding = read_json(Path::new("data/journal-native-binding-20261004.json"))?;
    let end_byte = source["bytes"].as_u64().unwrap_or(0);
    let hash_started = perf_ms();
    let hash = baal_collector::native_journal_restore::source_prefix_hash(&file, end_byte, None)?;
    let hash_ms = perf_ms() - hash_started;
    if hash != source["sha256"].as_str().unwrap_or("") {
        return Err("Original source prefix changed".into());
    }
    let native_started = perf_ms();
    let receipt_dir = r.join("journal-native-benchmark-receipts");
    let native = baal_collector::native_journal_restore::read_native_journal(
        &r, &binding, &file, end_byte, &hash, &receipt_dir, None)?;
    let native_ms = perf_ms() - native_started;
    let js_started = perf_ms();
    let mut sequence = 0i64;
    let mut previous = String::new();
    let mut last = V::Null;
    for line in journal_lines(&file, &JournalReadOptions { end_byte: Some(end_byte), ..Default::default() })? {
        if line.is_empty() { continue; }
        let row: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        let mut body = row.clone();
        let hash = row["hash"].as_str().unwrap_or("").to_string();
        if let V::Object(m) = &mut body { m.shift_remove("hash"); }
        if baal_service_runtime::json_text::js_int(&body["sequence"]) != Some(sequence + 1)
            || body["previous"].as_str().unwrap_or("") != previous
            || hash != sha(js_stringify(&body).as_bytes()) {
            return Err("Original JS journal integrity".into());
        }
        sequence = body["sequence"].as_i64().unwrap_or(0);
        previous = hash;
        last = body;
    }
    let js_ms = perf_ms() - js_started;
    if native["records"].as_i64() != Some(sequence)
        || native["tailHash"].as_str() != Some(previous.as_str())
        || ph(&last).ok() != ph(&native["lastBody"]).ok() {
        return Err("Complete legacy/native restoration mismatch".into());
    }
    let code_hashes: serde_json::Map<String, V> =
        ["live-paper/transport-runtime.mjs", "live-paper/journal-lines.mjs",
            "live-paper/native-journal-restore.mjs"].iter().map(|f| -> Result<(String, V), String> {
            Ok((f.to_string(), json!(sha(&std::fs::read(f).map_err(|e| e.to_string())?))))
        }).collect::<Result<_, String>>()?;
    let report = json!({"at": now_iso(),
        "source": {"bytes": source["bytes"], "records": sequence, "days": source["days"],
            "sha256": hash},
        "exactLastBodyParity": true, "exactSequenceAndHashParity": true,
        "lastBodyHash": ph(&last)?, "tailHash": previous,
        "javascriptRestoreMs": js_ms, "prefixHashMs": hash_ms, "nativeAdapterMs": native_ms,
        "nativeVerifyMs": native["verifyMs"],
        "nativeIncludingPrefixHashMs": hash_ms + native_ms,
        "speedupIncludingPrefixBinding": js_ms / (hash_ms + native_ms),
        "binding": binding,
        "measurement": "Same complete sealed original canonical journal; single run; native timing includes an independent Node prefix hash. Byte and economic-state restoration, not trading speed or profitability.",
        "sourceCodeHashes": V::Object(code_hashes),
        "networkRequests": 0, "competitionOrders": 0, "profitabilityEstablished": false});
    baal_service_runtime::atomic::atomic_json_canonical(
        Path::new("data/journal-native-benchmark-20261004.json"), &report)?;
    Ok(report)
}
