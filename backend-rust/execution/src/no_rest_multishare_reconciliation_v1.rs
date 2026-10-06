// execution/no-rest-multishare-reconciliation-v1.mjs — pure evidence
// reduction, not allocation, write authority, or a replacement for the
// one-share live verifiers. REST prices are side-relative and NO quantities
// are negative even for SELL (official public schema observed 2026-10-06).
//
// Port notes:
// - JS BigInt money -> i128; arbitrary-precision average lexemes -> BigInt.
// - `instant`/`amount`/`price` errors propagate with their own messages (not
//   REST_NO_MULTI_*), exactly as the uncaught JS throws do.
// - `x===undefined` checks become Option/contains_key presence checks;
//   `r.fetchInvocationId ?? null` in `source` covers absent and null.
use crate::core::{canonical_payload, payload_hash};
use crate::gates::normalize_official_fill_tick;
use crate::receipts::parse_number_lexemes;
use baal_service_runtime::fixed::{amount, instant, money, price};
use baal_service_runtime::json_text::{js_int, js_truthy, js_stringify, sha256};
use num_bigint::BigInt;
use serde_json::{json, Map, Value as V};
use std::collections::BTreeMap;

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("REST_NO_MULTI_{code}"))
    }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn hex(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
fn id(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        !s.is_empty()
            && !s.starts_with('0')
            && s.bytes().all(|b| b.is_ascii_digit())
            && s.parse::<i64>().is_ok_and(|n| n > 0 && n <= 9_007_199_254_740_991)
    })
}
fn int(x: &V, min: i64, max: i64) -> bool {
    js_int(x).is_some_and(|n| n >= min && n <= max)
}
fn int_loose(x: &V, min: i64) -> bool {
    int(x, min, i64::MAX)
}
fn instant_v(v: &V) -> Result<i64, String> {
    instant(v.as_str().unwrap_or(""))
}
fn amount_v(v: &V) -> Result<i128, String> {
    amount(v.as_str().unwrap_or(""))
}
fn same(a: &V, b: &V) -> Result<bool, String> {
    Ok(canonical_payload(a)? == canonical_payload(b)?)
}
// keys(x, allowed): object non-array whose every key is in `allowed`.
fn keys(x: &V, allowed: &[&str]) -> Result<(), String> {
    need(
        x.is_object()
            && x.as_object().map(|m| m.keys().all(|k| allowed.contains(&k.as_str()))).unwrap_or(false),
        "UNEXPECTED_METADATA",
    )
}
fn path_key(path: &[V]) -> String {
    serde_json::to_string(path).unwrap()
}
fn encode_uri_component(s: &str) -> String {
    let mut out = String::new();
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'!' | b'~' | b'*'
            | b'\'' | b'(' | b')' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
// source(r): the public shape of a raw receipt read.
fn source(r: &V) -> V {
    json!({
        "method": get(r, "method"),
        "endpoint": get(r, "endpoint"),
        "requestStartedAt": get(r, "requestStartedAt"),
        "receivedAt": get(r, "receivedAt"),
        "sourceBodyHash": get(r, "sourceBodyHash"),
        "fetchInvocationId": r.get("fetchInvocationId").cloned().unwrap_or(V::Null),
    })
}

/// REST_NO_MULTISHARE_CONTRACT_V1 (frozen object).
pub fn rest_no_multishare_contract_v1() -> V {
    json!({
        "schemaVersion": 1,
        "mode": "scoped_rest_no_multishare_fifo_v1",
        "minimumQuantity": 1,
        "maximumQuantity": 100,
        "maximumAttributedFills": 10000,
        "fee": "0.000000",
        "cashRoundingRadius": "0.010000",
        "maxReceiptAgeMs": 20000,
        "maxOrderLifetimeMs": 30000,
        "maxPages": 10,
        "averageAbsoluteTolerance": "0.000000000001",
        "priceBasis": "side_relative_rest_no",
        "documentationUrl": "https://sig.thesuper.market/api/v1/docs",
        "executionAuthority": false,
    })
}

#[derive(Clone)]
struct Read {
    data: V,
    numbers: BTreeMap<String, String>,
    src: V, // the source() record (method,endpoint,...,fetchInvocationId)
    start: i64,
    end: i64,
    endpoint: String,
    request_started_at: V,
    received_at: V,
    source_body_hash: String,
}

fn read(r: &V, method: &str, endpoint: &str, now: i64, fresh: bool) -> Result<Read, String> {
    need(
        get(r, "method").as_str() == Some(method)
            && get(r, "endpoint").as_str() == Some(endpoint)
            && get(r, "status") == &json!(200)
            && get(r, "bodyText").is_string(),
        "RECEIPT_SCOPE",
    )?;
    need(
        get(r, "fetchInvocationId").as_str().is_some_and(|s| !s.is_empty()),
        "RECEIPT_INVOCATION",
    )?;
    let start = instant_v(get(r, "requestStartedAt"))?;
    let end = instant_v(get(r, "receivedAt"))?;
    need(
        start <= end && end <= now && end - start <= 20_000 && (!fresh || now - start <= 20_000),
        "RECEIPT_TIME",
    )?;
    let source_body_hash = sha256(get(r, "bodyText").as_str().unwrap().as_bytes());
    need(
        get(r, "sourceBodyHash").as_str() == Some(source_body_hash.as_str()),
        "RECEIPT_HASH",
    )?;
    let (data, numbers) = parse_number_lexemes(get(r, "bodyText").as_str().unwrap())?;
    let mut src_in = r.clone();
    src_in
        .as_object_mut()
        .map(|m| m.insert("sourceBodyHash".into(), json!(source_body_hash)));
    Ok(Read {
        data,
        numbers,
        src: source(&src_in),
        start,
        end,
        endpoint: endpoint.to_string(),
        request_started_at: get(r, "requestStartedAt").clone(),
        received_at: get(r, "receivedAt").clone(),
        source_body_hash,
    })
}

// integerAt(r,path,value,min,max)
fn integer_at(r: &Read, path: &[V], value: &V, min: i64, max: i64) -> Result<i64, String> {
    let raw = r.numbers.get(&path_key(path));
    let canonical = raw.is_some_and(|l| {
        l.len() <= 128 && {
            let body = l.strip_prefix('-').unwrap_or(l);
            let mut parts = body.splitn(2, '.');
            let whole = parts.next().unwrap_or("");
            let frac = parts.next();
            (whole == "0" || (!whole.is_empty() && !whole.starts_with('0')))
                && whole.bytes().all(|b| b.is_ascii_digit())
                && match frac {
                    None => true,
                    Some(f) => !f.is_empty() && f.bytes().all(|b| b == b'0'),
                }
        }
    });
    need(
        canonical
            && int(value, min, max)
            && raw.unwrap().split('.').next().unwrap_or("").parse::<i128>().is_ok_and(|x| {
                Some(x) == js_int(value).map(|v| v as i128)
            }),
        "EXACT_INTEGER",
    )?;
    Ok(js_int(value).unwrap())
}
// amountAt(r,path)
fn amount_at(r: &Read, path: &[V]) -> Result<i128, String> {
    let raw = r.numbers.get(&path_key(path));
    need(raw.is_some(), "MONEY_SOURCE")?;
    amount(raw.unwrap()).map_err(|_| "REST_NO_MULTI_EXACT_MONEY".to_string())
}
// tickAt(r,path,value): official-tick read with the documented fallback.
fn tick_at(r: &Read, path: &[V], value: &V) -> Result<i128, String> {
    let raw = r.numbers.get(&path_key(path));
    need(raw.is_some(), "TICK_SOURCE")?;
    let raw = raw.unwrap();
    let v = match price(raw) {
        Ok(v) => v,
        Err(_) => {
            // Only the platform's specifically enumerated IEEE complement forms
            // are supported; an arbitrary long decimal that merely rounds to
            // one is not.
            need(raw == &js_stringify(value), "TICK_LEXEME")?;
            let t = normalize_official_fill_tick(value)
                .map_err(|_| "REST_NO_MULTI_TICK".to_string())?;
            amount(get(&t, "price").as_str().unwrap_or(""))
                .map_err(|_| "REST_NO_MULTI_TICK".to_string())?
        }
    };
    need(v >= 5000 && v <= 995_000, "TICK_RANGE")?;
    Ok(v)
}

// rational(raw): /^(-?)(0|[1-9]\d*)(?:\.(\d+))?(?:[eE]([+-]?\d+))?$/
fn rational(raw: &V) -> Result<(BigInt, BigInt), String> {
    need(raw.as_str().map(|s| s.len() <= 128).unwrap_or(false), "AVERAGE_SOURCE")?;
    let s = raw.as_str().unwrap();
    let bytes = s.as_bytes();
    let mut i = 0usize;
    let negative = bytes.first() == Some(&b'-');
    if negative {
        i += 1;
    }
    let w0 = i;
    while i < s.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    let whole = &s[w0..i];
    let whole_ok = !whole.is_empty()
        && (whole == "0" || !whole.starts_with('0'));
    if !whole_ok {
        return Err("REST_NO_MULTI_AVERAGE_DECIMAL".into());
    }
    let mut frac = "";
    if i < s.len() && bytes[i] == b'.' {
        i += 1;
        let f0 = i;
        while i < s.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        frac = &s[f0..i];
        if frac.is_empty() {
            return Err("REST_NO_MULTI_AVERAGE_DECIMAL".into());
        }
    }
    let mut exp: i64 = 0;
    if i < s.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        i += 1;
        let esign = if i < s.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
            let neg = bytes[i] == b'-';
            i += 1;
            neg
        } else {
            false
        };
        let e0 = i;
        while i < s.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i == e0 {
            return Err("REST_NO_MULTI_AVERAGE_DECIMAL".into());
        }
        exp = s[e0..i].parse::<i64>().map_err(|_| "REST_NO_MULTI_AVERAGE_DECIMAL".to_string())?;
        if esign {
            exp = -exp;
        }
    }
    if i != s.len() || exp.abs() > 30 {
        return Err("REST_NO_MULTI_AVERAGE_DECIMAL".into());
    }
    let shift = frac.len() as i64 - exp;
    let mut n: BigInt = format!("{whole}{frac}").parse().map_err(|_| "REST_NO_MULTI_AVERAGE_DECIMAL".to_string())?;
    let mut d = BigInt::from(1);
    if shift >= 0 {
        d = BigInt::from(10).pow(shift as u32);
    } else {
        n *= BigInt::from(10).pow((-shift) as u32);
    }
    if negative {
        n = -n;
    }
    Ok((n, d))
}

