use baal_service_runtime::json_text::js_int;
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Map, Value as V};
use std::collections::{BTreeMap, BTreeSet};

// execution/core.mjs — offline domain core. Pure state machine over JSON
// values; callers persist every returned state before sending traffic.
pub const SCALE: i128 = 1_000_000;
pub const TICK: i128 = 5_000;

fn default_policy() -> Map<String, V> {
    Map::from_iter([
        ("maxManagedMarkets".into(), json!(3)),
        ("maxAggregateCost".into(), json!("500.000000")),
        ("maxOrderCost".into(), json!("100.000000")),
        ("maxCumulativeBuyCost".into(), json!("500.000000")),
        ("cashReserve".into(), json!("0.000000")),
        ("maxAccountAgeMs".into(), json!(180_000)),
        ("maxBookAgeMs".into(), json!(20_000)),
        ("maxBookLatencyMs".into(), json!(20_000)),
        ("maxExpirationMs".into(), json!(120_000)),
    ])
}

fn fail<T>(code: &str) -> Result<T, String> {
    Err(code.to_string())
}
fn check(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { fail(code) }
}
fn id(x: &V) -> bool {
    x.as_str()
        .is_some_and(|s| !s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit()) && js_int(&json!(s.parse::<f64>().unwrap_or(f64::NAN))).is_some_and(|n| n > 0 && n <= 9_007_199_254_740_991))
}
fn id_str(s: &str) -> bool {
    id(&json!(s))
}
fn qty(v: &V) -> bool {
    js_int(v).is_some_and(|n| n > 0 && n <= 2_147_483_647)
}
fn stamp(v: &V) -> bool {
    v.as_str().is_some_and(|s| chrono::DateTime::parse_from_rfc3339(s).is_ok())
}
fn parse(s: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(s).unwrap().timestamp_millis()
}
fn clock(v: &V) -> Result<i64, String> {
    let n = if let Some(s) = v.as_str() {
        chrono::DateTime::parse_from_rfc3339(s).ok().map(|d| d.timestamp_millis())
    } else {
        js_int(v)
    };
    match n {
        Some(n) if n >= 0 => Ok(n),
        _ => fail("INVALID_CLOCK"),
    }
}
fn iso(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
fn safe_key(k: &str) -> bool {
    !["__proto__", "constructor", "prototype"].contains(&k)
}
fn plain(v: &V) -> bool {
    v.is_object()
}

// canonicalPayload: sorted-key JSON like ledger::canonical, plus plain-object
// and safeKey guards and the INVALID_JSON_NUMBER check.
pub fn canonical_payload(value: &V) -> Result<String, String> {
    fn walk(v: &V, out: &mut String) -> Result<(), String> {
        match v {
            V::Null | V::String(_) | V::Bool(_) => {
                out.push_str(&baal_service_runtime::json_text::js_stringify(v));
                Ok(())
            }
            V::Number(n) => {
                check(n.as_f64().is_some_and(|x| x.is_finite()), "INVALID_JSON_NUMBER")?;
                out.push_str(&baal_service_runtime::json_text::js_stringify(v));
                Ok(())
            }
            V::Array(a) => {
                out.push('[');
                for (i, x) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    walk(x, out)?;
                }
                out.push(']');
                Ok(())
            }
            V::Object(m) => {
                check(m.keys().all(|k| safe_key(k)), "INVALID_JSON_VALUE")?;
                out.push('{');
                let mut keys: Vec<&String> = m.keys().collect();
                keys.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
                for (i, k) in keys.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&serde_json::to_string(k).unwrap());
                    out.push(':');
                    walk(&m[*k], out)?;
                }
                out.push('}');
                Ok(())
            }
        }
    }
    let mut s = String::new();
    walk(value, &mut s)?;
    Ok(s)
}
pub fn payload_hash(value: &V) -> Result<String, String> {
    Ok(sha256(canonical_payload(value)?.as_bytes()))
}

// decimalMicros: canonical unsigned decimal with ≤6 places → micros.
pub fn decimal_micros(value: &V, rounding: &str) -> Result<i128, String> {
    let text = match value {
        V::String(s) => s.clone(),
        V::Number(n) => n.to_string(),
        _ => return fail("INVALID_DECIMAL"),
    };
    check(text.len() <= 128, "INVALID_DECIMAL")?;
    let (whole, fraction) = match text.split_once('.') {
        Some((w, f)) => (w, f),
        None => (text.as_str(), ""),
    };
    check(
        !whole.is_empty()
            && (whole == "0" || !whole.starts_with('0'))
            && whole.bytes().all(|b| b.is_ascii_digit())
            && fraction.bytes().all(|b| b.is_ascii_digit()),
        "INVALID_DECIMAL",
    )?;
    check(["exact", "floor", "ceil"].contains(&rounding), "INVALID_ROUNDING")?;
    let tail = &fraction[6.min(fraction.len())..];
    let extra = tail.bytes().any(|b| (b'1'..=b'9').contains(&b));
    check(rounding != "exact" || !extra, "EXCESS_DECIMAL_PRECISION")?;
    let frac6 = &fraction[..6.min(fraction.len())];
    let frac: i128 = frac6.parse::<i128>().unwrap_or(0) * 10_i128.pow((6 - frac6.len()) as u32);
    let whole_i: i128 = whole.parse().map_err(|_| "INVALID_DECIMAL".to_string())?;
    Ok(whole_i * SCALE + frac + if rounding == "ceil" && extra { 1 } else { 0 })
}
pub fn format_micros(value: i128) -> String {
    format!("{}.{:06}", value / SCALE, value % SCALE)
}
// Next representable double below `value` gives a conservative floor.
pub fn conservative_cash_floor(value: &V) -> Result<i128, String> {
    if let V::String(_) = value {
        return decimal_micros(value, "floor");
    }
    let n = value.as_f64();
    check(n.is_some_and(|x| x.is_finite() && x >= 0.0), "INVALID_CASH")?;
    let n = n.unwrap();
    if n == 0.0 {
        return Ok(0);
    }
    let bits = n.to_bits() - 1;
    let exponent = ((bits >> 52) & 2047) as i64;
    let mantissa = (bits & ((1u64 << 52) - 1)) as i128 + if exponent > 0 { 1i128 << 52 } else { 0 };
    let power = if exponent != 0 { exponent } else { 1 } - 1023 - 52;
    Ok(if power >= 0 {
        (mantissa << power) * SCALE
    } else {
        mantissa * SCALE / (1i128 << -power)
    })
}
pub fn price_micros(value: &V) -> Result<i128, String> {
    let units = decimal_micros(value, "exact")?;
    check(units >= TICK && units <= SCALE - TICK && units % TICK == 0, "INVALID_LIMIT_PRICE")?;
    Ok(units)
}

fn policy_of(input: Option<&V>) -> Result<Map<String, V>, String> {
    let defaults = default_policy();
    let input = input.unwrap_or(&V::Null);
    check(
        input.is_null() || (plain(input) && input.as_object().unwrap().keys().all(|k| defaults.contains_key(k))),
        "INVALID_POLICY",
    )?;
    let mut p = defaults.clone();
    if let Some(m) = input.as_object() {
        for (k, v) in m {
            p.insert(k.clone(), v.clone());
        }
    }
    let mm = p.get("maxManagedMarkets").and_then(V::as_i64).unwrap_or(-1);
    check(mm >= 1 && mm <= 3, "INVALID_MARKET_CAP")?;
    let order = decimal_micros(p.get("maxOrderCost").unwrap(), "exact")?;
    check(order > 0 && order <= 100 * SCALE, "INVALID_ORDER_CAP")?;
    let aggregate = decimal_micros(p.get("maxAggregateCost").unwrap(), "exact")?;
    check(aggregate >= order && aggregate <= 500 * SCALE, "INVALID_AGGREGATE_CAP")?;
    let cumulative = decimal_micros(p.get("maxCumulativeBuyCost").unwrap(), "exact")?;
    check(cumulative >= order && cumulative <= 500 * SCALE, "INVALID_CUMULATIVE_CAP")?;
    decimal_micros(p.get("cashReserve").unwrap(), "exact")?;
    for k in ["maxAccountAgeMs", "maxBookAgeMs", "maxBookLatencyMs", "maxExpirationMs"] {
        let v = js_int(p.get(k).unwrap());
        check(
            v.is_some() && v.unwrap() > 0 && v.unwrap() <= defaults.get(k).unwrap().as_i64().unwrap(),
            &format!("INVALID_{k}"),
        )?;
    }
    Ok(p)
}

