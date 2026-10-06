// live-paper/basket-pair-observer.mjs — a complete paired read is one economic
// observation: both receipts are validated through the original observer, but
// the first half pair is never published/counted.
use crate::basket_observer::observe_basket;
use baal_service_runtime::contract::parse_iso_ms;
use serde_json::Value as V;
use std::cmp::Ordering;

/// JS `===` between two possibly-undefined values (None = undefined).
/// Scalars compare by value (NaN never equal), objects/arrays by identity —
/// always distinct across parsed JSON.
fn strict_eq(a: Option<&V>, b: Option<&V>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(V::Null), Some(V::Null)) => true,
        (Some(V::Bool(x)), Some(V::Bool(y))) => x == y,
        (Some(V::Number(x)), Some(V::Number(y))) => x.as_f64() == y.as_f64(),
        (Some(V::String(x)), Some(V::String(y))) => x == y,
        _ => false,
    }
}
/// SameValueZero for `new Set(...)`: like `===` except NaN equals NaN.
fn same_value_zero(a: Option<&V>, b: Option<&V>) -> bool {
    match (a, b) {
        (Some(V::Number(x)), Some(V::Number(y))) => {
            let (x, y) = (x.as_f64().unwrap_or(f64::NAN), y.as_f64().unwrap_or(f64::NAN));
            x == y || (x.is_nan() && y.is_nan())
        }
        _ => strict_eq(a, b),
    }
}
/// `b?.exchangeId` — undefined unless b is an object carrying the key.
fn exch_id(b: &V) -> Option<V> {
    b.get("exchangeId").cloned()
}
fn received_ms(b: &V) -> Option<f64> {
    b.get("receivedAt").and_then(V::as_str).and_then(parse_iso_ms)
}
fn invalidated_status(observation: &V) -> bool {
    ["before_cutoff", "request_before_invalidation"]
        .contains(&observation["status"].as_str().unwrap_or(""))
}

/// observeCompleteBasketPair(input, books, bank) — returns the second leg's
/// {state, observation} after restoring only derived economics from input.
pub fn observe_complete_basket_pair(input: &V, books: &V, bank: &V) -> Result<V, String> {
    let bs = books.as_array().cloned().unwrap_or_default();
    // !Array.isArray(books) || books.length!==2 || new Set(exchangeIds).size!==2
    // || bank.legs.some(l => !books.some(b => b?.exchangeId===l.exchangeId && b.marketId===l.marketId))
    if !books.is_array() || bs.len() != 2 {
        return Err("INCOMPLETE_BASKET_PAIR".into());
    }
    if same_value_zero(exch_id(&bs[0]).as_ref(), exch_id(&bs[1]).as_ref()) {
        return Err("INCOMPLETE_BASKET_PAIR".into());
    }
    let legs = match bank.get("legs") {
        None => return Err("Cannot read properties of undefined (reading 'some')".into()),
        Some(V::Null) => return Err("Cannot read properties of null (reading 'some')".into()),
        Some(V::Array(a)) => a.clone(),
        Some(_) => return Err("bank.legs.some is not a function".into()),
    };
    if legs.iter().any(|l| {
        !bs.iter().any(|b| {
            strict_eq(b.get("exchangeId"), l.get("exchangeId"))
                && strict_eq(b.get("marketId"), l.get("marketId"))
        })
    }) {
        return Err("INCOMPLETE_BASKET_PAIR".into());
    }
    // [...books].sort((a,b)=>Date.parse(a.receivedAt)-Date.parse(b.receivedAt)):
    // stable; a NaN comparator result behaves as 0.
    let mut ordered = bs;
    ordered.sort_by(|a, b| match (received_ms(a), received_ms(b)) {
        (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
        _ => Ordering::Equal,
    });
    let first = observe_basket(input, &ordered[0], bank)?;
    match first.get("observation") {
        None => return Err("Cannot read properties of undefined (reading 'status')".into()),
        Some(V::Null) => return Err("Cannot read properties of null (reading 'status')".into()),
        _ => {}
    }
    if invalidated_status(&first["observation"]) {
        return Err("BASKET_PAIR_INVALIDATED_INPUT".into());
    }
    let mut staged = first["state"].clone();
    if !staged.is_object() {
        return Err("Cannot create property 'positiveAnchor' on a non-object value".into());
    }
    // Restore only derived economics, never clocks, depth, versions or exclusions.
    // A transient stale old peer does not erase the previous complete observation.
    // structuredClone(input[f]) with an absent key stores undefined, which
    // JSON.stringify drops — removing the key is equivalent.
    for f in ["positiveAnchor", "positivePersistenceCount", "lastPairHash", "lastObservation"] {
        match input.get(f) {
            Some(v) => staged[f] = v.clone(),
            None => {
                if let Some(m) = staged.as_object_mut() {
                    m.remove(f);
                }
            }
        }
    }
    // input.counts[f]: null/undefined throws, a primitive reads undefined.
    let input_counts = match input.get("counts") {
        None => return Err("Cannot read properties of undefined (reading 'cofreshPairs')".into()),
        Some(V::Null) => {
            return Err("Cannot read properties of null (reading 'cofreshPairs')".into());
        }
        Some(c) if c.is_object() => Some(c.clone()),
        Some(_) => None,
    };
    if !staged["counts"].is_object() {
        return Err("Cannot set properties of undefined (setting 'cofreshPairs')".into());
    }
    for f in ["cofreshPairs", "apparentPositivePairs", "independentPositiveConfirmations"] {
        match input_counts.as_ref().and_then(|c| c.get(f)) {
            Some(v) => staged["counts"][f] = v.clone(),
            None => {
                if let Some(m) = staged["counts"].as_object_mut() {
                    m.remove(f);
                }
            }
        }
    }
    let second = observe_basket(&staged, &ordered[1], bank)?;
    match second.get("observation") {
        None => return Err("Cannot read properties of undefined (reading 'status')".into()),
        Some(V::Null) => return Err("Cannot read properties of null (reading 'status')".into()),
        _ => {}
    }
    if invalidated_status(&second["observation"]) {
        return Err("BASKET_PAIR_INVALIDATED_INPUT".into());
    }
    Ok(second)
}
