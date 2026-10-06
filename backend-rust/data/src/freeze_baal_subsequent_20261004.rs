// data/freeze-baal-subsequent-20261004.mjs — declare the subsequent dip cohort
// before any evaluation (development grid-selected parameters).
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
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
fn write_wx(path: &Path, body: &[u8]) -> Result<(), String> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; o.mode(0o600); }
    let mut f = o.open(path).map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut f, body).map_err(|e| e.to_string())
}
fn need_eq(a: &V, b: &V) -> Result<(), String> {
    if a == b { Ok(()) } else { Err("assertion failed".into()) }
}
fn need(ok: bool) -> Result<(), String> {
    if ok { Ok(()) } else { Err("assertion failed".into()) }
}

pub fn run() -> Result<V, String> {
    let runtime = localappdata()?.join("SigTradingCup").join("paper-runtime");
    let prior = runtime.join("research-cohorts").join("baal-net-20261004-e");
    let grid = read_json(&prior.join("native-dip-grid-v2.json"))?;
    need_eq(&grid["capture"]["source_hash"],
        &json!("a19efc0c0ff5d52f8ce851ce5665631d3e3c76899e3e46c3dbc7653723c62507"))?;
    need_eq(&grid["capture"]["source_bytes"], &json!(4255286972u64))?;
    need_eq(&grid["capture"]["source_records"], &json!(9491))?;
    let last_pnl = |c: &V| c["points"].as_array().and_then(|p| p.last())
        .and_then(|p| p["conservativePnl"].as_f64()).unwrap_or(f64::NAN);
    let positives: Vec<V> = grid["sweep"]["scorecards"].as_array().cloned().unwrap_or_default()
        .into_iter().filter(|c| last_pnl(c) > 0.0).collect();
    need_eq(&json!(positives.len()), &json!(9))?;
    let mut baseline_group: Vec<V> = grid["sweep"]["scorecards"].as_array().cloned()
        .unwrap_or_default().into_iter()
        .filter(|c| c["parameters"]["gapResetSeconds"].as_i64() == Some(120)).collect();
    baseline_group.sort_by(|a, b| last_pnl(b).partial_cmp(&last_pnl(a))
        .unwrap_or(std::cmp::Ordering::Equal));
    let baseline = baseline_group.first().cloned().unwrap_or(V::Null);
    let cohort = "baal-dip-subsequent-20261004-v1";
    let dir = runtime.join("research-cohorts").join(cohort);
    need(!dir.exists())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    }
    let config = read_json(&prior.join("native-sweep-config.json"))?;
    need_eq(&config["feeBps"], &json!(0))?;
    let mut strategies: Vec<V> = positives.iter().enumerate()
        .map(|(i, c)| json!({"id": format!("development-positive-{}", i + 1),
            "parameters": c["parameters"]})).collect();
    strategies.push(json!({"id": "short-gap-control",
        "parameters": baseline["parameters"]}));
    let declaration = json!({"schemaVersion": 1,
        "mode": "causal_dip_subsequent_v1", "cohort": cohort,
        "declaredAt": now_iso(), "config": config,
        "development": {"records": 9491, "bytes": 4255286972u64,
            "hash": grid["capture"]["source_hash"],
            "through": baal_service_runtime::contract::ms_to_iso(
                baal_service_runtime::contract::parse_iso_ms(
                    grid["capture"]["through"].as_str().unwrap_or(""))
                .unwrap_or(f64::NAN))},
        "strategies": strategies,
        "scenarios": [
            {"id": "baseline", "latencyMs": 3000, "slippageTicks": 1,
                "depthFractionBps": 100},
            {"id": "fast-read", "latencyMs": 1000, "slippageTicks": 1,
                "depthFractionBps": 100},
            {"id": "slow-read", "latencyMs": 15000, "slippageTicks": 1,
                "depthFractionBps": 100},
            {"id": "two-tick-cost", "latencyMs": 3000, "slippageTicks": 2,
                "depthFractionBps": 100},
            {"id": "half-depth", "latencyMs": 3000, "slippageTicks": 1,
                "depthFractionBps": 50}],
        "stopBufferTicks": 2});
    let declaration_bytes = format!("{}\n",
        serde_json::to_string_pretty(&declaration).map_err(|e| e.to_string())?);
    write_wx(&dir.join("declaration.json"), declaration_bytes.as_bytes())?;
    let journal = runtime.join("journal.jsonl");
    let (end, journal_sha) = {
        let mut fd = std::fs::File::open(&journal).map_err(|e| e.to_string())?;
        let size = fd.metadata().map_err(|e| e.to_string())?.len();
        let tail_length = std::cmp::min(size, 8 * 1024 * 1024);
        let mut tail = vec![0u8; tail_length as usize];
        fd.seek(SeekFrom::Start(size - tail_length)).map_err(|e| e.to_string())?;
        fd.read_exact(&mut tail).map_err(|e| e.to_string())?;
        let line = tail.iter().rposition(|c| *c == b'\n').ok_or("assertion failed")?;
        let end = size - tail_length + line as u64 + 1;
        let mut digest = Sha256::new();
        let mut buffer = vec![0u8; 4 * 1024 * 1024];
        let mut at = 0u64;
        while at < end {
            let want = std::cmp::min(buffer.len() as u64, end - at) as usize;
            fd.seek(SeekFrom::Start(at)).map_err(|e| e.to_string())?;
            let n = fd.read(&mut buffer[..want]).map_err(|e| e.to_string())?;
            need(n > 0)?;
            digest.update(&buffer[..n]);
            at += n as u64;
        }
        (end, format!("{:x}", digest.finalize()))
    };
    let binding = json!({"at": now_iso(), "cohort": cohort, "endByte": end,
        "sourceHash": journal_sha,
        "declarationHash": sha(declaration_bytes.as_bytes()),
        "selectionSourceHash": grid["capture"]["source_hash"],
        "selectionSourceBytes": grid["capture"]["source_bytes"],
        "selectionSourceRecords": grid["capture"]["source_records"],
        "candidates": declaration["strategies"].as_array().map(|a| a.len()).unwrap_or(0)
            * declaration["scenarios"].as_array().map(|a| a.len()).unwrap_or(0) * 2,
        "marketDataWasCollectedEarlier": true,
        "subsequentPerformanceNotInspectedBeforeDeclaration": true,
        "evaluationType": "retrospective_subsequent_data_not_live_prospective_trading",
        "competitionOrders": 0, "networkRequests": 0});
    write_wx(&dir.join("source-binding.json"),
        format!("{}\n", serde_json::to_string_pretty(&binding).map_err(|e| e.to_string())?).as_bytes())?;
    std::fs::write("data/baal-subsequent-binding-20261004.json",
        format!("{}\n", serde_json::to_string_pretty(&binding).map_err(|e| e.to_string())?))
        .map_err(|e| e.to_string())?;
    Ok(binding)
}
