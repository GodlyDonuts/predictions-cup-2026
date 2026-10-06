// data/prepare-current-poll-publication-20261005.mjs — verify the recovered
// collector and closed pilot, then publish the current-polls milestone + pack.
use baal_service_runtime::json_text::{digest, sha256};
use serde_json::{json, Value as V};
use sha1::Sha1;
use sha2::Digest as _;
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
fn parse_iso_ms(s: &str) -> Option<f64> {
    baal_service_runtime::contract::parse_iso_ms(s)
}
fn write_pretty(p: &str, v: &V) -> Result<(), String> {
    std::fs::write(p, format!("{}\n", serde_json::to_string_pretty(v)
        .map_err(|e| e.to_string())?)).map_err(|e| e.to_string())
}
fn git_blob_sha1(b: &[u8]) -> String {
    let mut h = Sha1::new();
    h.update(format!("blob {}\0", b.len()).as_bytes());
    h.update(b);
    format!("{:x}", h.finalize())
}
fn secret_rejected(s: &str) -> bool {
    crate::prepare_current_election_publication_20261004::regex_lite_secret()
        .is_match(s)
}

pub fn run() -> Result<V, String> {
    let runtime = localappdata()?.join("SigTradingCup").join("paper-runtime");
    let s = read_json(&runtime.join("stats.json"))?;
    let c = read_json(&runtime.join("competition-stats.json"))?;
    let a = read_json(&runtime.join("competition-activation.json"))?;
    let pids = read_json(Path::new("data/current-runtime-pids.json"))?;
    let bank = read_json(&runtime.join("production-banks")
        .join(format!("{}.json", a["strategyBankHash"].as_str().unwrap_or(""))))?;
    let comparison = read_json(Path::new(
        "research/current-polls-v1/price-comparison-20261005-v1.json"))?;
    let assessments = read_json(Path::new(
        "research/current-polls-v1/assessments-20261005-v1.json"))?;
    let at = now_iso();
    if s["runtime"]["status"].as_str() != Some("running")
        || parse_iso_ms(&at).unwrap_or(f64::NAN)
            - parse_iso_ms(s["reportedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN)
            > 60000.0
        || s["runtime"]["pid"] != pids["collector"]
        || !s["runtime"]["errors"].as_array().map(|e| e.is_empty()).unwrap_or(false) {
        return Err("RECOVERED_COLLECTOR_NOT_CURRENT".into());
    }
    if a["strategyBankHash"].as_str()
            != Some("a948c395b14992a019e2180133604b8ee84dd78a9e94d08781efd18fbefc0128")
        || digest(&bank) != a["strategyBankHash"].as_str().unwrap_or("")
        || a["entryExpiresAt"].as_str() != Some("2026-10-05T03:59:22.131Z") {
        return Err("ORIGINAL_ACTIVATION_CHANGED".into());
    }
    if c["status"].as_str() != Some("entry_window_closed")
        || c["entryWindow"]["open"].as_bool() != Some(false)
        || c["ordersSubmitted"].as_i64() != Some(0)
        || c["unconfirmedIntents"].as_i64() != Some(0)
        || c["realizedPnl"].as_str() != Some("0.000000")
        || c["readiness"]["complete"].as_bool() != Some(true) {
        return Err("CLOSED_PILOT_NOT_VERIFIED".into());
    }
    if comparison["eligiblePriceReceipts"].as_i64() != Some(0)
        || comparison["rows"].as_array().map(|r| r.len()).unwrap_or(0) != 3
        || comparison["rows"].as_array().cloned().unwrap_or_default().iter()
            .any(|r| parse_iso_ms(r["bookRequestStartedAt"].as_str().unwrap_or(""))
                    .unwrap_or(f64::NAN)
                < parse_iso_ms(r["assessmentAt"].as_str().unwrap_or(""))
                    .unwrap_or(f64::NAN)
                || r["executionAuthority"].as_bool() != Some(false)) {
        return Err("SUBSEQUENT_SOURCE_COMPARISON_NOT_VERIFIED".into());
    }
    let milestone = json!({"schemaVersion": 1,
        "id": "production-recovery-current-polls-20261005-v1", "at": at,
        "goalTurnClassification": "progress", "requiredGoalWorkRemaining": true,
        "authority": "existing_direct_human_assignment", "runId": a["runId"],
        "bankHash": a["strategyBankHash"], "entryExpiresAt": a["entryExpiresAt"],
        "activationChanged": false, "productionBankChanged": false,
        "incident": {"observedAt": "2026-10-05T11:19:00Z",
            "observationPrecision": "minute",
            "previousPids": {"collector": 4760, "paper": 17228, "dashboard": 22396},
            "previousPidsAbsent": true,
            "lastSavedCollectorReportAt": "2026-10-04T23:43:43.768Z",
            "causeEstablished": false, "humanStopRemoved": false},
        "changes": [
            "Derived production entry-window status preserves blocked, uncertain and owned-risk states.",
            "Recovered the same three approved services; controlled collector reload deployed the status fix without changing activation.",
            "Captured six hashed primary polling/ballot sources and retained original clocks and population distinctions.",
            "Built separate Republican/Democratic source assessments with verified opposition, multinomial margin variance and frozen sensitivity assumptions.",
            "Compared three actual subsequent official contract/book receipts using existing auxiliary slots and exact price policy."],
        "checks": {"targetedPassed": 23, "failed": 0,
            "classes": ["entry-state projection",
                "settlement controller and expired-window integration",
                "primary polling assessment"]},
        "runtime": {"collectorPid": pids["collector"], "paperPid": pids["paper"],
            "dashboardPid": pids["dashboard"], "persistentServices": 3,
            "reportedAt": s["reportedAt"], "cycles": s["runtime"]["cycles"],
            "errors": s["runtime"]["errors"].as_array().map(|e| e.len())
                .unwrap_or(0),
            "productionReportedAt": c["updatedAt"], "status": c["status"],
            "reason": c["reason"], "entryWindow": c["entryWindow"],
            "ordersSubmitted": c["ordersSubmitted"],
            "dispatchAttempts": c["dispatchAttempts"],
            "unconfirmedIntents": c["unconfirmedIntents"],
            "realizedPnl": c["realizedPnl"], "addedFees": c["addedFees"],
            "readinessComplete": c["readiness"]["complete"]},
        "research": {"recordId": assessments["id"],
            "assessedAt": assessments["assessments"][0]["assessment"]["assessedAt"],
            "assessments": assessments["assessments"].as_array()
                .map(|x| x.len()).unwrap_or(0),
            "comparisonId": comparison["id"],
            "subsequentPriceReceipts": comparison["rows"].as_array()
                .map(|x| x.len()).unwrap_or(0),
            "qualifiedReceipts": comparison["eligiblePriceReceipts"],
            "additionalAccountClients": 0, "installedInProduction": false},
        "limitations": [
            "No order or positive official net performance from this pilot. Earlier official net−0.380 remains separate.",
            "Sensitivity endpoints are subjective valuations, not calibrated probability bounds or profit evidence.",
            "Three subsequent price receipts establish causal comparisons only, not a strategy-performance sample.",
            "The cause of the overnight service disappearance and reliable unattended recovery remain unresolved.",
            "The expired original entry window was not extended."],
        "next": [
            "Calibrate current party outcome forecasts and compare subsequent executable economics without tuning to force entries.",
            "Resolve the observed service-lifetime failure using concrete process evidence.",
            "Prepare a useful bounded continuation only after qualifying source/price evidence; preserve inventory and lifetime limits."],
        "profitabilityEstablished": false});
    write_pretty("data/production-recovery-20261005.json", &milestone)?;
    for file in ["data/quant-development-state.json", "data/daenerys-monitor.json"] {
        let mut state = read_json(Path::new(file))?;
        state["updatedAt"] = json!(at);
        state["productionRecoveryCurrentPolls"] = milestone.clone();
        state["nextConcreteMilestone"] = milestone["next"].clone();
        if file.contains("quant-development") {
            state["activeProcesses"] = json!({"collector": pids["collector"],
                "dashboard": pids["dashboard"],
                "probabilityWorker": pids["paper"], "persistent": 3, "extra": 0});
            state["currentRuntimePids"] = pids.clone();
            state["productionNext"] = milestone["next"].clone();
            state["lastMilestone"] = milestone["id"].clone();
        } else {
            state["lastCheckAt"] = json!(at);
            state["lastEngineeringMilestone"] = milestone["id"].clone();
        }
        write_pretty(file, &state)?;
    }
    let mut ownership = read_json(Path::new("coordination/execution_state.json"))?;
    ownership["recorded_at_utc"] = json!(at);
    ownership["activation_state"] = json!("original_entry_window_closed");
    ownership["automation_status_basis"] = json!(
        "Original human-assigned pilot preserved; entry expiry2026-10-05T03:59:22.131Z. Recovered collector reports entry_window_closed, zero orders and zero unconfirmed intents. This record is not a process mutex, renewed authority or profitability proof.");
    let mut pp = ownership["production_pilot"].clone();
    if let V::Object(m) = &mut pp {
        m.insert("status".into(), c["status"].clone());
        m.insert("qualified_current_forecasts".into(),
            json!(c["eligibleResearchMarkets"].as_array().map(|x| x.len())
                .unwrap_or(0)));
        m.insert("orders_submitted".into(), c["ordersSubmitted"].clone());
        m.insert("entry_expires_at".into(), a["entryExpiresAt"].clone());
        m.insert("runtime_recovery_evidence".into(),
            json!("data/production-recovery-20261005.json"));
        m.insert("latest_model_evidence".into(),
            json!("research/current-polls-v1/assessments-20261005-v1.json"));
        m.insert("latest_model_installed".into(), json!(false));
    }
    ownership["production_pilot"] = pp;
    write_pretty("coordination/execution_state.json", &ownership)?;
    let base = "research/current-polls-v1";
    let mut files: Vec<String> = vec![
        "execution/production-entry-state.mjs".to_string(),
        "execution/production-entry-state.test.mjs".to_string(),
        "execution/settlement-controller.mjs".to_string(),
        "execution/settlement-controller.test.mjs".to_string()];
    for f in ["capture.mjs", "assessment.mjs", "assessment.test.mjs", "build.mjs",
            "compare.mjs", "README.md", "sources.json", "ballot-sources.json",
            "assessments-20261005-v1.json", "price-comparison-20261005-v1.json"] {
        files.push(format!("{}/{}", base, f));
    }
    for f in ["README.md", "docs/CURRENT_STATE.md",
            "coordination/execution_state.json",
            "data/production-recovery-20261005.json",
            "data/prepare-current-poll-publication-20261005.mjs"] {
        files.push(f.to_string());
    }
    let mut elements = Vec::new();
    for file in &files {
        let content = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
        if secret_rejected(&content) { return Err("SECRET_SCAN_REJECTED".into()); }
        elements.push(json!({"path": file, "mode": "100644", "type": "blob",
            "content": content}));
    }
    let pack = serde_json::to_string(&elements).map_err(|e| e.to_string())?;
    std::fs::write("data/current-poll-publication-pack.json", &pack)
        .map_err(|e| e.to_string())?;
    let manifest: Vec<V> = elements.iter().map(|e| {
        let b = e["content"].as_str().unwrap_or("").as_bytes();
        json!({"path": e["path"], "bytes": b.len(), "sha256": sha(b),
            "gitBlobSha1": git_blob_sha1(b)})
    }).collect();
    write_pretty("data/current-poll-publication-manifest.json",
        &V::Array(manifest))?;
    Ok(json!({"at": at, "files": files.len(), "packChars": pack.len(),
        "runtime": milestone["runtime"], "research": milestone["research"],
        "profitabilityEstablished": false}))
}
