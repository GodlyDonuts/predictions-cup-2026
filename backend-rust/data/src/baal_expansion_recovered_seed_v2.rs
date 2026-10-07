// execution/baal-expansion-recovered-seed-v2.mjs — expansion custody seed
// plus a sealed physical-guard recovery: the affirmatively-blocked expired
// intent is reconciled to abandoned before the new run inherits custody.
//
// Port notes:
// - subarray(0, m.timingPrefixBytes) clamps like Buffer.subarray.
// - path.resolve(rt, m.actorRelative) uses the same lexical resolver as the
//   v2 seed port; the startsWith check compares against resolve(rt)+sep.
// - `o.proof || o.phase === 'abandoned'` -> js_truthy(proof) || phase.
use crate::baal_generic_no_controller_v8::{
    baal_generic_no_controller_v8_identity, replay_baal_generic_no_journal_v8,
};
use baal_execution::core::payload_hash;
use baal_service_runtime::json_text::{js_int, js_truthy, sha256};
use serde_json::{json, Map, Value as V};
use std::fs;
use std::path::{Path, PathBuf};

use crate::baal_expansion_custody_seed_v1::build_baal_expansion_custody_seed_v1;
use crate::physical_guard_reconciliation_v1::reconcile_physical_guard_admission_v1;

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("EXPANSION_RECOVERED_{code}"))
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
/// path.resolve(root, rel) — lexical normalization incl. `..` folding.
fn resolve(root: &Path, rel: &str) -> PathBuf {
    let mut out = root.to_path_buf();
    for comp in Path::new(rel).components() {
        use std::path::Component::*;
        match comp {
            Normal(c) => out.push(c),
            ParentDir => {
                out.pop();
            }
            CurDir => {}
            RootDir | Prefix(_) => out = comp.as_os_str().into(),
        }
    }
    out
}

