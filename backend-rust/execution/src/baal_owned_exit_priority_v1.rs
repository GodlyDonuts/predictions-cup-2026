// execution/baal-owned-exit-priority-v1.mjs — bulk quotes choose which owned
// book to read first. They never authorize an order or prove depth, a fill,
// settlement value, or realized profit.
//
// Port notes:
// - `fallback()` returns {ranked:[],orderEligible:false,executionAuthority:false}
//   on ANY soft-fail OR thrown error (the outer catch swallows everything).
// - `instant()` errors inside the try are caught the same way.
// - localeCompare on exchangeId -> Rust Ord on str (BMP ASCII ids).
// - `now` is a numeric millisecond clock (callers pass Date.now()), NOT an
//   ISO string — instant() is only applied to receipt timestamps.
// - JS BigInt money -> i128 micro units.
use crate::gates::normalize_official_fill_tick;
use baal_service_runtime::fixed::{amount, instant, money, price, SCALE, TICK};
use baal_service_runtime::json_text::{js_int, js_truthy};
use serde_json::{json, Value as V};

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn instant_v(v: &V) -> Result<i64, String> {
    instant(v.as_str().ok_or_else(|| "INVALID_INSTANT".to_string())?)
}
fn amount_v(v: &V) -> Result<i128, String> {
    amount(v.as_str().unwrap_or(""))
}
fn price_v(v: &V) -> Result<i128, String> {
    price(v.as_str().unwrap_or(""))
}
fn fallback() -> V {
    json!({
        "ranked": [],
        "orderEligible": false,
        "executionAuthority": false,
    })
}

/// rankBaalOwnedExitReadHintsV1({scope, universeHash, owned, hints, now,
/// maximumSharesPerOrder=100, maximumOrderCost='25.000000'})
pub fn rank_baal_owned_exit_read_hints_v1(input: &V) -> V {
    inner(input).unwrap_or_else(|_| fallback())
}

fn inner(input: &V) -> Result<V, String> {
    let scope = get(input, "scope");
    let universe_hash = get(input, "universeHash");
    let owned = get(input, "owned");
    let hints = get(input, "hints");
    let now = get(input, "now");
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
    let now_ms = now.as_f64().unwrap_or(f64::NAN);

    if scope.is_null()
        || get(hints, "tournamentId") != get(scope, "tournamentId")
        || get(hints, "universeHash") != universe_hash
        || !get(hints, "quoteCaptures").is_array()
        || !owned.is_array()
    {
        return Ok(fallback());
    }
    let cap = amount_v(&maximum_order_cost)?;
    let mut ranked: Vec<V> = Vec::new();
    let shares = js_int(&maximum_shares_per_order).unwrap_or(i64::MIN);
    if !(1..=100).contains(&shares) || cap <= 0 || cap > 25_000_000 {
        return Ok(fallback());
    }
    for slot in owned.as_array().unwrap() {
        let p = get(slot, "proof");
        if get(p, "remainingOwnedQuantity").is_null()
            || !js_truthy(Some(get(p, "remainingOwnedQuantity")))
            || !get(p, "remainingLots").is_array()
        {
            continue;
        }
        let quotes: Vec<&V> = get(hints, "quoteCaptures")
            .as_array()
            .unwrap()
            .iter()
            .filter(|q| {
                get(q, "exchangeId") == get(p, "exchangeId")
                    && get(q, "marketId") == get(p, "marketId")
                    && get(q, "option").as_str() == Some("YES")
            })
            .collect();
        if quotes.len() != 1 {
            continue;
        }
        let q = quotes[0];
        let hex = |v: &V| -> bool {
            v.as_str()
                .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
        };
        if (q.get("tournamentId").is_some()
            && get(q, "tournamentId") != get(scope, "tournamentId"))
            || get(q, "crossed") != &json!(false)
            || !hex(get(q, "sourceHash"))
        {
            continue;
        }
        let start = instant_v(get(q, "requestStartedAt"))?;
        let end = instant_v(get(q, "receivedAt"))?;
        let known = if get(q, "knownAt").is_null() {
            end
        } else {
            instant_v(get(q, "knownAt"))?
        };
        if start as f64 > end as f64
            || end as f64 > known as f64
            || known as f64 > now_ms
            || now_ms - start as f64 > 20_000.0
        {
            continue;
        }
        let bid = SCALE - price_v(&normalize_official_fill_tick(get(q, "bestAsk"))?["price"])?;
        if !(TICK..=SCALE - TICK).contains(&bid) {
            continue;
        }
        let owned_q = match js_int(get(p, "remainingOwnedQuantity")) {
            Some(n) => n,
            // JS Math.min(NaN, ...) -> NaN -> `!remaining` breaks on the first
            // lot and the slot is skipped.
            None => continue,
        };
        let mut remaining = std::cmp::min(owned_q, shares);
        remaining = std::cmp::min(remaining as i128, cap / bid) as i64;
        let mut quantity: i64 = 0;
        let mut principal: i128 = 0;
        for lot in get(p, "remainingLots").as_array().unwrap() {
            if remaining == 0 {
                break;
            }
            let lot_q = js_int(get(lot, "quantity"));
            if !lot_q.map(|n| n > 0).unwrap_or(false) {
                quantity = 0;
                break;
            }
            let cost = price_v(get(lot, "entryPrice"))?;
            if bid < cost + TICK {
                break;
            }
            let take = std::cmp::min(remaining, lot_q.unwrap());
            quantity += take;
            remaining -= take;
            principal += cost * take as i128;
        }
        if quantity == 0 {
            continue;
        }
        ranked.push(json!({
            "exchangeId": get(p, "exchangeId"),
            "marketId": get(p, "marketId"),
            "maximumHypotheticalQuantity": quantity,
            "quotedNoBid": money(bid),
            "upperBoundProfitWithoutDepth": money(bid * quantity as i128 - principal),
            "quoteReceivedAt": get(q, "receivedAt"),
            "sourceHash": get(q, "sourceHash"),
        }));
    }
    ranked.sort_by(|a, b| {
        let (pa, pb) = (
            amount_v(get(a, "upperBoundProfitWithoutDepth")).unwrap_or(0),
            amount_v(get(b, "upperBoundProfitWithoutDepth")).unwrap_or(0),
        );
        pb.cmp(&pa)
            .then_with(|| {
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

