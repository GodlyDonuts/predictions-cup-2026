// execution/baal-profitable-exit-depth-sweep-v1.mjs — pure economic selector:
// one book, one protected limit, contiguous owned FIFO. No receipt validation,
// account adoption, activation or order authority.
//
// Port notes:
// - `{yesAsks,...sizing}` rest spread: sizing is the input object minus the
//   yesAsks key (remainingLots and the policy fields flow through verbatim).
// - JS BigInt depth runs as i128 micro arithmetic; the numeric adapter is
//   clamped to 100 while the displayed aggregate stays exact.
// - candidates.sort comparator: protectedNetProfitIfFilled desc, then
//   quantity desc, then protectedLimit desc.
// - `try{px=price(level.price)}catch{INVALID_BOOK_PRICE}` — only the price
//   parse is guarded; everything else is a hard return.
use crate::baal_scaled_profitable_fifo_prefix_v3::size_profitable_owned_no_exit_prefix_v3;
use crate::core::payload_hash;
use baal_service_runtime::fixed::{amount, money, price};
use baal_service_runtime::json_text::js_int;
use serde_json::{json, Map, Value as V};

fn base() -> V {
    json!({
        "mode": "baal_profitable_exit_depth_sweep_v1",
        "executionAuthority": false,
        "orderEligible": false,
        "actualFillOrRealizedProfit": false,
    })
}
fn bail(reason: &str) -> V {
    let mut out = base().as_object().cloned().unwrap_or_else(Map::new);
    out.insert("reason".into(), json!(reason));
    out.insert("selected".into(), V::Null);
    V::Object(out)
}

/// selectBaalProfitableExitDepthSweepV1({yesAsks,...sizing})
pub fn select_baal_profitable_exit_depth_sweep_v1(input: &V) -> V {
    let yes_asks = match input.get("yesAsks") {
        Some(v) => v.clone(),
        None => V::Null,
    };
    let mut sizing = input.clone();
    sizing.as_object_mut().map(|m| m.shift_remove("yesAsks"));
    if !yes_asks.is_array()
        || yes_asks.as_array().map(|a| a.is_empty() || a.len() > 20).unwrap_or(true)
    {
        return bail("INVALID_BOOK_DEPTH");
    }
    let mut prior: i128 = -1;
    let mut depth: i128 = 0;
    let mut candidates: Vec<V> = Vec::new();
    for (index, level) in yes_asks.as_array().unwrap().iter().enumerate() {
        let px = match price(level.get("price").and_then(V::as_str).unwrap_or("")) {
            Ok(p) => p,
            Err(_) => return bail("INVALID_BOOK_PRICE"),
        };
        let lq = js_int(level.get("quantity").unwrap_or(&V::Null));
        if px <= prior || lq.map(|q| q <= 0).unwrap_or(true) {
            return bail("UNORDERED_DUPLICATE_OR_INVALID_DEPTH");
        }
        prior = px;
        depth += lq.unwrap() as i128;
        // Only 100 shares can enter this policy. Clamp the numeric adapter,
        // retain the full exact aggregate separately, and never fabricate
        // extra liquidity.
        let mut args = sizing.clone();
        args.as_object_mut().unwrap().insert(
            "yesAskLevel".into(),
            json!({
                "price": level.get("price"),
                "quantity": std::cmp::min(depth, 100),
            }),
        );
        let result = size_profitable_owned_no_exit_prefix_v3(&args);
        if js_int(result.get("quantity").unwrap_or(&V::Null)).unwrap_or(0) > 0 {
            candidates.push(json!({
                "throughLevel": index,
                "displayedAggregateQuantity": depth.to_string(),
                "protectedLimit": result.get("protectedLimit"),
                "quantity": result.get("quantity"),
                "orderNotional": result.get("orderNotional"),
                "removedPrincipalIfFilled": result.get("removedPrincipalIfFilled"),
                "protectedNetProfitIfFilled": result.get("protectedNetProfitIfFilled"),
                "consumedPrefixLots": result.get("consumedPrefixLots"),
            }));
        }
    }
    candidates.sort_by(|a, b| {
        let profit_of = |v: &V| -> i128 {
            amount(v.get("protectedNetProfitIfFilled").and_then(V::as_str).unwrap_or(""))
                .unwrap_or(i128::MIN)
        };
        let delta = profit_of(b) - profit_of(a);
        if delta > 0 {
            return std::cmp::Ordering::Greater;
        }
        if delta < 0 {
            return std::cmp::Ordering::Less;
        }
        let qa = js_int(a.get("quantity").unwrap_or(&V::Null)).unwrap_or(i64::MIN);
        let qb = js_int(b.get("quantity").unwrap_or(&V::Null)).unwrap_or(i64::MIN);
        qb.cmp(&qa).then_with(|| {
            let limit_of = |v: &V| -> i128 {
                price(v.get("protectedLimit").and_then(V::as_str).unwrap_or(""))
                    .unwrap_or(i128::MIN)
            };
            limit_of(b).cmp(&limit_of(a))
        })
    });
    let selected = candidates.first().cloned().unwrap_or(V::Null);
    let mut out = base().as_object().cloned().unwrap_or_else(Map::new);
    out.insert(
        "reason".into(),
        json!(if !selected.is_null() {
            "MAXIMUM_PROTECTED_DOLLAR_GAIN"
        } else {
            "NO_PROFITABLE_FIFO_PREFIX"
        }),
    );
    out.insert("selected".into(), selected);
    out.insert("candidates".into(), V::Array(candidates));
    // payloadHash throws are uncaught in the JS; on parsed JSON values the
    // only failure mode (non-finite numbers) cannot occur.
    out.insert(
        "sourceBookDepthHash".into(),
        json!(payload_hash(&yes_asks).unwrap()),
    );
    out.insert(
        "sourceLotsHash".into(),
        json!(payload_hash(sizing.get("remainingLots").unwrap_or(&V::Null)).unwrap()),
    );
    out.insert("fees".into(), json!(money(0)));
    out.insert("depthFromSingleReceipt".into(), json!(true));
    V::Object(out)
}
