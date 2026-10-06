// execution/baal-scaled-profitable-fifo-prefix-v3.mjs — pure prefix sizing for
// the explicit market-price or cushioned every-consumed-lot profit policy. No
// averaging past an expensive first lot, FIFO skipping, mutation or dispatch.
//
// Port notes:
// - JS BigInt money -> i128 micro units via baal_service_runtime::fixed.
// - `requestedQuantity=null` destructure default: absent OR explicit null.
// - `protectiveTicks=0` destructure default applies only when the key is absent
//   (JS destructuring defaults fire on `undefined`, never on explicit null).
// - `x ?? maximumSharesPerOrder` — nullish fallback only.
// - v3 uses the full single observed rung as depth (depthPolicy
//   'full_single_observed_rung'); v1's 1% floor is not applied.
// - catch: only FIFO_EXIT_PREFIX_* codes surface; anything else becomes
//   'INVALID_FIFO_EXIT_PREFIX_EVIDENCE'.
use crate::core::payload_hash;
use baal_service_runtime::fixed::{amount, money, price, SCALE, TICK};
use baal_service_runtime::json_text::js_int;
use serde_json::{json, Map, Value as V};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("FIFO_EXIT_PREFIX_{code}"))
    }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn amount_v(v: &V) -> Result<i128, String> {
    amount(v.as_str().unwrap_or(""))
}
fn price_v(v: &V) -> Result<i128, String> {
    price(v.as_str().unwrap_or(""))
}
fn owned_code(e: &str) -> bool {
    e.len() > "FIFO_EXIT_PREFIX_".len()
        && e.starts_with("FIFO_EXIT_PREFIX_")
        && e["FIFO_EXIT_PREFIX_".len()..]
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b == b'_')
}

/// sizeProfitableOwnedNoExitPrefixV3({remainingLots,remainingOwnedQuantity,
/// yesAskLevel,requestedQuantity=null,maximumSharesPerOrder=100,
/// maximumOrderCost='25.000000',feesPerShare='0.000000',
/// minimumNetProfitPerShare='0.005000',protectiveTicks=0})
pub fn size_profitable_owned_no_exit_prefix_v3(input: &V) -> V {
    let remaining_lots = get(input, "remainingLots");
    let remaining_owned_quantity = get(input, "remainingOwnedQuantity");
    let yes_ask_level = get(input, "yesAskLevel");
    let requested_quantity = input.get("requestedQuantity").cloned().unwrap_or(V::Null);
    let maximum_shares_per_order = input
        .get("maximumSharesPerOrder")
        .cloned()
        .unwrap_or(json!(100));
    let maximum_order_cost = input
        .get("maximumOrderCost")
        .cloned()
        .unwrap_or(json!("25.000000"));
    let fees_per_share = input
        .get("feesPerShare")
        .cloned()
        .unwrap_or(json!("0.000000"));
    let minimum_net_profit_per_share = input
        .get("minimumNetProfitPerShare")
        .cloned()
        .unwrap_or(json!("0.005000"));
    let protective_ticks = input.get("protectiveTicks").cloned().unwrap_or(json!(0));

    let base = json!({
        "schemaVersion": 1,
        "mode": "baal_profitable_fifo_no_exit_prefix_v3",
        "protectiveTicks": protective_ticks,
        "quantity": 0,
        "orderNotional": "0.000000",
        "costProtection": "each_consumed_fifo_lot",
        "fifoOrderPreserved": true,
        "inventoryChanged": false,
        "orderEligible": false,
        "executionAuthority": false,
        "actualFillOrRealizedProfit": false,
    });
    let inputs = (
        remaining_lots.clone(),
        remaining_owned_quantity.clone(),
        yes_ask_level.clone(),
        requested_quantity.clone(),
        maximum_shares_per_order.clone(),
        maximum_order_cost.clone(),
        fees_per_share.clone(),
        minimum_net_profit_per_share.clone(),
        protective_ticks.clone(),
    );
    match inner(&inputs, &base) {
        Ok(v) => v,
        Err(e) => {
            let mut out = base.as_object().cloned().unwrap_or_else(Map::new);
            out.insert(
                "reason".into(),
                json!(if owned_code(&e) {
                    e
                } else {
                    "INVALID_FIFO_EXIT_PREFIX_EVIDENCE".to_string()
                }),
            );
            V::Object(out)
        }
    }
}

