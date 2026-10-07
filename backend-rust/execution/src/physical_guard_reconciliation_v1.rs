// execution/physical-guard-reconciliation-v1.mjs — closes the affirmatively
// blocked intent after scoped account equality is re-proven. No API, writes,
// lock changes or STOP removal. This file throws bare PHYSICAL_BLOCK_* errors
// (the JS has no need() wrapper here).
use baal_service_runtime::contract::parse_iso_ms;
use baal_service_runtime::json_text::js_truthy;
use serde_json::{json, Map, Value as V};

use crate::core::payload_hash;
use crate::no_rest_sell_reconciliation_v2::inspect_scoped_rest_no_account;
use crate::physical_guard_admission_audit_v1::audit_physical_guard_admission_v1;

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}

fn sget<'a>(v: &'a V, k: &str) -> Option<&'a str> {
    v.get(k).and_then(V::as_str)
}

fn dparse(v: &V) -> f64 {
    v.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN)
}

/// ordered(rows) — [...rows].sort((a,b)=>a.id-b.id), numeric subtraction.
fn ordered(rows: &[V]) -> Vec<V> {
    let mut out = rows.to_vec();
    out.sort_by(|a, b| {
        let d = get(a, "id").as_f64().unwrap_or(f64::NAN)
            - get(b, "id").as_f64().unwrap_or(f64::NAN);
        if d.is_nan() {
            std::cmp::Ordering::Equal
        } else if d < 0.0 {
            std::cmp::Ordering::Less
        } else if d > 0.0 {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });
    out
}

/// reconcilePhysicalGuardAdmissionV1({state,events,timings,diagnostic,
///   guardFailure,stop,after,at})
pub fn reconcile_physical_guard_admission_v1(input: &V) -> Result<V, String> {
    let state = get(input, "state");
    let events = get(input, "events");
    let timings = get(input, "timings");
    let diagnostic = get(input, "diagnostic");
    let guard_failure = get(input, "guardFailure");
    let stop = get(input, "stop");
    let after = get(input, "after");
    let at = get(input, "at");
    let now = dparse(at);
    let audit = audit_physical_guard_admission_v1(&json!({
        "state": state,
        "events": events,
        "timings": timings,
        "diagnostic": diagnostic,
        "guardFailure": guard_failure,
        "stop": stop,
        "now": now,
    }))?;
    let orders = get(state, "orders").as_array().cloned().unwrap_or_default();
    let o = orders
        .iter()
        .find(|x| !js_truthy(Some(get(x, "proof"))) && sget(x, "phase") != Some("abandoned"))
        .cloned()
        .unwrap_or(V::Null);
    let member = get(get(state, "scope"), "exchanges")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .find(|x| get(x, "exchangeId") == get(get(&o, "body"), "exchangeId"))
        .unwrap_or(V::Null);
    let mut binding = get(state, "scope").as_object().cloned().unwrap_or_default();
    if let Some(m) = member.as_object() {
        for (k, v) in m {
            binding.insert(k.clone(), v.clone());
        }
    }
    let before = inspect_scoped_rest_no_account(&json!({
        "account": get(&o, "before"),
        "binding": V::Object(binding.clone()),
        "readAt": get(&o, "createdAt"),
    }))?;
    let current = inspect_scoped_rest_no_account(&json!({
        "account": after,
        "binding": V::Object(binding),
        "readAt": at,
    }))?;
    let mut receipts = vec![get(after, "cash").clone(), get(after, "positions").clone()];
    receipts.extend(get(after, "openOrdersPages").as_array().cloned().unwrap_or_default());
    receipts.extend(get(after, "scopedOrdersPages").as_array().cloned().unwrap_or_default());
    let expiry = dparse(get(get(&o, "body"), "expirationDate"));
    for r in &receipts {
        let started = dparse(get(r, "requestStartedAt"));
        if started <= expiry || now - started > 20000.0 {
            return Err("PHYSICAL_BLOCK_AFTER_EXPIRY".into());
        }
    }
    let target = |a: &V| -> Vec<V> {
        get(a, "positions")
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|p| get(p, "exchangeId") == get(&member, "exchangeId"))
            .collect()
    };
    if payload_hash(&V::Array(target(&before)))? != payload_hash(&V::Array(target(&current)))?
        || payload_hash(&V::Array(ordered(
            &get(&before, "scopedOrders").as_array().cloned().unwrap_or_default(),
        )))? != payload_hash(&V::Array(ordered(
            &get(&current, "scopedOrders").as_array().cloned().unwrap_or_default(),
        )))?
    {
        return Err("PHYSICAL_BLOCK_ACCOUNT_CHANGED".into());
    }
    let mut next = state.clone();
    if let Some(arr) = next.get_mut("orders").and_then(V::as_array_mut) {
        if let Some(closed) = arr
            .iter_mut()
            .find(|x| get(x, "intentId") == get(&o, "intentId"))
        {
            let m = closed.as_object_mut();
            if let Some(m) = m {
                m.insert("phase".into(), json!("abandoned"));
                m.insert("abandonedAt".into(), at.clone());
                m.insert(
                    "abandonedReason".into(),
                    json!("expired_affirmatively_blocked_before_fetch_reconciled"),
                );
                m.insert("blockedAdmissionAudit".into(), audit.clone());
            }
        }
    }
    let proof = json!({
        "schemaVersion": 1,
        "kind": "physical_guard_scoped_reconciliation_v1",
        "originalEconomicStateHash": payload_hash(state)?,
        "closedEconomicStateHash": payload_hash(&next)?,
        "auditHash": payload_hash(&audit)?,
        "afterHash": payload_hash(after)?,
        "at": at,
        "executionAuthority": false,
        "clearsStop": false,
    });
    let mut out = Map::new();
    out.insert("state".into(), next);
    out.insert("proof".into(), proof);
    Ok(V::Object(out))
}
