// execution/baal-scaled-profitable-exit-signal-v2.mjs — a separately named
// policy proposal. It grants no writer/activation authority.
use baal_collector::model::normalize_book;
use baal_execution::core::payload_hash;
use baal_execution::gates::normalize_official_fill_tick;
use baal_execution::receipts::parse_number_lexemes;
use baal_service_runtime::contract::instant_ns;
use baal_service_runtime::fixed::{amount, instant, money, price};
use baal_service_runtime::json_text::{js_int, js_stringify, sha256};
use serde_json::{json, Value as V};
use std::collections::BTreeMap;

fn need(x: bool, c: &str) -> Result<(), String> {
    if x { Ok(()) } else { Err(format!("SCALED_PROFIT_EXIT_{c}")) }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    static NULL: V = V::Null;
    v.as_object().and_then(|m| m.get(k)).unwrap_or(&NULL)
}
fn hex(v: &V) -> bool {
    v.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
}
fn same(a: &V, b: &V) -> Result<bool, String> {
    Ok(payload_hash(a)? == payload_hash(b)?)
}
fn sort(a: &V) -> V {
    let mut v = a.as_array().cloned().unwrap_or_default();
    v.sort_by(|x, y| {
        let nx = get(x, "id").as_f64().or_else(|| get(x, "id").as_str().and_then(|s| s.parse::<f64>().ok()));
        let ny = get(y, "id").as_f64().or_else(|| get(y, "id").as_str().and_then(|s| s.parse::<f64>().ok()));
        match (nx, ny) {
            (Some(a2), Some(b2)) if a2.is_finite() && b2.is_finite() => a2.partial_cmp(&b2).unwrap_or(std::cmp::Ordering::Equal),
            _ => std::cmp::Ordering::Equal,
        }
    });
    json!(v)
}
fn source(r: &V) -> V {
    json!({
        "method": get(r, "method"),
        "endpoint": get(r, "endpoint"),
        "sourceBodyHash": get(r, "sourceBodyHash"),
        "requestStartedAt": get(r, "requestStartedAt"),
        "receivedAt": get(r, "receivedAt"),
        "fetchInvocationId": get(r, "fetchInvocationId"),
    })
}
fn raw(r: &V, endpoint: &str, at: i64) -> Result<(V, BTreeMap<String, String>), String> {
    let body = get(r, "bodyText");
    need(
        get(r, "method").as_str() == Some("GET")
            && get(r, "endpoint").as_str() == Some(endpoint)
            && js_int(get(r, "status")) == Some(200)
            && body.is_string()
            && body.as_str().unwrap().as_bytes().len() <= 8 * 1024 * 1024
            && get(r, "sourceBodyHash").as_str() == Some(sha256(body.as_str().unwrap().as_bytes()).as_str())
            && get(r, "fetchInvocationId").as_str().is_some_and(|s| !s.is_empty()),
        "RAW_SOURCE",
    )?;
    let start = instant(get(r, "requestStartedAt").as_str().unwrap_or(""))?;
    let end = instant(get(r, "receivedAt").as_str().unwrap_or(""))?;
    need(start <= end && end <= at && at - start <= 20000, "SOURCE_AGE")?;
    parse_number_lexemes(body.as_str().unwrap())
}

