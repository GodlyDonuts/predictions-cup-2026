// data/prepare-neutral-sell-recovery-20261006.mjs — install the neutral-sell
// recovery request preserving all original journals.
use crate::neutral_sell_recovery_host_v1::neutral_sell_recovery_host_identity;
use crate::no_neutral_sell_all_v1::inspect_no_sell_ack_all_economics;
use crate::publication_pack::{now_ms, read_json, runtime_dir, sha};
use baal_collector::collector_median_discovery_v3::median_discovery_identity;
use baal_execution::core::payload_hash;
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
    let stop = read(&format!("{rt}/COLLECTOR_STOP"))?;
    assert_eq(&stop["source"], &json!("neutral-sell-recovery-install-20261006"))?;
    if pid_alive(stop["oldPid"].as_i64().unwrap_or(0)) {
        return Err("OLD_PRESENT".into());
    }
    if Path::new(&format!("{rt}/../bot.lock")).exists() || Path::new("STOP").exists()
        || Path::new(&format!("{rt}/STOP")).exists()
        || Path::new(&format!("{rt}/EXECUTION_STOP")).exists()
    {
        return Err("assert".into());
    }
    let profit = read(&format!("{rt}/owned-no-profit-request.json"))?;
    let run = profit["runId"].as_str().unwrap_or("");
    let base = format!("{rt}/paired-live-v2-runs/{run}/child-runtime/paired-live-runs/{run}/owned-no-profit-v1");
    if Path::new(&format!("{base}/execution-owner.lock")).exists()
        || Path::new(&format!("{base}/controller/execution-owner.lock")).exists()
    {
        return Err("assert".into());
    }
    let file = format!("{base}/controller/owned-no-exit-journal.jsonl");
    let prefix = std::fs::read(&file).map_err(|e| e.to_string())?;
    let last_line = String::from_utf8_lossy(&prefix).trim_end().split('\n').last().unwrap_or("").to_string();
    let state: V = serde_json::from_str::<V>(&last_line).map_err(|e| e.to_string())?["state"].clone();
    let o = state["intent"].clone();
    let p = state["buyProof"].clone();
    assert_eq(&o["orderId"], &json!("16539746"))?;
    assert_eq(&o["phase"], &json!("semantic_mismatch"))?;
    assert_eq(&p["scopedPrincipalCost"], &json!("0.070000"))?;
    let e = inspect_no_sell_ack_all_economics(&o["ack"])?;
    assert_eq(&e["allState"], &json!("observed_neutral_sell"))?;
    let old_profit = std::fs::read(format!("{base}/owned-no-profit-journal.jsonl")).map_err(|e| e.to_string())?;
    let started_at = ms_to_iso(now_ms() as f64);
    let started_ms = now_ms() as f64;
    let revision = json!({
        "kind": "same_run_neutral_all_sell_ack_reconciliation", "ownerId": "Codex",
        "runId": run, "tournamentId": p["tournamentId"], "lotId": p["lot"]["lotId"],
        "buyProofHash": p["verificationHash"], "orderId": o["orderId"],
        "originalPrefixBytes": prefix.len(), "originalPrefixHash": sha(&prefix),
        "ackSourceBodyHash": o["ack"]["sourceBodyHash"],
    });
    let request = json!({
        "schemaVersion": 1, "kind": "single_owned_no_neutral_sell_recovery_v1", "ownerId": "Codex",
        "runId": run, "tournamentId": p["tournamentId"],
        "profitRequestHash": payload_hash(&profit), "authorityHash": profit["authorityHash"],
        "oldProfitJournalHash": sha(&old_profit), "oldProfitJournalBytes": old_profit.len(),
        "startedAt": started_at, "expiresAt": ms_to_iso(started_ms + 3_600_000.0),
        "codeIdentity": neutral_sell_recovery_host_identity(Path::new(".")),
        "neutralSellRevision": revision,
    });
    if Path::new(&format!("{rt}/neutral-sell-recovery-request.json")).exists() { return Err("assert".into()); }
    atomic_json(Path::new(&format!("{rt}/neutral-sell-recovery-request.json")), &request)?;
    let cfile = format!("{rt}/median-discovery.json");
    let mut c = read(&cfile)?;
    std::fs::copy(&cfile, format!("{cfile}.before-neutral-sell-recovery-20261006")).map_err(|e| e.to_string())?;
    c.as_object_mut().unwrap().insert("codeIdentity".into(), median_discovery_identity(Path::new("."))?);
    atomic_json(Path::new(&cfile), &c)?;
    atomic_json(Path::new("data/neutral-sell-recovery-configuration-20261006.json"), &json!({
        "schemaVersion": 1, "configuredAt": started_at, "runId": run,
        "requestHash": payload_hash(&request), "originalPrefixBytes": prefix.len(),
        "originalPrefixHash": revision["originalPrefixHash"],
        "oldProfitJournalBytes": old_profit.len(),
        "oldProfitJournalHash": request["oldProfitJournalHash"],
        "originalBuyOrderId": p["orderId"], "saleOrderId": o["orderId"],
        "originalBasis": p["scopedPrincipalCost"], "ackReportedProceeds": "0.080000",
        "terminalSaleProofVerified": false, "oldWriterExpiry": profit["entryExpiresAt"],
        "oldWindowsExtended": false, "GETOnly": true, "newOrders": false, "newClients": 0,
        "collectorGraphFiles": c["codeIdentity"].as_object().map(|m| m.len()).unwrap_or(0),
    }))?;
    Ok(json!({
        "saleOrderId": o["orderId"], "originalPrefixBytes": prefix.len(),
        "GETOnly": true, "newOrders": false, "requestHash": payload_hash(&request),
    }))
}
fn pid_alive(pid: i64) -> bool {
    if pid <= 0 { return false; }
    std::process::Command::new("kill").arg("-0").arg(pid.to_string())
        .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
        .status().map(|s| s.success()).unwrap_or(false)
}
