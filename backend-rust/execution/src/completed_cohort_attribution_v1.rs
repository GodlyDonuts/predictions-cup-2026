// execution/completed-cohort-attribution-v1.mjs — verified-proof-only FIFO
// cohort attribution across completed markets. Throws on duplicates,
// unverified/zero-fee violations, bad clocks/quantities, unowned sells, and
// any group that is not flat or whose proceeds-cost != realized.
//
// Port notes:
// - JS Map groups -> insertion-ordered Vec<(key, Group)>; keys are matched by
//   JSON deep equality (exchangeId is a primitive in every recorded order).
// - JS BigInt micro money -> i128; hold-time sums stay exact f64 like JS
//   Number arithmetic.
// - new Date(null|NaN).toISOString() throws RangeError only for NaN; null ->
//   epoch — reproduced via Option<f64> + epoch fallback.
// - Math.abs(f.quantity) then Number.isSafeInteger — a non-numeric quantity
//   coerces to NaN and fails the integer check (QUANTITY).
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::fixed::{amount, money};
use baal_service_runtime::json_text::{js_int, js_truthy};
use serde_json::{json, Map, Value as V};
use std::collections::HashSet;

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}

fn sget<'a>(v: &'a V, k: &str) -> Option<&'a str> {
    v.get(k).and_then(V::as_str)
}

fn dparse(v: &V) -> f64 {
    v.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN)
}

#[derive(Default)]
struct Lot {
    quantity: i64,
    at: f64,
}

#[derive(Default)]
struct Group {
    exchange_id: V,
    market_id: V,
    bought: i64,
    sold: i64,
    cost: i128,
    proceeds: i128,
    realized: i128,
    first_buy: Option<f64>,
    last_sell: Option<f64>,
    weighted_hold_share_ms: f64,
    lots: Vec<Lot>,
    orders: i64,
    dispatch_fill_latencies_ms: Vec<f64>,
}

