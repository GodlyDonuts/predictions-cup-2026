// execution/baal-expansion-recovered-seed-v2.mjs — fold an affirmatively
// physical-guard-blocked run into the flat expansion seed: replay the sealed
// v8 journal, verify the sealed financial stop, and pin marker/result/actor/
// stop files as extra recovery sources.
//
// Port notes (same conventions as the sibling seeds):
// - `need` throws Error('EXPANSION_RECOVERED_<code>') — bare code strings.
// - `path.resolve(rt,m.actorRelative)` -> resolve(); `af.startsWith(rt+sep)`
//   is strictly-inside: af must live strictly below the runtime root.
// - `r.result.proof.at` and `m.actorRelative`/`timingPrefixBytes`-style member
//   access on nullish values is a JS TypeError; non-object primitives degrade
//   to undefined and the equality needs fail instead.
// - `x.proof` truthy || phase==='abandoned'; Object.values(null)/non-array
//   `.every` are TypeError-equivalents.
// - {...old,k:v} key order: existing keys keep position; the single new key
//   (currentRecoveryProofHash) appends; {...seed,seedHash} appends last.
use crate::baal_expansion_custody_seed_v1::build_baal_expansion_custody_seed_v1;
use crate::baal_generic_no_controller_v8::{
    baal_generic_no_controller_v8_identity, replay_baal_generic_no_journal_v8,
};
use crate::core::payload_hash;
use crate::physical_guard_reconciliation_v1::reconcile_physical_guard_admission_v1;
use baal_service_runtime::json_text::{js_truthy, sha256};
use serde_json::{json, Map, Value as V};
use std::fs;
use std::path::{Component, Path, PathBuf};

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
fn type_err() -> String {
    "TYPE_ERROR".to_string()
}
// Object.values(v): objects -> member values; arrays -> elements; strings /
// null / absent -> TypeError; numbers and booleans -> [] (vacuous every).
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
// path.resolve(root, rel): join, then lexically collapse '.'/'..' segments.
fn resolve(root: &Path, rel: &str) -> PathBuf {
    let mut out: Vec<Component> = root.components().collect();
    for c in Path::new(rel).components() {
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

/// buildBaalExpansionRecoveredSeedV2({frontier,scope,allocation,runtimeDir})
pub fn build_baal_expansion_recovered_seed_v2(
    frontier: &V,
    scope: &V,
    allocation: &V,
    runtime_dir: &Path,
) -> Result<V, String> {
    let base =
        build_baal_expansion_custody_seed_v1(frontier, scope, allocation, runtime_dir)?;
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
    let root: PathBuf = rt.components().collect();
    let af = resolve(&root, sget(&m, "actorRelative").unwrap_or(""));
    need(af.starts_with(&root) && af != root, "PATH")?;
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
    let timing_bytes = fs::read(rt.join("read-timings.jsonl")).map_err(|e| e.to_string())?;
    let prefix_len = get(&m, "timingPrefixBytes").as_u64().unwrap_or(u64::MAX) as usize;
    let tb: Vec<u8> = timing_bytes.into_iter().take(prefix_len).collect();
    need(
        tb.len() == prefix_len && sha(&tb) == sget(&m, "timingPrefixHash").unwrap_or(""),
        "TIMING",
    )?;
    let stop_file = rt.join("physical-guard-financial-stop-sealed-v1.json");
    let stop_bytes = fs::read(&stop_file).map_err(|e| e.to_string())?;
    need(
        sha(&stop_bytes) == sget(&m, "financialStopHash").unwrap_or(""),
        "EXACT_FINANCIAL_STOP",
    )?;
    let at = {
        let result = get(&r, "result");
        if result.is_null() {
            return Err(type_err());
        }
        let proof = get(result, "proof");
        if proof.is_null() {
            return Err(type_err());
        }
        get(proof, "at").clone()
    };
    let closed = reconcile_physical_guard_admission_v1(&json!({
        "state": get(&replay, "state"),
        "events": rows.iter().map(|x| get(&get(x, "event"), "input").clone()).collect::<Vec<V>>(),
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
        "at": at,
    }))?;
    need(
        payload_hash(&closed)? == payload_hash(get(&r, "result"))?,
        "EXACT_RECOVERY",
    )?;
    let closed_state = get(&closed, "state");
    need(
        get(closed_state, "orders")
            .as_array()
            .ok_or_else(type_err)?
            .iter()
            .all(|o| js_truthy(o.get("proof")) || sget(o, "phase") == Some("abandoned"))
            && object_values(get(closed_state, "inventory"))?
                .iter()
                .all(|p| get(get(p, "proof"), "remainingOwnedQuantity") == &json!(0)),
        "FLAT_CLOSED",
    )?;
    let mut body: Map<String, V> = base.as_object().cloned().unwrap_or_default();
    body.shift_remove("seedHash");
    let extra: Vec<V> = [mf, rf, af.clone(), stop_file]
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
    let mut orders: Vec<V> = get(&base, "inheritedClosedOrders")
        .as_array()
        .ok_or_else(type_err)?
        .clone();
    orders.extend(
        get(closed_state, "orders")
            .as_array()
            .ok_or_else(type_err)?
            .iter()
            .cloned(),
    );
    let mut expansion_sources: Vec<V> = get(&base, "expansionSources")
        .as_array()
        .ok_or_else(type_err)?
        .clone();
    expansion_sources.extend(extra);
    let mut summary = get(&base, "summary").as_object().cloned().unwrap_or_default();
    summary.insert("inheritedOrders".into(), json!(orders.len() as i64));
    summary.insert(
        "abandonedOrders".into(),
        json!(orders
            .iter()
            .filter(|o| sget(o, "phase") == Some("abandoned"))
            .count() as i64),
    );
    body.insert(
        "kind".into(),
        json!("baal_expansion_recovered_custody_seed_v2"),
    );
    body.insert("inheritedClosedOrders".into(), V::Array(orders));
    body.insert(
        "expansionSources".into(),
        V::Array(expansion_sources),
    );
    body.insert(
        "currentRecoveryProofHash".into(),
        json!(payload_hash(get(&closed, "proof"))?),
    );
    body.insert("summary".into(), V::Object(summary));
    let seed_hash = payload_hash(&V::Object(body.clone()))?;
    body.insert("seedHash".into(), json!(seed_hash));
    Ok(V::Object(body))
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
