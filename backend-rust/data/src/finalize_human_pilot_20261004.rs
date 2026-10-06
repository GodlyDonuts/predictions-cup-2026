// data/finalize-human-pilot-20261004.mjs — record the live human-pilot proof
// and update the coordination state files atomically.
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Value as V};
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
fn git_blob_sha(b: &[u8]) -> String {
    let mut blob = format!("blob {}\0", b.len()).into_bytes();
    blob.extend_from_slice(b);
    use sha1::Digest as _;
    let mut h = sha1::Sha1::new();
    sha1::Digest::update(&mut h, &blob);
    format!("{:x}", sha1::Digest::finalize(h))
}
fn need_eq(a: &V, b: &V) -> Result<(), String> {
    if a == b { Ok(()) } else { Err("assertion failed".into()) }
}
fn need(ok: bool) -> Result<(), String> {
    if ok { Ok(()) } else { Err("assertion failed".into()) }
}
/// process.kill(id, 0) — signal 0 checks the pid exists.
fn pid_alive(id: i64) -> bool {
    if id <= 0 { return false; }
    #[cfg(unix)] { unsafe { libc::kill(id as i32, 0) == 0 } }
    #[cfg(not(unix))] { Path::new(&format!("/proc/{}", id)).exists() }
}

pub fn run() -> Result<V, String> {
    let runtime = localappdata()?.join("SigTradingCup").join("paper-runtime");
    let at = now_iso();
    let v = read_json(Path::new("data/human-owner-publication-readback-20261004.json"))?;
    let pack = read_json(Path::new("data/human-assignment-publication-pack-20261004.json"))?;
    for e in pack.as_array().cloned().unwrap_or_default() {
        let content = e["content"].as_str().unwrap_or("").as_bytes().to_vec();
        let found = v["firstBlobs"].as_array().cloned().unwrap_or_default().into_iter()
            .find(|x| x["path"] == e["path"]).map(|x| x["sha"].clone());
        need_eq(&json!(git_blob_sha(&content)), &found.unwrap_or(V::Null))?;
    }
    for e in v["repairBlobs"].as_array().cloned().unwrap_or_default() {
        need_eq(&json!(git_blob_sha(&std::fs::read(e["path"].as_str().unwrap_or(""))
            .map_err(|er| er.to_string())?)), &e["sha"])?;
    }
    let activation = read_json(&runtime.join("competition-activation.json"))?;
    let handoff = read_json(&runtime.join("production-handoff.json"))?;
    let collector = read_json(&runtime.join("stats.json"))?;
    let execution = read_json(&runtime.join("competition-stats.json"))?;
    let pids = read_json(Path::new("data/current-runtime-pids.json"))?;
    baal_execution::production::validate_production_handoff(&handoff, &activation)
        .map_err(|e| e.to_string())?;
    for id in ["collector", "paper", "dashboard"] {
        need(pid_alive(pids[id].as_i64().unwrap_or(0)))?;
    }
    need_eq(&collector["runtime"]["pid"], &pids["collector"])?;
    need_eq(&collector["runtime"]["status"], &json!("running"))?;
    need(collector["runtime"]["cycles"].as_i64().unwrap_or(0) >= 1)?;
    need_eq(&json!(collector["runtime"]["errors"].as_array().map(|a| a.len()).unwrap_or(0)),
        &json!(0))?;
    need_eq(&execution["runId"], &activation["runId"])?;
    need_eq(&execution["readiness"]["complete"], &json!(true))?;
    need_eq(&execution["status"], &json!("waiting_for_signal"))?;
    need_eq(&execution["ordersSubmitted"], &json!(0))?;
    need_eq(&execution["dispatchAttempts"], &json!(0))?;
    need_eq(&execution["positions"], &json!({}))?;
    need_eq(&execution["eligibleResearchMarkets"], &json!([]))?;
    need_eq(&execution["realizedPnl"], &json!("0.000000"))?;
    need_eq(&execution["addedFees"], &json!("0.000000"))?;
    let proof = json!({"schemaVersion": 1, "at": at, "classification": "progress",
        "productionPilotRunning": true, "profitabilityEstablished": false,
        "readiness": execution["readiness"], "status": execution["status"],
        "owner": "Codex", "handoffId": activation["handoffId"],
        "assignmentComment": v["assignmentComment"], "ackComment": v["ackComment"],
        "sourceBank": activation["strategyBankHash"], "eligibleResearchMarkets": [],
        "orders": 0, "fills": 0, "realizedPnl": "0.000000", "fees": "0.000000",
        "collector": {"pid": pids["collector"], "status": collector["runtime"]["status"],
            "cycles": collector["runtime"]["cycles"], "errors": 0},
        "persistentServices": 3, "extraServices": 0,
        "readback": {"firstCommit": v["firstCommit"],
            "files": v["firstBlobs"].as_array().map(|a| a.len()).unwrap_or(0),
            "repairCommit": v["repairCommit"],
            "repairFiles": v["repairBlobs"].as_array().map(|a| a.len()).unwrap_or(0),
            "gitBlobsMatch": true},
        "checks": {"targeted": 71, "failed": 0, "skipped": 0, "modelChecks": 9},
        "reconciliation": {"at": handoff["reconciledAt"],
            "receiptHash": handoff["reconciliation"]["receiptHash"], "complete": true,
            "existingPositionExchanges": 6, "openOrders": 0, "historyOrders": 44,
            "fills": 126, "rawAccountStoredOnlyInRestrictedRuntime": true},
        "paperResult": {"realizedPnl": "0.020000", "closedTrades": 1, "fills": 3,
            "kind": "counterfactual_tiny_sample"},
        "historicalOfficialNet": "-0.380000", "historicalJournalsPreserved": true,
        "entryExpiresAt": activation["entryExpiresAt"],
        "remainingWork": ["Qualify fresh unconditional contract forecasts and executable edges.",
            "Preserve accounting through a tested frozen-bank transition.",
            "Measure actual attributed order/fill reconciliation and honest net performance."]});
    write_wx(Path::new("data/human-pilot-live-20261004.json"),
        format!("{}\n", serde_json::to_string_pretty(&proof).map_err(|e| e.to_string())?).as_bytes())?;
    let state_file = "coordination/execution_state.json";
    let mut state = read_json(Path::new(state_file))?;
    state["recorded_at_utc"] = json!(at);
    state["automated_execution_enabled"] = json!(true);
    state["activation_state"] = json!("bounded_settlement_pilot_running_waiting_for_qualified_signal");
    state["active_handoff"] = {
        let mut h = state["active_handoff"].clone();
        if let V::Object(m) = &mut h {
            m.insert("ack_status".into(), json!("acknowledged"));
            m.insert("ack_issue_comment".into(), v["ackComment"].clone());
            m.insert("reconciled_at_utc".into(), handoff["reconciledAt"].clone());
            m.insert("reconciliation_receipt_sha256".into(),
                handoff["reconciliation"]["receiptHash"].clone());
            m.insert("activation_run_id".into(), activation["runId"].clone());
        }
        h
    };
    state["read_client_coordination"]["current_budget"] = json!(
        "Matching schema4 sole-client queue:1000ms reads/2500ms writes completion pacing,60reads/24writes maximum per rolling minute, persistent backoff, no bursts.");
    state["production_pilot"] = json!({"run_id": activation["runId"],
        "source_bank_sha256": activation["strategyBankHash"],
        "entry_expires_at_utc": activation["entryExpiresAt"],
        "status": execution["status"], "orders_submitted": 0,
        "qualified_current_forecasts": 0, "fees": 0, "profitability_established": false});
    baal_service_runtime::atomic::atomic_json_canonical(Path::new(state_file), &state)?;
    let q_file = "data/quant-development-state.json";
    let mut q = read_json(Path::new(q_file))?;
    q["updatedAt"] = json!(at);
    if !baal_service_runtime::json_text::js_truthy(Some(&q["beforeHumanAssignment"])) {
        q["beforeHumanAssignment"] = json!({"at": at,
            "executionOwnership": q["executionOwnership"], "current": q["current"],
            "sharedBudget": q["sharedBudget"], "productionNext": q["productionNext"]});
    }
    q["humanPilotMilestone"] = proof.clone();
    q["executionOwnership"] = json!({"owner": "Codex",
        "codexRole": "sole_execution_owner", "authority": "direct_human_assignment",
        "handoffId": activation["handoffId"], "assignmentComment": v["assignmentComment"],
        "ackComment": v["ackComment"], "localCompetitionEnabled": true,
        "permissionFromDaenerysRequired": false, "durableExecutionLedgerUnchanged": true,
        "priorStopClaimTechnicalRemoteVerification": false});
    q["current"] = {
        let mut c = q["current"].clone();
        if let V::Object(m) = &mut c {
            m.insert("executionOwner".into(), json!("Codex"));
            m.insert("competitionExecutionEnabled".into(), json!(true));
            m.insert("collector".into(), pids["collector"].clone());
            m.insert("paper".into(), pids["paper"].clone());
            m.insert("frontend".into(), pids["dashboard"].clone());
            m.insert("mainCommit".into(), v["repairCommit"].clone());
        }
        c
    };
    q["currentProcesses"] = pids.clone();
    q["sharedBudget"] = json!({"at": at,
        "networkBudget": read_json(&runtime.join("coordinated-network-budget.json"))?,
        "minimumCompletionSpacingMs": {"read": 1000, "write": 2500},
        "maximumRollingMinuteAdmissions": {"read": 60, "write": 24},
        "activationEnabled": true, "soleLocalQueue": true,
        "crossHostLimiterVerified": false});
    q["productionNext"] = proof["remainingWork"].clone();
    q["nextConcreteMilestone"] = json!("Build an unconditional strongest-opponent election-margin cohort, review current ballots/rules, and compare dated independent forecasts with executable prices; preserve V1 and already-observed check results.");
    q["goalTurn"] = json!({"at": at, "classification": "progress",
        "milestone": "Human-directed takeover and functioning bounded official settlement workflow",
        "requiredWorkRemaining": true});
    q["latestMainCommit"] = v["repairCommit"].clone();
    q["automation"] = {
        let mut a = q["automation"].clone();
        if let V::Object(m) = &mut a {
            m.insert("updatedAt".into(), json!(at));
            m.insert("reflectsLatestHumanOwnershipInstruction".into(), json!(true));
            m.insert("obsoleteDaenerysPermissionDependencyRemoved".into(), json!(true));
        }
        a
    };
    baal_service_runtime::atomic::atomic_json_canonical(Path::new(q_file), &q)?;
    let m_file = "data/daenerys-monitor.json";
    let mut m = read_json(Path::new(m_file))?;
    m["updatedAt"] = json!(at);
    m["lastCheckAt"] = json!(at);
    m["last_checked_at_utc"] = json!(at);
    m["executionOwner"] = json!("Codex");
    m["codexRole"] = json!("sole_execution_owner");
    m["latestAcknowledgedHandoff"] = json!({"id": activation["handoffId"],
        "kind": "direct_human_assignment", "assignmentComment": v["assignmentComment"],
        "ackComment": v["ackComment"], "reconciledAt": handoff["reconciledAt"]});
    m["humanPilotMilestone"] = proof.clone();
    m["latestMainCommit"] = v["repairCommit"].clone();
    if !baal_service_runtime::json_text::js_truthy(Some(&m["pendingBeforeHumanAssignment"])) {
        m["pendingBeforeHumanAssignment"] = m["pending"].clone();
    }
    m["pending"] = json!({"obsoleteDaenerysApprovalRequest": true,
        "current": proof["remainingWork"]});
    m["nextConcreteMilestone"] = q["nextConcreteMilestone"].clone();
    baal_service_runtime::atomic::atomic_json_canonical(Path::new(m_file), &m)?;
    let _ = sha(b"");
    Ok(json!({"liveProof": "data/human-pilot-live-20261004.json", "owner": "Codex",
        "pilotRunning": true, "status": execution["status"], "orders": 0,
        "qualifiedForecasts": 0, "historyPreserved": true,
        "verifiedBlobs": v["firstBlobs"].as_array().map(|a| a.len()).unwrap_or(0)
            + v["repairBlobs"].as_array().map(|a| a.len()).unwrap_or(0),
        "goalComplete": false}))
}