struct Rational {
    numerator: num_bigint::BigInt,
    scale: i64,
}
fn decimal_rational(text: &V) -> Result<Rational, String> {
    let text = text.as_str().unwrap_or("");
    let ok_shape = !text.is_empty()
        && text.len() <= 128
        && {
            // /^(0|[1-9]\d*)(?:\.(\d+))?(?:[eE]([+-]?\d+))?$/
            let (mantissa, exp_part) = match text.find(['e', 'E']) {
                Some(i) => (&text[..i], Some(&text[i + 1..])),
                None => (text, None),
            };
            let exp_ok = exp_part.is_none_or(|e| {
                let s = e.strip_prefix(['+', '-']).unwrap_or(e);
                !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
            });
            let (int_part, frac_part) = match mantissa.split_once('.') {
                Some((i, f)) => (i, Some(f)),
                None => (mantissa, None),
            };
            let int_ok = int_part == "0"
                || (!int_part.is_empty()
                    && !int_part.starts_with('0')
                    && int_part.bytes().all(|b| b.is_ascii_digit()));
            let frac_ok = frac_part.is_none_or(|f| !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()));
            exp_ok && int_ok && frac_ok
        };
    need(ok_shape, "EXACT_PRICE_SOURCE")?;
    let (mantissa, exp_part) = match text.find(['e', 'E']) {
        Some(i) => (&text[..i], Some(&text[i + 1..])),
        None => (text, None),
    };
    let exponent: i64 = exp_part.unwrap_or("0").parse().map_err(|_| "EXACT_PRICE_SOURCE".to_string())?;
    need(exponent.abs() <= 128, "EXACT_PRICE_SOURCE")?;
    let (int_part, fraction) = match mantissa.split_once('.') {
        Some((i, f)) => (i.to_string(), f.to_string()),
        None => (mantissa.to_string(), String::new()),
    };
    let mut numerator: num_bigint::BigInt = format!("{}{}", int_part, fraction)
        .parse()
        .map_err(|_| "EXACT_PRICE_SOURCE".to_string())?;
    let mut scale: i64 = fraction.len() as i64 - exponent;
    if scale < 0 {
        numerator *= num_bigint::BigInt::from(10u32).pow((-scale) as u32);
        scale = 0;
    }
    while scale > 0 && &numerator % num_bigint::BigInt::from(10) == num_bigint::BigInt::from(0) {
        numerator /= num_bigint::BigInt::from(10);
        scale -= 1;
    }
    Ok(Rational { numerator, scale })
}
fn same_rational(a: &Rational, b: &Rational) -> bool {
    a.scale == b.scale && a.numerator == b.numerator
}
fn exact_price_lexeme(text: &V, value: &V) -> Result<String, String> {
    let p = normalize_official_fill_tick(value)?;
    let r = decimal_rational(text)?;
    let s = if value.is_string() { value.as_str().unwrap().to_string() } else { js_stringify(value) };
    need(
        same_rational(&r, &decimal_rational(&json!(get(&p, "price").as_str().unwrap_or("")))?)
            || same_rational(&r, &decimal_rational(&json!(s))?),
        "EXACT_PRICE_SOURCE",
    )?;
    Ok(get(&p, "price").as_str().unwrap_or("").to_string())
}
fn integer(p: &(V, BTreeMap<String, String>), path: &[V], n: &V, min: i64) -> Result<(), String> {
    let key = serde_json::to_string(&json!(path)).unwrap();
    let x = p.1.get(&key);
    let qty = js_int(n);
    let ok = x.is_some()
        && x.unwrap().len() <= 128
        && {
            let s = x.unwrap().as_str();
            let t = s.strip_prefix('-').unwrap_or(s);
            let int_part = t.split('.').next().unwrap_or("");
            !int_part.is_empty()
                && (!int_part.starts_with('0') || int_part == "0")
                && int_part.bytes().all(|b| b.is_ascii_digit())
                && s.split('.').nth(1).is_none_or(|f| !f.is_empty() && f.bytes().all(|b| b == b'0'))
        }
        && qty.is_some()
        && qty.unwrap() >= min
        && x.unwrap().split('.').next().unwrap_or("").parse::<i128>().map(|q| q == qty.unwrap() as i128).unwrap_or(false);
    need(ok, "EXACT_INTEGER")
}
fn account_view(a: &V, b: &V, at: i64) -> Result<V, String> {
    let view = crate::no_rest_sell_reconciliation_v2::inspect_scoped_rest_no_account(&json!({
        "account": a,
        "binding": b,
        "readAt": iso_of(at),
    }))?;
    let mut receipts = vec![get(a, "cash").clone(), get(a, "positions").clone()];
    receipts.extend(get(a, "openOrdersPages").as_array().cloned().unwrap_or_default());
    receipts.extend(get(a, "scopedOrdersPages").as_array().cloned().unwrap_or_default());
    for r in &receipts {
        raw(r, get(r, "endpoint").as_str().unwrap_or(""), at)?;
    }
    let (p_data, p_numbers) = parse_number_lexemes(get(get(a, "positions"), "bodyText").as_str().unwrap_or(""))?;
    let parsed = (p_data, p_numbers);
    for (i, v) in get(&parsed.0, "positions").as_array().cloned().unwrap_or_default().iter().enumerate() {
        integer(&parsed, &[json!("positions"), json!(i), json!("quantity")], get(v, "quantity"), -9_007_199_254_740_991)?;
    }
    for r in get(a, "scopedOrdersPages").as_array().cloned().unwrap_or_default() {
        let p2 = parse_number_lexemes(get(&r, "bodyText").as_str().unwrap_or(""))?;
        for (i, o) in get(&p2.0, "data").as_array().cloned().unwrap_or_default().iter().enumerate() {
            for k in ["id", "quantity", "quantityFilled"] {
                integer(&p2, &[json!("data"), json!(i), json!(k)], get(o, k), if k == "quantityFilled" { 0 } else { 1 })?;
            }
            need(
                get(o, "open").as_bool() == Some(false)
                    && js_int(get(o, "quantityFilled")).unwrap_or(i64::MIN)
                        <= js_int(get(o, "quantity")).unwrap_or(i64::MAX),
                "SCOPED_TERMINAL_HISTORY",
            )?;
        }
    }
    let mut out = view.as_object().cloned().unwrap_or_default();
    out.insert("sourceReceipts".into(), json!(receipts.iter().map(source).collect::<Vec<_>>()));
    Ok(V::Object(out))
}
fn iso_of(ms: i64) -> String {
    baal_service_runtime::contract::ms_to_iso(ms as f64)
}

