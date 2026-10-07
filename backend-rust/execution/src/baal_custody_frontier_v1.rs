// execution/baal-custody-frontier-v1.mjs — read-only import of the complete
// existing economic frontier for a successor. Returns private receipts in
// memory; callers must never publish the state. No writer, account client,
// activation, order, lease or file mutation.
//
// Port notes:
// - `stable` identity is {ino, birthtimeMs} -> payload_hash of the JSON
//   identity, birthtimeMs as the JS millisecond float.
// - `Journal.last` -> service-runtime Journal's last body (no hash field).
// - All file bytes are read to memory exactly like the JS source; the
//   budget-prefix re-read uses subarray(0, bound.bytes) verbatim.
use crate::baal_combined_custody_host_v6::baal_combined_custody_v6_identity;
use crate::baal_neutral_owned_counterpart_pilot_v2::{
    live_coordinator_identity, pilot_unknown, project_risk, replay_live_hedge_journal,
};
use crate::baal_scaled_controller_legacy_replay_v1::{
    baal_scaled_no_controller_identity, replay_baal_scaled_controller_v1,
};
use crate::core::payload_hash;
use baal_service_runtime::fixed::{amount, money};
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::{js_int, sha256};
use serde_json::{json, Map, Value as V};
use std::path::{Path, PathBuf};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("CUSTODY_FRONTIER_{code}"))
    }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn sget<'a>(v: &'a V, k: &str) -> Option<&'a str> {
    v.get(k).and_then(V::as_str)
}
fn sha(b: &[u8]) -> String {
    sha256(b)
}

struct Stable {
    bytes: Vec<u8>,
    identity: V,
    hash: String,
}

fn stable(file: &Path) -> Result<Stable, String> {
    use std::os::unix::fs::MetadataExt;
    let before = std::fs::metadata(file).map_err(|e| e.to_string())?;
    let bytes = std::fs::read(file).map_err(|e| e.to_string())?;
    let after = std::fs::metadata(file).map_err(|e| e.to_string())?;
    let before_birth = birth_ms(&before);
    let after_birth = birth_ms(&after);
    need(
        before.ino() == after.ino()
            && before_birth == after_birth
            && before.size() == after.size()
            && bytes.len() as u64 == after.size(),
        "SOURCE_CHANGED",
    )?;
    Ok(Stable {
        hash: sha(&bytes),
        bytes,
        identity: json!({
            "ino": before.ino() as i64,
            "birthtimeMs": before_birth,
        }),
    })
}
fn birth_ms(m: &std::fs::Metadata) -> f64 {
    m.created()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(f64::NAN)
}

fn summary_of(state: &V, risk: &V) -> Result<V, String> {
    let owned: Vec<V> = get(state, "currentInventory")
        .as_object()
        .map(|m| m.values().cloned().collect())
        .unwrap_or_default();
    let owned_quantity: i64 = owned
        .iter()
        .map(|p| js_int(get(get(p, "proof"), "remainingOwnedQuantity")).unwrap_or(0))
        .sum();
    let owned_principal: i128 = owned
        .iter()
        .try_fold(0i128, |n, p| {
            Ok::<i128, String>(n + amount(get(get(p, "proof"), "remainingOwnedCost").as_str().unwrap_or(""))?)
        })?;
    let realized: i128 = owned
        .iter()
        .try_fold(0i128, |n, p| {
            Ok::<i128, String>(n + amount(get(get(p, "proof"), "cumulativeRealizedPnl").as_str().unwrap_or(""))?)
        })?;
    let original = get(state, "originalOrders").as_array().cloned().unwrap_or_default();
    let current = get(state, "orders").as_array().cloned().unwrap_or_default();
    let abandoned = original
        .iter()
        .chain(current.iter())
        .filter(|o| sget(o, "phase") == Some("abandoned"))
        .count();
    Ok(json!({
        "ownedQuantity": owned_quantity,
        "ownedPrincipal": money(owned_principal),
        "realizedPnl": money(realized),
        "legacyOrders": original.len() as i64,
        "currentCoordinatorOrders": current.len() as i64,
        "inheritedOrders": (original.len() + current.len()) as i64,
        "abandonedOrders": abandoned as i64,
        "pairAssignments": get(state, "assignments").as_array().map(|a| a.len() as i64).unwrap_or(0),
        "pairRetirements": get(state, "pairRetirements").as_array().map(|a| a.len() as i64).unwrap_or(0),
        "unknownAcceptance": pilot_unknown(state),
        "unresolvedIntentIds": get(risk, "unresolvedIntentIds"),
    }))
}

