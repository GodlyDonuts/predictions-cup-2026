// data/benchmark-native-publication-20261004.mjs — native publication prefix
// vs JS book-publication reducer on the same sealed capture prefix.
use baal_collector::book_publication_v2::{apply_book_publication, create_book_publication_state};
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};

fn sha(b: &[u8]) -> String { sha256(b) }
fn ph(v: &V) -> Result<String, String> { baal_execution::core::payload_hash(v) }
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
fn write_wx(path: &Path, body: &[u8]) -> Result<(), String> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; o.mode(0o600); }
    let mut f = o.open(path).map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut f, body).map_err(|e| e.to_string())
}
fn uuid_v4() -> String { uuid::Uuid::new_v4().to_string() }

pub fn run() -> Result<V, String> {
    let runtime = localappdata()?.join("SigTradingCup").join("paper-runtime");
    let dir = runtime.join("native-publication").join(uuid_v4());
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    }
    let journal = runtime.join("public-books.jsonl");
    let raw = std::fs::read(&journal).map_err(|e| e.to_string())?;
    let bytes = match raw.iter().rposition(|c| *c == b'\n') {
        Some(n) => &raw[..n + 1],
        None => &raw[..0],
    };
    let source_hash = sha(bytes);
    let scope = dir.join("scope.json");
    let universe: V = serde_json::from_slice(&std::fs::read("live-paper/price-universe.json")
        .map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    write_wx(&scope, format!("{}\n", serde_json::to_string(&universe).map_err(|e| e.to_string())?).as_bytes())?;
    let executable = baal_collector::rust_backend::native_executable();
    let pinned = sha(&std::fs::read(&executable).map_err(|e| e.to_string())?);
    let mut start = perf_ms();
    let native = baal_collector::rust_backend::read_native_publication_prefix(
        &journal, &scope, bytes.len() as u64, &source_hash,
        &dir.join("native.json"), &executable, &pinned)?;
    let native_ms = perf_ms() - start;
    start = perf_ms();
    let mut state = create_book_publication_state(&universe)?;
    let text = String::from_utf8_lossy(bytes);
    for line in text.split('\n').collect::<Vec<_>>().iter().take(
        text.split('\n').count().saturating_sub(1)) {
        let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
        state = apply_book_publication(&state, &row)?;
    }
    let js_ms = perf_ms() - start;
    if native["state"] != state { return Err("assertion failed".into()); }
    let result = json!({"schemaVersion": 1, "at": now_iso(),
        "mode": "same_prefix_publication_validation_benchmark",
        "sourceBytes": bytes.len(), "sourceRecords": native["sourceRecords"],
        "sourceHash": source_hash, "scopeHash": native["scopeHash"],
        "engineHash": native["engineHash"],
        "binaryHash": sha(&std::fs::read(&executable).map_err(|e| e.to_string())?),
        "jsMs": js_ms, "nativeMs": native_ms, "nativeVerifyMs": native["verifyMs"],
        "speedup": js_ms / native_ms, "stateHash": ph(&state)?, "exactStateParity": true,
        "networkRequests": 0, "competitionOrders": 0, "profitabilityEstablished": false});
    std::fs::write("data/baal-native-publication-benchmark-20261004.json",
        format!("{}\n", serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?))
        .map_err(|e| e.to_string())?;
    Ok(result)
}
