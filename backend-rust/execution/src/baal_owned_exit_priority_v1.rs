// execution/baal-owned-exit-priority-v1.mjs — bulk quotes choose which owned
// book to read first. They never authorize an order or prove depth, a fill,
// settlement value, or realized profit.
//
// Port notes:
// - `now` is a numeric epoch-ms parameter (compared to instant() results).
// - Any throw inside the loop (invalid instant, price off-tick, etc.) lands in
//   the outer catch -> the empty fallback.
// - `!p?.remainingOwnedQuantity` is a falsy check: 0/missing both skip.
// - ranked.sort: upperBoundProfitWithoutDepth desc, then exchangeId
//   localeCompare (ASCII ids -> byte order).
use crate::api_tick_price::normalize_official_fill_tick;
use baal_service_runtime::fixed::{amount, instant, money, price, SCALE, TICK};
use baal_service_runtime::json_text::js_int;
use serde_json::{json, Value as V};

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn fallback() -> V {
    json!({"ranked": [], "orderEligible": false, "executionAuthority": false})
}
fn hex(v: &V) -> bool {
    v.as_str()
        .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
}
/// JS truthiness for `!x` checks.
fn truthy(v: &V) -> bool {
    match v {
        V::Null => false,
        V::Bool(b) => *b,
        V::Number(n) => n.as_f64().map(|f| f != 0.0 && !f.is_nan()).unwrap_or(false),
        V::String(s) => !s.is_empty(),
        _ => true,
    }
}
/// JS Number(v) coercion as used by Math.min/BigInt(take).
fn js_num(v: &V) -> f64 {
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
/// JSON output: integral doubles serialize as integers like JS
/// JSON.stringify(-5) -> '-5', not '-5.0'.
fn num_v(x: f64) -> V {
    if x.is_finite() && x.fract() == 0.0 && x.abs() <= 9_007_199_254_740_991.0 {
        json!(x as i64)
    } else {
        json!(x)
    }
}
/// BigInt(x) on a JS Number: throws a foreign RangeError unless integral.
fn js_bigint_num(x: f64) -> Result<i128, String> {
    if x.is_finite() && x.fract() == 0.0 && x.abs() <= 1.7e38 {
        Ok(x as i128)
    } else {
        Err("RangeError: The number NaN cannot be converted to a BigInt".into())
    }
}
/// Math.min: NaN propagates (Rust f64::min ignores NaN).
fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.min(b)
    }
}

/// rankBaalOwnedExitReadHintsV1({scope,universeHash,owned,hints,now,
/// maximumSharesPerOrder=100,maximumOrderCost='25.000000'})
pub fn rank_baal_owned_exit_read_hints_v1(input: &V) -> V {
    match inner(input) {
        Ok(v) => v,
        Err(_) => fallback(),
    }
}

