// execution/baal-scaled-profitable-exit-signal-v2.mjs — a separately named
// policy proposal. It grants no writer/activation authority.
//
// Port notes:
// - JS BigInt money -> i128 micro units via baal_service_runtime::fixed;
//   decimalRational numerators -> num_bigint::BigInt.
// - `String(x)` on a JSON number -> js_stringify (same shortest-roundtrip
//   spelling); on a JSON string it is the string itself.
// - parseNumberLexemes returns (data, numbers); numbers is the raw-lexeme map
//   keyed by JSON.stringify(path).
// - `x ?? default` destructure defaults: absent OR explicit null.
// - catch: only SCALED_PROFIT_EXIT_* codes surface; anything else becomes
//   'INVALID_OR_INCOMPLETE_EXIT_EVIDENCE' with holdExistingInventory:true.
use crate::baal_scaled_profitable_fifo_prefix_v3::size_profitable_owned_no_exit_prefix_v3;
use crate::core::payload_hash;
use crate::exact_money_receipts::parse_number_lexemes;
use crate::gates::normalize_official_fill_tick;
use crate::no_rest_sell_reconciliation_v2::inspect_scoped_rest_no_account;
use baal_service_runtime::contract::{instant_ns, ms_to_iso};
use baal_service_runtime::fixed::{amount, instant, money, price, SCALE, TICK};
use baal_service_runtime::json_text::{js_int, js_stringify, sha256};
use baal_service_runtime::market::normalize_book;
use num_bigint::BigInt;
use serde_json::{json, Map, Value as V};
use std::collections::{BTreeMap, BTreeSet};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("SCALED_PROFIT_EXIT_{code}"))
    }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn sget<'a>(v: &'a V, k: &str) -> Option<&'a str> {
    v.get(k).and_then(V::as_str)
}
fn hex(x: &V) -> bool {
    x.as_str()
        .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
}
fn sha_str(s: &str) -> String {
    sha256(s.as_bytes())
}
fn same(a: &V, b: &V) -> Result<bool, String> {
    Ok(payload_hash(a)? == payload_hash(b)?)
}
fn instant_v(v: &V) -> Result<i64, String> {
    instant(v.as_str().ok_or_else(|| "INVALID_INSTANT".to_string())?)
}
// sort(a=>[...a].sort((a,b)=>a.id-b.id)) — JS NaN comparator keeps order.
fn sort_by_id(a: &[V]) -> Vec<V> {
    let mut x: Vec<V> = a.to_vec();
    x.sort_by(|p, q| {
        get(p, "id")
            .as_f64()
            .partial_cmp(&get(q, "id").as_f64())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    x
}
fn source_of(r: &V) -> V {
    json!({
        "method": get(r, "method"),
        "endpoint": get(r, "endpoint"),
        "sourceBodyHash": get(r, "sourceBodyHash"),
        "requestStartedAt": get(r, "requestStartedAt"),
        "receivedAt": get(r, "receivedAt"),
        "fetchInvocationId": get(r, "fetchInvocationId"),
    })
}
// JS: raw(r, endpoint, at) checks r.endpoint === endpoint. Callers pass
// r.endpoint itself as the endpoint, so a missing endpoint compares
// undefined === undefined (true). Take the endpoint as a raw V.
fn raw(r: &V, endpoint: &V, at: i64) -> Result<(V, BTreeMap<String, String>), String> {
    need(
        get(r, "method").as_str() == Some("GET")
            && get(r, "endpoint") == endpoint
            && js_int(get(r, "status")) == Some(200)
            && get(r, "bodyText").as_str().is_some_and(|s| s.len() <= 8 * 1024 * 1024)
            && get(r, "sourceBodyHash").as_str()
                == Some(sha_str(get(r, "bodyText").as_str().unwrap_or("")).as_str())
            && get(r, "fetchInvocationId").as_str().is_some_and(|s| !s.is_empty()),
        "RAW_SOURCE",
    )?;
    let start = instant_v(get(r, "requestStartedAt"))?;
    let end = instant_v(get(r, "receivedAt"))?;
    need(start <= end && end <= at && at - start <= 20000, "SOURCE_AGE")?;
    parse_number_lexemes(get(r, "bodyText").as_str().unwrap())
}

// /^(0|[1-9]\d*)(?:\.(\d+))?(?:[eE]([+-]?\d+))?$/ — returns
// (whole_end, fraction_range, exponent_range) or None.
fn match_decimal(t: &str) -> Option<(usize, Option<(usize, usize)>, Option<(usize, usize)>)> {
    let b = t.as_bytes();
    let n = b.len();
    let mut j = 0usize;
    if j < n && b[j] == b'0' {
        j += 1;
    } else if j < n && (b'1'..=b'9').contains(&b[j]) {
        j += 1;
        while j < n && b[j].is_ascii_digit() {
            j += 1;
        }
    } else {
        return None;
    }
    let whole_end = j;
    let mut fraction = None;
    if j < n && b[j] == b'.' {
        j += 1;
        let s = j;
        while j < n && b[j].is_ascii_digit() {
            j += 1;
        }
        if j == s {
            return None;
        }
        fraction = Some((s, j));
    }
    let mut exponent = None;
    if j < n && (b[j] == b'e' || b[j] == b'E') {
        j += 1;
        let s = j;
        if j < n && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        let ds = j;
        while j < n && b[j].is_ascii_digit() {
            j += 1;
        }
        if j == ds {
            return None;
        }
        exponent = Some((s, j));
    }
    if j != n {
        return None;
    }
    Some((whole_end, fraction, exponent))
}
// decimalRational(text): {numerator, scale} — exact decimal with trailing
// zeros stripped, exponent folded in.
fn decimal_rational(text: &V) -> Result<(BigInt, i64), String> {
    need(
        text.is_string() && text.as_str().unwrap_or("").len() <= 128,
        "EXACT_PRICE_SOURCE",
    )?;
    let t = text.as_str().unwrap_or("").to_string();
    need(match_decimal(&t).is_some(), "EXACT_PRICE_SOURCE")?;
    let (whole_end, fraction, exponent) = match_decimal(&t).unwrap();
    let exponent: i64 = exponent
        .map(|(s, e)| t[s..e].parse().unwrap_or(i64::MAX))
        .unwrap_or(0);
    need(exponent.checked_abs().unwrap_or(i64::MAX) <= 128, "EXACT_PRICE_SOURCE")?;
    let digits = format!(
        "{}{}",
        &t[..whole_end],
        fraction.map(|(s, e)| &t[s..e]).unwrap_or("")
    );
    let mut numerator = BigInt::parse_bytes(digits.as_bytes(), 10)
        .ok_or_else(|| "SCALED_PROFIT_EXIT_EXACT_PRICE_SOURCE".to_string())?;
    let mut scale = fraction.map(|(s, e)| (e - s) as i64).unwrap_or(0) - exponent;
    if scale < 0 {
        numerator *= BigInt::from(10).pow((-scale) as u32);
        scale = 0;
    }
    while scale > 0 && numerator.clone() % 10 == BigInt::from(0) {
        numerator /= 10;
        scale -= 1;
    }
    Ok((numerator, scale))
}
fn same_rational(a: &(BigInt, i64), b: &(BigInt, i64)) -> bool {
    a.1 == b.1 && a.0 == b.0
}
// String(value): JS ToString. Strings pass through; numbers take the
// shortest-roundtrip spelling; objects become "[object Object]".
fn js_string(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Null => "null".to_string(),
        V::Bool(b) => b.to_string(),
        V::Number(_) => js_stringify(v),
        V::Object(_) => "[object Object]".to_string(),
        V::Array(a) => a
            .iter()
            .map(|x| match x {
                V::Null => String::new(),
                other => js_string(other),
            })
            .collect::<Vec<String>>()
            .join(","),
    }
}
fn exact_price_lexeme(text: &V, value: &V) -> Result<String, String> {
    let p = normalize_official_fill_tick(value)?;
    let p_price = sget(&p, "price").unwrap_or("").to_string();
    let r = decimal_rational(text)?;
    let same = same_rational(&r, &decimal_rational(&json!(p_price))?)
        || same_rational(&r, &decimal_rational(&json!(js_string(value)))?);
    need(same, "EXACT_PRICE_SOURCE")?;
    Ok(p_price)
}
// /^-?(0|[1-9]\d*)(\.0+)?$/ on the raw lexeme.
fn int_lexeme_ok(x: &str) -> bool {
    let t = x.strip_prefix('-').unwrap_or(x);
    let (w, f) = match t.split_once('.') {
        Some((w, f)) => (w, Some(f)),
        None => (t, None),
    };
    !w.is_empty()
        && (w == "0" || !w.starts_with('0'))
        && w.bytes().all(|b| b.is_ascii_digit())
        && f.is_none_or(|f| !f.is_empty() && f.bytes().all(|b| b == b'0'))
}
// integer(numbers,path,n,min): lexeme /^-?(0|[1-9]\d*)(\.0+)?$/ and
// BigInt(lexeme.split('.')[0]) === BigInt(n) — the sign is part of the
// compared integer.
fn integer(numbers: &BTreeMap<String, String>, path: &V, n: &V, min: i64) -> Result<(), String> {
    let key = js_stringify(path);
    let x = numbers.get(&key);
    need(
        x.is_some_and(|x| {
            x.len() <= 128
                && int_lexeme_ok(x)
                && js_int(n).is_some_and(|n| {
                    n >= min
                        && BigInt::parse_bytes(
                            x.split('.').next().unwrap_or("").as_bytes(),
                            10,
                        ) == Some(BigInt::from(n))
                })
        }),
        "EXACT_INTEGER",
    )
}

fn account_view(a: &V, b: &V, at: i64) -> Result<V, String> {
    let view = inspect_scoped_rest_no_account(&json!({
        "account": a,
        "binding": b,
        "readAt": ms_to_iso(at as f64),
    }))?;
    // JS [a.cash, a.positions, ...a.openOrdersPages, ...a.scopedOrdersPages]:
    // a non-array page list throws TypeError -> INVALID fallback.
    let mut receipts: Vec<V> = vec![get(a, "cash").clone(), get(a, "positions").clone()];
    for k in ["openOrdersPages", "scopedOrdersPages"] {
        match get(a, k).as_array() {
            Some(p) => receipts.extend(p.iter().cloned()),
            None => return Err("TypeError".to_string()),
        }
    }
    for r in &receipts {
        raw(r, get(r, "endpoint"), at)?;
    }
    let (pd, pnumbers) = parse_number_lexemes(sget(get(a, "positions"), "bodyText").unwrap_or(""))?;
    // p.data.positions.entries() — non-array throws TypeError in JS.
    let positions = get(&pd, "positions")
        .as_array()
        .ok_or_else(|| "TypeError".to_string())?;
    for (i, v) in positions.iter().enumerate() {
        integer(&pnumbers, &json!(["positions", i, "quantity"]), get(v, "quantity"), -9_007_199_254_740_991)?;
    }
    for r in get(a, "scopedOrdersPages").as_array().cloned().unwrap_or_default() {
        let (d, numbers) = parse_number_lexemes(sget(&r, "bodyText").unwrap_or(""))?;
        // p.data.data.entries() — non-array throws TypeError in JS.
        let data = get(&d, "data")
            .as_array()
            .ok_or_else(|| "TypeError".to_string())?;
        for (i, o) in data.iter().enumerate() {
            for k in ["id", "quantity", "quantityFilled"] {
                integer(
                    &numbers,
                    &json!(["data", i, k]),
                    get(o, k),
                    if k == "quantityFilled" { 0 } else { 1 },
                )?;
            }
            need(
                get(o, "open") == &V::Bool(false)
                    && js_int(get(o, "quantityFilled")).unwrap_or(i64::MAX)
                        <= js_int(get(o, "quantity")).unwrap_or(i64::MIN),
                "SCOPED_TERMINAL_HISTORY",
            )?;
        }
    }
    let mut out = view.as_object().cloned().unwrap_or_default();
    out.insert(
        "sourceReceipts".into(),
        json!(receipts.iter().map(source_of).collect::<Vec<V>>()),
    );
    Ok(V::Object(out))
}

fn owned_source(scope: &V, owned: &V, current: &V, at: i64) -> Result<(V, V), String> {
    let p = get(owned, "proof").clone();
    let verification_hash = get(&p, "verificationHash").clone();
    let mut body = p.as_object().cloned().unwrap_or_default();
    body.shift_remove("verificationHash");
    need(
        get(&p, "mode").as_str() == Some("scoped_rest_no_multishare_fifo_v1")
            && get(&p, "scopedTradeVerified") == &V::Bool(true)
            && get(&p, "executionAuthority") == &V::Bool(false)
            && get(&p, "side").as_str() == Some("no")
            && hex(&verification_hash)
            && payload_hash(&V::Object(body))? == verification_hash.as_str().unwrap_or(""),
        "OWNED_PROOF",
    )?;
    for k in ["ownerId", "runId", "tournamentId"] {
        need(get(&p, k) == get(scope, k), "OWNED_SCOPE")?;
    }
    need(
        sget(scope, "ownerId") == Some("Codex")
            && get(scope, "exchanges").as_array().is_some_and(|ms| {
                ms.iter().any(|m| {
                    get(m, "marketId") == get(&p, "marketId")
                        && get(m, "exchangeId") == get(&p, "exchangeId")
                })
            }),
        "MARKET_SCOPE",
    )?;
    let mut binding = scope.as_object().cloned().unwrap_or_default();
    binding.insert("marketId".into(), get(&p, "marketId").clone());
    binding.insert("exchangeId".into(), get(&p, "exchangeId").clone());
    let binding = V::Object(binding);
    let old = account_view(get(owned, "after"), &binding, instant_v(get(&p, "readAt"))?)?;
    let now = account_view(current, &binding, at)?;
    need(
        same(get(&old, "sourceReceipts"), get(get(&p, "sourceReceipts"), "after"))?
            && instant_v(get(&p, "readAt"))? <= instant_v(get(&now, "startedAt"))?
            && same(
                &json!(sort_by_id(
                    get(&old, "scopedOrders").as_array().cloned().unwrap_or_default().as_slice()
                )),
                &json!(sort_by_id(
                    get(&now, "scopedOrders").as_array().cloned().unwrap_or_default().as_slice()
                )),
            )?,
        "OWNED_HISTORY_OR_SOURCE",
    )?;
    for a in [&old, &now] {
        let target = get(a, "positions")
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .find(|v| get(v, "exchangeId") == get(&p, "exchangeId"));
        need(
            target.as_ref().is_some_and(|t| {
                get(t, "marketId") == get(&p, "marketId")
                    && get(t, "settled") == &V::Bool(false)
                    && js_int(get(t, "quantity"))
                        == Some(-js_int(get(&p, "remainingOwnedQuantity")).unwrap_or(i64::MAX))
            }),
            "OWNED_SIGNED_QUANTITY",
        )?;
    }
    need(
        get(&p, "remainingLots")
            .as_array()
            .is_some_and(|ls| !ls.is_empty() && ls.len() <= 10000)
            && js_int(get(&p, "remainingOwnedQuantity")).is_some_and(|q| q > 0)
            && get(&p, "attributedBuyFills").is_array(),
        "FIFO_LOTS",
    )?;
    let remaining_owned = js_int(get(&p, "remainingOwnedQuantity")).unwrap_or(0);
    let mut count: i64 = 0;
    let mut cost: i128 = 0;
    let mut last: Option<(i64, i64)> = None;
    let mut ids: BTreeSet<String> = BTreeSet::new();
    for l in get(&p, "remainingLots").as_array().cloned().unwrap_or_default() {
        let lot_id = sget(&l, "lotId").unwrap_or("").to_string();
        let qty = js_int(get(&l, "quantity")).unwrap_or(i64::MIN);
        let orig = js_int(get(&l, "originalQuantity")).unwrap_or(i64::MIN);
        let fill_id = js_int(get(&l, "fillId")).unwrap_or(i64::MIN);
        need(
            sget(&l, "lotId").is_some()
                && !ids.contains(&lot_id)
                && qty > 0
                && orig >= qty
                && orig <= 100
                && fill_id > 0
                && get(&l, "side").as_str() == Some("no"),
            "FIFO_LOT",
        )?;
        ids.insert(lot_id.clone());
        for k in ["ownerId", "runId", "tournamentId", "marketId", "exchangeId"] {
            need(get(&l, k) == get(&p, k), "FIFO_SCOPE")?;
        }
        let px = price(sget(&l, "entryPrice").ok_or_else(|| "INVALID".to_string())?)?;
        let opened = instant_v(get(&l, "openedAt"))?;
        let origin = get(&p, "attributedBuyFills")
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .find(|x| get(x, "lotId") == get(&l, "lotId"));
        let order = get(&old, "scopedOrders")
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .find(|x| js_string(get(x, "id")) == sget(&l, "orderId").unwrap_or(""));
        let mut basis = l.as_object().cloned().unwrap_or_default();
        basis.insert("quantity".into(), get(&l, "originalQuantity").clone());
        basis.insert("principalCost".into(), json!(money(px * orig as i128)));
        need(
            origin.is_some()
                && same(&V::Object(basis), origin.as_ref().unwrap())?
                && get(&l, "principalCost").as_str() == Some(money(px * qty as i128).as_str()),
            "FIFO_BASIS",
        )?;
        need(
            order.as_ref().is_some_and(|o| {
                get(o, "action").as_str() == Some("buy")
                    && get(o, "side").as_str() == Some("no")
                    && js_int(get(o, "quantityFilled")).unwrap_or(i64::MIN) >= orig
            }) && opened
                >= instant_v(get(order.as_ref().unwrap(), "createdAt"))?
                && opened <= instant_v(get(&p, "readAt"))?,
            "FIFO_ORDER_SOURCE",
        )?;
        need(
            last.is_none() || opened > last.unwrap().0 || (opened == last.unwrap().0 && fill_id > last.unwrap().1),
            "FIFO_CHRONOLOGY",
        )?;
        last = Some((opened, fill_id));
        count += qty;
        cost += px * qty as i128;
    }
    need(
        count == remaining_owned
            && money(cost) == sget(&p, "remainingOwnedCost").unwrap_or(""),
        "FIFO_TOTALS",
    )?;
    Ok((p, now))
}

/// planBaalScaledProfitableExitV2({scope,owned,account,bookReceipt,globalRisk,
/// now,protectiveTicks=0,requestedQuantity=null,maximumSharesPerOrder=100,
/// maximumOrderCost='25.000000',feesPerShare='0.000000',
/// previousBookVersion=null})
pub fn plan_baal_scaled_profitable_exit_v2(input: &V) -> V {
    let scope = get(input, "scope");
    let owned = get(input, "owned");
    let account = get(input, "account");
    let book_receipt = get(input, "bookReceipt");
    let global_risk = get(input, "globalRisk");
    let now = get(input, "now");
    let protective_ticks = input.get("protectiveTicks").cloned().unwrap_or(json!(0));
    let requested_quantity = input.get("requestedQuantity").cloned().unwrap_or(V::Null);
    let maximum_shares_per_order = input
        .get("maximumSharesPerOrder")
        .cloned()
        .unwrap_or(json!(100));
    let maximum_order_cost = input
        .get("maximumOrderCost")
        .cloned()
        .unwrap_or(json!("25.000000"));
    let fees_per_share = input
        .get("feesPerShare")
        .cloned()
        .unwrap_or(json!("0.000000"));
    let previous_book_version = input.get("previousBookVersion").cloned().unwrap_or(V::Null);
    let base = json!({
        "schemaVersion": 1,
        "mode": "baal_scaled_limit_protected_fifo_profit_exit_signal_v2",
        "policy": "each_consumed_fifo_lot_plus_one_tick_limit",
        "protectiveTicks": protective_ticks,
        "decisionAt": now,
        "proposalReady": false,
        "orderEligible": false,
        "executionAuthority": false,
        "inventoryChanged": false,
        "actualFillOrRealizedProfit": false,
        "fees": "0.000000",
    });
    match inner(
        scope,
        owned,
        account,
        book_receipt,
        global_risk,
        now,
        &base,
        (
            protective_ticks,
            requested_quantity,
            maximum_shares_per_order,
            maximum_order_cost,
            fees_per_share,
            previous_book_version,
        ),
    ) {
        Ok(v) => v,
        Err(e) => {
            let mut out = base.as_object().cloned().unwrap_or_else(Map::new);
            out.insert(
                "reason".into(),
                json!(if e.len() > "SCALED_PROFIT_EXIT_".len()
                    && e.starts_with("SCALED_PROFIT_EXIT_")
                    && e["SCALED_PROFIT_EXIT_".len()..]
                        .bytes()
                        .all(|b| b.is_ascii_uppercase() || b == b'_')
                {
                    e
                } else {
                    "INVALID_OR_INCOMPLETE_EXIT_EVIDENCE".to_string()
                }),
            );
            out.insert("holdExistingInventory".into(), json!(true));
            V::Object(out)
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn inner(
    scope: &V,
    owned: &V,
    account: &V,
    book_receipt: &V,
    g: &V,
    now: &V,
    base: &V,
    opts: (V, V, V, V, V, V),
) -> Result<V, String> {
    let (protective_ticks, requested_quantity, maximum_shares_per_order, maximum_order_cost,
        fees_per_share, previous_book_version) = opts;
    let at = instant_v(now)?;
    let (p, a) = owned_source(scope, owned, account, at)?;
    need(
        get(g, "ownerId") == get(scope, "ownerId")
            && get(g, "runId") == get(scope, "runId")
            && get(g, "tournamentId") == get(scope, "tournamentId")
            && get(g, "complete") == &V::Bool(true)
            && get(g, "eventScopeComplete") == &V::Bool(true)
            && get(g, "soleCollector") == &V::Bool(true)
            && get(g, "stopped") == &V::Bool(false)
            && get(g, "unknownAcceptance") == &V::Bool(false)
            && get(g, "unresolvedIntentIds").as_array().is_some_and(|x| x.is_empty())
            && hex(get(g, "sourceHash"))
            && instant_v(get(g, "at"))? <= at
            && at - instant_v(get(g, "at"))? <= 20000,
        "GLOBAL_UNKNOWN_OR_PENDING",
    )?;
    let endpoint = format!(
        "/markets/{}/orderbook?tournamentId={}&depth=20",
        sget(&p, "marketId").unwrap_or(""),
        sget(scope, "tournamentId").unwrap_or("")
    );
    let (d, numbers) = raw(book_receipt, &json!(endpoint), at)?;
    let contexts = get(&d, "contexts").as_array().cloned().unwrap_or_default();
    need(
        instant_v(get(book_receipt, "requestStartedAt"))? >= instant_v(get(&a, "completedAt"))?
            && (!d.as_object().map(|m| m.contains_key("marketId")).unwrap_or(false)
                || get(&d, "marketId") == get(&p, "marketId"))
            && contexts.len() == 1
            && get(&contexts[0], "tournament")
                .get("id")
                .and_then(V::as_str)
                == sget(scope, "tournamentId")
            && get(&contexts[0], "orderbook")
                .get("exchanges")
                .and_then(V::as_array)
                .is_some_and(|x| x.len() == 1),
        "ACCOUNT_BEFORE_SCOPED_BOOK",
    )?;
    let e = &contexts[0].get("orderbook").unwrap().get("exchanges").unwrap().as_array().unwrap()[0];
    integer(
        &numbers,
        &json!(["contexts", 0, "orderbook", "exchanges", 0, "asOf", "sequence"]),
        get(get(e, "asOf"), "sequence"),
        0,
    )?;
    for side in ["bids", "asks"] {
        // e[side] ?? [] — nullish -> []; a truthy non-array throws TypeError
        // in JS (for-of on .entries()).
        let side_levels: Vec<V> = match get(e, side) {
            V::Null => Vec::new(),
            v => v
                .as_array()
                .ok_or_else(|| "TypeError".to_string())?
                .clone(),
        };
        for (i, l) in side_levels.iter().enumerate() {
            integer(
                &numbers,
                &json!(["contexts", 0, "orderbook", "exchanges", 0, side, i, "quantity"]),
                get(l, "quantity"),
                1,
            )?;
            let lex = numbers.get(&js_stringify(&V::Array(vec![
                json!("contexts"),
                json!(0),
                json!("orderbook"),
                json!("exchanges"),
                json!(0),
                json!(side),
                json!(i),
                json!("price"),
            ]))).cloned();
            match lex {
                None => need(false, "EXACT_PRICE")?,
                Some(lex) => {
                    // exactPriceLexeme errors (e.g. EXACT_PRICE_SOURCE,
                    // UNRECOGNIZED_API_TICK_REPRESENTATION) propagate, same as
                    // JS; price() failures on the raw value do too.
                    let lp = exact_price_lexeme(&json!(lex), get(l, "price"))?;
                    need(price(&lp)? == price(&js_string(get(l, "price")))?, "EXACT_PRICE")?;
                }
            }
        }
    }
    let b = normalize_book(
        &d,
        sget(&p, "marketId").unwrap_or(""),
        sget(scope, "tournamentId").unwrap_or(""),
        sget(book_receipt, "receivedAt").unwrap_or(""),
        sget(book_receipt, "requestStartedAt").unwrap_or(""),
        20000.0,
        &BTreeSet::new(),
        "authoritative_receipt_v1",
    )?;
    need(
        get(&b, "exchangeId") == get(&p, "exchangeId")
            && get(&b, "bids").as_array().is_some_and(|x| x.len() <= 20)
            && get(&b, "asks").as_array().is_some_and(|x| x.len() <= 20),
        "BOOK_SCOPE",
    )?;
    let version = json!({
        "sequence": get(&b, "sequence"),
        "sourceAt": get(&b, "sourceAt"),
        "versionKey": get(&b, "versionKey"),
        "depthHash": payload_hash(&json!({"bids": get(&b, "bids"), "asks": get(&b, "asks")}))?,
        "requestStartedAt": get(&b, "requestStartedAt"),
        "receivedAt": get(&b, "receivedAt"),
    });
    if !previous_book_version.is_null() {
        need(
            hex(get(&previous_book_version, "depthHash"))
                && sget(&previous_book_version, "versionKey")
                    == Some(
                        format!(
                            "{}:{}",
                            js_string(get(&previous_book_version, "sequence")),
                            js_string(get(&previous_book_version, "sourceAt"))
                        )
                        .as_str(),
                    ),
            "PREVIOUS_VERSION",
        )?;
        let cur_seq = BigInt::parse_bytes(
            js_string(get(&version, "sequence")).as_bytes(),
            10,
        )
        .ok_or_else(|| "SCALED_PROFIT_EXIT_BOOK_ROLLBACK".to_string())?;
        let prev_seq = BigInt::parse_bytes(
            js_string(get(&previous_book_version, "sequence")).as_bytes(),
            10,
        )
        .ok_or_else(|| "SCALED_PROFIT_EXIT_BOOK_ROLLBACK".to_string())?;
        need(
            cur_seq >= prev_seq
                && instant_ns(sget(&version, "sourceAt").unwrap_or("")).map_err(|e| e.0)?
                    >= instant_ns(sget(&previous_book_version, "sourceAt").unwrap_or(""))
                        .map_err(|e| e.0)?
                && instant_v(get(&version, "requestStartedAt"))?
                    >= instant_v(get(&previous_book_version, "requestStartedAt"))?
                && instant_v(get(&version, "receivedAt"))?
                    >= instant_v(get(&previous_book_version, "receivedAt"))?,
            "BOOK_ROLLBACK",
        )?;
        if get(&version, "versionKey") == get(&previous_book_version, "versionKey") {
            need(
                get(&version, "depthHash") == get(&previous_book_version, "depthHash"),
                "BOOK_VERSION_CONFLICT",
            )?;
        }
    }
    let mut candidates: Vec<(V, usize, V)> = get(&b, "asks")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .map(|(index, level)| {
            let sizing = size_profitable_owned_no_exit_prefix_v3(&json!({
                "remainingLots": get(&p, "remainingLots"),
                "remainingOwnedQuantity": get(&p, "remainingOwnedQuantity"),
                "yesAskLevel": level,
                "requestedQuantity": requested_quantity,
                "maximumSharesPerOrder": maximum_shares_per_order,
                "maximumOrderCost": maximum_order_cost,
                "feesPerShare": fees_per_share,
                "protectiveTicks": protective_ticks,
            }));
            (level, index, sizing)
        })
        .filter(|(_, _, sizing)| js_int(get(sizing, "quantity")).unwrap_or(0) > 0)
        .collect();
    // y.quantity - x.quantity || Number(amount(y.limit) - amount(x.limit))
    candidates.sort_by(|x, y| {
        let xq = js_int(get(&x.2, "quantity")).unwrap_or(0);
        let yq = js_int(get(&y.2, "quantity")).unwrap_or(0);
        yq.cmp(&xq).then({
            let xl = amount(sget(&x.2, "protectedLimit").unwrap_or("")).unwrap_or(0);
            let yl = amount(sget(&y.2, "protectedLimit").unwrap_or("")).unwrap_or(0);
            yl.cmp(&xl)
        })
    });
    let chosen = candidates.into_iter().next();
    let expires_at = ms_to_iso(
        std::cmp::min(
            instant_v(get(&a, "startedAt"))? + 20000,
            instant_v(get(book_receipt, "requestStartedAt"))? + 20000,
        ) as f64,
    );
    let mut common = base.as_object().cloned().unwrap_or_default();
    for (k, v) in [
        ("ownerId", get(&p, "ownerId").clone()),
        ("runId", get(&p, "runId").clone()),
        ("tournamentId", get(&p, "tournamentId").clone()),
        ("marketId", get(&p, "marketId").clone()),
        ("exchangeId", get(&p, "exchangeId").clone()),
        ("side", json!("no")),
        ("action", json!("sell")),
        ("ownedProofHash", get(&p, "verificationHash").clone()),
        ("ownedQuantity", get(&p, "remainingOwnedQuantity").clone()),
        ("ownedPrincipalCost", get(&p, "remainingOwnedCost").clone()),
        ("globalRiskSourceHash", get(g, "sourceHash").clone()),
        (
            "sourceReceipts",
            json!({
                "account": get(&a, "sourceReceipts"),
                "book": source_of(book_receipt),
            }),
        ),
        ("bookVersion", version),
        ("expiresAt", json!(expires_at.clone())),
        ("minimumNetProfitPerShare", json!("0.005000")),
        ("depthFractionBps", json!(10000)),
        ("depthPolicy", json!("full_single_observed_rung")),
        ("depthCombinedAcrossLevels", json!(false)),
        (
            "fillContingency",
            json!("A later market move may leave the limit unfilled; only verified actual fills realize profit."),
        ),
    ] {
        common.insert(k.to_string(), v);
    }
    need(instant_v(&json!(expires_at))? > at, "SOURCE_EXPIRED")?;
    let result = match chosen {
        Some((level, index, sizing)) => {
            let mut r = common.clone();
            for (k, v) in [
                ("proposalReady", json!(true)),
                ("reason", json!("PROFITABLE_MAXIMUM_CONTIGUOUS_FIFO_PREFIX")),
                ("quantity", get(&sizing, "quantity").clone()),
                ("limit", get(&sizing, "protectedLimit").clone()),
                ("observedRawNoBid", get(&sizing, "rawObservedNoBid").clone()),
                ("orderNotional", get(&sizing, "orderNotional").clone()),
                (
                    "principalIfFullyFilled",
                    get(&sizing, "removedPrincipalIfFilled").clone(),
                ),
                (
                    "minimumProfitIfFullyFilled",
                    get(&sizing, "protectedNetProfitIfFilled").clone(),
                ),
                ("yesAskSourceLevel", level),
                ("sourceLevelIndex", json!(index)),
                ("prefix", sizing),
            ] {
                r.insert(k.to_string(), v);
            }
            V::Object(r)
        }
        None => {
            let mut r = common.clone();
            r.insert("reason".into(), json!("NO_DEPTH_QUALIFIED_PROFITABLE_FIFO_PREFIX"));
            r.insert("quantity".into(), json!(0));
            r.insert("limit".into(), V::Null);
            V::Object(r)
        }
    };
    let mut out = result.as_object().cloned().unwrap_or_default();
    out.insert("proposalHash".into(), json!(payload_hash(&result)?));
    Ok(V::Object(out))
}
