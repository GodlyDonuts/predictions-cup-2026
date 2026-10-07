// execution/baal-expansion-custody-seed-v1.mjs — new flat run, with complete
// verified old custody preserved as an immutable archive. Historical gains
// are not imported into the new run's spending/profit buffer.
//
// Port notes:
// - `x.proof.remainingOwnedQuantity === 0` uses js_int (whole-number check).
// - `o.proof && o.phase === 'reconciled'` -> js_truthy(proof) && phase, then
//   `|| o.phase === 'abandoned'` (JS &&/|| precedence preserved).
// - Object spread {...old, ...new keys}: seedHash is removed; replaced keys
//   keep their position, new keys append in literal order (IndexMap insert).
// - path.relative(runtimeDir, file) -> strip_prefix (files are direct joins).
use baal_execution::baal_cohort_recovered_custody_seed_v4::build_baal_cohort_recovered_custody_seed_v4;
use baal_execution::baal_generic_no_controller_v7::{
    baal_generic_no_controller_v7_identity, replay_baal_generic_no_journal_v7,
};
use baal_execution::core::payload_hash;
use baal_service_runtime::json_text::{js_int, js_truthy, sha256};
use serde_json::{json, Map, Value as V};
use std::fs;
use std::path::Path;

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
fn read_json(file: &Path) -> Result<V, String> {
    serde_json::from_slice(&fs::read(file).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

/// buildBaalExpansionCustodySeedV1({frontier,scope,allocation,runtimeDir})
pub fn build_baal_expansion_custody_seed_v1(
    frontier: &V,
    scope: &V,
    allocation: &V,
    runtime_dir: &Path,
) -> Result<V, String> {
    let req_file = runtime_dir.join("cohort-profit-lock-request-v2.json");
    let r = read_json(&req_file)?;
    let base = build_baal_cohort_recovered_custody_seed_v4(
        frontier,
        get(&r, "scope"),
        get(&r, "allocation"),
        runtime_dir,
    )?;
    let actor_file = runtime_dir
        .join("baal-generic-live-runs")
        .join(sget(&r, "runId").unwrap_or(""))
        .join(sget(&r, "epoch").unwrap_or(""))
        .join("controller")
        .join("baal-generic-no-v7-journal.jsonl");
    let bytes = fs::read(&actor_file).map_err(|e| e.to_string())?;
    let replay = replay_baal_generic_no_journal_v7(
        &String::from_utf8_lossy(&bytes),
        &baal_generic_no_controller_v7_identity()?,
        &base,
    )?;
    let s = get(&replay, "state");
    need(
        get(s, "inventory")
            .as_object()
            .map(|m| {
                m.values()
                    .all(|x| js_int(get(get(x, "proof"), "remainingOwnedQuantity")) == Some(0))
            })
            .unwrap_or(false)
            && get(s, "orders")
                .as_array()
                .map(|a| {
                    a.iter().all(|o| {
                        (js_truthy(o.get("proof")) && sget(o, "phase") == Some("reconciled"))
                            || sget(o, "phase") == Some("abandoned")
                    })
                })
                .unwrap_or(false),
        "PRIOR_NOT_FLAT_OR_PENDING",
    )?;
    let authority_file = runtime_dir.join("baal-expansion-authority-v1.json");
    let authority = read_json(&authority_file)?;
    need(
        payload_hash(get(&authority, "scope"))? == payload_hash(scope)?
            && payload_hash(get(&authority, "allocation"))? == payload_hash(allocation)?
            && get(&get(&authority, "priorActor"), "sha256").as_str()
                == Some(sha(&bytes).as_str())
            && js_int(get(&get(&authority, "priorActor"), "bytes"))
                == Some(bytes.len() as i64)
            && get(&get(&authority, "priorActor"), "hash")
                == get(&get(&replay, "anchor"), "hash"),
        "EXACT_AUTHORITY_AND_SOURCE",
    )?;
    let mut orders: Vec<V> = get(&base, "inheritedClosedOrders")
        .as_array()
        .cloned()
        .unwrap_or_default();
    orders.extend(
        get(s, "orders")
            .as_array()
            .cloned()
            .unwrap_or_default(),
    );
    let sources: Vec<V> = [&req_file, &actor_file, &authority_file]
        .iter()
        .map(|file| {
            let b = fs::read(file).map_err(|e| e.to_string())?;
            Ok(json!({
                "relative": file
                    .strip_prefix(runtime_dir)
                    .unwrap_or(file)
                    .to_string_lossy()
                    .to_string(),
                "bytes": b.len() as i64,
                "sha256": sha(&b),
            }))
        })
        .collect::<Result<_, String>>()?;
    let abandoned = orders
        .iter()
        .filter(|o| sget(o, "phase") == Some("abandoned"))
        .count() as i64;
    let mut seed: Map<String, V> = base.as_object().cloned().unwrap_or_default();
    seed.shift_remove("seedHash");
    seed.insert("kind".into(), json!("baal_expansion_flat_custody_seed_v1"));
    seed.insert("ownerId".into(), get(scope, "ownerId").clone());
    seed.insert("runId".into(), get(scope, "runId").clone());
    seed.insert("tournamentId".into(), get(scope, "tournamentId").clone());
    seed.insert("scopeHash".into(), json!(payload_hash(scope)?));
    seed.insert("allocationHash".into(), json!(payload_hash(allocation)?));
    seed.insert("inventory".into(), json!({}));
    seed.insert("ownedRuleHashes".into(), json!({}));
    seed.insert("inheritedClosedOrders".into(), V::Array(orders.clone()));
    seed.insert("inheritedPairAssignments".into(), json!([]));
    seed.insert("inheritedPairRetirements".into(), json!([]));
    seed.insert(
        "originalActivation".into(),
        get(&authority, "activationBounds").clone(),
    );
    seed.insert("expansionSources".into(), V::Array(sources));
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
            "abandonedOrders": abandoned,
        }),
    );
    seed.insert("executionAuthority".into(), json!(false));
    let hash = payload_hash(&V::Object(seed.clone()))?;
    seed.insert("seedHash".into(), json!(hash));
    Ok(V::Object(seed))
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
            && payload_hash(&V::Object(body))?.as_str() == seed_hash.as_str().unwrap_or("")
            && get(seed, "scopeHash").as_str() == Some(payload_hash(scope)?.as_str())
            && get(seed, "allocationHash").as_str()
                == Some(payload_hash(allocation)?.as_str()),
        "HASH_SCOPE",
    )?;
    for k in ["ownerId", "runId", "tournamentId"] {
        need(get(seed, k) == get(scope, k), "ATTRIBUTION")?;
    }
    need(
        get(seed, "inventory")
            .as_object()
            .map(|m| m.is_empty())
            .unwrap_or(false)
            && get(seed, "inheritedClosedOrders")
                .as_array()
                .map(|a| {
                    a.iter().all(|o| {
                        js_truthy(o.get("proof")) || sget(o, "phase") == Some("abandoned")
                    })
                })
                .unwrap_or(false),
        "FLAT_CLOSED",
    )?;
    Ok(seed_hash)
}
