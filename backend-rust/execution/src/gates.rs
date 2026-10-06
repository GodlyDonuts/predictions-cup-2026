use crate::core::{decimal_micros, format_micros, SCALE, TICK};
use serde_json::Value as V;
use std::collections::BTreeMap;

// execution/api-tick-price.mjs: accept exactly the binary representations of
// each official tick — the canonical value plus IEEE-754 complement artifacts.
fn build_representations() -> BTreeMap<u64, V> {
    // keyed by f64 bits so -0 and NaN stay distinct deterministically.
    let mut map: BTreeMap<u64, V> = BTreeMap::new();
    for tick in 0..=200i64 {
        let micros = tick as i128 * TICK;
        let price = format_micros(micros);
        let canonical: f64 = price.parse().unwrap();
        let opposite: f64 = format_micros(SCALE - micros).parse().unwrap();
        for (value, kind) in [
            (canonical, "canonical_tick"),
            (1.0 - opposite, "binary_tick_complement"),
            (1.0 - (1.0 - canonical), "binary_complement_roundtrip"),
        ] {
            if let Some(prior) = map.get(&value.to_bits()) {
                if prior.get("price").and_then(V::as_str) != Some(price.as_str()) {
                    panic!("API_TICK_REPRESENTATION_COLLISION");
                }
            } else {
                map.insert(
                    value.to_bits(),
                    serde_json::json!({"price": price, "tick": tick, "representation": kind}),
                );
            }
        }
    }
    map
}
pub fn normalize_official_fill_tick(value: &V) -> Result<V, String> {
    if let Some(n) = value.as_f64() {
        let found = if n.is_finite() && n.to_bits() != (-0.0f64).to_bits() {
            build_representations().get(&n.to_bits()).cloned()
        } else {
            None
        };
        return found.ok_or_else(|| "UNRECOGNIZED_API_TICK_REPRESENTATION".to_string());
    }
    let micros = decimal_micros(value, "exact")?;
    if micros > SCALE || micros % TICK != 0 {
        return Err("OFFICIAL_FILL_PRICE_OFF_TICK".into());
    }
    Ok(serde_json::json!({
        "price": format_micros(micros),
        "tick": (micros / TICK) as i64,
        "representation": "exact_decimal_tick",
    }))
}

// execution/entry-gates.mjs: exact rational comparison of decimal lexemes.
use num_bigint::BigInt;
#[derive(Clone, Debug)]
struct Ratio {
    n: BigInt,
    d: BigInt,
}
fn decimal(value: &V) -> Result<Ratio, String> {
    let text = match value {
        V::String(s) => s.clone(),
        V::Number(_) => baal_service_runtime::json_text::js_stringify(value),
        _ => return Err("INVALID_GATE_DECIMAL".into()),
    };
    // /^(-?)(\d+)(?:\.(\d+))?(?:e([+-]?\d+))?$/i
    if text.len() > 128 {
        return Err("INVALID_GATE_DECIMAL".into());
    }
    let lower = text.to_lowercase();
    let (mantissa, exp_part) = match lower.split_once('e') {
        Some((m, e)) => (m.to_string(), e.to_string()),
        None => (lower.clone(), String::new()),
    };
    let exp: i64 = if exp_part.is_empty() {
        0
    } else {
        let e = exp_part.clone();
        let digits = e.trim_start_matches(['+', '-']);
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return Err("INVALID_GATE_DECIMAL".into());
        }
        let v: i64 = digits.parse().map_err(|_| "INVALID_GATE_DECIMAL".to_string())?;
        if e.starts_with('-') { -v } else { v }
    };
    let (negative, digits_part) = match mantissa.strip_prefix('-') {
        Some(rest) => (true, rest.to_string()),
        None => (false, mantissa),
    };
    let (whole, frac, has_dot) = match digits_part.split_once('.') {
        Some((w, f)) => (w.to_string(), f.to_string(), true),
        None => (digits_part, String::new(), false),
    };
    let valid = !whole.is_empty()
        && whole.bytes().all(|b| b.is_ascii_digit())
        && (!has_dot || !frac.is_empty())
        && frac.bytes().all(|b| b.is_ascii_digit());
    if !valid {
        return Err("INVALID_GATE_DECIMAL".into());
    }
    let exponent = exp - frac.len() as i64;
    if exponent.unsigned_abs() > 400 {
        return Err("INVALID_GATE_DECIMAL".into());
    }
    let mut coefficient: BigInt = (whole + &frac).parse().map_err(|_| "INVALID_GATE_DECIMAL".to_string())?;
    if negative {
        coefficient = -coefficient;
    }
    Ok(if exponent >= 0 {
        Ratio { n: coefficient * BigInt::from(10u8).pow(exponent as u32), d: BigInt::from(1u8) }
    } else {
        Ratio { n: coefficient, d: BigInt::from(10u8).pow((-exponent) as u32) }
    })
}
fn sub(a: &Ratio, b: &Ratio) -> Ratio {
    Ratio { n: &a.n * &b.d - &b.n * &a.d, d: &a.d * &b.d }
}
fn mul(a: &Ratio, b: &Ratio) -> Ratio {
    Ratio { n: &a.n * &b.n, d: &a.d * &b.d }
}
fn compare(a: &Ratio, b: &Ratio) -> i32 {
    let l = &a.n * &b.d;
    let r = &b.n * &a.d;
    if l < r { -1 } else if l > r { 1 } else { 0 }
}
pub fn evaluate_entry_gates(reference: &V, ask: &V, bid: &V, policy: &V) -> Result<V, String> {
    if reference.get("ready") != Some(&V::Bool(true)) {
        return Ok(serde_json::json!({"admitted": false, "tests": null}));
    }
    let a = decimal(ask)?;
    let b = decimal(bid)?;
    let center = decimal(reference.get("center").unwrap_or(&V::Null))?;
    let deviation = decimal(reference.get("robustDeviation").unwrap_or(&V::Null))?;
    let discount = sub(&center, &a);
    let floor = decimal(&serde_json::json!(0.001))?;
    let zero = decimal(&serde_json::json!(0))?;
    let denominator = if compare(&center, &floor) >= 0 { center.clone() } else { floor };
    let get = |k: &str| decimal(policy.get(k).unwrap_or(&V::Null));
    let tests = serde_json::json!({
        "price": compare(&a, &get("minimumPrice")?) >= 0 && compare(&a, &get("maximumPrice")?) <= 0,
        "spread": compare(&a, &b) > 0 && compare(&sub(&a, &b), &get("maximumSpread")?) <= 0,
        "minimumDiscount": compare(&discount, &get("minimumDip")?) >= 0,
        "relativeDiscount": compare(&discount, &mul(&denominator, &get("minimumRelativeDip")?)) >= 0,
        "maximumDiscount": compare(&discount, &get("maximumDip")?) <= 0,
        "robustZ": compare(&deviation, &zero) > 0 && compare(&discount, &mul(&deviation, &get("minimumZ")?)) >= 0,
    });
    let admitted = tests.as_object().unwrap().values().all(|v| v == &V::Bool(true));
    Ok(serde_json::json!({"admitted": admitted, "tests": tests}))
}

