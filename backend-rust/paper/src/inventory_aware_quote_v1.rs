// paper/inventory-aware-quote-v1.mjs — research adaptation of inventory
// reservation pricing. No arrival intensity or profitability is presumed;
// maker fills require a separate causal model.
use crate::fixed::{big_amount, big_price, instant, money, SCALE, TICK};
use baal_service_runtime::json_text::sha256;
use num_bigint::BigInt;
use serde_json::{json, Map, Value as V};

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn need(v: bool, c: &str) -> Result<(), String> {
    if !v {
        return Err(format!("INVENTORY_QUOTE_{c}"));
    }
    Ok(())
}

// ../execution/core.mjs canonicalPayload: sorted-key JSON walk (UTF-16 key
// order) hashed with sha256. Local copy — baal-paper does not depend on
// baal-execution.
fn canonical_payload(value: &V) -> Result<String, String> {
    fn walk(v: &V, out: &mut String) -> Result<(), String> {
        match v {
            V::Null => out.push_str("null"),
            V::Bool(_) | V::Number(_) | V::String(_) => {
                out.push_str(&serde_json::to_string(v).map_err(|e| e.to_string())?)
            }
            V::Array(a) => {
                out.push('[');
                for (i, x) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    if x.is_null() {
                        out.push_str("null");
                    } else {
                        walk(x, out)?;
                    }
                }
                out.push(']');
            }
            V::Object(m) => {
                out.push('{');
                let mut keys: Vec<&String> = m.keys().collect();
                keys.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
                for (i, k) in keys.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&serde_json::to_string(k).map_err(|e| e.to_string())?);
                    out.push(':');
                    walk(&m[*k], out)?;
                }
                out.push('}');
            }
        }
        Ok(())
    }
    let mut s = String::new();
    walk(value, &mut s)?;
    Ok(s)
}
fn payload_hash(value: &V) -> Result<String, String> {
    Ok(sha256(canonical_payload(value)?.as_bytes()))
}

