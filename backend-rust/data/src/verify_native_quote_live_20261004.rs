// data/verify-native-quote-live-20261004.mjs — native quote live verification.
use baal_collector::native_quote_proof_v1::verify_native_quote_result;
use baal_service_runtime::atomic::atomic_json_canonical;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::{js_stringify, sha256};
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};

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
fn ae(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(code.to_string()) }
}

pub fn run(root: &Path) -> Result<V, String> {
    let r = rt();
    let active = read(&r.join("native-quote-active.json"))?;
    let dir = r.join("research-cohorts").join(active["cohort"].as_str().unwrap_or(""));
    let binding = read(&dir.join("binding.json"))?;
    let d = read(&dir.join("declaration.json"))?;
    let universe_raw: V = serde_json::from_slice(
        &std::fs::read(root.join("live-paper/price-universe.json")).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let universe = baal_collector::breadth::validate_universe(&universe_raw,
        d["config"]["tournamentId"].as_str().unwrap_or(""))?;
    let status = read(&r.join("stats.json"))?;
    let pids = read(&root.join("data/current-runtime-pids.json"))?;
    let view = read(&r.join("native-quote-status.json"))?;
    let pointer = read(&dir.join("host-latest.json"))?;
    let out = std::fs::read(dir.join(pointer["output"].as_str().unwrap_or("")))
        .map_err(|e| e.to_string())?;
    let result: V = serde_json::from_slice(&out).map_err(|e| e.to_string())?;
    ae(sha(&out) == pointer["outputHash"].as_str().unwrap_or(""), "outputHash")?;
    let projection = verify_native_quote_result(&result, &d, &binding, &universe, None)?;
    ae(status["runtime"]["status"].as_str() == Some("running"), "status")?;
    ae(status["runtime"]["pid"] == pids["collector"], "pid")?;
    ae(status["runtime"]["cycles"].as_i64().unwrap_or(0) >= 2, "cycles")?;
    ae(view["status"].as_str() == Some("running"), "view.status")?;
    ae(view["lastError"].is_null(), "view.lastError")?;
    ae(!view["verifiedAt"].is_null(), "view.verifiedAt")?;
    ae(projection["candidates"].as_i64() == Some(72), "candidates")?;
    ae(projection["fees"].as_i64() == Some(0), "fees")?;
    ae(projection["sources"]["eligibility"]["records"].as_i64().unwrap_or(0) >= 2, "eligibility")?;
    ae(result["capture"]["engineHash"] == binding["engineHash"], "engineHash")?;
    ae(!r.join("COLLECTOR_STOP").exists(), "COLLECTOR_STOP")?;
    ae(read(&r.join("competition-activation.json"))?["enabled"] == json!(false), "activation")?;
    let budget = read(&r.join("coordinated-network-budget.json"))?;
    ae(budget["writesEnabled"] == json!(false), "writesEnabled")?;
    ae(budget["maxReadsPerMinute"].as_i64() == Some(20), "maxReadsPerMinute")?;
    ae(budget["executionOwner"].as_str() == Some("Daenerys"), "executionOwner")?;
    for key in ["collector", "paper", "dashboard"] {
        if !pid_alive(pids[key].as_i64().unwrap_or(0)) {
            return Err(format!("process.kill {}", pids[key]));
        }
    }
    let errors = std::fs::read_to_string(
        r.join("collector-native-quote-20261004.stderr.log")).map_err(|e| e.to_string())?;
    ae(errors.trim().is_empty(), "stderr.log")?;
    let mut diagnostics: serde_json::Map<String, V> = serde_json::Map::new();
    for card in result["evaluation"]["scorecards"].as_array().cloned().unwrap_or_default() {
        for (k, n) in card["signalDiagnostics"].as_object().cloned().unwrap_or_default() {
            let n = n.as_f64().unwrap_or(f64::NAN);
            match diagnostics.get_mut(&k) {
                None => { diagnostics.insert(k, json!({"min": n, "max": n})); }
                Some(d) => {
                    let mn = d["min"].as_f64().unwrap_or(f64::NAN).min(n);
                    let mx = d["max"].as_f64().unwrap_or(f64::NAN).max(n);
                    d["min"] = json!(mn); d["max"] = json!(mx);
                }
            }
        }
    }
    let e = json!({"schemaVersion": 1, "at": ms_to_iso(now_ms()),
        "status": "native_quote_history_live_verified", "cohort": active["cohort"],
        "cutoff": d["declaredAt"], "declarationHash": binding["declarationHash"],
        "engineHash": binding["engineHash"], "binaryHash": binding["binaryHash"],
        "activeHash": sha(&std::fs::read(r.join("native-quote-active.json")).map_err(|e| e.to_string())?),
        "outputHash": pointer["outputHash"],
        "sourceBytes": result["capture"]["sources"].as_object().cloned().unwrap_or_default()
            .values().map(|s| s["bytes"].as_i64().unwrap_or(0)).sum::<i64>(),
        "sourceEvents": result["capture"]["events"], "sources": result["capture"]["sources"],
        "verifiedAt": view["verifiedAt"], "through": view["through"],
        "lastWallMs": view["lastWallMs"],
        "collector": {"pid": pids["collector"], "status": status["runtime"]["status"],
            "cycles": status["runtime"]["cycles"], "lastCycleAt": status["runtime"]["lastCycleAt"],
            "errors": status["runtime"]["errors"].as_array().map(|a| a.len() as i64).unwrap_or(0),
            "readRequests": status["runtime"]["readRequests"],
            "bookMarkets": status["runtime"]["scheduling"]["reads"].as_array()
                .cloned().unwrap_or_default().iter().map(|x| x["marketId"].clone()).collect::<Vec<V>>()},
        "oldNativeHostRevision": status["runtime"]["nativeDip"]["hostRevision"],
        "oldNativeGroups": status["runtime"]["nativeDip"]["groups"],
        "pids": pids, "persistentProcesses": 3, "additionalPersistentProcesses": 0,
        "candidates": 72, "heldMarketIds": projection["heldMarketIds"],
        "pendingMarketIds": view["pendingMarketIds"],
        "uniqueFillEvents": projection["uniqueFillEvents"], "cards": projection["cards"],
        "diagnostics": V::Object(diagnostics), "fees": 0, "readBudget": budget,
        "sourceBeforeRequestEligibilityVerified": true,
        "walletResultsAreCorrelated": true, "competitionOrders": 0,
        "profitabilityEstablished": false});
    atomic_json_canonical(&root.join("data/native-quote-live-20261004.json"), &e)
        .map_err(|e| e.to_string())?;
    println!("{}", js_stringify(&json!({"status": e["status"],
        "cycles": e["collector"]["cycles"], "candidates": 72,
        "lastWallMs": e["lastWallMs"],
        "eligibilityRecords": e["sources"]["eligibility"]["records"],
        "uniqueFillEvents": e["uniqueFillEvents"], "heldMarketIds": e["heldMarketIds"],
        "pendingMarketIds": e["pendingMarketIds"], "errors": e["collector"]["errors"],
        "bookMarkets": e["collector"]["bookMarkets"], "fees": 0,
        "profitabilityEstablished": false})));
    Ok(e)
}
