// execution/physical-guard-reconciliation-v1.mjs — reconcile one expired,
// affirmatively-blocked intent to abandoned, bound by an unchanged scoped
// account. Never clears the financial stop.
//
// Port notes:
// - structuredClone -> .clone(); mutation of the cloned order mirrors the JS.
// - `...after.openOrdersPages` spreads: missing/non-array spreads are treated
//   as empty (the JS would TypeError on non-iterables; real inputs are arrays).
// - ordered() sorts by numeric `.id` like `a.id - b.id`.
use baal_execution::core::payload_hash;
use baal_execution::no_rest_sell_reconciliation_v2::inspect_scoped_rest_no_account;
use baal_service_runtime::contract::parse_iso_ms;
use baal_service_runtime::json_text::js_truthy;
use serde_json::{json, Map, Value as V};

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

fn ordered(rows: &[V]) -> Vec<V> {
    let mut v = rows.to_vec();
    v.sort_by(|a, b| {
        let x = get(a, "id").as_f64().unwrap_or(f64::NAN);
        let y = get(b, "id").as_f64().unwrap_or(f64::NAN);
        x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal)
    });
    v
}

/// reconcilePhysicalGuardAdmissionV1({state,events,timings,diagnostic,guardFailure,stop,after,at})
pub fn reconcile_physical_guard_admission_v1(input: &V) -> Result<V, String> {
    let state = get(input, "state");
    let after = get(input, "after");
    let at = get(input, "at");
    let audit = audit_physical_guard_admission_v1(&json!({
        "state": state,
        "events": get(input, "events"),
        "timings": get(input, "timings"),
        "diagnostic": get(input, "diagnostic"),
        "guardFailure": get(input, "guardFailure"),
        "stop": get(input, "stop"),
        "now": dparse(at),
    }))?;
    let orders = get(state, "orders").as_array().cloned().unwrap_or_default();
    let o = orders
        .iter()
        .find(|x| !js_truthy(x.get("proof")) && sget(x, "phase") != Some("abandoned"))
        .cloned()
        .unwrap_or(V::Null);
    let scope_exchanges = get(get(state, "scope"), "exchanges")
        .as_array()
        .cloned()
        .unwrap_or_default();
    let member = scope_exchanges
        .iter()
        .find(|x| x.get("exchangeId") == Some(&get(get(&o, "body"), "exchangeId").clone()))
        .cloned()
        .unwrap_or(V::Null);
    let mut binding: Map<String, V> = get(state, "scope").as_object().cloned().unwrap_or_default();
    if let V::Object(m) = &member {
        for (k, v) in m {
            binding.insert(k.clone(), v.clone());
        }
    }
    let binding = V::Object(binding);
    let before = inspect_scoped_rest_no_account(&json!({
        "account": get(&o, "before"),
        "binding": binding,
        "readAt": get(&o, "createdAt"),
    }))?;
    let current = inspect_scoped_rest_no_account(&json!({
        "account": after,
        "binding": binding,
        "readAt": at,
    }))?;
    let mut receipt_rows: Vec<V> = vec![get(after, "cash").clone(), get(after, "positions").clone()];
    receipt_rows.extend(
        get(after, "openOrdersPages")
            .as_array()
            .cloned()
            .unwrap_or_default(),
    );
    receipt_rows.extend(
        get(after, "scopedOrdersPages")
            .as_array()
            .cloned()
            .unwrap_or_default(),
    );
    for r in &receipt_rows {
        if dparse(get(r, "requestStartedAt")) <= dparse(get(get(&o, "body"), "expirationDate"))
            || dparse(at) - dparse(get(r, "requestStartedAt")) > 20000.0
        {
            return Err("PHYSICAL_BLOCK_AFTER_EXPIRY".into());
        }
    }
    let member_exchange = member.get("exchangeId").cloned().unwrap_or(V::Null);
    let pos_before: Vec<V> = get(&before, "positions")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p.get("exchangeId") == Some(&member_exchange))
        .collect();
    let pos_current: Vec<V> = get(&current, "positions")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p.get("exchangeId") == Some(&member_exchange))
        .collect();
    let scoped_before = ordered(
        &get(&before, "scopedOrders")
            .as_array()
            .cloned()
            .unwrap_or_default(),
    );
    let scoped_current = ordered(
        &get(&current, "scopedOrders")
            .as_array()
            .cloned()
            .unwrap_or_default(),
    );
    if payload_hash(&V::Array(pos_before))? != payload_hash(&V::Array(pos_current))?
        || payload_hash(&V::Array(scoped_before))? != payload_hash(&V::Array(scoped_current))?
    {
        return Err("PHYSICAL_BLOCK_ACCOUNT_CHANGED".into());
    }
    let mut next = state.clone();
    let target_intent = o.get("intentId").cloned().unwrap_or(V::Null);
    if let V::Object(m) = &mut next {
        if let Some(V::Array(list)) = m.get_mut("orders") {
            if let Some(closed) = list
                .iter_mut()
                .find(|x| x.get("intentId") == Some(&target_intent))
            {
                if let V::Object(cm) = closed {
                    cm.insert("phase".into(), json!("abandoned"));
                    cm.insert("abandonedAt".into(), at.clone());
                    cm.insert(
                        "abandonedReason".into(),
                        json!("expired_affirmatively_blocked_before_fetch_reconciled"),
                    );
                    cm.insert("blockedAdmissionAudit".into(), audit.clone());
                }
            }
        }
    }
    Ok(json!({
        "state": next,
        "proof": {
            "schemaVersion": 1,
            "kind": "physical_guard_scoped_reconciliation_v1",
            "originalEconomicStateHash": payload_hash(state)?,
            "closedEconomicStateHash": payload_hash(&next)?,
            "auditHash": payload_hash(&audit)?,
            "afterHash": payload_hash(after)?,
            "at": at,
            "executionAuthority": false,
            "clearsStop": false,
        },
    }))
}
