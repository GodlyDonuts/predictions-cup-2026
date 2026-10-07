// execution/known-cancel-physical-fence-v1.mjs — the physical cancel fence is
// a bounded own-order DELETE on an expired, still-open order whose order body
// and global-risk snapshots are fresh. Returns {notAfter}; every other shape
// is a stop.
//
// Port notes:
// - `need` throws Error('CANCEL_PHYSICAL_<code>'); carried as the error string.
// - `meta.endpoint === '/orders/' + intent.orderId` — JS '+' ToString on the
//   orderId (js_concat_key); in practice it is an order-id string.
// - `meta.requestBodyHash === null` — key must exist and be JSON null; an
//   absent key (JS undefined) is !== null.
// - `d.id === Number(intent.orderId)` — JS Number() coercion of the string.
// - `Math.min` on NaN propagates NaN (f64::min would drop it), so the deadline
//   is NaN when either parse fails — Number.isSafeInteger then fails the need.
// - `now >= Date.parse(...)` on a bad date is `now >= NaN` -> false.
use baal_service_runtime::contract::parse_iso_ms;
use serde_json::{json, Value as V};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("CANCEL_PHYSICAL_{code}"))
    }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn sget<'a>(v: &'a V, k: &str) -> Option<&'a str> {
    v.get(k).and_then(V::as_str)
}
// JS === on JSON values: primitives equal by value/type; objects, arrays and
// null are only === to themselves, which two parsed values never are.
fn strict_eq(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Null, V::Null) => true,
        (V::Bool(x), V::Bool(y)) => x == y,
        (V::Number(x), V::Number(y)) => {
            x.as_f64().map(|v| v.to_bits()) == y.as_f64().map(|v| v.to_bits())
        }
        (V::String(x), V::String(y)) => x == y,
        _ => false,
    }
}
// JS ToString(value) for the '/orders/' + intent.orderId concatenation.
fn js_concat(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Number(n) => {
            let f = n.as_f64().unwrap_or(f64::NAN);
            if f.is_finite() && f.fract() == 0.0 && f.abs() < 1e21 {
                format!("{}", f as i128)
            } else {
                format!("{f}")
            }
        }
        V::Bool(b) => b.to_string(),
        V::Null => "null".to_string(),
        V::Array(a) => a.iter().map(js_concat).collect::<Vec<_>>().join(","),
        V::Object(_) => "[object Object]".to_string(),
    }
}
// JS Number(string): trimmed decimal parse; empty -> 0; failure -> NaN.
fn js_number(v: &V) -> f64 {
    match v {
        V::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        V::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                0.0
            } else {
                t.parse::<f64>().unwrap_or(f64::NAN)
            }
        }
        V::Bool(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        V::Null => 0.0,
        _ => f64::NAN,
    }
}
fn dparse(v: &V) -> f64 {
    v.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN)
}

/// knownCancelPhysicalDeadlineV1({meta,intent,evidence,now})
pub fn known_cancel_physical_deadline_v1(
    meta: &V,
    intent: &V,
    evidence: &V,
    now: f64,
) -> Result<V, String> {
    need(
        sget(meta, "method") == Some("DELETE")
            && baal_service_runtime::json_text::js_truthy(intent.get("orderId"))
            && get(intent, "cancelPending") == &V::Bool(true)
            && sget(meta, "endpoint")
                == Some(format!("/orders/{}", js_concat(get(intent, "orderId"))).as_str())
            && meta.get("requestBodyHash").is_some_and(|v| v.is_null()),
        "OWN_BOUND_DELETE",
    )?;
    // JSON.parse(evidence.order.bodyText): a missing key is JS undefined ->
    // SyntaxError; a non-string is coerced with String() before parsing. Parse
    // failures propagate as raw errors (not a CANCEL_PHYSICAL code).
    let d: V = match get(evidence, "order").get("bodyText") {
        None => return Err("bodyText is not valid JSON".into()),
        Some(V::String(s)) => serde_json::from_str(s),
        Some(other) => serde_json::from_str(&crate::js_key(other)),
    }
    .map_err(|e| e.to_string())?;
    let body = get(intent, "body");
    need(
        js_number(get(intent, "orderId"))
            == get(&d, "id").as_f64().unwrap_or(f64::NAN)
            && strict_eq(get(&d, "exchangeId"), get(intent, "exchangeId"))
            && strict_eq(get(&d, "side"), get(body, "side"))
            && strict_eq(get(&d, "action"), get(body, "action"))
            && get(&d, "open") == &V::Bool(true)
            && strict_eq(get(&d, "expirationDate"), get(body, "expirationDate"))
            && now >= dparse(get(body, "expirationDate")),
        "EXPIRED_CURRENT_OWN_ORDER",
    )?;
    let a = dparse(get(get(evidence, "order"), "requestStartedAt")) + 20000.0;
    let b = dparse(get(get(evidence, "globalRisk"), "at")) + 20000.0;
    let deadline = if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.min(b)
    };
    need(
        deadline.is_finite()
            && deadline.fract() == 0.0
            && deadline.abs() <= 9_007_199_254_740_991.0
            && now < deadline,
        "FRESH_ORDER_AND_RISK",
    )?;
    Ok(json!({"notAfter": deadline}))
}
