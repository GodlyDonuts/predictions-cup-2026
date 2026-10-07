// execution/baal-expansion-custody-seed-v1.mjs — new flat run, with complete
// verified old custody preserved as an immutable archive. Historical gains
// are not imported into the new run's spending/profit buffer.
//
// Port notes (same conventions as the sibling seeds):
// - `need` throws Error('EXPANSION_SEED_<code>') — bare code strings.
// - path.join -> PathBuf::join; path.relative(from,to) -> rel() over
//   components; fs.readFileSync -> fs::read (errors propagate raw).
// - {...old,k:v} key order: keys already present in `old` keep position and
//   are replaced in place; new keys append in literal order. The same rule
//   holds for the {seedHash,...rest} destructure used by validate.
// - `x.proof.remainingOwnedQuantity===0` is strict: only a real number 0
//   passes (as_f64 covers JSON int/float zero, not strings).
use crate::baal_cohort_recovered_custody_seed_v4::build_baal_cohort_recovered_custody_seed_v4;
use crate::baal_generic_no_controller_v7::{
    baal_generic_no_controller_v7_identity, replay_baal_generic_no_journal_v7,
};
use crate::core::payload_hash;
use baal_service_runtime::json_text::{js_truthy, sha256};
use serde_json::{json, Map, Value as V};
use std::fs;
use std::path::{Component, Path, PathBuf};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("EXPANSION_SEED_{code}"))
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
// path.relative(from,to): longest common component prefix + '..' steps.
fn rel(from: &Path, to: &Path) -> String {
    let f: Vec<Component> = from.components().collect();
    let t: Vec<Component> = to.components().collect();
    let mut i = 0;
    while i < f.len() && i < t.len() && f[i] == t[i] {
        i += 1;
    }
    let mut parts: Vec<String> = vec!["..".to_string(); f.len() - i];
    for c in &t[i..] {
        parts.push(c.as_os_str().to_string_lossy().into_owned());
    }
    parts.join("/")
}

