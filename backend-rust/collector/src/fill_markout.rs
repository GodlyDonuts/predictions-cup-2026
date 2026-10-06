// live-paper/fill-markout.mjs — post-fill markout versus published later books.
// Observed quote quality only. It does not create a sale, assume queue fills,
// change wallet accounting or treat hypothetical proceeds as realized profit.
use baal_paper::fixed::{big_amount, instant, money, TICK};
use baal_service_runtime::json_text::js_int;
use num_bigint::BigInt;
use serde_json::{json, Value as V};

fn need(v: bool, c: &str) -> Result<(), String> { if !v { return Err(format!("FILL_MARKOUT_{c}")); } Ok(()) }
fn level<'a>(book: &'a V, side: &str, quantity: i64) -> Option<&'a V> {
    book[side].as_array()?.iter().find(|l| l["quantity"].as_i64().map(|q| q / 100 >= quantity).unwrap_or(false))
}

/// fillMarkout(fill, entryRow, laterRows, {horizonsMs, maximumLateMs, throughAt})
pub fn fill_markout(fill: &V, entry_row: &V, later_rows: &[V], horizons_ms: Option<&[i64]>, maximum_late_ms: Option<i64>, through_at: &str) -> Result<V, String> {
    let horizons_ms: Vec<i64> = horizons_ms.map(|h| h.to_vec()).unwrap_or_else(|| vec![5000, 60000, 300000]);
    let maximum_late_ms = maximum_late_ms.unwrap_or(20000);
    need(fill["action"] == "buy"
        && js_int(&fill["quantity"]).is_some_and(|q| q > 0)
        && fill["fee"] == "0.000000", "FILL_SCOPE")?;
    let entered = instant(fill["at"].as_str().unwrap_or(""))?;
    let entry = &entry_row["payload"]["book"];
    need(entry_row["sequence"] == fill["sourceSequence"]
        && entry_row["hash"] == fill["sourceHash"]
        && entry["marketId"] == fill["marketId"]
        && entry["exchangeId"] == fill["exchangeId"]
        && entry["versionKey"] == fill["sourceVersion"], "EXACT_ENTRY_SOURCE")?;
    need(entry["requestStartedAt"] == fill["requestStartedAt"]
        && entry["receivedAt"] == fill["receivedAt"]
        && instant(entry_row["at"].as_str().unwrap_or(""))? <= entered
        && instant(entry["receivedAt"].as_str().unwrap_or(""))? <= entered, "ENTRY_CLOCK")?;
    let q = BigInt::from(js_int(&fill["quantity"]).unwrap());
    let paid = big_amount(fill["price"].as_str().unwrap_or(""))?;
    let ask = entry["asks"].as_array().and_then(|a| a.iter().find(|l|
        l["quantity"].as_i64().map(|x| x / 100 >= js_int(&fill["quantity"]).unwrap()).unwrap_or(false)
        && big_amount(l["price"].as_str().unwrap_or("")).map(|p| p + BigInt::from(TICK)).ok() == Some(paid.clone())));
    let bid = level(entry, "bids", js_int(&fill["quantity"]).unwrap());
    need(ask.is_some() && bid.is_some(), "ENTRY_DEPTH_OR_PRICE")?;
    let ask = ask.unwrap();
    let bid = bid.unwrap();
    let raw_ask = big_amount(ask["price"].as_str().unwrap_or(""))?;
    let entry_bid = big_amount(bid["price"].as_str().unwrap_or(""))?;
    let through = instant(through_at)?;
    let entry_cost_components = json!({
        "observedSpread": money(&((&entry_bid - &raw_ask) * &q)),
        "modeledEntryTick": money(&((&raw_ask - &paid) * &q)),
        "modeledExitTick": money(&(-BigInt::from(TICK) * &q)),
    });
    let mut horizons = Vec::with_capacity(horizons_ms.len());
    for &ms in &horizons_ms {
        need(ms > 0 && ms <= 9_007_199_254_740_991, "HORIZON")?;
        let target = entered + ms;
        let mut row: Option<&V> = None;
        for r in later_rows {
            if r["kind"] == "book"
                && r["payload"]["book"]["marketId"] == fill["marketId"]
                && r["payload"]["book"]["exchangeId"] == fill["exchangeId"]
            {
                if instant(r["payload"]["book"]["requestStartedAt"].as_str().unwrap_or(""))? >= target
                    && instant(r["at"].as_str().unwrap_or(""))? <= through {
                    row = Some(r);
                    break;
                }
            }
        }
        let row = match row {
            None => {
                horizons.push(json!({"horizonMs":ms,
                    "status":if through < target { "not_matured" } else { "missing_later_book" },
                    "realizedPnl":null}));
                continue;
            }
            Some(r) => r,
        };
        let b = &row["payload"]["book"];
        let received = instant(b["receivedAt"].as_str().unwrap_or(""))?;
        let requested = instant(b["requestStartedAt"].as_str().unwrap_or(""))?;
        let sale = level(b, "bids", js_int(&fill["quantity"]).unwrap());
        if received - target > maximum_late_ms || received - requested > 20000 {
            horizons.push(json!({"horizonMs":ms,"status":"capture_gap",
                "receivedAt":b["receivedAt"],"realizedPnl":null}));
            continue;
        }
        let sale_price = sale.and_then(|l| l["price"].as_str()).and_then(|p| big_amount(p).ok());
        if sale_price.is_none() || sale_price.as_ref() < Some(&BigInt::from(TICK)) {
            horizons.push(json!({"horizonMs":ms,"status":"insufficient_later_depth","realizedPnl":null}));
            continue;
        }
        let future_bid = sale_price.unwrap();
        let price_change = (&future_bid - &entry_bid) * &q;
        let net = (&future_bid - BigInt::from(TICK) - &paid) * &q;
        need(big_amount(entry_cost_components["observedSpread"].as_str().unwrap())?
            + big_amount(entry_cost_components["modeledEntryTick"].as_str().unwrap())?
            + big_amount(entry_cost_components["modeledExitTick"].as_str().unwrap())?
            + &price_change == net, "DECOMPOSITION")?;
        horizons.push(json!({"horizonMs":ms,"status":"observed",
            "requestStartedAt":b["requestStartedAt"],"receivedAt":b["receivedAt"],
            "publicationSequence":row["sequence"],"publicationHash":row["hash"],
            "rawBid":money(&future_bid),"subsequentBidChange":money(&price_change),
            "rawQuoteRoundtrip":money(&((&future_bid - &raw_ask) * &q)),
            "modeledLiquidationNet":money(&net),
            "entryCostComponents":entry_cost_components,"realizedPnl":null}));
    }
    Ok(json!({"marketId":fill["marketId"],"exchangeId":fill["exchangeId"],
        "filledAt":fill["at"],"quantity":fill["quantity"],"paidPrice":fill["price"],
        "rawEntryAsk":ask["price"],"rawEntryBid":bid["price"],
        "entryCostComponents":entry_cost_components,"horizons":horizons,
        "fees":"0.000000","simulatedSales":0,"profitabilityEstablished":false}))
}
