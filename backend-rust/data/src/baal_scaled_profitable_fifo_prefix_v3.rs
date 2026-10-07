// execution/baal-scaled-profitable-fifo-prefix-v3.mjs — pure prefix sizing for
// the explicit market-price or cushioned every-consumed-lot profit policy. No
// averaging past an expensive first lot, FIFO skipping, mutation or dispatch.
use baal_execution::core::payload_hash;
use baal_service_runtime::fixed::{amount, money, price, SCALE, TICK};
use baal_service_runtime::json_text::{js_int, js_truthy};
use serde_json::{json, Value as V};

fn need(x: bool, c: &str) -> Result<(), String> {
    if x { Ok(()) } else { Err(format!("FIFO_EXIT_PREFIX_{c}")) }
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

/// sizeProfitableOwnedNoExitPrefixV3({remainingLots,remainingOwnedQuantity,
/// yesAskLevel,requestedQuantity=null,maximumSharesPerOrder=100,
/// maximumOrderCost='25.000000',feesPerShare='0.000000',
/// minimumNetProfitPerShare='0.005000',protectiveTicks=0})
pub fn size_profitable_owned_no_exit_prefix_v3(input: &V) -> V {
    // JS binds the raw param into `base`; a non-integer/null only fails later.
    let protective_ticks_raw = input.get("protectiveTicks").cloned().unwrap_or(json!(0));
    let base = json!({
        "schemaVersion": 1,
        "mode": "baal_profitable_fifo_no_exit_prefix_v3",
        "protectiveTicks": protective_ticks_raw,
        "quantity": 0,
        "orderNotional": "0.000000",
        "costProtection": "each_consumed_fifo_lot",
        "fifoOrderPreserved": true,
        "inventoryChanged": false,
        "orderEligible": false,
        "executionAuthority": false,
        "actualFillOrRealizedProfit": false,
    });
    (|| -> Result<V, String> {
        let remaining_lots = get(input, "remainingLots");
        need(remaining_lots.is_array() && !remaining_lots.as_array().unwrap().is_empty(), "OWNED_LOTS")?;
        let remaining_owned_quantity = js_int(get(input, "remainingOwnedQuantity"));
        need(remaining_owned_quantity.is_some_and(|q| q > 0), "OWNED_LOTS")?;
        let mut owned: i128 = 0;
        for l in remaining_lots.as_array().unwrap() {
            need(js_int(get(l, "quantity")).is_some_and(|q| q > 0), "EXACT_LOT_QUANTITY")?;
            price(get(l, "entryPrice").as_str().unwrap_or(""))?;
            owned += js_int(get(l, "quantity")).unwrap() as i128;
            need(owned <= 9_007_199_254_740_991, "OWNED_TOTAL")?;
        }
        need(owned == remaining_owned_quantity.unwrap() as i128, "OWNED_TOTAL")?;

        // requestedQuantity===null (absent) is fine; present-and-non-integer
        // or <1 fails SHARE_CAP.
        let rq = input.get("requestedQuantity");
        let rq_present = rq.is_some_and(|v| !v.is_null());
        let requested_quantity = rq.and_then(js_int);
        let maximum_shares_per_order = js_int(input.get("maximumSharesPerOrder").unwrap_or(&json!(100)));
        need(
            maximum_shares_per_order.is_some_and(|n| (1..=100).contains(&n))
                && (!rq_present
                    || (requested_quantity.is_some() && requested_quantity.unwrap() >= 1)),
            "SHARE_CAP",
        )?;
        let maximum_shares_per_order = maximum_shares_per_order.unwrap();
        let maximum_order_cost = str_param(input, "maximumOrderCost", "25.000000");
        let fees_per_share = str_param(input, "feesPerShare", "0.000000");
        let minimum_net_profit_per_share = str_param(input, "minimumNetProfitPerShare", "0.005000");
        let maximum_cost = amount(maximum_order_cost)?;
        let protective_ticks = js_int(&protective_ticks_raw);
        need(
            maximum_cost > 0
                && maximum_cost <= 25_000_000
                && fees_per_share == "0.000000"
                && minimum_net_profit_per_share == "0.005000"
                && protective_ticks.is_some_and(|t| [0i64, 1i64].contains(&t)),
            "UNCHANGED_EXACT_POLICY",
        )?;
        let protective_ticks = protective_ticks.unwrap();
        let yes_ask_level = get(input, "yesAskLevel");
        need(
            js_truthy(Some(yes_ask_level))
                && js_int(get(yes_ask_level, "quantity")).is_some_and(|q| q > 0),
            "EXACT_DEPTH",
        )?;
        let raw_no_bid = SCALE - price(get(yes_ask_level, "price").as_str().unwrap_or(""))?;
        let limit = raw_no_bid - protective_ticks as i128 * TICK;
        need(limit >= TICK && limit <= SCALE - TICK && limit % TICK == 0, "PROTECTED_LIMIT")?;
        let depth = js_int(get(yes_ask_level, "quantity")).unwrap();
        let cap = (requested_quantity.unwrap_or(maximum_shares_per_order) as i128)
            .min(maximum_shares_per_order as i128)
            .min(remaining_owned_quantity.unwrap() as i128)
            .min(depth as i128)
            .min(maximum_cost / limit);
        let mut lots: Vec<V> = Vec::new();
        let mut quantity: i128 = 0;
        let mut principal: i128 = 0;
        let mut max_cost: i128 = 0;
        let mut first_blocked_lot: V = V::Null;
        for (index, l) in remaining_lots.as_array().unwrap().iter().enumerate() {
            if quantity >= cap {
                break;
            }
            let cost = price(get(l, "entryPrice").as_str().unwrap_or(""))?;
            if limit < cost + TICK {
                first_blocked_lot = json!({
                    "index": index,
                    "entryPrice": get(l, "entryPrice"),
                    "quantity": get(l, "quantity"),
                    "reason": "FIRST_NONPROFITABLE_FIFO_LOT",
                });
                break;
            }
            let take = std::cmp::min(js_int(get(l, "quantity")).unwrap() as i128, cap - quantity);
            quantity += take;
            principal += cost * take;
            if cost > max_cost {
                max_cost = cost;
            }
            lots.push(json!({
                "index": index,
                "quantity": take,
                "entryPrice": get(l, "entryPrice"),
                "principalCost": money(cost * take),
            }));
        }
        let mut body = base.clone();
        let m = body.as_object_mut().unwrap();
        m.insert("quantity".into(), json!(quantity));
        m.insert(
            "reason".into(),
            json!(if quantity > 0 {
                "PROFITABLE_CONTIGUOUS_FIFO_PREFIX"
            } else if cap == 0 {
                "NO_INDIVIDUAL_DEPTH_OR_COST_CAPACITY"
            } else {
                "FIRST_FIFO_LOT_CANNOT_CLEAR_PROTECTION"
            }),
        );
        m.insert("rawObservedNoBid".into(), json!(money(raw_no_bid)));
        m.insert("protectedLimit".into(), json!(money(limit)));
        m.insert("displayedIndividualLevelQuantity".into(), json!(depth));
        m.insert("depthPolicy".into(), json!("full_single_observed_rung"));
        m.insert("maximumAdmissibleQuantity".into(), json!(cap));
        m.insert(
            "requestedQuantity".into(),
            rq.cloned().unwrap_or(V::Null),
        );
        m.insert("maximumSharesPerOrder".into(), json!(maximum_shares_per_order));
        m.insert("maximumOrderCost".into(), json!(maximum_order_cost));
        m.insert("orderNotional".into(), json!(money(limit * quantity)));
        m.insert("removedPrincipalIfFilled".into(), json!(money(principal)));
        m.insert(
            "protectedNetProfitIfFilled".into(),
            json!(money(limit * quantity - principal)),
        );
        m.insert("minimumNetProfitPerShare".into(), json!(minimum_net_profit_per_share));
        m.insert(
            "maximumConsumedUnitCost".into(),
            if quantity != 0 { json!(money(max_cost)) } else { V::Null },
        );
        m.insert("consumedPrefixLots".into(), json!(lots));
        m.insert("firstBlockedLot".into(), first_blocked_lot);
        m.insert("fees".into(), json!("0.000000"));
        m.insert("sourceLotsHash".into(), json!(payload_hash(remaining_lots)?));
        m.insert("sourceLevelHash".into(), json!(payload_hash(yes_ask_level)?));
        m.insert("sourceOwnedQuantity".into(), json!(remaining_owned_quantity.unwrap()));
        let hash = payload_hash(&body)?;
        let mut out = body;
        out.as_object_mut()
            .unwrap()
            .insert("prefixSizingHash".into(), json!(hash));
        Ok(out)
    })()
    .unwrap_or_else(|e| {
        let mut out = base.clone();
        // /^FIFO_EXIT_PREFIX_[A-Z_]+$/
        let reason = if e.len() > "FIFO_EXIT_PREFIX_".len()
            && e.starts_with("FIFO_EXIT_PREFIX_")
            && e["FIFO_EXIT_PREFIX_".len()..]
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b == b'_')
        {
            e
        } else {
            "INVALID_FIFO_EXIT_PREFIX_EVIDENCE".to_string()
        };
        out.as_object_mut().unwrap().insert("reason".into(), json!(reason));
        out
    })
}
