// execution/baal-recovered-custody-seed-v2.mjs — recovered custody seed: a
// successor may import this closure only together with sealed original actor
// and reconciliation receipts. No API, writes or lock changes.
use baal_execution::baal_generic_custody_seed_v1::build_baal_generic_custody_seed_v1;
use baal_execution::baal_generic_no_controller_v4::{
    baal_generic_no_controller_v4_identity, replay_baal_generic_no_journal_v4,
};
use baal_execution::blocked_admission_reconciliation_v1::reconcile_blocked_admission_v1;
use baal_execution::core::payload_hash;
use baal_service_runtime::json_text::{js_int, sha256};
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

/// buildBaalRecoveredCustodySeedV2({frontier,scope,allocation,runtimeDir})
pub fn build_baal_recovered_custody_seed_v2(
    frontier: &V,
    scope: &V,
    allocation: &V,
    runtime_dir: &Path,
) -> Result<V, String> {
    let base = build_baal_generic_custody_seed_v1(frontier, scope, allocation)?;
    let marker_file = runtime_dir.join("blocked-admission-recovery-request.json");
    let result_file = runtime_dir.join("blocked-admission-recovery-result.json");
    let marker_bytes = fs::read(&marker_file).map_err(|e| e.to_string())?;
    let result_bytes = fs::read(&result_file).map_err(|e| e.to_string())?;
    let m: V = serde_json::from_slice(&marker_bytes).map_err(|e| e.to_string())?;
    let r: V = serde_json::from_slice(&result_bytes).map_err(|e| e.to_string())?;
    need(
        get(&r, "markerHash").as_str() == Some(sha(&marker_bytes).as_str())
            && get(&m, "runId") == get(scope, "runId"),
        "REQUEST_RESULT",
    )?;
    let root: PathBuf = resolve(Path::new("/"), &runtime_dir.to_string_lossy());
    let actor_file: PathBuf = resolve(&root, sget(&m, "actorRelative").unwrap_or(""));
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
    let first_row = rows.first().cloned().unwrap_or(V::Null);
    let prior_seed = get(
        &get(&get(&first_row, "event"), "input"),
        "custodySeed",
    )
    .clone();
    // The new scope may widen, but prior inventory/history must be this exact
    // base.
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
    let timing_all = fs::read(&timing_file).map_err(|e| e.to_string())?;
    let prefix_len = js_int(get(&m, "timingPrefixBytes")).unwrap_or(-1);
    let timing: Vec<u8> = timing_all
        .iter()
        .take(prefix_len.max(0) as usize)
        .cloned()
        .collect();
    need(
        timing.len() as i64 == prefix_len
            && sha(&timing) == sget(&m, "timingPrefixHash").unwrap_or(""),
        "TIMING_PREFIX",
    )?;
    let events: Vec<V> = rows
        .iter()
        .map(|x| get(&get(x, "event"), "input").clone())
        .collect();
    // split('\n').map(JSON.parse): no empty-line filter — a blank element
    // fails JSON.parse in JS, so it must fail here too.
    let timings: Vec<V> = String::from_utf8_lossy(&timing)
        .trim_end()
        .split('\n')
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
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
    need(
        get(&get(&closure, "state"), "orders")
            .as_array()
            .map(|a| a.iter().all(|o| sget(o, "phase") == Some("abandoned")))
            .unwrap_or(false)
            && payload_hash(get(&get(&closure, "state"), "inventory"))?
                == payload_hash(get(&base, "inventory"))?,
        "NO_NEW_LOTS_OR_PENDING",
    )?;
    let closed: Vec<V> = get(&get(&closure, "state"), "orders")
        .as_array()
        .cloned()
        .unwrap_or_default();
    let base_body = base.as_object().cloned().unwrap_or_default();
    let recovery_sources: Vec<V> = [&marker_file, &result_file, &actor_file]
        .iter()
        .map(|file| {
            let bytes = fs::read(file).map_err(|e| e.to_string())?;
            let relative = file
                .strip_prefix(&root)
                .unwrap_or(file)
                .to_string_lossy()
                .to_string();
            Ok(json!({
                "relative": relative,
                "bytes": bytes.len() as i64,
                "sha256": sha(&bytes),
            }))
        })
        .collect::<Result<_, String>>()?;
    let mut inherited: Vec<V> = get(&base, "inheritedClosedOrders")
        .as_array()
        .cloned()
        .unwrap_or_default();
    let base_inherited_len = inherited.len();
    inherited.extend(closed.iter().cloned());
    let base_summary = get(&base, "summary").as_object().cloned().unwrap_or_default();
    let base_abandoned = js_int(get(&V::Object(base_summary.clone()), "abandonedOrders"))
        .unwrap_or(0);
    let mut summary = base_summary;
    summary.insert(
        "inheritedOrders".into(),
        json!((base_inherited_len + closed.len()) as i64),
    );
    summary.insert(
        "abandonedOrders".into(),
        json!((base_abandoned as usize + closed.len()) as i64),
    );
    let mut seed = Map::new();
    for (k, v) in base_body.iter() {
        if k == "seedHash" {
            continue;
        }
        let nv = match k.as_str() {
            "kind" => json!("baal_recovered_custody_seed_v2"),
            "inheritedClosedOrders" => V::Array(inherited.clone()),
            "summary" => V::Object(summary.clone()),
            _ => v.clone(),
        };
        seed.insert(k.clone(), nv);
    }
    // Spread semantics: keys missing from body append in literal order —
    // kind, inheritedClosedOrders, then the recovery keys, then summary.
    seed.entry("kind".to_string())
        .or_insert(json!("baal_recovered_custody_seed_v2"));
    seed.entry("inheritedClosedOrders".to_string())
        .or_insert(V::Array(inherited));
    seed.insert("recoverySources".into(), V::Array(recovery_sources));
    seed.insert(
        "recoveryProofHash".into(),
        json!(payload_hash(get(&closure, "proof"))?),
    );
    seed.insert(
        "recoveryActorAnchor".into(),
        get(&replay, "anchor").clone(),
    );
    let mut timing_prefix = Map::new();
    timing_prefix.insert("relative".into(), json!("read-timings.jsonl"));
    timing_prefix.insert("bytes".into(), json!(timing.len() as i64));
    timing_prefix.insert("sha256".into(), json!(sha(&timing)));
    seed.insert("recoveryTimingPrefix".into(), V::Object(timing_prefix));
    seed
        .entry("summary".to_string())
        .or_insert(V::Object(summary));
    let hash = payload_hash(&V::Object(seed.clone()))?;
    seed.insert("seedHash".into(), json!(hash));
    Ok(V::Object(seed))
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
