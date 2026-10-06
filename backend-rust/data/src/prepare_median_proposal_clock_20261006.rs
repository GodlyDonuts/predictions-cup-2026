// data/prepare-median-proposal-clock-20261006.mjs — reset the proposal window
// clock on the untouched production journals, preserving every prior page.
use crate::publication_pack::{now_ms, read_json, runtime_dir};
use baal_collector::breadth::validate_universe;
use baal_collector::model::digest;
use baal_execution::core::payload_hash;
use baal_execution::median_controller::validate_median_bank;
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::journal::Journal;
use serde_json::{json, Value as V};
use std::path::Path;

pub fn run() -> Result<V, String> {
    let runtime = runtime_dir();
    let workspace = Path::new(".");
    let activation = read_json(Path::new(&format!("{runtime}/competition-activation.json")))?;
    let universe = validate_universe(
        &read_json(Path::new("live-paper/price-universe.json"))?,
        activation["tournamentId"].as_str().unwrap_or(""),
    )?;
    let prior_bank = read_json(Path::new(&format!(
        "{}/production-banks/{}.json", runtime, activation["strategyBankHash"].as_str().unwrap_or("")
    )))?;
    validate_median_bank(&prior_bank, &activation, &universe, workspace)?;
    let dir = format!("{}/competition-runs/{}", runtime, activation["runId"].as_str().unwrap_or(""));
    let core = Journal::open(Path::new(&format!("{dir}/core-journal.jsonl")))?;
    let proposals = Journal::open(Path::new(&format!("{dir}/median-proposals.jsonl")))?;
    if core.last.is_none() || proposals.last.is_none() {
        return Err("Journals must be nonempty".into());
    }
    let proposals_last = proposals.last.clone().unwrap();
    let pending: Vec<String> = proposals_last["state"]["proposals"].as_object()
        .map(|m| m.iter()
            .filter(|(_, p)| p["status"] == json!("proposed"))
            .map(|(id, _)| id.clone()).collect())
        .unwrap_or_default();
    if !pending.is_empty() {
        return Err(format!("PROPOSALS_STILL_OPEN:{}", pending.join(",")));
    }
    let started_at = ms_to_iso(now_ms() as f64);
    let clock = json!({
        "schemaVersion": 1, "kind": "same_run_median_proposal_window_reset",
        "ownerId": "Codex", "runId": activation["runId"], "tournamentId": activation["tournamentId"],
        "activationHash": payload_hash(&activation), "bankHash": activation["strategyBankHash"],
        "priorProposalSequence": proposals.sequence, "priorProposalHash": proposals.previous,
        "priorProposalStateHash": payload_hash(&proposals_last["state"]),
        "openedAt": started_at,
        "expiresAt": ms_to_iso(now_ms() as f64 + 3_600_000.0),
        "reason": "fresh_bounded_proposal_window_after_all_prior_proposals_resolved",
    });
    let file = format!("{dir}/median-proposal-clock-20261006.json");
    if Path::new(&file).exists() { return Err("Never overwrite a recorded clock".into()); }
    atomic_json(Path::new(&file), &clock)?;
    let evidence = json!({
        "schemaVersion": 1, "configuredAt": started_at, "runId": activation["runId"],
        "clockHash": payload_hash(&clock), "expiresAt": clock["expiresAt"],
        "bankHash": activation["strategyBankHash"], "bankFileHash": digest(&prior_bank),
        "priorProposalsResolved": true, "journalsPreserved": true,
        "ordersSubmitted": 0, "extraAccountClients": 0, "newCredentials": 0,
        "activationUnchanged": true, "profitabilityEstablished": false,
    });
    atomic_json(Path::new("data/median-proposal-clock-20261006.json"), &evidence)?;
    Ok(evidence)
}