// The 3-state reported average: JS distinguishes `null` (literal field),
// `undefined` (missing lexeme), and a string. Both are falsy-but-different for
// `===` — model as an enum so projection-change checks stay exact.
#[derive(Clone, PartialEq)]
enum Reported {
    Undefined,
    Null,
    Lexeme(String),
}
impl Reported {
    fn as_v(&self) -> V {
        match self {
            Reported::Lexeme(s) => json!(s),
            _ => V::Null,
        }
    }
}

// averageCheck(raw,principal,quantity)
fn average_check(raw: &Reported, principal: i128, quantity: i64) -> Result<Option<V>, String> {
    if quantity == 0 {
        need(matches!(raw, Reported::Null), "EMPTY_AVERAGE")?;
        return Ok(None);
    }
    let raw_v = match raw {
        Reported::Lexeme(s) => json!(s),
        // JS passes null/undefined; typeof !== 'string' fails AVERAGE_SOURCE.
        _ => V::Null,
    };
    let (n, d) = rational(&raw_v)?;
    let q = BigInt::from(quantity);
    let million = BigInt::from(1_000_000i64);
    let delta = &n * &q * &million - BigInt::from(principal) * &d;
    let abs_delta = if delta < BigInt::from(0) { -&delta } else { delta.clone() };
    need(
        n > BigInt::from(0) && abs_delta * BigInt::from(1_000_000_000_000i64) <= &d * &q * &million,
        "AVERAGE_NOTIONAL",
    )?;
    Ok(Some(json!({
        "numeratorMicros": principal.to_string(),
        "denominatorShares": quantity,
        "reportedDecimal": raw.as_v(),
        "basis": "exact_fill_notional_divided_by_filled_shares",
        "reportedAverageUsedForPrincipal": false,
    })))
}

// account(raw,b,at,requiredFunding,reservedElsewhere): shared scoped account
// parse plus strengthened exact-integer order rows and source receipts.
fn account(
    raw: &V,
    b: &V,
    at: &V,
    required_funding: &V,
    reserved_elsewhere: &V,
) -> Result<V, String> {
    let now_i = instant_v(at)?;
    let mut input = Map::new();
    input.insert("account".into(), raw.clone());
    input.insert("binding".into(), b.clone());
    input.insert("readAt".into(), at.clone());
    input.insert("requiredFunding".into(), required_funding.clone());
    input.insert("reservedElsewhere".into(), reserved_elsewhere.clone());
    let mut a = crate::no_rest_sell_reconciliation_v2::inspect_scoped_rest_no_account(&V::Object(input))?
        .as_object()
        .cloned()
        .unwrap_or_else(Map::new);
    for page in get(raw, "scopedOrdersPages").as_array().cloned().unwrap_or_default() {
        let r = read(&page, "GET", get(&page, "endpoint").as_str().unwrap_or(""), now_i, true)?;
        for (i, row) in get(&r.data, "data").as_array().cloned().unwrap_or_default().iter().enumerate() {
            integer_at(&r, &[json!("data"), json!(i), json!("id")], get(row, "id"), 1, i64::MAX)?;
            integer_at(&r, &[json!("data"), json!(i), json!("quantity")], get(row, "quantity"), 1, i64::MAX)?;
            integer_at(
                &r,
                &[json!("data"), json!(i), json!("quantityFilled")],
                get(row, "quantityFilled"),
                0,
                js_int(get(row, "quantity")).unwrap_or(0),
            )?;
            need(get(row, "open") == &json!(false), "PRIOR_OPEN_ORDER")?;
        }
    }
    // Re-read each raw receipt under its own endpoint for sourceReceipts.
    let mut receipts: Vec<V> = Vec::new();
    receipts.push(get(raw, "cash").clone());
    receipts.push(get(raw, "positions").clone());
    receipts.extend(get(raw, "openOrdersPages").as_array().cloned().unwrap_or_default());
    receipts.extend(get(raw, "scopedOrdersPages").as_array().cloned().unwrap_or_default());
    let mut source_receipts = Vec::new();
    for r in &receipts {
        let rr = read(r, "GET", get(r, "endpoint").as_str().unwrap_or(""), now_i, true)?;
        source_receipts.push(rr.src);
    }
    a.insert("sourceReceipts".into(), V::Array(source_receipts));
    Ok(V::Object(a))
}

