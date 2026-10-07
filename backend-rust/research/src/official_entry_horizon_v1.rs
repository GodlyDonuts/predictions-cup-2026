// research/official-entry-horizon-v1.mjs — a future dated depth value for an
// actual purchase; never a simulated fill.
use baal_execution::no_liquidation_depth_value_v1::value_no_liquidation_depth_v1;
use baal_service_runtime::contract::parse_iso_ms;
use serde_json::{json, Value as V};

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}

pub fn entry_horizon_observation_v1(input: &V) -> Result<V, String> {
    let entry = get(input, "entry");
    let book = get(input, "book");
    let horizon_minutes = get(input, "horizonMinutes").as_f64().unwrap_or(f64::NAN);
    let maximum_delay_ms = get(input, "maximumDelayMs")
        .as_f64()
        .unwrap_or(300000.0);
    // JS Date.parse: invalid input -> NaN, and NaN comparisons are all false,
    // so an unparseable timestamp does NOT trigger the early null return.
    let fill_at = get(entry, "fillAt").as_str().unwrap_or("");
    let target = parse_iso_ms(fill_at).unwrap_or(f64::NAN) + horizon_minutes * 60000.0;
    let start = parse_iso_ms(get(book, "requestStartedAt").as_str().unwrap_or(""))
        .unwrap_or(f64::NAN);
    let end = parse_iso_ms(get(book, "receivedAt").as_str().unwrap_or(""))
        .unwrap_or(f64::NAN);
    if get(book, "exchangeId") != get(entry, "exchangeId")
        || get(book, "marketId") != get(entry, "marketId")
        || start < target
        || end < start
        || start - target > maximum_delay_ms
    {
        return Ok(V::Null);
    }
    Ok(json!({
        "horizonMinutes": get(input, "horizonMinutes").clone(),
        "requestStartedAt": get(book, "requestStartedAt").clone(),
        "receivedAt": get(book, "receivedAt").clone(),
        "delayAfterHorizonMs": start - target,
        "exactObservedDepth": value_no_liquidation_depth_v1(&json!({
            "quantity": get(entry, "quantity").clone(),
            "remainingCost": get(entry, "cost").clone(),
            "yesAsks": get(book, "asks").clone(),
            "slippageTicks": 0,
        }))?,
        "oneTickStress": value_no_liquidation_depth_v1(&json!({
            "quantity": get(entry, "quantity").clone(),
            "remainingCost": get(entry, "cost").clone(),
            "yesAsks": get(book, "asks").clone(),
            "slippageTicks": 1,
        }))?,
        "actualSale": false,
    }))
}
