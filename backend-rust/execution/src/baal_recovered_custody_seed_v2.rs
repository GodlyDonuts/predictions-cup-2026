// execution/baal-recovered-custody-seed-v2.mjs — the base generic custody seed
// extended by a verified blocked-admission recovery replay. The new scope may
// widen, but prior inventory/history must be this exact base.
use crate::baal_generic_custody_seed_v1::build_baal_generic_custody_seed_v1;
use crate::baal_generic_no_controller_v4::{
    baal_generic_no_controller_v4_identity, replay_baal_generic_no_journal_v4,
};
use crate::blocked_admission_reconciliation_v1::reconcile_blocked_admission_v1;
use crate::core::payload_hash;
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Map, Value as V};
use std::fs;
use std::path::{Path, PathBuf};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("RECOVERED_CUSTODY_{code}"))
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
// path.resolve(dir,rel): joins then lexically removes . and .. segments.
fn resolve(root: &Path, rel: &str) -> PathBuf {
    root.join(rel).components().collect()
}

/// buildBaalRecoveredCustodySeedV2({frontier,scope,allocation,runtimeDir})
pub fn build_baal_recovered_custody_seed_v2(
    frontier: &V,
    scope: &V,
    allocation: &V,
    runtime_dir: &Path,
) -> Result<V, String> {
    let base = build_baal_generic_custody_seed_v1(frontier, scope, allocation)?;
    let root: PathBuf = runtime_dir.components().collect();
    let marker_file = resolve(&root, "blocked-admission-recovery-request.json");
    let result_file = resolve(&root, "blocked-admission-recovery-result.json");
    let marker_bytes = fs::read(&marker_file).map_err(|e| e.to_string())?;
    let result_bytes = fs::read(&result_file).map_err(|e| e.to_string())?;
    let m: V = serde_json::from_slice(&marker_bytes).map_err(|e| e.to_string())?;
    let r: V = serde_json::from_slice(&result_bytes).map_err(|e| e.to_string())?;
    need(
        get(&r, "markerHash").as_str() == Some(sha(&marker_bytes).as_str())
            && get(&m, "runId") == get(scope, "runId"),
        "REQUEST_RESULT",
    )?;
    let actor_file = resolve(&root, sget(&m, "actorRelative").unwrap_or(""));
    need(actor_file.starts_with(&root), "ACTOR_PATH")?;
    let actor = fs::read(&actor_file).map_err(|e| e.to_string())?;
    need(
        sha(&actor) == sget(&m, "actorSha256").unwrap_or(""),
        "ACTOR_SEAL",
    )?;
    let actor_text = String::from_utf8_lossy(&actor).to_string();
    let rows: Vec<V> = actor_text
        .trim_end()
        .split('\n')
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    let prior_seed = get(
        &get(&get(&rows[0], "event"), "input"),
        "custodySeed",
    )
    .clone();
    // The new scope may widen, but prior inventory/history must be this exact base.
    need(
        get(&prior_seed, "sourceStateHash") == get(&base, "sourceStateHash")
            && payload_hash(get(&prior_seed, "inventory"))?
                == payload_hash(get(&base, "inventory"))?
            && payload_hash(get(&prior_seed, "inheritedClosedOrders"))?
                == payload_hash(get(&base, "inheritedClosedOrders"))?,
        "ORIGINAL_CUSTODY",
    )?;
    let replay = replay_baal_generic_no_journal_v4(
        &actor_text,
        &baal_generic_no_controller_v4_identity()?,
        &prior_seed,
    )?;
    let timing_file = runtime_dir.join("read-timings.jsonl");
    let timing_bytes = fs::read(&timing_file).map_err(|e| e.to_string())?;
    let prefix_len = get(&m, "timingPrefixBytes").as_u64().unwrap_or(u64::MAX) as usize;
    let timing: Vec<u8> = timing_bytes.into_iter().take(prefix_len).collect();
    need(
        timing.len() == prefix_len
            && sha(&timing) == sget(&m, "timingPrefixHash").unwrap_or(""),
        "TIMING_PREFIX",
    )?;
    let timings: Vec<V> = String::from_utf8_lossy(&timing)
        .trim_end()
        .split('\n')
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    let events: Vec<V> = rows
        .iter()
        .map(|x| get(&get(x, "event"), "input").clone())
        .collect();
    let closure = reconcile_blocked_admission_v1(&json!({
        "state": get(&replay, "state"),
        "events": events,
        "timings": timings,
        "after": get(&r, "after"),
        "at": get(&get(&r, "result"), "proof").get("at").cloned().unwrap_or(V::Null),
    }))?;
    need(
        payload_hash(&closure)? == payload_hash(get(&r, "result"))?,
        "EXACT_RECOVERY_REPLAY",
    )?;
    let closure_orders = get(&get(&closure, "state"), "orders")
        .as_array()
        .cloned()
        .unwrap_or_default();
    need(
        closure_orders
            .iter()
            .all(|o| sget(o, "phase") == Some("abandoned"))
            && payload_hash(get(&get(&closure, "state"), "inventory"))?
                == payload_hash(get(&base, "inventory"))?,
        "NO_NEW_LOTS_OR_PENDING",
    )?;
    let mut body: Map<String, V> = base.as_object().cloned().unwrap_or_default();
    body.shift_remove("seedHash");
    let base_inherited_len = get(&V::Object(body.clone()), "inheritedClosedOrders")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .len() as i64;
    let base_abandoned = get(&get(&V::Object(body.clone()), "summary"), "abandonedOrders")
        .as_i64()
        .unwrap_or(0);
    let recovery_sources: Vec<V> = [&marker_file, &result_file, &actor_file]
        .iter()
        .map(|file| {
            let bytes = fs::read(file).map_err(|e| e.to_string())?;
            Ok(json!({
                "relative": file
                    .strip_prefix(&root)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default(),
                "bytes": bytes.len(),
                "sha256": sha(&bytes),
            }))
        })
        .collect::<Result<_, String>>()?;
    let mut inherited = get(&V::Object(body.clone()), "inheritedClosedOrders")
        .as_array()
        .cloned()
        .unwrap_or_default();
    inherited.extend(closure_orders.iter().cloned());
    let mut summary = get(&V::Object(body.clone()), "summary")
        .as_object()
        .cloned()
        .unwrap_or_default();
    summary.insert(
        "inheritedOrders".into(),
        json!(base_inherited_len + closure_orders.len() as i64),
    );
    summary.insert(
        "abandonedOrders".into(),
        json!(base_abandoned + closure_orders.len() as i64),
    );
    // Key order mirrors the JS literal: existing keys keep position; the four
    // recovery keys append before seedHash.
    body.insert("kind".into(), json!("baal_recovered_custody_seed_v2"));
    body.insert("inheritedClosedOrders".into(), V::Array(inherited));
    body.insert("recoverySources".into(), json!(recovery_sources));
    body.insert(
        "recoveryProofHash".into(),
        json!(payload_hash(get(&closure, "proof"))?),
    );
    body.insert("recoveryActorAnchor".into(), get(&replay, "anchor").clone());
    body.insert(
        "recoveryTimingPrefix".into(),
        json!({
            "relative": "read-timings.jsonl",
            "bytes": timing.len(),
            "sha256": sha(&timing),
        }),
    );
    body.insert("summary".into(), V::Object(summary));
    let seed_hash = payload_hash(&V::Object(body.clone()))?;
    body.insert("seedHash".into(), json!(seed_hash));
    Ok(V::Object(body))
}