/// readBaalCustodyFrontierV1({runtimeDir, workspace})
pub fn read_baal_custody_frontier_v1(runtime_dir: &Path, workspace: &Path) -> Result<V, String> {
    need(
        runtime_dir.is_absolute() && workspace.is_absolute(),
        "PATHS",
    )?;
    let descriptor_file = runtime_dir.join("baal-combined-custody-request-v6.json");
    let descriptor = stable(&descriptor_file)?;
    let d: V = serde_json::from_slice(&descriptor.bytes).map_err(|e| e.to_string())?;
    need(
        sget(&d, "kind") == Some("baal_combined_custody_host_v6")
            && sget(&d, "ownerId") == Some("Codex")
            && payload_hash(get(&d, "codeIdentity"))?
                == payload_hash(&baal_combined_custody_v6_identity(workspace)?)?,
        "DESCRIPTOR_GRAPH",
    )?;
    let epoch_ok = sget(get(&d, "migration"), "implementationEpoch").is_some_and(|s| {
        let b = s.as_bytes();
        !b.is_empty()
            && b.len() <= 100
            && b.iter()
                .all(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c == b'-')
    });
    let run_ok = sget(&d, "runId").is_some_and(|s| {
        let b = s.as_bytes();
        b.len() >= 16
            && b.len() <= 95
            && s.starts_with("baal-scaled-no-")
            && s[15..].bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
    });
    need(epoch_ok && run_ok, "SOURCE_PATH_COMPONENTS")?;
    let legacy_file = runtime_dir
        .join("baal-scaled-live-runs")
        .join(sget(&d, "runId").unwrap_or(""))
        .join("controller")
        .join("baal-scaled-no-journal.jsonl");
    let legacy = stable(&legacy_file)?;
    let seed = replay_baal_scaled_controller_v1(&json!({
        "bodyText": String::from_utf8_lossy(&legacy.bytes),
        "expectedCodeIdentity": baal_scaled_no_controller_identity()?,
    }))?;
    need(
        payload_hash(&seed)? == sget(&d, "seedHash").unwrap_or("")
            && payload_hash(get(&seed, "anchor"))? == payload_hash(get(&d, "legacyAnchor"))?,
        "ORIGINAL_SEED",
    )?;
    let dir = runtime_dir
        .join("baal-combined-custody-runs")
        .join(sget(&d, "runId").unwrap_or(""))
        .join(sget(get(&d, "migration"), "implementationEpoch").unwrap_or(""));
    let actor_file = dir
        .join("coordinator")
        .join("neutral-owned-counterpart-pilot-v2.jsonl");
    let actor = stable(&actor_file)?;
    let code = live_coordinator_identity(workspace)?;
    let current = replay_live_hedge_journal(
        &String::from_utf8_lossy(&actor.bytes),
        &seed,
        get(&d, "activation"),
        get(&d, "migration"),
        &code,
    )?;
    let budget_file = dir.join("budget").join("owned-hedge-shared-budget-v2.jsonl");
    let budget = stable(&budget_file)?;
    let journal = Journal::open(&budget_file)?;
    let last = journal.last.clone().unwrap_or(V::Null);
    need(
        sget(get(&last, "event"), "kind") == Some("installed_source_verified_hedge_reserve")
            && get(get(&last, "state"), "seedHash") == get(&d, "seedHash")
            && get(get(&last, "state"), "coordinatorCodeIdentityHash").as_str()
                == Some(payload_hash(&code)?.as_str()),
        "BUDGET_BINDING",
    )?;
    let bound = get(get(&last, "state"), "actorAnchor").clone();
    let bound_bytes = js_int(get(&bound, "bytes"));
    need(
        bound_bytes.is_some_and(|b| b > 0 && (b as u64) <= actor.bytes.len() as u64)
            && sha(&actor.bytes[..bound_bytes.unwrap_or(0) as usize]) == sget(&bound, "sha256").unwrap_or(""),
        "BUDGET_ACTOR_PREFIX",
    )?;
    let prefix_len = bound_bytes.unwrap() as usize;
    let budget_actor = replay_live_hedge_journal(
        &String::from_utf8_lossy(&actor.bytes[..prefix_len]),
        &seed,
        get(&d, "activation"),
        get(&d, "migration"),
        &code,
    )?;
    need(
        payload_hash(get(&budget_actor, "anchor"))? == payload_hash(&bound)?
            && payload_hash(&project_risk(get(&budget_actor, "state"))?)?
                == payload_hash(&get(get(&last, "state"), "projection")
                    .get("record")
                    .cloned()
                    .unwrap_or(V::Null))?,
        "BUDGET_ECONOMICS",
    )?;
    let state = get(&current, "state").clone();
    let risk = project_risk(&state)?;
    let summary = summary_of(&state, &risk)?;
    let source_records = vec![
        (descriptor_file.clone(), &descriptor),
        (legacy_file.clone(), &legacy),
        (actor_file.clone(), &actor),
        (budget_file.clone(), &budget),
    ];
    for (file, source) in &source_records {
        let fresh = stable(file)?;
        need(
            fresh.hash == source.hash && payload_hash(&fresh.identity)? == payload_hash(&source.identity)?,
            "FRONTIER_ADVANCED_DURING_IMPORT",
        )?;
    }
    let source_hashes: Vec<V> = source_records
        .iter()
        .map(|(file, source)| {
            let relative = file
                .strip_prefix(runtime_dir)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| file.to_string_lossy().to_string());
            json!({
                "relative": relative,
                "bytes": source.bytes.len() as i64,
                "sha256": source.hash,
            })
        })
        .collect();
    Ok(json!({
        "schemaVersion": 1,
        "kind": "baal_complete_custody_frontier_v1",
        "descriptorHash": payload_hash(&d)?,
        "originalSeedHash": get(&d, "seedHash"),
        "originalActivation": get(&d, "activation"),
        "migration": get(&d, "migration"),
        "legacyAnchor": get(&seed, "anchor"),
        "currentActorAnchor": get(&current, "anchor"),
        "budgetAnchor": {
            "bytes": budget.bytes.len() as i64,
            "sha256": budget.hash,
            "sequence": journal.sequence as i64,
            "hash": journal.previous,
        },
        "currentStateHash": payload_hash(&state)?,
        "currentState": state,
        "risk": risk,
        "summary": summary,
        "sourceHashes": source_hashes,
        "executionAuthority": false,
        "networkRequests": 0,
        "originalJournalsChanged": false,
    }))
}

