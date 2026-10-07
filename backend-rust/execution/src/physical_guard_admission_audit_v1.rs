// execution/physical-guard-admission-audit-v1.mjs — requires affirmative
// producer-side rejection evidence, not mere missing fills.
//
// Port notes:
// - `need` throws Error('PHYSICAL_BLOCK_<code>') — bare code strings.
// - `.filter/.slice` on a non-array would be a JS TypeError; surfaced here as
//   the un-prefixed "TYPE_ERROR" string like other TypeError-equivalents.
// - `x===null` requires an explicit JSON null (absent key rejects), while
//   `x?.length===n` reads the JS `length` property of arrays, strings, and
//   objects carrying a numeric `length` key.
// - Date.parse of a non-string yields NaN; NaN comparisons are false.
// - payloadHash throws propagate as Err, exactly like the JS calls.
use baal_service_runtime::contract::parse_iso_ms;
use baal_service_runtime::json_text::js_truthy;
use serde_json::{json, Map, Value as V};

use crate::core::payload_hash;
use crate::read_timing::validate_timing;

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("PHYSICAL_BLOCK_{code}"))
    }
}
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
// `v?.length === n` — JS property access, not Array.isArray.
fn js_len_eq(v: &V, n: u64) -> bool {
    match v {
        V::Array(a) => a.len() as u64 == n,
        V::String(s) => s.encode_utf16().count() as u64 == n,
        V::Object(m) => m.get("length").and_then(V::as_f64) == Some(n as f64),
        _ => false,
    }
}

/// auditPhysicalGuardAdmissionV1({state,events,timings,diagnostic,guardFailure,
///   stop,now})
pub fn audit_physical_guard_admission_v1(input: &V) -> Result<V, String> {
    let state = get(input, "state");
    let events = get(input, "events")
        .as_array()
        .ok_or_else(type_err)?;
    let timings = get(input, "timings")
        .as_array()
        .ok_or_else(type_err)?;
    let diagnostic = get(input, "diagnostic");
    let guard_failure = get(input, "guardFailure");
    let stop = get(input, "stop");
    let now = get(input, "now").as_f64().unwrap_or(f64::NAN);

    let orders = get(state, "orders").as_array().ok_or_else(type_err)?;
    let active: Vec<&V> = orders
        .iter()
        .filter(|o| !js_truthy(o.get("proof")) && sget(o, "phase") != Some("abandoned"))
        .collect();
    need(active.len() == 1, "ONE_INTENT")?;
    let o = active[0];
    need(
        sget(o, "phase") == Some("uncertain")
            && sget(o, "operation") == Some("place")
            && o.get("orderId") == Some(&V::Null)
            && o.get("ack") == Some(&V::Null)
            && js_len_eq(get(o, "fences"), 1)
            && js_len_eq(get(o, "fetchStarts"), 0)
            && js_len_eq(get(o, "receipts"), 0)
            && !js_truthy(o.get("cancelPending"))
            && get(o, "bodyHash").as_str()
                == Some(payload_hash(get(o, "body"))?.as_str())
            && dparse(get(get(o, "body"), "expirationDate")) < now,
        "EXPIRED_NO_INVOCATION",
    )?;
    let tail: Vec<V> = events
        .iter()
        .rev()
        .take(3)
        .rev()
        .cloned()
        .collect();
    let t0 = tail.first().unwrap_or(&V::Null);
    let t1 = tail.get(1).unwrap_or(&V::Null);
    let t2 = tail.get(2).unwrap_or(&V::Null);
    need(
        sget(t0, "kind") == Some("admitted")
            && sget(t1, "kind") == Some("network_fence")
            && sget(t2, "kind") == Some("error")
            && sget(t2, "code") == Some("REQUEST_UNCONFIRMED")
            && sget(t0, "operation") == Some("place")
            && sget(t1, "operation") == Some("place"),
        "EXACT_PATH",
    )?;
    let failed = dparse(get(t2, "at"));
    let fenced = dparse(get(t1, "at"));
    let from = dparse(get(o, "createdAt"));
    need(
        sget(guard_failure, "stage") == Some("physical")
            && sget(guard_failure, "code") == Some("BAAL_SCALED_PHYSICAL_FETCH_FENCE")
            && get(guard_failure, "deadlineRemainingMs")
                .as_f64()
                .is_some_and(|x| x < 0.0)
            && dparse(get(guard_failure, "at")) >= fenced
            && dparse(get(guard_failure, "at")) <= failed,
        "PHYSICAL_CLOCK_FAILURE",
    )?;
    let failures = get(diagnostic, "failures")
        .as_array()
        .ok_or_else(type_err)?;
    let faults: Vec<&V> = failures
        .iter()
        .filter(|f| {
            sget(f, "method") == Some("WRITE")
                && sget(f, "code") == Some("REST_RECEIPT_BEFORE_FETCH_GUARD")
                && get(f, "mayHaveExecuted") == &V::Bool(false)
                && get(f, "durableRecordingFailed") == &V::Bool(false)
                && get(f, "at") == get(guard_failure, "at")
        })
        .collect();
    need(faults.len() == 1, "AFFIRMATIVE_BEFORE_FETCH")?;
    let rows: Vec<&V> = timings
        .iter()
        .filter(|t| {
            sget(t, "kind") == Some("orders")
                && dparse(get(t, "queuedAt")) >= from
                && dparse(get(t, "queuedAt")) <= failed
        })
        .collect();
    need(rows.len() == 1, "UNIQUE_TIMING")?;
    let t = rows[0];
    validate_timing(t)?;
    need(
        sget(t, "outcome") == Some("connection_error")
            && t.get("statusCode") == Some(&V::Null)
            && t.get("bodyMs") == Some(&V::Null)
            && dparse(get(t, "admittedAt")) >= fenced
            && dparse(get(t, "finishedAt")) <= failed,
        "NO_HTTP_RESPONSE",
    )?;
    need(
        sget(stop, "kind") == Some("generic_financial_uncertainty")
            && get(stop, "sourceHash") == get(o, "bodyHash")
            && get(stop, "ownerId") == get(get(state, "scope"), "ownerId")
            && get(stop, "runId") == get(get(state, "scope"), "runId")
            && get(stop, "allWritersStop") == &V::Bool(true),
        "EXACT_STOP",
    )?;
    let mut out = Map::new();
    out.insert("schemaVersion".into(), json!(1));
    out.insert(
        "kind".into(),
        json!("affirmative_physical_guard_block_audit_v1"),
    );
    out.insert("intentBodyHash".into(), get(o, "bodyHash").clone());
    out.insert(
        "eventHash".into(),
        json!(payload_hash(&V::Array(tail))?),
    );
    out.insert("timingHash".into(), json!(payload_hash(t)?));
    out.insert("diagnosticHash".into(), json!(payload_hash(faults[0])?));
    out.insert(
        "guardFailureHash".into(),
        json!(payload_hash(guard_failure)?),
    );
    out.insert("stopHash".into(), json!(payload_hash(stop)?));
    out.insert("expired".into(), json!(true));
    out.insert("recordedPhysicalFetches".into(), json!(0));
    out.insert("physicalFetchAffirmativelyRejected".into(), json!(true));
    out.insert("executionAuthority".into(), json!(false));
    out.insert("clearsStop".into(), json!(false));
    Ok(V::Object(out))
}
