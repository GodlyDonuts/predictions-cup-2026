// execution/no-liquidation-depth-value-v1.mjs — a dated depth-based
// counterfactual value, never a fill, payout or order permit.
//
// Port notes:
// - JS BigInt micro arithmetic -> i128 (values are far below i128 bounds).
// - `price()`/`amount()` failures propagate their own raw error string, exactly
//   like the JS throws; only the explicit input/level guards use the
//   NO_DEPTH_VALUE_* codes.
use baal_service_runtime::fixed::{amount, money, price, SCALE, TICK};
use baal_service_runtime::json_text::js_int;
use serde_json::{json, Value as V};

/// valueNoLiquidationDepthV1({quantity,remainingCost,yesAsks,slippageTicks=0})
pub fn value_no_liquidation_depth_v1(input: &V) -> Result<V, String> {
    let v = |k: &str| input.get(k).unwrap_or(&V::Null);
    let quantity = js_int(v("quantity"));
    // slippageTicks defaults only when absent; an explicit null is not the
    // JS destructuring default and fails the safe-integer check.
    let slippage_ok = match input.get("slippageTicks") {
        None => true,
        Some(x) => js_int(x).map(|t| (0..=10).contains(&t)).unwrap_or(false),
    };
    let yes_asks = v("yesAsks");
    if !(quantity.map(|q| (1..=100).contains(&q)).unwrap_or(false)
        && slippage_ok
        && yes_asks.is_array()
        && (1..=20).contains(&yes_asks.as_array().unwrap().len()))
    {
        return Err("NO_DEPTH_VALUE_INPUT".into());
    }
    let quantity = quantity.unwrap();
    let slippage_ticks = match input.get("slippageTicks") {
        None => 0_i64,
        Some(x) => js_int(x).unwrap_or(0),
    };

    let cost = amount(v("remainingCost").as_str().unwrap_or("\u{0}"))?;
    if cost < 0 {
        return Err("NO_DEPTH_VALUE_COST".into());
    }
    let mut previous: i128 = -1;
    let mut left: i64 = quantity;
    let mut proceeds: i128 = 0;
    let mut consumed: Vec<V> = Vec::new();
    for level in yes_asks.as_array().unwrap() {
        // price() throws its own raw error in JS before the level guard.
        let ask = price(level.get("price").and_then(V::as_str).unwrap_or(""))?;
        let lq = js_int(level.get("quantity").unwrap_or(&V::Null));
        if ask <= previous
            || !(TICK..=SCALE - TICK).contains(&ask)
            || lq.map(|q| q < 1).unwrap_or(true)
        {
            return Err("NO_DEPTH_VALUE_LEVEL".into());
        }
        previous = ask;
        let no_bid = SCALE - ask - slippage_ticks as i128 * TICK;
        if left != 0 && no_bid >= TICK {
            let used = left.min(lq.unwrap());
            proceeds += used as i128 * no_bid;
            left -= used;
            consumed.push(json!({
                "quantity": used,
                "noPrice": money(no_bid),
            }));
        }
    }

    let complete = left == 0;
    Ok(json!({
        "schemaVersion": 1,
        "kind": "observed_NO_depth_counterfactual_value_v1",
        "quantity": quantity,
        "valuedQuantity": quantity - left,
        "unvaluedQuantity": left,
        "complete": complete,
        "proceeds": money(proceeds),
        "remainingCost": v("remainingCost").clone(),
        "unrealizedPnl": if complete {
            json!(money(proceeds - cost))
        } else {
            V::Null
        },
        "fees": "0.000000",
        "slippageTicks": slippage_ticks,
        "consumed": consumed,
        "executionAuthority": false,
        "actualFill": false,
    }))
}
