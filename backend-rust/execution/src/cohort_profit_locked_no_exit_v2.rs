// execution/cohort-profit-locked-no-exit-v2.mjs — alternative to refusing
// every losing lot: spend only already-realized cohort gains, and protect the
// profit floor for EVERY possible FIFO partial fill.
//
// Port notes:
// - JS BigInt micro arithmetic -> i128 (values are far below i128 bounds).
// - `price()`/`amount()` failures propagate their own raw error string, exactly
//   like the JS throws; only `need` failures are prefixed COHORT_EXIT_.
// - Destructured defaults apply only to absent keys: an explicit null is not a
//   default trigger in JS, so `input.get(k)` distinguishes None (absent ->
//   default) from Some(Null) (explicit -> parsed/checked like any value).
use crate::core::payload_hash;
use baal_service_runtime::fixed::{amount, money, price, SCALE, TICK};
use baal_service_runtime::json_text::{js_int, js_truthy};
use serde_json::{json, Value as V};

fn err(code: &str) -> String {
    format!("COHORT_EXIT_{code}")
}

// Absent key -> JS destructuring default; explicit value -> the value's string
// (non-strings take the \0 sentinel so amount() rejects, as JS amount() does).
fn opt_str<'a>(input: &'a V, k: &str, default: &'a str) -> &'a str {
    match input.get(k) {
        None => default,
        Some(x) => x.as_str().unwrap_or("\u{0}"),
    }
}

/// sizeCohortProfitLockedNoExitV2({remainingLots,remainingOwnedQuantity,
///   yesAskLevel,realizedProfit,minimumCohortProfit='0.005000',
///   protectiveTicks=1,maximumOrderCost='25.000000',
///   maximumSharesPerOrder=100,feesPerShare='0.000000'})
pub fn size_cohort_profit_locked_no_exit_v2(input: &V) -> Result<V, String> {
    let v = |k: &str| input.get(k).unwrap_or(&V::Null);
    let remaining_lots = v("remainingLots");
    let remaining_owned = js_int(v("remainingOwnedQuantity"));
    if !(remaining_lots.is_array()
        && !remaining_lots.as_array().unwrap().is_empty()
        && remaining_owned.map(|x| x > 0).unwrap_or(false))
    {
        return Err(err("LOTS"));
    }
    let mut owned: i64 = 0;
    for lot in remaining_lots.as_array().unwrap() {
        let q = js_int(lot.get("quantity").unwrap_or(&V::Null));
        if q.map(|x| x <= 0).unwrap_or(true) {
            return Err(err("LOT_QUANTITY"));
        }
        // price() is called for validation only; its error propagates raw.
        price(lot.get("entryPrice").and_then(V::as_str).unwrap_or(""))?;
        owned += q.unwrap();
    }
    if owned != remaining_owned.unwrap() {
        return Err(err("OWNED_TOTAL"));
    }

    let realized = amount(v("realizedProfit").as_str().unwrap_or("\u{0}"))?;
    let floor = amount(opt_str(input, "minimumCohortProfit", "0.005000"))?;
    let order_cap = amount(opt_str(input, "maximumOrderCost", "25.000000"))?;
    let fees = opt_str(input, "feesPerShare", "0.000000");
    let protective_ticks = match input.get("protectiveTicks") {
        None => 1_i64,
        Some(x) => js_int(x).unwrap_or(-1),
    };
    let maximum_shares_per_order = match input.get("maximumSharesPerOrder") {
        None => 100_i64,
        Some(x) => js_int(x).unwrap_or(-1),
    };
    if !(floor >= TICK
        && fees == "0.000000"
        && (protective_ticks == 0 || protective_ticks == 1)
        && order_cap > 0
        && order_cap <= 100_000_000
        && maximum_shares_per_order > 0
        && maximum_shares_per_order <= 100)
    {
        return Err(err("POLICY"));
    }

    let yes_ask_level = v("yesAskLevel");
    let ask_quantity = js_int(yes_ask_level.get("quantity").unwrap_or(&V::Null));
    if !(js_truthy(Some(yes_ask_level)) && ask_quantity.map(|q| q > 0).unwrap_or(false)) {
        return Err(err("DEPTH"));
    }
    let ask_price = price(yes_ask_level.get("price").and_then(V::as_str).unwrap_or(""))?;
    let limit = SCALE - ask_price - protective_ticks as i128 * TICK;
    if !(limit >= TICK && limit <= SCALE - TICK && limit % TICK == 0) {
        return Err(err("LIMIT"));
    }

    let cap = remaining_owned
        .unwrap()
        .min(ask_quantity.unwrap())
        .min(maximum_shares_per_order)
        .min((order_cap / limit) as i64);
    let mut quantity: i64 = 0;
    let mut principal: i128 = 0;
    let mut delta: i128 = 0;
    let mut worst: i128 = realized;
    let mut blocked = false;
    let mut prefixes: Vec<V> = Vec::new();
    {
        for lot in remaining_lots.as_array().unwrap() {
            let lot_price = price(lot.get("entryPrice").and_then(V::as_str).unwrap_or(""))?;
            for _i in 0..js_int(lot.get("quantity").unwrap_or(&V::Null)).unwrap() {
                if quantity >= cap {
                    break;
                }
                let next = delta + limit - lot_price;
                if realized + next < floor {
                    blocked = true;
                    break;
                }
                delta = next;
                quantity += 1;
                principal += lot_price;
                if realized + delta < worst {
                    worst = realized + delta;
                }
                prefixes.push(json!({
                    "quantity": quantity,
                    "cohortRealizedLowerBound": money(realized + delta),
                }));
            }
            if blocked || quantity == cap {
                break;
            }
        }
    }

    let mut result = json!({
        "schemaVersion": 1,
        "kind": "cohort_profit_locked_FIFO_NO_exit_v1",
        "quantity": quantity,
        "protectedLimit": money(limit),
        "orderNotional": money(limit * quantity as i128),
        "removedPrincipalIfFilled": money(principal),
        "protectedTradePnlIfFilled": money(delta),
        "cohortRealizedBefore": v("realizedProfit").clone(),
        "cohortRealizedIfFullyFilledLowerBound": money(realized + delta),
        "worstPartialFillCohortLowerBound": money(worst),
        "minimumCohortProfit": opt_str(input, "minimumCohortProfit", "0.005000"),
        "partialFillBounds": prefixes,
        "sourceLotsHash": payload_hash(remaining_lots)?,
        "sourceLevelHash": payload_hash(yes_ask_level)?,
        "fees": fees,
        "executionAuthority": false,
        "actualFill": false,
    });
    let h = payload_hash(&result)?;
    result
        .as_object_mut()
        .unwrap()
        .insert("proposalHash".into(), json!(h));
    Ok(result)
}
