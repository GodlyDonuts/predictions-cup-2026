// execution/no-rest-multishare-reconciliation-v1.mjs — pure evidence
// reduction, not allocation, write authority, or a replacement for the
// one-share live verifiers. REST prices are side-relative and NO quantities
// are negative even for SELL (official public schema observed 2026-10-06).
//
// Port notes:
// - JS BigInt money accumulations -> num_bigint::BigInt where JS used BigInt
//   (principals, costs, deltas); micro i128 elsewhere via service-runtime
//   fixed.
// - `Object.freeze`/`structuredClone` are no-ops/.clone() — Rust values are
//   owned immutably by construction.
// - The JS regexes are manual parsers; `same` is canonical_payload equality.
use crate::core::{canonical_payload, payload_hash};
use crate::gates::normalize_official_fill_tick;
use crate::no_rest_sell_reconciliation_v2::inspect_scoped_rest_no_account;
use crate::receipts::parse_number_lexemes;
use baal_paper::fixed::money as bmoney;
use baal_service_runtime::fixed::{amount, instant, price};
use baal_service_runtime::json_text::{js_int, js_stringify, sha256};
use num_bigint::BigInt;
use serde_json::{json, Map, Value as V};
use std::collections::{BTreeMap, BTreeSet, HashSet};

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
            && s.parse::<u64>().is_ok_and(|n| n <= 9_007_199_254_740_991)
    })
}
fn int(x: &V, min: i64, max: i64) -> bool {
    js_int(x).is_some_and(|n| n >= min && n <= max)
}
fn same(a: &V, b: &V) -> Result<bool, String> {
    Ok(canonical_payload(a)? == canonical_payload(b)?)
}
fn keys(x: &V, allowed: &[&str]) -> Result<(), String> {
    need(
        x.is_object()
            && !x.is_array()
            && x.as_object()
                .map(|m| m.keys().all(|k| allowed.contains(&k.as_str())))
                .unwrap_or(false),
        "UNEXPECTED_METADATA",
    )
}
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
fn encode_uri_component(s: &str) -> String {
    let mut out = String::new();
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
fn path_key(path: &[V]) -> String {
    js_stringify(&V::Array(path.to_vec()))
}
fn instant_v(v: &V) -> Result<i64, String> {
    instant(v.as_str().unwrap_or(""))
}
// String(v) — JS primitive-to-string coercion.
fn js_string(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Number(_) | V::Bool(_) => js_stringify(v),
        V::Null => "null".to_string(),
        _ => "undefined".to_string(),
    }
}

// read(r, method, endpoint, now, fresh=true): `data`, `numbers` plus the
// captured source fields and the receipt's own start/end instants.
struct Read {
    data: V,
    numbers: BTreeMap<String, String>,
    source: V,
    start: i64,
    end: i64,
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
        start <= end && end <= now && end - start <= 20000 && (!fresh || now - start <= 20000),
        "RECEIPT_TIME",
    )?;
    let source_body_hash = sha256(get(r, "bodyText").as_str().unwrap().as_bytes());
    need(
        get(r, "sourceBodyHash").as_str() == Some(source_body_hash.as_str()),
        "RECEIPT_HASH",
    )?;
    let mut with_hash = r.clone();
    with_hash
        .as_object_mut()
        .map(|m| m.insert("sourceBodyHash".into(), json!(source_body_hash)));
    let (data, numbers) = parse_number_lexemes(get(r, "bodyText").as_str().unwrap())?;
    Ok(Read {
        data,
        numbers,
        source: source(&with_hash),
        start,
        end,
    })
}

// integerAt(r,path,value,min,max)
fn integer_at(r: &Read, path: &[V], value: &V, min: i64, max: i64) -> Result<i64, String> {
    let raw = r.numbers.get(&path_key(path)).cloned();
    let lexeme_ok = raw.as_deref().is_some_and(|l| {
        if l.len() > 128 {
            return false;
        }
        // /^-?(0|[1-9]\d*)(\.0+)?$/
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
    });
    let v = js_int(value);
    need(
        lexeme_ok
            && v.is_some_and(|n| n >= min && n <= max)
            && raw
                .as_deref()
                .and_then(|l| l.split('.').next())
                .and_then(|s| s.parse::<BigInt>().ok())
                == v.map(BigInt::from),
        "EXACT_INTEGER",
    )?;
    Ok(v.unwrap_or(0))
}

// amountAt(r,path)
fn amount_at(r: &Read, path: &[V]) -> Result<BigInt, String> {
    let raw = r.numbers.get(&path_key(path)).cloned();
    need(raw.is_some(), "MONEY_SOURCE")?;
    amount(raw.as_deref().unwrap_or(""))
        .map(BigInt::from)
        .map_err(|_| "REST_NO_MULTI_EXACT_MONEY".to_string())
}

// tickAt(r,path,value): only the platform's specifically enumerated IEEE
// complement forms are supported — an arbitrary long decimal which merely
// rounds to one is not.
fn tick_at(r: &Read, path: &[V], value: &V) -> Result<i128, String> {
    let raw = r.numbers.get(&path_key(path)).cloned();
    need(raw.is_some(), "TICK_SOURCE")?;
    let raw = raw.unwrap();
    let v = match price(&raw) {
        Ok(v) => v,
        Err(_) => {
            need(raw == js_stringify(value), "TICK_LEXEME")?;
            let t = normalize_official_fill_tick(value)?;
            amount(get(&t, "price").as_str().unwrap_or(""))
                .map_err(|_| "REST_NO_MULTI_TICK".to_string())?
        }
    };
    need((5000..=995000).contains(&v), "TICK_RANGE")?;
    Ok(v)
}

#[derive(Clone)]
struct Rat {
    n: BigInt,
    d: BigInt,
}
// rational(raw): /^(-?)(0|[1-9]\d*)(?:\.(\d+))?(?:[eE]([+-]?\d+))?$/
fn rational(raw: &str) -> Result<Rat, String> {
    need(raw.len() <= 128, "AVERAGE_SOURCE")?;
    let b = raw.as_bytes();
    let mut i = 0usize;
    let neg = if i < b.len() && b[i] == b'-' {
        i += 1;
        true
    } else {
        false
    };
    let digits = |i: &mut usize| -> bool {
        let s = *i;
        while *i < b.len() && b[*i].is_ascii_digit() {
            *i += 1;
        }
        *i > s
    };
    if i >= b.len() {
        return Err("REST_NO_MULTI_AVERAGE_DECIMAL".into());
    }
    if b[i] == b'0' {
        i += 1;
        need(i >= b.len() || !b[i].is_ascii_digit(), "AVERAGE_DECIMAL")?;
    } else {
        need(digits(&mut i), "AVERAGE_DECIMAL")?;
    }
    let int_part = &raw[..i];
    let mut fraction = "";
    if i < b.len() && b[i] == b'.' {
        i += 1;
        let s = i;
        need(digits(&mut i), "AVERAGE_DECIMAL")?;
        fraction = &raw[s..i];
    }
    let mut exponent: i64 = 0;
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        let eneg = if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            let n = b[i] == b'-';
            i += 1;
            n
        } else {
            false
        };
        let s = i;
        need(digits(&mut i), "AVERAGE_DECIMAL")?;
        exponent = raw[s..i].parse::<i64>().map_err(|_| "REST_NO_MULTI_AVERAGE_DECIMAL".to_string())?;
        if eneg {
            exponent = -exponent;
        }
    }
    need(i == b.len(), "AVERAGE_DECIMAL")?;
    need(exponent.abs() <= 30, "AVERAGE_DECIMAL")?;
    let mut n: BigInt = format!("{int_part}{fraction}")
        .parse()
        .map_err(|_| "REST_NO_MULTI_AVERAGE_DECIMAL".to_string())?;
    let mut d = BigInt::from(1);
    let shift = fraction.len() as i64 - exponent;
    if shift >= 0 {
        d = BigInt::from(10u32).pow(shift as u32);
    } else {
        n *= BigInt::from(10u32).pow((-shift) as u32);
    }
    if neg {
        n = -n;
    }
    Ok(Rat { n, d })
}

