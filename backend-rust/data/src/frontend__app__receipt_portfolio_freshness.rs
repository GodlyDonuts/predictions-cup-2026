// frontend/app/receipt-portfolio-freshness.mjs — receipt-portfolio freshness view.
use serde_json::{json, Value as V};

fn parse_ms(s: &str) -> f64 {
    baal_service_runtime::contract::parse_iso_ms(s).unwrap_or(f64::NAN)
}
fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64).unwrap_or(0.0)
}

fn valid_clock(at: &V, now: f64, max_age: f64) -> bool {
    at.is_string() && {
        let t = parse_ms(at.as_str().unwrap());
        t.is_finite() && now >= t && now - t <= max_age
    }
}

pub fn receipt_observation_health(wallet: &V, now: Option<f64>) -> V {
    let now = now.unwrap_or_else(now_ms);
    if wallet["available"].as_bool() != Some(true) {
        return json!({"healthy": false, "label": "Paper observation unavailable", "status": "unavailable"});
    }
    if wallet["status"].as_str() == Some("failed") {
        return json!({"healthy": false, "label": "Paper batch failed", "status": "failed"});
    }
    if !now.is_finite() || !parse_ms(wallet["updatedAt"].as_str().unwrap_or("")).is_finite()
        || now < parse_ms(wallet["updatedAt"].as_str().unwrap_or("")) {
        return json!({"healthy": false, "label": "Paper observation clock invalid", "status": "invalid_clock"});
    }
    let max_age = wallet["receiptPortfolio"]["maxObservationAgeMs"].as_f64().unwrap_or(60000.0);
    if !valid_clock(&wallet["updatedAt"], now, max_age) {
        return json!({"healthy": false, "label": "Paper observation stale · one-shot batch", "status": "stale"});
    }
    json!({"healthy": true, "label": "Paper observation fresh · batch completed", "status": "completed_batch"})
}

pub fn age_receipt_portfolio_wallet(wallet: &V, now: Option<f64>) -> V {
    let now = now.unwrap_or_else(now_ms);
    if wallet["receiptPortfolio"].is_null() { return wallet.clone(); }
    let max_age = wallet["receiptPortfolio"]["maxQuoteAgeMs"].as_f64().unwrap_or(20000.0);
    let positions: Vec<V> = wallet["positions"].as_array().cloned().unwrap_or_default().iter().map(|p| {
        let known = valid_clock(&p["quoteReceivedAt"], now, max_age)
            && valid_clock(&p["quoteRequestStartedAt"], now, max_age)
            && valid_clock(&p["quoteAvailableAt"], now, max_age)
            && parse_ms(p["quoteRequestStartedAt"].as_str().unwrap_or("")) <= parse_ms(p["quoteReceivedAt"].as_str().unwrap_or(""))
            && parse_ms(p["quoteReceivedAt"].as_str().unwrap_or("")) <= parse_ms(p["quoteAvailableAt"].as_str().unwrap_or(""));
        let current = known && !p["snapshotLiquidationValue"].is_null() && wallet["status"].as_str() != Some("failed");
        let mut p = p.clone();
        let quote_age = if p["quoteReceivedAt"].is_string() && parse_ms(p["quoteReceivedAt"].as_str().unwrap()).is_finite() {
            Some(now - parse_ms(p["quoteReceivedAt"].as_str().unwrap()))
        } else { None };
        p["quoteAgeMs"] = quote_age.map_or(V::Null, |a| json!(a));
        p["quoteInvalidated"] = json!(!current);
        let liquidation = if current { p["snapshotLiquidationValue"].clone() } else { V::Null };
        p["liquidationValue"] = liquidation;
        let unsold = if current { json!(0) } else { p["quantity"].clone() };
        p["unsoldShares"] = unsold;
        p["liquidationStatus"] = json!(if current { "estimated_from_observed_depth" } else if known { "unavailable_depth" } else { "stale_or_unverified_quote" });
        p
    }).collect();
    let all_fresh = positions.iter().all(|p| p["quoteInvalidated"].as_bool() != Some(true));
    let mut metrics = wallet["metrics"].clone();
    metrics["liquidationPnl"] = if all_fresh && wallet["status"].as_str() != Some("failed") {
        wallet["receiptPortfolio"]["snapshotLiquidationPnl"].clone()
    } else { V::Null };
    let mut sensitivities = serde_json::Map::new();
    for (key, c) in wallet["receiptPortfolio"]["snapshotSensitivities"].as_object().cloned().unwrap_or_default() {
        let mut c = c.clone();
        let lp = if all_fresh && wallet["status"].as_str() != Some("failed") { c["estimatedLiquidationPnl"].clone() } else { V::Null };
        c["estimatedLiquidationPnl"] = lp;
        let ep = if all_fresh && wallet["status"].as_str() != Some("failed") { c["estimatedProceeds"].clone() } else { V::Null };
        c["estimatedProceeds"] = ep;
        c["realized"] = json!(false);
        sensitivities.insert(key, c);
    }
    let mut out = wallet.clone();
    out["positions"] = json!(positions);
    out["metrics"] = metrics;
    out["receiptPortfolio"]["liquidationSensitivities"] = V::Object(sensitivities);
    out["receiptPortfolio"]["quoteMarksFresh"] = json!(all_fresh);
    out
}