struct OwnedSource {
    proof: V,
    current: V,
}
fn owned_source(scope: &V, owned: &V, current: &V, at: i64) -> Result<OwnedSource, String> {
    let p = get(owned, "proof");
    let mut body = p.as_object().cloned().unwrap_or_default();
    let verification_hash = body.remove("verificationHash").unwrap_or(V::Null);
    need(
        get(p, "mode").as_str() == Some("scoped_rest_no_multishare_fifo_v1")
            && get(p, "scopedTradeVerified").as_bool() == Some(true)
            && get(p, "executionAuthority").as_bool() == Some(false)
            && get(p, "side").as_str() == Some("no")
            && hex(&verification_hash)
            && payload_hash(&V::Object(body))? == verification_hash.as_str().unwrap_or(""),
        "OWNED_PROOF",
    )?;
    for k in ["ownerId", "runId", "tournamentId"] {
        need(get(p, k) == get(scope, k), "OWNED_SCOPE")?;
    }
    need(
        get(scope, "ownerId").as_str() == Some("Codex")
            && get(scope, "exchanges").as_array().is_some_and(|ms| {
                ms.iter().any(|m| {
                    get(m, "marketId") == get(p, "marketId") && get(m, "exchangeId") == get(p, "exchangeId")
                })
            }),
        "MARKET_SCOPE",
    )?;
    let mut binding = scope.as_object().cloned().unwrap_or_default();
    binding.insert("marketId".into(), get(p, "marketId").clone());
    binding.insert("exchangeId".into(), get(p, "exchangeId").clone());
    let binding = V::Object(binding);
    let old = account_view(
        get(owned, "after"),
        &binding,
        instant(get(p, "readAt").as_str().unwrap_or(""))?,
    )?;
    let now_view = account_view(current, &binding, at)?;
    need(
        same(get(&old, "sourceReceipts"), get(get(p, "sourceReceipts"), "after"))?
            && instant(get(p, "readAt").as_str().unwrap_or(""))?
                <= instant(get(&now_view, "startedAt").as_str().unwrap_or(""))?
            && same(&sort(get(&old, "scopedOrders")), &sort(get(&now_view, "scopedOrders")))?,
        "OWNED_HISTORY_OR_SOURCE",
    )?;
    for view in [&old, &now_view] {
        let target = get(view, "positions")
            .as_array()
            .and_then(|rows| rows.iter().find(|v| get(v, "exchangeId") == get(p, "exchangeId")))
            .cloned()
            .unwrap_or(V::Null);
        need(
            get(&target, "marketId") == get(p, "marketId")
                && get(&target, "settled").as_bool() == Some(false)
                && js_int(get(&target, "quantity"))
                    == js_int(get(p, "remainingOwnedQuantity")).map(|q| -q),
            "OWNED_SIGNED_QUANTITY",
        )?;
    }
    let lots = get(p, "remainingLots");
    need(
        lots.as_array().is_some_and(|a| !a.is_empty() && a.len() <= 10000)
            && js_int(get(p, "remainingOwnedQuantity")).is_some_and(|q| q > 0)
            && get(p, "attributedBuyFills").is_array(),
        "FIFO_LOTS",
    )?;
    let mut count: i64 = 0;
    let mut cost: i128 = 0;
    let mut last: Option<(i64, i64)> = None;
    let mut ids = std::collections::BTreeSet::new();
    for l in lots.as_array().unwrap() {
        let lot_q = js_int(get(l, "quantity"));
        let orig_q = js_int(get(l, "originalQuantity"));
        let fill_id = js_int(get(l, "fillId"));
        need(
            get(l, "lotId").as_str().is_some()
                && !ids.contains(get(l, "lotId").as_str().unwrap_or(""))
                && lot_q.is_some_and(|q| q > 0)
                && orig_q.is_some_and(|q| q >= lot_q.unwrap_or(i64::MIN) && q <= 100)
                && fill_id.is_some_and(|f| f > 0)
                && get(l, "side").as_str() == Some("no"),
            "FIFO_LOT",
        )?;
        ids.insert(get(l, "lotId").as_str().unwrap_or("").to_string());
        for k in ["ownerId", "runId", "tournamentId", "marketId", "exchangeId"] {
            need(get(l, k) == get(p, k), "FIFO_SCOPE")?;
        }
        let px = price(get(l, "entryPrice").as_str().unwrap_or(""))?;
        let opened = instant(get(l, "openedAt").as_str().unwrap_or(""))?;
        let origin = get(p, "attributedBuyFills")
            .as_array()
            .and_then(|rows| rows.iter().find(|x| get(x, "lotId") == get(l, "lotId")).cloned());
        let order = get(&old, "scopedOrders").as_array().and_then(|rows| {
            rows.iter()
                .find(|x| {
                    let idv = get(x, "id");
                    let s = idv.as_str().map(String::from).or_else(|| idv.as_f64().map(|n| js_stringify(&json!(n)))).unwrap_or_default();
                    s == get(l, "orderId").as_str().unwrap_or("\u{7f}")
                })
                .cloned()
        });
        let mut expected = l.as_object().cloned().unwrap_or_default();
        expected.insert("quantity".into(), json!(orig_q.unwrap()));
        expected.insert("principalCost".into(), json!(money(px * orig_q.unwrap() as i128)));
        need(
            origin.is_some()
                && same(&V::Object(expected), origin.as_ref().unwrap())?
                && get(l, "principalCost").as_str() == Some(money(px * lot_q.unwrap() as i128).as_str()),
            "FIFO_BASIS",
        )?;
        need(
            get(&order.clone().unwrap_or(V::Null), "action").as_str() == Some("buy")
                && get(&order.clone().unwrap_or(V::Null), "side").as_str() == Some("no")
                && js_int(get(&order.clone().unwrap_or(V::Null), "quantityFilled")).unwrap_or(i64::MIN)
                    >= orig_q.unwrap()
                && opened >= instant(get(&order.clone().unwrap_or(V::Null), "createdAt").as_str().unwrap_or(""))?
                && opened <= instant(get(p, "readAt").as_str().unwrap_or(""))?,
            "FIFO_ORDER_SOURCE",
        )?;
        need(
            match last {
                Some((la, lf)) => opened > la || (opened == la && fill_id.unwrap() > lf),
                None => true,
            },
            "FIFO_CHRONOLOGY",
        )?;
        last = Some((opened, fill_id.unwrap()));
        count += lot_q.unwrap();
        cost += px * lot_q.unwrap() as i128;
    }
    need(
        count == js_int(get(p, "remainingOwnedQuantity")).unwrap()
            && money(cost) == get(p, "remainingOwnedCost").as_str().unwrap_or(""),
        "FIFO_TOTALS",
    )?;
    Ok(OwnedSource { proof: p.clone(), current: now_view })
}

