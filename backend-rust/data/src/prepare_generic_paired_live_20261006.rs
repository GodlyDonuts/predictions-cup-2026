// data/prepare-generic-paired-live-20261006.mjs — retire the expired pair
// request and install the generic allowed-universe pair request.
use crate::paired_live_host_v2::paired_live_v2_identity;
use crate::publication_pack::{now_ms, read_json, runtime_dir};
use baal_collector::breadth::validate_universe;
use baal_collector::collector_median_discovery_v3::median_discovery_identity;
use baal_execution::core::payload_hash;
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use serde_json::{json, Value as V};
use std::path::Path;

fn assert_eq(a: &V, b: &V) -> Result<(), String> {
    if a == b { Ok(()) } else { Err("assert".into()) }
}
pub fn run() -> Result<V, String> {
    let rt = runtime_dir();
    let read = |p: &str| read_json(Path::new(p));
    let marker = read(&format!("{rt}/COLLECTOR_STOP"))?;
    assert_eq(&marker["source"], &json!("generic-pair-selector-install-20261006"))?;
    if Path::new(&format!("{rt}/../bot.lock")).exists() || Path::new("STOP").exists()
        || Path::new(&format!("{rt}/STOP")).exists()
        || Path::new(&format!("{rt}/EXECUTION_STOP")).exists()
    {
        return Err("assert".into());
    }
    let old_file = format!("{rt}/paired-live-request.json");
    let old = read(&old_file)?;
    if now_ms() < parse_iso_ms(old["entryExpiresAt"].as_str().unwrap_or("")).unwrap_or(0.0) as i64 {
        return Err("assert".into());
    }
    let pair = read(&format!("{rt}/paired-live-stats.json"))?;
    let original = read(&format!("{rt}/competition-stats.json"))?;
    assert_eq(&json!(pair["orders"].as_array().map(|a| a.len()).unwrap_or(0)), &json!(0))?;
    assert_eq(&json!(pair["owned"].as_array().map(|a| a.len()).unwrap_or(0)), &json!(0))?;
    assert_eq(&original["unconfirmedIntents"], &json!(0))?;
    assert_eq(&original["openIntent"], &V::Null)?;
    assert_eq(&json!(original["positions"].as_object().map(|o| o.len()).unwrap_or(0)), &json!(0))?;
    if Path::new(&format!("{rt}/paired-live-v2-request.json")).exists() { return Err("assert".into()); }
    let archive = format!("{old_file}.expired-zero-order-before-generic-20261006");
    if Path::new(&archive).exists() { return Err("assert".into()); }
    std::fs::rename(&old_file, &archive).map_err(|e| e.to_string())?;
    let universe = validate_universe(&read("live-paper/price-universe.json")?, old["tournamentId"].as_str().unwrap_or(""))?;
    let source = read("data/party-winner-no-source-screen-20261006T105231940Z.local.json")?;
    assert_eq(&source["universe"]["hash"], &universe["hash"])?;
    let catalog = json!({
        "schemaVersion": 1, "universeHash": universe["hash"],
        "sourceObservedAt": source["now"], "rootCaptures": source["rootCaptures"],
    });
    atomic_json(Path::new(&format!("{rt}/party-pair-root-catalog.json")), &catalog)?;
    let started_at = ms_to_iso(now_ms() as f64);
    let started_ms = now_ms() as f64;
    let authority = read(&format!("{rt}/paired-live-authority.json"))?;
    let allowed: Vec<V> = universe["exchanges"].as_array().map(|a| {
        a.iter().filter(|x| x["option"] == json!("YES"))
            .map(|x| json!({"marketId": x["marketId"], "exchangeId": x["exchangeId"]})).collect()
    }).unwrap_or_default();
    let request = json!({
        "schemaVersion": 2, "mode": "one_pair_from_allowed_markets_v2", "enabled": true,
        "ownerId": "Codex", "runId": "sig-pilot-paired-generic-20261006-human",
        "tournamentId": old["tournamentId"], "tournamentSlug": old["tournamentSlug"],
        "universeHash": universe["hash"], "allowedMarkets": allowed,
        "startedAt": started_at, "entryExpiresAt": ms_to_iso(started_ms + 3_600_000.0),
        "authorityHash": payload_hash(&authority),
        "codeIdentity": paired_live_v2_identity(Path::new(".")),
    });
    atomic_json(Path::new(&format!("{rt}/paired-live-v2-request.json")), &request)?;
    let source_plan = json!({
        "schemaVersion": 1, "owner": "Codex", "id": "generic-pair-read-plan-20261006-v1",
        "mode": "allowed_universe_pair_read_candidates", "universeHash": universe["hash"],
        "createdAt": started_at, "expiresAt": ms_to_iso(started_ms + 1_800_000.0),
        "protectedBookSlots": 4, "maximumAuxiliarySlots": 2,
        "rootCatalogHash": payload_hash(&catalog["rootCaptures"]),
    });
    atomic_json(Path::new(&format!("{rt}/party-pair-source-plan.json")), &source_plan)?;
    let config_file = format!("{rt}/median-discovery.json");
    let mut config = read(&config_file)?;
    std::fs::copy(&config_file, format!("{config_file}.before-generic-pair-20261006")).map_err(|e| e.to_string())?;
    config.as_object_mut().unwrap().insert("codeIdentity".into(), median_discovery_identity(Path::new("."))?);
    atomic_json(Path::new(&config_file), &config)?;
    let evidence = json!({
        "schemaVersion": 1, "configuredAt": started_at, "retiredRun": old["runId"],
        "retiredExpiry": old["entryExpiresAt"], "retiredOrders": 0,
        "retiredJournalsPreserved": true, "newRun": request["runId"],
        "entryExpiresAt": request["entryExpiresAt"], "sourceReadExpiresAt": source_plan["expiresAt"],
        "authorityHash": request["authorityHash"],
        "allowedMarkets": request["allowedMarkets"].as_array().map(|a| a.len()).unwrap_or(0),
        "rootCaptures": catalog["rootCaptures"].as_array().map(|a| a.len()).unwrap_or(0),
        "allocation": "2.000000", "maxPairs": 1, "quantityPerLeg": 1,
        "pairCeiling": "0.990000", "minimumOrdinaryMargin": "0.010000",
        "bookSlots": {"ordinary": 4, "auxiliary": 2},
        "extraAccountClients": 0, "newCredentials": 0, "ordersSubmitted": 0,
    });
    atomic_json(Path::new("data/generic-paired-live-configuration-20261006.json"), &evidence)?;
    Ok(evidence)
}
