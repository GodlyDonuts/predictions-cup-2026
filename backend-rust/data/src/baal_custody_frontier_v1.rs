// execution/baal-custody-frontier-v1.mjs — read-only import of the complete
// existing economic frontier for a successor. Returns private receipts in
// memory; callers must never publish the state. No writer, account client,
// activation, order, lease or file mutation.
//
// Port notes:
// - stable() performs the same stat/read/stat ino+birthtimeMs+size identity
//   check; a changed or unidentifiable source is CUSTODY_FRONTIER_SOURCE_CHANGED.
// - need(v,c) throws Error('CUSTODY_FRONTIER_'+c) -> Err("CUSTODY_FRONTIER_<c>").
// - workspace-relative frozen-source identities resolve through
//   baal-collector's collect_frozen_paper_files, matching
//   live-paper/frozen-paper-graph.mjs.
use baal_collector::frozen_paper_graph::collect_frozen_paper_files;
use baal_execution::baal_scaled_controller_legacy_replay_v1::{
    baal_scaled_no_controller_identity, replay_baal_scaled_controller_v1,
};
use baal_execution::core::payload_hash;
use baal_service_runtime::fixed::amount;
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::{js_int, sha256};
use serde_json::{json, Value as V};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::baal_neutral_owned_counterpart_pilot_v2::{
    live_coordinator_identity, pilot_unknown, project_risk, replay_live_hedge_journal,
};

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
fn js_str(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Null => "null".into(),
        V::Bool(b) => b.to_string(),
        _ => baal_service_runtime::json_text::js_stringify(v),
    }
}
fn type_error() -> String {
    "TypeError: Cannot read properties of null or undefined".to_string()
}
// JS Number(v): numbers pass through; non-numbers (incl. strings) are NaN.
fn js_num(v: &V) -> Option<f64> {
    match v {
        V::Number(n) => n.as_f64(),
        V::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        V::Null => Some(0.0),
        _ => None,
    }
}
fn copy(v: &V) -> V {
    v.clone()
}
fn arr<'a>(v: &'a V, k: &str) -> &'a Vec<V> {
    static EMPTY: Vec<V> = Vec::new();
    get(v, k).as_array().unwrap_or(&EMPTY)
}

fn birth_ms(md: &fs::Metadata) -> f64 {
    md.created()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(f64::NAN)
}

struct Stable {
    bytes: Vec<u8>,
    identity: V,
    hash: String,
}
fn stable(file: &Path) -> Result<Stable, String> {
    let before = fs::metadata(file).map_err(|e| e.to_string())?;
    let bytes = fs::read(file).map_err(|e| e.to_string())?;
    let after = fs::metadata(file).map_err(|e| e.to_string())?;
    need(
        before.ino() == after.ino()
            && birth_ms(&before) == birth_ms(&after)
            && before.len() == after.len()
            && bytes.len() as u64 == after.len(),
        "SOURCE_CHANGED",
    )?;
    let hash = sha(&bytes);
    Ok(Stable {
        bytes,
        identity: json!({
            "ino": before.ino(),
            "birthtimeMs": birth_ms(&before),
        }),
        hash,
    })
}

// baalCombinedCustodyV6Identity(workspace): frozen-source manifest for the
// combined custody host v6 graph.
fn baal_combined_custody_v6_identity(workspace: &Path) -> Result<V, String> {
    let files = collect_frozen_paper_files(
        workspace,
        &[
            "execution/baal-combined-custody-host-v6.mjs".to_string(),
            "execution/scoped-rest-receipts-v5.mjs".to_string(),
        ],
    )?;
    serde_json::to_value(files).map_err(|e| e.to_string())
}

