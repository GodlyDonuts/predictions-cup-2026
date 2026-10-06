// data/record-nh-milestone-20261004.mjs — NH paper v5 milestone record.
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
fn rt() -> PathBuf {
    Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default()).join("SigTradingCup").join("paper-runtime")
}

pub fn run(root: &Path) -> Result<V, String> {
    let r = rt();
    let live = read(&root.join("data/nh-paper-v5-live-20261004.json"))?;
    let preserved = read(&root.join("data/nh-paper-v5-preservation-20261004.json"))?;
    let at = now_iso();
    let milestone = json!({"at": at, "status": "deployed_and_live_recovery_verified",
        "claim": 5979346397i64, "finding": 5979379893i64, "research": 5978588243i64, "tests": 42,
        "evidence": ["data/nh-paper-v5-live-20261004.json", "data/nh-paper-v5-preservation-20261004.json",
            "data/nh-conditional-refund-review-20261004.json", "data/nh-source-capture-20261004.json"],
        "cohorts": live["cohorts"],
        "bankHashes": {"maker": preserved["maker"]["bankHash"], "pair": preserved["pair"]["bankHash"]},
        "recoveryCompletion": live["recovery"]["completion"], "wallets": live["wallets"],
        "reviewExpiresAt": live["reviewExpiresAt"], "accountingReset": false,
        "profitabilityEstablished": false, "additionalReadSlots": 0, "additionalPersistentProcesses": 0,
        "competitionOrders": 0,
        "unresolved": ["Independent review of checkpoint pinning, interleaved recovery pagination and preserved continuation frontiers",
            "Fresh subsequent maker and paired economic measurements; no profit evidence from a reservation release",
            "Optimize collector cold-start public source replay using existing pinned native validator"]});
    atomic_json(&root.join("data/nh-paper-v5-milestone-20261004.json"), &milestone)?;
    let mut q = read(&root.join("data/quant-development-state.json"))?;
    q["updatedAt"] = json!(at);
    q["activeProcesses"] = json!({"collector": live["pids"]["collector"], "paper": live["pids"]["paper"],
        "dashboard": live["pids"]["dashboard"], "total": 3});
    q["nhPaperV5Milestone"] = milestone.clone();
    let mut backlog = q["baalBacklog"].as_array().cloned().unwrap_or_default();
    for item in &mut backlog {
        if item["evidence"].as_i64() == Some(5978588243) {
            item["status"] = json!("completed");
            item["completedAt"] = json!(at);
            item["integrationEvidence"] = milestone["evidence"][0].clone();
        }
    }
    backlog.push(json!({"status": "completed",
        "task": "Repair moving post-expiry maker checkpoints and recover original quote coverage within existing serialized tape slots",
        "completedAt": at, "evidence": milestone["evidence"][0]}));
    q["baalBacklog"] = json!(backlog);
    q["next"] = json!("Optimize the remaining collector public-journal cold startup with pinned Rust validation and exact state/admission parity, preserving all frozen wallets. Measure fresh maker/pair fills and causal cost gates; resolved reservations are not profit. Existing read budget and Daenerys contest ownership remain unchanged. No extra clients, tokens or contest writes.");
    atomic_json(&root.join("data/quant-development-state.json"), &q)?;
    let mut m = read(&root.join("data/daenerys-monitor.json"))?;
    m["updatedAt"] = json!(at);
    let fingerprints = read(&r.join("nh-issue-fingerprints.json"))?;
    if !m["github"].is_object() { return Err("Cannot set properties of undefined (setting 'comments')".into()); }
    m["github"]["comments"] = fingerprints.clone();
    m["nhPaperV5Milestone"] = milestone.clone();
    atomic_json(&root.join("data/daenerys-monitor.json"), &m)?;
    let out = json!({"at": at, "status": milestone["status"], "cohorts": milestone["cohorts"],
        "fingerprints": fingerprints.as_array().map(|a| a.len() as i64).unwrap_or(0),
        "profitabilityEstablished": false});
    println!("{}", js_stringify(&out));
    Ok(milestone)
}
