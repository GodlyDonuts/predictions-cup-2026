// data/prepare-baal-scaled-live-20261006.mjs — verify the retired collector and
// legacy scaled risk are fully flat, then seal the scaled live request, root
// catalog and collector code identity with canonical atomic writes.
use crate::baal_scaled_legacy_risk_v1::read_baal_scaled_legacy_risk;
use crate::baal_scaled_live_host_v1::baal_scaled_live_identity;
use crate::publication_pack::{now_ms, read_json, runtime_dir};
use baal_collector::breadth::validate_universe;
use baal_collector::collector_median_discovery_v3::median_discovery_identity;
use baal_execution::core::{create_execution_state, payload_hash};
use baal_service_runtime::atomic::atomic_json_canonical;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::{js_int, js_truthy};
use serde_json::{json, Value as V};
use std::path::Path;

const TID: &str = "bda92870-621e-47b0-bc3c-3602c5c26f55";
fn assert_eq(a: &V, b: &V) -> Result<(), String> {
    if a == b { Ok(()) } else { Err("assert".into()) }
}
fn assert(ok: bool) -> Result<(), String> {
    if ok { Ok(()) } else { Err("assert".into()) }
}

pub fn run() -> Result<V, String> {
    let rt = runtime_dir();
    let read = |p: &str| read_json(Path::new(p));
    let stop = read(&format!("{rt}/COLLECTOR_STOP"))?;
    assert_eq(&stop["source"], &json!("baal-scaled-capital-install-20261006"))?;
    // process.kill(stop.oldPid, 0) must throw ESRCH — any surviving old
    // process, EPERM or an invalid pid all fail the same assert.
    let esrch = match js_int(&stop["oldPid"]) {
        Some(p) if p > 0 && p <= i32::MAX as i64 => {
            let r = unsafe { libc::kill(p as i32, 0) };
            r != 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        }
        _ => false,
    };
    assert(esrch)?;
    assert(
        !Path::new(&format!("{rt}/../bot.lock")).exists()
            && !Path::new("STOP").exists()
            && !Path::new(&format!("{rt}/STOP")).exists()
            && !Path::new(&format!("{rt}/EXECUTION_STOP")).exists(),
    )?;
    let old = read(&format!("{rt}/owned-no-profit-stats.json"))?;
    assert_eq(&old["phase"], &json!("retired"))?;
    assert_eq(&old["realizedPnl"], &json!("0.010000"))?;
    assert_eq(&old["currentBudget"]["ownedPrincipal"], &json!("0.000000"))?;
    assert_eq(&old["currentBudget"]["reservedCash"], &json!("0.000000"))?;
    let universe = validate_universe(&read("live-paper/price-universe.json")?, TID)?;
    let human = read(&format!("{rt}/baal-capital-allocation.json"))?;
    assert_eq(&human["allocatedNominalCapital"], &json!("1000.000000"))?;
    assert_eq(&human["maximumOrderNominalCost"], &json!("25.000000"))?;
    let audited = read_baal_scaled_legacy_risk(Path::new(&rt), || create_execution_state(TID, None))?;
    assert(js_truthy(audited.get("complete")))?;
    let prior_catalog = read(&format!("{rt}/party-pair-root-catalog.json"))?;
    let catalog = json!({
        "schemaVersion": 1,
        "tournamentId": TID,
        "universeHash": universe["hash"].clone(),
        "rootCaptures": prior_catalog["rootCaptures"].clone(),
    });
    atomic_json_canonical(Path::new(&format!("{rt}/baal-scaled-rootcatalog.json")), &catalog)?;
    let started_ms = now_ms();
    let started_at = ms_to_iso(started_ms as f64);
    let allowed_markets: Vec<V> = universe["exchanges"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|m| m["option"] == json!("YES"))
                .map(|m| m["marketId"].clone())
                .collect()
        })
        .unwrap_or_default();
    let request = json!({
        "schemaVersion": 1,
        "kind": "baal_scaled_live_production_v1",
        "ownerId": "Codex",
        "runId": "baal-scaled-no-20261006-human",
        "tournamentId": TID,
        "tournamentSlug": "midterm-elections",
        "universeHash": universe["hash"].clone(),
        "allowedMarkets": allowed_markets,
        "humanAllocationSourceHash": payload_hash(&human)?,
        "authorityHash": payload_hash(&read(&format!("{rt}/paired-live-authority.json"))?)?,
        "catalogHash": payload_hash(&catalog)?,
        "codeIdentity": baal_scaled_live_identity(&std::env::current_dir().map_err(|e| e.to_string())?)?,
        "startedAt": started_at,
        "entryExpiresAt": ms_to_iso(started_ms as f64 + 3_600_000.0),
        "maximumActiveMarkets": 20,
    });
    assert(!Path::new(&format!("{rt}/baal-scaled-live-request.json")).exists())?;
    atomic_json_canonical(Path::new(&format!("{rt}/baal-scaled-live-request.json")), &request)?;
    let config_file = format!("{rt}/median-discovery.json");
    let mut config = read(&config_file)?;
    std::fs::copy(&config_file, format!("{config_file}.before-scaled-live-20261006"))
        .map_err(|e| e.to_string())?;
    let code_identity = median_discovery_identity(&std::env::current_dir().map_err(|e| e.to_string())?)?;
    config.as_object_mut().ok_or("assert")?.insert("codeIdentity".into(), code_identity);
    atomic_json_canonical(Path::new(&config_file), &config)?;
    let out = json!({
        "schemaVersion": 1,
        "configuredAt": started_at,
        "runId": request["runId"].clone(),
        "requestHash": payload_hash(&request)?,
        "entryExpiresAt": request["entryExpiresAt"].clone(),
        "capital": "1000.000000",
        "maximumOrderCost": "25.000000",
        "maximumMarketCost": "100.000000",
        "maximumEventCost": "200.000000",
        "maximumSharesPerOrder": 100,
        "maximumActiveMarkets": 20,
        "allowedMarkets": request["allowedMarkets"].as_array().map(|a| a.len() as i64).unwrap_or(0),
        "rootCaptures": catalog["rootCaptures"].as_array().map(|a| a.len() as i64).unwrap_or(0),
        "fees": "0.000000",
        "newClients": 0,
        "newCredentials": 0,
        "oldHistoryPreserved": true,
        "oldWindowsRenewed": false,
        "liveAccountReconciliationStillRequired": true,
        "collectorGraphFiles": config["codeIdentity"].as_object().map(|o| o.len() as i64).unwrap_or(0),
    });
    atomic_json_canonical(Path::new("data/baal-scaled-live-configuration-20261006.json"), &out)?;
    Ok(out)
}
