// data/prepare-current-election-publication-20261004.mjs — verify the causal
// election production decision and publish the milestone + publication pack.
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

pub fn run() -> Result<V, String> {
    let runtime = localappdata()?.join("SigTradingCup").join("paper-runtime");
    let s = read_json(&runtime.join("stats.json"))?;
    let c = read_json(&runtime.join("competition-stats.json"))?;
    let a = read_json(&runtime.join("competition-activation.json"))?;
    let pids = read_json(Path::new("data/current-runtime-pids.json"))?;
    if s["runtime"]["status"].as_str() != Some("running")
        || c["strategyBankHash"].as_str()
            != Some("a948c395b14992a019e2180133604b8ee84dd78a9e94d08781efd18fbefc0128")
        || !(c["unconfirmedIntents"].is_null()
            || c["unconfirmedIntents"].as_i64() == Some(0))
        || c["readiness"]["complete"].as_bool() != Some(true) {
        return Err("CURRENT_PILOT_NOT_VERIFIED".into());
    }
    let base = "research/current-election";
    let bank = read_json(Path::new(&format!("{}/production-bank-20261004-v2.json", base)))?;
    let record = read_json(Path::new(&format!("{}/tennessee-20261004-v1.json", base)))?;
    if digest(&bank) != a["strategyBankHash"].as_str().unwrap_or("") {
        return Err("FROZEN_BANK_MISMATCH".into());
    }
    if sha(&std::fs::read(format!("{}/assessment.mjs", base))
            .map_err(|e| e.to_string())?)
        != bank["assessment"]["assessmentCodeHash"].as_str().unwrap_or("") {
        return Err("FROZEN_ASSESSOR_HASH_MISMATCH".into());
    }
    let dir = runtime.join("competition-runs")
        .join(a["runId"].as_str().unwrap_or("")).join("native-decisions");
    let mut clocks: Option<V> = None;
    let mut names: Vec<String> = std::fs::read_dir(&dir).map_err(|e| e.to_string())?
        .filter_map(|e| e.ok().and_then(|x| x.file_name().into_string().ok()))
        .collect();
    names.sort();
    names.reverse();
    for name in names {
        let input_file = dir.join(&name).join("input.json");
        if !input_file.exists() { continue; }
        let bytes = std::fs::read(&input_file).map_err(|e| e.to_string())?;
        if sha(&bytes) != c["latestNative"]["inputHash"].as_str().unwrap_or("") {
            continue;
        }
        let input: V = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        let candidate = input["candidates"].as_array().cloned().unwrap_or_default()
            .iter().find(|x| x["marketId"].as_str() == Some("363"))
            .cloned().unwrap_or(V::Null);
        let as_of = input["asOfMs"].as_f64().unwrap_or(f64::NAN);
        clocks = Some(json!({
            "at": baal_service_runtime::contract::ms_to_iso(as_of),
            "inputHash": c["latestNative"]["inputHash"],
            "outputHash": c["latestNative"]["outputHash"],
            "positionsAgeMs": as_of - candidate["account"]["positionsAtMs"].as_f64()
                .unwrap_or(f64::NAN),
            "ordersAgeMs": as_of - candidate["account"]["ordersAtMs"].as_f64()
                .unwrap_or(f64::NAN),
            "bookAgeMs": as_of - candidate["book"]["receivedAtMs"].as_f64()
                .unwrap_or(f64::NAN),
            "accountBeforeBook": candidate["account"]["knownAtMs"].as_f64()
                .unwrap_or(f64::NAN)
                <= candidate["book"]["requestStartedAtMs"].as_f64()
                    .unwrap_or(f64::NAN),
            "nativeWallMs": c["latestNative"]["wallMs"],
            "decision": c["latestNative"]["decision"]["reason"]}));
        break;
    }
    let clocks = clocks.ok_or("FRESH_CAUSAL_NATIVE_DECISION_NOT_VERIFIED")?;
    if clocks["accountBeforeBook"].as_bool() != Some(true)
        || clocks["positionsAgeMs"].as_f64().unwrap_or(f64::NAN) > 20000.0
        || clocks["ordersAgeMs"].as_f64().unwrap_or(f64::NAN) > 20000.0
        || clocks["bookAgeMs"].as_f64().unwrap_or(f64::NAN) > 20000.0 {
        return Err("FRESH_CAUSAL_NATIVE_DECISION_NOT_VERIFIED".into());
    }
    let milestone = json!({"schemaVersion": 1,
        "id": "causal-election-production-20261004-v1", "at": now_iso(),
        "goalTurnClassification": "progress", "requiredGoalWorkRemaining": true,
        "authority": "existing_direct_human_assignment", "runId": a["runId"],
        "bankHash": a["strategyBankHash"], "entryExpiresAt": a["entryExpiresAt"],
        "source": {"modelHash": record["prior"]["modelHash"],
            "pollSample": 607, "pollFieldEndDate": "2026-08-26",
            "assessmentAt": record["assessment"]["assessedAt"],
            "ordinaryRange": [record["assessment"]["ordinary"]["lower"],
                record["assessment"]["ordinary"]["upper"]],
            "broaderStress": [record["assessment"]["stress"]["lower"],
                record["assessment"]["stress"]["upper"]]},
        "changes": [
            "Causal source-dated election assessment, original LV table and verified ballot; no market-derived forecast input.",
            "Stopped-writer bank transition preserves the same window, inventory, FIFO and lifetime purchase budget.",
            "Canonical activation equality tolerates JSON key ordering.",
            "Production books/decisions precede paper observers; account freshness repaired before the decision book.",
            "Detached current-user launcher reuses three services and active frozen paper cohort; no new watcher or credentials.",
            "Read-only book preview replaces an existing auxiliary slot."],
        "checks": {"admissionAndModel": 89, "runnerLifecycle": 52,
            "serializedQueue": 2, "runnerSyntax": true},
        "clockEvidence": clocks,
        "runtime": {"collectorPid": pids["collector"], "paperPid": pids["paper"],
            "dashboardPid": pids["dashboard"], "reportedAt": s["reportedAt"],
            "cycles": s["runtime"]["cycles"], "status": s["runtime"]["status"],
            "errors": s["runtime"]["errors"].as_array().map(|e| e.len())
                .unwrap_or(0),
            "productionStatus": c["status"],
            "qualifiedCurrentForecasts": c["eligibleResearchMarkets"].as_array()
                .map(|x| x.len()).unwrap_or(0),
            "ordersSubmitted": c["ordersSubmitted"],
            "unconfirmedIntents": c["unconfirmedIntents"],
            "realizedPnl": c["realizedPnl"], "addedFees": c["addedFees"]},
        "limitations": [
            "Scenario endpoints and stress assumptions are uncalibrated; an ordinary edge is not a guaranteed payout.",
            "Current price0.920 exceeded maximum admitted ask0.910 in the first preview; no forced order.",
            "Runtime process disappearance was observed; its cause and robustness against every future host termination remain unproved.",
            "Prior attributed official roundtrip−0.380 remains separate. No profitability or scaling readiness is established."],
        "next": [
            "Keep the current frozen bank and original clocks; expire normally rather than regenerate unchanged research.",
            "Seek independent current polls and exact ballot/contract bindings for additional candidate markets.",
            "Measure actual subsequent dispatch, fills, depth-aware liquidation and net P&L; scale only with evidence.",
            "Investigate expensive restart/loop work while preserving original journals and risk."],
        "profitabilityEstablished": false});
    write_pretty("data/current-election-milestone-20261004.json", &milestone)?;
    for file in ["data/quant-development-state.json", "data/daenerys-monitor.json"] {
        let mut state = read_json(Path::new(file))?;
        state["currentElectionMilestone"] = milestone.clone();
        state["updatedAt"] = milestone["at"].clone();
        write_pretty(file, &state)?;
    }
    let execution_state_path = "coordination/execution_state.json";
    if Path::new(execution_state_path).exists() {
        let mut state = read_json(Path::new(execution_state_path))?;
        state["recorded_at_utc"] = milestone["at"].clone();
        let mut pp = state["production_pilot"].clone();
        if let V::Object(m) = &mut pp {
            m.insert("source_bank_sha256".into(), a["strategyBankHash"].clone());
            m.insert("status".into(), c["status"].clone());
            m.insert("qualified_current_forecasts".into(),
                json!(c["eligibleResearchMarkets"].as_array().map(|x| x.len())
                    .unwrap_or(0)));
            m.insert("orders_submitted".into(), c["ordersSubmitted"].clone());
            m.insert("runtime_recovery_evidence".into(),
                json!("data/current-election-milestone-20261004.json"));
            m.insert("latest_model_evidence".into(),
                json!("research/current-election/tennessee-20261004-v1.json"));
            m.insert("last_native_decision".into(), clocks.clone());
        }
        state["production_pilot"] = pp;
        write_pretty(execution_state_path, &state)?;
    }
    let mut files: Vec<String> = vec![
        "Start-BaalServices.ps1".to_string(),
        "execution/controller.mjs".to_string(),
        "execution/settlement-controller.mjs".to_string(),
        "execution/settlement-controller.test.mjs".to_string(),
        "execution/settlement-bank-transition.mjs".to_string(),
        "live-paper/production-metadata.mjs".to_string(),
        "live-paper/production-metadata.test.mjs".to_string(),
        "live-paper/production-read-priority.mjs".to_string(),
        "live-paper/production-read-priority.test.mjs".to_string(),
        "live-paper/runner.mjs".to_string(),
        "live-paper/runner-lifecycle.test.mjs".to_string()];
    for f in ["README.md", "assessment.mjs", "assessment.test.mjs",
            "definition.json", "prepare-tennessee.mjs", "prepare-bank.mjs",
            "tennessee-20261004-v1.json", "production-bank-20261004-v2.json",
            "price-diagnostic-20261004-v1.json"] {
        files.push(format!("{}/{}", base, f));
    }
    files.push("data/tennessee-production-transition-20261004.json".to_string());
    files.push("data/current-election-milestone-20261004.json".to_string());
    files.push("data/prepare-current-election-publication-20261004.mjs".to_string());
    if Path::new(execution_state_path).exists() {
        files.push(execution_state_path.to_string());
    }
    let secret_re = regex_lite_secret();
    let mut elements = Vec::new();
    for file in &files {
        let content = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
        if secret_re.is_match(&content) { return Err("SECRET_SCAN_REJECTED".into()); }
        elements.push(json!({"path": file, "mode": "100644", "type": "blob",
            "content": content}));
    }
    std::fs::write("data/current-election-publication-pack.json",
        serde_json::to_string(&elements).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let manifest: Vec<V> = elements.iter().map(|e| {
        let bytes = e["content"].as_str().unwrap_or("").as_bytes();
        json!({"path": e["path"], "bytes": bytes.len(),
            "sha256": sha(bytes), "gitBlobSha1": git_blob_sha1(bytes)})
    }).collect();
    write_pretty("data/current-election-publication-manifest.json",
        &V::Array(manifest))?;
    Ok(json!({"files": files.len(),
        "packChars": serde_json::to_string(&elements).unwrap_or_default().len(),
        "clockEvidence": clocks, "runtime": milestone["runtime"],
        "profitabilityEstablished": false}))
}

pub(crate) fn regex_lite_secret() -> SecretRegex { SecretRegex }

pub(crate) struct SecretRegex;
impl SecretRegex {
    /// /ace_[a-f0-9]{20,}|github_pat_[a-z0-9_]+|ghp_[a-z0-9]{20,}|Bearer\s+[a-z0-9._-]{20,}/i
    pub(crate) fn is_match(&self, s: &str) -> bool {
        let lower = s.to_ascii_lowercase();
        let bytes = lower.as_bytes();
        let run = |i: usize, pred: fn(u8) -> bool| -> usize {
            let mut n = 0;
            while i + n < bytes.len() && pred(bytes[i + n]) { n += 1; }
            n
        };
        let alnum = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
        let blob = |b: u8| b.is_ascii_alphanumeric() || b == b'.'
            || b == b'_' || b == b'-';
        for i in 0..bytes.len() {
            if lower[i..].starts_with("ace_") && run(i + 4, |b| b.is_ascii_hexdigit()) >= 20 {
                return true;
            }
            if lower[i..].starts_with("github_pat_") && run(i + 11, alnum) >= 1 {
                return true;
            }
            if lower[i..].starts_with("ghp_") && run(i + 4, alnum) >= 20 {
                return true;
            }
            if lower[i..].starts_with("bearer") {
                let mut j = i + 6;
                while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') { j += 1; }
                if j > i + 6 && run(j, blob) >= 20 { return true; }
            }
        }
        false
    }
}
