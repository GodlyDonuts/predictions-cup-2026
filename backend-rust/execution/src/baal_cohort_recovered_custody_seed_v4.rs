// execution/baal-cohort-recovered-custody-seed-v4.mjs — pure private-state
// conversion: replay the sealed v6 controller journal through the
// blocked-admission reconciliation closure and pin the marker/result/
// request/actor/budget files as cohort recovery sources. Preserves all
// attributable lots and closed history; never grants write authority.
//
// Port notes (same conventions as the sibling seeds):
// - `need` throws Error('COHORT_RECOVERED_SEED_<code>') — bare code strings.
// - `path.resolve(dir,m.actorRelative)` -> resolve(); `path.dirname` ->
//   parent(); `path.relative` -> rel() over components.
// - Buffer.subarray(0,n) -> bytes[..min(n,len)]; the length check after it
//   rejects short prefixes exactly like `timing.length===m.timingPrefixBytes`.
// - m.actorRelative/timingPrefixBytes/etc non-JSON-typed values reach the
//   same need() failures as the JS paths do.
// - {...body,k:v} key order: existing keys keep position, the three
//   cohortRecovery* keys append (previousActivation already exists in the
//   v3 base, so it is replaced in place).
use crate::baal_current_custody_seed_v3::build_baal_current_custody_seed_v3;
use crate::baal_generic_no_controller_v6::{
    baal_generic_no_controller_v6_identity, replay_baal_generic_no_journal_v6,
};
use crate::blocked_admission_reconciliation_v2::reconcile_blocked_admission_v2;
use crate::core::payload_hash;
use baal_service_runtime::fixed::{amount, money};
use baal_service_runtime::json_text::{js_int, js_truthy, sha256};
use serde_json::{json, Map, Value as V};
use std::fs;
use std::path::{Component, Path, PathBuf};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("COHORT_RECOVERED_SEED_{code}"))
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
fn str_amount(v: &V) -> Result<i128, String> {
    amount(v.as_str().ok_or_else(|| "INVALID".to_string())?)
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

/// buildBaalCohortRecoveredCustodySeedV4({frontier,scope,allocation,runtimeDir})
pub fn build_baal_cohort_recovered_custody_seed_v4(
    frontier: &V,
    scope: &V,
    allocation: &V,
    runtime_dir: &Path,
) -> Result<V, String> {
    let base = build_baal_current_custody_seed_v3(frontier, scope, allocation, runtime_dir)?;
    let marker_file = runtime_dir.join("blocked-admission-recovery-v2-request.json");
    let result_file = runtime_dir.join("blocked-admission-recovery-v2-result.json");
    let request_file = runtime_dir.join("cohort-profit-lock-request-v1.json");
    let mb = fs::read(&marker_file).map_err(|e| e.to_string())?;
    let rb = fs::read(&result_file).map_err(|e| e.to_string())?;
    let m: V = serde_json::from_slice(&mb).map_err(|e| e.to_string())?;
    let r: V = serde_json::from_slice(&rb).map_err(|e| e.to_string())?;
    let request: V = serde_json::from_str(&String::from_utf8_lossy(
        &fs::read(&request_file).map_err(|e| e.to_string())?,
    ))
    .map_err(|e| e.to_string())?;
    need(
        get(&r, "markerHash").as_str() == Some(sha(&mb).as_str())
            && get(&request, "inheritanceHash") == get(&base, "seedHash")
            && payload_hash(get(&request, "scope"))? == payload_hash(scope)?,
        "EXACT_PARENT",
    )?;
    let root: PathBuf = runtime_dir.components().collect();
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
    let replay = replay_baal_generic_no_journal_v6(
        &actor_text,
        &baal_generic_no_controller_v6_identity()?,
        &base,
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
    let closure = reconcile_blocked_admission_v2(&json!({
        "state": get(&replay, "state"),
        "events": events,
        "timings": timings,
        "after": get(&r, "after"),
        "at": get(&get(&r, "result"), "proof").get("at").cloned().unwrap_or(V::Null),
    }))?;
    let closure_orders = get(&get(&closure, "state"), "orders")
        .as_array()
        .cloned()
        .unwrap_or_default();
    need(
        payload_hash(&closure)? == payload_hash(get(&r, "result"))?
            && closure_orders
                .iter()
                .all(|o| js_truthy(Some(get(o, "proof"))) || sget(o, "phase") == Some("abandoned")),
        "EXACT_CLOSED_RECOVERY",
    )?;
    let mut body: Map<String, V> = base.as_object().cloned().unwrap_or_default();
    body.shift_remove("seedHash");
    let inventory = get(&get(&closure, "state"), "inventory").clone();
    let actor_dir = actor_file.parent().and_then(|p| p.parent());
    let budget_file = actor_dir
        .map(|d| d.join("budget").join("baal-generic-shared-budget-v6.jsonl"))
        .unwrap_or_else(|| PathBuf::from("budget").join("baal-generic-shared-budget-v6.jsonl"));
    let extra_sources: Vec<V> = [
        marker_file.clone(),
        result_file.clone(),
        request_file.clone(),
        actor_file.clone(),
        budget_file,
    ]
    .iter()
    .map(|file| {
        let resolved: PathBuf = file.components().collect();
        let bytes = fs::read(file).map_err(|e| e.to_string())?;
        Ok(json!({
            "relative": rel(&root, &resolved),
            "bytes": bytes.len(),
            "sha256": sha(&bytes),
        }))
    })
    .collect::<Result<_, String>>()?;
    let mut orders: Vec<V> = get(&V::Object(body.clone()), "inheritedClosedOrders")
        .as_array()
        .cloned()
        .unwrap_or_default();
    orders.extend(closure_orders.iter().cloned());
    let slots: Vec<V> = inventory
        .as_object()
        .map(|m| m.values().cloned().collect())
        .unwrap_or_default();
    let mut owned: i64 = 0;
    let mut principal: i128 = 0;
    let mut realized: i128 = 0;
    for p in &slots {
        owned += js_int(get(get(p, "proof"), "remainingOwnedQuantity")).unwrap_or(0);
        principal += str_amount(get(get(p, "proof"), "remainingOwnedCost"))?;
        realized += str_amount(get(get(p, "proof"), "cumulativeRealizedPnl"))?;
    }
    let mut summary = get(&V::Object(body.clone()), "summary")
        .as_object()
        .cloned()
        .unwrap_or_default();
    summary.insert("ownedQuantity".into(), json!(owned));
    summary.insert("ownedPrincipal".into(), json!(money(principal)));
    summary.insert("realizedPnl".into(), json!(money(realized)));
    summary.insert("inheritedOrders".into(), json!(orders.len() as i64));
    summary.insert(
        "abandonedOrders".into(),
        json!(orders
            .iter()
            .filter(|o| sget(o, "phase") == Some("abandoned"))
            .count() as i64),
    );
    // Key order mirrors the JS literal: kind/inventory/inheritedClosedOrders
    // replace in place; the three cohortRecovery* keys append;
    // previousActivation already exists in the v3 base so it stays in place.
    body.insert("kind".into(), json!("baal_cohort_recovered_custody_seed_v4"));
    body.insert("inventory".into(), inventory);
    body.insert("inheritedClosedOrders".into(), V::Array(orders));
    body.insert("cohortRecoverySources".into(), json!(extra_sources));
    body.insert(
        "cohortRecoveryProofHash".into(),
        json!(payload_hash(get(&closure, "proof"))?),
    );
    body.insert(
        "cohortRecoveryActorAnchor".into(),
        get(&replay, "anchor").clone(),
    );
    body.insert(
        "previousActivation".into(),
        get(&get(&closure, "state"), "activation").clone(),
    );
    body.insert("summary".into(), V::Object(summary));
    let seed_hash = payload_hash(&V::Object(body.clone()))?;
    body.insert("seedHash".into(), json!(seed_hash));
    Ok(V::Object(body))
}

/// validateBaalCohortRecoveredCustodySeedV4(seed,scope,allocation)
pub fn validate_baal_cohort_recovered_custody_seed_v4(
    seed: &V,
    scope: &V,
    allocation: &V,
) -> Result<V, String> {
    let mut body = seed.as_object().cloned().unwrap_or_default();
    let seed_hash = body.shift_remove("seedHash").unwrap_or(V::Null);
    need(
        sget(seed, "kind") == Some("baal_cohort_recovered_custody_seed_v4")
            && get(seed, "executionAuthority") == &V::Bool(false)
            && payload_hash(&V::Object(body))? == seed_hash.as_str().unwrap_or("")
            && get(seed, "scopeHash").as_str() == Some(payload_hash(scope)?.as_str())
            && get(seed, "allocationHash").as_str()
                == Some(payload_hash(allocation)?.as_str()),
        "HASH_SCOPE",
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
