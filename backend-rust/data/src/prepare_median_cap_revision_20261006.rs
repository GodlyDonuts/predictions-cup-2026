// data/prepare-median-cap-revision-20261006.mjs — stage (--install) or
// prepare a same-run median entry-cap bank revision.
use crate::publication_pack::{now_ms, read_json, runtime_dir, sha};
use baal_collector::breadth::validate_universe;
use baal_collector::model::digest;
use baal_execution::core::payload_hash;
use baal_execution::median_controller::validate_median_bank;
use baal_execution::median_controller_v2::{
    median_controller_v2_identity, median_execution_policy_v2, validate_median_revision,
};
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::canonical_json;
use serde_json::{json, Value as V};
use std::path::Path;

fn exists(p: &str) -> bool { Path::new(p).exists() }
pub fn run(install: bool) -> Result<V, String> {
    let runtime = runtime_dir();
    let workspace = Path::new(".");
    let activation_file = format!("{runtime}/competition-activation.json");
    let activation_bytes = std::fs::read(&activation_file).map_err(|e| e.to_string())?;
    let activation: V = serde_json::from_slice(&activation_bytes).map_err(|e| e.to_string())?;
    let universe = validate_universe(&read_json(Path::new("live-paper/price-universe.json"))?, activation["tournamentId"].as_str().unwrap_or(""))?;
    let prior_bank = read_json(Path::new(&format!(
        "{}/production-banks/{}.json", runtime, activation["strategyBankHash"].as_str().unwrap_or("")
    )))?;
    validate_median_bank(&prior_bank, &activation, &universe, workspace)?;
    let archive_dir = format!("{}/production-code/{}", runtime, activation["strategyBankHash"].as_str().unwrap_or(""));
    for (relative, pin) in prior_bank["codeIdentity"].as_object().into_iter().flatten() {
        let source = std::fs::canonicalize(format!("./{relative}")).map_err(|e| e.to_string())?;
        let dest = std::fs::canonicalize(&archive_dir).unwrap_or(Path::new(&archive_dir).to_path_buf()).join(relative);
        if !source.starts_with(std::fs::canonicalize(".").map_err(|e| e.to_string())?) {
            return Err("assert".into());
        }
        let bytes = std::fs::read(&source).map_err(|e| e.to_string())?;
        if sha(&bytes) != *pin { return Err("assert".into()); }
        if let Some(parent) = dest.parent() { std::fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
        if !dest.exists() {
            use std::io::Write;
            std::fs::OpenOptions::new().write(true).create_new(true).open(&dest)
                .map_err(|e| e.to_string())?.write_all(&bytes).map_err(|e| e.to_string())?;
        }
        if sha(&std::fs::read(&dest).map_err(|e| e.to_string())?) != *pin {
            return Err("assert".into());
        }
    }
    let prepared_file = format!("{runtime}/median-cap-revision-prepared-20261006.json");
    if !install {
        let next: V = serde_json::from_str(&canonical_json(&json!({
            "schemaVersion": 2, "mode": "baal_historical_median_production_v2",
            "tournamentId": prior_bank["tournamentId"], "universeHash": prior_bank["universeHash"],
            "cutoff": prior_bank["cutoff"], "paperBinding": prior_bank["paperBinding"],
            "sourcePrefix": prior_bank["sourcePrefix"], "policy": median_execution_policy_v2(),
            "codeIdentity": median_controller_v2_identity(workspace)?,
            "legacyCompatibility": {
                "priorBankHash": activation["strategyBankHash"],
                "priorPolicyHash": payload_hash(&prior_bank["policy"]),
                "priorCodeIdentityHash": payload_hash(&prior_bank["codeIdentity"]),
            },
        }))?).map_err(|e| e.to_string())?;
        let bank_hash = digest(&next);
        let bank_file = format!("{runtime}/production-banks/{bank_hash}.json");
        if !exists(&bank_file) { atomic_json(Path::new(&bank_file), &next)?; }
        if digest(&read_json(Path::new(&bank_file))?) != bank_hash { return Err("assert".into()); }
        atomic_json(Path::new(&prepared_file), &json!({
            "schemaVersion": 1, "activationHash": payload_hash(&activation),
            "priorBankHash": activation["strategyBankHash"], "nextBankHash": bank_hash,
            "at": ms_to_iso(now_ms() as f64),
        }))?;
        return Ok(json!({
            "prepared": true, "nextBankHash": bank_hash,
            "originalActivationUnchanged": true, "ordersSubmitted": 0,
        }));
    }
    let stop_file = format!("{runtime}/COLLECTOR_STOP");
    if read_json(Path::new(&stop_file))?["source"] != json!("median-cap-v2-controlled-install-20261006") {
        return Err("assert".into());
    }
    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
    if exists(&format!("{local}/SigTradingCup/bot.lock")) { return Err("assert".into()); }
    let dir = format!("{}/competition-runs/{}", runtime, activation["runId"].as_str().unwrap_or(""));
    if exists(&format!("{dir}/execution-owner.lock")) { return Err("assert".into()); }
    let previous_pid = read_json(Path::new("data/current-runtime-pids.json"))?["collector"].as_i64().unwrap_or(-1);
    if pid_alive(previous_pid) {
        return Err("Prior collector must be stopped, not inferred stopped from saved reports".into());
    }
    let core = Journal::open(Path::new(&format!("{dir}/core-journal.jsonl")))?;
    let proposals = Journal::open(Path::new(&format!("{dir}/median-proposals.jsonl")))?;
    if core.last.is_none() || proposals.last.is_none() { return Err("assert".into()); }
    let core_last = core.last.clone().unwrap();
    if !core_last["state"]["intents"].as_object().map(|m| m.values().all(|i| i["status"] == json!("closed"))).unwrap_or(false) {
        return Err("assert".into());
    }
    if !core_last["state"]["control"]["cancellations"].as_object()
        .map(|m| m.values().all(|x| x["phase"] == json!("reconciled"))).unwrap_or(false)
    {
        return Err("assert".into());
    }
    let proposals_last = proposals.last.clone().unwrap();
    if !proposals_last["state"]["proposals"].as_object().map(|m| m.values().all(|p| {
        p["status"] != json!("proposed")
            || baal_service_runtime::contract::parse_iso_ms(p["expiresAt"].as_str().unwrap_or("")).unwrap_or(0.0) <= now_ms() as f64
    })).unwrap_or(false) {
        return Err("assert".into());
    }
    let prepared = read_json(Path::new(&prepared_file))?;
    if prepared["activationHash"] != json!(payload_hash(&activation)) { return Err("assert".into()); }
    if prepared["priorBankHash"] != activation["strategyBankHash"] { return Err("assert".into()); }
    let proposal_bytes = std::fs::read(&proposals.file).map_err(|e| e.to_string())?;
    let revision = json!({
        "schemaVersion": 1, "kind": "same_run_median_entry_cap_revision",
        "revisionId": "median-entry-cap-20261006-v2", "ownerId": "Codex",
        "runId": activation["runId"], "tournamentId": activation["tournamentId"],
        "activationHash": payload_hash(&activation), "priorBankHash": prepared["priorBankHash"],
        "nextBankHash": prepared["nextBankHash"], "recordedAt": ms_to_iso(now_ms() as f64),
        "coreCheckpoint": {
            "sequence": core.sequence, "hash": core.previous,
            "stateHash": payload_hash(&core_last["state"]),
        },
        "proposalCheckpoint": {
            "sequence": proposals.sequence, "hash": proposals.previous,
            "stateHash": payload_hash(&proposals_last["state"]),
            "bytes": proposal_bytes.len(), "sha256": sha(&proposal_bytes),
        },
    });
    let revision_file = format!("{dir}/median-entry-cap-revision.json");
    if exists(&revision_file) { return Err("assert".into()); }
    atomic_json(Path::new(&revision_file), &revision)?;
    let next_bank = read_json(Path::new(&format!(
        "{}/production-banks/{}.json", runtime, prepared["nextBankHash"].as_str().unwrap_or("")
    )))?;
    validate_median_revision(&revision, &next_bank, &prior_bank, &activation, &universe,
        workspace, Path::new(&archive_dir), now_ms())?;
    atomic_json(Path::new("data/median-cap-revision-evidence-20261006.json"), &json!({
        "schemaVersion": 1, "installedAt": revision["recordedAt"], "kind": revision["kind"],
        "revisionId": revision["revisionId"], "runId": revision["runId"],
        "priorBankHash": revision["priorBankHash"], "nextBankHash": revision["nextBankHash"],
        "coreCheckpoint": revision["coreCheckpoint"], "proposalCheckpoint": revision["proposalCheckpoint"],
        "revisionHash": payload_hash(&revision), "activationUnchanged": true,
        "proposalSealingVerified": true, "ordersSubmitted": 0,
        "profitabilityEstablished": false,
    }))?;
    if std::fs::read(&activation_file).map_err(|e| e.to_string())? != activation_bytes {
        return Err("assert".into());
    }
    Ok(json!({
        "installed": true, "revisionId": revision["revisionId"],
        "nextBankHash": revision["nextBankHash"], "ordersSubmitted": 0,
    }))
}
fn pid_alive(pid: i64) -> bool {
    if pid <= 0 { return false; }
    std::process::Command::new("kill").arg("-0").arg(pid.to_string())
        .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
        .status().map(|s| s.success()).unwrap_or(false)
}