fn history(a: &V) -> Vec<V> {
    let mut rows = get(a, "scopedOrders").as_array().cloned().unwrap_or_default();
    rows.sort_by_key(|x| js_int(get(x, "id")).unwrap_or(0));
    rows
}
// target(a,b): signed position quantity of the target exchange (0 if absent),
// with the POSITION_SCOPE identity need when present.
fn target(a: &V, b: &V) -> Result<i64, String> {
    let row = get(a, "positions")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .find(|x| get(x, "exchangeId") == get(b, "exchangeId"));
    if let Some(row) = &row {
        need(
            get(row, "marketId") == get(b, "marketId") && get(row, "settled") == &json!(false),
            "POSITION_SCOPE",
        )?;
    }
    Ok(row.and_then(|r| js_int(get(&r, "quantity"))).unwrap_or(0))
}
fn other_positions(a: &V, b: &V) -> Vec<V> {
    // JS `x.quantity!==0` — any value except the strict number 0 (including
    // null/undefined) counts as a position.
    let mut rows: Vec<V> = get(a, "positions")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|x| get(x, "exchangeId") != get(b, "exchangeId") && x.get("quantity").and_then(V::as_f64) != Some(0.0))
        .collect();
    rows.sort_by(|x, y| {
        get(x, "exchangeId")
            .as_str()
            .unwrap_or("")
            .parse::<f64>()
            .unwrap_or(f64::NAN)
            .partial_cmp(&get(y, "exchangeId").as_str().unwrap_or("").parse::<f64>().unwrap_or(f64::NAN))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    rows
}

fn projection_progress(before: &V, after: &V) -> Result<(), String> {
    for k in ["openOrdersPages", "scopedOrdersPages"] {
        let project = |arr: &V| -> Result<Vec<V>, String> {
            let mut out = Vec::new();
            for r in arr.as_array().cloned().unwrap_or_default() {
                let (d, _) = parse_number_lexemes(get(&r, "bodyText").as_str().unwrap_or(""))?;
                // JS `d.coverage.projectedThroughSequence` — a missing coverage
                // key throws there, mirrored by an untyped error here.
                let v = match d.get("coverage") {
                    // JS `d.coverage.projectedThroughSequence` throws on
                    // absent or null coverage.
                    None => return Err("TypeError: coverage undefined".to_string()),
                    Some(V::Null) => return Err("TypeError: coverage null".to_string()),
                    Some(c) => c.get("projectedThroughSequence").cloned().unwrap_or(V::Null),
                };
                if !v.is_null() {
                    out.push(v);
                }
            }
            Ok(out)
        };
        let first = project(get(before, k))?;
        let last = project(get(after, k))?;
        if !first.is_empty() && !last.is_empty() {
            let mn = last.iter().filter_map(js_int).min().unwrap_or(i64::MAX);
            let mx = first.iter().filter_map(js_int).max().unwrap_or(i64::MIN);
            need(mn >= mx, "ACCOUNT_PROJECTION_ROLLBACK")?;
        }
    }
    Ok(())
}

struct PriorInventory {
    lots: Vec<V>,
    quantity: i64,
    #[allow(dead_code)]
    cost: i128,
    cumulative_realized: i128,
}

// priorInventory(proof,rawAfter,pre,b)
fn prior_inventory(proof: &V, raw_after: &V, pre: &V, b: &V) -> Result<PriorInventory, String> {
    if proof.is_null() {
        need(raw_after.is_null() && target(pre, b)? == 0, "FLAT_INITIAL_BUY_REQUIRED")?;
        return Ok(PriorInventory { lots: Vec::new(), quantity: 0, cost: 0, cumulative_realized: 0 });
    }
    // const {verificationHash, ...body} = proof
    let mut body = proof.as_object().cloned().unwrap_or_else(Map::new);
    let verification_hash = body.remove("verificationHash").unwrap_or(V::Null);
    need(
        hex(&verification_hash)
            && payload_hash(&V::Object(body.clone()))? == verification_hash.as_str().unwrap_or("")
            && get(proof, "mode").as_str() == Some("scoped_rest_no_multishare_fifo_v1")
            && get(proof, "scopedTradeVerified") == &json!(true)
            && get(proof, "executionAuthority") == &json!(false),
        "PRIOR_PROOF",
    )?;
    for k in ["ownerId", "runId", "tournamentId", "marketId", "exchangeId"] {
        need(get(proof, k) == get(b, k), "PRIOR_SCOPE")?;
    }
    let zero = json!("0.000000");
    let old = account(
        raw_after,
        b,
        get(proof, "readAt"),
        &zero,
        &zero,
    )?;
    need(
        same(get(&old, "sourceReceipts"), get(get(proof, "sourceReceipts"), "after"))?
            && instant_v(get(proof, "readAt"))? <= instant_v(get(pre, "startedAt"))?
            && instant_v(get(&old, "completedAt"))? <= instant_v(get(pre, "startedAt"))?
            && same(&V::Array(history(&old)), &V::Array(history(pre)))?,
        "PRIOR_SOURCE_OR_HISTORY",
    )?;
    // `proof.lastObservedFill===null` is strict: an absent key fails.
    let lof = get(proof, "lastObservedFill");
    need(
        proof.get("lastObservedFill") == Some(&V::Null)
            || (int(get(lof, "fillId"), 1, i64::MAX)
                && instant_v(get(lof, "filledAt"))? <= instant_v(get(proof, "readAt"))?),
        "PRIOR_FILL_WATERMARK",
    )?;
    need(
        int_loose(get(proof, "remainingOwnedQuantity"), 0)
            && target(&old, b)? == -js_int(get(proof, "remainingOwnedQuantity")).unwrap_or(0)
            && target(pre, b)? == -js_int(get(proof, "remainingOwnedQuantity")).unwrap_or(0),
        "PRIOR_POSITION",
    )?;
    need(
        get(proof, "remainingLots").as_array().is_some_and(|a| a.len() <= 10_000)
            && get(proof, "attributedBuyFills").as_array().is_some_and(|a| a.len() <= 10_000),
        "PRIOR_LOTS",
    )?;
    let mut seen: Vec<String> = Vec::new();
    let mut quantity: i64 = 0;
    let mut cost: i128 = 0;
    let mut last: Option<(i64, i64)> = None;
    for lot in get(proof, "remainingLots").as_array().cloned().unwrap_or_default() {
        need(
            get(&lot, "lotId").is_string()
                && !seen.contains(&get(&lot, "lotId").as_str().unwrap_or("").to_string())
                && id(get(&lot, "orderId"))
                && int(get(&lot, "fillId"), 1, i64::MAX)
                && int(get(&lot, "originalQuantity"), 1, 100)
                && int(
                    get(&lot, "quantity"),
                    1,
                    js_int(get(&lot, "originalQuantity")).unwrap_or(0),
                )
                && get(&lot, "side").as_str() == Some("no"),
            "PRIOR_LOT",
        )?;
        seen.push(get(&lot, "lotId").as_str().unwrap_or("").to_string());
        for k in ["ownerId", "runId", "tournamentId", "marketId", "exchangeId"] {
            need(get(&lot, k) == get(b, k), "PRIOR_LOT_SCOPE")?;
        }
        let p = price(get(&lot, "entryPrice").as_str().unwrap_or(""))?;
        let at = instant_v(get(&lot, "openedAt"))?;
        let original = get(proof, "attributedBuyFills")
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .find(|f| get(f, "lotId") == get(&lot, "lotId"));
        let mut expected = lot.as_object().cloned().unwrap_or_else(Map::new);
        expected.insert("quantity".into(), get(&lot, "originalQuantity").clone());
        expected.insert(
            "principalCost".into(),
            json!(money(p * js_int(get(&lot, "originalQuantity")).unwrap_or(0) as i128)),
        );
        need(
            original.is_some_and(|o| same(&V::Object(expected.clone()), &o).unwrap_or(false)),
            "PRIOR_LOT_ORIGIN",
        )?;
        let order = get(&old, "scopedOrders")
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .find(|o| crate::js_key(get(o, "id")) == get(&lot, "orderId").as_str().unwrap_or(""));
            let order_ok = order.as_ref().is_some_and(|o| {
            get(o, "side").as_str() == Some("no")
                && get(o, "action").as_str() == Some("buy")
                && js_int(get(o, "quantityFilled")).unwrap_or(0)
                    >= js_int(get(&lot, "originalQuantity")).unwrap_or(0)
                && get(o, "open") == &json!(false)
        });
        need(
            order_ok
                && at >= instant_v(get(order.as_ref().unwrap(), "createdAt"))?,
            "PRIOR_BUY_ORDER",
        )?;
        need(
            last.is_none()
                || at > last.unwrap().0
                || (at == last.unwrap().0
                    && js_int(get(&lot, "fillId")).unwrap_or(0) > last.unwrap().1),
            "FIFO_ORDER",
        )?;
        last = Some((at, js_int(get(&lot, "fillId")).unwrap_or(0)));
        need(
            get(&lot, "principalCost")
                == &json!(money(p * js_int(get(&lot, "quantity")).unwrap_or(0) as i128)),
            "PRIOR_LOT_COST",
        )?;
        quantity += js_int(get(&lot, "quantity")).unwrap_or(0);
        cost += p * js_int(get(&lot, "quantity")).unwrap_or(0) as i128;
    }
    need(
        quantity == js_int(get(proof, "remainingOwnedQuantity")).unwrap_or(i64::MIN)
            && money(cost) == get(proof, "remainingOwnedCost").as_str().unwrap_or(""),
        "PRIOR_TOTALS",
    )?;
    Ok(PriorInventory {
        lots: get(proof, "remainingLots").as_array().cloned().unwrap_or_default(),
        quantity,
        cost,
        cumulative_realized: amount_v(get(proof, "cumulativeRealizedPnl"))?,
    })
}

// neutralAll(a,cost,b) — the ACK `all` economics block.
fn neutral_all(a: &Read, cost: i128, b: &V) -> Result<V, String> {
    // `data.all===null` requires an explicit null — an absent key fails
    // ALL_REQUIRED (JS `all===null` is strict).
    let all_opt = a.data.get("all");
    need(
        all_opt.is_some() && (all_opt.unwrap().is_null() || all_opt.unwrap().is_object()),
        "ALL_REQUIRED",
    )?;
    let all = all_opt.unwrap();
    if all.is_null() {
        return Ok(json!({"kind": "null_all", "exactGlobalCash": false}));
    }
    let fields = [
        "fullNotionalCost",
        "effectiveEntryCost",
        "collateralSavings",
        "relationshipIds",
        "componentId",
        "guaranteedPayoutFloorAfter",
        "outstandingAdvanceAfter",
        "collateralRepayment",
        "netBuyingPowerImpact",
        "redemptionCredit",
    ];
    let mut all_keys: Vec<String> = all.as_object().unwrap().keys().cloned().collect();
    all_keys.sort();
    let mut sorted_fields: Vec<String> = fields.iter().map(|s| s.to_string()).collect();
    sorted_fields.sort();
    need(
        same(
            &V::Array(all_keys.iter().map(|k| json!(k)).collect()),
            &V::Array(sorted_fields.iter().map(|k| json!(k)).collect()),
        )? && get(all, "relationshipIds").as_array().is_some_and(|a| a.is_empty())
            && get(all, "componentId").is_null(),
        "NON_NEUTRAL_ALL",
    )?;
    for k in ["fullNotionalCost", "effectiveEntryCost"] {
        need(amount_at(a, &[json!("all"), json!(k)])? == cost, "ALL_PRINCIPAL")?;
    }
    for k in [
        "collateralSavings",
        "guaranteedPayoutFloorAfter",
        "outstandingAdvanceAfter",
        "collateralRepayment",
        "redemptionCredit",
    ] {
        need(amount_at(a, &[json!("all"), json!(k)])? == 0, "NON_NEUTRAL_ALL")?;
    }
    let cash_sign = if get(b, "action").as_str() == Some("buy") { cost } else { -cost };
    need(
        amount_at(a, &[json!("all"), json!("netBuyingPowerImpact")])? == cash_sign,
        "ALL_CASH_SIGN",
    )?;
    Ok(json!({
        "kind": "neutral_all",
        "fullNotionalCost": money(cost),
        "effectiveEntryCost": money(cost),
        "netBuyingPowerImpact": money(cash_sign),
        "collateralEffects": "zero",
        "relationshipCount": 0,
        "exactGlobalCash": false,
    }))
}

/** verifyScopedRestNoMultishare({binding,before,ack,order,fillPages,after,
 *  globalRisk,dispatchReceipt,readAt,reservedFunding='0.000000',
 *  reservedElsewhere='0.000000',priorProof=null,priorAfter=null,
 *  accountActivity=null})
 *
 *  Caller must obtain all receipts through its existing serialized collector.
 *  priorProof is an immutable output of this reducer; its raw priorAfter must
 *  be supplied for continuation. Hashes check integrity, not external
 *  authority. dispatchReceipt is the caller's durable actual-fetch record,
 *  never a queue fence timestamp. This reducer cannot authenticate journal
 *  ownership for it.
 */
pub fn verify_scoped_rest_no_multishare(input: &V) -> Result<V, String> {
    let b = get(input, "binding");
    let before = get(input, "before");
    let ack = get(input, "ack");
    let order = get(input, "order");
    let fill_pages = get(input, "fillPages");
    let after = get(input, "after");
    let global_risk = get(input, "globalRisk");
    let dispatch_receipt = get(input, "dispatchReceipt");
    let read_at = get(input, "readAt");
    let default_zero = json!("0.000000");
    let reserved_funding = input.get("reservedFunding").unwrap_or(&default_zero);
    let reserved_elsewhere = input.get("reservedElsewhere").unwrap_or(&default_zero);
    let prior_proof = input.get("priorProof").cloned().unwrap_or(V::Null);
    let prior_after = input.get("priorAfter").cloned().unwrap_or(V::Null);
    let account_activity = input.get("accountActivity").cloned().unwrap_or(V::Null);
    let now = instant_v(read_at)?;
    need(
        js_truthy(Some(b))
            && ["orderId", "marketId", "exchangeId"].iter().all(|k| id(get(b, k)))
            && ["ownerId", "runId", "intentId", "idempotencyKey", "tournamentId"]
                .iter()
                .all(|k| get(b, k).as_str().map(|s| !s.is_empty()).unwrap_or(false))
            && get(b, "side").as_str() == Some("no")
            && ["buy", "sell"].contains(&get(b, "action").as_str().unwrap_or(""))
            && int(get(b, "quantity"), 1, 100),
        "BINDING",
    )?;
    let limit = price(get(b, "limit").as_str().unwrap_or(""))?;
    let created = instant_v(get(b, "createdAt"))?;
    let expires = instant_v(get(b, "expirationDate"))?;
    need(created <= now && expires > created && expires - created <= 30_000, "BINDING_LIFETIME")?;
    need(
        get(global_risk, "ownerId") == get(b, "ownerId")
            && get(global_risk, "runId") == get(b, "runId")
            && get(global_risk, "tournamentId") == get(b, "tournamentId")
            && get(global_risk, "complete") == &json!(true)
            && get(global_risk, "soleCollector") == &json!(true)
            && get(global_risk, "stopped") == &json!(false)
            && hex(get(global_risk, "sourceHash"))
            && get(global_risk, "unresolvedIntentIds").as_array().is_some_and(|a| a.len() <= 1)
            && get(global_risk, "unresolvedIntentIds")
                .as_array()
                .map(|a| a.iter().all(|x| x == get(b, "intentId")))
                .unwrap_or(false)
            && instant_v(get(global_risk, "at"))? <= now
            && now - instant_v(get(global_risk, "at"))? <= 20_000,
        "GLOBAL_UNKNOWN_OR_OWNER",
    )?;
    need(account_activity.is_null(), "GLOBAL_ACTIVITY_UNSUPPORTED")?;
    let funding = amount_v(reserved_funding)?;
    let other = amount_v(reserved_elsewhere)?;
    need(
        funding >= 0
            && other >= 0
            && (get(b, "action").as_str() != Some("buy")
                || funding >= limit * js_int(get(b, "quantity")).unwrap_or(0) as i128),
        "FULL_FUNDING_RESERVATION",
    )?;
    let zero = json!("0.000000");
    let pre = account(
        before,
        b,
        get(b, "createdAt"),
        if get(b, "action").as_str() == Some("buy") { reserved_funding } else { &zero },
        reserved_elsewhere,
    )?;
    let post = account(after, b, read_at, &zero, &zero)?;
    projection_progress(before, after)?;
    if !prior_after.is_null() {
        projection_progress(&prior_after, before)?;
    }
    need(
        (get(b, "action").as_str() == Some("sell") || js_truthy(pre.get("funded")))
            && instant_v(get(&pre, "completedAt"))? <= created
            && instant_v(get(&post, "startedAt"))? >= created,
        "ACCOUNT_CAUSALITY_OR_FUNDING",
    )?;
    let prior = prior_inventory(&prior_proof, &prior_after, &pre, b)?;
    need(
        if get(b, "action").as_str() == Some("buy") {
            int(
                &json!(prior.quantity + js_int(get(b, "quantity")).unwrap_or(0)),
                0,
                i64::MAX,
            )
        } else {
            !prior_proof.is_null() && js_int(get(b, "quantity")).unwrap_or(i64::MAX) <= prior.quantity
        },
        "OWNED_QUANTITY",
    )?;
    let request_body = json!({
        "idempotencyKey": get(b, "idempotencyKey"),
        "exchangeId": get(b, "exchangeId"),
        "side": "no",
        "action": get(b, "action"),
        "quantity": get(b, "quantity"),
        "price": get(b, "limit").as_str().unwrap_or("").parse::<f64>().unwrap_or(f64::NAN),
        "expirationDate": get(b, "expirationDate"),
        "tournamentId": get(b, "tournamentId"),
    });
    let body_json = canonical_payload(&request_body)?;
    let body_hash = payload_hash(&request_body)?;
    let wire = dispatch_receipt;
    keys(
        wire,
        &[
            "method",
            "endpoint",
            "requestBodyJson",
            "requestBodyHash",
            "fetchInvocationId",
            "requestStartedAt",
            "ownerId",
            "runId",
            "tournamentId",
            "intentId",
            "idempotencyKey",
            "leaseIdentity",
            "leaseHash",
            "tournamentSlug",
            "marketId",
            "exchangeId",
            "kind",
        ],
    )?;
    if wire.get("kind").is_some() {
        need(get(wire, "kind").as_str() == Some("place"), "DISPATCH_OPERATION")?;
    }
    for k in ["marketId", "exchangeId", "tournamentSlug"] {
        if wire.get(k).is_some() {
            need(get(wire, k) == get(b, k), "DISPATCH_SCOPE")?;
        }
    }
    need(
        get(wire, "method").as_str() == Some("POST")
            && get(wire, "endpoint").as_str() == Some("/orders")
            && get(wire, "requestBodyJson").as_str() == Some(body_json.as_str())
            && get(wire, "requestBodyHash").as_str() == Some(body_hash.as_str())
            && get(wire, "fetchInvocationId").as_str().is_some_and(|s| !s.is_empty())
            && get(wire, "idempotencyKey") == get(b, "idempotencyKey"),
        "DISPATCH_BODY",
    )?;
    for k in ["ownerId", "runId", "tournamentId", "intentId"] {
        need(get(wire, k) == get(b, k), "DISPATCH_OWNER")?;
    }
    keys(
        get(wire, "leaseIdentity"),
        &["ownerId", "runId", "tournamentId", "leaseId"],
    )?;
    need(
        get(get(wire, "leaseIdentity"), "ownerId") == get(b, "ownerId")
            && get(get(wire, "leaseIdentity"), "tournamentId") == get(b, "tournamentId")
            && get(get(wire, "leaseIdentity"), "leaseId").as_str().is_some_and(|s| !s.is_empty())
            && get(wire, "leaseHash").as_str()
                == Some(payload_hash(get(wire, "leaseIdentity"))?.as_str()),
        "DISPATCH_LEASE",
    )?;
    let a = read(ack, "POST", "/orders", now, false)?;
    let d = a.data.clone();
    need(
        get(ack, "requestBodyHash").as_str() == Some(body_hash.as_str())
            && get(ack, "fetchInvocationId") == get(wire, "fetchInvocationId")
            && a.request_started_at == *get(wire, "requestStartedAt")
            && a.start >= created
            && a.start < expires,
        "ORIGINAL_FETCH_PROVENANCE",
    )?;
    need(
        crate::js_key(get(&d, "orderId")) == get(b, "orderId").as_str().unwrap_or("")
            && get(&d, "exchangeId") == get(b, "exchangeId")
            && get(&d, "action") == get(b, "action")
            && get(&d, "side").as_str() == Some("no")
            && get(&d, "open").is_boolean()
            && tick_at(&a, &[json!("price")], get(&d, "price"))? == limit,
        "ACK_CANONICAL",
    )?;
    integer_at(&a, &[json!("orderId")], get(&d, "orderId"), 1, i64::MAX)?;
    integer_at(&a, &[json!("quantity")], get(&d, "quantity"), 1, js_int(get(b, "quantity")).unwrap_or(0))?;
    integer_at(&a, &[json!("quantityTraded")], get(&d, "quantityTraded"), 0, js_int(get(&d, "quantity")).unwrap_or(0))?;
    integer_at(&a, &[json!("remainingQuantity")], get(&d, "remainingQuantity"), 0, js_int(get(&d, "quantity")).unwrap_or(0))?;
    need(
        js_int(get(&d, "quantityTraded")).unwrap_or(0)
            + js_int(get(&d, "remainingQuantity")).unwrap_or(0)
            == js_int(get(&d, "quantity")).unwrap_or(-1)
            && (!js_truthy(d.get("open")) || js_int(get(&d, "remainingQuantity")).unwrap_or(0) > 0),
        "ACK_QUANTITY",
    )?;
    let ack_cost = amount_at(&a, &[json!("totalCost")])?;
    need(
        ack_cost >= 0
            && ack_cost <= 995_000 * js_int(get(&d, "quantityTraded")).unwrap_or(0) as i128
            && (if js_truthy(d.get("quantityTraded")) {
                ack_cost >= 5000 * js_int(get(&d, "quantityTraded")).unwrap_or(0) as i128
            } else {
                ack_cost == 0
            }),
        "ACK_NOTIONAL",
    )?;
    let ack_economics = neutral_all(&a, ack_cost, b)?;
    let terminal = read(
        order,
        "GET",
        &format!("/orders/{}", get(b, "orderId").as_str().unwrap_or("")),
        now,
        true,
    )?;
    let od = terminal.data.clone();
    need(
        terminal.start >= a.end
            && crate::js_key(get(&od, "id")) == get(b, "orderId").as_str().unwrap_or("")
            && get(&od, "exchangeId") == get(b, "exchangeId")
            && get(&od, "tournamentId") == get(b, "tournamentId")
            && get(&od, "side").as_str() == Some("no")
            && get(&od, "action") == get(b, "action")
            && get(&od, "open") == &json!(false)
            && tick_at(&terminal, &[json!("priceLimit")], get(&od, "priceLimit"))? == limit
            && get(&od, "expirationDate") == get(b, "expirationDate"),
        "TERMINAL_SCOPE",
    )?;
    // The documented terminal quantity may shrink when unfilled remainder is
    // discarded. It never changes the original request, key or funding bound.
    integer_at(&terminal, &[json!("id")], get(&od, "id"), 1, i64::MAX)?;
    integer_at(
        &terminal,
        &[json!("quantity")],
        get(&od, "quantity"),
        1,
        js_int(get(&d, "quantity")).unwrap_or(0),
    )?;
    integer_at(
        &terminal,
        &[json!("quantityFilled")],
        get(&od, "quantityFilled"),
        0,
        js_int(get(&od, "quantity")).unwrap_or(0),
    )?;
    let server_created = instant_v(get(&od, "createdAt"))?;
    need(
        server_created >= created - 2000 && server_created <= std::cmp::min(a.end, expires) + 2000,
        "SERVER_ORDER_CLOCK",
    )?;
    need(
        fill_pages.as_array().is_some_and(|x| !x.is_empty() && x.len() <= 10),
        "FILL_PAGES",
    )?;
    let mut cursor: Option<String> = None;
    let mut last_end = terminal.end;
    let mut checkpoint: Option<i64> = None;
    let mut total: Option<i64> = None;
    let mut reported_average = Reported::Undefined;
    let mut cursors: Vec<String> = Vec::new();
    let mut ids: Vec<i64> = Vec::new();
    let mut fills: Vec<V> = Vec::new();
    let mut fill_sources: Vec<V> = Vec::new();
    for (page_index, raw) in fill_pages.as_array().unwrap().iter().enumerate() {
        let endpoint = format!(
            "/orders/{}/fills?limit=200{}",
            get(b, "orderId").as_str().unwrap_or(""),
            match &cursor {
                None => String::new(),
                Some(c) => format!("&cursor={}", encode_uri_component(c)),
            }
        );
        let r = read(raw, "GET", &endpoint, now, true)?;
        let v = r.data.clone();
        let p = get(&v, "pagination");
        need(
            r.start >= last_end
                && get(&v, "data").as_array().is_some_and(|x| x.len() <= 200)
                && crate::js_key(get(&v, "orderId")) == get(b, "orderId").as_str().unwrap_or("")
                && get(&v, "exchangeId") == get(b, "exchangeId")
                && get(&v, "tournamentId") == get(b, "tournamentId"),
            "FILL_SCOPE",
        )?;
        last_end = r.end;
        integer_at(&r, &[json!("orderId")], get(&v, "orderId"), 1, i64::MAX)?;
        integer_at(
            &r,
            &[json!("totalQuantityFilled")],
            get(&v, "totalQuantityFilled"),
            -js_int(get(b, "quantity")).unwrap_or(0),
            0,
        )?;
        let cov = get(&v, "coverage");
        need(
            get(p, "limit") == &json!(200)
                && get(p, "hasMore").is_boolean()
                && (p.get("nextCursor") == Some(&V::Null)
                    || get(p, "nextCursor").as_str().is_some_and(|s| !s.is_empty() && s.len() <= 1000))
                && get(cov, "complete") == &json!(true)
                && (cov.get("projectedThroughSequence") == Some(&V::Null)
                    || int_loose(cov.get("projectedThroughSequence").unwrap_or(&V::Null), 0)),
            "FILL_COVERAGE",
        )?;
        let projected_value = cov.get("projectedThroughSequence").and_then(js_int);
        if checkpoint.is_some() && projected_value.is_some() {
            need(projected_value.unwrap() >= checkpoint.unwrap(), "FILL_PROJECTION_ROLLBACK")?;
        }
        if projected_value.is_some() {
            checkpoint = projected_value;
        }
        let av = if get(&v, "avgFillPrice").is_null() {
            Reported::Null
        } else {
            match r.numbers.get(&path_key(&[json!("avgFillPrice")])).cloned() {
                Some(s) => Reported::Lexeme(s),
                None => Reported::Undefined,
            }
        };
        if let Some(t) = total {
            need(
                t == js_int(get(&v, "totalQuantityFilled")).unwrap_or(i64::MIN) && av == reported_average,
                "FILL_PROJECTION_CHANGED",
            )?;
        }
        total = js_int(get(&v, "totalQuantityFilled"));
        reported_average = av;
        for (i, f) in get(&v, "data").as_array().cloned().unwrap_or_default().iter().enumerate() {
            integer_at(&r, &[json!("data"), json!(i), json!("id")], get(f, "id"), 1, i64::MAX)?;
            integer_at(&r, &[json!("data"), json!(i), json!("quantity")], get(f, "quantity"), -100, -1)?;
            need(
                !ids.contains(&js_int(get(f, "id")).unwrap_or(i64::MIN))
                    && get(f, "side").as_str() == Some("no"),
                "FILL_IDENTITY",
            )?;
            ids.push(js_int(get(f, "id")).unwrap_or(i64::MIN));
            let qty = -js_int(get(f, "quantity")).unwrap_or(0);
            let px = tick_at(&r, &[json!("data"), json!(i), json!("price")], get(f, "price"))?;
            let at = instant_v(get(f, "filledAt"))?;
            need(
                at >= server_created
                    && at <= r.end
                    && (if get(b, "action").as_str() == Some("buy") { px <= limit } else { px >= limit }),
                "FILL_PRICE_OR_TIME",
            )?;
            fills.push(json!({
                "id": get(f, "id"),
                "orderId": get(b, "orderId"),
                "marketId": get(b, "marketId"),
                "exchangeId": get(b, "exchangeId"),
                "tournamentId": get(b, "tournamentId"),
                "side": "no",
                "action": get(b, "action"),
                "signedQuantity": -qty,
                "quantity": qty,
                "price": money(px),
                "principal": money(px * qty as i128),
                "filledAt": get(f, "filledAt"),
                "source": "rest_order_fills",
                "priceBasis": "side_relative",
            }));
        }
        if page_index < fill_pages.as_array().unwrap().len() - 1 {
            need(
                get(p, "hasMore") == &json!(true)
                    && p.get("nextCursor") != Some(&V::Null)
                    && get(p, "nextCursor").as_str().is_some_and(|c| !cursors.contains(&c.to_string())),
                "FILL_CURSOR",
            )?;
            cursors.push(get(p, "nextCursor").as_str().unwrap().to_string());
            cursor = Some(get(p, "nextCursor").as_str().unwrap().to_string());
        } else {
            need(
                get(p, "hasMore") == &json!(false) && p.get("nextCursor") == Some(&V::Null),
                "FILL_INCOMPLETE",
            )?;
        }
        fill_sources.push(r.src.clone());
    }
    fills.sort_by_key(|f| {
        (
            instant_v(get(f, "filledAt")).unwrap_or(0),
            js_int(get(f, "id")).unwrap_or(0),
        )
    });
    if !get(&prior_proof, "lastObservedFill").is_null() && !fills.is_empty() {
        let last = get(&prior_proof, "lastObservedFill");
        let first = fills.first().unwrap();
        need(
            instant_v(get(first, "filledAt"))? > instant_v(get(last, "filledAt"))?
                || (get(first, "filledAt") == get(last, "filledAt")
                    && js_int(get(first, "id")).unwrap_or(0) > js_int(get(last, "fillId")).unwrap_or(0)),
            "CONTINUATION_FILL_CLOCK",
        )?;
    }
    let quantity: i64 = fills.iter().map(|f| js_int(get(f, "quantity")).unwrap_or(0)).sum();
    let principal: i128 = fills
        .iter()
        .map(|f| amount(get(f, "principal").as_str().unwrap_or("")))
        .collect::<Result<Vec<i128>, String>>()?
        .into_iter()
        .sum();
    need(
        quantity <= js_int(get(&od, "quantity")).unwrap_or(0)
            && total == Some(-quantity)
            && js_int(get(&od, "quantityFilled")) == Some(quantity)
            && js_int(get(&d, "quantityTraded")).unwrap_or(i64::MAX) <= quantity
            && (js_truthy(d.get("open")) || js_int(get(&d, "quantityTraded")) == Some(quantity)),
        "COMPLETE_FILL_TOTAL",
    )?;
    let average = average_check(&reported_average, principal, quantity)?;
    let mut ack_quantity = js_int(get(&d, "quantityTraded")).unwrap_or(0);
    let mut ack_principal: i128 = 0;
    let mut last_ack_price: Option<i128> = None;
    for f in &fills {
        let q = std::cmp::min(ack_quantity, js_int(get(f, "quantity")).unwrap_or(0));
        if q != 0 {
            need(instant_v(get(f, "filledAt"))? <= a.end, "ACK_FILL_CLOCK")?;
            ack_principal += amount_v(get(f, "price"))? * q as i128;
            last_ack_price = Some(amount_v(get(f, "price"))?);
            ack_quantity -= q;
        }
    }
    need(ack_quantity == 0 && ack_principal == ack_cost, "ACK_FILL_NOTIONAL")?;
    let mut ack_fill_price_summary = V::Null;
    if js_truthy(d.get("quantityTraded")) {
        let mut matches: Vec<String> = Vec::new();
        let raw_average = a.numbers.get(&path_key(&[json!("fillPrice")])).cloned();
        // try{if(tickAt(a,['fillPrice'],d.fillPrice)===lastAckPrice)matches.push(...)}catch{}
        if tick_at(&a, &[json!("fillPrice")], get(&d, "fillPrice"))
            .map(|t| last_ack_price == Some(t))
            .unwrap_or(false)
        {
            matches.push("last_ack_prefix_fill_tick".to_string());
        }
        let raw_avg_v = raw_average.clone().map(|s| json!(s)).unwrap_or(V::Null);
        if average_check(
            &match raw_average {
                Some(ref s) => Reported::Lexeme(s.clone()),
                None => Reported::Undefined,
            },
            ack_cost,
            js_int(get(&d, "quantityTraded")).unwrap_or(0),
        )
        .is_ok()
        {
            matches.push("ack_prefix_weighted_average".to_string());
        }
        need(!matches.is_empty(), "ACK_FILL_PRICE_SUMMARY")?;
        // reportedDecimal:rawAverage — an undefined lexeme later fails
        // payloadHash (INVALID_JSON_VALUE), mirrored at hash time below.
        ack_fill_price_summary = json!({
            "reportedDecimal": raw_avg_v,
            "matches": matches,
            "principalDerivedFromSummary": false,
        });
    } else {
        need(
            d.get("fillPrice") == Some(&V::Null),
            "ACK_UNFILLED_PRICE",
        )?;
    }
    need(
        instant_v(get(&post, "startedAt"))? >= last_end
            && instant_v(get(global_risk, "at"))? >= instant_v(get(&post, "completedAt"))?,
        "POST_ACCOUNT_CAUSALITY",
    )?;
    let prior_history = history(&pre);
    let after_history = history(&post);
    let oid = get(b, "orderId").as_str().unwrap_or("").parse::<f64>().unwrap_or(f64::NAN);
    need(
        !prior_history
            .iter()
            .any(|o| get(o, "id").as_f64() == Some(oid))
            && same(
                &V::Array(prior_history.clone()),
                &V::Array(
                    after_history
                        .iter()
                        .filter(|o| get(o, "id").as_f64() != Some(oid))
                        .cloned()
                        .collect(),
                ),
            )?,
        "EXTERNAL_SCOPED_ACTIVITY",
    )?;
    let final_order = after_history
        .iter()
        .find(|o| get(o, "id").as_f64() == Some(oid))
        .cloned()
        .unwrap_or(V::Null);
    need(!final_order.is_null(), "SCOPED_ORDER_MISSING")?;
    for k in [
        "id",
        "exchangeId",
        "tournamentId",
        "side",
        "action",
        "quantity",
        "quantityFilled",
        "priceLimit",
        "open",
        "createdAt",
        "expirationDate",
    ] {
        need(
            canon_or_missing_v(final_order.get(k))? == canon_or_missing_v(od.as_object().and_then(|m| m.get(k)))?,
            "SCOPED_ORDER_MISMATCH",
        )?;
    }
    let signed_delta = if get(b, "action").as_str() == Some("buy") { -quantity } else { quantity };
    let remaining_quantity = prior.quantity + if get(b, "action").as_str() == Some("buy") { quantity } else { -quantity };
    let target_post = target(&post, b)?;
    let target_pre = target(&pre, b)?;
    need(
        target_post == -remaining_quantity
            && target_post - target_pre == signed_delta
            && same(&V::Array(other_positions(&pre, b)), &V::Array(other_positions(&post, b)))?,
        "POSITION_DELTA_OR_EXTERNAL_ACTIVITY",
    )?;
    let cash_delta = if get(b, "action").as_str() == Some("buy") { -principal } else { principal };
    let delta_low = amount_v(get(&post, "cashLower"))? - amount_v(get(&pre, "cashUpper"))?;
    let delta_high = amount_v(get(&post, "cashUpper"))? - amount_v(get(&pre, "cashLower"))?;
    need(cash_delta >= delta_low && cash_delta <= delta_high, "CASH_INTERVAL_INCONSISTENT")?;
    let mut new_receipts: Vec<V> = Vec::new();
    new_receipts.extend(get(&pre, "sourceReceipts").as_array().cloned().unwrap_or_default());
    new_receipts.push(a.src.clone());
    new_receipts.push(terminal.src.clone());
    new_receipts.extend(fill_sources.iter().cloned());
    new_receipts.extend(get(&post, "sourceReceipts").as_array().cloned().unwrap_or_default());
    {
        let mut inv: Vec<V> = Vec::new();
        let mut unique = true;
        for r in &new_receipts {
            let fid = get(r, "fetchInvocationId").clone();
            if inv.contains(&fid) {
                unique = false;
                break;
            }
            inv.push(fid);
        }
        need(unique && inv.len() == new_receipts.len(), "REUSED_RECEIPT_INVOCATION")?;
    }
    let mut lots = prior.lots.clone();
    let mut removed_lots: Vec<V> = Vec::new();
    let mut origins: Vec<V> = get(&prior_proof, "attributedBuyFills")
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut removed_cost: i128 = 0;
    if get(b, "action").as_str() == Some("buy") {
        for f in &fills {
            let lot = json!({
                "lotId": format!("{}:{}", get(b, "orderId").as_str().unwrap_or(""), crate::js_key(get(f, "id"))),
                "orderId": get(b, "orderId"),
                "fillId": get(f, "id"),
                "intentId": get(b, "intentId"),
                "ownerId": get(b, "ownerId"),
                "runId": get(b, "runId"),
                "tournamentId": get(b, "tournamentId"),
                "marketId": get(b, "marketId"),
                "exchangeId": get(b, "exchangeId"),
                "side": "no",
                "originalQuantity": get(f, "quantity"),
                "quantity": get(f, "quantity"),
                "entryPrice": get(f, "price"),
                "principalCost": get(f, "principal"),
                "openedAt": get(f, "filledAt"),
            });
            lots.push(lot.clone());
            origins.push(lot);
        }
    } else {
        let mut left = quantity;
        while left > 0 {
            need(!lots.is_empty(), "FIFO_OVERSELL")?;
            let mut lot = lots.remove(0);
            let lot_qty = js_int(get(&lot, "quantity")).unwrap_or(0);
            let q = std::cmp::min(left, lot_qty);
            let cost = price(get(&lot, "entryPrice").as_str().unwrap_or(""))? * q as i128;
            removed_cost += cost;
            let mut removed = lot.as_object().cloned().unwrap_or_else(Map::new);
            removed.insert("quantity".into(), json!(q));
            removed.insert("principalCost".into(), json!(money(cost)));
            removed_lots.push(V::Object(removed));
            left -= q;
            let new_qty = lot_qty - q;
            if new_qty != 0 {
                let entry = get(&lot, "entryPrice").clone();
                if let Some(m) = lot.as_object_mut() {
                    m.insert("quantity".into(), json!(new_qty));
                    m.insert(
                        "principalCost".into(),
                        json!(money(price(entry.as_str().unwrap_or("")).unwrap_or(0) * new_qty as i128)),
                    );
                }
                lots.insert(0, lot);
            }
        }
    }
    lots.sort_by_key(|x| {
        (
            instant_v(get(x, "openedAt")).unwrap_or(0),
            js_int(get(x, "fillId")).unwrap_or(0),
        )
    });
    need(origins.len() <= 10_000, "ATTRIBUTION_SIZE")?;
    let realized = if get(b, "action").as_str() == Some("sell") { principal - removed_cost } else { 0 };
    let remaining_cost: i128 = lots
        .iter()
        .map(|l| amount_v(get(l, "principalCost")))
        .collect::<Result<Vec<i128>, String>>()?
        .into_iter()
        .sum();
    let last_observed = if let Some(f) = fills.last() {
        json!({"filledAt": get(f, "filledAt"), "fillId": get(f, "id")})
    } else {
        get(&prior_proof, "lastObservedFill").clone()
    };
    let mut wire_dispatch = wire.as_object().cloned().unwrap_or_else(Map::new);
    wire_dispatch.insert("dispatchRecordHash".into(), json!(payload_hash(wire)?));
    let post_expiry_fill = fills
        .iter()
        .map(|f| instant_v(get(f, "filledAt")))
        .collect::<Result<Vec<i64>, String>>()?
        .iter()
        .any(|&x| x > expires);
    // Built field-by-field (json! object order = insertion order); the macro
    // recursion limit makes one big literal impractical at this size.
    let mut result = Map::new();
    fn put(k: &str, v: V, result: &mut Map<String, V>) {
        result.insert(k.to_string(), v);
    }
    put("schemaVersion", json!(1), &mut result);
    put("mode", json!("scoped_rest_no_multishare_fifo_v1"), &mut result);
    put("readAt", read_at.clone(), &mut result);
    put("ownerId", get(b, "ownerId").clone(), &mut result);
    put("runId", get(b, "runId").clone(), &mut result);
    put("tournamentId", get(b, "tournamentId").clone(), &mut result);
    put("marketId", get(b, "marketId").clone(), &mut result);
    put("exchangeId", get(b, "exchangeId").clone(), &mut result);
    put("orderId", get(b, "orderId").clone(), &mut result);
    put("intentId", get(b, "intentId").clone(), &mut result);
    put("action", get(b, "action").clone(), &mut result);
    put("side", json!("no"), &mut result);
    put("bindingHash", json!(payload_hash(b)?), &mut result);
    put("priorProofHash", get(&prior_proof, "verificationHash").clone(), &mut result);
    put("scopedTradeVerified", json!(true), &mut result);
    put("requestedQuantity", get(b, "quantity").clone(), &mut result);
    put("canonicalAcceptedQuantity", get(&d, "quantity").clone(), &mut result);
    put(
        "admissionDiscardedQuantity",
        json!(js_int(get(b, "quantity")).unwrap_or(0) - js_int(get(&d, "quantity")).unwrap_or(0)),
        &mut result,
    );
    put("terminalReportedQuantity", get(&od, "quantity").clone(), &mut result);
    put(
        "terminalReportedUnfilledQuantity",
        json!(js_int(get(&od, "quantity")).unwrap_or(0) - quantity),
        &mut result,
    );
    put("filledQuantity", json!(quantity), &mut result);
    put(
        "terminalUnfilledQuantity",
        json!(js_int(get(b, "quantity")).unwrap_or(0) - quantity),
        &mut result,
    );
    put("signedPositionDelta", json!(signed_delta), &mut result);
    put(
        "scopedPrincipalCost",
        json!(if get(b, "action").as_str() == Some("buy") { money(principal) } else { "0.000000".to_string() }),
        &mut result,
    );
    put(
        "scopedProceeds",
        json!(if get(b, "action").as_str() == Some("sell") { money(principal) } else { "0.000000".to_string() }),
        &mut result,
    );
    put("fillNotional", json!(money(principal)), &mut result);
    put("averageFillPrice", average.clone().unwrap_or(V::Null), &mut result);
    put("removedPrincipalCost", json!(money(removed_cost)), &mut result);
    put("realizedPnl", json!(money(realized)), &mut result);
    put("cumulativeRealizedPnl", json!(money(prior.cumulative_realized + realized)), &mut result);
    put("remainingOwnedQuantity", json!(remaining_quantity), &mut result);
    put("remainingOwnedCost", json!(money(remaining_cost)), &mut result);
    put("remainingLots", V::Array(lots.clone()), &mut result);
    put("removedLots", V::Array(removed_lots.clone()), &mut result);
    put("attributedBuyFills", V::Array(origins.clone()), &mut result);
    put("fills", V::Array(fills.clone()), &mut result);
    put("lastObservedFill", last_observed.clone(), &mut result);
    put("requestedLimit", json!(money(limit)), &mut result);
    put("requestedExpirationDate", get(b, "expirationDate").clone(), &mut result);
    put("postExpiryFillObserved", json!(post_expiry_fill), &mut result);
    put("canonicalAckFilledQuantity", get(&d, "quantityTraded").clone(), &mut result);
    put("ackNotional", json!(money(ack_cost)), &mut result);
    put(
        "ackCoversFinalFill",
        json!(js_int(get(&d, "quantityTraded")) == Some(quantity)),
        &mut result,
    );
    put("ackFillPriceSummary", ack_fill_price_summary.clone(), &mut result);
    put("ackEconomics", ack_economics.clone(), &mut result);
    put("fee", json!("0.000000"), &mut result);
    put("expectedCanonicalTradeCashDelta", json!(money(cash_delta)), &mut result);
    put(
        "accountCash",
        json!({
            "basis": "rounded_cent_plus_minus_one_cent",
            "before": {"lower": get(&pre, "cashLower"), "upper": get(&pre, "cashUpper")},
            "after": {"lower": get(&post, "cashLower"), "upper": get(&post, "cashUpper")},
            "observedDeltaInterval": {"lower": money(delta_low), "upper": money(delta_high)},
            "consistent": true,
            "exactDelta": V::Null,
        }),
        &mut result,
    );
    put(
        "funding",
        json!({
            "reservedFunding": reserved_funding,
            "reservedElsewhere": reserved_elsewhere,
            "fullyFundedAtSubmission": if get(b, "action").as_str() == Some("buy") { json!(true) } else { V::Null },
            "inventoryCoveredAtSubmission": if get(b, "action").as_str() == Some("sell") { json!(true) } else { V::Null },
        }),
        &mut result,
    );
    put(
        "inventory",
        json!({
            "priorSignedQuantity": -prior.quantity,
            "remainingSignedQuantity": -remaining_quantity,
            "unbackedSell": false,
            "foreignLotsAdopted": false,
        }),
        &mut result,
    );
    put(
        "sourceReceipts",
        json!({
            "before": get(&pre, "sourceReceipts"),
            "dispatch": wire_dispatch,
            "ack": a.src,
            "terminal": terminal.src,
            "fills": fill_sources,
            "after": get(&post, "sourceReceipts"),
        }),
        &mut result,
    );
    put(
        "globalIntentCensus",
        json!({
            "sourceHash": get(global_risk, "sourceHash"),
            "at": get(global_risk, "at"),
            "unresolvedIntentIds": get(global_risk, "unresolvedIntentIds"),
        }),
        &mut result,
    );
    put(
        "globalActivity",
        json!({"status": "unknown", "independentlyComplete": false}),
        &mut result,
    );
    put("globalCashReconciledExactly", json!(false), &mut result);
    put("globalAccountActivityVerified", json!(false), &mut result);
    put("authoritativeReconciliation", json!(false), &mut result);
    put("executionAuthority", json!(false), &mut result);
    put("newSubmissionAuthorized", json!(false), &mut result);
    let result = V::Object(result);
    // `reportedDecimal` carries a JS `undefined` when the fillPrice lexeme is
    // absent; canonicalPayload then throws INVALID_JSON_VALUE at hash time.
    if js_truthy(d.get("quantityTraded"))
        && get(&ack_fill_price_summary, "reportedDecimal").is_null()
    {
        return Err("INVALID_JSON_VALUE".into());
    }
    let mut out = result.as_object().cloned().unwrap_or_else(Map::new);
    out.insert("verificationHash".into(), json!(payload_hash(&result)?));
    Ok(V::Object(out))
}

// canonicalPayload(x) — absent keys throw INVALID_JSON_VALUE as in JS.
fn canon_or_missing_v(v: Option<&V>) -> Result<String, String> {
    match v {
        Some(x) => canonical_payload(x),
        None => Err("INVALID_JSON_VALUE".into()),
    }
}