/// validateBaalRecoveredCustodySeedV2(seed,scope,allocation)
pub fn validate_baal_recovered_custody_seed_v2(
    seed: &V,
    scope: &V,
    allocation: &V,
) -> Result<V, String> {
    let mut body = seed.as_object().cloned().unwrap_or_default();
    let seed_hash = body.shift_remove("seedHash").unwrap_or(V::Null);
    need(
        sget(seed, "kind") == Some("baal_recovered_custody_seed_v2")
            && get(seed, "executionAuthority") == &V::Bool(false)
            && payload_hash(&V::Object(body))? == seed_hash.as_str().unwrap_or(""),
        "HASH",
    )?;
    need(
        get(seed, "scopeHash").as_str() == Some(payload_hash(scope)?.as_str())
            && get(seed, "allocationHash").as_str()
                == Some(payload_hash(allocation)?.as_str()),
        "CONFIGURATION",
    )?;
    for k in ["ownerId", "runId", "tournamentId"] {
        need(get(seed, k) == get(scope, k), "ATTRIBUTION")?;
    }
    need(
        get(seed, "inheritedClosedOrders")
            .as_array()
            .map(|a| {
                a.iter()
                    .all(|o| !get(o, "proof").is_null() || sget(o, "phase") == Some("abandoned"))
            })
            .unwrap_or(false),
        "PENDING",
    )?;
    Ok(seed_hash)
}
