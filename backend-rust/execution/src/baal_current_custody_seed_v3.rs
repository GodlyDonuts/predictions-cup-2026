// execution/baal-current-custody-seed-v3.mjs — pure private-state conversion:
// replay the sealed v5 controller journal to its terminal state, pin the
// current request/actor/budget files as sources, and carry the owned-rule
// root hashes forward. Preserves all attributable lots and closed history;
// never grants write authority.
//
// Port notes:
// - `need` throws Error('CURRENT_CUSTODY_<code>') — the bare code string.
// - JS `&&`/`?.` short-circuit -> Rust && evaluation order preserved.
// - `path.join`/`path.resolve` -> lexical join; `path.relative` -> rel().
// - fs.readFileSync errors -> io error strings; JSON.parse -> serde errors.
// - `new Journal(file).last?.state` -> Journal::open + last body.state.
// - {...body,k:v} semantics: existing keys keep position, new keys append.
// - `o.proposal?.sourceReceipts?.roots ?? []` -> missing/null roots -> [];
//   the inserted ownedRuleHashes map keeps first-set key order.
use crate::baal_generic_no_controller_v5::{
    baal_generic_no_controller_v5_identity, project_baal_generic_reservation_v5,
    replay_baal_generic_no_journal_v5,
};
use crate::baal_recovered_custody_seed_v2::build_baal_recovered_custody_seed_v2;
use crate::core::payload_hash;
use baal_service_runtime::fixed::{amount, money};
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::{js_int, js_truthy, sha256};
use serde_json::{json, Map, Value as V};
use std::fs;
use std::path::{Component, Path, PathBuf};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("CURRENT_CUSTODY_{code}"))
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
// path.join: lexical join (no normalization like the sibling seeds).
fn join(root: &Path, parts: &[&str]) -> PathBuf {
    let mut p = root.to_path_buf();
    for x in parts {
        p.push(x);
    }
    p
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
fn hex(v: &V) -> bool {
    v.as_str()
        .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
}
// inventory[ex]?.proof.remainingOwnedQuantity — the ?. guard yields Null for
// a missing slot; a present slot without a proof key throws in JS (TypeError)
// so the ported path errors too.
fn slot_remaining(inventory: &V, ex: &str) -> Result<Option<i64>, String> {
    match inventory.get(ex) {
        None => Ok(None),
        Some(slot) if slot.is_null() => Ok(None),
        Some(slot) => {
            let proof = slot
                .get("proof")
                .filter(|p| !p.is_null())
                .ok_or_else(|| "CURRENT_CUSTODY_TYPE_ERROR".to_string())?;
            Ok(js_int(proof.get("remainingOwnedQuantity").unwrap_or(&V::Null)))
        }
    }
}

/// buildBaalCurrentCustodySeedV3({frontier,scope,allocation,runtimeDir})
pub fn build_baal_current_custody_seed_v3(
    frontier: &V,
    scope: &V,
    allocation: &V,
    runtime_dir: &Path,
) -> Result<V, String> {
    let base = build_baal_recovered_custody_seed_v2(frontier, scope, allocation, runtime_dir)?;
    let request_file = join(runtime_dir, &["baal-generic-live-request-v3.json"]);
    let request_bytes = fs::read(&request_file).map_err(|e| e.to_string())?;
    let r: V = serde_json::from_slice(&request_bytes).map_err(|e| e.to_string())?;
    let dir = join(
        runtime_dir,
        &[
            "baal-generic-live-runs",
            sget(&r, "runId").unwrap_or(""),
            sget(&r, "epoch").unwrap_or(""),
        ],
    );
    let actor_file = join(&dir, &["controller", "baal-generic-no-v5-journal.jsonl"]);
    let budget_file = join(&dir, &["budget", "baal-generic-shared-budget-v5.jsonl"]);
    let actor = fs::read(&actor_file).map_err(|e| e.to_string())?;
    let budget = fs::read(&budget_file).map_err(|e| e.to_string())?;
    let actor_text = String::from_utf8_lossy(&actor).to_string();
    // rows is computed then unused in the JS; the parse itself must still run.
    let _rows: Vec<V> = actor_text
        .trim_end()
        .split('\n')
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    need(
        payload_hash(get(&r, "scope"))? == payload_hash(scope)?
            && payload_hash(get(&r, "allocation"))? == payload_hash(allocation)?
            && get(&r, "inheritanceHash") == get(&base, "seedHash"),
        "PRIOR_EXACT_CONFIGURATION",
    )?;
    let replay = replay_baal_generic_no_journal_v5(
        &actor_text,
        &baal_generic_no_controller_v5_identity()?,
        &base,
    )?;
    let s = get(&replay, "state");
    need(
        get(s, "orders")
            .as_array()
            .map(|a| {
                a.iter().all(|o| {
                    (js_truthy(Some(get(o, "proof"))) && sget(o, "phase") == Some("reconciled"))
                        || sget(o, "phase") == Some("abandoned")
                })
            })
            .unwrap_or(false),
        "NO_PENDING_OR_UNKNOWN",
    )?;
    let funding = Journal::open(&budget_file)
        .map_err(|e| e.to_string())?
        .last
        .and_then(|b| b.get("state").cloned())
        .unwrap_or(V::Null);
    let record = project_baal_generic_reservation_v5(s)?;
    need(
        get(&funding, "controllerCodeIdentityHash")
            == get(&get(&r, "activation"), "codeIdentityHash")
            && get(&funding, "allocationHash").as_str()
                == Some(payload_hash(allocation)?.as_str())
            && get(&get(&funding, "actorProjection"), "record")
                .get("reservationHash")
                == record.get("reservationHash")
            && get(&funding, "unknownAcceptance") == &V::Bool(false),
        "EXACT_CURRENT_RESERVATION",
    )?;
    need(
        fs::read(&actor_file).map_err(|e| e.to_string())? == actor
            && fs::read(&budget_file).map_err(|e| e.to_string())? == budget
            && fs::read(&request_file).map_err(|e| e.to_string())? == request_bytes,
        "STABLE_SNAPSHOT",
    )?;
    let mut body: Map<String, V> = base.as_object().cloned().unwrap_or_default();
    body.shift_remove("seedHash");
    let root: PathBuf = runtime_dir.components().collect();
    let current_sources: Vec<V> = [&request_file, &actor_file, &budget_file]
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
    let inventory = get(s, "inventory").clone();
    let slots: Vec<V> = inventory
        .as_object()
        .map(|m| m.values().cloned().collect())
        .unwrap_or_default();
    let mut realized: i128 = 0;
    let mut owned: i64 = 0;
    let mut principal: i128 = 0;
    for p in &slots {
        realized += str_amount(get(get(p, "proof"), "cumulativeRealizedPnl"))?;
        owned += js_int(get(get(p, "proof"), "remainingOwnedQuantity")).unwrap_or(0);
        principal += str_amount(get(get(p, "proof"), "remainingOwnedCost"))?;
    }
    // ownedRuleHashes: first-seen exchangeId order, as a JS object literal.
    let mut owned_rule_hashes = Map::new();
    let mut orders_in: Vec<V> = get(&V::Object(body.clone()), "inheritedClosedOrders")
        .as_array()
        .cloned()
        .unwrap_or_default();
    orders_in.extend(get(s, "orders").as_array().cloned().unwrap_or_default());
    for o in &orders_in {
        if !(js_truthy(Some(get(o, "proof")))
            && sget(get(o, "body"), "action") == Some("buy"))
        {
            continue;
        }
        let roots = get(get(get(o, "proposal"), "sourceReceipts"), "roots");
        let roots: Vec<V> = roots.as_array().cloned().unwrap_or_default();
        for root in &roots {
            let ex = sget(root, "exchangeId").unwrap_or("").to_string();
            if slot_remaining(&inventory, &ex)?.unwrap_or(0) > 0 {
                let rh = get(root, "rootHash");
                if let Some(cur) = owned_rule_hashes.get(&ex) {
                    need(!js_truthy(Some(cur)) || cur == rh, "UNCHANGED_OWNED_RULES")?;
                }
                owned_rule_hashes.insert(ex, rh.clone());
            }
        }
    }
    for (ex, slot) in inventory
        .as_object()
        .map(|m| m.iter().collect::<Vec<_>>())
        .unwrap_or_default()
    {
        let proof = slot
            .get("proof")
            .filter(|p| !p.is_null())
            .ok_or_else(|| "CURRENT_CUSTODY_TYPE_ERROR".to_string())?;
        if js_int(proof.get("remainingOwnedQuantity").unwrap_or(&V::Null)).unwrap_or(0) > 0 {
            need(
                hex(owned_rule_hashes.get(ex.as_str()).unwrap_or(&V::Null)),
                "OWNED_RULE_SOURCE",
            )?;
        }
    }
    let inherited: Vec<V> = orders_in;
    let abandoned_in_orders = get(s, "orders")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter(|o| sget(o, "phase") == Some("abandoned"))
        .count() as i64;
    let base_inherited_len = get(&V::Object(body.clone()), "inheritedClosedOrders")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .len() as i64;
    let s_orders_len = get(s, "orders").as_array().cloned().unwrap_or_default().len() as i64;
    let base_abandoned = get(&get(&V::Object(body.clone()), "summary"), "abandonedOrders")
        .as_i64()
        .unwrap_or(0);
    let mut summary = get(&V::Object(body.clone()), "summary")
        .as_object()
        .cloned()
        .unwrap_or_default();
    summary.insert("ownedQuantity".into(), json!(owned));
    summary.insert("ownedPrincipal".into(), json!(money(principal)));
    summary.insert("realizedPnl".into(), json!(money(realized)));
    summary.insert(
        "inheritedOrders".into(),
        json!(base_inherited_len + s_orders_len),
    );
    summary.insert(
        "abandonedOrders".into(),
        json!(base_abandoned + abandoned_in_orders),
    );
    // Key order mirrors the JS literal: ownedRuleHashes is the first new key
    // appended after the base fields; existing keys stay in place.
    body.insert(
        "ownedRuleHashes".into(),
        V::Object(owned_rule_hashes),
    );
    body.insert("kind".into(), json!("baal_current_custody_seed_v3"));
    body.insert("inventory".into(), inventory);
    body.insert("inheritedClosedOrders".into(), V::Array(inherited));
    body.insert("currentSources".into(), json!(current_sources));
    body.insert("currentActorAnchor".into(), get(&replay, "anchor").clone());
    body.insert("previousActivation".into(), get(s, "activation").clone());
    body.insert("summary".into(), V::Object(summary));
    let seed_hash = payload_hash(&V::Object(body.clone()))?;
    body.insert("seedHash".into(), json!(seed_hash));
    Ok(V::Object(body))
}

/// validateBaalCurrentCustodySeedV3(seed,scope,allocation)
pub fn validate_baal_current_custody_seed_v3(
    seed: &V,
    scope: &V,
    allocation: &V,
) -> Result<V, String> {
    let mut body = seed.as_object().cloned().unwrap_or_default();
    let seed_hash = body.shift_remove("seedHash").unwrap_or(V::Null);
    need(
        sget(seed, "kind") == Some("baal_current_custody_seed_v3")
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
