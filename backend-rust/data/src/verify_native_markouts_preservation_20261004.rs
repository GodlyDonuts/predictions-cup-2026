// data/verify-native-markouts-preservation-20261004.mjs — native markouts preservation + live verify.
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::{js_stringify, sha256};
use serde_json::{json, Value as V};

use std::path::{Path, PathBuf};
use std::time::Instant;

fn rt() -> PathBuf {
    Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default()).join("SigTradingCup").join("paper-runtime")
}
fn read(f: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(f).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn sha(b: &[u8]) -> String { sha256(b) }
fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
}
fn pid_alive(pid: i64) -> bool { pid > 0 && Path::new(&format!("/proc/{}", pid)).exists() }
fn assert_eq(a: bool, code: &str) -> Result<(), String> {
    if a { Ok(()) } else { Err(code.to_string()) }
}

pub fn run(root: &Path) -> Result<V, String> {
    let r = rt();
    let active_file = r.join("native-quote-active.json");
    let active_hash = sha(&std::fs::read(&active_file).map_err(|e| e.to_string())?);
    let a = read(&active_file)?;
    let dir = r.join("research-cohorts").join(a["cohort"].as_str().unwrap_or(""));
    let pointer = read(&dir.join("host-latest.json"))?;
    let bytes = std::fs::read(dir.join(pointer["output"].as_str().unwrap_or("")))
        .map_err(|e| e.to_string())?;
    assert_eq(sha(&bytes) == pointer["outputHash"].as_str().unwrap_or(""), "outputHash")?;
    let prior: V = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let d = read(&dir.join("declaration.json"))?;
    let map = [("books", "public-books.jsonl"), ("quotes", "market-prices.jsonl"),
        ("eligibility", "account-eligibility.jsonl")];
    let mut sources = serde_json::Map::new();
    for (key, value) in prior["capture"]["sources"].as_object().cloned().unwrap_or_default() {
        let file = map.iter().find(|(k, _)| *k == key.as_str())
            .map(|(_, f)| f).copied().unwrap_or("");
        sources.insert(key.clone(), json!({
            "path": r.join(file), "bytes": value["bytes"], "hash": value["hash"],
            "checkpoints": [d["development"][key.as_str()].clone()]}));
    }
    let sf = dir.join("markouts-preservation.sources.json");
    let out = dir.join("markouts-preservation.json");
    std::fs::OpenOptions::new().write(true).create_new(true).open(&sf)
        .and_then(|mut f| std::io::Write::write_all(&mut f,
            js_stringify(&V::Object(sources)).as_bytes()))
        .map_err(|e| e.to_string())?;
    let start = Instant::now();
    let child = std::process::Command::new(baal_collector::rust_backend::native_executable())
        .args(["quote-replay", "--declaration"])
        .arg(dir.join("declaration.json")).args(["--sources"]).arg(&sf)
        .args(["--workers", "8", "--out"]).arg(&out)
        .output().map_err(|e| e.to_string())?;
    let wall_ms = start.elapsed().as_secs_f64() * 1000.0;
    if child.status.code() != Some(0) {
        return Err(String::from_utf8_lossy(&child.stderr).to_string());
    }
    let next = read(&out)?;
    if next["evaluation"]["scorecards"] != prior["evaluation"]["scorecards"] {
        return Err("deepEqual next.evaluation.scorecards prior.evaluation.scorecards".into());
    }
    assert_eq(sha(&std::fs::read(&active_file).map_err(|e| e.to_string())?) == active_hash, "activeFile")?;
    let mut groups: indexmap::IndexMap<String, V> = indexmap::IndexMap::new();
    for c in prior["evaluation"]["scorecards"].as_array().cloned().unwrap_or_default() {
        let key = js_stringify(&json!({"cash": c["point"]["cash"],
            "realizedPnl": c["point"]["realizedPnl"],
            "liquidationPnl": c["point"]["liquidationPnl"],
            "completedTrades": c["point"]["completedTrades"],
            "positions": c["point"]["positions"],
            "fills": c["fills"].as_array().map(|a| a.len()).unwrap_or(0)}));
        if !groups.contains_key(&key) {
            groups.insert(key.clone(), json!({"wallets": 0, "cash": c["point"]["cash"],
                "realizedPnl": c["point"]["realizedPnl"],
                "liquidationPnl": c["point"]["liquidationPnl"],
                "completedTrades": c["point"]["completedTrades"],
                "fillCount": c["fills"].as_array().map(|a| a.len() as i64).unwrap_or(0)}));
        }
        if let Some(g) = groups.get_mut(&key) {
            g["wallets"] = json!(g["wallets"].as_i64().unwrap_or(0) + 1);
        }
    }
    let mut fills: indexmap::IndexMap<String, V> = indexmap::IndexMap::new();
    for c in prior["evaluation"]["scorecards"].as_array().cloned().unwrap_or_default() {
        for f in c["fills"].as_array().cloned().unwrap_or_default() {
            fills.entry(sha(js_stringify(&f).as_bytes())).or_insert(f);
        }
    }
    let stats = read(&r.join("stats.json"))?;
    let view = read(&r.join("native-quote-status.json"))?;
    let pids = read(&root.join("data/current-runtime-pids.json"))?;
    for key in ["collector", "paper", "dashboard"] {
        if !pid_alive(pids[key].as_i64().unwrap_or(0)) {
            return Err(format!("process.kill {}", pids[key]));
        }
    }
    assert_eq(stats["runtime"]["status"].as_str() == Some("running"), "status")?;
    assert_eq(stats["runtime"]["errors"].as_array().map(|a| a.is_empty()).unwrap_or(false), "errors")?;
    assert_eq(view["status"].as_str() == Some("running"), "view.status")?;
    assert_eq(view["lastError"].is_null(), "view.lastError")?;
    assert_eq(read(&r.join("competition-activation.json"))?["enabled"] == json!(false), "activation")?;
    assert_eq(read(&r.join("coordinated-network-budget.json"))?["executionOwner"].as_str()
        == Some("Daenerys"), "executionOwner")?;
    let e = json!({"schemaVersion": 1, "at": ms_to_iso(now_ms()),
        "status": "native_markouts_preservation_and_live_verified",
        "claimComment": 5982524583i64, "through": prior["evaluation"]["evaluationThrough"],
        "activeEngineHash": prior["capture"]["engineHash"],
        "newEngineHash": next["capture"]["engineHash"],
        "activeHash": active_hash, "priorOutputHash": pointer["outputHash"],
        "newOutputHash": sha(&std::fs::read(&out).map_err(|e| e.to_string())?),
        "exactOriginalCards": prior["evaluation"]["scorecards"].as_array().map(|a| a.len() as i64).unwrap_or(0),
        "allOriginalFillsExact": true, "originalWalletsReset": false,
        "activeCohortReplaced": false, "wallMs": wall_ms,
        "paperGroups": V::Array(groups.values().cloned().collect()),
        "uniqueCounterfactualFillVariants": fills.len() as i64,
        "paperFillVariants": V::Array(fills.values().cloned().collect()),
        "collector": {"pid": pids["collector"], "status": stats["runtime"]["status"],
            "cycles": stats["runtime"]["cycles"], "lastCycleAt": stats["runtime"]["lastCycleAt"],
            "errorCount": stats["runtime"]["errors"].as_array().map(|a| a.len() as i64).unwrap_or(0)},
        "native": {"status": view["status"], "verifiedAt": view["verifiedAt"],
            "lastError": view["lastError"], "heldMarketIds": view["heldMarketIds"]},
        "pids": pids, "persistentProcesses": 3, "additionalPersistentProcesses": 0,
        "fees": 0, "networkRequests": 0, "competitionOrders": 0,
        "profitabilityEstablished": false});
    std::fs::OpenOptions::new().write(true).create_new(true)
        .open(root.join("data/native-markouts-preservation-20261004.json"))
        .and_then(|mut f| std::io::Write::write_all(&mut f,
            format!("{}\n", serde_json::to_string_pretty(&e).unwrap_or_default()).as_bytes()))
        .map_err(|e| e.to_string())?;
    println!("{}", js_stringify(&json!({"exactOriginalCards": e["exactOriginalCards"],
        "allOriginalFillsExact": true, "wallMs": e["wallMs"], "through": e["through"],
        "paperGroups": e["paperGroups"], "collector": e["collector"],
        "persistentProcesses": 3, "additionalPersistentProcesses": 0,
        "profitabilityEstablished": false})));
    Ok(e)
}
