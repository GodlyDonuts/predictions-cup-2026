// paper/test-helpers.mjs — synthetic config/event builders for engine tests.
// Values are plain JSON (serde_json::Value); object literals/spreads keep the
// JS first-seen key order through serde_json's order-preserving Map.
use serde_json::{json, Map, Value as V};

// Date.UTC(2026, 0, 1) in epoch milliseconds.
const BASE_MS: f64 = 1_767_225_600_000.0;

// `{...fields}` — JS object spread. Map::insert updates an existing key in
// place (the key keeps its original position) and appends new keys, exactly
// like the JS literal. Arrays spread index keys, strings spread UTF-16 code
// units as one-char strings, and other primitives contribute nothing.
fn spread(m: &mut Map<String, V>, fields: &V) {
    match fields {
        V::Object(o) => {
            for (k, v) in o {
                m.insert(k.clone(), v.clone());
            }
        }
        V::Array(a) => {
            for (i, v) in a.iter().enumerate() {
                m.insert(i.to_string(), v.clone());
            }
        }
        V::String(s) => {
            for (i, u) in s.encode_utf16().enumerate() {
                m.insert(i.to_string(), json!(char::from_u32(u as u32).map(|c| c.to_string()).unwrap_or_default()));
            }
        }
        _ => {}
    }
}

// JS ToString for a finite number — the exact spelling `${at}` / `String(at)`
// produces (shortest round-trip decimal, exponent form beyond 1e21). A
// non-finite `at` yields no event in JS either: timestamp() throws RangeError
// before the literal finishes evaluating.
fn js_num(n: f64) -> String {
    baal_service_runtime::json_text::js_stringify(&json!(n))
}

/// timestamp(milliseconds) — new Date(Date.UTC(2026, 0, 1) + ms).toISOString().
pub fn timestamp(milliseconds: impl Into<f64>) -> String {
    baal_service_runtime::contract::ms_to_iso(BASE_MS + milliseconds.into())
}

/// config(overrides) — `{schemaVersion: 1, initialCash: '100.000000', ...,
/// costs: {...}, ...overrides}`: a shallow spread, so an override like `costs`
/// replaces the whole nested value. Pass &V::Null or an empty object for the
/// JS `config()` call (overrides === undefined spreads nothing).
pub fn config(overrides: &V) -> V {
    let mut c = json!({
        "schemaVersion": 1,
        "initialCash": "100.000000",
        "maxDataAgeMs": 10_000,
        "orderLatencyMs": 0,
        "cancelLatencyMs": 50,
        "maxPositionShares": 100,
        "costs": {"feeBps": 0, "feePerShare": "0", "assumption": "unverified_scenario"},
    });
    spread(c.as_object_mut().unwrap(), overrides);
    c
}
/// config() — overrides omitted.
pub fn config_default() -> V {
    config(&V::Null)
}

/// event(type, at, fields = {}) — `{schemaVersion: 1, eventId: `${type}-${at}`,
/// type, occurredAt, receivedAt, ...fields}`.
pub fn event(ty: &str, at: impl Into<f64>, fields: &V) -> V {
    let at = at.into();
    let iso = timestamp(at);
    let mut e = json!({
        "schemaVersion": 1,
        "eventId": format!("{ty}-{}", js_num(at)),
        "type": ty,
        "occurredAt": iso,
        "receivedAt": iso,
    });
    spread(e.as_object_mut().unwrap(), fields);
    e
}
/// event(type, at) — fields omitted.
pub fn event_default(ty: &str, at: impl Into<f64>) -> V {
    event(ty, at, &V::Null)
}

/// level(price, quantity, ownQuantity = 0) — `{price, quantity, ownQuantity}`.
pub fn level(price: impl Into<V>, quantity: impl Into<V>, own_quantity: impl Into<V>) -> V {
    json!({"price": price.into(), "quantity": quantity.into(), "ownQuantity": own_quantity.into()})
}
/// level(price, quantity) — ownQuantity omitted.
pub fn level_default(price: impl Into<V>, quantity: impl Into<V>) -> V {
    level(price, quantity, 0)
}

/// snapshot(at = 0, fields = {}) — event('snapshot', at, {instrument:
/// 'synthetic:YES', sequence: '1', authoritative: true, nextExpiryAt: null,
/// bids: [level('0.400', 10)], asks: [level('0.600', 10)], ...fields}).
pub fn snapshot(at: impl Into<f64>, fields: &V) -> V {
    let at = at.into();
    let mut f = json!({
        "instrument": "synthetic:YES",
        "sequence": "1",
        "authoritative": true,
        "nextExpiryAt": null,
        "bids": [level("0.400", 10, 0)],
        "asks": [level("0.600", 10, 0)],
    });
    spread(f.as_object_mut().unwrap(), fields);
    event("snapshot", at, &f)
}
/// snapshot() — the at = 0 default.
pub fn snapshot_default() -> V {
    snapshot(0, &V::Null)
}

/// order(at = 1, fields = {}) — event('submit', at, {instrument:
/// 'synthetic:YES', orderId: `o-${at}`, action: 'buy', quantity: 5,
/// limitPrice: '0.400', expiresAt: null, ...fields}).
pub fn order(at: impl Into<f64>, fields: &V) -> V {
    let at = at.into();
    let mut f = json!({
        "instrument": "synthetic:YES",
        "orderId": format!("o-{}", js_num(at)),
        "action": "buy",
        "quantity": 5,
        "limitPrice": "0.400",
        "expiresAt": null,
    });
    spread(f.as_object_mut().unwrap(), fields);
    event("submit", at, &f)
}
/// order() — the at = 1 default.
pub fn order_default() -> V {
    order(1, &V::Null)
}

/// trade(at = 2, fields = {}) — event('trade', at, {instrument:
/// 'synthetic:YES', sequence: String(at), bookSequence: '1', price: '0.400',
/// quantity: 11, aggressor: 'sell', evidence: 'explicit_trade',
/// externalAggressor: true, ...fields}).
pub fn trade(at: impl Into<f64>, fields: &V) -> V {
    let at = at.into();
    let mut f = json!({
        "instrument": "synthetic:YES",
        "sequence": js_num(at),
        "bookSequence": "1",
        "price": "0.400",
        "quantity": 11,
        "aggressor": "sell",
        "evidence": "explicit_trade",
        "externalAggressor": true,
    });
    spread(f.as_object_mut().unwrap(), fields);
    event("trade", at, &f)
}
/// trade() — the at = 2 default.
pub fn trade_default() -> V {
    trade(2, &V::Null)
}