pub fn inventory_aware_quotes_v1(input: &V) -> Result<V, String> {
    let book = get(input, "book");
    let inventory = get(input, "inventory");
    let variance_per_second = get(input, "variancePerSecond")
        .as_f64()
        .unwrap_or(f64::NAN);
    let risk_aversion = get(input, "riskAversion").as_f64().unwrap_or(10.0);
    let horizon_seconds = get(input, "horizonSeconds")
        .as_f64()
        .unwrap_or(300.0);
    let inventory_scale = get(input, "inventoryScale").as_i64().unwrap_or(100);
    let maximum_shares = get(input, "maximumShares").as_i64().unwrap_or(100);
    let maximum_order_cost = get(input, "maximumOrderCost")
        .as_str()
        .unwrap_or("100.000000");
    let now = get(input, "now").as_str().unwrap_or("");

    let at = instant(now)?;
    need(
        !book.is_null()
            && at - instant(get(book, "requestStartedAt").as_str().unwrap_or(""))? <= 20000
            && instant(get(book, "receivedAt").as_str().unwrap_or(""))? <= at
            && instant(get(book, "requestStartedAt").as_str().unwrap_or(""))?
                <= instant(get(book, "receivedAt").as_str().unwrap_or(""))?,
        "FRESH_BOOK",
    )?;
    let b = get(book, "bids").as_array().and_then(|a| a.first());
    let a = get(book, "asks").as_array().and_then(|a| a.first());
    let b_qty = b.map(|x| get(x, "quantity").as_i64());
    let a_qty = a.map(|x| get(x, "quantity").as_i64());
    need(
        b.is_some()
            && a.is_some()
            && matches!(b_qty, Some(Some(q)) if q > 0)
            && matches!(a_qty, Some(Some(q)) if q > 0),
        "DEPTH",
    )?;
    let bid = big_price(get(b.unwrap(), "price").as_str().unwrap_or(""))?;
    let ask = big_price(get(a.unwrap(), "price").as_str().unwrap_or(""))?;
    need(bid < ask, "UNCROSSED")?;
    need(
        ["yes", "no"]
            .iter()
            .all(|k| matches!(get(inventory, k).as_i64(), Some(q) if q >= 0))
            && variance_per_second.is_finite()
            && variance_per_second >= 0.0
            && variance_per_second <= 0.25
            && risk_aversion.is_finite()
            && risk_aversion >= 0.0
            && risk_aversion <= 1000.0
            && horizon_seconds.is_finite()
            && horizon_seconds > 0.0
            && horizon_seconds <= 3600.0
            && inventory_scale > 0
            && maximum_shares > 0
            && maximum_shares <= 100,
        "PARAMETERS",
    )?;
    let cash = big_amount(get(input, "cashAvailable").as_str().unwrap_or(""))?;
    let cap = big_amount(maximum_order_cost)?;
    need(
        cash >= BigInt::from(0) && cap > BigInt::from(0) && cap <= BigInt::from(100_000_000i64),
        "FUNDED_CAP",
    )?;

    let inv_yes = get(inventory, "yes").as_i64().unwrap_or(0);
    let inv_no = get(inventory, "no").as_i64().unwrap_or(0);
    let bq = get(b.unwrap(), "quantity").as_i64().unwrap_or(0);
    let aq = get(a.unwrap(), "quantity").as_i64().unwrap_or(0);
    let bid_f = bid.to_string().parse::<f64>().unwrap_or(0.0);
    let ask_f = ask.to_string().parse::<f64>().unwrap_or(0.0);
    let midpoint = (bid.clone() + ask.clone())
        .to_string()
        .parse::<f64>()
        .unwrap_or(0.0)
        / 2e6;
    let microprice =
        (ask_f * bq as f64 + bid_f * aq as f64) / (bq + aq) as f64 / 1e6;
    let q = (inv_yes - inv_no) as f64 / inventory_scale as f64;
    let penalty = risk_aversion * variance_per_second * horizon_seconds;
    let reservation = microprice - q * penalty;
    let half_spread = f64::max(0.005, (ask_f - bid_f) / 2e6 + penalty / 2.0);

    let buy_ticks = (((reservation - half_spread) / 0.005).floor() as i64).clamp(1, 199);
    let sell_ticks = (((reservation + half_spread) / 0.005).ceil() as i64).clamp(1, 199);
    need(buy_ticks < sell_ticks, "NONCROSSED_QUOTING")?;
    let yes_buy = BigInt::from(buy_ticks) * BigInt::from(TICK);
    let yes_sell = BigInt::from(sell_ticks) * BigInt::from(TICK);
    let scale = BigInt::from(SCALE);
    let no_buy = &scale - &yes_sell;
    let no_sell = &scale - &yes_buy;

    let buy_budget = if cash < cap.clone() { cash.clone() } else { cap.clone() };
    let one_unit_package = &yes_buy + &no_buy;
    let mut buys: Vec<V> = Vec::new();
    let mut sells: Vec<V> = Vec::new();
    let mut reserved = BigInt::from(0);

    // Reserve distinct old shares for direct sale and opposite-side
    // acquisition. Otherwise both fills could consume the same lot and create
    // a naked sale.
    let sell_yes = if inv_yes > 0 {
        std::cmp::min(maximum_shares, std::cmp::max(1, inv_yes / 2))
    } else {
        0
    };
    let sell_no = if inv_no > 0 {
        std::cmp::min(maximum_shares, std::cmp::max(1, inv_no / 2))
    } else {
        0
    };
    if sell_yes > 0 {
        sells.push(json!({"side":"YES","action":"sell","quantity":sell_yes,"limit":money(&yes_sell)}));
    }
    if sell_no > 0 {
        sells.push(json!({"side":"NO","action":"sell","quantity":sell_no,"limit":money(&no_sell)}));
    }
    let mut add_buy = |side: &str,
                       limit: &BigInt,
                       wanted: i64,
                       reserved: &mut BigInt,
                       buys: &mut Vec<V>| {
        if wanted < 1 {
            return;
        }
        let affordable = ((&buy_budget - &*reserved) / limit)
            .to_string()
            .parse::<i64>()
            .unwrap_or(0);
        let quantity = std::cmp::min(maximum_shares, std::cmp::min(wanted, affordable));
        if quantity > 0 {
            *reserved += limit * BigInt::from(quantity);
            buys.push(json!({"side":side,"action":"buy","quantity":quantity,"limit":money(limit)}));
        }
    };
    let ask_bi = ask.clone();
    let bid_bi = bid.clone();
    if yes_buy < ask_bi && no_buy < &scale - &bid_bi {
        if inv_yes == 0 && inv_no == 0 {
            let units = std::cmp::min(
                maximum_shares,
                (&buy_budget / &one_unit_package)
                    .to_string()
                    .parse::<i64>()
                    .unwrap_or(0),
            );
            add_buy("YES", &yes_buy, units, &mut reserved, &mut buys);
            add_buy("NO", &no_buy, units, &mut reserved, &mut buys);
        } else {
            let want_yes = if inv_no > 0 {
                inv_no - sell_no
            } else {
                std::cmp::max(0, maximum_shares - inv_yes)
            };
            add_buy("YES", &yes_buy, want_yes, &mut reserved, &mut buys);
            let want_no = if inv_yes > 0 {
                inv_yes - sell_yes
            } else {
                std::cmp::max(0, maximum_shares - inv_no)
            };
            add_buy("NO", &no_buy, want_no, &mut reserved, &mut buys);
        }
    }

    let mut out = Map::new();
    out.insert("schemaVersion".into(), json!(1));
    out.insert(
        "mode".into(),
        json!("bounded_inventory_reservation_quote_research_v1"),
    );
    out.insert("marketId".into(), get(book, "marketId").clone());
    out.insert("exchangeId".into(), get(book, "exchangeId").clone());
    out.insert("at".into(), json!(now));
    out.insert("midpoint".into(), json!(midpoint));
    out.insert("microprice".into(), json!(microprice));
    out.insert("inventoryUnits".into(), json!(q));
    out.insert("reservationPrice".into(), json!(reservation));
    out.insert("variancePerSecond".into(), json!(variance_per_second));
    out.insert("riskAversion".into(), json!(risk_aversion));
    out.insert("horizonSeconds".into(), json!(horizon_seconds));
    out.insert("halfSpread".into(), json!(half_spread));
    out.insert("buys".into(), json!(buys));
    out.insert("sells".into(), json!(sells));
    out.insert(
        "maximumQuoteReservation".into(),
        json!(money(&reserved)),
    );
    out.insert(
        "conditionalComplementSurplusPerPairedUnit".into(),
        json!(money(&(&scale - &one_unit_package))),
    );
    out.insert("fees".into(), json!("0.000000"));
    out.insert("executionAuthority".into(), json!(false));
    out.insert("networkRequests".into(), json!(0));
    out.insert("estimatedFillProbability".into(), V::Null);
    out.insert("profitabilityEstablished".into(), json!(false));
    out.insert(
        "assumptions".into(),
        json!([
            "Microprice is a heuristic rather than a calibrated future-value forecast.",
            "Arrival rates and queue position are not estimated.",
            "Inventory-driven ask can realize a loss; no profitability floor is fabricated.",
            "Only later independently observed fills may affect a paper wallet."
        ]),
    );
    let out = V::Object(out);
    let mut result = out.as_object().unwrap().clone();
    result.insert("proposalHash".into(), json!(payload_hash(&out)?));
    Ok(V::Object(result))
}
