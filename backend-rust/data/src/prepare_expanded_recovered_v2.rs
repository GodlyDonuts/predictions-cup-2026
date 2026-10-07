// data/prepare-expanded-recovered-v2.mjs — stage the recovered capital
// expansion: rebuild the expansion recovered custody seed v2 and stage the
// baal-expansion-live-request-v2 request from the v1 request's bounds.
use crate::baal_custody_frontier_v1::read_baal_custody_frontier_v1;
use crate::baal_expansion_recovered_seed_v2::build_baal_expansion_recovered_seed_v2;
use crate::baal_generic_live_host_v8::make_baal_generic_live_request_v8;
use crate::publication_pack::{read_json, runtime_dir};
use baal_collector::breadth::validate_universe;
use baal_execution::core::payload_hash;
use baal_service_runtime::atomic::atomic_json_canonical;
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
    let old = read_json(Path::new(&format!("{rt}/baal-expansion-live-request-v1.json")))?;
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let frontier = read_baal_custody_frontier_v1(&json!({
        "runtimeDir": rt,
        "workspace": cwd.to_string_lossy(),
    }))?;
    let seed = build_baal_expansion_recovered_seed_v2(
        &frontier,
        &old["scope"],
        &old["allocation"],
        Path::new(&rt),
    )?;
    let universe = validate_universe(
        &read_json(Path::new("live-paper/price-universe.json"))?,
        old["tournamentId"].as_str().unwrap_or(""),
    )?;
    let r = make_baal_generic_live_request_v8(
        &cwd,
        Path::new(&rt),
        &universe,
        &frontier,
        "expanded-recovered-20261007-root",
        old["activation"]["startedAt"].as_str().unwrap_or(""),
        old["activation"]["entryExpiresAt"].as_str().unwrap_or(""),
    )?;
    assert_eq(&seed["summary"]["inheritedOrders"], &json!(24))?;
    assert_eq(
        &r["activation"]["exitExpiresAt"],
        &old["activation"]["exitExpiresAt"],
    )?;
    assert_eq(&r["allocation"]["capital"], &json!("10000.000000"))?;
    assert(!Path::new(&rt)
        .join("baal-expansion-live-request-v2.json")
        .exists())?;
    atomic_json_canonical(
        &Path::new(&rt).join("baal-expansion-live-request-v2.staged.json"),
        &r,
    )?;
    Ok(json!({
        "staged": true,
        "stopUnchanged": true,
        "inheritedClosed": 24,
        "capital": r["allocation"]["capital"],
        "entryExpiresAt": r["activation"]["entryExpiresAt"],
        "exitExpiresAt": r["activation"]["exitExpiresAt"],
        "requestHash": payload_hash(&r)?,
    }))
}
