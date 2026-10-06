// live-paper/rebound-entry.mjs — rebound confirmation watch.
use num_bigint::BigInt;
use serde_json::{json, Value as V};

fn need(ok: bool) -> Result<(), String> {
    if !ok { return Err("REBOUND_ENTRY_INVALID".into()); }
    Ok(())
}
/// REBOUND_ENTRY_POLICY
pub fn rebound_entry_policy() -> V {
    json!({"minimumInitialDiscount":"0.010000","minimumRemainingDiscount":"0.005000",
        "confirmationTicks":1,"confirmationLifetimeMs":120000,"maximumQuoteGapMs":90000,
        "maximumReceiptLagMs":20000,"entryLifetimeMs":60000})
}
fn price(value: f64) -> Result<String, String> {
    Ok(baal_paper::fixed::money(&BigInt::from((value * 1e6).round() as i64)))
}
/// createReboundWatch(dip)
pub fn create_rebound_watch(dip: &V) -> Result<V, String> {
    need(dip["marketId"].is_string() && dip["exchangeId"].is_string())?;
    let anchor = baal_paper::fixed::big_amount(dip["reference"]["anchor"].as_str().unwrap_or(""))?;
    let bid = baal_paper::fixed::big_amount(dip["bid"].as_str().unwrap_or(""))?;
    let ask = baal_paper::fixed::big_amount(dip["ask"].as_str().unwrap_or(""))?;
    need(bid > BigInt::from(0) && ask > bid
        && &anchor - &ask >= baal_paper::fixed::big_amount(
            rebound_entry_policy()["minimumInitialDiscount"].as_str().unwrap_or(""))?)?;
    baal_paper::fixed::instant(dip["availableAt"].as_str().unwrap_or(""))?;
    baal_paper::fixed::instant(dip["receivedAt"].as_str().unwrap_or(""))?;
    Ok(json!({"marketId":dip["marketId"],"exchangeId":dip["exchangeId"],
        "anchor":baal_paper::fixed::money(&anchor),"status":"watching",
        "troughBid":baal_paper::fixed::money(&bid),"troughAvailableAt":dip["availableAt"],
        "lastAvailableAt":dip["availableAt"],"lastReceivedAt":dip["receivedAt"],
        "expiresAt":baal_service_runtime::contract::ms_to_iso(
            (baal_paper::fixed::instant(dip["availableAt"].as_str().unwrap_or(""))?
                + rebound_entry_policy()["confirmationLifetimeMs"].as_i64().unwrap_or(0)) as f64),
        "signal":V::Null}))
}
/// advanceReboundWatch(w, q, snapshot, availableAt) — mutates w in place.
pub fn advance_rebound_watch(w: &mut V, q: &V, snapshot: &V, available_at: &str) -> Result<(), String> {
    if w["status"].as_str() != Some("watching") { return Ok(()); }
    need(q["marketId"] == w["marketId"] && q["exchangeId"] == w["exchangeId"]
        && q["option"].as_str() == Some("YES"))?;
    let at = baal_paper::fixed::instant(available_at)?;
    let received = baal_paper::fixed::instant(snapshot["receivedAt"].as_str().unwrap_or(""))?;
    let request = baal_paper::fixed::instant(snapshot["requestStartedAt"].as_str().unwrap_or(""))?;
    need(request <= received && received <= at
        && at >= baal_paper::fixed::instant(w["lastAvailableAt"].as_str().unwrap_or(""))?)?;
    if at > baal_paper::fixed::instant(w["expiresAt"].as_str().unwrap_or(""))? {
        w["status"] = json!("confirmation_not_observed");
        return Ok(());
    }
    if received - baal_paper::fixed::instant(w["lastReceivedAt"].as_str().unwrap_or(""))?
        > rebound_entry_policy()["maximumQuoteGapMs"].as_i64().unwrap_or(0)
        || at - received > rebound_entry_policy()["maximumReceiptLagMs"].as_i64().unwrap_or(0) {
        w["status"] = json!("coverage_gap");
        return Ok(());
    }
    if baal_service_runtime::json_text::js_truthy(Some(&q["crossed"]))
        || !q["bestBid"].is_number() || !q["bestAsk"].is_number()
        || q["bestBid"].as_f64().unwrap_or(0.0) <= 0.0
        || q["bestAsk"].as_f64().unwrap_or(1.0) >= 1.0
        || q["bestAsk"].as_f64().unwrap_or(0.0) <= q["bestBid"].as_f64().unwrap_or(1.0) {
        w["status"] = json!("invalid_quote");
        return Ok(());
    }
    if request <= baal_paper::fixed::instant(w["troughAvailableAt"].as_str().unwrap_or(""))? {
        return Ok(());
    }
    w["lastAvailableAt"] = json!(available_at);
    w["lastReceivedAt"] = snapshot["receivedAt"].clone();
    let bid = baal_paper::fixed::big_amount(&price(q["bestBid"].as_f64().unwrap_or(0.0))?)?;
    let ask = baal_paper::fixed::big_amount(&price(q["bestAsk"].as_f64().unwrap_or(0.0))?)?;
    let trough = baal_paper::fixed::big_amount(w["troughBid"].as_str().unwrap_or(""))?;
    if bid < trough {
        w["troughBid"] = json!(baal_paper::fixed::money(&bid));
        w["troughAvailableAt"] = json!(available_at);
        return Ok(());
    }
    if bid < trough + BigInt::from(rebound_entry_policy()["confirmationTicks"].as_i64().unwrap_or(0))
            * BigInt::from(baal_paper::fixed::TICK)
        || baal_paper::fixed::big_amount(w["anchor"].as_str().unwrap_or(""))? - &ask
            < baal_paper::fixed::big_amount(
                rebound_entry_policy()["minimumRemainingDiscount"].as_str().unwrap_or(""))? {
        return Ok(());
    }
    w["status"] = json!("confirmed");
    w["signal"] = json!({"marketId":w["marketId"],"exchangeId":w["exchangeId"],
        "availableAt":available_at,"receivedAt":snapshot["receivedAt"],
        "requestStartedAt":snapshot["requestStartedAt"],
        "troughAvailableAt":w["troughAvailableAt"],"troughBid":w["troughBid"],
        "anchor":w["anchor"],"limit":baal_paper::fixed::money(&ask),
        "remainingDiscount":baal_paper::fixed::money(
            &(baal_paper::fixed::big_amount(w["anchor"].as_str().unwrap_or(""))? - &ask)),
        "depthVerified":false});
    Ok(())
}
