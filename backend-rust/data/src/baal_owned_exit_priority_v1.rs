// execution/baal-owned-exit-priority-v1.mjs — bulk quotes choose which owned
// book to read first. They never authorize an order or prove depth, a fill,
// settlement value, or realized profit.
use baal_execution::gates::normalize_official_fill_tick;
use baal_service_runtime::fixed::{amount, instant, money, price, SCALE, TICK};
use baal_service_runtime::json_text::js_truthy;
use serde_json::{json, Value as V};

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn hex64(s: Option<&str>) -> bool {
    s.is_some_and(|s| {
        s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
fn js_f64(v: &V) -> f64 {
    v.as_f64().unwrap_or(f64::NAN)
}
// BigInt(x): throws unless x is integral (and finite).
fn bigint_of(x: f64) -> Result<i128, String> {
    if x.is_finite() && x.fract() == 0.0 && x.abs() <= 1.7e38 {
        Ok(x as i128)
    } else {
        Err("Cannot convert non-integer to BigInt".into())
    }
}
fn is_safe_int(v: &V) -> bool {
    baal_service_runtime::json_text::js_int(v).is_some()
}

fn fallback() -> V {
    json!({"ranked": [], "orderEligible": false, "executionAuthority": false})
}

/// rankBaalOwnedExitReadHintsV1({scope,universeHash,owned,hints,now,
/// maximumSharesPerOrder=100,maximumOrderCost='25.000000'})
pub fn rank_baal_owned_exit_read_hints_v1(input: &V) -> V {
    (|| -> Result<V, String> {
        let scope = input.get("scope").cloned().unwrap_or(V::Null);
        if scope.is_null() {
            return Ok(fallback());
        }
        let hints = input.get("hints").cloned().unwrap_or(V::Null);
        let now = js_f64(input.get("now").unwrap_or(&V::Null));
        let maximum_shares_per_order =
            js_f64(input.get("maximumSharesPerOrder").unwrap_or(&json!(100)));
        let maximum_order_cost = input
            .get("maximumOrderCost")
            .and_then(V::as_str)
            .unwrap_or("25.000000");
        if get(&hints, "tournamentId") != get(&scope, "tournamentId")
            || get(&hints, "universeHash") != input.get("universeHash").unwrap_or(&V::Null)
            || !get(&hints, "quoteCaptures").is_array()
            || !input.get("owned").is_some_and(V::is_array)
        {
            return Ok(fallback());
        }
        let cap = amount(maximum_order_cost)?;
        if !(maximum_shares_per_order.is_finite()
            && maximum_shares_per_order.fract() == 0.0
            && maximum_shares_per_order >= 1.0
            && maximum_shares_per_order <= 100.0
            && cap > 0
            && cap <= 25_000_000)
        {
            return Ok(fallback());
        }
        let maximum_shares_per_order = maximum_shares_per_order as i64;
        let mut ranked: Vec<V> = Vec::new();
        for slot in input.get("owned").unwrap().as_array().unwrap() {
            let p = get(slot, "proof");
            // !p?.remainingOwnedQuantity — falsy covers 0, null, undefined, "".
            if !js_truthy(p.get("remainingOwnedQuantity")) || !get(p, "remainingLots").is_array() {
                continue;
            }
            let quotes: Vec<&V> = get(&hints, "quoteCaptures")
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
            if (q.get("tournamentId").is_some()
                && get(q, "tournamentId") != get(&scope, "tournamentId"))
                || get(q, "crossed") != &V::Bool(false)
                || !hex64(get(q, "sourceHash").as_str())
            {
                continue;
            }
            let start = instant(get(q, "requestStartedAt").as_str().unwrap_or(""))? as f64;
            let end = instant(get(q, "receivedAt").as_str().unwrap_or(""))? as f64;
            let known = match q.get("knownAt") {
                None | Some(V::Null) => end,
                Some(k) => instant(k.as_str().unwrap_or(""))? as f64,
            };
            // undefined knownAt -> end; start>end, end>known, known>now,
            // or stale > 20s all skip.
            if !(start <= end) || !(end <= known) || !(known <= now) || !(now - start <= 20000.0) {
                continue;
            }
            let tick = normalize_official_fill_tick(get(q, "bestAsk"))?;
            let bid = SCALE - price(get(&tick, "price").as_str().unwrap_or(""))?;
            if bid < TICK || bid > SCALE - TICK {
                continue;
            }
            let mut remaining = js_f64(get(p, "remainingOwnedQuantity"))
                .min(maximum_shares_per_order as f64)
                .min((cap / bid) as f64);
            let mut quantity = 0f64;
            let mut principal = 0i128;
            for lot in get(p, "remainingLots").as_array().unwrap() {
                if remaining == 0.0 {
                    break;
                }
                if !(is_safe_int(get(lot, "quantity")) && js_f64(get(lot, "quantity")) > 0.0) {
                    quantity = 0.0;
                    break;
                }
                let cost = price(get(lot, "entryPrice").as_str().unwrap_or(""))?;
                if bid < cost + TICK {
                    break;
                }
                let take = remaining.min(js_f64(get(lot, "quantity")));
                quantity += take;
                remaining -= take;
                principal += cost * bigint_of(take)?;
            }
            if quantity == 0.0 {
                continue;
            }
            ranked.push(json!({
                "exchangeId": get(p, "exchangeId"),
                "marketId": get(p, "marketId"),
                "maximumHypotheticalQuantity": quantity,
                "quotedNoBid": money(bid),
                "upperBoundProfitWithoutDepth": money(bid * bigint_of(quantity)? - principal),
                "quoteReceivedAt": get(q, "receivedAt"),
                "sourceHash": get(q, "sourceHash"),
            }));
        }
        ranked.sort_by(|a, b| {
            let pa = amount(get(a, "upperBoundProfitWithoutDepth").as_str().unwrap_or(""))
                .unwrap_or(0);
            let pb = amount(get(b, "upperBoundProfitWithoutDepth").as_str().unwrap_or(""))
                .unwrap_or(0);
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
    })()
    .unwrap_or_else(|_| fallback())
}
