// execution/baal-generic-prior-custody-v1.mjs — verifies a prior live request's
// custody is exactly sealed before a new activation inherits it.
use serde_json::{json, Value as V};
use std::fs;
use std::path::{Path, PathBuf};

use baal_service_runtime::json_text::sha256;

use crate::baal_generic_no_controller_v3::{
    baal_generic_no_controller_v3_identity, replay_baal_generic_no_journal_v3,
};
use crate::core::payload_hash;

fn need(v: bool, c: &str) -> Result<(), String> {
    if !v {
        return Err(format!("PRIOR_GENERIC_{c}"));
    }
    Ok(())
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn sget<'a>(v: &'a V, k: &str) -> Option<&'a str> {
    v.get(k).and_then(|x| x.as_str())
}

// verifyBaalGenericPriorCustodyV1({runtimeDir,custodySeed})
pub fn verify_baal_generic_prior_custody_v1(
    runtime_dir: &Path,
    custody_seed: &V,
) -> Result<Option<V>, String> {
    let request_file = runtime_dir.join("baal-generic-live-request-v1.json");
    if !request_file.exists() {
        return Ok(None);
    }
    let request: V = serde_json::from_str(
        &fs::read_to_string(&request_file).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let root = runtime_dir
        .join("baal-generic-live-runs")
        .join(sget(&request, "runId").unwrap_or(""))
        .join(sget(&request, "epoch").unwrap_or("").to_string());
    need(
        sget(&request, "runId") == sget(custody_seed, "runId")
            && sget(&request, "ownerId") == sget(custody_seed, "ownerId")
            && sget(&request, "inheritanceHash") == sget(custody_seed, "seedHash"),
        "EXACT_INHERITANCE",
    )?;
    let actor = root.join("controller").join("baal-generic-no-v3-journal.jsonl");
    let budget = root.join("budget").join("baal-generic-shared-budget-v3.jsonl");
    let replay = replay_baal_generic_no_journal_v3(&json!({
        "bodyText": fs::read_to_string(&actor).map_err(|e| e.to_string())?,
        "expectedCodeIdentity": baal_generic_no_controller_v3_identity()?,
        "custodySeed": custody_seed,
    }))?;
    need(
        get(get(&replay, "state"), "orders")
            .as_array()
            .is_some_and(|a| a.is_empty())
            && payload_hash(get(get(&replay, "state"), "inventory"))
                .map_err(|e| e.to_string())?
                == payload_hash(get(custody_seed, "inventory")).map_err(|e| e.to_string())?,
        "NO_NEW_OR_PENDING_ORDERS",
    )?;
    let mut seals: Vec<V> = Vec::new();
    for file in [&request_file, &actor, &budget] {
        let bytes = fs::read(file).map_err(|e| e.to_string())?;
        let relative: PathBuf = file
            .strip_prefix(runtime_dir)
            .unwrap_or(file)
            .to_path_buf();
        seals.push(json!({
            "relative": relative.to_string_lossy(),
            "bytes": bytes.len(),
            "sha256": sha256(&bytes),
        }));
    }
    Ok(Some(json!({
        "epoch": get(&request, "epoch"),
        "anchor": get(&replay, "anchor"),
        "seals": seals,
        "newOrders": 0,
        "economicReplayPerformed": true,
    })))
}