/// buildBaalExpansionCustodySeedV1({frontier,scope,allocation,runtimeDir})
pub fn build_baal_expansion_custody_seed_v1(
    frontier: &V,
    scope: &V,
    allocation: &V,
    runtime_dir: &Path,
) -> Result<V, String> {
    let req_file = runtime_dir.join("cohort-profit-lock-request-v2.json");
    let r: V = serde_json::from_slice(&fs::read(&req_file).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let base = build_baal_cohort_recovered_custody_seed_v4(
        frontier,
        get(&r, "scope"),
        get(&r, "allocation"),
        runtime_dir,
    )?;
    let actor_file: PathBuf = runtime_dir
        .join("baal-generic-live-runs")
        .join(sget(&r, "runId").unwrap_or(""))
        .join(sget(&r, "epoch").unwrap_or(""))
        .join("controller")
        .join("baal-generic-no-v7-journal.jsonl");
    let bytes = fs::read(&actor_file).map_err(|e| e.to_string())?;
    let body_text = String::from_utf8_lossy(&bytes).into_owned();
    let replay = replay_baal_generic_no_journal_v7(
        &body_text,
        &baal_generic_no_controller_v7_identity()?,
        &base,
    )?;
    let s = get(&replay, "state");
    need(
        get(s, "inventory")
            .as_object()
            .map(|inv| {
                inv.values()
                    .all(|x| get(get(x, "proof"), "remainingOwnedQuantity").as_f64() == Some(0.0))
            })
            .unwrap_or(false)
            && get(s, "orders")
                .as_array()
                .map(|orders| {
                    orders.iter().all(|o| {
                        (js_truthy(o.get("proof"))
                            && sget(o, "phase") == Some("reconciled"))
                            || sget(o, "phase") == Some("abandoned")
                    })
                })
                .unwrap_or(false),
        "PRIOR_NOT_FLAT_OR_PENDING",
    )?;
    let authority_file = runtime_dir.join("baal-expansion-authority-v1.json");
    let authority: V =
        serde_json::from_slice(&fs::read(&authority_file).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    need(
        payload_hash(get(&authority, "scope"))? == payload_hash(scope)?
            && payload_hash(get(&authority, "allocation"))? == payload_hash(allocation)?
            && get(get(&authority, "priorActor"), "sha256").as_str() == Some(sha(&bytes).as_str())
            && get(get(&authority, "priorActor"), "bytes").as_f64() == Some(bytes.len() as f64)
            && get(get(&authority, "priorActor"), "hash")
                == get(get(&replay, "anchor"), "hash"),
        "EXACT_AUTHORITY_AND_SOURCE",
    )?;
    // const {seedHash,...old}=base
    let mut old = base.as_object().cloned().unwrap_or_default();
    old.shift_remove("seedHash");
    let mut orders = get(&V::Object(old.clone()), "inheritedClosedOrders")
        .as_array()
        .cloned()
        .unwrap_or_default();
    orders.extend(get(s, "orders").as_array().cloned().unwrap_or_default());
    let mut sources: Vec<V> = Vec::new();
    for file in [&req_file, &actor_file, &authority_file] {
        let b = fs::read(file).map_err(|e| e.to_string())?;
        sources.push(json!({
            "relative": rel(runtime_dir, file),
            "bytes": b.len() as i64,
            "sha256": sha(&b),
        }));
    }
    let mut seed = old.clone();
    seed.insert(
        "kind".into(),
        json!("baal_expansion_flat_custody_seed_v1"),
    );
    seed.insert("ownerId".into(), get(scope, "ownerId").clone());
    seed.insert("runId".into(), get(scope, "runId").clone());
    seed.insert("tournamentId".into(), get(scope, "tournamentId").clone());
    seed.insert("scopeHash".into(), json!(payload_hash(scope)?));
    seed.insert("allocationHash".into(), json!(payload_hash(allocation)?));
    seed.insert("inventory".into(), json!({}));
    seed.insert("ownedRuleHashes".into(), json!({}));
    seed.insert("inheritedClosedOrders".into(), json!(orders));
    seed.insert("inheritedPairAssignments".into(), json!([]));
    seed.insert("inheritedPairRetirements".into(), json!([]));
    seed.insert(
        "originalActivation".into(),
        get(&authority, "activationBounds").clone(),
    );
    seed.insert("expansionSources".into(), json!(sources));
    seed.insert(
        "archivedPrior".into(),
        json!({
            "runId": get(&r, "runId"),
            "actorAnchor": get(&replay, "anchor"),
            "inventory": get(s, "inventory"),
            "closedIntentCount": orders.len() as i64,
        }),
    );
    seed.insert(
        "summary".into(),
        json!({
            "ownedQuantity": 0,
            "ownedPrincipal": "0.000000",
            "realizedPnl": "0.000000",
            "inheritedOrders": orders.len() as i64,
            "abandonedOrders": orders
                .iter()
                .filter(|o| sget(o, "phase") == Some("abandoned"))
                .count() as i64,
        }),
    );
    seed.insert("executionAuthority".into(), json!(false));
    let mut out = seed.clone();
    out.insert("seedHash".into(), json!(payload_hash(&V::Object(seed))?));
    Ok(V::Object(out))
}

/// validateBaalExpansionCustodySeedV1(seed,scope,allocation) — returns the
/// seedHash value verbatim (a hex string in every valid seed).
pub fn validate_baal_expansion_custody_seed_v1(
    seed: &V,
    scope: &V,
    allocation: &V,
) -> Result<V, String> {
    // const {seedHash,...body}=seed??{}
    let mut body = seed.as_object().cloned().unwrap_or_default();
    body.shift_remove("seedHash");
    let seed_hash = sget(seed, "seedHash");
    need(
        sget(seed, "kind") == Some("baal_expansion_flat_custody_seed_v1")
            && seed.get("executionAuthority") == Some(&V::Bool(false))
            && seed_hash == Some(payload_hash(&V::Object(body))?.as_str())
            && get(seed, "scopeHash").as_str() == Some(payload_hash(scope)?.as_str())
            && get(seed, "allocationHash").as_str() == Some(payload_hash(allocation)?.as_str()),
        "HASH_SCOPE",
    )?;
    for k in ["ownerId", "runId", "tournamentId"] {
        need(get(seed, k) == get(scope, k), "ATTRIBUTION")?;
    }
    // Object.keys(seed.inventory).length===0: objects/arrays count elements,
    // strings count units, numbers/bools have none; null/undefined would throw
    // a TypeError in JS and surfaces here as FLAT_CLOSED.
    let inventory_empty = match get(seed, "inventory") {
        V::Object(m) => m.is_empty(),
        V::Array(a) => a.is_empty(),
        V::String(s) => s.is_empty(),
        V::Number(_) | V::Bool(_) => true,
        V::Null => false,
    };
    need(
        inventory_empty
            && get(seed, "inheritedClosedOrders")
                .as_array()
                .map(|orders| {
                    orders.iter().all(|o| {
                        js_truthy(o.get("proof")) || sget(o, "phase") == Some("abandoned")
                    })
                })
                .unwrap_or(false),
        "FLAT_CLOSED",
    )?;
    Ok(get(seed, "seedHash").clone())
}