// avgFillPrice lexeme state distinguishes JS null (parsed null), a string
// lexeme, and a missing lexeme (undefined).
#[derive(Clone)]
enum RawAverage {
    Null,
    Missing,
    Str(String),
}
impl RawAverage {
    fn as_option(&self) -> Option<String> {
        match self {
            RawAverage::Str(s) => Some(s.clone()),
            _ => None,
        }
    }
}

// averageCheck(raw, principal, quantity)
fn average_check(raw: &RawAverage, principal: &BigInt, quantity: i64) -> Result<Option<V>, String> {
    if quantity == 0 {
        need(matches!(raw, RawAverage::Null), "EMPTY_AVERAGE")?;
        return Ok(None);
    }
    let rat = rational(&raw.as_option().unwrap_or_default())?;
    let delta = &rat.n * BigInt::from(quantity) * BigInt::from(1_000_000i64) - principal * &rat.d;
    need(
        rat.n > BigInt::from(0)
            && (if delta < BigInt::from(0) { -delta.clone() } else { delta.clone() }) * BigInt::from(1_000_000_000_000i64)
                <= &rat.d * BigInt::from(quantity) * BigInt::from(1_000_000i64),
        "AVERAGE_NOTIONAL",
    )?;
    Ok(Some(json!({
        "numeratorMicros": principal.to_string(),
        "denominatorShares": quantity,
        "reportedDecimal": raw.as_option().map(V::from).unwrap_or(V::Null),
        "basis": "exact_fill_notional_divided_by_filled_shares",
        "reportedAverageUsedForPrincipal": false,
    })))
}

// account(raw,b,at,requiredFunding='0.000000',reservedElsewhere='0.000000')
fn account(raw: &V, b: &V, at: &V, required_funding: &V, reserved_elsewhere: &V) -> Result<V, String> {
    let mut a = inspect_scoped_rest_no_account(&json!({
        "account": raw,
        "binding": b,
        "readAt": at,
        "requiredFunding": required_funding,
        "reservedElsewhere": reserved_elsewhere,
    }))?;
    let at_ms = instant_v(at)?;
    // Strengthen the shared account parser's historical order quantities using
    // original number lexemes, including closed-order quantityFilled.
    for page in get(raw, "scopedOrdersPages").as_array().cloned().unwrap_or_default() {
        let r = read(&page, "GET", get(&page, "endpoint").as_str().unwrap_or(""), at_ms, true)?;
        for (i, row) in get(&r.data, "data").as_array().cloned().unwrap_or_default().iter().enumerate() {
            integer_at(&r, &[json!("data"), json!(i), json!("id")], get(row, "id"), 1, i64::MAX)?;
            integer_at(&r, &[json!("data"), json!(i), json!("quantity")], get(row, "quantity"), 1, i64::MAX)?;
            let qty = js_int(get(row, "quantity")).unwrap_or(i64::MAX);
            integer_at(&r, &[json!("data"), json!(i), json!("quantityFilled")], get(row, "quantityFilled"), 0, qty)?;
            need(get(row, "open") == &json!(false), "PRIOR_OPEN_ORDER")?;
        }
    }
    // The shared parser validates exact cash/positions/page scope and completeness.
    let mut receipts: Vec<V> = vec![get(raw, "cash").clone(), get(raw, "positions").clone()];
    receipts.extend(get(raw, "openOrdersPages").as_array().cloned().unwrap_or_default());
    receipts.extend(get(raw, "scopedOrdersPages").as_array().cloned().unwrap_or_default());
    let mut sources = Vec::new();
    for r in &receipts {
        let rd = read(r, "GET", get(r, "endpoint").as_str().unwrap_or(""), at_ms, true)?;
        sources.push(rd.source);
    }
    a.as_object_mut()
        .map(|m| m.insert("sourceReceipts".into(), V::Array(sources)));
    Ok(a)
}

// projectionProgress(before, after) — both account snapshots.
fn projection_progress(before: &V, after: &V) -> Result<(), String> {
    for k in ["openOrdersPages", "scopedOrdersPages"] {
        // Keeps every non-null projectedThroughSequence (absent keeps NaN like
        // JS Math.min(undefined,...) which fails the comparison when both
        // lists are non-empty).
        let collect = |snap: &V| -> Result<Vec<f64>, String> {
            let mut out = Vec::new();
            for r in get(snap, k).as_array().cloned().unwrap_or_default() {
                let (d, _) = parse_number_lexemes(get(&r, "bodyText").as_str().unwrap_or(""))?;
                // JS `data.coverage.projectedThroughSequence` throws a TypeError
                // when coverage is absent or null; a non-object coverage yields
                // undefined which !== null and poisons Math.min with NaN.
                let cov = d.get("coverage");
                if cov.is_none() || cov.is_some_and(|c| c.is_null()) {
                    return Err("Cannot read properties of null".to_string());
                }
                let cov = cov.unwrap();
                if cov.is_object() {
                    match cov.get("projectedThroughSequence") {
                        Some(V::Null) => {}
                        Some(v) => out.push(v.as_f64().unwrap_or(f64::NAN)),
                        None => out.push(f64::NAN),
                    }
                } else {
                    out.push(f64::NAN);
                }
            }
            Ok(out)
        };
        let first = collect(before)?;
        let last = collect(after)?;
        if !first.is_empty() && !last.is_empty() {
            let min_last = last.iter().cloned().fold(f64::NAN, f64::min);
            let max_first = first.iter().cloned().fold(f64::NAN, f64::max);
            need(min_last >= max_first, "ACCOUNT_PROJECTION_ROLLBACK")?;
        }
    }
    Ok(())
}