/// attributeCompletedCohortV1(orders)
pub fn attribute_completed_cohort_v1(orders: &V) -> Result<V, String> {
    let orders = orders
        .as_array()
        .ok_or_else(|| "orders is not iterable".to_string())?;
    let mut groups: Vec<(V, Group)> = Vec::new();
    let mut ids: HashSet<String> = HashSet::new();
    let mut invested: i128 = 0;
    let mut proceeds: i128 = 0;
    let mut realized: i128 = 0;
    let mut orders_count: i64 = 0;
    for o in orders {
        if !js_truthy(Some(get(o, "proof"))) {
            continue;
        }
        let id_key = baal_service_runtime::json_text::js_stringify(get(o, "intentId"));
        if ids.contains(&id_key) {
            return Err("COHORT_ATTRIBUTION_DUPLICATE".to_string());
        }
        ids.insert(id_key);
        let p = get(o, "proof");
        if !(js_truthy(Some(get(p, "scopedTradeVerified")))
            && sget(p, "fee") == Some("0.000000")
            && matches!(sget(get(o, "body"), "action"), Some("buy") | Some("sell")))
        {
            return Err("COHORT_ATTRIBUTION_PROOF".to_string());
        }
        let ex = get(get(o, "body"), "exchangeId").clone();
        let gi = match groups.iter().position(|(k, _)| *k == ex) {
            Some(i) => i,
            None => {
                groups.push((
                    ex,
                    Group {
                        exchange_id: get(get(o, "body"), "exchangeId").clone(),
                        market_id: get(o, "marketId").clone(),
                        ..Group::default()
                    },
                ));
                groups.len() - 1
            }
        };
        let g = &mut groups[gi].1;
        g.orders += 1;
        orders_count += 1;
        let fills = get(p, "fills").as_array().cloned().unwrap_or_default();
        let times: Vec<f64> = fills.iter().map(|f| dparse(get(f, "filledAt"))).collect();
        if times.iter().any(|t| !t.is_finite()) {
            return Err("COHORT_ATTRIBUTION_CLOCK".to_string());
        }
        let order_created = dparse(get(o, "createdAt"));
        for (i, f) in fills.iter().enumerate() {
            // Math.abs coerces non-numbers to NaN; isSafeInteger then fails.
            let quantity = get(f, "quantity").as_f64().map(|q| q.abs());
            let t = times[i];
            match quantity.and_then(|q| {
                if q.is_finite() && q.fract() == 0.0 && q.abs() <= 9_007_199_254_740_991.0 {
                    Some(q as i64)
                } else {
                    None
                }
            }) {
                Some(q) if q >= 1 => {
                    if sget(get(o, "body"), "action") == Some("buy") {
                        g.bought += q;
                        g.first_buy = Some(g.first_buy.map_or(t, |b| b.min(t)));
                        g.lots.push(Lot { quantity: q, at: t });
                    } else {
                        g.sold += q;
                        g.last_sell = Some(g.last_sell.map_or(t, |s| s.max(t)));
                        let mut left = q;
                        for lot in g.lots.iter_mut() {
                            let take = left.min(lot.quantity);
                            if t < lot.at {
                                return Err("COHORT_ATTRIBUTION_FIFO_CLOCK".to_string());
                            }
                            g.weighted_hold_share_ms += take as f64 * (t - lot.at);
                            lot.quantity -= take;
                            left -= take;
                            if left == 0 {
                                break;
                            }
                        }
                        if left != 0 {
                            return Err("COHORT_ATTRIBUTION_UNOWNED_SELL".to_string());
                        }
                    }
                    g.dispatch_fill_latencies_ms.push(t - order_created);
                }
                _ => return Err("COHORT_ATTRIBUTION_QUANTITY".to_string()),
            }
        }
        let cost = amount(sget(p, "scopedPrincipalCost").unwrap_or("\u{0}"))?;
        let cash = amount(sget(p, "scopedProceeds").unwrap_or("\u{0}"))?;
        let delta = amount(sget(p, "realizedPnl").unwrap_or("\u{0}"))?;
        g.cost += cost;
        g.proceeds += cash;
        g.realized += delta;
        invested += cost;
        proceeds += cash;
        realized += delta;
    }
    let mut markets: Vec<V> = Vec::new();
    for (_, g) in groups.iter() {
        if g.bought != g.sold || g.lots.iter().any(|l| l.quantity != 0) {
            return Err("COHORT_ATTRIBUTION_NOT_FLAT".to_string());
        }
        if g.proceeds - g.cost != g.realized {
            return Err("COHORT_ATTRIBUTION_ECONOMICS".to_string());
        }
        let mut m = Map::new();
        m.insert("exchangeId".into(), g.exchange_id.clone());
        m.insert("marketId".into(), g.market_id.clone());
        m.insert("bought".into(), json!(g.bought));
        m.insert("sold".into(), json!(g.sold));
        m.insert("buyCost".into(), json!(money(g.cost)));
        m.insert("sellProceeds".into(), json!(money(g.proceeds)));
        m.insert("realized".into(), json!(money(g.realized)));
        m.insert(
            "costReturnPercent".into(),
            json!(g.realized as f64 * 100.0 / g.cost as f64),
        );
        m.insert(
            "weightedHoldingHours".into(),
            json!(g.weighted_hold_share_ms / g.sold as f64 / 3_600_000.0),
        );
        m.insert(
            "firstBuyAt".into(),
            json!(ms_to_iso(g.first_buy.unwrap_or(0.0))),
        );
        m.insert(
            "lastSellAt".into(),
            json!(ms_to_iso(g.last_sell.unwrap_or(0.0))),
        );
        m.insert("orders".into(), json!(g.orders));
        m.insert(
            "dispatchFillLatenciesMs".into(),
            json!(g.dispatch_fill_latencies_ms),
        );
        markets.push(V::Object(m));
    }
    let mut out = Map::new();
    out.insert("schemaVersion".into(), json!(1));
    out.insert("markets".into(), V::Array(markets.clone()));
    out.insert("verifiedOrders".into(), json!(orders_count));
    out.insert("invested".into(), json!(money(invested)));
    out.insert("proceeds".into(), json!(money(proceeds)));
    out.insert("realized".into(), json!(money(realized)));
    out.insert(
        "returnOnDeployedPrincipalPercent".into(),
        json!(realized as f64 * 100.0 / invested as f64),
    );
    out.insert("fees".into(), json!("0.000000"));
    out.insert("sampleMarkets".into(), json!(markets.len() as i64));
    out.insert("scalableEdgeEstablished".into(), json!(false));
    Ok(V::Object(out))
}
