// execution/cohort-profit-locked-no-exit-v1.mjs — alternative to refusing
// every losing lot: spend only already-realized cohort gains, and protect the
// profit floor for EVERY possible FIFO partial fill.
use baal_execution::core::payload_hash;
use baal_service_runtime::fixed::{amount, money, price, SCALE, TICK};
use baal_service_runtime::json_text::{js_int, js_truthy};
use serde_json::{json, Map, Value as V};

fn need(x: bool, c: &str) -> Result<(), String> {
    if x {
        Ok(())
    } else {
        Err(format!("COHORT_EXIT_{c}"))
    }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
// Destructured default applies only when the key is absent (or undefined);
// an explicit null stays null. `dflt` is used when absent; a present value is
// returned as-is (non-string callers compare it verbatim, which fails).
fn str_param<'a>(input: &'a V, k: &str, dflt: &'a str) -> &'a str {
    match input.get(k) {
        None => dflt,
        Some(v) => v.as_str().unwrap_or("\u{7f}"),
    }
}
// Destructured default for raw JSON outputs: the verbatim param value or the
// default literal when the key is absent.
fn val_param(input: &V, k: &str, dflt: &str) -> V {
    input.get(k).cloned().unwrap_or(json!(dflt))
}

/// sizeCohortProfitLockedNoExitV1({remainingLots,remainingOwnedQuantity,
/// yesAskLevel,realizedProfit,minimumCohortProfit='0.005000',protectiveTicks=1,
/// maximumOrderCost='25.000000',maximumSharesPerOrder=100,feesPerShare='0.000000'})
pub fn size_cohort_profit_locked_no_exit_v1(input: &V) -> Result<V, String> {
    let remaining_lots = get(input, "remainingLots");
    let remaining_owned_quantity = js_int(get(input, "remainingOwnedQuantity"));
    need(
        remaining_lots.is_array()
            && !remaining_lots.as_array().unwrap().is_empty()
            && remaining_owned_quantity.is_some_and(|q| q > 0),
        "LOTS",
    )?;
    let mut owned: i128 = 0;
    for lot in remaining_lots.as_array().unwrap() {
        let lot_quantity = js_int(get(lot, "quantity"));
        need(lot_quantity.is_some_and(|q| q > 0), "LOT_QUANTITY")?;
        price(get(lot, "entryPrice").as_str().unwrap_or(""))?;
        owned += lot_quantity.unwrap() as i128;
    }
    need(
        owned == remaining_owned_quantity.unwrap() as i128,
        "OWNED_TOTAL",
    )?;
    let minimum_cohort_profit = str_param(input, "minimumCohortProfit", "0.005000");
    let maximum_order_cost = str_param(input, "maximumOrderCost", "25.000000");
    let fees_per_share = str_param(input, "feesPerShare", "0.000000");
    let realized = amount(get(input, "realizedProfit").as_str().unwrap_or(""))?;
    let floor = amount(minimum_cohort_profit)?;
    let order_cap = amount(maximum_order_cost)?;
    let protective_ticks = js_int(input.get("protectiveTicks").unwrap_or(&json!(1)));
    let maximum_shares_per_order = js_int(input.get("maximumSharesPerOrder").unwrap_or(&json!(100)));
    need(
        floor >= TICK
            && fees_per_share == "0.000000"
            && protective_ticks.is_some_and(|t| [0i64, 1i64].contains(&t))
            && order_cap > 0
            && order_cap <= 25_000_000
            && maximum_shares_per_order.is_some_and(|n| (1..=100).contains(&n)),
        "POLICY",
    )?;
    let yes_ask_level = get(input, "yesAskLevel");
    need(
        js_truthy(Some(yes_ask_level))
            && js_int(get(yes_ask_level, "quantity")).is_some_and(|q| q > 0),
        "DEPTH",
    )?;
    let limit = SCALE
        - price(get(yes_ask_level, "price").as_str().unwrap_or(""))?
        - protective_ticks.unwrap() as i128 * TICK;
    need(
        limit >= TICK && limit <= SCALE - TICK && limit % TICK == 0,
        "LIMIT",
    )?;
    let cap = (remaining_owned_quantity.unwrap() as i128)
        .min(js_int(get(yes_ask_level, "quantity")).unwrap() as i128)
        .min(maximum_shares_per_order.unwrap() as i128)
        .min(order_cap / limit);
    let mut quantity: i128 = 0;
    let mut principal: i128 = 0;
    let mut delta: i128 = 0;
    let mut worst = realized;
    let mut blocked = false;
    let mut prefixes: Vec<V> = Vec::new();
    if realized >= floor {
        'lots: for lot in remaining_lots.as_array().unwrap() {
            let lot_quantity = js_int(get(lot, "quantity")).unwrap();
            let entry = price(get(lot, "entryPrice").as_str().unwrap_or(""))?;
            for _ in 0..lot_quantity {
                if quantity >= cap {
                    break 'lots;
                }
                let next = delta + limit - entry;
                if realized + next < floor {
                    blocked = true;
                    break;
                }
                delta = next;
                quantity += 1;
                principal += entry;
                if realized + delta < worst {
                    worst = realized + delta;
                }
                prefixes.push(json!({
                    "quantity": quantity as i64,
                    "cohortRealizedLowerBound": money(realized + delta),
                }));
            }
            if blocked {
                break;
            }
        }
    }
    let result = json!({
        "schemaVersion": 1,
        "kind": "cohort_profit_locked_FIFO_NO_exit_v1",
        "quantity": quantity as i64,
        "protectedLimit": money(limit),
        "orderNotional": money(limit * quantity),
        "removedPrincipalIfFilled": money(principal),
        "protectedTradePnlIfFilled": money(delta),
        "cohortRealizedBefore": get(input, "realizedProfit"),
        "cohortRealizedIfFullyFilledLowerBound": money(realized + delta),
        "worstPartialFillCohortLowerBound": money(worst),
        "minimumCohortProfit": val_param(input, "minimumCohortProfit", "0.005000"),
        "partialFillBounds": prefixes,
        "sourceLotsHash": payload_hash(remaining_lots)?,
        "sourceLevelHash": payload_hash(yes_ask_level)?,
        "fees": val_param(input, "feesPerShare", "0.000000"),
        "executionAuthority": false,
        "actualFill": false,
    });
    let mut out: Map<String, V> = result.as_object().cloned().unwrap_or_default();
    out.insert("proposalHash".into(), json!(payload_hash(&result)?));
    Ok(V::Object(out))
}
