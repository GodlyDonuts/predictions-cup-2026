// execution/no-liquidation-depth-value-v1.mjs — a dated depth-based
// counterfactual value, never a fill, payout or order permit.
use baal_service_runtime::fixed::{amount, money, price, SCALE, TICK};
use baal_service_runtime::json_text::js_int;
use serde_json::{json, Value as V};

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}

/// valueNoLiquidationDepthV1({quantity,remainingCost,yesAsks,slippageTicks=0})
pub fn value_no_liquidation_depth_v1(input: &V) -> Result<V, String> {
    let quantity = js_int(get(input, "quantity"));
    // Destructured default applies only when the key is absent; explicit null
    // stays null and fails isSafeInteger.
    let slippage_raw = input.get("slippageTicks").cloned().unwrap_or(json!(0));
    let slippage_ticks = js_int(&slippage_raw);
    let yes_asks = get(input, "yesAsks");
    if !(quantity.is_some_and(|q| (1..=100).contains(&q))
        && slippage_ticks.is_some_and(|t| (0..=10).contains(&t))
        && yes_asks.is_array()
        && (1..=20).contains(&yes_asks.as_array().unwrap().len()))
    {
        return Err("NO_DEPTH_VALUE_INPUT".into());
    }
    // amount() validates and throws its own message, as in JS.
    let cost = amount(get(input, "remainingCost").as_str().unwrap_or(""))?;
    if cost < 0 {
        return Err("NO_DEPTH_VALUE_COST".into());
    }
    let quantity = quantity.unwrap();
    let slippage_ticks = slippage_ticks.unwrap();
    let mut previous: i128 = -1;
    let mut left = quantity as i128;
    let mut proceeds: i128 = 0;
    let mut consumed: Vec<V> = Vec::new();
    for level in yes_asks.as_array().unwrap() {
        let ask = price(get(level, "price").as_str().unwrap_or(""))?;
        if ask <= previous
            || ask < 5000
            || ask > 995000
            || !js_int(get(level, "quantity")).is_some_and(|q| q >= 1)
        {
            return Err("NO_DEPTH_VALUE_LEVEL".into());
        }
        previous = ask;
        let no_bid = SCALE - ask - slippage_ticks as i128 * TICK;
        if left != 0 && no_bid >= 5000 {
            let used = std::cmp::min(left, js_int(get(level, "quantity")).unwrap() as i128);
            proceeds += used * no_bid;
            left -= used;
            consumed.push(json!({"quantity": used as i64, "noPrice": money(no_bid)}));
        }
    }
    let valued_quantity = quantity as i128 - left;
    let complete = left == 0;
    Ok(json!({
        "schemaVersion": 1,
        "kind": "observed_NO_depth_counterfactual_value_v1",
        "quantity": quantity,
        "valuedQuantity": valued_quantity as i64,
        "unvaluedQuantity": left as i64,
        "complete": complete,
        "proceeds": money(proceeds),
        "remainingCost": get(input, "remainingCost"),
        "unrealizedPnl": if complete { json!(money(proceeds - cost)) } else { V::Null },
        "fees": "0.000000",
        "slippageTicks": slippage_raw,
        "consumed": consumed,
        "executionAuthority": false,
        "actualFill": false,
    }))
}