// planBaalScaledProfitableExitV2({scope,owned,account,bookReceipt,globalRisk,
//   now,protectiveTicks=0,requestedQuantity=null,maximumSharesPerOrder=100,
//   maximumOrderCost='25.000000',feesPerShare='0.000000',previousBookVersion=null})
pub fn plan_baal_scaled_profitable_exit_v2(input: &V) -> V {
    let scope = get(input, "scope");
    let owned = get(input, "owned");
    let account = get(input, "account");
    let book_receipt = get(input, "bookReceipt");
    let g = get(input, "globalRisk");
    let now = get(input, "now");
    let protective_ticks = if get(input, "protectiveTicks").is_null() { json!(0) } else { get(input, "protectiveTicks").clone() };
    let requested_quantity = get(input, "requestedQuantity").clone();
    let maximum_shares = if get(input, "maximumSharesPerOrder").is_null() { json!(100) } else { get(input, "maximumSharesPerOrder").clone() };
    let maximum_order_cost = if get(input, "maximumOrderCost").is_null() { json!("25.000000") } else { get(input, "maximumOrderCost").clone() };
    let fees_per_share = if get(input, "feesPerShare").is_null() { json!("0.000000") } else { get(input, "feesPerShare").clone() };
    let previous_book_version = get(input, "previousBookVersion");

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
    let base_c = base.clone();
    let out = (|| -> Result<V, String> {
        let at = instant(now.as_str().unwrap_or(""))?;
        let OwnedSource { proof: p, current: a } = owned_source(scope, owned, account, at)?;
        need(
            get(g, "ownerId") == get(scope, "ownerId")
                && get(g, "runId") == get(scope, "runId")
                && get(g, "tournamentId") == get(scope, "tournamentId")
                && get(g, "complete").as_bool() == Some(true)
                && get(g, "eventScopeComplete").as_bool() == Some(true)
                && get(g, "soleCollector").as_bool() == Some(true)
                && get(g, "stopped").as_bool() == Some(false)
                && get(g, "unknownAcceptance").as_bool() == Some(false)
                && get(g, "unresolvedIntentIds").as_array().is_some_and(|x| x.is_empty())
                && hex(get(g, "sourceHash"))
                && instant(get(g, "at").as_str().unwrap_or(""))? <= at
                && at - instant(get(g, "at").as_str().unwrap_or(""))? <= 20000,
            "GLOBAL_UNKNOWN_OR_PENDING",
        )?;
        let endpoint = format!(
            "/markets/{}/orderbook?tournamentId={}&depth=20",
            get(&p, "marketId").as_str().unwrap_or(""),
            get(scope, "tournamentId").as_str().unwrap_or("")
        );
        let parsed = raw(book_receipt, &endpoint, at)?;
        let d = &parsed.0;
        need(
            instant(get(book_receipt, "requestStartedAt").as_str().unwrap_or(""))?
                >= instant(get(&a, "completedAt").as_str().unwrap_or(""))?
                && (!d.as_object().map(|o| o.contains_key("marketId")).unwrap_or(false)
                    || get(d, "marketId") == get(&p, "marketId"))
                && get(d, "contexts").as_array().is_some_and(|c| c.len() == 1)
                && get(&get(&get(d, "contexts")[0], "tournament"), "id") == get(scope, "tournamentId")
                && get(&get(&get(d, "contexts")[0], "orderbook"), "exchanges")
                    .as_array()
                    .is_some_and(|e| e.len() == 1),
            "ACCOUNT_BEFORE_SCOPED_BOOK",
        )?;
        let e = &get(&get(&get(d, "contexts")[0], "orderbook"), "exchanges")[0];
        integer(
            &parsed,
            &[json!("contexts"), json!(0), json!("orderbook"), json!("exchanges"), json!(0), json!("asOf"), json!("sequence")],
            get(get(e, "asOf"), "sequence"),
            0,
        )?;
        for side in ["bids", "asks"] {
            for (i, l) in get(e, side).as_array().cloned().unwrap_or_default().iter().enumerate() {
                integer(
                    &parsed,
                    &[json!("contexts"), json!(0), json!("orderbook"), json!("exchanges"), json!(0), json!(side), json!(i), json!("quantity")],
                    get(l, "quantity"),
                    1,
                )?;
                let lex = parsed.1.get(&serde_json::to_string(&json!([
                    "contexts", 0, "orderbook", "exchanges", 0, side, i, "price"
                ])).unwrap()).cloned();
                let lex_v = lex.map(|s| json!(s)).unwrap_or(V::Null);
                need(
                    lex_v.is_string()
                        && price(&exact_price_lexeme(&lex_v, get(l, "price"))?)?
                            == price(&js_str(get(l, "price")))?,
                    "EXACT_PRICE",
                )?;
            }
        }
        let b = normalize_book(
            d,
            get(&p, "marketId").as_str().unwrap_or(""),
            get(scope, "tournamentId").as_str().unwrap_or(""),
            get(book_receipt, "receivedAt").as_str().unwrap_or(""),
            get(book_receipt, "requestStartedAt").as_str().unwrap_or(""),
            20000,
            &std::collections::BTreeSet::new(),
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
                hex(get(previous_book_version, "depthHash"))
                    && get(previous_book_version, "versionKey").as_str()
                        == Some(
                            format!(
                                "{}:{}",
                                get(previous_book_version, "sequence").as_str().unwrap_or(""),
                                get(previous_book_version, "sourceAt").as_str().unwrap_or("")
                            )
                            .as_str(),
                        ),
                "PREVIOUS_VERSION",
            )?;
            need(
                bigint_str(get(&version, "sequence").as_str().unwrap_or(""))
                    >= bigint_str(get(previous_book_version, "sequence").as_str().unwrap_or(""))
                    && instant_ns(get(&version, "sourceAt").as_str().unwrap_or("")).map_err(|e| e.0)?
                        >= instant_ns(get(previous_book_version, "sourceAt").as_str().unwrap_or("")).map_err(|e| e.0)?
                    && instant(get(&version, "requestStartedAt").as_str().unwrap_or(""))?
                        >= instant(get(previous_book_version, "requestStartedAt").as_str().unwrap_or(""))?
                    && instant(get(&version, "receivedAt").as_str().unwrap_or(""))?
                        >= instant(get(previous_book_version, "receivedAt").as_str().unwrap_or(""))?,
                "BOOK_ROLLBACK",
            )?;
            if get(&version, "versionKey") == get(previous_book_version, "versionKey") {
                need(
                    get(&version, "depthHash") == get(previous_book_version, "depthHash"),
                    "BOOK_VERSION_CONFLICT",
                )?;
            }
        }
        let mut candidates: Vec<(usize, V, V)> = Vec::new();
        for (index, level) in get(&b, "asks").as_array().cloned().unwrap_or_default().iter().enumerate() {
            let sizing = crate::baal_scaled_profitable_fifo_prefix_v3::size_profitable_owned_no_exit_prefix_v3(&json!({
                "remainingLots": get(&p, "remainingLots"),
                "remainingOwnedQuantity": get(&p, "remainingOwnedQuantity"),
                "yesAskLevel": level,
                "requestedQuantity": requested_quantity,
                "maximumSharesPerOrder": maximum_shares,
                "maximumOrderCost": maximum_order_cost,
                "feesPerShare": fees_per_share,
                "protectiveTicks": protective_ticks,
            }));
            if js_int(get(&sizing, "quantity")).unwrap_or(0) > 0 {
                candidates.push((index, level.clone(), sizing));
            }
        }
        candidates.sort_by(|x, y| {
            let qx = js_int(get(&x.2, "quantity")).unwrap_or(0);
            let qy = js_int(get(&y.2, "quantity")).unwrap_or(0);
            qy.cmp(&qx).then_with(|| {
                let lx = amount(get(&x.2, "protectedLimit").as_str().unwrap_or("")).unwrap_or(0);
                let ly = amount(get(&y.2, "protectedLimit").as_str().unwrap_or("")).unwrap_or(0);
                ly.cmp(&lx)
            })
        });
        let chosen = candidates.first();
        let mut common = base.as_object().cloned().unwrap_or_default();
        common.insert("ownerId".into(), get(&p, "ownerId").clone());
        common.insert("runId".into(), get(&p, "runId").clone());
        common.insert("tournamentId".into(), get(&p, "tournamentId").clone());
        common.insert("marketId".into(), get(&p, "marketId").clone());
        common.insert("exchangeId".into(), get(&p, "exchangeId").clone());
        common.insert("side".into(), json!("no"));
        common.insert("action".into(), json!("sell"));
        common.insert("ownedProofHash".into(), get(&p, "verificationHash").clone());
        common.insert("ownedQuantity".into(), get(&p, "remainingOwnedQuantity").clone());
        common.insert("ownedPrincipalCost".into(), get(&p, "remainingOwnedCost").clone());
        common.insert("globalRiskSourceHash".into(), get(g, "sourceHash").clone());
        common.insert(
            "sourceReceipts".into(),
            json!({"account": get(&a, "sourceReceipts"), "book": source(book_receipt)}),
        );
        common.insert("bookVersion".into(), version);
        let expires_at = iso_of(
            (instant(get(&a, "startedAt").as_str().unwrap_or(""))? + 20000)
                .min(instant(get(book_receipt, "requestStartedAt").as_str().unwrap_or(""))? + 20000),
        );
        common.insert("expiresAt".into(), json!(expires_at));
        common.insert("minimumNetProfitPerShare".into(), json!("0.005000"));
        common.insert("depthFractionBps".into(), json!(10000));
        common.insert("depthPolicy".into(), json!("full_single_observed_rung"));
        common.insert("depthCombinedAcrossLevels".into(), json!(false));
        common.insert("fillContingency".into(), json!("A later market move may leave the limit unfilled; only verified actual fills realize profit."));
        need(instant(&expires_at)? > at, "SOURCE_EXPIRED")?;
        let mut result = serde_json::Map::new();
        match chosen {
            Some((index, level, sizing)) => {
                result = common;
                result.insert("proposalReady".into(), json!(true));
                result.insert("reason".into(), json!("PROFITABLE_MAXIMUM_CONTIGUOUS_FIFO_PREFIX"));
                result.insert("quantity".into(), get(sizing, "quantity").clone());
                result.insert("limit".into(), get(sizing, "protectedLimit").clone());
                result.insert("observedRawNoBid".into(), get(sizing, "rawObservedNoBid").clone());
                result.insert("orderNotional".into(), get(sizing, "orderNotional").clone());
                result.insert("principalIfFullyFilled".into(), get(sizing, "removedPrincipalIfFilled").clone());
                result.insert("minimumProfitIfFullyFilled".into(), get(sizing, "protectedNetProfitIfFilled").clone());
                result.insert("yesAskSourceLevel".into(), level.clone());
                result.insert("sourceLevelIndex".into(), json!(index));
                result.insert("prefix".into(), sizing.clone());
            }
            None => {
                result = common;
                result.insert("reason".into(), json!("NO_DEPTH_QUALIFIED_PROFITABLE_FIFO_PREFIX"));
                result.insert("quantity".into(), json!(0));
                result.insert("limit".into(), V::Null);
            }
        }
        let result = V::Object(result);
        let hash = payload_hash(&result)?;
        let mut o = result.as_object().cloned().unwrap_or_default();
        o.insert("proposalHash".into(), json!(hash));
        Ok(V::Object(o))
    })();
    match out {
        Ok(v) => v,
        Err(e) => {
            let mut o = base_c.as_object().cloned().unwrap_or_default();
            let is_ours = e.starts_with("SCALED_PROFIT_EXIT_")
                && e["SCALED_PROFIT_EXIT_".len()..]
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b == b'_')
                && e.len() > "SCALED_PROFIT_EXIT_".len();
            o.insert(
                "reason".into(),
                json!(if is_ours { e } else { "INVALID_OR_INCOMPLETE_EXIT_EVIDENCE".to_string() }),
            );
            o.insert("holdExistingInventory".into(), json!(true));
            V::Object(o)
        }
    }
}
fn bigint_str(s: &str) -> i128 {
    s.parse::<i128>().unwrap_or(i128::MIN)
}
fn js_str(v: &V) -> String {
    v.as_str()
        .map(String::from)
        .or_else(|| v.as_i64().map(|x| x.to_string()))
        .or_else(|| v.as_u64().map(|x| x.to_string()))
        .or_else(|| v.as_f64().map(|x| js_stringify(&json!(x))))
        .unwrap_or_default()
}