// execution/production-entry-state.mjs.
pub fn production_entry_state(
    activation: &V,
    now_ms: f64,
    research_market_ids: &[String],
    owned: &V,
    active_intent: Option<&V>,
    status: &str,
    reason: &str,
) -> V {
    let start = activation.get("startedAt").and_then(V::as_str).and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()).map(|d| d.timestamp_millis() as f64);
    let end = activation.get("entryExpiresAt").and_then(V::as_str).and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()).map(|d| d.timestamp_millis() as f64);
    let clock_valid = [start, end, Some(now_ms)].iter().all(|x| x.is_some_and(|v| v.is_finite())) && end > start;
    let phase = if !clock_valid {
        "invalid_clock"
    } else if now_ms < start.unwrap() {
        "not_started"
    } else if now_ms >= end.unwrap() {
        "closed"
    } else {
        "open"
    };
    let entry_window = serde_json::json!({
        "phase": phase,
        "startedAt": activation.get("startedAt").unwrap().clone(),
        "expiresAt": activation.get("entryExpiresAt").unwrap().clone(),
        "open": phase == "open",
    });
    let positions = owned
        .as_object()
        .map(|m| m.values().filter(|p| p.get("quantity").and_then(V::as_i64).unwrap_or(0) > 0).count())
        .unwrap_or(0);
    let mut dedup: Vec<String> = Vec::new();
    for id in research_market_ids {
        if !dedup.contains(id) {
            dedup.push(id.clone());
        }
    }
    let ordinary = [
        "monitoring", "waiting_for_signal", "waiting_for_research",
        "entry_window_closed", "entry_window_not_started", "managing_inventory",
    ]
    .contains(&status);
    let mut status = status.to_string();
    let mut reason = reason.to_string();
    if ordinary && active_intent.is_none() {
        if phase != "open" {
            status = if positions > 0 {
                "managing_inventory".into()
            } else {
                match phase {
                    "closed" => "entry_window_closed".into(),
                    "not_started" => "entry_window_not_started".into(),
                    _ => "entry_clock_invalid".into(),
                }
            };
            reason = match phase {
                "closed" => "ENTRY_WINDOW_CLOSED".into(),
                "not_started" => "ENTRY_WINDOW_NOT_STARTED".into(),
                _ => "INVALID_ACTIVATION_CLOCK".into(),
            };
        } else if positions == 0 && dedup.is_empty() {
            status = "waiting_for_research".into();
            reason = "NO_CURRENT_RESEARCH".into();
        }
    }
    serde_json::json!({
        "status": status,
        "reason": reason,
        "entryWindow": entry_window,
        "currentResearchMarkets": dedup,
        "attributedPositionsCount": positions,
        "entryAuthorityExtended": false,
    })
}