/// assertBaalCustodyFrontierCanContinue(frontier)
pub fn assert_baal_custody_frontier_can_continue(frontier: &V) -> Result<V, String> {
    need(
        sget(frontier, "kind") == Some("baal_complete_custody_frontier_v1")
            && get(frontier, "executionAuthority") == &V::Bool(false)
            && payload_hash(get(frontier, "currentState"))?
                == sget(frontier, "currentStateHash").unwrap_or(""),
        "EXACT_FRONTIER",
    )?;
    need(
        payload_hash(&summary_of(get(frontier, "currentState"), get(frontier, "risk"))?)?
            == payload_hash(get(frontier, "summary"))?,
        "SUMMARY_ECONOMICS",
    )?;
    need(
        get(get(frontier, "summary"), "unknownAcceptance") == &V::Bool(false)
            && get(get(frontier, "summary"), "unresolvedIntentIds")
                .as_array()
                .is_some_and(|a| a.is_empty())
            && get(get(frontier, "risk"), "unresolvedIntentIds")
                .as_array()
                .is_some_and(|a| a.is_empty()),
        "UNRESOLVED_ACCEPTANCE_OR_INTENT",
    )?;
    need(
        payload_hash(&project_risk(get(frontier, "currentState"))?)?
            == payload_hash(get(frontier, "risk"))?,
        "CURRENT_FULL_RESERVATION",
    )?;
    Ok(json!({
        "continuationStateHash": get(frontier, "currentStateHash"),
        "sourceActorAnchor": get(frontier, "currentActorAnchor"),
        "summary": get(frontier, "summary"),
        "executionAuthority": false,
        "freshAccountStillRequired": true,
        "newActivationStillRequired": true,
    }))
}
