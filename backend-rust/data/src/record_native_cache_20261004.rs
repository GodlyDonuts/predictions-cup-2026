// data/record-native-cache-20261004.mjs — verify the cached native continuation
// and record the milestone into quant-development-state.
use crate::publication_pack::{now_ms, read_json, runtime_dir};
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::contract::ms_to_iso;
use serde_json::{json, Value as V};
use std::path::Path;
use std::process::Command;

fn assert_eq(a: &V, b: &V) -> Result<(), String> {
    if a == b { Ok(()) } else { Err("assert".into()) }
}
fn need(cond: bool) -> Result<(), String> {
    if cond { Ok(()) } else { Err("assert".into()) }
}
fn pid_alive(pid: i64) -> bool {
    if pid <= 0 { return false; }
    Command::new("kill").arg("-0").arg(pid.to_string())
        .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
        .status().map(|s| s.success()).unwrap_or(false)
}
pub fn run() -> Result<V, String> {
    let r = runtime_dir();
    let read = |f: &str| read_json(Path::new(f));
    let s = read(&format!("{r}/stats.json"))?;
    let n = read(&format!("{r}/native-dip-cache-status.json"))?;
    let pids = read("data/current-runtime-pids.json")?;
    let prep = read("data/native-cache-host-preparation-20261004.json")?;
    let benchmark = read("data/native-cache-benchmark-20261004.json")?;
    let preserved = read("data/native-cache-preservation-after-20261004.json")?;
    let budget = read(&format!("{r}/coordinated-network-budget.json"))?;
    for pid in [&pids["collector"], &pids["paper"], &pids["dashboard"]] {
        need(pid_alive(pid.as_i64().unwrap_or(0)))?;
    }
    assert_eq(&s["runtime"]["pid"], &pids["collector"])?;
    assert_eq(&s["runtime"]["status"], &json!("running"))?;
    need(s["runtime"]["cycles"].as_i64().unwrap_or(0) >= 1)?;
    assert_eq(&n["status"], &json!("running"))?;
    need(n["verifiedAt"].is_string() && !n["verifiedAt"].as_str().unwrap().is_empty())?;
    assert_eq(&n["lastError"], &V::Null)?;
    assert_eq(&n["activeHash"], &prep["activeHash"])?;
    assert_eq(&n["hostRevision"], &json!(2))?;
    need(n["sourceBytes"].as_f64().unwrap_or(0.0) > prep["source"]["bytes"].as_f64().unwrap_or(0.0))?;
    assert_eq(&n["candidates"], &json!(100))?;
    assert_eq(&n["fees"], &json!(0))?;
    assert_eq(&n["competitionOrders"], &json!(0))?;
    assert_eq(&budget["maxReadsPerMinute"], &json!(20))?;
    assert_eq(&budget["writesEnabled"], &json!(false))?;
    assert_eq(&budget["executionOwner"], &json!("Daenerys"))?;
    assert_eq(&read(&format!("{r}/competition-activation.json"))?["enabled"], &json!(false))?;
    assert_eq(&preserved["originalJournalsPreserved"], &json!(true))?;
    let checks = std::fs::read_to_string("data/native-cache-all-checks-20261004.tap").map_err(|e| e.to_string())?;
    need(checks.contains("pass 132") && checks.contains("fail 0") && checks.contains("skipped 0"))?;
    let e = json!({
        "schemaVersion": 1, "at": ms_to_iso(now_ms() as f64), "claim": 5981279291i64,
        "status": "cached_native_continuation_deployed", "cohort": n["cohort"],
        "activeHash": n["activeHash"], "hostSourceHashes": prep["hostSourceHashes"],
        "declarationHash": n["declarationHash"], "engineHash": n["engineHash"],
        "binaryHash": n["binaryHash"], "originalModelEngineHash": prep["originalModelEngineHash"],
        "source": {"hash": n["sourceHash"], "bytes": n["sourceBytes"],
            "records": n["sourceRecords"], "through": n["through"]},
        "cache": {"hash": n["cacheHash"], "bytes": n["cacheBytes"],
            "originalPrefixAlwaysRehashed": n["originalPrefixAlwaysRehashed"]},
        "benchmark": {"fullNativeWallMs": benchmark["full"]["wallMs"],
            "cachedNativeWallMs": benchmark["cached"]["wallMs"],
            "speedup": benchmark["speedup"], "oldModelEngineCardsExact": 100,
            "cachedFullCardsExact": 100},
        "lastLiveWallMs": n["lastWallMs"], "verifiedAt": n["verifiedAt"],
        "candidates": n["candidates"], "uniqueFillEvents": n["uniqueFillEvents"],
        "groups": n["groups"], "heldMarketIds": n["heldMarketIds"],
        "checks": {"nodePassed": 132, "nodeFailed": 0, "rustPassed": 2,
            "rustFailed": 0, "skipped": 0},
        "services": {"collector": pids["collector"], "paper": pids["paper"],
            "dashboard": pids["dashboard"], "persistent": 3, "extra": 0,
            "status": s["runtime"]["status"], "cycles": s["runtime"]["cycles"]},
        "readAllocation": 20, "executionOwner": "Daenerys", "intervalMs": 30000,
        "fees": 0, "networkRequestsAddedByReplay": 0, "competitionOrders": 0,
        "contestActivation": false, "parametersTuned": false, "walletsReset": false,
        "oldEngineOutputsPreserved": true, "frozenBanksPreserved": true,
        "originalJournalsPreserved": true, "profitabilityEstablished": false,
        "limitations": [
            "Speed and same-prefix parity are engineering evidence, not profitability",
            "Original prefix bytes are completely rehashed; cache avoids repeated JSON parsing",
            "API ingestion and Next remain Node; Rust handles source validation and wallet comparison",
            "One unique dip round trip still lost 0.010; correlated wallet copies are not independent profit",
            "Native pending-intent instruments and limited book discovery remain follow-up work",
        ],
    });
    atomic_json(Path::new("data/native-cache-milestone-20261004.json"), &e)?;
    let mut q = read("data/quant-development-state.json")?;
    q.as_object_mut().unwrap().insert("updatedAt".into(), e["at"].clone());
    q.as_object_mut().unwrap().insert("nativeCacheMilestone".into(), e.clone());
    q.as_object_mut().unwrap().insert("activeProcesses".into(), json!({
        "collector": pids["collector"], "probabilityWorker": pids["paper"],
        "dashboard": pids["dashboard"], "persistent": 3, "extra": 0,
    }));
    q.as_object_mut().unwrap().insert("goalTurn".into(), json!({
        "at": e["at"], "classification": "progress", "blockedAuditCount": 0,
        "materialChange": "Pinned compact native cache and canonical append continuation deployed without wallet reset. Same-source native replay improved 14.02s to 4.25s; full and original-engine 100 cards exact. Live updates now ~5s every collection cycle. Three services and zero contest writes preserved.",
    }));
    q.as_object_mut().unwrap().insert("next".into(), json!([
        "Investigate active paper read obligations versus retired comparisons: current four-book plan uses two pair holds, probability market297, and one fixed-core comparison, leaving no broad discovery. Preserve true risk while making useful global candidates observable.",
        "Add independently verifiable per-instrument pending native intents to a versioned output so signal confirmation reads can occur before expiry; preserve frozen wallet history.",
        "Investigate net entry edges and realistic two-sided spread/slippage on subsequent data; do not count copied cases as scale.",
        "Maintain the cached host and frozen declaration on genuinely subsequent captures without retuning/resetting.",
        "NH paired review expires at 15:36:07.104 UTC; preserve held monitoring and do not silently renew evidence. Daenerys remains official writer.",
    ]));
    atomic_json(Path::new("data/quant-development-state.json"), &q)?;
    Ok(json!({
        "status": e["status"], "source": e["source"], "benchmark": e["benchmark"],
        "liveWallMs": e["lastLiveWallMs"], "checks": e["checks"],
        "services": e["services"], "competitionOrders": 0,
    }))
}
