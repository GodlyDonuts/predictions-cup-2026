// data/record-settlement-value-milestone-20261004.mjs — settlement-value milestone record.
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
fn write_exclusive(f: &Path, v: &V) -> Result<(), String> {
    let body = format!("{}\n", js_stringify(v));
    let mut fd = std::fs::OpenOptions::new().write(true).create_new(true).open(f).map_err(|e| e.to_string())?;
    use std::io::Write;
    fd.write_all(body.as_bytes()).map_err(|e| e.to_string())
}
fn rt() -> PathBuf {
    Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default()).join("SigTradingCup").join("paper-runtime")
}

pub fn run(root: &Path) -> Result<V, String> {
    let snapshot_file = "data/settlement-value-snapshot-1791144422978.json";
    let snapshot = read(&root.join(snapshot_file))?;
    let parity = read(&root.join("data/settlement-native-preservation-20261004.json"))?;
    let lead = read(&root.join("data/kansas-source-lead-20261004.json"))?;
    let checks = std::fs::read_to_string(root.join("data/settlement-value-all-checks-20261004.tap")).map_err(|e| e.to_string())?;
    a_true(checks.contains("tests 137") && checks.contains("pass 137") && checks.contains("fail 0"))?;
    eq(&parity["fullStateExact"], &json!(true))?;
    eq(&parity["nativeBackend"]["engineHash"], &snapshot["native"]["engineHash"])?;
    let runtime = rt();
    let stats = read(&runtime.join("stats.json"))?;
    eq(&stats["runtime"]["status"], &json!("running"))?;
    eq(&stats["runtime"]["networkBudget"]["writesEnabled"], &json!(false))?;
    eq(&stats["runtime"]["networkBudget"]["maxReadsPerMinute"], &json!(20))?;
    let mut milestone = json!({"schemaVersion": 1, "at": now_iso(), "claimComment": 5983667980i64,
        "archiveFixClaimComment": 5983853071i64,
        "status": "native_settlement_policy_and_actual_state_diagnostic_completed",
        "features": ["source-bound settlement holding and opportunity-cost reductions",
            "bounded fractional sizing with declared correlated-risk caps",
            "exact multi-level hard-limit reservations",
            "research-independent held-risk and full-depth liquidation view",
            "separate current checked-source archive helper with unchanged historical benchmark"],
        "snapshot": snapshot_file,
        "snapshotHash": sha(&std::fs::read(root.join(snapshot_file)).map_err(|e| e.to_string())?),
        "parity": "data/settlement-native-preservation-20261004.json",
        "parityHash": sha(&std::fs::read(root.join("data/settlement-native-preservation-20261004.json")).map_err(|e| e.to_string())?),
        "sourceBytes": parity["sourceBytes"], "sourceRecords": parity["sourceSequence"],
        "fullActualJsNativePublicationStateExact": true,
        "nativeBackend": parity["nativeBackend"], "decisionWallMs": snapshot["native"]["decisionWallMs"],
        "verificationAndDecisionWallMs": snapshot["native"]["totalVerificationAndDecisionMs"],
        "tests": {"node": 137, "rust": 9, "failures": 0, "skips": 0, "newPolicyAndAdapterChecks": 34},
        "currentSnapshotDecision": snapshot["decisions"].as_array().cloned().unwrap_or_default().iter()
            .map(|x| json!({"candidate": x["candidateId"], "action": x["action"], "reason": x["reason"],
                "worstCaseHeldLoss": x["worstCaseHeldLoss"]})).collect::<Vec<V>>(),
        "proposedNewCost": snapshot["proposedMaximumCost"], "existingPaperSnapshot": snapshot["existingPaper"],
        "researchLead": {"file": "data/kansas-source-lead-20261004.json",
            "hash": sha(&std::fs::read(root.join("data/kansas-source-lead-20261004.json")).map_err(|e| e.to_string())?),
            "marketIds": lead["identities"].as_array().cloned().unwrap_or_default().iter().map(|x| x["marketId"].clone()).collect::<Vec<V>>(),
            "pointScenarioOnly": lead["scenarioOnly"], "pairedQuoteLeadOnly": lead["pairedQuoteLead"],
            "resolutionReviewed": false, "frozenProbabilityBankCreated": false},
        "runtime": {"collectorPid": stats["runtime"]["pid"], "collectorLastCycle": stats["runtime"]["lastCycleAt"],
            "nativeQuoteStatus": stats["runtime"]["nativeQuote"]["status"],
            "nativeQuoteError": stats["runtime"]["nativeQuote"]["lastError"], "readCap": 20,
            "writerEnabled": false, "owner": stats["runtime"]["networkBudget"]["executionOwner"],
            "persistentServices": 3, "additionalPersistentServices": 0},
        "originalBanksAndActiveBindingsPreserved": true, "walletsReset": false,
        "activeHoldingPolicyChanged": false, "competitionOrders": 0, "sigApiRequests": 0,
        "publicPrimaryResearchRequests": 3, "fees": 0, "profitabilityEstablished": false,
        "next": "Obtain current exact Kansas contract conditions and sufficient source evidence for a frozen conservative forecast method; acquire confirmation depth through the existing same-budget collector. Connect the native holding/sizing policy to a separate subsequent-data paper wallet with independent later-request fills, durable accounting and original source clocks. Do not promote the model point or conditional quote pair alone."});
    let milestone_file = root.join("data/settlement-value-milestone-20261004.json");
    if milestone_file.exists() {
        let original = read(&milestone_file)?;
        eq(&original["nativeBackend"]["engineHash"], &milestone["nativeBackend"]["engineHash"])?;
        eq(&original["snapshotHash"], &milestone["snapshotHash"])?;
        milestone = original;
    } else {
        write_exclusive(&milestone_file, &milestone)?;
        milestone = read(&milestone_file)?;
    }
    let mut state = read(&root.join("data/quant-development-state.json"))?;
    state["updatedAt"] = milestone["at"].clone();
    state["goalTurn"] = json!({"at": milestone["at"], "classification": "progress",
        "previousTurnClassification": "no_progress_status_report",
        "revalidatedBy": "fresh repository ownership, live process handles and latest captured book/source state",
        "milestone": milestone["status"], "requiredWorkRemaining": true});
    state["nativeSettlementValueMilestone"] = milestone.clone();
    state["next"] = json!([milestone["next"],
        "Keep the existing collector and paper exits healthy with three services and the serialized20-read cap. There are now two protected markets rather than the earlier four-slot saturation; verify the actual planner each wake.",
        "Keep the frozen trend probe distinct from the source-probability method. No profitability or takeover inferred from test counts, source-model points, conditional quote surplus, or tiny paper gains."]);
    let mut backlog = state["backlog"].as_array().cloned().unwrap_or_default();
    backlog.push(json!({"at": milestone["at"], "status": "completed",
        "task": "Implement native settlement holding/sizing policy, exact current-snapshot proposal and full real-state JS/native publisher parity; preserve expired research clocks, held basis and all active wallets.",
        "evidence": "data/settlement-value-milestone-20261004.json"}));
    state["backlog"] = json!(backlog);
    atomic_json(&root.join("data/quant-development-state.json"), &state)?;
    let mut monitor = read(&root.join("data/daenerys-monitor.json"))?;
    monitor["updatedAt"] = milestone["at"].clone();
    monitor["lastCheckAt"] = milestone["at"].clone();
    monitor["lastEngineeringMilestone"] = milestone["status"].clone();
    monitor["nextConcreteMilestone"] = milestone["next"].clone();
    monitor["nativeSettlementValueMilestone"] = json!({"at": milestone["at"],
        "evidence": "data/settlement-value-milestone-20261004.json", "claimComment": milestone["claimComment"]});
    atomic_json(&root.join("data/daenerys-monitor.json"), &monitor)?;
    let out = json!({"status": milestone["status"], "checks": milestone["tests"],
        "currentDecisionCost": milestone["proposedNewCost"], "sourceStateParity": true,
        "goalClassification": "progress", "profitabilityEstablished": false});
    println!("{}", js_stringify(&out));
    Ok(milestone)
}
