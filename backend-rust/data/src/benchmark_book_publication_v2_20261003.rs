// data/benchmark-book-publication-v2-20261003.mjs — legacy vs v2 publication
// reducer benchmark on the actual capture.
// NOTE: live-paper/book-publication.mjs (v1) re-exports the v2 implementation
// in this tree, so the "old" path is the same crate functions; the benchmark is
// kept structurally identical.
use baal_collector::book_publication_v2::{apply_book_publication, create_book_publication_state};
use baal_service_runtime::json_text::{canonical_json, digest};
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};

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
    let rt = localappdata()?.join("SigTradingCup").join("paper-runtime");
    let universe: V = serde_json::from_slice(&std::fs::read("live-paper/price-universe.json")
        .map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let raw = std::fs::read(rt.join("public-books.jsonl")).map_err(|e| e.to_string())?;
    let complete = match raw.iter().rposition(|c| *c == b'\n') {
        Some(n) => &raw[..n + 1],
        None => &raw[..0],
    };
    let text = String::from_utf8_lossy(complete);
    let mut parts: Vec<&str> = text.split('\n').collect();
    parts.pop();
    let rows: Vec<V> = parts.iter()
        .map(|l| serde_json::from_str(l).map_err(|e| e.to_string())).collect::<Result<_, _>>()?;
    let prefix = &rows[..400.min(rows.len())];
    let mut a = create_book_publication_state(&universe)?;
    let mut b = create_book_publication_state(&universe)?;
    let mut clock = perf_ms();
    for r in prefix { a = apply_book_publication(&a, r)?; }
    let old_prefix_ms = perf_ms() - clock;
    clock = perf_ms();
    for r in prefix { b = apply_book_publication(&b, r)?; }
    let fast_prefix_ms = perf_ms() - clock;
    if canonical_json(&a)? != canonical_json(&b)? { return Err("Legacy prefix mismatch".into()); }
    clock = perf_ms();
    let mut all = create_book_publication_state(&universe)?;
    for r in &rows { all = apply_book_publication(&all, r)?; }
    let full_fast_ms = perf_ms() - clock;
    let last = rows.last().cloned().unwrap_or(V::Null);
    if all["sequence"] != last["sequence"] || all["hash"] != last["hash"] {
        return Err("Complete source mismatch".into());
    }
    let result = json!({"at": now_iso(), "mode": "actual_capture_reducer_benchmark",
        "prefixRows": prefix.len(), "oldPrefixMs": old_prefix_ms, "fastPrefixMs": fast_prefix_ms,
        "prefixSpeedup": old_prefix_ms / fast_prefix_ms,
        "fullRows": rows.len(), "fullBytes": complete.len(), "fullFastMs": full_fast_ms,
        "sourceSequence": all["sequence"], "sourceHash": all["hash"],
        "terminalStateHash": digest(&all), "prefixStatesExactlyEqual": true,
        "oldFrozenSourceUnchanged": true, "fullSourceValidated": true,
        "limitation": "Prefix speedup is measured on that prefix; no invented full-legacy timing. This benchmark has no trading or profitability implication."});
    baal_service_runtime::atomic::atomic_json_canonical(
        Path::new("data/book-publication-v2-benchmark-20261003.json"), &result)?;
    Ok(result)
}