fn history(a: &V) -> Vec<V> {
    let mut rows = get(a, "scopedOrders").as_array().cloned().unwrap_or_default();
    rows.sort_by_key(|o| js_int(get(o, "id")).unwrap_or(0));
    rows
}
fn target(a: &V, b: &V) -> Result<i64, String> {
    let row = get(a, "positions")
        .as_array()
        .and_then(|p| p.iter().find(|x| get(x, "exchangeId") == get(b, "exchangeId")))
        .cloned();
    if let Some(row) = &row {
        need(
            get(row, "marketId") == get(b, "marketId") && get(row, "settled") == &json!(false),
            "POSITION_SCOPE",
        )?;
    }
    Ok(row
        .and_then(|r| js_int(get(&r, "quantity")))
        .unwrap_or(0))
}
fn other_positions(a: &V, b: &V) -> Vec<V> {
    let mut rows: Vec<V> = get(a, "positions")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|x| {
            get(x, "exchangeId") != get(b, "exchangeId") && get(x, "quantity") != &json!(0)
        })
        .collect();
    rows.sort_by_key(|x| {
        get(x, "exchangeId")
            .as_str()
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0)
    });
    rows
}

struct Prior {
    lots: Vec<V>,
    quantity: i64,
    cost: BigInt,
    cumulative_realized: BigInt,
}
// priorInventory(proof, rawAfter, pre, b)
fn prior_inventory(proof: &V, raw_after: &V, pre: &V, b: &V) -> Result<Prior, String> {
    if proof.is_null() {
        need(
            raw_after.is_null() && target(pre, b)? == 0,
            "FLAT_INITIAL_BUY_REQUIRED",
        )?;
        return Ok(Prior {
            lots: Vec::new(),
            quantity: 0,
            cost: BigInt::from(0),
            cumulative_realized: BigInt::from(0),
        });
    }
    let mut body = proof.clone();
    let verification_hash = body
        .as_object_mut()
        .and_then(|m| m.shift_remove("verificationHash"))
        .unwrap_or(V::Null);
    need(
        hex(&verification_hash)
            && payload_hash(&body)? == verification_hash.as_str().unwrap_or("")
            && get(proof, "mode").as_str() == Some("scoped_rest_no_multishare_fifo_v1")
            && get(proof, "scopedTradeVerified") == &json!(true)
            && get(proof, "executionAuthority") == &json!(false),
        "PRIOR_PROOF",
    )?;
    for k in ["ownerId", "runId", "tournamentId", "marketId", "exchangeId"] {
        need(get(proof, k) == get(b, k), "PRIOR_SCOPE")?;
    }
    let old = account(raw_after, b, get(proof, "readAt"), &json!("0.000000"), &json!("0.000000"))?;
    need(
        same(get(&old, "sourceReceipts"), get(get(proof, "sourceReceipts"), "after"))?
            && instant_v(get(proof, "readAt"))? <= instant_v(get(pre, "startedAt"))?
            && instant_v(get(&old, "completedAt"))? <= instant_v(get(pre, "startedAt"))?
            && same(&json!(history(&old)), &json!(history(pre)))?,
        "PRIOR_SOURCE_OR_HISTORY",
    )?;
    let lof = get(proof, "lastObservedFill");
    need(
        lof.is_null()
            || (!lof.is_null()
                && int(get(lof, "fillId"), 1, i64::MAX)
                && instant_v(get(lof, "filledAt")).unwrap_or(i64::MIN)
                    <= instant_v(get(proof, "readAt")).unwrap_or(i64::MAX)),
        "PRIOR_FILL_WATERMARK",
    )?;
    need(
        js_int(get(proof, "remainingOwnedQuantity")).is_some()
            && target(&old, b)? == -js_int(get(proof, "remainingOwnedQuantity")).unwrap_or(i64::MIN)
            && target(pre, b)? == -js_int(get(proof, "remainingOwnedQuantity")).unwrap_or(i64::MIN),
        "PRIOR_POSITION",
    )?;
    need(
        get(proof, "remainingLots").is_array()
            && get(proof, "remainingLots").as_array().unwrap().len() <= 10000
            && get(proof, "attributedBuyFills").is_array()
            && get(proof, "attributedBuyFills").as_array().unwrap().len() <= 10000,
        "PRIOR_LOTS",
    )?;
    let mut seen: HashSet<String> = HashSet::new();
    let mut quantity: i64 = 0;
    let mut cost = BigInt::from(0);
    let mut last: Option<(i64, i64)> = None;
    for lot in get(proof, "remainingLots").as_array().unwrap() {
        need(
            get(lot, "lotId").is_string()
                && !seen.contains(get(lot, "lotId").as_str().unwrap_or(""))
                && id(get(lot, "orderId"))
                && int(get(lot, "fillId"), 1, i64::MAX)
                && int(get(lot, "originalQuantity"), 1, 100)
                && int(get(lot, "quantity"), 1, js_int(get(lot, "originalQuantity")).unwrap_or(0))
                && get(lot, "side").as_str() == Some("no"),
            "PRIOR_LOT",
        )?;
        seen.insert(get(lot, "lotId").as_str().unwrap_or("").to_string());
        for k in ["ownerId", "runId", "tournamentId", "marketId", "exchangeId"] {
            need(get(lot, k) == get(b, k), "PRIOR_LOT_SCOPE")?;
        }
        let p = price(get(lot, "entryPrice").as_str().unwrap_or(""))?;
        let at = instant_v(get(lot, "openedAt"))?;
        let original = get(proof, "attributedBuyFills")
            .as_array()
            .and_then(|a| a.iter().find(|f| get(f, "lotId") == get(lot, "lotId")))
            .cloned()
            .unwrap_or(V::Null);
        let mut expected = lot.clone();
        if let V::Object(m) = &mut expected {
            m.insert("quantity".into(), get(lot, "originalQuantity").clone());
            m.insert(
                "principalCost".into(),
                json!(bmoney(&(BigInt::from(p) * js_int(get(lot, "originalQuantity")).map(BigInt::from).unwrap_or_default()))),
            );
        }
        need(!original.is_null() && same(&expected, &original)?, "PRIOR_LOT_ORIGIN")?;
        let order = get(&old, "scopedOrders")
            .as_array()
            .and_then(|a| {
                a.iter()
                    .find(|o| js_string(get(o, "id")) == get(lot, "orderId").as_str().unwrap_or(""))
            })
            .cloned()
            .unwrap_or(V::Null);
        need(
            get(&order, "side").as_str() == Some("no")
                && get(&order, "action").as_str() == Some("buy")
                && js_int(get(&order, "quantityFilled")).unwrap_or(-1)
                    >= js_int(get(lot, "originalQuantity")).unwrap_or(i64::MAX)
                && get(&order, "open") == &json!(false)
                && at >= instant_v(get(&order, "createdAt"))?,
            "PRIOR_BUY_ORDER",
        )?;
        let fill_id = js_int(get(lot, "fillId")).unwrap_or(0);
        need(
            last.is_none_or(|(la, lf)| at > la || (at == la && fill_id > lf)),
            "FIFO_ORDER",
        )?;
        last = Some((at, fill_id));
        need(
            get(lot, "principalCost").as_str()
                == Some(bmoney(&(BigInt::from(p) * BigInt::from(js_int(get(lot, "quantity")).unwrap_or(0)))).as_str()),
            "PRIOR_LOT_COST",
        )?;
        quantity += js_int(get(lot, "quantity")).unwrap_or(0);
        cost += BigInt::from(p) * BigInt::from(js_int(get(lot, "quantity")).unwrap_or(0));
    }
    need(
        quantity == js_int(get(proof, "remainingOwnedQuantity")).unwrap_or(-1)
            && bmoney(&cost.clone()) == get(proof, "remainingOwnedCost").as_str().unwrap_or(""),
        "PRIOR_TOTALS",
    )?;
    Ok(Prior {
        lots: get(proof, "remainingLots").as_array().cloned().unwrap_or_default(),
        quantity,
        cost,
        cumulative_realized: amount(get(proof, "cumulativeRealizedPnl").as_str().unwrap_or(""))
            .map(BigInt::from)?,
    })
}

