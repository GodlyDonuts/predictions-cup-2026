// execution/physical-guard-admission-audit-v1.mjs — requires affirmative
// producer-side rejection evidence, not mere missing fills.
//
// Port notes:
// - `now` arrives as a JS number (ms); `Date.parse` sites become f64 with NaN
//   semantics (NaN comparisons are false, like JavaScript).
// - `=== null` tests compare Option presence (missing key fails like JS
//   `undefined === null` -> false); `!x` uses js_truthy.
use baal_collector::read_timing::validate_timing;
use baal_execution::core::payload_hash;
use baal_service_runtime::contract::parse_iso_ms;
use baal_service_runtime::json_text::js_truthy;
use serde_json::{json, Value as V};

fn need(x: bool, c: &str) -> Result<(), String> {
    if x {
        Ok(())
    } else {
        Err(format!("PHYSICAL_BLOCK_{c}"))
    }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn sget<'a>(v: &'a V, k: &str) -> Option<&'a str> {
    v.get(k).and_then(V::as_str)
}
fn dparse(v: &V) -> f64 {
    v.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN)
}

/// auditPhysicalGuardAdmissionV1({state,events,timings,diagnostic,guardFailure,stop,now})
pub fn audit_physical_guard_admission_v1(input: &V) -> Result<V, String> {
    let state = get(input, "state");
    let events = get(input, "events");
    let timings = get(input, "timings");
    let diagnostic = get(input, "diagnostic");
    let guard_failure = get(input, "guardFailure");
    let stop = get(input, "stop");
    let now = get(input, "now").as_f64().unwrap_or(f64::NAN);

    let orders = get(state, "orders").as_array().cloned().unwrap_or_default();
    let active: Vec<&V> = orders
        .iter()
        .filter(|o| {
            !js_truthy(o.get("proof")) && sget(o, "phase") != Some("abandoned")
        })
        .collect();
    need(active.len() == 1, "ONE_INTENT")?;
    let o = active[0];
    need(
        sget(o, "phase") == Some("uncertain")
            && sget(o, "operation") == Some("place")
            && o.get("orderId") == Some(&V::Null)
            && o.get("ack") == Some(&V::Null)
            && get(o, "fences").as_array().is_some_and(|a| a.len() == 1)
            && get(o, "fetchStarts").as_array().is_some_and(|a| a.is_empty())
            && get(o, "receipts").as_array().is_some_and(|a| a.is_empty())
            && !js_truthy(o.get("cancelPending"))
            && get(o, "bodyHash").as_str()
                == Some(payload_hash(get(o, "body"))?.as_str())
            && dparse(get(get(o, "body"), "expirationDate")) < now,
        "EXPIRED_NO_INVOCATION",
    )?;

    let all_events = events.as_array().cloned().unwrap_or_default();
    let tail: Vec<V> = all_events
        .iter()
        .skip(all_events.len().saturating_sub(3))
        .cloned()
        .collect();
    need(
        sget(idx(&tail, 0), "kind") == Some("admitted")
            && sget(idx(&tail, 1), "kind") == Some("network_fence")
            && sget(idx(&tail, 2), "kind") == Some("error")
            && sget(idx(&tail, 2), "code") == Some("REQUEST_UNCONFIRMED")
            && sget(idx(&tail, 0), "operation") == Some("place")
            && sget(idx(&tail, 1), "operation") == Some("place"),
        "EXACT_PATH",
    )?;
    let failed = dparse(get(idx(&tail, 2), "at"));
    let fenced = dparse(get(idx(&tail, 1), "at"));
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
        .cloned()
        .unwrap_or_default();
    let faults: Vec<&V> = failures
        .iter()
        .filter(|f| {
            sget(f, "method") == Some("WRITE")
                && sget(f, "code") == Some("REST_RECEIPT_BEFORE_FETCH_GUARD")
                && f.get("mayHaveExecuted") == Some(&V::Bool(false))
                && f.get("durableRecordingFailed") == Some(&V::Bool(false))
                && f.get("at") == guard_failure.get("at")
        })
        .collect();
    need(faults.len() == 1, "AFFIRMATIVE_BEFORE_FETCH")?;
    let timing_rows = timings.as_array().cloned().unwrap_or_default();
    let rows: Vec<&V> = timing_rows.iter()
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
            && stop.get("sourceHash") == o.get("bodyHash")
            && stop.get("ownerId") == get(state, "scope").get("ownerId")
            && stop.get("runId") == get(state, "scope").get("runId")
            && stop.get("allWritersStop") == Some(&V::Bool(true)),
        "EXACT_STOP",
    )?;
    Ok(json!({
        "schemaVersion": 1,
        "kind": "affirmative_physical_guard_block_audit_v1",
        "intentBodyHash": get(o, "bodyHash"),
        "eventHash": payload_hash(&V::Array(tail))?,
        "timingHash": payload_hash(t)?,
        "diagnosticHash": payload_hash(faults[0])?,
        "guardFailureHash": payload_hash(guard_failure)?,
        "stopHash": payload_hash(stop)?,
        "expired": true,
        "recordedPhysicalFetches": 0,
        "physicalFetchAffirmativelyRejected": true,
        "executionAuthority": false,
        "clearsStop": false,
    }))
}

fn idx<'a>(v: &'a [V], i: usize) -> &'a V {
    v.get(i).unwrap_or(&V::Null)
}
