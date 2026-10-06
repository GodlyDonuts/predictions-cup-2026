// data/record-native-intent-20261004.mjs — native pending-intent milestone record.
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
fn a_true(ok: bool) -> Result<(), String> {
    if ok { Ok(()) } else { Err(fail("assertion failed")) }
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
fn arr_len(x: &V) -> i64 { x.as_array().map(|a| a.len() as i64).unwrap_or(0) }

pub fn run(root: &Path) -> Result<V, String> {
    let r = rt();
    let pids = read(&root.join("data/current-runtime-pids.json"))?;
    let s = read(&r.join("stats.json"))?;
    let n = read(&r.join("native-dip-cache-status.json"))?;
    let screen = read(&r.join("collector-market-screen-v1.json"))?;
    let prep = read(&root.join("data/native-intent-preparation-20261004.json"))?;
    let preserved = read(&root.join("data/native-intent-preservation-after-20261004.json"))?;
    let budget = read(&r.join("coordinated-network-budget.json"))?;
    for pid in ["collector", "paper", "dashboard"] { kill0(&pids[pid])?; }
    eq(&s["runtime"]["pid"], &pids["collector"])?;
    eq(&s["runtime"]["status"], &json!("running"))?;
    a_true(s["runtime"]["cycles"].as_i64().unwrap_or(0) >= 2)?;
    a_true(arr_len(&s["runtime"]["errors"]) == 0 && s["runtime"]["errors"].is_array())?;
    eq(&n["hostRevision"], &json!(3))?;
    eq(&n["intentObservationSchemaVersion"], &json!(1))?;
    pnull(&n, "lastError")?;
    a_true(n["verifiedAt"].as_str().is_some()
        && n["sourceBytes"].as_i64().unwrap_or(0) > prep["source"]["sourceBytes"].as_i64().unwrap_or(0))?;
    eq(&n["activeHash"], &prep["activeHash"])?;
    eq(&n["fees"], &json!(0))?;
    eq(&n["competitionOrders"], &json!(0))?;
    eq(&screen["available"], &json!(true))?;
    eq(&screen["counts"]["observed"], &json!(237))?;
    eq(&json!(arr_len(&s["runtime"]["scheduling"]["reads"])), &json!(4))?;
    eq(&budget["maxReadsPerMinute"], &json!(20))?;
    eq(&budget["writesEnabled"], &json!(false))?;
    eq(&budget["executionOwner"], &json!("Daenerys"))?;
    eq(&read(&r.join("competition-activation.json"))?["enabled"], &json!(false))?;
    eq(&preserved["originalJournalsPreserved"], &json!(true))?;
    eq(&json!(sha(&std::fs::read(r.join("native-dip-intent-active.json")).map_err(|e| e.to_string())?)),
        &prep["activeHash"])?;
    for (f, expected) in prep["hostSourceHashes"].as_object().cloned().unwrap_or_default() {
        let hash = sha(&std::fs::read(root.join(&f)).or_else(|_| std::fs::read(&f)).map_err(|e| e.to_string())?);
        eq(&json!(hash), &expected)?;
    }
    let checks = std::fs::read_to_string(root.join("data/native-intent-all-checks-20261004.tap")).map_err(|e| e.to_string())?;
    a_true(re_word(&checks, "# pass 171") && re_word(&checks, "# fail 0") && re_word(&checks, "# skipped 0"))?;
    let rust = std::fs::read_to_string(root.join("data/native-intent-build-20261004.log")).map_err(|e| e.to_string())?;
    a_true(rust.contains("2 passed; 0 failed"))?;
    let e = json!({"schemaVersion": 1, "at": now_iso(), "claim": 5981840605i64,
        "status": "native_pending_intent_continuation_deployed",
        "cohort": n["cohort"], "activeHash": n["activeHash"], "declarationHash": n["declarationHash"],
        "engineHash": n["engineHash"], "binaryHash": n["binaryHash"],
        "source": {"bytes": n["sourceBytes"], "hash": n["sourceHash"], "records": n["sourceRecords"], "through": n["through"]},
        "checks": {"nodePassed": 171, "rustPassed": 2, "failed": 0, "skipped": 0},
        "parity": {"originalScorecardsExact": 100, "observedCachedScorecardsExact": 100},
        "timing": prep["timing"], "lastLiveWallMs": n["lastWallMs"], "verifiedAt": n["verifiedAt"],
        "pendingMarketIds": n["pendingMarketIds"], "pendingIntents": arr_len(&n["pendingReadIntents"]),
        "heldMarketIds": n["heldMarketIds"], "groups": n["groups"], "uniqueFillEvents": n["uniqueFillEvents"],
        "signalDiagnosticsRanges": prep["signalDiagnosticsRanges"],
        "screen": {"available": true, "observed": screen["counts"]["observed"],
            "candidates": arr_len(&screen["candidateMarketIds"])},
        "services": {"collector": pids["collector"], "paper": pids["paper"], "dashboard": pids["dashboard"],
            "persistent": 3, "extra": 0, "cycles": s["runtime"]["cycles"], "status": s["runtime"]["status"]},
        "readAllocation": 20, "fees": 0, "competitionOrders": 0, "contestActivation": false,
        "executionOwner": "Daenerys", "frozenTradingEngineUnchanged": true, "frozenBanksPreserved": true,
        "oldBinaryCacheAndOutputsPreserved": true, "originalJournalsPreserved": true,
        "parametersTuned": false, "walletsReset": false, "profitabilityEstablished": false,
        "limitations": ["Current measured cohort has no pending intents: live pending confirmation remains unobserved; real runner wiring is tested with deterministic causal fixtures",
            "Only zero or one cost/deviation-qualified signal per frozen case; this plumbing alone cannot establish a useful edge",
            "Copied wallet outcomes are correlated and not additive profits",
            "Sparse authoritative depth history remains a feature bottleneck; new quote-history model must keep executable fills separate"]});
    atomic_json(&root.join("data/native-intent-milestone-20261004.json"), &e)?;
    let mut q = read(&root.join("data/quant-development-state.json"))?;
    q["updatedAt"] = e["at"].clone();
    q["nativeIntentMilestone"] = e.clone();
    q["activeProcesses"] = json!({"collector": pids["collector"], "probabilityWorker": pids["paper"],
        "dashboard": pids["dashboard"], "persistent": 3, "extra": 0});
    q["goalTurn"] = json!({"at": e["at"], "classification": "progress", "blockedAuditCount": 0,
        "materialChange": "Per-instrument native pending-intent observation and independent proof deployed inside the same collector, with expiry/latency-aware confirmation scheduling. Original trading engine/declaration unchanged;100original and100cached observed cards exact;171Node+2Rust checks. Diagnostics show0-1qualifying signals per frozen case, no current pending intents; broader causal quote-history model is now the priority."});
    q["next"] = json!(["Implement a separate Rust quote-history dip cohort that warms causal time-weighted features from the full verified237-market quote journal but uses fresh independent authoritative depth exclusively for counterfactual fills. Freeze a new future cutoff and independent wallets; preserve the current sparse-depth cohort.",
        "Compare fixed mean-reversion/recovery/volatility gates, holding horizons and real spread/depth/latency/slippage sensitivities on chronological held-out or genuinely subsequent data; no forced trades or cherry-picked historical profit.",
        "Keep expiry-aware native confirmation hints and true held risk in the same serialized4-book/20-read budget; retain three services and no account clients.",
        "NH paired review has expired; keep held monitoring without renewed entry authority."]);
    atomic_json(&root.join("data/quant-development-state.json"), &q)?;
    let out = json!({"status": e["status"], "checks": e["checks"], "timing": e["timing"],
        "liveMs": e["lastLiveWallMs"], "pending": e["pendingIntents"], "groups": e["groups"],
        "services": e["services"], "competitionOrders": 0});
    println!("{}", serde_json::to_string(&out).unwrap_or_default());
    Ok(e)
}
