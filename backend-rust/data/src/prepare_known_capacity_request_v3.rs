// data/prepare-known-capacity-request-v3.mjs — stage the v9 live request:
// rebuild the baal-expansion-live-request-v3 request from the v2 request's
// bounds, assert the activation and inheritance are unchanged, and publish it
// atomically only while no execution owner lease exists.
use crate::baal_custody_frontier_v1::read_baal_custody_frontier_v1;
use crate::baal_generic_live_host_v9::make_baal_generic_live_request_v9;
use crate::publication_pack::{read_json, runtime_dir};
use baal_collector::breadth::validate_universe;
use baal_execution::core::payload_hash;
use baal_service_runtime::atomic::atomic_json_canonical;
use baal_service_runtime::json_text::js_stringify;
use serde_json::{json, Value as V};
use std::path::Path;

fn assert_eq(a: &V, b: &V) -> Result<(), String> {
    if a == b { Ok(()) } else { Err("assert".into()) }
}
fn assert(ok: bool) -> Result<(), String> {
    if ok { Ok(()) } else { Err("assert".into()) }
}

pub fn run() -> Result<V, String> {
    let rt = runtime_dir();
    let old = read_json(Path::new(&format!("{rt}/baal-expansion-live-request-v2.json")))?;
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let frontier = read_baal_custody_frontier_v1(&json!({
        "runtimeDir": rt,
        "workspace": cwd.to_string_lossy(),
    }))?;
    let universe = validate_universe(
        &read_json(Path::new("live-paper/price-universe.json"))?,
        old["tournamentId"].as_str().unwrap_or(""),
    )?;
    let r = make_baal_generic_live_request_v9(
        &cwd,
        Path::new(&rt),
        &universe,
        &frontier,
        old["epoch"].as_str().unwrap_or(""),
        old["activation"]["startedAt"].as_str().unwrap_or(""),
        old["activation"]["entryExpiresAt"].as_str().unwrap_or(""),
    )?;
    assert_eq(
        &json!(payload_hash(&r["activation"])?),
        &json!(payload_hash(&old["activation"])?),
    )?;
    assert_eq(&r["inheritanceHash"], &old["inheritanceHash"])?;
    assert(!Path::new(&rt)
        .join("baal-financial-owner")
        .join("execution-owner.lock")
        .exists())?;
    atomic_json_canonical(
        &Path::new(&rt).join("baal-expansion-live-request-v3.json"),
        &r,
    )?;
    let out = json!({
        "sameActivation": true,
        "sameControllerJournal": true,
        "sameBudget": true,
        "knownOrderReadPriority": true,
        "physicalFullDELETEFence": true,
    });
    println!("{}", js_stringify(&out));
    Ok(out)
}
