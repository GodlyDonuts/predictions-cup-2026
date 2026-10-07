// execution/baal-expansion-custody-seed-v1.mjs — new flat run, with complete
// verified old custody preserved as an immutable archive. Historical gains are
// not imported into the new run's spending/profit buffer.
//
// Port notes (same conventions as the sibling seeds):
// - `need` throws Error('EXPANSION_SEED_<code>') — bare code strings.
// - The request's own scope/allocation (not the caller's) seed the v4 base.
// - `authority.priorActor` must be a real object: property access on a nullish
//   value would be a JS TypeError; on a primitive it yields undefined and the
//   `===` checks fail into EXACT_AUTHORITY_AND_SOURCE instead.
// - `===` on JSON values is strict: objects/arrays are never ===-equal here
//   (distinct parses), reproduced by strict_eq for the two hash fields.
// - `.every` on a non-array `orders`/`inventory` is a TypeError-equivalent.
// - {...old,k:v} key order: existing keys keep position; the four new keys
//   (inheritedPairAssignments/Retirements/expansionSources/archivedPrior)
//   append in literal order, then {...seed,seedHash} appends seedHash last.
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
fn type_err() -> String {
    "TYPE_ERROR".to_string()
}
// Object.values(v): objects -> member values; arrays -> elements; strings /
// null / absent -> TypeError (chars have no usable .proof); numbers and
// booleans -> [] (vacuous every).
fn object_values(v: &V) -> Result<Vec<V>, String> {
    match v {
        V::Object(m) => Ok(m.values().cloned().collect()),
        V::Array(a) => Ok(a.clone()),
        V::Null => Err(type_err()),
        V::String(_) => Err(type_err()),
        _ => Ok(vec![]),
    }
}
// Object.keys(v).length: objects -> member count; arrays/strings -> index
// count; numbers/booleans -> 0; null/absent -> TypeError.
fn object_keys_len(v: &V) -> Result<usize, String> {
    match v {
        V::Object(m) => Ok(m.len()),
        V::Array(a) => Ok(a.len()),
        V::String(s) => Ok(s.encode_utf16().count()),
        V::Null => Err(type_err()),
        _ => Ok(0),
    }
}
// JS `a===b` over JSON values: primitives compare by value, objects by
// reference (distinct parses -> always false).
fn strict_eq(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Null, V::Null) => true,
        (V::Bool(x), V::Bool(y)) => x == y,
        (V::Number(x), V::Number(y)) => x == y,
        (V::String(x), V::String(y)) => x == y,
        _ => false,
    }
}
// path.join(dir, segments...) + normalize: collapse '.'/'..' lexically.
fn join(root: &Path, segments: &[&str]) -> PathBuf {
    let mut out: Vec<Component> = root.components().collect();
    for seg in segments {
        for c in Path::new(seg).components() {
            match c {
                Component::CurDir => {}
                Component::ParentDir => {
                    if matches!(out.last(), Some(Component::Normal(_))) {
                        out.pop();
                    } else {
                        out.push(c);
                    }
                }
                other => out.push(other),
            }
        }
    }
    out.iter().collect()
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
    let base =
        build_baal_cohort_recovered_custody_seed_v4(frontier, get(&r, "scope"), get(&r, "allocation"), runtime_dir)?;
    // path.join(runtimeDir,'baal-generic-live-runs',runId,epoch,'controller',..)
    // rejects a non-string segment with a JS TypeError.
    let run_id = sget(&r, "runId").ok_or_else(type_err)?;
    let epoch = sget(&r, "epoch").ok_or_else(type_err)?;
    let actor_file = join(
        runtime_dir,
        &[
            "baal-generic-live-runs",
            run_id,
            epoch,
            "controller",
            "baal-generic-no-v7-journal.jsonl",
        ],
    );
    let bytes = fs::read(&actor_file).map_err(|e| e.to_string())?;
    let replay = replay_baal_generic_no_journal_v7(
        &String::from_utf8_lossy(&bytes),
        &baal_generic_no_controller_v7_identity()?,
        &base,
    )?;
    let s = get(&replay, "state");
    let inventory = get(s, "inventory");
    let inventory_slots_ok = object_values(inventory)?
        .iter()
        .all(|x| get(get(x, "proof"), "remainingOwnedQuantity") == &json!(0));
    let orders_flat = get(s, "orders")
        .as_array()
        .ok_or_else(type_err)?
        .iter()
        .all(|o| {
            (js_truthy(o.get("proof")) && sget(o, "phase") == Some("reconciled"))
                || sget(o, "phase") == Some("abandoned")
        });
    need(
        inventory_slots_ok && orders_flat,
        "PRIOR_NOT_FLAT_OR_PENDING",
    )?;
    let authority_file = runtime_dir.join("baal-expansion-authority-v1.json");
    let authority: V = serde_json::from_slice(
        &fs::read(&authority_file).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let prior_actor = get(&authority, "priorActor");
    if prior_actor.is_null() {
        return Err(type_err());
    }
    need(
        payload_hash(get(&authority, "scope"))? == payload_hash(scope)?
            && payload_hash(get(&authority, "allocation"))? == payload_hash(allocation)?
            && sget(prior_actor, "sha256") == Some(sha(&bytes).as_str())
            && get(prior_actor, "bytes").as_f64() == Some(bytes.len() as f64)
            && strict_eq(
                get(prior_actor, "hash"),
                get(&get(&replay, "anchor"), "hash"),
            ),
        "EXACT_AUTHORITY_AND_SOURCE",
    )?;
    let mut body: Map<String, V> = base.as_object().cloned().unwrap_or_default();
    body.shift_remove("seedHash");
    let mut orders: Vec<V> = get(&base, "inheritedClosedOrders")
        .as_array()
        .ok_or_else(type_err)?
        .clone();
    orders.extend(
        get(s, "orders")
            .as_array()
            .ok_or_else(type_err)?
            .iter()
            .cloned(),
    );
    let root: PathBuf = runtime_dir.components().collect();
    let sources: Vec<V> = [req_file, actor_file.clone(), authority_file]
        .iter()
        .map(|file| {
            let resolved: PathBuf = file.components().collect();
            let b = fs::read(file).map_err(|e| e.to_string())?;
            Ok(json!({
                "relative": rel(&root, &resolved),
                "bytes": b.len(),
                "sha256": sha(&b),
            }))
        })
        .collect::<Result<_, String>>()?;
    let abandoned = orders
        .iter()
        .filter(|o| sget(o, "phase") == Some("abandoned"))
        .count() as i64;
    body.insert(
        "kind".into(),
        json!("baal_expansion_flat_custody_seed_v1"),
    );
    body.insert("ownerId".into(), get(scope, "ownerId").clone());
    body.insert("runId".into(), get(scope, "runId").clone());
    body.insert(
        "tournamentId".into(),
        get(scope, "tournamentId").clone(),
    );
    body.insert("scopeHash".into(), json!(payload_hash(scope)?));
    body.insert(
        "allocationHash".into(),
        json!(payload_hash(allocation)?),
    );
    body.insert("inventory".into(), json!({}));
    body.insert("ownedRuleHashes".into(), json!({}));
    body.insert("inheritedClosedOrders".into(), V::Array(orders.clone()));
    body.insert("inheritedPairAssignments".into(), json!([]));
    body.insert("inheritedPairRetirements".into(), json!([]));
    body.insert(
        "originalActivation".into(),
        get(&authority, "activationBounds").clone(),
    );
    body.insert("expansionSources".into(), json!(sources));
    body.insert(
        "archivedPrior".into(),
        json!({
            "runId": get(&r, "runId"),
            "actorAnchor": get(&replay, "anchor"),
            "inventory": inventory,
            "closedIntentCount": orders.len() as i64,
        }),
    );
    body.insert(
        "summary".into(),
        json!({
            "ownedQuantity": 0,
            "ownedPrincipal": "0.000000",
            "realizedPnl": "0.000000",
            "inheritedOrders": orders.len() as i64,
            "abandonedOrders": abandoned,
        }),
    );
    body.insert("executionAuthority".into(), json!(false));
    let seed_hash = payload_hash(&V::Object(body.clone()))?;
    body.insert("seedHash".into(), json!(seed_hash));
    Ok(V::Object(body))
}

/// validateBaalExpansionCustodySeedV1(seed,scope,allocation)
pub fn validate_baal_expansion_custody_seed_v1(
    seed: &V,
    scope: &V,
    allocation: &V,
) -> Result<V, String> {
    let mut body = seed.as_object().cloned().unwrap_or_default();
    let seed_hash = body.shift_remove("seedHash").unwrap_or(V::Null);
    need(
        sget(seed, "kind") == Some("baal_expansion_flat_custody_seed_v1")
            && get(seed, "executionAuthority") == &V::Bool(false)
            && payload_hash(&V::Object(body))? == seed_hash
            && get(seed, "scopeHash").as_str() == Some(payload_hash(scope)?.as_str())
            && get(seed, "allocationHash").as_str()
                == Some(payload_hash(allocation)?.as_str()),
        "HASH_SCOPE",
    )?;
    for k in ["ownerId", "runId", "tournamentId"] {
        need(get(seed, k) == get(scope, k), "ATTRIBUTION")?;
    }
    let inventory = get(seed, "inventory");
    need(
        object_keys_len(inventory)? == 0
            && get(seed, "inheritedClosedOrders")
                .as_array()
                .ok_or_else(type_err)?
                .iter()
                .all(|o| js_truthy(o.get("proof")) || sget(o, "phase") == Some("abandoned")),
        "FLAT_CLOSED",
    )?;
    Ok(seed_hash)
}