/// buildBaalExpansionRecoveredSeedV2(args)
pub fn build_baal_expansion_recovered_seed_v2(
    frontier: &V,
    scope: &V,
    allocation: &V,
    runtime_dir: &Path,
) -> Result<V, String> {
    let base = build_baal_expansion_custody_seed_v1(frontier, scope, allocation, runtime_dir)?;
    let rt = runtime_dir;
    let mf = rt.join("physical-guard-recovery-request-v1.json");
    let rf = rt.join("physical-guard-recovery-result-v1.json");
    let mb = fs::read(&mf).map_err(|e| e.to_string())?;
    let m: V = serde_json::from_slice(&mb).map_err(|e| e.to_string())?;
    let r: V = serde_json::from_slice(&fs::read(&rf).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    need(
        get(&r, "markerHash").as_str() == Some(sha(&mb).as_str()),
        "MARKER",
    )?;
    let root = resolve(Path::new("/"), &rt.to_string_lossy());
    let af = resolve(&root, sget(&m, "actorRelative").unwrap_or(""));
    let root_prefix = format!("{}{}", root.to_string_lossy(), std::path::MAIN_SEPARATOR);
    need(
        af.to_string_lossy().starts_with(&root_prefix),
        "PATH",
    )?;
    let ab = fs::read(&af).map_err(|e| e.to_string())?;
    need(
        sha(&ab) == sget(&m, "actorSha256").unwrap_or(""),
        "ACTOR",
    )?;
    let actor_text = String::from_utf8_lossy(&ab).to_string();
    let rows: Vec<V> = actor_text
        .trim_end()
        .split('\n')
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    let replay = replay_baal_generic_no_journal_v8(
        &actor_text,
        &baal_generic_no_controller_v8_identity()?,
        &base,
    )?;
    let timing_all = fs::read(rt.join("read-timings.jsonl")).map_err(|e| e.to_string())?;
    let prefix_len = js_int(get(&m, "timingPrefixBytes")).unwrap_or(-1).max(0) as usize;
    let tb: Vec<u8> = timing_all
        .iter()
        .take(prefix_len)
        .cloned()
        .collect();
    need(
        tb.len() as i64 == js_int(get(&m, "timingPrefixBytes")).unwrap_or(-1)
            && sha(&tb) == sget(&m, "timingPrefixHash").unwrap_or(""),
        "TIMING",
    )?;
    let stop_file = rt.join("physical-guard-financial-stop-sealed-v1.json");
    let stop_bytes = fs::read(&stop_file).map_err(|e| e.to_string())?;
    need(
        sha(&stop_bytes) == sget(&m, "financialStopHash").unwrap_or(""),
        "EXACT_FINANCIAL_STOP",
    )?;
    let closed = reconcile_physical_guard_admission_v1(&json!({
        "state": get(&replay, "state"),
        "events": rows
            .iter()
            .map(|x| get(&get(x, "event"), "input").clone())
            .collect::<Vec<V>>(),
        "timings": String::from_utf8_lossy(&tb)
            .trim_end()
            .split('\n')
            .map(serde_json::from_str)
            .collect::<Result<Vec<V>, _>>()
            .map_err(|e| e.to_string())?,
        "diagnostic": get(&m, "diagnostic"),
        "guardFailure": get(&m, "guardFailure"),
        "stop": serde_json::from_slice::<V>(&stop_bytes).map_err(|e| e.to_string())?,
        "after": get(&r, "after"),
        "at": get(&get(&r, "result"), "proof").get("at").cloned().unwrap_or(V::Null),
    }))?;
    need(
        payload_hash(&closed)? == payload_hash(get(&r, "result"))?,
        "EXACT_RECOVERY",
    )?;
    need(
        get(&get(&closed, "state"), "orders")
            .as_array()
            .map(|a| {
                a.iter().all(|o| {
                    js_truthy(o.get("proof")) || sget(o, "phase") == Some("abandoned")
                })
            })
            .unwrap_or(false)
            && get(&get(&closed, "state"), "inventory")
                .as_object()
                .map(|im| {
                    im.values()
                        .all(|p| js_int(get(get(p, "proof"), "remainingOwnedQuantity")) == Some(0))
                })
                .unwrap_or(false),
        "FLAT_CLOSED",
    )?;
    let extra: Vec<V> = [&mf, &rf, &af, &stop_file]
        .iter()
        .map(|file| {
            let b = fs::read(file).map_err(|e| e.to_string())?;
            Ok(json!({
                "relative": file
                    .strip_prefix(&root)
                    .unwrap_or(file)
                    .to_string_lossy()
                    .to_string(),
                "bytes": b.len() as i64,
                "sha256": sha(&b),
            }))
        })
        .collect::<Result<_, String>>()?;
    let mut orders: Vec<V> = get(&base, "inheritedClosedOrders")
        .as_array()
        .cloned()
        .unwrap_or_default();
    orders.extend(
        get(&get(&closed, "state"), "orders")
            .as_array()
            .cloned()
            .unwrap_or_default(),
    );
    let mut expansion_sources: Vec<V> = get(&base, "expansionSources")
        .as_array()
        .cloned()
        .unwrap_or_default();
    expansion_sources.extend(extra);
    let mut summary: Map<String, V> = get(&base, "summary")
        .as_object()
        .cloned()
        .unwrap_or_default();
    summary.insert("inheritedOrders".into(), json!(orders.len() as i64));
    summary.insert(
        "abandonedOrders".into(),
        json!(orders
            .iter()
            .filter(|o| sget(o, "phase") == Some("abandoned"))
            .count() as i64),
    );
    let mut seed: Map<String, V> = base.as_object().cloned().unwrap_or_default();
    seed.shift_remove("seedHash");
    seed.insert("kind".into(), json!("baal_expansion_recovered_custody_seed_v2"));
    seed.insert("inheritedClosedOrders".into(), V::Array(orders));
    seed.insert("expansionSources".into(), V::Array(expansion_sources));
    seed.insert(
        "currentRecoveryProofHash".into(),
        json!(payload_hash(get(&closed, "proof"))?),
    );
    seed.insert("summary".into(), V::Object(summary));
    let hash = payload_hash(&V::Object(seed.clone()))?;
    seed.insert("seedHash".into(), json!(hash));
    Ok(V::Object(seed))
}

/// validateBaalExpansionRecoveredSeedV2(seed,scope,allocation)
pub fn validate_baal_expansion_recovered_seed_v2(
    seed: &V,
    scope: &V,
    allocation: &V,
) -> Result<V, String> {
    let mut body = seed.as_object().cloned().unwrap_or_default();
    let seed_hash = body.shift_remove("seedHash").unwrap_or(V::Null);
    need(
        sget(seed, "kind") == Some("baal_expansion_recovered_custody_seed_v2")
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
