// data/record-native-dip-host-20261004.mjs — native dip host milestone record.
use baal_service_runtime::atomic::atomic_json_canonical as atomic_json;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::js_stringify;
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};

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
fn a_true(v: &V) -> Result<(), String> {
    let t = match v {
        V::Null => false,
        V::Bool(b) => *b,
        V::Number(n) => n.as_f64().is_some_and(|x| x != 0.0 && !x.is_nan()),
        V::String(s) => !s.is_empty(),
        _ => true,
    };
    if t { Ok(()) } else { Err(fail("assertion failed")) }
}
fn pnull(x: &V, key: &str) -> Result<(), String> {
    if x.as_object().is_some_and(|m| m.get(key).is_some_and(|v| v.is_null())) { Ok(()) } else { Err(fail("assert.equal: not null")) }
}
fn kill0(pid: &V) -> Result<(), String> {
    let p = pid.as_i64().unwrap_or(-1);
    let ok = std::process::Command::new("/bin/kill").arg("-0").arg(p.to_string()).status()
        .map(|s| s.success()).unwrap_or(false);
    if ok { Ok(()) } else { Err(format!("kill ESRCH {}", p)) }
}
fn re_word(text: &str, pat: &str) -> bool {
    // /pat\b/ — pat followed by a non-word char or end.
    match text.find(pat) {
        Some(i) => {
            let next = text.as_bytes().get(i + pat.len());
            next.is_none_or(|c| !c.is_ascii_alphanumeric() && *c != b'_')
        }
        None => false,
    }
}
fn rt() -> PathBuf {
    Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default()).join("SigTradingCup").join("paper-runtime")
}

pub fn run(root: &Path) -> Result<V, String> {
    let r = rt();
    let s = read(&r.join("stats.json"))?;
    let n = read(&r.join("native-dip-status.json"))?;
    let pids = read(&root.join("data/current-runtime-pids.json"))?;
    let prep = read(&root.join("data/native-dip-host-preparation-20261004.json"))?;
    let preserved = read(&root.join("data/native-dip-host-preservation-after-20261004.json"))?;
    let budget = read(&r.join("coordinated-network-budget.json"))?;
    for pid in ["collector", "paper", "dashboard"] { kill0(&pids[pid])?; }
    eq(&s["runtime"]["pid"], &pids["collector"])?;
    eq(&s["runtime"]["status"], &json!("running"))?;
    if !(s["runtime"]["cycles"].as_i64().unwrap_or(0) >= 1) { return Err(fail("assertion failed")); }
    eq(&n["status"], &json!("running"))?;
    a_true(&n["verifiedAt"])?;
    pnull(&n, "lastError")?;
    eq(&n["activeHash"], &prep["activeHash"])?;
    if !(n["sourceBytes"].as_i64().unwrap_or(0) > prep["sourceBytes"].as_i64().unwrap_or(0)) { return Err(fail("assertion failed")); }
    eq(&n["candidates"], &json!(100))?;
    eq(&n["fees"], &json!(0))?;
    eq(&n["competitionOrders"], &json!(0))?;
    eq(&budget["maxReadsPerMinute"], &json!(20))?;
    eq(&budget["writesEnabled"], &json!(false))?;
    eq(&budget["executionOwner"], &json!("Daenerys"))?;
    eq(&read(&r.join("competition-activation.json"))?["enabled"], &json!(false))?;
    eq(&preserved["originalJournalsPreserved"], &json!(true))?;
    let checks = std::fs::read_to_string(root.join("data/native-dip-host-checks-20261004.tap")).map_err(|e| e.to_string())?;
    if !(checks.contains("pass 96") && checks.contains("fail 0") && checks.contains("skipped 0")) {
        return Err(fail("assertion failed"));
    }
    let e = json!({"schemaVersion": 1, "at": now_iso(), "claim": 5981079530i64,
        "status": "deployed_single_collector_native_paper_host",
        "cohort": n["cohort"], "activeHash": n["activeHash"], "hostSourceHashes": prep["hostSourceHashes"],
        "declarationHash": n["declarationHash"], "engineHash": n["engineHash"], "binaryHash": n["binaryHash"],
        "source": {"hash": n["sourceHash"], "bytes": n["sourceBytes"], "records": n["sourceRecords"], "through": n["through"]},
        "candidates": n["candidates"], "uniqueFillEvents": n["uniqueFillEvents"], "cardsWithFills": n["cardsWithFills"],
        "groups": n["groups"], "heldMarketIds": n["heldMarketIds"], "lastWallMs": n["lastWallMs"],
        "verifiedAt": n["verifiedAt"], "checks": {"passed": 96, "failed": 0, "skipped": 0},
        "services": {"collector": pids["collector"], "paper": pids["paper"], "dashboard": pids["dashboard"],
            "persistent": 3, "extra": 0, "status": s["runtime"]["status"], "cycles": s["runtime"]["cycles"]},
        "readAllocation": 20, "executionOwner": "Daenerys", "fees": 0, "networkRequestsAddedByReplay": 0,
        "contestActivation": false, "competitionOrders": 0, "parametersTuned": false, "walletsReset": false,
        "frozenBanksPreserved": true, "originalJournalsPreserved": true, "walletResultsAreCorrelated": true,
        "profitabilityEstablished": false,
        "limitations": ["Native replay is local counterfactual paper trading",
            "One correlated dip round trip lost 0.010 per active wallet; copied wallets do not add independent evidence",
            "The first close happened before this integration was deployed",
            "Known held risk is retained across replay failures; pending buys are not yet per-instrument risk hints",
            "Read budget is unchanged; over-capacity risk can still outstrip freshness",
            "API collector and Next frontend remain Node; Rust executes capture verification and comparisons"]});
    atomic_json(&root.join("data/native-dip-host-milestone-20261004.json"), &e)?;
    let mut q = read(&root.join("data/quant-development-state.json"))?;
    q["updatedAt"] = e["at"].clone();
    q["nativeDipHostMilestone"] = e.clone();
    q["activeProcesses"] = json!({"collector": pids["collector"], "probabilityWorker": pids["paper"],
        "dashboard": pids["dashboard"], "persistent": 3, "extra": 0});
    q["goalTurn"] = json!({"at": e["at"], "classification": "progress", "blockedAuditCount": 0,
        "materialChange": "Frozen native cohort is now refreshed automatically within the sole collector; independently reconciled holdings join its existing scheduler without new API clients or services. First unique subsequent dip round trip closed at -0.010, not profit."});
    q["next"] = json!(["Keep native host healthy and continue frozen subsequent observations without refit or accounting reset; investigate net entry edges against two-sided costs rather than counting copied fills as scale.",
        "Reduce original-capture parsing work through a verified compact native cache or incremental native state; preserve exact original source and clocks.",
        "Add per-instrument pending-intent risk hints to a versioned native output without altering active cohort fills.",
        "NH paired-paper review expires at 15:36:07.104 UTC; do not silently renew research evidence.",
        "Daenerys remains official execution owner; no activation or official order change."]);
    atomic_json(&root.join("data/quant-development-state.json"), &q)?;
    let out = json!({"status": e["status"], "checks": e["checks"], "services": e["services"],
        "candidates": e["candidates"], "uniqueFillEvents": e["uniqueFillEvents"], "held": e["heldMarketIds"],
        "groups": e["groups"], "lastWallMs": e["lastWallMs"], "competitionOrders": 0});
    println!("{}", serde_json::to_string(&out).unwrap_or_default());
    Ok(e)
}
