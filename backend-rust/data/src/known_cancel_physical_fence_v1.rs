// execution/known-cancel-physical-fence-v1.mjs
use baal_service_runtime::json_text::js_truthy;
use serde_json::{json, Value as V};

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn sget<'a>(v: &'a V, k: &str) -> Option<&'a str> {
    v.get(k).and_then(V::as_str)
}
fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("CANCEL_PHYSICAL_{code}"))
    }
}

// Date.parse: these fields hold canonical UTC ISO timestamps; anything else
// yields NaN so comparisons fail exactly like JS (NaN !== NaN, x >= NaN is
// false, Math.min propagates NaN, Number.isSafeInteger(NaN) is false).
fn date_parse(v: &V) -> f64 {
    v.as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.timestamp_millis() as f64)
        .unwrap_or(f64::NAN)
}

// '/orders/'+intent.orderId — JS string concatenation coercion.
fn js_concat_string(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Number(n) => n.to_string(),
        V::Bool(b) => b.to_string(),
        _ => String::new(),
    }
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
            && js_truthy(intent.get("orderId"))
            && get(intent, "cancelPending") == &V::Bool(true)
            && sget(meta, "endpoint")
                == Some(format!("/orders/{}", js_concat_string(get(intent, "orderId"))).as_str())
            && meta.get("requestBodyHash") == Some(&V::Null),
        "OWN_BOUND_DELETE",
    )?;
    let body_text = sget(get(evidence, "order"), "bodyText").unwrap_or("");
    let d: V = serde_json::from_str(body_text).map_err(|e| e.to_string())?;
    let order_num = js_concat_string(get(intent, "orderId")).parse::<f64>().ok();
    need(
        get(&d, "id").as_f64() == order_num
            && sget(&d, "exchangeId") == sget(intent, "exchangeId")
            && sget(&d, "side") == sget(get(intent, "body"), "side")
            && sget(&d, "action") == sget(get(intent, "body"), "action")
            && get(&d, "open") == &V::Bool(true)
            && sget(&d, "expirationDate") == sget(get(intent, "body"), "expirationDate")
            && now >= date_parse(get(get(intent, "body"), "expirationDate")),
        "EXPIRED_CURRENT_OWN_ORDER",
    )?;
    let a = date_parse(get(get(evidence, "order"), "requestStartedAt")) + 20000.0;
    let b = date_parse(get(get(evidence, "globalRisk"), "at")) + 20000.0;
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
    let not_after = if deadline.fract() == 0.0 && deadline.abs() <= 9.0e15 {
        json!(deadline as i64)
    } else {
        json!(deadline)
    };
    Ok(json!({"notAfter": not_after}))
}