// neutralAll(a, cost, b) — the ack's account-economics `all` block.
fn neutral_all(a: &Read, cost: &BigInt, b: &V) -> Result<V, String> {
    let all = get(&a.data, "all").clone();
    need(all.is_null() || (all.is_object() && !all.is_array()), "ALL_REQUIRED")?;
    if all.is_null() {
        return Ok(json!({"kind": "null_all", "exactGlobalCash": false}));
    }
    let mut fields: Vec<String> = all.as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
    fields.sort();
    let mut want: Vec<String> = [
        "fullNotionalCost", "effectiveEntryCost", "collateralSavings", "relationshipIds",
        "componentId", "guaranteedPayoutFloorAfter", "outstandingAdvanceAfter",
        "collateralRepayment", "netBuyingPowerImpact", "redemptionCredit",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    want.sort();
    need(
        fields == want
            && get(&all, "relationshipIds").is_array()
            && get(&all, "relationshipIds").as_array().unwrap().is_empty()
            && get(&all, "componentId").is_null(),
        "NON_NEUTRAL_ALL",
    )?;
    for k in ["fullNotionalCost", "effectiveEntryCost"] {
        need(
            amount_at(a, &[json!("all"), json!(k)])? == *cost,
            "ALL_PRINCIPAL",
        )?;
    }
    for k in [
        "collateralSavings", "guaranteedPayoutFloorAfter", "outstandingAdvanceAfter",
        "collateralRepayment", "redemptionCredit",
    ] {
        need(
            amount_at(a, &[json!("all"), json!(k)])? == BigInt::from(0),
            "NON_NEUTRAL_ALL",
        )?;
    }
    let signed = if get(b, "action").as_str() == Some("buy") {
        cost.clone()
    } else {
        -cost.clone()
    };
    need(
        amount_at(a, &[json!("all"), json!("netBuyingPowerImpact")])? == signed,
        "ALL_CASH_SIGN",
    )?;
    Ok(json!({
        "kind": "neutral_all",
        "fullNotionalCost": bmoney(&cost.clone()),
        "effectiveEntryCost": bmoney(&cost.clone()),
        "netBuyingPowerImpact": bmoney(&signed),
        "collateralEffects": "zero",
        "relationshipCount": 0,
        "exactGlobalCash": false,
    }))
}

/// verifyScopedRestNoMultishare({binding, before, ack, order, fillPages, after,
/// globalRisk, dispatchReceipt, readAt, reservedFunding='0.000000',
/// reservedElsewhere='0.000000', priorProof=null, priorAfter=null,
/// accountActivity=null})
///
/// Caller must obtain all receipts through its existing serialized collector.
/// priorProof is an immutable output of this reducer; its raw priorAfter must
/// be supplied for continuation. Hashes check integrity, not external
/// authority. dispatchReceipt is the caller's durable actual-fetch record,
/// never a queue fence timestamp. This reducer cannot authenticate journal
/// ownership for it.
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
    let reserved_funding = input.get("reservedFunding").cloned().unwrap_or(json!("0.000000"));
    let reserved_elsewhere = input.get("reservedElsewhere").cloned().unwrap_or(json!("0.000000"));
    let prior_proof = input.get("priorProof").cloned().unwrap_or(V::Null);
    let prior_after = input.get("priorAfter").cloned().unwrap_or(V::Null);
    let account_activity = input.get("accountActivity").cloned().unwrap_or(V::Null);
    let now = instant_v(read_at)?;
    need(
        ["orderId", "marketId", "exchangeId"].iter().all(|k| id(get(b, k)))
            && ["ownerId", "runId", "intentId", "idempotencyKey", "tournamentId"]
                .iter()
                .all(|k| get(b, k).as_str().is_some_and(|s| !s.is_empty()))
            && get(b, "side").as_str() == Some("no")
            && ["buy", "sell"].contains(&get(b, "action").as_str().unwrap_or(""))
            && int(get(b, "quantity"), 1, 100),
        "BINDING",
    )?;
    let limit = price(get(b, "limit").as_str().unwrap_or(""))?;
    let created = instant_v(get(b, "createdAt"))?;
    let expires = instant_v(get(b, "expirationDate"))?;
    need(
        created <= now && expires > created && expires - created <= 30000,
        "BINDING_LIFETIME",
    )?;
    need(
        get(global_risk, "ownerId") == get(b, "ownerId")
            && get(global_risk, "runId") == get(b, "runId")
            && get(global_risk, "tournamentId") == get(b, "tournamentId")
            && get(global_risk, "complete") == &json!(true)
            && get(global_risk, "soleCollector") == &json!(true)
            && get(global_risk, "stopped") == &json!(false)
            && hex(get(global_risk, "sourceHash"))
            && get(global_risk, "unresolvedIntentIds").is_array()
            && get(global_risk, "unresolvedIntentIds").as_array().unwrap().len() <= 1
            && get(global_risk, "unresolvedIntentIds")
                .as_array()
                .unwrap()
                .iter()
                .all(|x| x == get(b, "intentId"))
            && instant_v(get(global_risk, "at")).map(|t| t <= now).unwrap_or(false)
            && now - instant_v(get(global_risk, "at")).unwrap_or(i64::MAX) <= 20000,
        "GLOBAL_UNKNOWN_OR_OWNER",
    )?;
    need(account_activity.is_null(), "GLOBAL_ACTIVITY_UNSUPPORTED")?;
    let funding = amount(reserved_funding.as_str().unwrap_or("")).map(BigInt::from)?;
    let other = amount(reserved_elsewhere.as_str().unwrap_or("")).map(BigInt::from)?;
    need(
        funding >= BigInt::from(0)
            && other >= BigInt::from(0)
            && (get(b, "action").as_str() != Some("buy")
                || funding >= BigInt::from(limit) * js_int(get(b, "quantity")).map(BigInt::from).unwrap_or_default()),
        "FULL_FUNDING_RESERVATION",
    )?;
    let default_zero = json!("0.000000");
    let pre = account(
        before,
        b,
        get(b, "createdAt"),
        if get(b, "action").as_str() == Some("buy") { &reserved_funding } else { &default_zero },
        &reserved_elsewhere,
    )?;
    let post = account(after, b, read_at, &default_zero, &default_zero)?;
    projection_progress(before, after)?;
    if !prior_after.is_null() {
        projection_progress(&prior_after, before)?;
    }
    need(
        (get(b, "action").as_str() == Some("sell") || get(&pre, "funded") == &json!(true))
            && instant_v(get(&pre, "completedAt"))? <= created
            && instant_v(get(&post, "startedAt"))? >= created,
        "ACCOUNT_CAUSALITY_OR_FUNDING",
    )?;
    let prior = prior_inventory(&prior_proof, &prior_after, &pre, b)?;
    need(
        if get(b, "action").as_str() == Some("buy") {
            js_int(&json!(prior.quantity + js_int(get(b, "quantity")).unwrap_or(0))).is_some()
        } else {
            !prior_proof.is_null()
                && js_int(get(b, "quantity")).unwrap_or(i64::MAX) <= prior.quantity
        },
        "OWNED_QUANTITY",
    )?;
    let request_body = json!({
        "idempotencyKey": get(b, "idempotencyKey"),
        "exchangeId": get(b, "exchangeId"),
        "side": "no",
        "action": get(b, "action"),
        "quantity": get(b, "quantity"),
        "price": get(b, "limit").as_str().and_then(|s| s.parse::<f64>().ok()).map(V::from).unwrap_or(V::Null),
        "expirationDate": get(b, "expirationDate"),
        "tournamentId": get(b, "tournamentId"),
    });
    let body_json = canonical_payload(&request_body)?;
    let body_hash = payload_hash(&request_body)?;
    let wire = dispatch_receipt;
    keys(
        wire,
        &[
            "method", "endpoint", "requestBodyJson", "requestBodyHash", "fetchInvocationId",
            "requestStartedAt", "ownerId", "runId", "tournamentId", "intentId", "idempotencyKey",
            "leaseIdentity", "leaseHash", "tournamentSlug", "marketId", "exchangeId", "kind",
        ],
    )?;
    // JS `wire.kind!==undefined`: an explicit null kind is checked and fails.
    if let Some(kind) = wire.get("kind") {
        need(kind == &json!("place"), "DISPATCH_OPERATION")?;
    }
    for k in ["marketId", "exchangeId", "tournamentSlug"] {
        if let Some(v) = wire.get(k) {
            need(v == get(b, k), "DISPATCH_SCOPE")?;
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
            && get(&a.source, "requestStartedAt") == get(wire, "requestStartedAt")
            && a.start >= created
            && a.start < expires,
        "ORIGINAL_FETCH_PROVENANCE",
    )?;
    need(
        js_string(get(&d, "orderId")) == get(b, "orderId").as_str().unwrap_or("")
            && get(&d, "exchangeId") == get(b, "exchangeId")
            && get(&d, "action") == get(b, "action")
            && get(&d, "side").as_str() == Some("no")
            && get(&d, "open").is_boolean()
            && tick_at(&a, &[json!("price")], get(&d, "price"))? == limit,
        "ACK_CANONICAL",
    )?;
    integer_at(&a, &[json!("orderId")], get(&d, "orderId"), 1, i64::MAX)?;
    integer_at(&a, &[json!("quantity")], get(&d, "quantity"), 1, js_int(get(b, "quantity")).unwrap_or(0))?;
    integer_at(&a, &[json!("quantityTraded")], get(&d, "quantityTraded"), 0, js_int(get(&d, "quantity")).unwrap_or(i64::MAX))?;
    integer_at(&a, &[json!("remainingQuantity")], get(&d, "remainingQuantity"), 0, js_int(get(&d, "quantity")).unwrap_or(i64::MAX))?;
    need(
        js_int(get(&d, "quantityTraded")).unwrap_or(i64::MIN) + js_int(get(&d, "remainingQuantity")).unwrap_or(i64::MIN)
            == js_int(get(&d, "quantity")).unwrap_or(i64::MAX)
            && (get(&d, "open") == &json!(false) || js_int(get(&d, "remainingQuantity")).unwrap_or(0) > 0),
        "ACK_QUANTITY",
    )?;
    let ack_cost = amount_at(&a, &[json!("totalCost")])?;
    need(
        ack_cost >= BigInt::from(0)
            && ack_cost <= BigInt::from(995_000i64) * js_int(get(&d, "quantityTraded")).map(BigInt::from).unwrap_or_default()
            && (if js_int(get(&d, "quantityTraded")).unwrap_or(0) != 0 {
                ack_cost >= BigInt::from(5_000i64) * js_int(get(&d, "quantityTraded")).map(BigInt::from).unwrap_or_default()
            } else {
                ack_cost == BigInt::from(0)
            }),
        "ACK_NOTIONAL",
    )?;
    let ack_economics = neutral_all(&a, &ack_cost, b)?;
    let terminal = read(order, "GET", &format!("/orders/{}", get(b, "orderId").as_str().unwrap_or("")), now, true)?;
    let od = terminal.data.clone();
    need(
        terminal.start >= a.end
            && js_string(get(&od, "id")) == get(b, "orderId").as_str().unwrap_or("")
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
    integer_at(&terminal, &[json!("quantity")], get(&od, "quantity"), 1, js_int(get(&d, "quantity")).unwrap_or(0))?;
    integer_at(&terminal, &[json!("quantityFilled")], get(&od, "quantityFilled"), 0, js_int(get(&od, "quantity")).unwrap_or(i64::MAX))?;
    let server_created = instant_v(get(&od, "createdAt"))?;
    need(
        server_created >= created - 2000 && server_created <= std::cmp::min(a.end, expires) + 2000,
        "SERVER_ORDER_CLOCK",
    )?;
    need(
        fill_pages.is_array()
            && !fill_pages.as_array().unwrap().is_empty()
            && fill_pages.as_array().unwrap().len() <= 10,
        "FILL_PAGES",
    )?;
    let mut cursor: Option<String> = None;
    let mut last_end = terminal.end;
    let mut checkpoint: Option<i64> = None;
    let mut total: Option<i64> = None;
    let mut reported_average = RawAverage::Missing;
    let mut cursors: HashSet<String> = HashSet::new();
    let mut ids: HashSet<i64> = HashSet::new();
    let mut fills: Vec<V> = Vec::new();
    let mut fill_sources: Vec<V> = Vec::new();
    let fill_pages_list = fill_pages.as_array().unwrap().clone();
    for (page_index, raw) in fill_pages_list.iter().enumerate() {
        let endpoint = format!(
            "/orders/{}/fills?limit=200{}",
            get(b, "orderId").as_str().unwrap_or(""),
            cursor
                .as_ref()
                .map(|c| format!("&cursor={}", encode_uri_component(c)))
                .unwrap_or_default()
        );
        let r = read(raw, "GET", &endpoint, now, true)?;
        let v = r.data.clone();
        let p = get(&v, "pagination").clone();
        need(
            r.start >= last_end
                && get(&v, "data").is_array()
                && get(&v, "data").as_array().unwrap().len() <= 200
                && js_string(get(&v, "orderId")) == get(b, "orderId").as_str().unwrap_or("")
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
        need(
            get(&p, "limit") == &json!(200)
                && get(&p, "hasMore").is_boolean()
                && (get(&p, "nextCursor").is_null()
                    || get(&p, "nextCursor").as_str().is_some_and(|s| !s.is_empty() && s.len() <= 1000))
                && get(&get(&v, "coverage"), "complete") == &json!(true)
                && (get(&get(&v, "coverage"), "projectedThroughSequence").is_null()
                    || js_int(get(&get(&v, "coverage"), "projectedThroughSequence")).is_some()),
            "FILL_COVERAGE",
        )?;
        let projected = get(&get(&v, "coverage"), "projectedThroughSequence").clone();
        if checkpoint.is_some() && !projected.is_null() {
            need(
                js_int(&projected).unwrap_or(i64::MIN) >= checkpoint.unwrap_or(0),
                "FILL_PROJECTION_ROLLBACK",
            )?;
        }
        if !projected.is_null() {
            checkpoint = js_int(&projected);
        }
        let av = if get(&v, "avgFillPrice").is_null() {
            RawAverage::Null
        } else {
            match r.numbers.get(&path_key(&[json!("avgFillPrice")])) {
                Some(s) => RawAverage::Str(s.clone()),
                None => RawAverage::Missing,
            }
        };
        if total.is_some() {
            need(
                total == js_int(get(&v, "totalQuantityFilled"))
                    && match (&av, &reported_average) {
                        (RawAverage::Null, RawAverage::Null) => true,
                        (RawAverage::Missing, RawAverage::Missing) => true,
                        (RawAverage::Str(x), RawAverage::Str(y)) => x == y,
                        // JS `av===reportedAverage` — a string never equals
                        // null or undefined.
                        _ => false,
                    },
                "FILL_PROJECTION_CHANGED",
            )?;
        }
        total = js_int(get(&v, "totalQuantityFilled"));
        reported_average = av;
        for (i, f) in get(&v, "data").as_array().unwrap().iter().enumerate() {
            integer_at(&r, &[json!("data"), json!(i), json!("id")], get(f, "id"), 1, i64::MAX)?;
            integer_at(&r, &[json!("data"), json!(i), json!("quantity")], get(f, "quantity"), -100, -1)?;
            let fid = js_int(get(f, "id")).unwrap_or(0);
            need(
                !ids.contains(&fid) && get(f, "side").as_str() == Some("no"),
                "FILL_IDENTITY",
            )?;
            ids.insert(fid);
            let qty = -js_int(get(f, "quantity")).unwrap_or(0);
            let px = tick_at(&r, &[json!("data"), json!(i), json!("price")], get(f, "price"))?;
            let at = instant_v(get(f, "filledAt"))?;
            need(
                at >= server_created
                    && at <= r.end
                    && if get(b, "action").as_str() == Some("buy") {
                        px <= limit
                    } else {
                        px >= limit
                    },
                "FILL_PRICE_OR_TIME",
            )?;
            fills.push(json!({
                "id": fid,
                "orderId": get(b, "orderId"),
                "marketId": get(b, "marketId"),
                "exchangeId": get(b, "exchangeId"),
                "tournamentId": get(b, "tournamentId"),
                "side": "no",
                "action": get(b, "action"),
                "signedQuantity": -qty,
                "quantity": qty,
                "price": bmoney(&BigInt::from(px)),
                "principal": bmoney(&(BigInt::from(px) * BigInt::from(qty))),
                "filledAt": get(f, "filledAt"),
                "source": "rest_order_fills",
                "priceBasis": "side_relative",
            }));
        }
        if page_index < fill_pages_list.len() - 1 {
            need(
                get(&p, "hasMore") == &json!(true)
                    && !get(&p, "nextCursor").is_null()
                    && !cursors.contains(get(&p, "nextCursor").as_str().unwrap_or("")),
                "FILL_CURSOR",
            )?;
            cursors.insert(get(&p, "nextCursor").as_str().unwrap_or("").to_string());
            cursor = Some(get(&p, "nextCursor").as_str().unwrap_or("").to_string());
        } else {
            need(
                get(&p, "hasMore") == &json!(false) && get(&p, "nextCursor").is_null(),
                "FILL_INCOMPLETE",
            )?;
        }
        fill_sources.push(r.source.clone());
    }
    fills.sort_by(|x, y| {
        instant_v(get(x, "filledAt"))
            .unwrap_or(0)
            .cmp(&instant_v(get(y, "filledAt")).unwrap_or(0))
            .then(js_int(get(x, "id")).unwrap_or(0).cmp(&js_int(get(y, "id")).unwrap_or(0)))
    });
    let prior_lof = get(&prior_proof, "lastObservedFill").clone();
    if !prior_lof.is_null() && !fills.is_empty() {
        let first = &fills[0];
        need(
            instant_v(get(first, "filledAt"))? > instant_v(get(&prior_lof, "filledAt"))?
                || (get(first, "filledAt") == get(&prior_lof, "filledAt")
                    && js_int(get(first, "id")).unwrap_or(0) > js_int(get(&prior_lof, "fillId")).unwrap_or(i64::MAX)),
            "CONTINUATION_FILL_CLOCK",
        )?;
    }
    let quantity: i64 = fills.iter().map(|f| js_int(get(f, "quantity")).unwrap_or(0)).sum();
    let principal: BigInt = fills
        .iter()
        .map(|f| get(f, "principal").as_str().and_then(|s| amount(s).ok()).map(BigInt::from).unwrap_or_default())
        .fold(BigInt::from(0), |x, y| x + y);
    need(
        quantity <= js_int(get(&od, "quantity")).unwrap_or(i64::MAX)
            && total == Some(-quantity)
            && js_int(get(&od, "quantityFilled")).unwrap_or(i64::MIN) == quantity
            && js_int(get(&d, "quantityTraded")).unwrap_or(i64::MAX) <= quantity
            && (get(&d, "open") == &json!(false)
                || js_int(get(&d, "quantityTraded")).unwrap_or(-1) == quantity),
        "COMPLETE_FILL_TOTAL",
    )?;
    let average = average_check(&reported_average, &principal, quantity)?;
    let mut ack_quantity = js_int(get(&d, "quantityTraded")).unwrap_or(0);
    let mut ack_principal = BigInt::from(0);
    let mut last_ack_price: Option<BigInt> = None;
    for f in &fills {
        let q = std::cmp::min(ack_quantity, js_int(get(f, "quantity")).unwrap_or(0));
        if q != 0 {
            need(instant_v(get(f, "filledAt"))? <= a.end, "ACK_FILL_CLOCK")?;
            ack_principal += amount(get(f, "price").as_str().unwrap_or("")).map(BigInt::from)? * BigInt::from(q);
            last_ack_price = Some(amount(get(f, "price").as_str().unwrap_or("")).map(BigInt::from)?);
            ack_quantity -= q;
        }
    }
    need(ack_quantity == 0 && ack_principal == ack_cost, "ACK_FILL_NOTIONAL")?;
    let mut ack_fill_price_summary = V::Null;
    if js_int(get(&d, "quantityTraded")).unwrap_or(0) != 0 {
        let mut matches: Vec<String> = Vec::new();
        let raw_average = a
            .numbers
            .get(&path_key(&[json!("fillPrice")]))
            .cloned()
            .map(RawAverage::Str)
            .unwrap_or(RawAverage::Missing);
        if let Ok(px) = tick_at(&a, &[json!("fillPrice")], get(&d, "fillPrice")) {
            if last_ack_price.as_ref() == Some(&BigInt::from(px)) {
                matches.push("last_ack_prefix_fill_tick".into());
            }
        }
        if average_check(&raw_average, &ack_cost, js_int(get(&d, "quantityTraded")).unwrap_or(0)).is_ok() {
            matches.push("ack_prefix_weighted_average".into());
        }
        need(!matches.is_empty(), "ACK_FILL_PRICE_SUMMARY")?;
        ack_fill_price_summary = json!({
            "reportedDecimal": raw_average.as_option().map(V::from).unwrap_or(V::Null),
            "matches": matches,
            "principalDerivedFromSummary": false,
        });
    } else {
        need(get(&d, "fillPrice").is_null(), "ACK_UNFILLED_PRICE")?;
    }
    need(
        instant_v(get(&post, "startedAt"))? >= last_end
            && instant_v(get(global_risk, "at"))? >= instant_v(get(&post, "completedAt"))?,
        "POST_ACCOUNT_CAUSALITY",
    )?;
    let prior_history = history(&pre);
    let after_history = history(&post);
    let oid = js_int(get(b, "orderId")).unwrap_or(-1);
    need(
        !prior_history.iter().any(|o| js_int(get(o, "id")) == Some(oid))
            && same(
                &json!(prior_history),
                &json!(after_history.iter().filter(|o| js_int(get(o, "id")) != Some(oid)).cloned().collect::<Vec<V>>()),
            )?,
        "EXTERNAL_SCOPED_ACTIVITY",
    )?;
    let final_order = after_history
        .iter()
        .find(|o| js_int(get(o, "id")) == Some(oid))
        .cloned()
        .unwrap_or(V::Null);
    need(!final_order.is_null(), "SCOPED_ORDER_MISSING")?;
    for k in [
        "id", "exchangeId", "tournamentId", "side", "action", "quantity", "quantityFilled",
        "priceLimit", "open", "createdAt", "expirationDate",
    ] {
        need(same(get(&final_order, k), get(&od, k))?, "SCOPED_ORDER_MISMATCH")?;
    }
    let signed_delta = if get(b, "action").as_str() == Some("buy") { -quantity } else { quantity };
    let remaining_quantity = prior.quantity
        + if get(b, "action").as_str() == Some("buy") { quantity } else { -quantity };
    need(
        target(&post, b)? == -remaining_quantity
            && target(&post, b)? - target(&pre, b)? == signed_delta
            && same(&json!(other_positions(&pre, b)), &json!(other_positions(&post, b)))?,
        "POSITION_DELTA_OR_EXTERNAL_ACTIVITY",
    )?;
    let cash_delta = if get(b, "action").as_str() == Some("buy") {
        -principal.clone()
    } else {
        principal.clone()
    };
    let delta_low = amount(get(&post, "cashLower").as_str().unwrap_or("")).map(BigInt::from)?
        - amount(get(&pre, "cashUpper").as_str().unwrap_or("")).map(BigInt::from)?;
    let delta_high = amount(get(&post, "cashUpper").as_str().unwrap_or("")).map(BigInt::from)?
        - amount(get(&pre, "cashLower").as_str().unwrap_or("")).map(BigInt::from)?;
    need(cash_delta >= delta_low && cash_delta <= delta_high, "CASH_INTERVAL_INCONSISTENT")?;
    let mut new_receipts: Vec<V> = get(&pre, "sourceReceipts").as_array().cloned().unwrap_or_default();
    new_receipts.push(a.source.clone());
    new_receipts.push(terminal.source.clone());
    new_receipts.extend(fill_sources.iter().cloned());
    new_receipts.extend(get(&post, "sourceReceipts").as_array().cloned().unwrap_or_default());
    let unique: HashSet<String> = new_receipts
        .iter()
        .map(|r| get(r, "fetchInvocationId").as_str().unwrap_or("\u{1}").to_string())
        .collect();
    need(unique.len() == new_receipts.len(), "REUSED_RECEIPT_INVOCATION")?;
    let mut lots = prior.lots.clone();
    let mut removed_lots: Vec<V> = Vec::new();
    let mut origins = get(&prior_proof, "attributedBuyFills")
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut removed_cost = BigInt::from(0);
    if get(b, "action").as_str() == Some("buy") {
        for f in &fills {
            let lot = json!({
                "lotId": format!("{}:{}", get(b, "orderId").as_str().unwrap_or(""), js_int(get(f, "id")).unwrap_or(0)),
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
        while left != 0 {
            need(!lots.is_empty(), "FIFO_OVERSELL")?;
            let lot = lots[0].clone();
            let q = std::cmp::min(left, js_int(get(&lot, "quantity")).unwrap_or(0));
            let lot_price = price(get(&lot, "entryPrice").as_str().unwrap_or(""))?;
            let cost = BigInt::from(lot_price) * BigInt::from(q);
            removed_cost += &cost;
            let mut removed = lot.clone();
            if let V::Object(m) = &mut removed {
                m.insert("quantity".into(), json!(q));
                m.insert("principalCost".into(), json!(bmoney(&cost)));
            }
            removed_lots.push(removed);
            left -= q;
            let new_qty = js_int(get(&lot, "quantity")).unwrap_or(0) - q;
            if new_qty == 0 {
                lots.remove(0);
            } else {
                if let V::Object(m) = &mut lots[0] {
                    m.insert("quantity".into(), json!(new_qty));
                    m.insert(
                        "principalCost".into(),
                        json!(bmoney(&(BigInt::from(lot_price) * BigInt::from(new_qty)))),
                    );
                }
            }
        }
    }
    lots.sort_by(|x, y| {
        instant_v(get(x, "openedAt"))
            .unwrap_or(0)
            .cmp(&instant_v(get(y, "openedAt")).unwrap_or(0))
            .then(js_int(get(x, "fillId")).unwrap_or(0).cmp(&js_int(get(y, "fillId")).unwrap_or(0)))
    });
    need(origins.len() <= 10000, "ATTRIBUTION_SIZE")?;
    let realized = if get(b, "action").as_str() == Some("sell") {
        &principal - &removed_cost
    } else {
        BigInt::from(0)
    };
    let remaining_cost: BigInt = lots.iter().fold(BigInt::from(0), |n, l| {
        n + get(l, "principalCost")
            .as_str()
            .and_then(|s| amount(s).ok())
            .map(BigInt::from)
            .unwrap_or_default()
    });
    let last_observed_fill = if let Some(f) = fills.last() {
        json!({
            "filledAt": get(f, "filledAt"),
            "fillId": get(f, "id"),
        })
    } else {
        get(&prior_proof, "lastObservedFill").clone()
    };
    let wire_hash = payload_hash(wire)?;
    let mut wire_with_hash = dispatch_receipt.clone();
    wire_with_hash
        .as_object_mut()
        .map(|m| m.insert("dispatchRecordHash".into(), json!(wire_hash)));
    // Built imperatively in JS literal order (the json! literal hits the
    // macro recursion limit).
    let mut result = json!({});
    {
        let m = result.as_object_mut().unwrap();
        let buy = get(b, "action").as_str() == Some("buy");
        m.insert("schemaVersion".into(), json!(1));
        m.insert("mode".into(), json!("scoped_rest_no_multishare_fifo_v1"));
        m.insert("readAt".into(), read_at.clone());
        m.insert("ownerId".into(), get(b, "ownerId").clone());
        m.insert("runId".into(), get(b, "runId").clone());
        m.insert("tournamentId".into(), get(b, "tournamentId").clone());
        m.insert("marketId".into(), get(b, "marketId").clone());
        m.insert("exchangeId".into(), get(b, "exchangeId").clone());
        m.insert("orderId".into(), get(b, "orderId").clone());
        m.insert("intentId".into(), get(b, "intentId").clone());
        m.insert("action".into(), get(b, "action").clone());
        m.insert("side".into(), json!("no"));
        m.insert("bindingHash".into(), json!(payload_hash(b)?));
        m.insert("priorProofHash".into(), get(&prior_proof, "verificationHash").clone());
        m.insert("scopedTradeVerified".into(), json!(true));
        m.insert("requestedQuantity".into(), get(b, "quantity").clone());
        m.insert("canonicalAcceptedQuantity".into(), get(&d, "quantity").clone());
        m.insert(
            "admissionDiscardedQuantity".into(),
            json!(js_int(get(b, "quantity")).unwrap_or(0) - js_int(get(&d, "quantity")).unwrap_or(0)),
        );
        m.insert("terminalReportedQuantity".into(), get(&od, "quantity").clone());
        m.insert(
            "terminalReportedUnfilledQuantity".into(),
            json!(js_int(get(&od, "quantity")).unwrap_or(0) - quantity),
        );
        m.insert("filledQuantity".into(), json!(quantity));
        m.insert(
            "terminalUnfilledQuantity".into(),
            json!(js_int(get(b, "quantity")).unwrap_or(0) - quantity),
        );
        m.insert("signedPositionDelta".into(), json!(signed_delta));
        m.insert(
            "scopedPrincipalCost".into(),
            if buy { json!(bmoney(&principal.clone())) } else { json!("0.000000") },
        );
        m.insert(
            "scopedProceeds".into(),
            if !buy { json!(bmoney(&principal.clone())) } else { json!("0.000000") },
        );
        m.insert("fillNotional".into(), json!(bmoney(&principal.clone())));
        m.insert("averageFillPrice".into(), average.clone().unwrap_or(V::Null));
        m.insert("removedPrincipalCost".into(), json!(bmoney(&removed_cost.clone())));
        m.insert("realizedPnl".into(), json!(bmoney(&realized.clone())));
        m.insert(
            "cumulativeRealizedPnl".into(),
            json!(bmoney(&(&prior.cumulative_realized + &realized))),
        );
        m.insert("remainingOwnedQuantity".into(), json!(remaining_quantity));
        m.insert("remainingOwnedCost".into(), json!(bmoney(&remaining_cost.clone())));
        m.insert("remainingLots".into(), V::Array(lots.clone()));
        m.insert("removedLots".into(), V::Array(removed_lots.clone()));
        m.insert("attributedBuyFills".into(), V::Array(origins.clone()));
        m.insert("fills".into(), V::Array(fills.clone()));
        m.insert("lastObservedFill".into(), last_observed_fill.clone());
        m.insert("requestedLimit".into(), json!(bmoney(&BigInt::from(limit))));
        m.insert("requestedExpirationDate".into(), get(b, "expirationDate").clone());
        m.insert(
            "postExpiryFillObserved".into(),
            json!(fills.iter().any(|f| instant_v(get(f, "filledAt")).map(|t| t > expires).unwrap_or(false))),
        );
        m.insert("canonicalAckFilledQuantity".into(), get(&d, "quantityTraded").clone());
        m.insert("ackNotional".into(), json!(bmoney(&ack_cost.clone())));
        m.insert(
            "ackCoversFinalFill".into(),
            json!(js_int(get(&d, "quantityTraded")).unwrap_or(-1) == quantity),
        );
        m.insert("ackFillPriceSummary".into(), ack_fill_price_summary.clone());
        m.insert("ackEconomics".into(), ack_economics.clone());
        m.insert("fee".into(), json!("0.000000"));
        m.insert("expectedCanonicalTradeCashDelta".into(), json!(bmoney(&cash_delta.clone())));
        m.insert(
            "accountCash".into(),
            json!({
                "basis": "rounded_cent_plus_minus_one_cent",
                "before": {"lower": get(&pre, "cashLower"), "upper": get(&pre, "cashUpper")},
                "after": {"lower": get(&post, "cashLower"), "upper": get(&post, "cashUpper")},
                "observedDeltaInterval": {"lower": bmoney(&delta_low), "upper": bmoney(&delta_high)},
                "consistent": true,
                "exactDelta": V::Null,
            }),
        );
        m.insert(
            "funding".into(),
            json!({
                "reservedFunding": reserved_funding,
                "reservedElsewhere": reserved_elsewhere,
                "fullyFundedAtSubmission": if buy { json!(true) } else { V::Null },
                "inventoryCoveredAtSubmission": if !buy { json!(true) } else { V::Null },
            }),
        );
        m.insert(
            "inventory".into(),
            json!({
                "priorSignedQuantity": -prior.quantity,
                "remainingSignedQuantity": -remaining_quantity,
                "unbackedSell": false,
                "foreignLotsAdopted": false,
            }),
        );
        m.insert(
            "sourceReceipts".into(),
            json!({
                "before": get(&pre, "sourceReceipts"),
                "dispatch": wire_with_hash,
                "ack": a.source,
                "terminal": terminal.source,
                "fills": fill_sources,
                "after": get(&post, "sourceReceipts"),
            }),
        );
        m.insert(
            "globalIntentCensus".into(),
            json!({
                "sourceHash": get(global_risk, "sourceHash"),
                "at": get(global_risk, "at"),
                "unresolvedIntentIds": get(global_risk, "unresolvedIntentIds"),
            }),
        );
        m.insert(
            "globalActivity".into(),
            json!({"status": "unknown", "independentlyComplete": false}),
        );
        m.insert("globalCashReconciledExactly".into(), json!(false));
        m.insert("globalAccountActivityVerified".into(), json!(false));
        m.insert("authoritativeReconciliation".into(), json!(false));
        m.insert("executionAuthority".into(), json!(false));
        m.insert("newSubmissionAuthorized".into(), json!(false));
    }
    let hash = payload_hash(&result)?;
    result
        .as_object_mut()
        .map(|m: &mut Map<String, V>| m.insert("verificationHash".into(), json!(hash)));
    Ok(result)
}
