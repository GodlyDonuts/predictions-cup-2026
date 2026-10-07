// data/prepare-baal-expansion-v1.mjs — stage the direct-human capital
// expansion: seal baal-expansion-authority-v1.json, rebuild the expansion
// custody seed, and stage the baal-expansion-live-request-v1 request.
use crate::baal_custody_frontier_v1::read_baal_custody_frontier_v1;
use crate::baal_expansion_custody_seed_v1::build_baal_expansion_custody_seed_v1;
use crate::baal_generic_live_host_v7::make_baal_generic_live_request_v7;
use crate::publication_pack::{now_ms, read_json, runtime_dir};
use baal_collector::breadth::validate_universe;
use baal_execution::core::payload_hash;
use baal_service_runtime::atomic::atomic_json_canonical;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Value as V};
use std::fs;
use std::path::Path;

fn assert_eq(a: &V, b: &V) -> Result<(), String> {
    if a == b { Ok(()) } else { Err("assert".into()) }
}
fn assert(ok: bool) -> Result<(), String> {
    if ok { Ok(()) } else { Err("assert".into()) }
}
fn sha(b: &[u8]) -> String {
    sha256(b)
}

pub fn run() -> Result<V, String> {
    let rt = runtime_dir();
    let read = |p: &str| read_json(Path::new(p));
    let old = read(&format!("{rt}/cohort-profit-lock-request-v2.json"))?;
    let stat = read(&format!("{rt}/baal-scaled-live-stats.json"))?;
    assert_eq(&stat["controller"]["intent"], &V::Null)?;
    assert(
        stat["controller"]["inventory"]
            .as_object()
            .map(|m| m.values().all(|x| x["quantity"].as_i64() == Some(0)))
            .unwrap_or(false),
    )?;
    let prior_file = format!(
        "{rt}/baal-generic-live-runs/{}/{}/controller/baal-generic-no-v7-journal.jsonl",
        old["runId"].as_str().unwrap_or(""),
        old["epoch"].as_str().unwrap_or("")
    );
    let bytes = fs::read(&prior_file).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&bytes);
    let tail_line = text.trim_end().lines().last().unwrap_or("");
    let tail: V = serde_json::from_str(tail_line).map_err(|e| e.to_string())?;
    let started_at = ms_to_iso(now_ms() as f64);
    let human = "Since Baal is showing signs of working massively increase its capital and volume. And continue improving.";
    let universe = validate_universe(
        &read("live-paper/price-universe.json")?,
        old["tournamentId"].as_str().unwrap_or(""),
    )?;
    let mut scope = old["scope"].as_object().cloned().unwrap_or_default();
    scope.insert(
        "runId".into(),
        json!("baal-expanded-no-20261007-human"),
    );
    let scope = V::Object(scope);
    let mut allocation = old["allocation"].as_object().cloned().unwrap_or_default();
    allocation.insert("runId".into(), scope["runId"].clone());
    allocation.insert("capital".into(), json!("10000.000000"));
    allocation.insert("maximumOrderCost".into(), json!("100.000000"));
    allocation.insert("maximumMarketCost".into(), json!("1000.000000"));
    allocation.insert("maximumEventCost".into(), json!("2000.000000"));
    allocation.insert("maximumActiveMarkets".into(), json!(100));
    allocation.insert(
        "humanAllocationSourceHash".into(),
        json!(sha(human.as_bytes())),
    );
    allocation.insert("recordedAt".into(), json!(started_at));
    let allocation = V::Object(allocation);
    let start_ms = parse_iso_ms(&started_at).ok_or("INVALID_INSTANT")?;
    let activation_bounds = json!({
        "authorityHash": sha(human.as_bytes()),
        "startedAt": started_at,
        "entryExpiresAt": ms_to_iso(start_ms + 4.0 * 3600000.0),
        "exitExpiresAt": ms_to_iso(start_ms + 24.0 * 3600000.0),
    });
    let file = Path::new(&rt).join("baal-expansion-authority-v1.json");
    if file.exists() {
        return Err("NEW_AUTHORITY_ONLY".into());
    }
    atomic_json_canonical(
        &file,
        &json!({
            "schemaVersion": 1,
            "kind": "direct_human_baal_expansion_v1",
            "scope": scope,
            "allocation": allocation,
            "activationBounds": activation_bounds,
            "priorActor": {
                "relative": prior_file
                    .strip_prefix(&format!("{rt}/"))
                    .unwrap_or(&prior_file)
                    .to_string(),
                "bytes": bytes.len(),
                "sha256": sha(&bytes),
                "hash": tail["hash"],
            },
            "previousCapital": "1000.000000",
            "previousRealizedProfit": stat["controller"]["realizedPnl"],
        }),
    )?;
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let frontier = read_baal_custody_frontier_v1(&json!({
        "runtimeDir": rt,
        "workspace": cwd.to_string_lossy(),
    }))?;
    let seed = build_baal_expansion_custody_seed_v1(
        &frontier,
        &scope,
        &allocation,
        Path::new(&rt),
    )?;
    let request = make_baal_generic_live_request_v7(
        &cwd,
        Path::new(&rt),
        &universe,
        &frontier,
        "expanded-20261007-root",
        &started_at,
        activation_bounds["entryExpiresAt"].as_str().unwrap_or(""),
    )?;
    assert_eq(
        &json!(seed["inheritedClosedOrders"].as_array().map(|a| a.len()).unwrap_or(0)),
        &json!(23),
    )?;
    assert_eq(&seed["summary"]["realizedPnl"], &json!("0.000000"))?;
    assert_eq(
        &json!(seed["inventory"].as_object().map(|m| m.len()).unwrap_or(0)),
        &json!(0),
    )?;
    atomic_json_canonical(
        &Path::new(&rt).join("baal-expansion-live-request-v1.staged.json"),
        &request,
    )?;
    Ok(json!({
        "staged": true,
        "activated": false,
        "capital": allocation["capital"],
        "maximumOrderCost": allocation["maximumOrderCost"],
        "inheritedClosedIntents": 23,
        "previousRealizedPreserved": stat["controller"]["realizedPnl"],
        "newRunRealized": "0.000000",
        "startedAt": started_at,
        "entryExpiresAt": activation_bounds["entryExpiresAt"],
        "exitExpiresAt": activation_bounds["exitExpiresAt"],
        "requestHash": payload_hash(&request)?,
    }))
}
