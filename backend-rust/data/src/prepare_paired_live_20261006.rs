// data/prepare-paired-live-20261006.mjs — activate the bounded paired-live
// request after the revised median bank validates.
use crate::paired_live_host::paired_live_identity;
use crate::publication_pack::{now_ms, read_json, runtime_dir};
use baal_collector::breadth::validate_universe;
use baal_collector::collector_median_discovery_v3::median_discovery_identity;
use baal_execution::core::payload_hash;
use baal_execution::median_controller_v2::validate_median_revision;
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::journal::Journal;
use serde_json::{json, Value as V};
use std::path::Path;

fn assert_eq(a: &V, b: &V) -> Result<(), String> {
    if a == b { Ok(()) } else { Err("assert".into()) }
}
pub fn run() -> Result<V, String> {
    let runtime = runtime_dir();
    let read = |p: &str| read_json(Path::new(p));
    let marker = read(&format!("{runtime}/COLLECTOR_STOP"))?;
    assert_eq(&marker["source"], &json!("paired-rest-host-install-20261006"))?;
    if Path::new(&format!("{runtime}/../bot.lock")).exists() || Path::new("STOP").exists()
        || Path::new(&format!("{runtime}/STOP")).exists()
        || Path::new(&format!("{runtime}/EXECUTION_STOP")).exists()
    {
        return Err("assert".into());
    }
    let activation_file = format!("{runtime}/competition-activation.json");
    let activation_bytes = std::fs::read(&activation_file).map_err(|e| e.to_string())?;
    let activation: V = serde_json::from_slice(&activation_bytes).map_err(|e| e.to_string())?;
    let universe = validate_universe(
        &read("live-paper/price-universe.json")?,
        activation["tournamentId"].as_str().unwrap_or(""),
    )?;
    let dir = format!("{}/competition-runs/{}", runtime, activation["runId"].as_str().unwrap_or(""));
    if Path::new(&format!("{dir}/execution-owner.lock")).exists() { return Err("assert".into()); }
    let core = Journal::open(Path::new(&format!("{dir}/core-journal.jsonl")))?;
    let core_last = core.last.clone().unwrap_or(V::Null);
    if !core_last["state"]["intents"].as_object().map(|m| m.values().all(|i| i["status"] == json!("closed"))).unwrap_or(false) {
        return Err("assert".into());
    }
    assert_eq(&json!(core_last["state"]["owned"].as_object().map(|o| o.len()).unwrap_or(0)), &json!(0))?;
    let revision = read(&format!("{dir}/median-entry-cap-revision.json"))?;
    validate_median_revision(&revision,
        &read(&format!("{}/production-banks/{}.json", runtime, revision["nextBankHash"].as_str().unwrap_or("")))?,
        &read(&format!("{}/production-banks/{}.json", runtime, activation["strategyBankHash"].as_str().unwrap_or("")))?,
        &activation, &universe, Path::new("."),
        Path::new(&format!("{}/production-code/{}", runtime, activation["strategyBankHash"].as_str().unwrap_or(""))),
        now_ms())?;
    let request_file = format!("{runtime}/paired-live-request.json");
    if Path::new(&request_file).exists() {
        return Err("Existing paired activation must not be replaced".into());
    }
    let started_at = ms_to_iso(now_ms() as f64);
    let started_ms = now_ms() as f64;
    let authority = json!({
        "schemaVersion": 1, "kind": "direct_human_authorized_bot_update", "ownerId": "Codex",
        "threadId": "01a0f8cc-b9bb-77d2-9f4c-2844396f1b79",
        "humanProductionInstruction": "If it is useful evidence, why dont we let it run now in production. And we continue to find improvements in paper and everytime we do we push it to prod",
        "humanStrategyInstruction": "If you think of a better strategy than what i told you, go for it. Your only job is to make the best bot.",
        "existingHumanAuditHash": payload_hash(&read(&format!("{runtime}/median-human-activation-authority-20261006.json"))?),
        "recordedByCodexAt": started_at, "exploratory": true, "profitabilityEstablished": false,
        "purpose": "One small prospective opposing-party NO pair under existing human production/update authority",
        "allocatedCapital": "2.000000", "quantity": 1, "maxPairs": 1,
        "ordinaryResolutionOnly": true, "exceptionalLossAcknowledged": true,
    });
    atomic_json(Path::new(&format!("{runtime}/paired-live-authority.json")), &authority)?;
    let mut markets = Vec::new();
    for market_id in ["354", "355"] {
        let e = universe["exchanges"].as_array().map(|a| {
            a.iter().find(|x| x["marketId"] == json!(market_id)).cloned()
        }).flatten();
        let e = e.ok_or("assert")?;
        if e["option"] != json!("YES") { return Err("assert".into()); }
        markets.push(json!({"marketId": market_id, "exchangeId": e["exchangeId"]}));
    }
    let request = json!({
        "schemaVersion": 1, "enabled": true, "ownerId": "Codex",
        "runId": "sig-pilot-paired-20261006-human", "tournamentId": activation["tournamentId"],
        "tournamentSlug": "midterm-elections", "markets": markets,
        "startedAt": started_at, "entryExpiresAt": ms_to_iso(started_ms + 3_600_000.0),
        "authorityHash": payload_hash(&authority), "codeIdentity": paired_live_identity(Path::new(".")),
    });
    atomic_json(Path::new(&request_file), &request)?;
    atomic_json(Path::new(&format!("{runtime}/paired-candidate-request.json")), &json!({
        "schemaVersion": 1, "owner": "Codex", "name": "florida-senate-minpay-20261006-v2",
        "marketIds": markets.iter().map(|m| m["marketId"].clone()).collect::<Vec<V>>(),
        "createdAt": started_at, "expiresAt": ms_to_iso(started_ms + 1_800_000.0),
    }))?;
    atomic_json(Path::new(&format!("{runtime}/MEDIAN_ENTRY_DRAIN")), &json!({
        "source": "profitability-source-bound-pair-pivot-20261006", "recordedAt": started_at,
        "humanStop": false,
        "reason": "Preserve all original accounting and exits; park new unproven dip buys during the separately bounded pair trial.",
    }))?;
    let config_file = format!("{runtime}/median-discovery.json");
    let mut config = read(&config_file)?;
    std::fs::copy(&config_file, format!("{config_file}.before-paired-live-20261006")).map_err(|e| e.to_string())?;
    config.as_object_mut().unwrap().insert("codeIdentity".into(), median_discovery_identity(Path::new("."))?);
    atomic_json(Path::new(&config_file), &config)?;
    if std::fs::read(&activation_file).map_err(|e| e.to_string())? != activation_bytes {
        return Err("assert".into());
    }
    let evidence = json!({
        "schemaVersion": 1, "configuredAt": started_at, "runId": request["runId"],
        "requestHash": payload_hash(&request), "authorityHash": request["authorityHash"],
        "entryExpiresAt": request["entryExpiresAt"], "markets": markets,
        "allocatedCapital": "2.000000", "maxOrderCost": "1.000000", "quantity": 1,
        "maxPairs": 1, "minimumOrdinarySurplus": "0.010000", "pairCostCeiling": "0.990000",
        "ordinaryResolutionOnly": true, "originalActivationUnchanged": true,
        "originalEntryExpiresAt": activation["entryExpiresAt"],
        "originalNewEntriesDraining": true,
        "codeFiles": request["codeIdentity"].as_object().map(|o| o.len()).unwrap_or(0),
        "collectorCodeFiles": config["codeIdentity"].as_object().map(|o| o.len()).unwrap_or(0),
        "ordersSubmitted": 0, "newCredentials": 0, "extraAccountClients": 0,
    });
    atomic_json(Path::new("data/paired-live-configuration-20261006.json"), &evidence)?;
    Ok(evidence)
}
