// execution/baal-current-custody-seed-v3.mjs — extends the recovered custody
// seed with the immediately-current actor: a sealed v5 journal replay plus
// the shared budget journal's last funding record. Produces ownedRuleHashes
// pinning the market-rule roots for every still-owned exchange.
//
// Port notes:
// - `path.join(runtimeDir,'baal-generic-live-runs',r.runId,r.epoch)` requires
//   string runId/epoch — non-strings throw a TypeError in JS; reproduced as a
//   plain error string.
// - `funding.actorProjection.record.reservationHash` is a direct property
//   chain (no ?.): an absent intermediate throws a TypeError in JS, which the
//   `prop` helper reproduces as an unprefixed error string.
// - `{...base}` key order: existing keys keep position; new keys
//   (ownedRuleHashes,currentSources,currentActorAnchor,previousActivation)
//   append in literal order; seedHash is appended last.
// - Object key lookup coerces keys to strings; exchangeIds are strings.
use crate::baal_generic_no_controller_v5::{
    baal_generic_no_controller_v5_identity, project_baal_generic_reservation_v5,
    replay_baal_generic_no_journal_v5,
};
use crate::baal_recovered_custody_seed_v2::build_baal_recovered_custody_seed_v2;
use crate::core::payload_hash;
use baal_service_runtime::fixed::{amount, money};
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::{js_int, js_stringify, js_truthy, sha256};
use serde_json::{json, Map, Value as V};
use std::fs;
use std::path::{Path, PathBuf};

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
fn hex64(v: &V) -> bool {
    v.as_str()
        .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
}
// JS `o.key` property access: throws a TypeError-style error when the receiver
// is null/undefined/non-object, exactly like the unprefixed JS throw.
fn prop<'a>(v: &'a V, k: &str) -> Result<&'a V, String> {
    match v {
        V::Object(m) => Ok(m.get(k).unwrap_or(&V::Null)),
        V::Null => Err(format!("Cannot read properties of null (reading '{k}')")),
        _ => Err(format!("Cannot read properties of undefined (reading '{k}')")),
    }
}
fn okey(v: &V) -> String {
    v.as_str().map(String::from).unwrap_or_else(|| js_stringify(v))
}
fn resolve(root: &Path, rel: &str) -> PathBuf {
    root.join(rel).components().collect()
}

