// data/prepare-baal-cost-study-20261004.mjs — declare the causal dip
// development cost diagnostic cohort (selection from the prior native grid).
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Value as V};
use std::io::{Read, Seek, SeekFrom};
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
fn write_wx(p: &Path, s: &str) -> Result<(), String> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)] { std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600); }
    let mut f = o.open(p).map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut f, s.as_bytes()).map_err(|e| e.to_string())
}
fn need_eq(a: &V, b: &V) -> Result<(), String> {
    if a == b { Ok(()) } else { Err("assertion failed".into()) }
}
fn conservative_pnl(c: &V) -> f64 {
    c["points"].as_array().and_then(|p| p.last())
        .and_then(|p| p["conservativePnl"].as_f64()).unwrap_or(f64::NAN)
}

pub fn run() -> Result<V, String> {
    let runtime = localappdata()?.join("SigTradingCup").join("paper-runtime");
    let prior = runtime.join("research-cohorts").join("baal-net-20261004-e");
    let grid = read_json(&prior.join("native-dip-grid-v2.json"))?;
    need_eq(&grid["capture"]["source_hash"],
        &json!("a19efc0c0ff5d52f8ce851ce5665631d3e3c76899e3e46c3dbc7653723c62507"))?;
    need_eq(&grid["capture"]["source_records"], &json!(9491))?;
    need_eq(&grid["capture"]["source_bytes"], &json!(4255286972u64))?;
    let positive: Vec<V> = grid["sweep"]["scorecards"].as_array().cloned()
        .unwrap_or_default().into_iter()
        .filter(|c| conservative_pnl(c) > 0.0).collect();
    need_eq(&json!(positive.len() as i64), &json!(9))?;
    let mut selected: indexmap::IndexMap<String, V> = indexmap::IndexMap::new();
    let mut add = |c: &V, id: String| {
        let key = serde_json::to_string(&c["parameters"]).unwrap_or_default();
        if !selected.contains_key(&key) {
            selected.insert(key, json!({"id": id, "parameters": c["parameters"]}));
        }
    };
    for (i, c) in positive.iter().enumerate() {
        add(c, format!("historical-positive-{}", i + 1));
    }
    let cards_all: Vec<V> = grid["sweep"]["scorecards"].as_array().cloned()
        .unwrap_or_default();
    for gap in [120i64, 900, 3600, 21600] {
        let mut cards: Vec<V> = cards_all.iter()
            .filter(|c| c["parameters"]["gapResetSeconds"].as_i64() == Some(gap))
            .cloned().collect();
        cards.sort_by(|a, b| conservative_pnl(b).partial_cmp(&conservative_pnl(a))
            .unwrap_or(std::cmp::Ordering::Equal));
        add(&cards[0], format!("gap-{}-best", gap));
        add(&cards[cards.len() / 2], format!("gap-{}-median", gap));
        add(&cards[cards.len() - 1], format!("gap-{}-worst", gap));
    }
    // Read the first journal record (byte-exact).
    let journal = runtime.join("journal.jsonl");
    let mut fd = std::fs::File::open(&journal).map_err(|e| e.to_string())?;
    let mut first: Option<Vec<u8>> = None;
    let mut chunks: Vec<Vec<u8>> = Vec::new();
    let mut length = 0u64;
    while first.is_none() {
        let mut chunk = vec![0u8; 65536];
        fd.seek(SeekFrom::Start(length)).map_err(|e| e.to_string())?;
        let n = fd.read(&mut chunk).map_err(|e| e.to_string())?;
        if n == 0 { return Err("assertion failed".into()); }
        let index = chunk[..n].iter().position(|c| *c == b'\n');
        let take = index.map(|i| i + 1).unwrap_or(n);
        chunks.push(chunk[..take].to_vec());
        length += take as u64;
        if length > 8 * 1024 * 1024 { return Err("assertion failed".into()); }
        if index.is_some() { first = Some(chunks.concat()); }
    }
    let first = first.unwrap();
    let first_record: V = serde_json::from_slice(&first).map_err(|e| e.to_string())?;
    need_eq(&first_record["sequence"], &json!(1))?;
    let cohort = "baal-dip-development-costs-20261004-v1";
    let dir = runtime.join("research-cohorts").join(cohort);
    if dir.exists() { return Err("assertion failed".into()); }
    #[cfg(unix)] {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().mode(0o700).create(&dir).map_err(|e| e.to_string())?;
    }
    #[cfg(not(unix))] { std::fs::create_dir(&dir).map_err(|e| e.to_string())?; }
    let config = read_json(&prior.join("native-sweep-config.json"))?;
    need_eq(&config["feeBps"], &json!(0))?;
    let declaration = json!({"schemaVersion": 1,
        "mode": "causal_dip_development_diagnostic_v1", "cohort": cohort,
        "declaredAt": now_iso(), "config": config,
        "development": {"records": 1, "bytes": first.len(),
            "hash": sha(&first),
            "through": first_record["at"]},
        "strategies": selected.values().cloned().collect::<Vec<_>>(),
        "scenarios": [
            {"id": "baseline", "latencyMs": 3000, "slippageTicks": 1, "depthFractionBps": 100},
            {"id": "fast-read", "latencyMs": 1000, "slippageTicks": 1, "depthFractionBps": 100},
            {"id": "slow-read", "latencyMs": 15000, "slippageTicks": 1, "depthFractionBps": 100},
            {"id": "two-tick-cost", "latencyMs": 3000, "slippageTicks": 2, "depthFractionBps": 100},
            {"id": "half-depth", "latencyMs": 3000, "slippageTicks": 1, "depthFractionBps": 50}],
        "stopBufferTicks": 2});
    let bytes = format!("{}\n", serde_json::to_string_pretty(&declaration)
        .map_err(|e| e.to_string())?);
    write_wx(&dir.join("declaration.json"), &bytes)?;
    let binding = json!({"at": now_iso(), "cohort": cohort,
        "evaluationType": "reused_development_data_cost_diagnostic_not_holdout_or_profit_proof",
        "selectionPolicy": "all-nine-development-positive-plus-best-median-worst-per-gap-deduplicated",
        "selectionSourceHash": grid["capture"]["source_hash"],
        "selectionSourceRecords": grid["capture"]["source_records"],
        "selectionSourceBytes": grid["capture"]["source_bytes"],
        "endByte": grid["capture"]["source_bytes"],
        "sourceHash": grid["capture"]["source_hash"],
        "declarationHash": sha(bytes.as_bytes()),
        "strategies": declaration["strategies"].as_array().map(|a| a.len()).unwrap_or(0),
        "candidates": declaration["strategies"].as_array().map(|a| a.len()).unwrap_or(0)
            * declaration["scenarios"].as_array().map(|a| a.len()).unwrap_or(0) * 2,
        "competitionOrders": 0, "networkRequests": 0});
    write_wx(&dir.join("source-binding.json"),
        &format!("{}\n", serde_json::to_string_pretty(&binding)
            .map_err(|e| e.to_string())?))?;
    Ok(binding)
}
