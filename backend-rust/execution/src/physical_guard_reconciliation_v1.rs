// execution/physical-guard-reconciliation-v1.mjs — close out a single expired
// place intent that is affirmatively proven never to have reached the network:
// the audit, a same-account scoped reconciliation, and an exact recovery hash.
//
// Port notes:
// - Bare `throw Error(code)` sites keep their literal codes; `.find/.filter/
//   spread` on a non-array is a JS TypeError-equivalent -> "TYPE_ERROR".
// - `x.phase==='abandoned'`/`x.exchangeId===y` are strict string comparisons;
//   `x.intentId===o.intentId` is full strict equality (absent === absent).
// - Date.parse of a non-string yields NaN; NaN comparisons are false.
// - Key order: `closed` keeps `phase`'s original slot; abandonedAt /
//   abandonedReason / blockedAdmissionAudit append in assignment order.
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
fn type_err() -> String {
    "TYPE_ERROR".to_string()
}
fn dparse(v: &V) -> f64 {
    v.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN)
}
// ordered(rows): ascending numeric sort by `.id`; NaN keys hold position.
fn ordered(rows: &[V]) -> Vec<V> {
    let mut x = rows.to_vec();
    x.sort_by(|a, b| {
        let d = get(a, "id").as_f64().unwrap_or(f64::NAN)
            - get(b, "id").as_f64().unwrap_or(f64::NAN);
        d.partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal)
    });
    x
}

/// reconcilePhysicalGuardAdmissionV1({state,events,timings,diagnostic,
///   guardFailure,stop,after,at})
pub fn reconcile_physical_guard_admission_v1(input: &V) -> Result<V, String> {
    let state = get(input, "state");
    let after = get(input, "after");
    let at = get(input, "at");
    let at_ms = dparse(at);
    let audit = audit_physical_guard_admission_v1(&json!({
        "state": state,
        "events": get(input, "events"),
        "timings": get(input, "timings"),
        "diagnostic": get(input, "diagnostic"),
        "guardFailure": get(input, "guardFailure"),
        "stop": get(input, "stop"),
        "now": V::from(at_ms),
    }))?;
    let orders = state
        .get("orders")
        .and_then(V::as_array)
        .ok_or_else(type_err)?;
    let o = orders
        .iter()
        .find(|x| !js_truthy(x.get("proof")) && sget(x, "phase") != Some("abandoned"))
        .cloned()
        .unwrap_or(V::Null);
    if o.is_null() {
        return Err(type_err());
    }
    let member = get(get(state, "scope"), "exchanges")
        .as_array()
        .ok_or_else(type_err)?
        .iter()
        .find(|x| get(x, "exchangeId") == get(get(&o, "body"), "exchangeId"))
        .cloned()
        .unwrap_or(V::Null);
    // binding = {...state.scope, ...member}: member keys override scope's.
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
    let exp_ms = dparse(get(get(&o, "body"), "expirationDate"));
    let mut receipts: Vec<V> = vec![get(after, "cash").clone(), get(after, "positions").clone()];
    receipts.extend(
        get(after, "openOrdersPages")
            .as_array()
            .ok_or_else(type_err)?
            .iter()
            .cloned(),
    );
    receipts.extend(
        get(after, "scopedOrdersPages")
            .as_array()
            .ok_or_else(type_err)?
            .iter()
            .cloned(),
    );
    for r in &receipts {
        let req_ms = dparse(get(r, "requestStartedAt"));
        if req_ms <= exp_ms || at_ms - req_ms > 20000.0 {
            return Err("PHYSICAL_BLOCK_AFTER_EXPIRY".to_string());
        }
    }
    let member_ex = sget(&member, "exchangeId").ok_or_else(type_err)?;
    let before_positions = get(&before, "positions")
        .as_array()
        .ok_or_else(type_err)?
        .iter()
        .filter(|p| sget(p, "exchangeId") == Some(member_ex))
        .cloned()
        .collect::<Vec<V>>();
    let current_positions = get(&current, "positions")
        .as_array()
        .ok_or_else(type_err)?
        .iter()
        .filter(|p| sget(p, "exchangeId") == Some(member_ex))
        .cloned()
        .collect::<Vec<V>>();
    let before_orders = ordered(
        get(&before, "scopedOrders")
            .as_array()
            .ok_or_else(type_err)?,
    );
    let current_orders = ordered(
        get(&current, "scopedOrders")
            .as_array()
            .ok_or_else(type_err)?,
    );
    if payload_hash(&V::Array(before_positions))? != payload_hash(&V::Array(current_positions))?
        || payload_hash(&V::Array(before_orders))? != payload_hash(&V::Array(current_orders))?
    {
        return Err("PHYSICAL_BLOCK_ACCOUNT_CHANGED".to_string());
    }
    let mut next = state.clone();
    let closed_list = next
        .get_mut("orders")
        .and_then(V::as_array_mut)
        .ok_or_else(type_err)?;
    let closed = closed_list
        .iter_mut()
        .find(|x| get(x, "intentId") == get(&o, "intentId"))
        .ok_or_else(type_err)?;
    let closed = closed.as_object_mut().ok_or_else(type_err)?;
    closed.insert("phase".into(), json!("abandoned"));
    closed.insert("abandonedAt".into(), at.clone());
    closed.insert(
        "abandonedReason".into(),
        json!("expired_affirmatively_blocked_before_fetch_reconciled"),
    );
    closed.insert("blockedAdmissionAudit".into(), audit.clone());
    let mut proof = Map::new();
    proof.insert("schemaVersion".into(), json!(1));
    proof.insert(
        "kind".into(),
        json!("physical_guard_scoped_reconciliation_v1"),
    );
    proof.insert(
        "originalEconomicStateHash".into(),
        json!(payload_hash(state)?),
    );
    proof.insert(
        "closedEconomicStateHash".into(),
        json!(payload_hash(&next)?),
    );
    proof.insert("auditHash".into(), json!(payload_hash(&audit)?));
    proof.insert("afterHash".into(), json!(payload_hash(after)?));
    proof.insert("at".into(), at.clone());
    proof.insert("executionAuthority".into(), json!(false));
    proof.insert("clearsStop".into(), json!(false));
    let mut out = Map::new();
    out.insert("state".into(), next);
    out.insert("proof".into(), V::Object(proof));
    Ok(V::Object(out))
}