/// buildBaalCurrentCustodySeedV3({frontier,scope,allocation,runtimeDir})
pub fn build_baal_current_custody_seed_v3(
    frontier: &V,
    scope: &V,
    allocation: &V,
    runtime_dir: &Path,
) -> Result<V, String> {
    let base = build_baal_recovered_custody_seed_v2(frontier, scope, allocation, runtime_dir)?;
    let root: PathBuf = runtime_dir.components().collect();
    let request_file = resolve(&root, "baal-generic-live-request-v3.json");
    let request_bytes = fs::read(&request_file).map_err(|e| e.to_string())?;
    let r: V = serde_json::from_slice(&request_bytes).map_err(|e| e.to_string())?;
    let run_id = sget(&r, "runId")
        .ok_or_else(|| "The \"path\" argument must be of type string".to_string())?;
    let epoch = sget(&r, "epoch")
        .ok_or_else(|| "The \"path\" argument must be of type string".to_string())?;
    // path.join normalizes . / .. segments lexically.
    let dir: PathBuf = root
        .join("baal-generic-live-runs")
        .join(run_id)
        .join(epoch)
        .components()
        .collect();
    let actor_file = dir.join("controller").join("baal-generic-no-v5-journal.jsonl");
    let budget_file = dir
        .join("budget")
        .join("baal-generic-shared-budget-v5.jsonl");
    let actor = fs::read(&actor_file).map_err(|e| e.to_string())?;
    let budget = fs::read(&budget_file).map_err(|e| e.to_string())?;
    let actor_text = String::from_utf8_lossy(&actor).into_owned();
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
    let s = get(&replay, "state").clone();
    need(
        get(&s, "orders").as_array().is_some_and(|orders| {
            orders.iter().all(|o| {
                (js_truthy(Some(get(o, "proof"))) && sget(o, "phase") == Some("reconciled"))
                    || sget(o, "phase") == Some("abandoned")
            })
        }),
        "NO_PENDING_OR_UNKNOWN",
    )?;
    let funding = Journal::open(&budget_file)
        .map_err(|e| e.to_string())?
        .last
        .map(|b| get(&b, "state").clone())
        .unwrap_or(V::Null);
    let record = project_baal_generic_reservation_v5(&s)?;
    need(
        {
            // funding?.controllerCodeIdentityHash ===
            //   r.activation.codeIdentityHash  (RHS still evaluated when
            //   funding is null, so prop() may throw first, as in JS)
            let lhs = if funding.is_null() {
                V::Null
            } else {
                get(&funding, "controllerCodeIdentityHash").clone()
            };
            lhs == *prop(prop(&r, "activation")?, "codeIdentityHash")?
        } && get(&funding, "allocationHash").as_str()
            == Some(payload_hash(allocation)?.as_str())
            && prop(&funding, "actorProjection")
                .and_then(|a| prop(a, "record"))
                .map(|rec| get(rec, "reservationHash"))
                .map_err(|e| e)?
                == get(&record, "reservationHash")
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
    let current_sources: Vec<V> = [&request_file, &actor_file, &budget_file]
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
    let inventory = get(&s, "inventory").clone();
    let mut realized: i128 = 0;
    let mut owned: i64 = 0;
    let mut principal: i128 = 0;
    for (_ex, p) in inventory.as_object().into_iter().flatten() {
        realized += amount(sget(get(p, "proof"), "cumulativeRealizedPnl").unwrap_or("\u{0}"))?;
        owned += js_int(get(get(p, "proof"), "remainingOwnedQuantity")).unwrap_or(0);
        principal += amount(sget(get(p, "proof"), "remainingOwnedCost").unwrap_or("\u{0}"))?;
    }
    let mut owned_rule_hashes = Map::new();
    let mut chain_orders = get(&V::Object(body.clone()), "inheritedClosedOrders")
        .as_array()
        .cloned()
        .unwrap_or_default();
    chain_orders.extend(get(&s, "orders").as_array().cloned().unwrap_or_default());
    for o in &chain_orders {
        if !(js_truthy(Some(get(o, "proof"))) && sget(get(o, "body"), "action") == Some("buy")) {
            continue;
        }
        let roots = prop(o, "proposal")
            .and_then(|p| prop(p, "sourceReceipts"))
            .map(|sr| get(sr, "roots").as_array().cloned().unwrap_or_default())
            .unwrap_or_default();
        for root in roots.iter() {
            let key = okey(get(root, "exchangeId"));
            let owned_here = inventory
                .get(&key)
                .and_then(|slot| js_int(get(get(slot, "proof"), "remainingOwnedQuantity")))
                .map(|q| q > 0)
                .unwrap_or(false);
            if owned_here {
                match owned_rule_hashes.get(&key) {
                    Some(prev) => need(prev == get(root, "rootHash"), "UNCHANGED_OWNED_RULES")?,
                    None => {
                        owned_rule_hashes.insert(key, get(root, "rootHash").clone());
                    }
                }
            }
        }
    }
    for (ex, slot) in inventory.as_object().into_iter().flatten() {
        if js_int(get(get(slot, "proof"), "remainingOwnedQuantity")).map(|q| q > 0).unwrap_or(false)
        {
            need(
                owned_rule_hashes.get(ex).is_some_and(|h| hex64(h)),
                "OWNED_RULE_SOURCE",
            )?;
        }
    }
    // seed = {...body,ownedRuleHashes,kind,inventory,inheritedClosedOrders,
    //   currentSources,currentActorAnchor,previousActivation,summary:{...}} —
    // existing keys keep position, new keys append in write order.
    body.insert("ownedRuleHashes".into(), V::Object(owned_rule_hashes));
    body.insert("kind".into(), json!("baal_current_custody_seed_v3"));
    body.insert("inventory".into(), inventory);
    let mut inherited = get(&V::Object(body.clone()), "inheritedClosedOrders")
        .as_array()
        .cloned()
        .unwrap_or_default();
    let s_orders = get(&s, "orders").as_array().cloned().unwrap_or_default();
    let base_inherited_len = inherited.len() as i64;
    inherited.extend(s_orders.iter().cloned());
    body.insert("inheritedClosedOrders".into(), V::Array(inherited));
    body.insert("currentSources".into(), V::Array(current_sources));
    body.insert("currentActorAnchor".into(), get(&replay, "anchor").clone());
    body.insert(
        "previousActivation".into(),
        get(&s, "activation").clone(),
    );
    let mut summary = get(&V::Object(body.clone()), "summary")
        .as_object()
        .cloned()
        .unwrap_or_default();
    let abandoned_in_s = s_orders
        .iter()
        .filter(|o| sget(o, "phase") == Some("abandoned"))
        .count() as i64;
    let base_abandoned = get(&get(&V::Object(body.clone()), "summary"), "abandonedOrders").clone();
    let abandoned_total = match base_abandoned.as_i64() {
        Some(x) => json!(x + abandoned_in_s),
        None => match base_abandoned.as_f64() {
            Some(x) => json!(x + abandoned_in_s as f64),
            None => V::Null, // undefined + n -> NaN -> null on stringify
        },
    };
    summary.insert("ownedQuantity".into(), json!(owned));
    summary.insert("ownedPrincipal".into(), json!(money(principal)));
    summary.insert("realizedPnl".into(), json!(money(realized)));
    summary.insert(
        "inheritedOrders".into(),
        json!(base_inherited_len + s_orders.len() as i64),
    );
    summary.insert("abandonedOrders".into(), abandoned_total);
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