fn epoch_ok(s: &str) -> bool {
    // /^[a-zA-Z0-9_-]{1,100}$/
    !s.is_empty()
        && s.len() <= 100
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
fn run_id_ok(s: &str) -> bool {
    // /^baal-scaled-no-[a-z0-9-]{1,80}$/
    s.strip_prefix("baal-scaled-no-").is_some_and(|r| {
        !r.is_empty()
            && r.len() <= 80
            && r.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    })
}

fn summary_of(state: &V, risk: &V) -> Result<V, String> {
    // Object.values(state.currentInventory): null currentInventory throws.
    if get(state, "currentInventory").is_null() {
        return Err(type_error());
    }
    let owned: Vec<V> = get(state, "currentInventory")
        .as_object()
        .map(|m| m.values().cloned().collect())
        .unwrap_or_default();
    // n + p.proof.remainingOwnedQuantity: a raw JS numeric sum (fractional or
    // missing quantity yields NaN downstream); p.proof null throws TypeError.
    let mut owned_quantity = 0.0f64;
    for p in &owned {
        if get(p, "proof").is_null() {
            return Err(type_error());
        }
        owned_quantity +=
            js_num(get(get(p, "proof"), "remainingOwnedQuantity")).unwrap_or(f64::NAN);
    }
    let owned_principal: i128 = owned
        .iter()
        .map(|p| {
            get(get(p, "proof"), "remainingOwnedCost")
                .as_str()
                .ok_or_else(|| "Money must be a canonical decimal string with at most six places".to_string())
                .and_then(amount)
        })
        .collect::<Result<Vec<i128>, String>>()?
        .iter()
        .sum();
    let realized: i128 = owned
        .iter()
        .map(|p| {
            get(get(p, "proof"), "cumulativeRealizedPnl")
                .as_str()
                .ok_or_else(|| "Money must be a canonical decimal string with at most six places".to_string())
                .and_then(amount)
        })
        .collect::<Result<Vec<i128>, String>>()?
        .iter()
        .sum();
    // state.{originalOrders,orders,assignments,pairRetirements}.length:
    // null fields throw TypeError.
    for k in ["originalOrders", "orders", "assignments", "pairRetirements"] {
        if get(state, k).is_null() {
            return Err(type_error());
        }
    }
    let legacy_orders = arr(state, "originalOrders").len();
    let current_orders = arr(state, "orders").len();
    let abandoned = arr(state, "originalOrders")
        .iter()
        .chain(arr(state, "orders").iter())
        .filter(|o| get(o, "phase").as_str() == Some("abandoned"))
        .count();
    // [...risk.unresolvedIntentIds]: non-iterable throws TypeError.
    let risk_ids = get(risk, "unresolvedIntentIds");
    if !risk_ids.is_array() {
        return Err(type_error());
    }
    Ok(json!({
        "ownedQuantity": if owned_quantity.is_finite() && owned_quantity.fract() == 0.0 && owned_quantity.abs() < 1e15 {
            json!(owned_quantity as i64)
        } else {
            json!(owned_quantity)
        },
        "ownedPrincipal": baal_service_runtime::fixed::money(owned_principal),
        "realizedPnl": baal_service_runtime::fixed::money(realized),
        "legacyOrders": legacy_orders,
        "currentCoordinatorOrders": current_orders,
        "inheritedOrders": legacy_orders + current_orders,
        "abandonedOrders": abandoned,
        "pairAssignments": arr(state, "assignments").len(),
        "pairRetirements": arr(state, "pairRetirements").len(),
        "unknownAcceptance": pilot_unknown(state),
        "unresolvedIntentIds": risk_ids.clone(),
    }))
}

fn relative(runtime: &Path, file: &Path) -> String {
    file.strip_prefix(runtime)
        .map(|r| r.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default()
}

/// readBaalCustodyFrontierV1({runtimeDir,workspace})
pub fn read_baal_custody_frontier_v1(input: &V) -> Result<V, String> {
    let runtime_dir = get(input, "runtimeDir").as_str().unwrap_or("");
    let workspace = get(input, "workspace").as_str().unwrap_or("");
    need(
        Path::new(runtime_dir).is_absolute() && Path::new(workspace).is_absolute(),
        "PATHS",
    )?;
    let runtime = PathBuf::from(runtime_dir);
    let ws = Path::new(workspace);

    let descriptor_file = runtime.join("baal-combined-custody-request-v6.json");
    let descriptor = stable(&descriptor_file)?;
    let d: V = serde_json::from_slice(&descriptor.bytes).map_err(|e| format!("SyntaxError: {e}"))?;
    need(
        get(&d, "kind").as_str() == Some("baal_combined_custody_host_v6")
            && get(&d, "ownerId").as_str() == Some("Codex")
            && payload_hash(get(&d, "codeIdentity"))?
                == payload_hash(&baal_combined_custody_v6_identity(ws)?)?,
        "DESCRIPTOR_GRAPH",
    )?;
    let migration = get(&d, "migration");
    // d.migration.implementationEpoch — null/undefined migration throws
    // TypeError; any other primitive coerces via RegExp.test's ToString.
    if migration.is_null() {
        return Err(type_error());
    }
    need(
        epoch_ok(&js_str(get(migration, "implementationEpoch")))
            && run_id_ok(&js_str(get(&d, "runId"))),
        "SOURCE_PATH_COMPONENTS",
    )?;
    let run_id = js_str(get(&d, "runId"));
    let epoch = js_str(get(migration, "implementationEpoch"));

    let legacy_file = runtime
        .join("baal-scaled-live-runs")
        .join(&run_id)
        .join("controller")
        .join("baal-scaled-no-journal.jsonl");
    let legacy = stable(&legacy_file)?;
    let seed = replay_baal_scaled_controller_v1(&json!({
        "bodyText": String::from_utf8_lossy(&legacy.bytes),
        "expectedCodeIdentity": baal_scaled_no_controller_identity()?,
    }))?;
    need(
        payload_hash(&seed)? == js_str(get(&d, "seedHash"))
            && payload_hash(get(&seed, "anchor"))? == payload_hash(get(&d, "legacyAnchor"))?,
        "ORIGINAL_SEED",
    )?;

    let dir = runtime
        .join("baal-combined-custody-runs")
        .join(&run_id)
        .join(&epoch);
    let actor_file = dir
        .join("coordinator")
        .join("neutral-owned-counterpart-pilot-v2.jsonl");
    let actor = stable(&actor_file)?;
    let code = live_coordinator_identity(ws)?;
    let current = replay_live_hedge_journal(&json!({
        "bodyText": String::from_utf8_lossy(&actor.bytes),
        "seed": seed,
        "activation": get(&d, "activation"),
        "migration": migration,
        "expectedCodeIdentity": code,
    }))?;

    let budget_file = dir.join("budget").join("owned-hedge-shared-budget-v2.jsonl");
    let budget = stable(&budget_file)?;
    let journal = Journal::open(&budget_file)?;
    let last = journal.last.clone().unwrap_or(V::Null);
    // JS evaluates last.event.kind first; only a match touches last.state,
    // where a missing state throws TypeError rather than the coded error.
    if get(get(&last, "event"), "kind").as_str() == Some("installed_source_verified_hedge_reserve")
        && !get(&last, "state").is_object()
    {
        return Err(type_error());
    }
    need(
        get(get(&last, "event"), "kind").as_str() == Some("installed_source_verified_hedge_reserve")
            && get(get(&last, "state"), "seedHash") == get(&d, "seedHash")
            && get(get(&last, "state"), "coordinatorCodeIdentityHash").as_str()
                == Some(payload_hash(&code)?.as_str()),
        "BUDGET_BINDING",
    )?;
    let bound = get(get(&last, "state"), "actorAnchor");
    let bound_bytes = js_int(get(bound, "bytes"));
    need(
        bound_bytes.is_some_and(|b| b > 0 && (b as usize) <= actor.bytes.len()),
        "BUDGET_ACTOR_PREFIX",
    )?;
    let bound_bytes = bound_bytes.unwrap() as usize;
    need(
        sha(&actor.bytes[..bound_bytes]) == js_str(get(bound, "sha256")),
        "BUDGET_ACTOR_PREFIX",
    )?;
    let budget_actor = replay_live_hedge_journal(&json!({
        "bodyText": String::from_utf8_lossy(&actor.bytes[..bound_bytes]),
        "seed": seed,
        "activation": get(&d, "activation"),
        "migration": migration,
        "expectedCodeIdentity": code,
    }))?;
    let projection = get(get(&last, "state"), "projection");
    if !projection.is_object() {
        return Err(type_error());
    }
    need(
        payload_hash(get(&budget_actor, "anchor"))? == payload_hash(bound)?
            && payload_hash(&project_risk(get(&budget_actor, "state"))?)?
                == payload_hash(get(projection, "record"))?,
        "BUDGET_ECONOMICS",
    )?;

    let state = get(&current, "state").clone();
    let risk = project_risk(&state)?;
    let summary = summary_of(&state, &risk)?;

    let source_records = [
        (descriptor_file.clone(), &descriptor),
        (legacy_file.clone(), &legacy),
        (actor_file.clone(), &actor),
        (budget_file.clone(), &budget),
    ];
    for (file, source) in &source_records {
        let fresh = stable(file)?;
        need(
            fresh.hash == source.hash
                && payload_hash(&fresh.identity)? == payload_hash(&source.identity)?,
            "FRONTIER_ADVANCED_DURING_IMPORT",
        )?;
    }
    let source_hashes: Vec<V> = source_records
        .iter()
        .map(|(file, source)| {
            json!({
                "relative": relative(&runtime, file),
                "bytes": source.bytes.len(),
                "sha256": source.hash,
            })
        })
        .collect();
    Ok(json!({
        "schemaVersion": 1,
        "kind": "baal_complete_custody_frontier_v1",
        "descriptorHash": payload_hash(&d)?,
        "originalSeedHash": get(&d, "seedHash"),
        "originalActivation": copy(get(&d, "activation")),
        "migration": copy(migration),
        "legacyAnchor": get(&seed, "anchor"),
        "currentActorAnchor": get(&current, "anchor"),
        "budgetAnchor": {
            "bytes": budget.bytes.len(),
            "sha256": budget.hash,
            "sequence": journal.sequence,
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
        get(frontier, "kind").as_str() == Some("baal_complete_custody_frontier_v1")
            && get(frontier, "executionAuthority") == &V::Bool(false)
            && payload_hash(get(frontier, "currentState"))?
                == js_str(get(frontier, "currentStateHash")),
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
                .is_some_and(|x| x.is_empty())
            && get(get(frontier, "risk"), "unresolvedIntentIds")
                .as_array()
                .is_some_and(|x| x.is_empty()),
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
        "summary": copy(get(frontier, "summary")),
        "executionAuthority": false,
        "freshAccountStillRequired": true,
        "newActivationStillRequired": true,
    }))
}