const ORDER_BODY_KEYS: [&str; 8] = [
    "idempotencyKey", "exchangeId", "side", "action", "quantity", "price",
    "expirationDate", "tournamentId",
];
const STATUSES: [&str; 6] =
    ["prepared", "dispatching", "uncertain", "acknowledged", "reconciled", "closed"];

fn assert_state(s: &V) -> Result<(), String> {
    check(
        plain(s)
            && s.get("schemaVersion") == Some(&json!(1))
            && s.get("tournamentId").and_then(V::as_str).is_some_and(|t| t.len() == 36 && t.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-'))
            && plain(s.get("intents").unwrap_or(&V::Null))
            && plain(s.get("owned").unwrap_or(&V::Null)),
        "INVALID_STATE",
    )?;
    policy_of(s.get("policy"))?;
    let tid = s.get("tournamentId").and_then(V::as_str).unwrap().to_string();
    let mut keys = BTreeSet::new();
    let mut ordinals = BTreeSet::new();
    let mut order_ids = BTreeSet::new();
    for (intent_id, i) in s.get("intents").unwrap().as_object().unwrap() {
        check(
            safe_key(intent_id)
                && plain(i)
                && i.get("body").is_some_and(|b| {
                    b.as_object().is_some_and(|m| {
                        m.len() == 8 && m.keys().all(|k| ORDER_BODY_KEYS.contains(&k.as_str()))
                    })
                }),
            "INVALID_INTENT",
        )?;
        let body = i.get("body").unwrap();
        check(
            i.get("intentId").and_then(V::as_str) == Some(intent_id)
                && i.get("bodyJson").and_then(V::as_str) == Some(&canonical_payload(body)?)
                && i.get("payloadHash").and_then(V::as_str) == Some(&payload_hash(body)?),
            "INTENT_INTEGRITY_FAILURE",
        )?;
        check(
            i.get("idempotencyKey") == body.get("idempotencyKey")
                && body.get("tournamentId").and_then(V::as_str) == Some(tid.as_str())
                && !keys.contains(body.get("idempotencyKey").and_then(V::as_str).unwrap_or(""))
                && i.get("exchangeId") == body.get("exchangeId")
                && i.get("side") == body.get("side")
                && i.get("action") == body.get("action")
                && i.get("quantity") == body.get("quantity")
                && i.get("expirationDate") == body.get("expirationDate")
                && price_micros(i.get("price").unwrap_or(&V::Null))? == price_micros(body.get("price").unwrap_or(&V::Null))?,
            "INTENT_IDENTITY_FAILURE",
        )?;
        check(
            id(i.get("exchangeId").unwrap_or(&V::Null))
                && id(i.get("marketId").unwrap_or(&V::Null))
                && i.get("side").and_then(V::as_str) == Some("yes")
                && ["buy", "sell"].contains(&i.get("action").and_then(V::as_str).unwrap_or(""))
                && qty(i.get("quantity").unwrap_or(&V::Null))
                && i.get("idempotencyKey").and_then(V::as_str).is_some_and(|k| !k.is_empty() && k.len() <= 255)
                && stamp(i.get("expirationDate").unwrap_or(&V::Null)),
            "INVALID_INTENT_TERMS",
        )?;
        let status = i.get("status").and_then(V::as_str).unwrap_or("");
        let attempts = js_int(i.get("attempts").unwrap_or(&V::Null));
        let ordinal = js_int(i.get("ordinal").unwrap_or(&V::Null));
        check(
            STATUSES.contains(&status)
                && attempts.is_some_and(|n| n >= 0)
                && ordinal.is_some_and(|n| n >= 1 && !ordinals.contains(&n)),
            "INVALID_INTENT_STATE",
        )?;
        let retry_allowed = i.get("retryAllowed").and_then(V::as_bool);
        let outcome_not_submitted = i.get("outcome").and_then(V::as_str) == Some("not_submitted");
        check(
            retry_allowed.is_some()
                && if status == "prepared" {
                    attempts == Some(0) && i.get("orderId").unwrap_or(&V::Null).is_null()
                } else if status == "closed" && outcome_not_submitted {
                    attempts == Some(0)
                } else {
                    attempts.is_some_and(|n| n > 0)
                },
            "INVALID_DISPATCH_HISTORY",
        )?;
        let order_id = i.get("orderId").unwrap_or(&V::Null);
        check(
            order_id.is_null() || js_int(order_id).is_some_and(|n| n > 0),
            "INVALID_ORDER_ID",
        )?;
        if !order_id.is_null() {
            let oid = js_int(order_id).unwrap();
            check(!order_ids.contains(&oid), "DUPLICATE_ORDER_ATTRIBUTION")?;
            order_ids.insert(oid);
        }
        check(
            !(["acknowledged", "reconciled"].contains(&status)) || !order_id.is_null(),
            "UNATTRIBUTABLE_STATE",
        )?;
        check(
            !retry_allowed.unwrap()
                || status == "uncertain" && order_id.is_null() && stamp(i.get("retryAfterAt").unwrap_or(&V::Null)),
            "INVALID_RETRY_STATE",
        )?;
        check(
            i.get("acceptanceUncertain").and_then(V::as_bool).is_some(),
            "INVALID_ACCEPTANCE_STATE",
        )?;
        let fills_v = i.get("reconciledFills").unwrap_or(&V::Null);
        check(fills_v.is_array(), "INVALID_RECONCILED_FILLS")?;
        let mut fill_ids = BTreeSet::new();
        let mut filled = 0i64;
        for f in fills_v.as_array().unwrap() {
            let fid = js_int(f.get("id").unwrap_or(&V::Null));
            let fq = f.get("quantity").unwrap_or(&V::Null);
            check(
                plain(f)
                    && fid.is_some_and(|n| n > 0)
                    && !fill_ids.contains(&fid.unwrap())
                    && qty(fq)
                    && stamp(f.get("filledAt").unwrap_or(&V::Null)),
                "INVALID_RECONCILED_FILL",
            )?;
            let recon_at = i.get("reconciledAt").and_then(V::as_str);
            let created = i.get("orderCreatedAt").and_then(V::as_str)
                .or_else(|| i.get("preparedAt").and_then(V::as_str));
            check(
                recon_at.is_some()
                    && parse(f.get("filledAt").and_then(V::as_str).unwrap()) <= parse(recon_at.unwrap())
                    && parse(f.get("filledAt").and_then(V::as_str).unwrap()) >= created.map(parse).unwrap_or(i64::MAX),
                "INVALID_RECONCILED_FILL_TIME",
            )?;
            let p = price_micros(f.get("price").unwrap_or(&V::Null))?;
            let limit = price_micros(i.get("price").unwrap())?;
            check(
                if i.get("action").and_then(V::as_str) == Some("buy") { p <= limit } else { p >= limit },
                "FILL_OUTSIDE_LIMIT",
            )?;
            filled += fq.as_i64().unwrap();
            fill_ids.insert(fid.unwrap());
        }
        check(
            filled <= i.get("quantity").and_then(V::as_i64).unwrap_or(-1)
                && (fills_v.as_array().unwrap().is_empty() || !order_id.is_null())
                && (i.get("filledQuantity").is_none()
                    || i.get("filledQuantity").unwrap().is_null()
                    || js_int(i.get("filledQuantity").unwrap()) == Some(filled)),
            "INVALID_FILL_HISTORY",
        )?;
        keys.insert(body.get("idempotencyKey").and_then(V::as_str).unwrap().to_string());
        ordinals.insert(ordinal.unwrap());
    }
    check(ordinals.iter().all(|n| *n <= ordinals.len() as i64), "INVALID_INTENT_SEQUENCE")?;
    for (exchange_id, p) in s.get("owned").unwrap().as_object().unwrap() {
        check(
            id_str(exchange_id)
                && id(p.get("marketId").unwrap_or(&V::Null))
                && js_int(p.get("quantity").unwrap_or(&V::Null)).is_some_and(|n| n > 0)
                && p.get("lots").is_some_and(V::is_array),
            "INVALID_OWNED_POSITION",
        )?;
        let lots = p.get("lots").unwrap().as_array().unwrap();
        check(
            lots.iter().all(|l| qty(l.get("quantity").unwrap_or(&V::Null)))
                && lots.iter().map(|l| l.get("quantity").and_then(V::as_i64).unwrap()).sum::<i64>()
                    == p.get("quantity").and_then(V::as_i64).unwrap(),
            "OWNED_QUANTITY_MISMATCH",
        )?;
        let cost_sum: i128 = lots
            .iter()
            .map(|l| decimal_micros(l.get("cost").unwrap_or(&V::Null), "exact"))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .sum();
        check(cost_sum == decimal_micros(p.get("cost").unwrap_or(&V::Null), "exact")?, "OWNED_COST_MISMATCH")?;
    }
    // Ownership provenance: replay fills and compare canonical owned sets.
    let mut replay = json!({"owned": {}});
    let mut ordered: Vec<&V> = s.get("intents").unwrap().as_object().unwrap().values().collect();
    ordered.sort_by_key(|i| js_int(i.get("ordinal").unwrap_or(&V::Null)).unwrap_or(0));
    for i in ordered {
        apply_fills(
            replay.get_mut("owned").unwrap().as_object_mut().unwrap(),
            i,
            i.get("reconciledFills").unwrap().as_array().unwrap(),
        )?;
    }
    check(
        canonical_payload(&json!({"owned": replay.get("owned").unwrap().clone()}))?
            == canonical_payload(&json!({"owned": s.get("owned").unwrap().clone()}))?,
        "OWNERSHIP_PROVENANCE_MISMATCH",
    )?;
    check(
        s.get("intents").unwrap().as_object().unwrap().values().filter(|i| {
            i.get("status").and_then(V::as_str) != Some("closed")
        }).count() <= 1,
        "MULTIPLE_UNRESOLVED_INTENTS",
    )?;
    Ok(())
}
pub fn validate_execution_state(state: &V) -> Result<V, String> {
    assert_state(state)?;
    Ok(state.clone())
}
pub fn create_execution_state(tournament_id: &str, policy: Option<&V>) -> Result<V, String> {
    check(
        tournament_id.len() == 36
            && tournament_id.bytes().enumerate().all(|(i, b)| {
                if [8, 13, 18, 23].contains(&i) { b == b'-' } else { b.is_ascii_hexdigit() }
            }),
        "INVALID_TOURNAMENT",
    )?;
    Ok(json!({
        "schemaVersion": 1,
        "tournamentId": tournament_id,
        "policy": V::Object(policy_of(policy)?),
        "intents": {},
        "owned": {},
    }))
}

fn fresh(part: &V, tournament_id: &str, now: i64, max_age: i64) -> Result<(), String> {
    check(part.get("tournamentId").and_then(V::as_str) == Some(tournament_id), "SCOPE_MISMATCH")?;
    let at = part.get("at").and_then(V::as_str);
    check(
        at.is_some() && stamp(&json!(at)) && parse(at.unwrap()) <= now && now - parse(at.unwrap()) <= max_age,
        "STALE_ACCOUNT_EVIDENCE",
    )
}
struct AccountSnapshot {
    cash: i128,
    positions: BTreeMap<String, V>,
    orders: Vec<V>,
}
fn account_snapshot(s: &V, snapshot: &V, now: i64) -> Result<AccountSnapshot, String> {
    check(snapshot.get("tournamentId").and_then(V::as_str) == s.get("tournamentId").and_then(V::as_str), "SCOPE_MISMATCH")?;
    let max_age = s.get("policy").and_then(|p| p.get("maxAccountAgeMs")).and_then(V::as_i64).unwrap();
    let tid = s.get("tournamentId").and_then(V::as_str).unwrap();
    for k in ["cash", "positions", "orders"] {
        fresh(snapshot.get(k).unwrap_or(&V::Null), tid, now, max_age)?;
    }
    let cash = conservative_cash_floor(snapshot.pointer("/cash/amount").unwrap_or(&V::Null))?;
    for k in ["positions", "orders"] {
        check(
            snapshot.get(k).and_then(|p| p.get("complete")).and_then(V::as_bool) == Some(true)
                && snapshot.get(k).and_then(|p| p.get("rows")).is_some_and(V::is_array),
            "INCOMPLETE_ACCOUNT_EVIDENCE",
        )?;
    }
    let mut positions = BTreeMap::new();
    for row in snapshot.pointer("/positions/rows").unwrap().as_array().unwrap() {
        check(
            id(row.get("exchangeId").unwrap_or(&V::Null))
                && id(row.get("marketId").unwrap_or(&V::Null))
                && js_int(row.get("quantity").unwrap_or(&V::Null)).is_some()
                && !positions.contains_key(row.get("exchangeId").and_then(V::as_str).unwrap_or("")),
            "INVALID_POSITION",
        )?;
        if let Some(t) = row.get("tournamentId") {
            check(t == &json!(tid), "SCOPE_MISMATCH")?;
        }
        positions.insert(row.get("exchangeId").unwrap().as_str().unwrap().to_string(), row.clone());
    }
    let mut order_ids = BTreeSet::new();
    for row in snapshot.pointer("/orders/rows").unwrap().as_array().unwrap() {
        check(
            js_int(row.get("id").unwrap_or(&V::Null)).is_some_and(|n| n > 0)
                && !order_ids.contains(&js_int(row.get("id").unwrap()).unwrap())
                && id(row.get("exchangeId").unwrap_or(&V::Null))
                && qty(row.get("quantity").unwrap_or(&V::Null))
                && row.get("open") == Some(&V::Bool(true))
                && ["yes", "no"].contains(&row.get("side").and_then(V::as_str).unwrap_or(""))
                && ["buy", "sell"].contains(&row.get("action").and_then(V::as_str).unwrap_or("")),
            "INVALID_OPEN_ORDER",
        )?;
        if let Some(t) = row.get("tournamentId") {
            check(t == &json!(tid), "SCOPE_MISMATCH")?;
        }
        price_micros(row.get("priceLimit").unwrap_or(&V::Null))?;
        order_ids.insert(js_int(row.get("id").unwrap()).unwrap());
    }
    Ok(AccountSnapshot { cash, positions, orders: snapshot.pointer("/orders/rows").unwrap().as_array().unwrap().clone() })
}
const ATTRIBUTION_KEYS: [&str; 5] = ["runId", "strategyId", "modelHash", "signalId", "reason"];
fn check_terms(spec: &V, s: &V, now: i64) -> Result<i128, String> {
    let intent_id = spec.get("intentId").and_then(V::as_str).unwrap_or("");
    check(
        !intent_id.is_empty()
            && safe_key(intent_id)
            && intent_id.len() <= 128
            && intent_id.bytes().nth(0).is_some_and(|b| b.is_ascii_alphanumeric())
            && intent_id.bytes().all(|b| b.is_ascii_alphanumeric() || b":_.-".contains(&b)),
        "INVALID_INTENT_ID",
    )?;
    let ikey = spec.get("idempotencyKey").and_then(V::as_str).unwrap_or("");
    check(
        !ikey.is_empty() && ikey.len() <= 255 && !ikey.bytes().any(|b| b <= 0x1f),
        "INVALID_IDEMPOTENCY_KEY",
    )?;
    check(
        id(spec.get("marketId").unwrap_or(&V::Null))
            && id(spec.get("exchangeId").unwrap_or(&V::Null))
            && spec.get("side").and_then(V::as_str) == Some("yes")
            && ["buy", "sell"].contains(&spec.get("action").and_then(V::as_str).unwrap_or(""))
            && qty(spec.get("quantity").unwrap_or(&V::Null)),
        "INVALID_ORDER_TERMS",
    )?;
    if let Some(t) = spec.get("tournamentId") {
        check(t == s.get("tournamentId").unwrap_or(&V::Null), "SCOPE_MISMATCH")?;
    }
    let exp = spec.get("expirationDate").and_then(V::as_str);
    check(
        exp.is_some()
            && stamp(&json!(exp))
            && parse(exp.unwrap()) > now
            && parse(exp.unwrap()) - now
                <= s.get("policy").and_then(|p| p.get("maxExpirationMs")).and_then(V::as_i64).unwrap(),
        "INVALID_EXPIRATION",
    )?;
    let attribution = spec.get("attribution").unwrap_or(&V::Null);
    check(
        attribution.get("runId").and_then(V::as_str).is_some_and(|x| !x.is_empty())
            && attribution.get("strategyId").and_then(V::as_str).is_some_and(|x| !x.is_empty()),
        "ATTRIBUTION_REQUIRED",
    )?;
    check(
        attribution.as_object().is_some_and(|m| {
            m.keys().all(|k| ATTRIBUTION_KEYS.contains(&k.as_str()))
                && m.values().all(|v| v.as_str().is_some_and(|s| s.len() <= 1000))
        }),
        "INVALID_ATTRIBUTION",
    )?;
    price_micros(spec.get("price").unwrap_or(&V::Null))
}
fn preflight(s: &V, spec: &V, snapshot: &V, now: i64, ignore_intent: Option<&str>) -> Result<V, String> {
    let price = check_terms(spec, s, now)?;
    let snap = account_snapshot(s, snapshot, now)?;
    check(
        !s.get("intents").unwrap().as_object().unwrap().values().any(|i| {
            i.get("intentId").and_then(V::as_str) != ignore_intent
                && i.get("status").and_then(V::as_str) != Some("closed")
        }),
        "UNRESOLVED_INTENT",
    )?;
    for (exchange_id, own) in s.get("owned").unwrap().as_object().unwrap() {
        let p = snap.positions.get(exchange_id);
        check(
            p.and_then(|x| x.get("quantity")).and_then(V::as_i64) == own.get("quantity").and_then(V::as_i64)
                && p.and_then(|x| x.get("marketId")) == own.get("marketId"),
            "MANAGED_POSITION_RECONCILIATION_REQUIRED",
        )?;
    }
    let exchange_id = spec.get("exchangeId").and_then(V::as_str).unwrap();
    let owned = s.get("owned").and_then(|o| o.get(exchange_id));
    let actual = snap.positions.get(exchange_id);
    check(
        actual.is_none() || actual.unwrap().get("marketId") == spec.get("marketId"),
        "MARKET_IDENTITY_MISMATCH",
    )?;
    check(
        actual.and_then(|x| x.get("quantity")).and_then(V::as_i64).unwrap_or(0) >= 0,
        "OPPOSITE_SIDE_POSITION",
    )?;
    check(
        actual.and_then(|x| x.get("quantity")).and_then(V::as_i64).unwrap_or(0)
            == owned.and_then(|o| o.get("quantity")).and_then(V::as_i64).unwrap_or(0),
        "UNATTRIBUTED_POSITION",
    )?;
    check(
        owned.is_none() || owned.unwrap().get("marketId") == spec.get("marketId"),
        "MARKET_IDENTITY_MISMATCH",
    )?;
    check(
        !snap.orders.iter().any(|o| o.get("exchangeId").and_then(V::as_str) == Some(exchange_id)),
        "EXISTING_EXCHANGE_ORDER",
    )?;
    let quantity = js_int(spec.get("quantity").unwrap()).unwrap() as i128;
    let cost = price * quantity;
    check(cost <= decimal_micros(s.get("policy").and_then(|p| p.get("maxOrderCost")).unwrap_or(&V::Null), "exact")?, "ORDER_COST_CAP")?;
    let aggregate: i128 = s
        .get("owned")
        .unwrap()
        .as_object()
        .unwrap()
        .values()
        .map(|p| decimal_micros(p.get("cost").unwrap_or(&V::Null), "exact").unwrap())
        .sum();
    let action = spec.get("action").and_then(V::as_str).unwrap();
    if action == "buy" {
        let reserved: i128 = snap
            .orders
            .iter()
            .filter(|o| o.get("action").and_then(V::as_str) == Some("buy"))
            .map(|o| {
                price_micros(o.get("priceLimit").unwrap_or(&V::Null)).unwrap()
                    * js_int(o.get("quantity").unwrap_or(&V::Null)).unwrap() as i128
            })
            .sum();
        check(
            snap.cash - reserved - cost
                >= decimal_micros(s.get("policy").and_then(|p| p.get("cashReserve")).unwrap_or(&V::Null), "exact")?,
            "INSUFFICIENT_UNRESERVED_CASH",
        )?;
        check(
            aggregate + cost
                <= decimal_micros(s.get("policy").and_then(|p| p.get("maxAggregateCost")).unwrap_or(&V::Null), "exact")?,
            "AGGREGATE_COST_CAP",
        )?;
        let cumulative: i128 = s
            .get("intents")
            .unwrap()
            .as_object()
            .unwrap()
            .values()
            .filter(|i| i.get("action").and_then(V::as_str) == Some("buy"))
            .map(|i| {
                i.get("reconciledFills")
                    .and_then(V::as_array)
                    .map(|a| {
                        a.iter()
                            .map(|f| {
                                price_micros(f.get("price").unwrap_or(&V::Null)).unwrap()
                                    * js_int(f.get("quantity").unwrap_or(&V::Null)).unwrap() as i128
                            })
                            .sum::<i128>()
                    })
                    .unwrap_or(0)
            })
            .sum();
        check(
            cumulative + cost
                <= decimal_micros(s.get("policy").and_then(|p| p.get("maxCumulativeBuyCost")).unwrap_or(&V::Null), "exact")?,
            "CUMULATIVE_BUY_COST_CAP",
        )?;
        let mut markets: BTreeSet<&str> = s
            .get("owned")
            .unwrap()
            .as_object()
            .unwrap()
            .values()
            .filter_map(|p| p.get("marketId").and_then(V::as_str))
            .collect();
        markets.insert(spec.get("marketId").and_then(V::as_str).unwrap_or(""));
        check(
            markets.len() as i64 <= s.get("policy").and_then(|p| p.get("maxManagedMarkets")).and_then(V::as_i64).unwrap(),
            "MANAGED_MARKET_CAP",
        )?;
    } else {
        check(
            owned.is_some() && quantity <= owned.unwrap().get("quantity").and_then(V::as_i64).unwrap_or(0) as i128,
            "OWNED_INVENTORY_REQUIRED",
        )?;
    }
    let b = snapshot.get("book").unwrap_or(&V::Null);
    check(
        b.get("tournamentId") == s.get("tournamentId")
            && b.get("marketId") == spec.get("marketId")
            && b.get("exchangeId") == spec.get("exchangeId"),
        "BOOK_SCOPE_MISMATCH",
    )?;
    let bat = b.get("at").and_then(V::as_str);
    let breq = b.get("requestStartedAt").and_then(V::as_str);
    check(
        bat.is_some()
            && stamp(&json!(bat))
            && breq.is_some()
            && stamp(&json!(breq))
            && parse(bat.unwrap()) <= now
            && now - parse(bat.unwrap()) <= s.get("policy").and_then(|p| p.get("maxBookAgeMs")).and_then(V::as_i64).unwrap()
            && parse(bat.unwrap()) >= parse(breq.unwrap())
            && parse(bat.unwrap()) - parse(breq.unwrap())
                <= s.get("policy").and_then(|p| p.get("maxBookLatencyMs")).and_then(V::as_i64).unwrap(),
        "STALE_BOOK_EVIDENCE",
    )?;
    let levels = |rows: &V, descending: bool| -> Result<Vec<(i128, i64)>, String> {
        let arr = rows.as_array().ok_or("MISSING_BOOK_DEPTH")?;
        check(!arr.is_empty(), "MISSING_BOOK_DEPTH")?;
        let mut out = Vec::new();
        let mut previous: Option<i128> = None;
        for r in arr {
            let p = price_micros(r.get("price").unwrap_or(&V::Null))?;
            check(
                qty(r.get("quantity").unwrap_or(&V::Null))
                    && previous.map_or(true, |prev| if descending { p < prev } else { p > prev }),
                "INVALID_BOOK_DEPTH",
            )?;
            previous = Some(p);
            out.push((p, js_int(r.get("quantity").unwrap()).unwrap()));
        }
        Ok(out)
    };
    let bids = levels(b.get("bids").unwrap_or(&V::Null), true)?;
    let asks = levels(b.get("asks").unwrap_or(&V::Null), false)?;
    check(bids[0].0 < asks[0].0, "LOCKED_OR_CROSSED_BOOK")?;
    let depth: i128 = (if action == "buy" { asks } else { bids })
        .iter()
        .filter(|(p, _)| if action == "buy" { *p <= price } else { *p >= price })
        .map(|(_, q)| *q as i128)
        .sum();
    check(depth >= quantity, "INSUFFICIENT_EXECUTABLE_DEPTH")?;
    Ok(json!({
        "maxNotional": format_micros(cost),
        "observedCashFloor": format_micros(snap.cash),
        "aggregateCostAfter": format_micros(aggregate + if action == "buy" { cost } else { 0 }),
        "checkedAt": iso(now),
    }))
}

fn result(state: V, intent_id: Option<&str>, code: &str, extra: Map<String, V>) -> Result<V, String> {
    assert_state(&state)?;
    let mut out = Map::from_iter([("ok".into(), V::Bool(true)), ("code".into(), json!(code)), ("state".into(), state.clone())]);
    if let Some(id) = intent_id {
        if let Some(i) = state.pointer(&format!("/intents/{id}")) {
            out.insert("intent".into(), i.clone());
        }
    }
    out.extend(extra);
    Ok(V::Object(out))
}
fn denied(state: &V, code: &str) -> V {
    json!({"ok": false, "code": code, "state": state.clone()})
}
fn request_for(i: &V) -> V {
    json!({
        "method": "POST",
        "path": "/orders",
        "body": i.get("body").unwrap().clone(),
        "bodyJson": i.get("bodyJson").unwrap().clone(),
        "payloadHash": i.get("payloadHash").unwrap().clone(),
    })
}
pub fn revalidate_intent_evidence(state: &V, intent_id: &str, snapshot: &V, now: &V) -> V {
    match (|| -> Result<V, String> {
        assert_state(state)?;
        let now = clock(now)?;
        let i = state.pointer(&format!("/intents/{intent_id}")).ok_or("UNKNOWN_ACTIVE_INTENT")?;
        check(i.get("status").and_then(V::as_str) != Some("closed"), "UNKNOWN_ACTIVE_INTENT")?;
        let risk = preflight(state, i, snapshot, now, Some(intent_id))?;
        let mut extra = Map::new();
        extra.insert("risk".into(), risk);
        result(state.clone(), Some(intent_id), "EVIDENCE_REVALIDATED", extra)
    })() {
        Ok(r) => r,
        Err(e) => denied(state, &e),
    }
}
pub fn prepare_intent(state: &V, spec: &V, snapshot: &V, now: &V) -> V {
    match (|| -> Result<V, String> {
        assert_state(state)?;
        let now = clock(now)?;
        let intent_id = spec.get("intentId").and_then(V::as_str).unwrap_or("");
        check(!state.pointer(&format!("/intents/{intent_id}")).is_some(), "INTENT_ID_ALREADY_USED")?;
        check(
            !state.get("intents").unwrap().as_object().unwrap().values().any(|i| {
                i.get("idempotencyKey") == spec.get("idempotencyKey")
            }),
            "IDEMPOTENCY_KEY_ALREADY_USED",
        )?;
        let risk = preflight(state, spec, snapshot, now, None)?;
        let price = price_micros(spec.get("price").unwrap_or(&V::Null))?;
        let body = json!({
            "idempotencyKey": spec.get("idempotencyKey").unwrap().clone(),
            "exchangeId": spec.get("exchangeId").unwrap().clone(),
            "side": "yes",
            "action": spec.get("action").unwrap().clone(),
            "quantity": spec.get("quantity").unwrap().clone(),
            "price": price as f64 / SCALE as f64,
            "expirationDate": spec.get("expirationDate").unwrap().clone(),
            "tournamentId": state.get("tournamentId").unwrap().clone(),
        });
        let i = json!({
            "intentId": spec.get("intentId").unwrap().clone(),
            "idempotencyKey": spec.get("idempotencyKey").unwrap().clone(),
            "marketId": spec.get("marketId").unwrap().clone(),
            "exchangeId": spec.get("exchangeId").unwrap().clone(),
            "side": spec.get("side").unwrap().clone(),
            "action": spec.get("action").unwrap().clone(),
            "quantity": spec.get("quantity").unwrap().clone(),
            "price": format_micros(price),
            "expirationDate": spec.get("expirationDate").unwrap().clone(),
            "attribution": spec.get("attribution").unwrap().clone(),
            "method": "POST",
            "path": "/orders",
            "ordinal": state.get("intents").unwrap().as_object().unwrap().len() as i64 + 1,
            "body": body,
            "bodyJson": canonical_payload(&body)?,
            "payloadHash": payload_hash(&body)?,
            "risk": risk,
            "status": "prepared",
            "preparedAt": iso(now),
            "attempts": 0,
            "orderId": null,
            "retryAllowed": false,
            "acceptanceUncertain": false,
            "reconciledFills": [],
        });
        let mut s = state.clone();
        s.get_mut("intents")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(intent_id.to_string(), i.clone());
        let mut extra = Map::new();
        extra.insert("request".into(), request_for(&i));
        result(s, Some(intent_id), "PREPARED", extra)
    })() {
        Ok(r) => r,
        Err(e) => denied(state, &e),
    }
}
pub fn begin_dispatch(state: &V, intent_id: &str, snapshot: &V, now: &V, retry: bool) -> V {
    match (|| -> Result<V, String> {
        assert_state(state)?;
        let now = clock(now)?;
        let prior = state.pointer(&format!("/intents/{intent_id}")).cloned();
        check(prior.is_some(), "UNKNOWN_INTENT")?;
        let prior = prior.unwrap();
        if prior.get("status").and_then(V::as_str) == Some("prepared") {
            check(!retry, "RETRY_BEFORE_FIRST_DISPATCH")?;
            preflight(state, &prior, snapshot, now, Some(intent_id))?;
        } else {
            check(
                retry
                    && prior.get("status").and_then(V::as_str) == Some("uncertain")
                    && prior.get("retryAllowed") == Some(&V::Bool(true)),
                "DISPATCH_NOT_ALLOWED",
            )?;
            check(
                now >= parse(prior.get("retryAfterAt").and_then(V::as_str).unwrap_or("")),
                "RETRY_TOO_EARLY",
            )?;
            if now < parse(prior.get("expirationDate").and_then(V::as_str).unwrap_or("")) {
                preflight(state, &prior, snapshot, now, Some(intent_id))?;
            }
        }
        let mut s = state.clone();
        let i = s.get_mut("intents").unwrap().as_object_mut().unwrap().get_mut(intent_id).unwrap();
        i.as_object_mut().unwrap().insert("status".into(), json!("dispatching"));
        let next_attempt = i.get("attempts").and_then(V::as_i64).unwrap() + 1;
        i.as_object_mut().unwrap().insert("attempts".into(), json!(next_attempt));
        i.as_object_mut().unwrap().insert("dispatchedAt".into(), json!(iso(now)));
        i.as_object_mut().unwrap().insert("retryAllowed".into(), json!(false));
        let mut extra = Map::new();
        extra.insert("request".into(), request_for(i));
        result(s, Some(intent_id), "DISPATCHING", extra)
    })() {
        Ok(r) => r,
        Err(e) => denied(state, &e),
    }
}
fn uncertain(state: &V, intent_id: &str, code: &str, now: i64, retry_allowed: bool, delay: i64, could_have_accepted: bool) -> V {
    let mut s = state.clone();
    let i = s.get_mut("intents").unwrap().as_object_mut().unwrap().get_mut(intent_id).unwrap();
    let obj = i.as_object_mut().unwrap();
    obj.insert("status".into(), json!("uncertain"));
    obj.insert("reasonCode".into(), json!(code));
    obj.insert("retryAllowed".into(), json!(retry_allowed));
    obj.insert("retryAfterAt".into(), json!(iso(now + delay)));
    if could_have_accepted {
        obj.insert("acceptanceUncertain".into(), json!(true));
    }
    let mut extra = Map::new();
    let intent = s.pointer(&format!("/intents/{intent_id}")).unwrap().clone();
    extra.insert("intent".into(), intent);
    let mut out = match result(s, Some(intent_id), code, extra) {
        Ok(r) => r,
        Err(e) => {
            return json!({"ok": false, "code": e, "state": state.clone()});
        }
    };
    out.as_object_mut().unwrap().insert("ok".into(), V::Bool(false));
    out
}
pub fn record_placement(state: &V, intent_id: &str, outcome: &V, now: &V) -> V {
    match (|| -> Result<V, String> {
        assert_state(state)?;
        let now = clock(now)?;
        let prior = state.pointer(&format!("/intents/{intent_id}")).cloned();
        check(
            prior.as_ref().and_then(|p| p.get("status")).and_then(V::as_str) == Some("dispatching"),
            "PLACEMENT_STATE_MISMATCH",
        )?;
        let prior = prior.unwrap();
        let status = outcome.get("status").and_then(V::as_i64);
        let code = outcome.get("code").and_then(V::as_str).unwrap_or("TRANSPORT_AMBIGUOUS").to_string();
        let r = outcome.get("response").unwrap_or(&V::Null);
        if status != Some(200) {
            if status == Some(409) && code != "REQUEST_IN_FLIGHT" {
                return Ok(uncertain(state, intent_id, &code, now, false, 0, true));
            }
            if status.is_none()
                || [429, 500, 502, 503, 504].contains(&status.unwrap_or(-1))
                || (status == Some(409) && code == "REQUEST_IN_FLIGHT")
            {
                let supplied = outcome.get("retryAfterMs").unwrap_or(&V::Null);
                let supplied_ms = js_int(supplied);
                check(
                    supplied_ms.is_some_and(|n| (0..=86_400_000).contains(&n)) || supplied.is_null(),
                    "MANUAL_RETRY_REVIEW_REQUIRED",
                )?;
                let supplied_ms = supplied_ms.unwrap_or(0);
                let floor = if status == Some(409) { 90_000 } else if status == Some(429) { 60_000 } else { 1000 };
                return Ok(uncertain(
                    state,
                    intent_id,
                    &code,
                    now,
                    true,
                    supplied_ms.max(floor),
                    status != Some(429),
                ));
            }
            if status.is_some_and(|x| (400..500).contains(&x)) {
                if prior.get("acceptanceUncertain") == Some(&V::Bool(true)) {
                    return Ok(uncertain(state, intent_id, "RETRY_REJECTED_AFTER_AMBIGUITY", now, false, 0, true));
                }
                let mut s = state.clone();
                let i = s.get_mut("intents").unwrap().as_object_mut().unwrap().get_mut(intent_id).unwrap();
                let o = i.as_object_mut().unwrap();
                o.insert("status".into(), json!("closed"));
                o.insert("outcome".into(), json!("rejected"));
                o.insert("reasonCode".into(), json!(code));
                o.insert("closedAt".into(), json!(iso(now)));
                return result(s, Some(intent_id), "REJECTED", Map::new());
            }
            return Ok(uncertain(state, intent_id, "UNEXPECTED_PLACEMENT_STATUS", now, false, 0, true));
        }
        let order_id = js_int(r.get("orderId").unwrap_or(&V::Null));
        if !order_id.is_some_and(|n| n > 0) {
            return Ok(uncertain(state, intent_id, "UNATTRIBUTABLE_ORDER_ID", now, false, 0, true));
        }
        let mut s = state.clone();
        s.get_mut("intents").unwrap().as_object_mut().unwrap()
            .get_mut(intent_id).unwrap().as_object_mut().unwrap()
            .insert("orderId".into(), json!(order_id.unwrap()));
        let semantic = (|| -> Result<(), String> {
            let body = prior.get("body").unwrap();
            check(
                r.get("exchangeId") == body.get("exchangeId")
                    && (r.get("tournamentId").is_none()
                        || r.get("tournamentId") == s.get("tournamentId"))
                    && r.get("side").and_then(V::as_str) == Some("yes")
                    && r.get("action") == body.get("action")
                    && r.get("quantity") == body.get("quantity")
                    && price_micros(r.get("price").unwrap_or(&V::Null))?
                        == price_micros(body.get("price").unwrap_or(&V::Null))?,
                "CANONICAL_ORDER_MISMATCH",
            )?;
            check(
                r.get("open").and_then(V::as_bool).is_some()
                    && js_int(r.get("quantityTraded").unwrap_or(&V::Null)).is_some_and(|n| n >= 0)
                    && js_int(r.get("remainingQuantity").unwrap_or(&V::Null)).is_some_and(|n| n >= 0)
                    && js_int(r.get("quantityTraded").unwrap()).unwrap()
                        + js_int(r.get("remainingQuantity").unwrap()).unwrap()
                        <= body.get("quantity").and_then(V::as_i64).unwrap(),
                "INVALID_PLACEMENT_QUANTITIES",
            )?;
            check(
                r.get("open") == Some(&V::Bool(false))
                    || js_int(r.get("remainingQuantity").unwrap()).unwrap() > 0,
                "INVALID_RESTING_ORDER",
            )?;
            check(
                r.get("totalCost").is_some()
                    && !r.get("totalCost").unwrap().is_null()
                    && conservative_cash_floor(r.get("totalCost").unwrap())? >= 0,
                "INVALID_PLACEMENT_COST",
            )?;
            Ok(())
        })();
        if let Err(e) = semantic {
            return Ok(uncertain(&s, intent_id, &e, now, false, 0, true));
        }
        let i = s.get_mut("intents").unwrap().as_object_mut().unwrap().get_mut(intent_id).unwrap();
        let o = i.as_object_mut().unwrap();
        o.insert("status".into(), json!("acknowledged"));
        o.insert("acknowledgedAt".into(), json!(iso(now)));
        o.insert(
            "responseSummary".into(),
            json!({
                "orderId": order_id.unwrap(),
                "open": r.get("open").unwrap().clone(),
                "quantityTraded": r.get("quantityTraded").unwrap().clone(),
                "remainingQuantity": r.get("remainingQuantity").unwrap().clone(),
                "terminalReasonCode": r.get("terminalReasonCode").unwrap_or(&V::Null).clone(),
            }),
        );
        result(s, Some(intent_id), "ACKNOWLEDGED", Map::new())
    })() {
        Ok(r) => r,
        Err(e) => {
            if state.pointer(&format!("/intents/{intent_id}/status")).and_then(V::as_str) == Some("dispatching") {
                let n = clock(now).unwrap_or(0);
                uncertain(state, intent_id, &e, n, false, 0, true)
            } else {
                denied(state, &e)
            }
        }
    }
}

fn apply_fills(owned: &mut Map<String, V>, i: &V, fills: &[V]) -> Result<(), String> {
    let exchange_id = i.get("exchangeId").and_then(V::as_str).unwrap_or("").to_string();
    let action = i.get("action").and_then(V::as_str).unwrap_or("");
    for f in fills {
        let cost = price_micros(f.get("price").unwrap_or(&V::Null))? * js_int(f.get("quantity").unwrap_or(&V::Null)).unwrap() as i128;
        if action == "buy" {
            if !owned.contains_key(&exchange_id) {
                owned.insert(
                    exchange_id.clone(),
                    json!({"marketId": i.get("marketId").unwrap().clone(), "quantity": 0, "cost": "0.000000", "lots": []}),
                );
            }
            let o = owned.get_mut(&exchange_id).unwrap();
            let q = o.get_mut("quantity").unwrap().as_i64().unwrap() + js_int(f.get("quantity").unwrap()).unwrap();
            o.as_object_mut().unwrap().insert("quantity".into(), json!(q));
            let new_cost = decimal_micros(o.get("cost").unwrap(), "exact")? + cost;
            o.as_object_mut().unwrap().insert("cost".into(), json!(format_micros(new_cost)));
            o.get_mut("lots").unwrap().as_array_mut().unwrap().push(json!({
                "quantity": js_int(f.get("quantity").unwrap()).unwrap(),
                "cost": format_micros(cost),
            }));
        } else {
            check(
                owned.get(&exchange_id).and_then(|o| o.get("quantity")).and_then(V::as_i64).unwrap_or(0)
                    >= js_int(f.get("quantity").unwrap()).unwrap(),
                "RECONCILIATION_OVERSELL",
            )?;
            let o = owned.get_mut(&exchange_id).unwrap();
            let mut left = js_int(f.get("quantity").unwrap()).unwrap();
            let mut used: i128 = 0;
            while left > 0 {
                let lots = o.get_mut("lots").unwrap().as_array_mut().unwrap();
                let lot = lots.first_mut().ok_or("RECONCILIATION_OVERSELL")?;
                let lot_q = js_int(lot.get("quantity").unwrap()).unwrap();
                let n = left.min(lot_q);
                let part = if n == lot_q {
                    decimal_micros(lot.get("cost").unwrap(), "exact")?
                } else {
                    decimal_micros(lot.get("cost").unwrap(), "exact")? * n as i128 / lot_q as i128
                };
                used += part;
                let remaining_q = lot_q - n;
                lot.as_object_mut().unwrap().insert("quantity".into(), json!(remaining_q));
                let lot_cost = decimal_micros(lot.get("cost").unwrap(), "exact")? - part;
                lot.as_object_mut().unwrap().insert("cost".into(), json!(format_micros(lot_cost)));
                left -= n;
                if remaining_q == 0 {
                    lots.remove(0);
                }
            }
            let o = owned.get_mut(&exchange_id).unwrap();
            let q = o.get("quantity").and_then(V::as_i64).unwrap() - js_int(f.get("quantity").unwrap()).unwrap();
            o.as_object_mut().unwrap().insert("quantity".into(), json!(q));
            let new_cost = decimal_micros(o.get("cost").unwrap(), "exact")? - used;
            o.as_object_mut().unwrap().insert("cost".into(), json!(format_micros(new_cost)));
        }
    }
    if owned.get(&exchange_id).and_then(|o| o.get("quantity")).and_then(V::as_i64).unwrap_or(0) == 0 {
        owned.remove(&exchange_id);
    }
    Ok(())
}
pub fn reconcile_intent(state: &V, intent_id: &str, evidence: &V, now: &V) -> V {
    match (|| -> Result<V, String> {
        assert_state(state)?;
        let now = clock(now)?;
        let prior = state.pointer(&format!("/intents/{intent_id}")).cloned();
        check(
            prior.as_ref().is_some_and(|p| {
                ["acknowledged", "reconciled", "uncertain"]
                    .contains(&p.get("status").and_then(V::as_str).unwrap_or(""))
                    && js_int(p.get("orderId").unwrap_or(&V::Null)).is_some_and(|n| n > 0)
            }),
            "RECONCILIATION_NOT_ATTRIBUTABLE",
        )?;
        let prior = prior.unwrap();
        let snap = account_snapshot(state, evidence.get("snapshot").unwrap_or(&V::Null), now)?;
        let o = evidence.get("order").unwrap_or(&V::Null);
        let f = evidence.get("fills").unwrap_or(&V::Null);
        let tid = state.get("tournamentId").and_then(V::as_str).unwrap();
        check(
            o.get("id").and_then(V::as_i64) == prior.get("orderId").and_then(V::as_i64)
                && o.get("exchangeId") == prior.get("exchangeId")
                && o.get("tournamentId").and_then(V::as_str) == Some(tid)
                && o.get("side").and_then(V::as_str) == Some("yes")
                && o.get("action") == prior.get("action")
                && price_micros(o.get("priceLimit").unwrap_or(&V::Null))?
                    == price_micros(prior.get("price").unwrap_or(&V::Null))?
                && stamp(o.get("expirationDate").unwrap_or(&V::Null))
                && parse(o.get("expirationDate").and_then(V::as_str).unwrap())
                    == parse(prior.get("expirationDate").and_then(V::as_str).unwrap())
                && o.get("open").and_then(V::as_bool).is_some(),
            "ORDER_RECONCILIATION_MISMATCH",
        )?;
        let eat = evidence.get("at").and_then(V::as_str);
        check(
            eat.is_some()
                && stamp(&json!(eat))
                && parse(eat.unwrap()) <= now
                && now - parse(eat.unwrap())
                    <= state.get("policy").and_then(|p| p.get("maxAccountAgeMs")).and_then(V::as_i64).unwrap(),
            "STALE_ORDER_EVIDENCE",
        )?;
        check(
            f.get("complete") == Some(&V::Bool(true))
                && f.get("orderId").and_then(V::as_i64) == prior.get("orderId").and_then(V::as_i64)
                && f.get("exchangeId") == prior.get("exchangeId")
                && f.get("tournamentId").and_then(V::as_str) == Some(tid)
                && f.get("rows").is_some_and(V::is_array),
            "INCOMPLETE_FILL_EVIDENCE",
        )?;
        // undefined defaults to now; any other non-string parses as NaN.
        let fill_read_at = match evidence.get("fillsReceivedAt") {
            None => Some(now),
            Some(V::String(s)) => chrono::DateTime::parse_from_rfc3339(s).ok().map(|d| d.timestamp_millis()),
            Some(_) => None,
        };
        check(fill_read_at.is_some() && fill_read_at.unwrap() <= now, "INVALID_FILL_READ_TIME")?;
        let fill_read_at = fill_read_at.unwrap();
        // createdAt defaults to preparedAt only when the key is absent.
        let created_at = match o.get("createdAt") {
            None => prior.get("preparedAt").and_then(V::as_str).unwrap_or("").to_string(),
            Some(V::String(s)) => s.clone(),
            Some(_) => return fail("INVALID_ORDER_CREATION_TIME"),
        };
        check(
            stamp(&json!(created_at))
                && parse(&created_at) <= fill_read_at
                && (prior.get("orderCreatedAt").is_none()
                    || prior.get("orderCreatedAt").unwrap().is_null()
                    || parse(prior.get("orderCreatedAt").and_then(V::as_str).unwrap()) == parse(&created_at)),
            "INVALID_ORDER_CREATION_TIME",
        )?;
        let mut seen = BTreeSet::new();
        let mut fills = Vec::new();
        for row in f.get("rows").unwrap().as_array().unwrap() {
            let fid = js_int(row.get("id").unwrap_or(&V::Null));
            check(
                fid.is_some_and(|n| n > 0)
                    && !seen.contains(&fid.unwrap())
                    && row.get("side").and_then(V::as_str) == Some("yes")
                    && qty(row.get("quantity").unwrap_or(&V::Null))
                    && stamp(row.get("filledAt").unwrap_or(&V::Null)),
                "INVALID_FILL",
            )?;
            seen.insert(fid.unwrap());
            let fat = parse(row.get("filledAt").and_then(V::as_str).unwrap());
            check(fat <= fill_read_at && fat >= parse(&created_at), "INVALID_FILL_TIME")?;
            let p = price_micros(row.get("price").unwrap_or(&V::Null))?;
            let limit = price_micros(prior.get("price").unwrap())?;
            check(
                if prior.get("action").and_then(V::as_str) == Some("buy") { p <= limit } else { p >= limit },
                "FILL_OUTSIDE_LIMIT",
            )?;
            fills.push(json!({
                "id": fid.unwrap(),
                "quantity": row.get("quantity").unwrap().clone(),
                "price": format_micros(p),
                "filledAt": row.get("filledAt").unwrap().clone(),
            }));
        }
        fills.sort_by(|a, b| {
            parse(a.get("filledAt").and_then(V::as_str).unwrap())
                .cmp(&parse(b.get("filledAt").and_then(V::as_str).unwrap()))
                .then(js_int(a.get("id").unwrap()).unwrap().cmp(&js_int(b.get("id").unwrap()).unwrap()))
        });
        let total: i64 = fills.iter().map(|f| f.get("quantity").and_then(V::as_i64).unwrap()).sum();
        check(
            total <= prior.get("quantity").and_then(V::as_i64).unwrap()
                && f.get("totalQuantityFilled").and_then(V::as_i64) == Some(total),
            "FILL_TOTAL_MISMATCH",
        )?;
        check(
            total >= prior.pointer("/responseSummary/quantityTraded").and_then(V::as_i64).unwrap_or(0),
            "FILLS_BEHIND_PLACEMENT_RESPONSE",
        )?;
        for old in prior.get("reconciledFills").unwrap().as_array().unwrap() {
            check(
                fills.iter().any(|x| canonical_payload(x).ok() == canonical_payload(old).ok()),
                "FILL_HISTORY_CHANGED",
            )?;
        }
        let expected_remainder = prior.get("quantity").and_then(V::as_i64).unwrap() - total;
        let oq = js_int(o.get("quantity").unwrap_or(&V::Null));
        let oopen = o.get("open").and_then(V::as_bool).unwrap();
        check(
            oq.is_some_and(|n| n >= 0)
                && if oopen {
                    oq == Some(expected_remainder) && oq.unwrap() > 0
                } else {
                    oq.unwrap() >= total && oq.unwrap() <= prior.get("quantity").and_then(V::as_i64).unwrap()
                },
            "ORDER_QUANTITY_MISMATCH",
        )?;
        let exchange_id = prior.get("exchangeId").and_then(V::as_str).unwrap();
        let target_orders: Vec<&V> = snap
            .orders
            .iter()
            .filter(|x| x.get("exchangeId").and_then(V::as_str) == Some(exchange_id))
            .collect();
        check(
            if oopen {
                target_orders.len() == 1
                    && target_orders[0].get("id") == o.get("id")
                    && target_orders[0].get("quantity") == o.get("quantity")
                    && target_orders[0].get("side").and_then(V::as_str) == Some("yes")
                    && target_orders[0].get("action") == prior.get("action")
                    && price_micros(target_orders[0].get("priceLimit").unwrap_or(&V::Null)).ok()
                        == price_micros(prior.get("price").unwrap_or(&V::Null)).ok()
            } else {
                target_orders.is_empty()
            },
            "OPEN_ORDER_RECONCILIATION_MISMATCH",
        )?;
        let mut s = state.clone();
        let old_ids: BTreeSet<i64> = prior
            .get("reconciledFills")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|x| js_int(x.get("id").unwrap()).unwrap())
            .collect();
        {
            let owned_map = s.get_mut("owned").unwrap().as_object_mut().unwrap();
            let new_fills: Vec<V> = fills
                .iter()
                .filter(|x| !old_ids.contains(&js_int(x.get("id").unwrap()).unwrap()))
                .cloned()
                .collect();
            apply_fills(owned_map, &prior, &new_fills)?;
        }
        check(
            snap.positions
                .get(exchange_id)
                .and_then(|p| p.get("quantity"))
                .and_then(V::as_i64)
                .unwrap_or(0)
                == s.get("owned").and_then(|o| o.get(exchange_id)).and_then(|p| p.get("quantity")).and_then(V::as_i64).unwrap_or(0),
            "POSITION_RECONCILIATION_MISMATCH",
        )?;
        check(
            !snap.positions.contains_key(exchange_id)
                || snap.positions.get(exchange_id).unwrap().get("marketId") == prior.get("marketId"),
            "MARKET_IDENTITY_MISMATCH",
        )?;
        let i = s.get_mut("intents").unwrap().as_object_mut().unwrap().get_mut(intent_id).unwrap();
        let obj = i.as_object_mut().unwrap();
        obj.insert("reconciledFills".into(), json!(fills));
        obj.insert("reconciledAt".into(), json!(iso(now)));
        obj.insert("orderCreatedAt".into(), json!(created_at));
        obj.insert("retryAllowed".into(), json!(false));
        obj.insert("status".into(), json!(if oopen { "reconciled" } else { "closed" }));
        obj.insert("filledQuantity".into(), json!(total));
        if !oopen {
            obj.insert("closedAt".into(), json!(iso(now)));
            obj.insert(
                "outcome".into(),
                json!(if total == prior.get("quantity").and_then(V::as_i64).unwrap() {
                    "filled"
                } else if total > 0 {
                    "partially_filled"
                } else {
                    "unfilled"
                }),
            );
        }
        result(s, Some(intent_id), if oopen { "RECONCILED_OPEN" } else { "CLOSED" }, Map::new())
    })() {
        Ok(r) => r,
        Err(e) => denied(state, &e),
    }
}
pub fn abandon_prepared_intent(state: &V, intent_id: &str, reason: &str, now: &V) -> V {
    match (|| -> Result<V, String> {
        assert_state(state)?;
        check(
            state.pointer(&format!("/intents/{intent_id}/status")).and_then(V::as_str) == Some("prepared"),
            "CANNOT_ABANDON_DISPATCHED_INTENT",
        )?;
        check(!reason.is_empty() && reason.len() <= 120, "INVALID_REASON")?;
        let now = clock(now)?;
        let mut s = state.clone();
        let i = s.get_mut("intents").unwrap().as_object_mut().unwrap().get_mut(intent_id).unwrap();
        let o = i.as_object_mut().unwrap();
        o.insert("status".into(), json!("closed"));
        o.insert("outcome".into(), json!("not_submitted"));
        o.insert("reasonCode".into(), json!(reason));
        o.insert("closedAt".into(), json!(iso(now)));
        result(s, Some(intent_id), "ABANDONED", Map::new())
    })() {
        Ok(r) => r,
        Err(e) => denied(state, &e),
    }
}
