// execution/blocked-admission-audit-v1.mjs — historical evidence only. This
// does not close the intent or clear a STOP.
use baal_service_runtime::contract::parse_iso_ms;
use baal_service_runtime::json_text::js_truthy;
use serde_json::{json, Map, Value as V};

use crate::core::payload_hash;
use crate::read_timing::validate_timing;

fn need(ok: bool, c: &str) -> Result<(), String> {
    if !ok {
        Err(format!("BLOCKED_ADMISSION_{c}"))
    } else {
        Ok(())
    }
}

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}

fn sget<'a>(v: &'a V, k: &str) -> Option<&'a str> {
    v.get(k).and_then(V::as_str)
}

/// Date.parse(x): NaN on any non-string/unparseable value — every comparison
/// against it fails, exactly like JavaScript.
fn dparse(v: &V) -> f64 {
    v.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN)
}

/// auditBlockedAdmissionV1({state,events,timings,now})
pub fn audit_blocked_admission_v1(input: &V) -> Result<V, String> {
    let state = get(input, "state");
    let events = get(input, "events");
    let timings = get(input, "timings");
    let now = get(input, "now").as_f64().unwrap_or(f64::NAN);
    let orders = get(state, "orders").as_array().cloned().unwrap_or_default();
    let active: Vec<&V> = orders
        .iter()
        .filter(|o| !js_truthy(Some(get(o, "proof"))) && sget(o, "phase") != Some("abandoned"))
        .collect();
    need(active.len() == 1, "ONE_INTENT")?;
    let o = active[0];
    need(
        sget(o, "phase") == Some("uncertain")
            && sget(o, "operation") == Some("place")
            && o.get("orderId") == Some(&V::Null)
            && o.get("ack") == Some(&V::Null)
            && !js_truthy(Some(get(o, "proof")))
            && !js_truthy(Some(get(o, "cancelPending")))
            && get(o, "fetchStarts").as_array().is_some_and(|a| a.is_empty())
            && get(o, "fences").as_array().is_some_and(|a| a.is_empty())
            && get(o, "receipts").as_array().is_some_and(|a| a.is_empty()),
        "NO_PHYSICAL_OR_ACCEPTANCE_EVIDENCE",
    )?;
    need(
        get(o, "bodyHash").as_str() == Some(payload_hash(get(o, "body"))?.as_str())
            && dparse(get(get(o, "body"), "expirationDate")) < now,
        "EXACT_EXPIRED_BODY",
    )?;
    let ev = events.as_array().cloned().unwrap_or_default();
    let tail: Vec<V> = ev.iter().rev().take(2).rev().cloned().collect();
    need(
        tail.len() == 2
            && sget(&tail[0], "kind") == Some("admitted")
            && sget(&tail[0], "operation") == Some("place")
            && sget(&tail[1], "kind") == Some("error")
            && sget(&tail[1], "code") == Some("EXECUTION_AUTHORITY_UNAVAILABLE"),
        "EXACT_TERMINAL_GATE_PATH",
    )?;
    let from = dparse(get(o, "createdAt"));
    let admitted = dparse(get(&tail[0], "at"));
    let failed = dparse(get(&tail[1], "at"));
    need(from <= admitted && admitted <= failed, "CLOCKS")?;
    let rows: Vec<V> = timings
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|t| {
            sget(t, "kind") == Some("orders")
                && dparse(get(t, "queuedAt")) >= from
                && dparse(get(t, "queuedAt")) <= failed
        })
        .collect();
    need(rows.len() == 1, "UNIQUE_ORDER_TIMING")?;
    let t = &rows[0];
    validate_timing(t)?;
    need(
        sget(t, "outcome") == Some("blocked")
            && t.get("admittedAt") == Some(&V::Null)
            && t.get("serviceMs") == Some(&V::Null)
            && t.get("statusCode") == Some(&V::Null)
            && dparse(get(t, "finishedAt")) >= admitted
            && dparse(get(t, "finishedAt")) <= failed,
        "BEFORE_PHYSICAL_CLIENT_FETCH",
    )?;
    let mut out = Map::new();
    out.insert("schemaVersion".into(), json!(1));
    out.insert("kind".into(), json!("historical_blocked_admission_audit_v1"));
    out.insert("intentBodyHash".into(), get(o, "bodyHash").clone());
    out.insert("eventHash".into(), json!(payload_hash(&V::Array(tail))?));
    out.insert("timingHash".into(), json!(payload_hash(t)?));
    out.insert("expired".into(), json!(true));
    out.insert("recordedPhysicalFetches".into(), json!(0));
    out.insert("recordedNetworkFences".into(), json!(0));
    out.insert("executionAuthority".into(), json!(false));
    out.insert("clearsStop".into(), json!(false));
    Ok(V::Object(out))
}
