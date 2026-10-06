// data/build-realtime-quote-dataset-20261005.mjs — causal quote->outcome
// training dataset over the same sealed sources (latencyMs===3000 policy).
use baal_collector::realtime_microprice_dataset::{microprice_features_spec};
use baal_collector::realtime_spread_economics::{
    create_realtime_spread_economics, realtime_economics_policies};
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

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
fn write_wx(path: &Path, body: &[u8]) -> Result<(), String> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; o.mode(0o600); }
    let mut f = o.open(path).map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut f, body).map_err(|e| e.to_string())
}
fn hash_file(file: &Path) -> Result<String, String> {
    let mut h = Sha256::new();
    let mut fd = std::fs::File::open(file).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = fd.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 { break; }
        h.update(&buf[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}

pub fn run() -> Result<V, String> {
    let root = localappdata()?.join("SigTradingCup").join("paper-runtime");
    let sources = read_json(Path::new("data/realtime-spread-economics-20261005.json"))?["sources"]
        .as_array().cloned().unwrap_or_default();
    let policies: Vec<V> = realtime_economics_policies().into_iter()
        .filter(|p| p["latencyMs"].as_i64() == Some(3000)).collect();
    let rows: Arc<Mutex<Vec<V>>> = Arc::new(Mutex::new(Vec::new()));
    let quotes: Arc<Mutex<Vec<V>>> = Arc::new(Mutex::new(Vec::new()));
    let q_cb = quotes.clone();
    let o_cb = rows.clone();
    let mut study = create_realtime_spread_economics(
        Some(policies.clone()),
        |_: V| V::Bool(true),
        move |q: &V| { q_cb.lock().unwrap().push(q.clone()); },
        |_: &V| {},
        move |o: &V| { o_cb.lock().unwrap().push(o.clone()); });
    let started = perf_ms();
    let out = Path::new("data/realtime-quote-dataset-20261005.json");
    if out.exists() { return Err("FROZEN_DATASET_EXISTS".into()); }
    for s in &sources {
        let file = s["file"].as_str().unwrap_or("");
        let stream = std::fs::File::open(root.join(file)).map_err(|e| e.to_string())?;
        let mut reader = BufReader::with_capacity(1024 * 1024, stream);
        let mut line = String::new();
        loop {
            line.clear();
            if reader.read_line(&mut line).map_err(|e| e.to_string())? == 0 { break; }
            let trimmed = line.trim_end_matches(['\n', '\r']);
            if trimmed.is_empty() { continue; }
            let row: V = serde_json::from_str(trimmed).map_err(|e| e.to_string())?;
            study.consume(&row, file)?;
        }
        if hash_file(&root.join(file))? != s["sha256"].as_str().unwrap_or("") {
            return Err("SOURCE_HASH_CHANGED".into());
        }
    }
    let rows = rows.lock().unwrap().clone();
    let quotes = quotes.lock().unwrap().clone();
    let mut report = study.report();
    // The report is a compact summary; rows already preserve each exact outcome.
    if let V::Array(ps) = &mut report["policies"] {
        for p in ps.iter_mut() {
            if let V::Object(m) = p { m.shift_remove("outcomes"); }
        }
    }
    let result = json!({"schemaVersion": 1, "frozenAt": now_iso(),
        "mode": "causal_quote_outcome_training_dataset",
        "features": microprice_features_spec(), "policies": policies, "sources": sources,
        "quotes": quotes, "rows": rows, "report": report,
        "elapsedMs": perf_ms() - started, "networkRequests": 0, "competitionOrders": 0,
        "profitabilityEstablished": false,
        "limitations": ["Strict-through anonymous prints are hypothetical fills; queue position and actual execution are not proven.",
            "Censored outcomes have unknown economic value, never a zero-profit training target.",
            "Frozen decision features contain no later fills or prices. Overlapping outcomes are not funded P&L.",
            "Development captures were previously examined; independent subsequent replay and prospective wallet accounting remain necessary."]});
    write_wx(out, format!("{}\n", serde_json::to_string(&result).map_err(|e| e.to_string())?).as_bytes())?;
    let mut summary = result.clone();
    let q_len = result["quotes"].as_array().map(|a| a.len()).unwrap_or(0);
    let r_len = result["rows"].as_array().map(|a| a.len()).unwrap_or(0);
    if let V::Object(m) = &mut summary {
        m.shift_remove("quotes");
        m.shift_remove("rows");
        // JS destructures quotes/rows out then re-appends counts at the END.
        m.insert("quotes".into(), json!(q_len));
        m.insert("rows".into(), json!(r_len));
        m.insert("file".into(), json!(out.to_string_lossy()));
        m.insert("sha256".into(), json!(sha256(&std::fs::read(out).map_err(|e| e.to_string())?)));
    }
    write_wx(Path::new("data/realtime-quote-dataset-summary-20261005.json"),
        format!("{}\n", serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())?).as_bytes())?;
    Ok(summary)
}
