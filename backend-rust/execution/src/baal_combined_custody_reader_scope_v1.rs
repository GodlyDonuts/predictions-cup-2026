// execution/baal-combined-custody-reader-scope-v1.mjs — decoder scope only:
// financial authority remains with the combined host. Startup replay binds
// graph reads to current owned packages, including their counterpart. Creates
// no account client and makes no request.
use crate::baal_scaled_controller_legacy_replay_v1::{
    baal_scaled_no_controller_identity, replay_baal_scaled_controller_v1,
};
use crate::core::payload_hash;
use serde_json::{json, Value as V};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("CUSTODY_READER_SCOPE_{code}"))
    }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}

/// readBaalCombinedCustodyReaderScopeV1({runtimeDir,universe})
pub fn read_baal_combined_custody_reader_scope_v1(
    runtime_dir: &Path,
    universe: &V,
) -> Result<V, String> {
    need(runtime_dir.is_absolute(), "RUNTIME")?;
    let descriptor: V = serde_json::from_str(
        &fs::read_to_string(runtime_dir.join("baal-combined-custody-request-v6.json"))
            .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let run_id = get(&descriptor, "runId").as_str().unwrap_or("");
    need(
        get(&descriptor, "kind").as_str() == Some("baal_combined_custody_host_v6")
            && get(&descriptor, "ownerId").as_str() == Some("Codex")
            && get(&descriptor, "tournamentId") == get(universe, "tournamentId")
            && get(&descriptor, "universeHash") == get(universe, "hash")
            && !run_id.is_empty()
            && run_id.len() <= 96
            && run_id.starts_with("baal-scaled-no-")
            && run_id[15..]
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
        "DESCRIPTOR",
    )?;
    let file = runtime_dir
        .join("baal-scaled-live-runs")
        .join(run_id)
        .join("controller")
        .join("baal-scaled-no-journal.jsonl");
    let seed = replay_baal_scaled_controller_v1(&json!({
        "bodyText": fs::read_to_string(&file).map_err(|e| e.to_string())?,
        "expectedCodeIdentity": baal_scaled_no_controller_identity()?,
    }))?;
    need(
        payload_hash(&seed)? == get(&descriptor, "seedHash").as_str().unwrap_or("")
            && payload_hash(get(&seed, "anchor"))?
                == payload_hash(get(&descriptor, "legacyAnchor"))?
            && get(&seed, "legacyCodeIdentityHash")
                == get(&descriptor, "legacyCodeIdentityHash")
            && get(&get(&get(&seed, "state"), "scope"), "runId")
                == get(&descriptor, "runId")
            && get(&get(&get(&seed, "state"), "scope"), "tournamentId")
                == get(&descriptor, "tournamentId"),
        "EXACT_SEAL",
    )?;
    let members: BTreeMap<String, String> = get(universe, "exchanges")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|l| {
            (
                get(l, "exchangeId").as_str().unwrap_or("").to_string(),
                get(l, "marketId").as_str().unwrap_or("").to_string(),
            )
        })
        .collect();
    let mut ids = BTreeSet::new();
    if let Some(inv) = get(&get(&seed, "state"), "inventory").as_object() {
        for (exchange_id, slot) in inv {
            if get(get(slot, "proof"), "remainingOwnedQuantity").as_i64() == Some(0) {
                continue;
            }
            need(
                get(get(slot, "proof"), "remainingOwnedQuantity")
                    .as_i64()
                    .is_some_and(|q| q > 0)
                    && get(get(slot, "proof"), "exchangeId").as_str() == Some(exchange_id.as_str())
                    && get(get(slot, "package"), "legs")
                        .as_array()
                        .map(|l| l.len() == 2)
                        .unwrap_or(false),
                "OWNED_PACKAGE",
            )?;
            for leg in get(get(slot, "package"), "legs").as_array().cloned().unwrap_or_default() {
                need(
                    members.get(get(&leg, "exchangeId").as_str().unwrap_or(""))
                        .map(|m| m.as_str())
                        == get(&leg, "marketId").as_str(),
                    "UNIVERSE_LEG",
                )?;
                ids.insert(get(&leg, "marketId").as_str().unwrap_or("").to_string());
            }
        }
    }
    need(!ids.is_empty() && ids.len() <= 20, "GRAPH_SCOPE_BOUND")?;
    let mut sorted: Vec<String> = ids.into_iter().collect();
    sorted.sort_by_key(|a| a.parse::<u64>().unwrap_or(0));
    Ok(json!({
        "allowedOwnedGraphMarketIds": sorted,
        "seedHash": get(&descriptor, "seedHash"),
        "executionAuthority": false,
        "networkRequests": 0,
    }))
}
