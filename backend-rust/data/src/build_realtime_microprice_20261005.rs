// data/build-realtime-microprice-20261005.mjs — market-only causal microprice
// training dataset over the same sealed sources.
use baal_collector::realtime_microprice_dataset::MicropriceDataset;
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

pub fn run() -> Result<V, String> {
    let root = localappdata()?.join("SigTradingCup").join("paper-runtime");
    let sources = read_json(Path::new("data/realtime-spread-economics-20261005.json"))?["sources"]
        .as_array().cloned().unwrap_or_default();
    let rows: Arc<Mutex<Vec<V>>> = Arc::new(Mutex::new(Vec::new()));
    let cb = rows.clone();
    let mut builder = MicropriceDataset::new(
        Box::new(move |r: V| { cb.lock().unwrap().push(r); }), None)?;
    let started = perf_ms();
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
            builder.consume(&row, file)?;
        }
        let mut h = Sha256::new();
        let mut fd = std::fs::File::open(root.join(file)).map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; 1024 * 1024];
        loop {
            let n = fd.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 { break; }
            h.update(&buf[..n]);
        }
        if format!("{:x}", h.finalize()) != s["sha256"].as_str().unwrap_or("") {
            return Err("SOURCE_HASH_CHANGED".into());
        }
    }
    let rows = rows.lock().unwrap().clone();
    let mut m = serde_json::Map::new();
    m.insert("schemaVersion".into(), json!(2));
    m.insert("frozenAt".into(), json!(now_iso()));
    m.insert("mode".into(), json!("market_only_causal_microprice_training_dataset"));
    let mut report = builder.report();
    if let V::Object(r) = &mut report { for (k, v) in std::mem::take(r).into_iter() { m.insert(k, v); } }
    m.insert("rows".into(), json!(rows.clone()));
    m.insert("sources".into(), json!(sources));
    m.insert("elapsedMs".into(), json!(perf_ms() - started));
    m.insert("networkRequests".into(), json!(0));
    m.insert("competitionOrders".into(), json!(0));
    m.insert("profitabilityEstablished".into(), json!(false));
    let record = V::Object(m);
    write_wx(Path::new("data/realtime-microprice-dataset-v2-20261005.json"),
        format!("{}\n", serde_json::to_string(&record).map_err(|e| e.to_string())?).as_bytes())?;
    let mut summary = record.clone();
    if let V::Object(sm) = &mut summary {
        sm.insert("rows".into(), json!(rows.len()));
        sm.insert("sources".into(), json!(sources.len()));
    }
    Ok(summary)
}