fn inner(input: &V) -> Result<V, String> {
    let scope = get(input, "scope");
    let universe_hash = get(input, "universeHash");
    let owned = get(input, "owned");
    let hints = get(input, "hints");
    let now = input.get("now").and_then(V::as_f64).unwrap_or(f64::NAN);
    let maximum_shares_per_order = get(input, "maximumSharesPerOrder");
    let maximum_shares_per_order = if maximum_shares_per_order.is_null() {
        json!(100)
    } else {
        maximum_shares_per_order.clone()
    };
    let maximum_order_cost = get(input, "maximumOrderCost");
    let maximum_order_cost = if maximum_order_cost.is_null() {
        json!("25.000000")
    } else {
        maximum_order_cost.clone()
    };
    if scope.is_null()
        || get(hints, "tournamentId") != get(scope, "tournamentId")
        || get(hints, "universeHash") != universe_hash
        || !get(hints, "quoteCaptures").is_array()
        || !owned.is_array()
    {
        return Ok(fallback());
    }
    let cap = amount(maximum_order_cost.as_str().unwrap_or(""))?;
    let shares_cap = js_int(&maximum_shares_per_order).unwrap_or(i64::MIN);
    if !(1..=100).contains(&shares_cap) || cap <= 0 || cap > 25_000_000 {
        return Ok(fallback());
    }
    let mut ranked: Vec<V> = Vec::new();
    for slot in owned.as_array().unwrap() {
        let p = get(slot, "proof");
        // !p?.remainingOwnedQuantity — JS falsy: null/undefined/0/NaN/''/false.
        if !truthy(get(p, "remainingOwnedQuantity")) || !get(p, "remainingLots").is_array() {
            continue;
        }
        let quotes: Vec<V> = get(hints, "quoteCaptures")
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|q| {
                get(q, "exchangeId") == get(p, "exchangeId")
                    && get(q, "marketId") == get(p, "marketId")
                    && get(q, "option") == &json!("YES")
            })
            .collect();
        if quotes.len() != 1 {
            continue;
        }
        let q = &quotes[0];
        if (q.as_object().map(|o| o.contains_key("tournamentId")).unwrap_or(false)
            && get(q, "tournamentId") != get(scope, "tournamentId"))
            || get(q, "crossed") != &json!(false)
            || !hex(get(q, "sourceHash"))
        {
            continue;
        }
        let start = instant(get(q, "requestStartedAt").as_str().unwrap_or(""))?;
        let end = instant(get(q, "receivedAt").as_str().unwrap_or(""))?;
        let known = if q.get("knownAt").is_none() {
            end
        } else {
            instant(get(q, "knownAt").as_str().unwrap_or(""))?
        };
        if start > end
            || end > known
            || (known as f64) > now
            || now - start as f64 > 20000.0
        {
            continue;
        }
        let bid = SCALE - price(
            get(&normalize_official_fill_tick(get(q, "bestAsk"))?, "price")
                .as_str()
                .unwrap_or(""),
        )?;
        if bid < TICK || bid > SCALE - TICK {
            continue;
        }
        // Math.min coerces each argument with Number(); a non-numeric string
        // or array yields NaN which never equals 0 in the `!remaining` break
        // but makes BigInt(take) throw inside the lot loop -> outer fallback.
        let mut remaining = js_min(
            js_min(js_num(get(p, "remainingOwnedQuantity")), shares_cap as f64),
            (cap / bid) as f64,
        );
        let mut quantity: f64 = 0.0;
        let mut principal: i128 = 0;
        for lot in get(p, "remainingLots").as_array().unwrap() {
            if remaining == 0.0 || remaining.is_nan() {
                break;
            }
            let lq = js_int(get(lot, "quantity"));
            if lq.map(|x| x <= 0).unwrap_or(true) {
                quantity = 0.0;
                break;
            }
            let cost = price(get(lot, "entryPrice").as_str().unwrap_or(""))?;
            if bid < cost + TICK {
                break;
            }
            let take = js_min(remaining, lq.unwrap() as f64);
            quantity += take;
            remaining -= take;
            principal += cost * js_bigint_num(take)?;
        }
        if quantity == 0.0 || quantity.is_nan() {
            continue;
        }
        ranked.push(json!({
            "exchangeId": get(p, "exchangeId"),
            "marketId": get(p, "marketId"),
            "maximumHypotheticalQuantity": num_v(quantity),
            "quotedNoBid": money(bid),
            "upperBoundProfitWithoutDepth": money(bid * js_bigint_num(quantity)? - principal),
            "quoteReceivedAt": get(q, "receivedAt"),
            "sourceHash": get(q, "sourceHash"),
        }));
    }
    ranked.sort_by(|a, b| {
        let pa = amount(get(a, "upperBoundProfitWithoutDepth").as_str().unwrap_or("")).unwrap_or(i128::MIN);
        let pb = amount(get(b, "upperBoundProfitWithoutDepth").as_str().unwrap_or("")).unwrap_or(i128::MIN);
        pb.cmp(&pa).then_with(|| {
            get(a, "exchangeId")
                .as_str()
                .unwrap_or("")
                .cmp(get(b, "exchangeId").as_str().unwrap_or(""))
        })
    });
    Ok(json!({
        "ranked": ranked,
        "priority": "largest_quoted_fifo_gain_first",
        "depthVerified": false,
        "actualProfitClaim": false,
        "orderEligible": false,
        "executionAuthority": false,
    }))
}
