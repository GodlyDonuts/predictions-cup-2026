// data/install-foreign-candidate-host-v11.local.mjs — patch the paper runner
// to dispatch the v11 host chain, refresh the median discovery codeIdentity,
// and stage the baal-expansion-live-request-v5 request from the v4 request's
// bounds. Asserts no execution owner lease is held first.
use crate::baal_custody_frontier_v1::read_baal_custody_frontier_v1;
use crate::baal_generic_live_host_v11::make_baal_generic_live_request_v11;
use crate::publication_pack::{read_json, runtime_dir};
use baal_collector::breadth::validate_universe;
use baal_collector::collector_median_discovery_v1::median_discovery_identity;
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
    assert(!Path::new(&rt)
        .join("baal-financial-owner")
        .join("execution-owner.lock")
        .exists())?;
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let mut s = String::from_utf8(
        std::fs::read(Path::new("live-paper/runner.mjs")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    s = format!(
        "import {{openBaalGenericLiveHostV11}} from '../execution/baal-generic-live-host-v11.mjs';\n{s}"
    );
    let old = "?openPhysicalGuardRecoveryHostV1:fs.existsSync(path.join(runtimeDir,'baal-expansion-live-request-v4.json'))?";
    assert(s.contains(old))?;
    s = s.replace(
        old,
        "?openPhysicalGuardRecoveryHostV1:fs.existsSync(path.join(runtimeDir,'baal-expansion-live-request-v5.json'))?openBaalGenericLiveHostV11:fs.existsSync(path.join(runtimeDir,'baal-expansion-live-request-v4.json'))?",
    );
    s = s.replace(
        "(fs.existsSync(path.join(runtimeDir,'baal-expansion-live-request-v4.json'))||fs.existsSync(path.join(runtimeDir,'baal-expansion-live-request-v3.json')))?createScopedRestReceiptsV7",
        "(fs.existsSync(path.join(runtimeDir,'baal-expansion-live-request-v5.json'))||fs.existsSync(path.join(runtimeDir,'baal-expansion-live-request-v4.json'))||fs.existsSync(path.join(runtimeDir,'baal-expansion-live-request-v3.json')))?createScopedRestReceiptsV7",
    );
    std::fs::write(Path::new("live-paper/runner.mjs"), s).map_err(|e| e.to_string())?;

    let f = Path::new(&rt).join("median-discovery.json");
    let mut c = read_json(&f)?;
    c["codeIdentity"] = median_discovery_identity(&cwd)?;
    atomic_json_canonical(&f, &c)?;

    let prior = read_json(Path::new(&format!("{rt}/baal-expansion-live-request-v4.json")))?;
    let frontier = read_baal_custody_frontier_v1(&json!({
        "runtimeDir": rt,
        "workspace": cwd.to_string_lossy(),
    }))?;
    let universe = validate_universe(
        &read_json(Path::new("live-paper/price-universe.json"))?,
        prior["tournamentId"].as_str().unwrap_or(""),
    )?;
    let r = make_baal_generic_live_request_v11(
        &cwd,
        Path::new(&rt),
        &universe,
        &frontier,
        prior["epoch"].as_str().unwrap_or(""),
        prior["activation"]["startedAt"].as_str().unwrap_or(""),
        prior["activation"]["entryExpiresAt"].as_str().unwrap_or(""),
    )?;
    assert_eq(
        &json!(payload_hash(&r["activation"])?),
        &json!(payload_hash(&prior["activation"])?),
    )?;
    assert_eq(&r["inheritanceHash"], &prior["inheritanceHash"])?;
    atomic_json_canonical(
        &Path::new(&rt).join("baal-expansion-live-request-v5.json"),
        &r,
    )?;
    let out = json!({
        "sameActivation": true,
        "sameController": true,
        "sameBudget": true,
        "foreignHoldingAdoption": false,
    });
    println!("{}", js_stringify(&out));
    Ok(out)
}
