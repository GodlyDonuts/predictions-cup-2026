// data/build-realtime-horizons-20261005.mjs — one pass over the same sealed
// sources building causal endpoint-horizon microprice datasets at 5/30/120 min.
use baal_collector::realtime_microprice_dataset::{microprice_data_policy, MicropriceDataset};
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

struct Cohort {
    minutes: i64, out: PathBuf, samples: Arc<Mutex<Vec<V>>>,
    builder: MicropriceDataset<Box<dyn FnMut(V)>>,
}

pub fn run() -> Result<V, String> {
    let root = localappdata()?.join("SigTradingCup").join("paper-runtime");
    let sources = read_json(Path::new("data/realtime-spread-economics-20261005.json"))?["sources"]
        .as_array().cloned().unwrap_or_default();
    let started = perf_ms();
    let mut cohorts: Vec<Cohort> = Vec::new();
    for minutes in [5i64, 30, 120] {
        let out = PathBuf::from(format!("data/realtime-horizon-{}m-dataset-20261005.json", minutes));
        if out.exists() { return Err(format!("FROZEN_DATASET_EXISTS:{}", out.display())); }
        let mut policy = microprice_data_policy();
        policy["horizonMs"] = json!(minutes * 60000);
        policy["endpointResearchAcrossGaps"] = json!(true);
        let samples: Arc<Mutex<Vec<V>>> = Arc::new(Mutex::new(Vec::new()));
        let cb = samples.clone();
        cohorts.push(Cohort { minutes, out, samples,
            builder: MicropriceDataset::<Box<dyn FnMut(V)>>::new(
                Box::new(move |r: V| { cb.lock().unwrap().push(r); }), Some(policy))? });
    }
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
            for c in &mut cohorts { c.builder.consume(&row, file)?; }
        }
        if hash_file(&root.join(file))? != s["sha256"].as_str().unwrap_or("") {
            return Err(format!("SOURCE_HASH_CHANGED:{}", file));
        }
    }
    let mut summaries: Vec<V> = Vec::new();
    for c in cohorts {
        let rows = c.samples.lock().unwrap().clone();
        let mut record_map = serde_json::Map::new();
        record_map.insert("schemaVersion".into(), json!(2));
        record_map.insert("frozenAt".into(), json!(now_iso()));
        record_map.insert("mode".into(), json!("causal_endpoint_horizon_research"));
        let mut report = c.builder.report();
        if let V::Object(r) = &mut report {
            for (k, v) in std::mem::take(r).into_iter() { record_map.insert(k, v); }
        }
        record_map.insert("rows".into(), json!(rows));
        record_map.insert("sources".into(), json!(sources));
        record_map.insert("elapsedMs".into(), json!(perf_ms() - started));
        record_map.insert("networkRequests".into(), json!(0));
        record_map.insert("competitionOrders".into(), json!(0));
        record_map.insert("profitabilityEstablished".into(), json!(false));
        record_map.insert("limitations".into(), json!([
            "Independently observed entry and liquidation prices are endpoint forecasting labels; fills and path-dependent risk are not established.",
            "Missing paths are flagged, never silently treated as continuous. Unlabelled/censored decisions are excluded from forecast metrics, so labelled subsets may have selection bias.",
            "Only fixed chronological purged splits may select models; this is not a funded wallet or a prospective profit result."]));
        let record = V::Object(record_map);
        write_wx(&c.out, format!("{}\n", serde_json::to_string(&record).map_err(|e| e.to_string())?).as_bytes())?;
        let mut summary = record.clone();
        if let V::Object(m) = &mut summary {
            m.insert("rows".into(), json!(record["rows"].as_array().map(|a| a.len()).unwrap_or(0)));
            m.insert("sources".into(), json!(sources.len()));
            m.insert("file".into(), json!(c.out.to_string_lossy()));
            m.insert("sha256".into(), json!(hash_file(&c.out)?));
        }
        summaries.push(summary);
    }
    write_wx(Path::new("data/realtime-horizons-dataset-summary-20261005.json"),
        format!("{}\n", serde_json::to_string_pretty(&json!({
            "schemaVersion": 1, "frozenAt": now_iso(), "horizons": summaries,
            "networkRequests": 0, "competitionOrders": 0
        })).map_err(|e| e.to_string())?).as_bytes())?;
    let console: Vec<V> = summaries.iter().map(|s| json!({
        "policy": s["policy"], "counts": s["counts"], "rows": s["rows"],
        "unresolved": s["unresolved"], "file": s["file"]})).collect();
    Ok(json!(console))
}
