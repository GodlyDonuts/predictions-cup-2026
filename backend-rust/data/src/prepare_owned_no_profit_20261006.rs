// data/prepare-owned-no-profit-20261006.mjs — install the bounded owned-NO
// profit exit request after the ack recovery descriptor validates.
use crate::frozen_paper_pause_adapter_v1::read_frozen_paper_pause;
use crate::owned_no_profit_host_v1::owned_no_profit_host_identity;
use crate::publication_pack::{now_ms, read_json, runtime_dir};
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
    assert_eq(&stop["source"], &json!("owned-no-profit-host-install-20261006"))?;
    if pid_alive(stop["oldPid"].as_i64().unwrap_or(0)) {
        return Err("OLD_COLLECTOR_PRESENT".into());
    }
    if Path::new(&format!("{rt}/../bot.lock")).exists() || Path::new("STOP").exists()
        || Path::new(&format!("{rt}/STOP")).exists()
        || Path::new(&format!("{rt}/EXECUTION_STOP")).exists()
    {
        return Err("assert".into());
    }
    let recovery = read(&format!("{rt}/paired-neutral-ack-recovery.json"))?;
    let run = recovery["runId"].as_str().unwrap_or("");
    let base = format!("{rt}/paired-live-v2-runs/{run}/child-runtime/paired-live-runs/{run}");
    let actor_file = format!("{base}/controller/paired-rest-journal.jsonl");
    for d in [
        format!("{rt}/paired-live-v2-runs/{run}"),
        format!("{base}/controller"),
        format!("{base}/funding"),
    ] {
        if Path::new(&format!("{d}/execution-owner.lock")).exists() { return Err("assert".into()); }
    }
    let tail: V = serde_json::from_str(
        std::fs::read_to_string(&actor_file).map_err(|e| e.to_string())?
            .trim().split('\n').last().unwrap_or("{}"),
    ).map_err(|e| e.to_string())?;
    let o = tail["state"]["orders"][0].clone();
    let p = o["proof"].clone();
    assert_eq(&json!(tail["state"]["orders"].as_array().map(|a| a.len()).unwrap_or(0)), &json!(1))?;
    assert_eq(&o["phase"], &json!("reconciled"))?;
    assert_eq(&p["scopedPrincipalCost"], &json!("0.070000"))?;
    assert_eq(&p["filledQuantity"], &json!(1))?;
    assert_eq(&p["orderId"], &json!("16297733"))?;
    let pause = read_frozen_paper_pause(
        Path::new(&rt), "settlement_portfolio_v2", "settlement-portfolio-v2-active.json",
        "settlement-portfolio-v2-cohorts", &ms_to_iso(now_ms() as f64),
    )?.ok_or("assert")?;
    let pause = &pause.fields;
    assert_eq(&pause["paused"], &json!(true))?;
    let started_at = ms_to_iso(now_ms() as f64);
    let started_ms = now_ms() as f64;
    let request = json!({
        "schemaVersion": 1, "kind": "single_owned_no_protected_profit_exit_v1", "ownerId": "Codex",
        "runId": run, "tournamentId": p["tournamentId"], "marketId": p["marketId"],
        "exchangeId": p["exchangeId"], "recoveryDescriptorHash": payload_hash(&recovery),
        "buyProofHash": p["verificationHash"], "startedAt": started_at,
        "entryExpiresAt": ms_to_iso(started_ms + 3_600_000.0),
        "authorityHash": payload_hash(&read(&format!("{rt}/paired-live-authority.json"))?),
        "codeIdentity": owned_no_profit_host_identity(Path::new(".")),
    });
    let file = format!("{rt}/owned-no-profit-request.json");
    if Path::new(&file).exists() { return Err("assert".into()); }
    atomic_json(Path::new(&file), &request)?;
    let config_file = format!("{rt}/median-discovery.json");
    let mut config = read(&config_file)?;
    std::fs::copy(&config_file, format!("{config_file}.before-owned-no-profit-20261006")).map_err(|e| e.to_string())?;
    config.as_object_mut().unwrap().insert("codeIdentity".into(), median_discovery_identity(Path::new("."))?);
    atomic_json(Path::new(&config_file), &config)?;
    let out = json!({
        "schemaVersion": 1, "configuredAt": started_at, "runId": run,
        "requestHash": payload_hash(&request), "buyProofHash": p["verificationHash"],
        "originalBuyOrderId": p["orderId"], "marketId": p["marketId"],
        "exchangeId": p["exchangeId"], "originalPrincipalCost": p["scopedPrincipalCost"],
        "quantity": 1, "maximumExitOrders": 1, "minimumProtectedNetProfit": "0.005000",
        "fees": "0.000000", "entryExpiresAt": request["entryExpiresAt"],
        "originalReviewExpiresAt": tail["state"]["plan"]["reviewExpiresAt"],
        "originalWindowsExtended": false, "extraAccountClients": 0, "newCredentials": 0,
        "pausedCounterfactualCohort": pause["cohort"],
        "retainedPaperCases": pause["retainedRisk"].as_array().map(|a| a.len()).unwrap_or(0),
        "collectorGraphFiles": config["codeIdentity"].as_object().map(|m| m.len()).unwrap_or(0),
        "ordersSubmitted": 0,
    });
    atomic_json(Path::new("data/owned-no-profit-configuration-20261006.json"), &out)?;
    Ok(out)
}
fn pid_alive(pid: i64) -> bool {
    if pid <= 0 { return false; }
    std::process::Command::new("kill").arg("-0").arg(pid.to_string())
        .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
        .status().map(|s| s.success()).unwrap_or(false)
}