#[allow(clippy::type_complexity)]
fn inner(inputs: &(V, V, V, V, V, V, V, V, V), base: &V) -> Result<V, String> {
    let (remaining_lots, remaining_owned_quantity, yes_ask_level, requested_quantity,
        maximum_shares_per_order, maximum_order_cost, fees_per_share,
        minimum_net_profit_per_share, protective_ticks) = inputs;
    need(
        remaining_lots.is_array()
            && remaining_lots.as_array().map(|a| !a.is_empty()).unwrap_or(false)
            && js_int(remaining_owned_quantity).map(|q| q > 0).unwrap_or(false),
        "OWNED_LOTS",
    )?;
    let mut owned: i64 = 0;
    for l in remaining_lots.as_array().unwrap() {
        need(
            js_int(get(l, "quantity")).map(|q| q > 0).unwrap_or(false),
            "EXACT_LOT_QUANTITY",
        )?;
        price_v(get(l, "entryPrice"))?;
        owned += js_int(get(l, "quantity")).unwrap();
        need(owned.unsigned_abs() <= 9_007_199_254_740_991, "OWNED_TOTAL")?;
    }
    need(owned == js_int(remaining_owned_quantity).unwrap_or(i64::MIN), "OWNED_TOTAL")?;
    let maximum_shares_per_order = js_int(maximum_shares_per_order).unwrap_or(i64::MIN);
    need(
        (1..=100).contains(&maximum_shares_per_order)
            && (requested_quantity.is_null()
                || js_int(requested_quantity).map(|q| q >= 1).unwrap_or(false)),
        "SHARE_CAP",
    )?;
    let maximum_cost = amount_v(maximum_order_cost)?;
    need(
        maximum_cost > 0
            && maximum_cost <= 25_000_000
            && fees_per_share == &json!("0.000000")
            && minimum_net_profit_per_share == &json!("0.005000")
            && (protective_ticks == &json!(0) || protective_ticks == &json!(1)),
        "UNCHANGED_EXACT_POLICY",
    )?;
    need(
        js_int(get(yes_ask_level, "quantity")).map(|q| q > 0).unwrap_or(false),
        "EXACT_DEPTH",
    )?;
    let raw_no_bid = SCALE - price_v(get(yes_ask_level, "price"))?;
    let limit = raw_no_bid - js_int(protective_ticks).unwrap_or(0) as i128 * TICK;
    need(limit >= TICK && limit <= SCALE - TICK && limit % TICK == 0, "PROTECTED_LIMIT")?;
    let depth = js_int(get(yes_ask_level, "quantity")).unwrap();
    let requested_or_cap = if requested_quantity.is_null() {
        maximum_shares_per_order
    } else {
        js_int(requested_quantity).unwrap_or(i64::MAX)
    };
    let cap = [
        requested_or_cap as i128,
        maximum_shares_per_order as i128,
        js_int(remaining_owned_quantity).unwrap() as i128,
        depth as i128,
        maximum_cost / limit,
    ]
    .into_iter()
    .min()
    .unwrap() as i64;
    let mut lots: Vec<V> = Vec::new();
    let mut quantity: i64 = 0;
    let mut principal: i128 = 0;
    let mut max_cost: i128 = 0;
    let mut first_blocked_lot = V::Null;
    for (index, l) in remaining_lots.as_array().unwrap().iter().enumerate() {
        if quantity >= cap {
            break;
        }
        let cost = price_v(get(l, "entryPrice"))?;
        if limit < cost + TICK {
            first_blocked_lot = json!({
                "index": index,
                "entryPrice": get(l, "entryPrice"),
                "quantity": get(l, "quantity"),
                "reason": "FIRST_NONPROFITABLE_FIFO_LOT",
            });
            break;
        }
        let take = std::cmp::min(js_int(get(l, "quantity")).unwrap(), cap - quantity);
        quantity += take;
        principal += cost * take as i128;
        if cost > max_cost {
            max_cost = cost;
        }
        lots.push(json!({
            "index": index,
            "quantity": take,
            "entryPrice": get(l, "entryPrice"),
            "principalCost": money(cost * take as i128),
        }));
    }
    let mut body = base.as_object().cloned().unwrap_or_else(Map::new);
    for (k, v) in [
        ("quantity", json!(quantity)),
        (
            "reason",
            json!(if quantity > 0 {
                "PROFITABLE_CONTIGUOUS_FIFO_PREFIX"
            } else if cap == 0 {
                "NO_INDIVIDUAL_DEPTH_OR_COST_CAPACITY"
            } else {
                "FIRST_FIFO_LOT_CANNOT_CLEAR_PROTECTION"
            }),
        ),
        ("rawObservedNoBid", json!(money(raw_no_bid))),
        ("protectedLimit", json!(money(limit))),
        ("displayedIndividualLevelQuantity", json!(depth)),
        ("depthPolicy", json!("full_single_observed_rung")),
        ("maximumAdmissibleQuantity", json!(cap)),
        ("requestedQuantity", requested_quantity.clone()),
        ("maximumSharesPerOrder", json!(maximum_shares_per_order)),
        ("maximumOrderCost", maximum_order_cost.clone()),
        ("orderNotional", json!(money(limit * quantity as i128))),
        ("removedPrincipalIfFilled", json!(money(principal))),
        (
            "protectedNetProfitIfFilled",
            json!(money(limit * quantity as i128 - principal)),
        ),
        ("minimumNetProfitPerShare", minimum_net_profit_per_share.clone()),
        (
            "maximumConsumedUnitCost",
            if quantity > 0 { json!(money(max_cost)) } else { V::Null },
        ),
        ("consumedPrefixLots", V::Array(lots)),
        ("firstBlockedLot", first_blocked_lot),
        ("fees", json!("0.000000")),
        ("sourceLotsHash", json!(payload_hash(remaining_lots)?)),
        ("sourceLevelHash", json!(payload_hash(yes_ask_level)?)),
        ("sourceOwnedQuantity", remaining_owned_quantity.clone()),
    ] {
        body.insert(k.to_string(), v);
    }
    let mut out = body.clone();
    out.insert("prefixSizingHash".into(), json!(payload_hash(&V::Object(body))?));
    Ok(V::Object(out))
}
