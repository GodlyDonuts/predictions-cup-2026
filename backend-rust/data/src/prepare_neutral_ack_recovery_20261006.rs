// data/prepare-neutral-ack-recovery-20261006.mjs — install the neutral-ack
// recovery descriptor after the prior collector is proven stopped.
use crate::paired_neutral_recovery_host_v1::paired_neutral_recovery_identity;
use crate::publication_pack::{now_ms, read_json, runtime_dir, sha};
use baal_collector::collector_median_discovery_v3::median_discovery_identity;
use baal_execution::core::payload_hash;
use baal_execution::no_neutral_all::inspect_no_ack_all_economics;
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::contract::ms_to_iso;
use serde_json::{json, Value as V};
use std::path::Path;

fn assert_eq(a: &V, b: &V) -> Result<(), String> {
    if a == b { Ok(()) } else { Err("assert".into()) }
}
pub fn run() -> Result<V, String> {
    let rt = runtime_dir();
    let read = |p: &str| read_json(Path::new(p));
    let marker = read(&format!("{rt}/COLLECTOR_STOP"))?;
    assert_eq(&marker["source"], &json!("neutral-no-reconciliation-install-20261006"))?;
    assert_eq(&marker["oldPid"], &json!(28804))?;
    if pid_alive(marker["oldPid"].as_i64().unwrap_or(0)) {
        return Err("PREVIOUS_COLLECTOR_PRESENT".into());
    }
    if Path::new(&format!("{rt}/../bot.lock")).exists() || Path::new("STOP").exists()
        || Path::new(&format!("{rt}/STOP")).exists()
        || Path::new(&format!("{rt}/EXECUTION_STOP")).exists()
    {
        return Err("assert".into());
    }
    let outer = read(&format!("{rt}/paired-live-v2-request.json"))?;
    let run_id = outer["runId"].as_str().unwrap_or("");
    let dir = format!("{rt}/paired-live-v2-runs/{run_id}");
    let child = format!("{dir}/child-runtime");
    let run_dir = format!("{child}/paired-live-runs/{run_id}");
    assert_eq(&outer["runId"], &json!("sig-pilot-paired-generic-20261006-human"))?;
    for d in [&dir, &format!("{run_dir}/controller"), &format!("{run_dir}/funding")] {
        if Path::new(&format!("{d}/execution-owner.lock")).exists() { return Err("assert".into()); }
    }
    let rows_raw = std::fs::read_to_string(format!("{dir}/selection.jsonl")).map_err(|e| e.to_string())?;
    let rows: Vec<&str> = rows_raw.trim().split('\n').collect();
    assert_eq(&json!(rows.len()), &json!(1))?;
    let selection: V = serde_json::from_str::<V>(rows[0]).map_err(|e| e.to_string())?["state"].clone();
    let prefix = std::fs::read(format!("{run_dir}/controller/paired-rest-journal.jsonl")).map_err(|e| e.to_string())?;
    if *prefix.last().unwrap_or(&0) != b'\n' { return Err("assert".into()); }
    let last_line = String::from_utf8_lossy(&prefix).trim_end().split('\n').last().unwrap_or("").to_string();
    let state: V = serde_json::from_str::<V>(&last_line).map_err(|e| e.to_string())?["state"].clone();
    assert_eq(&json!(state["orders"].as_array().map(|a| a.len()).unwrap_or(0)), &json!(1))?;
    let order = state["orders"][0].clone();
    assert_eq(&order["orderId"], &json!("16297733"))?;
    assert_eq(&order["phase"], &json!("semantic_mismatch"))?;
    let economics = inspect_no_ack_all_economics(&order["ack"])?;
    assert_eq(&economics["allState"], &json!("neutral"))?;
    assert_eq(&economics["ackNotional"], &json!("0.070000"))?;
    let descriptor = json!({
        "schemaVersion": 1, "kind": "paired_neutral_ack_recovery_only", "ownerId": "Codex",
        "runId": outer["runId"], "tournamentId": outer["tournamentId"],
        "outerRequestHash": payload_hash(&outer), "selectionHash": payload_hash(&selection),
        "childRequestHash": payload_hash(&read(&format!("{child}/paired-live-request.json"))?),
        "authorityHash": outer["authorityHash"],
        "activationHash": payload_hash(&read(&format!("{run_dir}/activation.json"))?),
        "codeIdentity": paired_neutral_recovery_identity(Path::new(".")),
        "neutralAckRevision": {
            "kind": "same_run_neutral_all_ack_reconciliation", "ownerId": "Codex",
            "runId": outer["runId"], "scopeHash": payload_hash(&selection["scope"]),
            "originalPrefixBytes": prefix.len(), "originalPrefixHash": sha(&prefix),
            "orderId": order["orderId"], "ackSourceBodyHash": order["ack"]["sourceBodyHash"],
        },
    });
    if Path::new(&format!("{rt}/paired-neutral-ack-recovery.json")).exists() { return Err("assert".into()); }
    atomic_json(Path::new(&format!("{rt}/paired-neutral-ack-recovery.json")), &descriptor)?;
    let config_file = format!("{rt}/median-discovery.json");
    let mut config = read(&config_file)?;
    std::fs::copy(&config_file, format!("{config_file}.before-neutral-ack-recovery-20261006")).map_err(|e| e.to_string())?;
    config.as_object_mut().unwrap().insert("codeIdentity".into(), median_discovery_identity(Path::new("."))?);
    atomic_json(Path::new(&config_file), &config)?;
    let evidence = json!({
        "schemaVersion": 1, "configuredAt": ms_to_iso(now_ms() as f64),
        "runId": outer["runId"], "orderId": order["orderId"], "mode": descriptor["kind"],
        "descriptorHash": payload_hash(&descriptor),
        "originalPrefixBytes": prefix.len(),
        "originalPrefixHash": descriptor["neutralAckRevision"]["originalPrefixHash"],
        "ackSourceBodyHash": order["ack"]["sourceBodyHash"],
        "ackReportedCost": economics["ackNotional"],
        "originalReviewExpiresAt": selection["reviewExpiresAt"],
        "originalOuterExpiresAt": outer["entryExpiresAt"],
        "expiryExtended": false, "entryAllowed": false, "writeCapability": false,
        "extraAccountClients": 0, "originalJournalsPreserved": true,
        "terminalRestReconciliationVerified": false,
        "collectorGraphFiles": config["codeIdentity"].as_object().map(|o| o.len()).unwrap_or(0),
    });
    atomic_json(Path::new("data/neutral-ack-recovery-configuration-20261006.json"), &evidence)?;
    Ok(evidence)
}
fn pid_alive(pid: i64) -> bool {
    if pid <= 0 { return false; }
    std::process::Command::new("kill").arg("-0").arg(pid.to_string())
        .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
        .status().map(|s| s.success()).unwrap_or(false)
}
