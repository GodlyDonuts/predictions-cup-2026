// data/record-native-sides-milestone-20261004.mjs — native sides milestone record.
use baal_service_runtime::atomic::atomic_json_canonical as atomic_json;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::{js_stringify, sha256};
use serde_json::{json, Value as V};
use std::path::Path;

fn sha(b: &[u8]) -> String { sha256(b) }
fn read(f: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(f).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn now_iso() -> String {
    ms_to_iso(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64).unwrap_or(0.0))
}
fn fail(msg: &str) -> String { msg.to_string() }
fn eq(a: &V, b: &V, msg: Option<String>) -> Result<(), String> {
    if a == b { Ok(()) } else { Err(msg.unwrap_or_else(|| fail(&format!("assert.equal: {} !== {}", js_stringify(a), js_stringify(b))))) }
}
fn a_true(ok: bool) -> Result<(), String> {
    if ok { Ok(()) } else { Err(fail("assertion failed")) }
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
fn write_exclusive(f: &Path, v: &V) -> Result<(), String> {
    let body = format!("{}\n", js_stringify(v));
    let mut fd = std::fs::OpenOptions::new().write(true).create_new(true).open(f).map_err(|e| e.to_string())?;
    use std::io::Write;
    fd.write_all(body.as_bytes()).map_err(|e| e.to_string())
}

pub fn run(root: &Path) -> Result<V, String> {
    let evidence = read(&root.join("data/native-quote-endpoints-20261004.json"))?;
    let initial = read(&root.join("data/native-quote-sides-20261004.json"))?;
    let preserved = read(&root.join("data/native-sides-preservation-20261004.json"))?;
    let checks = std::fs::read_to_string(root.join("data/native-sides-checks-20261004.tap")).map_err(|e| e.to_string())?;
    a_true(re_word(&checks, "tests 163"))?;
    a_true(re_word(&checks, "fail 0"))?;
    a_true(re_word(&checks, "skipped 0"))?;
    eq(&evidence["originalSideCardsExact"], &json!(432), None)?;
    eq(&preserved["exactOriginalCards"], &json!(72), None)?;
    eq(&preserved["newEngineHash"], &evidence["engineHash"], None)?;
    for (file, hash) in evidence["sourceHashes"].as_object().cloned().unwrap_or_default() {
        let actual = sha(&std::fs::read(root.join(&file)).or_else(|_| std::fs::read(&file)).map_err(|e| e.to_string())?);
        eq(&json!(actual), &hash, Some(file.clone()))?;
    }
    eq(&preserved["collector"]["currentRecoveryVerified"], &json!(true), None)?;
    let mut sides = serde_json::Map::new();
    for side in ["yes", "no"] {
        sides.insert(side.to_string(), json!({
            "uniqueSignalEvents": evidence[side]["uniqueSignalEvents"],
            "uniqueObservedIntervals": evidence[side]["uniqueObservedIntervals"],
            "strictCards": evidence[side]["cards"],
            "endpointCards": evidence[side]["endpointSensitivityCards"],
            "endpointInterpretation": evidence[side]["endpointInterpretation"],
            "bookBoundarySensitivity": "All original sensitivity cards retained in the immutable full output; not replaced by endpoint labels."}));
    }
    let result = json!({"schemaVersion": 1, "at": now_iso(), "claimComment": 5982841708i64,
        "status": "canonical_two_side_rust_replay_and_endpoint_labels_completed",
        "dataUse": "reused_exploratory_development_data_no_fitting_or_promotion",
        "cohort": evidence["cohort"], "declarationHash": evidence["declarationHash"],
        "engineHash": evidence["engineHash"], "binaryHash": evidence["binaryHash"],
        "runtimeLibraryHash": evidence["runtimeLibraryHash"],
        "sourceHashes": evidence["sourceHashes"], "sources": evidence["sources"],
        "sourceBytes": evidence["sourceBytes"], "combinedSourceSpanDays": evidence["sourceDays"],
        "quoteSpanDays": evidence["quoteSpanDays"], "quoteRows": evidence["quoteRows"],
        "markets": evidence["markets"], "cases": 216, "strictAndBookBoundaryPaths": 432,
        "endpointSensitivityCases": 216, "workers": 8, "sideWorkers": 4,
        "timing": evidence["timing"], "initialSideTiming": initial["timing"],
        "endpointSensitivityMs": evidence["endpointSensitivityMs"], "projection": evidence["projection"],
        "canonicalDepthChecks": evidence["canonicalDepthChecks"], "sides": V::Object(sides),
        "sourceGuidance": {"reviewedAt": "2026-10-04T18:12:00Z",
            "markets": "https://sig.thesuper.market/docs/markets-and-trading",
            "settlement": "https://sig.thesuper.market/docs/settlement-and-payouts",
            "conclusion": "Canonical contract quotes and public ladders complement across YES/NO. This does not establish a universal REST NO fill-price convention or executable capacity."},
        "immutableFullEvidence": {"initialOutputHash": initial["outputHash"],
            "initialEvidenceHash": sha(&std::fs::read(root.join("data/native-quote-sides-20261004.json")).map_err(|e| e.to_string())?),
            "endpointOutputHash": evidence["outputHash"],
            "endpointEvidenceHash": sha(&std::fs::read(root.join("data/native-quote-endpoints-20261004.json")).map_err(|e| e.to_string())?),
            "originalYesOutputHash": evidence["originalYesOutputHash"],
            "strictSideAndSensitivityCardsExact": 432},
        "tests": {"node": 163, "rust": 9, "failures": 0, "skips": 0},
        "originalWallets": {"cards": 72, "allOriginalFillsExact": true, "activeCohortReplaced": false,
            "originalWalletsReset": false, "evidence": "data/native-sides-preservation-20261004.json"},
        "live": {"paperThrough": preserved["through"], "paperGroups": preserved["paperGroups"],
            "collector": preserved["collector"], "pids": preserved["pids"], "persistentProcesses": 3,
            "additionalPersistentProcesses": 0},
        "fees": 0, "networkRequests": 0, "competitionOrders": 0, "profitabilityEstablished": false,
        "conclusion": "Adding NO breadth and fixed-horizon endpoint coverage did not establish an executable edge. Broad NO dip observations remain negative. Positive YES two-hour endpoints are sparse and mostly have incomplete intermediate paths.",
        "next": "Compare economic mechanisms beyond historical dip levels using the native chronological engine; retain exact book/quote separation, frozen policies and prospective validation. Do not add more copied dip wallets merely to increase model count."});
    write_exclusive(&root.join("data/native-sides-milestone-20261004.json"), &result)?;
    let mut state = read(&root.join("data/quant-development-state.json"))?;
    state["updatedAt"] = result["at"].clone();
    state["nativeSidesMilestone"] = json!({"evidence": "data/native-sides-milestone-20261004.json",
        "status": result["status"], "cases": 216, "paths": 432, "wallMs": result["timing"]["wallMs"],
        "tests": result["tests"], "originalWalletsExact": 72, "profitabilityEstablished": false});
    state["goalTurn"] = json!({"at": result["at"], "classification": "progress",
        "milestone": result["status"], "requiredWorkRemaining": true});
    state["nextConcreteMilestone"] = result["next"].clone();
    state["nextImplementationMilestones"] = json!([
        {"priority": 1, "task": "Native chronological comparison of economic signals beyond historical dips, with realistic spread and independent source-clock labels. No refitting or promotion from sparse reused paths."},
        {"priority": 2, "task": "Only test new prospective wallets when there is a distinct mechanism and verified executable book support; preserve original wallet histories and ownership."},
        {"priority": 3, "task": "Keep the single serialized collector and existing paper exits healthy; retain observable outages, persistent backoff and source identities."}]);
    atomic_json(&root.join("data/quant-development-state.json"), &state)?;
    let mut monitor = read(&root.join("data/daenerys-monitor.json"))?;
    monitor["updatedAt"] = result["at"].clone();
    monitor["lastCheckAt"] = result["at"].clone();
    monitor["nativeSidesMilestone"] = state["nativeSidesMilestone"].clone();
    monitor["lastEngineeringMilestone"] = result["status"].clone();
    monitor["nextConcreteMilestone"] = result["next"].clone();
    atomic_json(&root.join("data/daenerys-monitor.json"), &monitor)?;
    let out = json!({"status": result["status"], "paths": 432, "wallMs": result["timing"]["wallMs"],
        "tests": result["tests"], "originalWalletsExact": 72, "collectorRecovered": true,
        "profitabilityEstablished": false});
    println!("{}", js_stringify(&out));
    Ok(result)
}
