// data/record-native-request-clock-20261004.mjs — request-clock milestone update.
use crate::native_request_clock_evidence_helpers_20261004::{aggregate_future, verify_services};
use baal_service_runtime::atomic::atomic_json_canonical as atomic_json;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::{js_stringify, sha256};
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};

fn sha(b: &[u8]) -> String { sha256(b) }
fn read(f: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(f).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn now_iso() -> String {
    ms_to_iso(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64).unwrap_or(0.0))
}
fn fail(msg: &str) -> String { msg.to_string() }
fn eq(a: &V, b: &V) -> Result<(), String> {
    if a == b { Ok(()) } else { Err(fail(&format!("assert.equal: {} !== {}", js_stringify(a), js_stringify(b)))) }
}
fn rt() -> PathBuf {
    Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default()).join("SigTradingCup").join("paper-runtime")
}

pub fn run(root: &Path) -> Result<V, String> {
    let file = root.join("data/native-request-clock-milestone-20261004.json");
    let mut e = read(&file)?;
    let dir = rt().join("research-cohorts").join(e["subsequent"]["cohort"].as_str().unwrap_or(""));
    let matches: Vec<String> = std::fs::read_dir(&dir).map_err(|e2| e2.to_string())?
        .filter_map(|x| x.ok().map(|d| d.file_name().to_string_lossy().to_string()))
        .filter(|f| {
            f.len() >= 6 && f.ends_with(".json")
                && f[..f.len() - 5].bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase() || b == b'-')
                && !f[..f.len() - 5].is_empty()
                && sha(&std::fs::read(dir.join(f)).unwrap_or_default()) == e["subsequent"]["outputHash"].as_str().unwrap_or("")
        }).collect();
    eq(&json!(matches.len() as i64), &json!(1))?;
    let result = read(&dir.join(&matches[0]))?;
    eq(&result["capture"]["engine_hash"], &e["engineHash"])?;
    // const {cardsWithFills,totalFills,...s}=e.subsequent
    let mut s = e["subsequent"].as_object().cloned().unwrap_or_default();
    s.shift_remove("cardsWithFills");
    s.shift_remove("totalFills");
    let agg = aggregate_future(result["evaluation"]["scorecards"].as_array().cloned().unwrap_or_default().as_slice())?;
    for (k, v) in agg.as_object().unwrap() { s.insert(k.clone(), v.clone()); }
    e["subsequent"] = V::Object(s);
    e["services"] = verify_services()?;
    let mut seen = std::collections::HashSet::new();
    let mut limitations: Vec<V> = Vec::new();
    for x in e["limitations"].as_array().cloned().unwrap_or_default().into_iter()
        .chain([json!("Correlated wallets and copied signals are not independent trades or additive P&L"),
            json!("Cash-only inventory-zero floors with stale marks are not realized losses")]) {
        let k = js_stringify(&x);
        if seen.insert(k) { limitations.push(x); }
    }
    e["limitations"] = json!(limitations);
    atomic_json(&file, &e)?;
    for name in ["data/quant-development-state.json", "data/daenerys-monitor.json"] {
        let f = root.join(name);
        let mut x = read(&f)?;
        x["updatedAt"] = json!(now_iso());
        x["nativeRequestClockMilestone"] = e.clone();
        x["nativeRequestClockDeclaration"] = read(&root.join("data/native-request-clock-declaration-20261004.json"))?["receipt"].clone();
        if name.contains("quant-development") {
            x["previousGoalTurnClassification"] = json!("progress");
            x["goalTurn"] = json!({"at": x["updatedAt"], "classification": "progress", "status": "active",
                "blockingCondition": V::Null, "consecutiveBlockedTurns": 0,
                "evidence": ["data/native-request-clock-milestone-20261004.json", "data/pair-anchor-milestone-20261004.json"]});
            x["next"] = json!("Continue the frozen post-13:47:13 UTC native cohort on later original prefixes; no refit/reset. Copied fills are one correlated signal, not 72 independent wins; cash-only inventory-zero floors are not realized losses. Keep pair/maker/probability services running, track genuine closes and cost/depth/latency robustness, and honor NH review expiry. Current official owner remains Daenerys; goal is active.");
            let mut backlog = x["baalBacklog"].as_array().cloned().unwrap_or_default();
            backlog.push(json!({"status": "completed",
                "task": "Versioned Rust request-clock fills, request-age marks and declaration-time starts with exact legacy replay retention",
                "completedAt": x["updatedAt"], "evidence": "data/native-request-clock-milestone-20261004.json"}));
            x["baalBacklog"] = json!(backlog);
        }
        atomic_json(&f, &x)?;
    }
    let out = json!({"recorded": e["status"], "checks": e["checks"],
        "historicalChangedCards": e["historical"]["changedCards"], "subsequent": e["subsequent"],
        "services": e["services"]});
    println!("{}", js_stringify(&out));
    Ok(e)
}
