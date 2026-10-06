// frontend/app/wallet-view.mjs — wallet projection used by the bulk-dip view.
use serde_json::{json, Value as V};

fn parse_ms(s: &str) -> f64 {
    baal_service_runtime::contract::parse_iso_ms(s).unwrap_or(f64::NAN)
}
fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64).unwrap_or(0.0)
}

pub fn paper_cohorts() -> Vec<(&'static str, &'static str)> {
    vec![("dip-responsive", "Responsive dip"), ("shock", "Shock rebound"), ("dip-scale", "Scaled dip"), ("dip", "Conservative dip")]
}

pub fn paper_wallet_view(d: &V, id: &str, name: &str) -> Result<V, String> {
    if d["strategy"].is_null() { return Err("Unavailable".into()); }
    let s = &d["strategy"];
    let map_pos = |x: &V| json!({
        "instrument": x["instrument"], "label": x["label"], "quantity": x["quantity"], "cost": x["cost"],
        "enteredAt": x["enteredAt"],
        "observationAgeMs": if x["observationAgeMs"].is_null() { V::Null } else { x["observationAgeMs"].clone() },
        "quoteReceivedAt": if x["quoteReceivedAt"].is_null() { V::Null } else { x["quoteReceivedAt"].clone() },
        "quoteAgeMs": if !x["quoteAgeMs"].is_null() { x["quoteAgeMs"].clone() }
            else if !x["observationAgeMs"].is_null() { x["observationAgeMs"].clone() } else { V::Null },
        "quoteInvalidated": x["quoteInvalidated"].as_bool() == Some(true),
        "unsoldShares": if x["unsoldShares"].is_null() { V::Null } else { x["unsoldShares"].clone() },
        "estimatedProceeds": if x["estimatedProceeds"].is_null() { V::Null } else { x["estimatedProceeds"].clone() },
        "liquidationValue": if x["quoteInvalidated"].as_bool() == Some(true)
            || x["observationAgeMs"].is_null() || x["observationAgeMs"].as_f64().unwrap_or(0.0) < 0.0
            || x["unsoldShares"].as_f64().unwrap_or(1.0) != 0.0 { V::Null } else { x["estimatedProceeds"].clone() },
    });
    let positions: Vec<V> = s["positions"].as_array().cloned().unwrap_or_default().iter().map(|x| map_pos(x)).collect();
    let trades: Vec<V> = s["recentFills"].as_array().cloned().unwrap_or_default().iter().map(|x| json!({
        "instrument": x["instrument"], "label": x["label"], "action": x["action"],
        "quantity": x["quantity"], "price": x["price"], "fee": x["fee"], "at": x["at"]})).collect();
    let pending: Vec<V> = s["pending"].as_array().cloned().unwrap_or_default().iter().map(|x| json!({
        "instrument": x["instrument"], "label": x["label"], "action": x["action"],
        "quantity": x["quantity"], "limit": x["limit"], "createdAt": x["createdAt"]})).collect();
    Ok(json!({
        "id": id, "name": name, "available": true, "updatedAt": d["updatedAt"],
        "status": if !d["workerHealth"]["status"].is_null() { d["workerHealth"]["status"].clone() } else { d["status"].clone() },
        "collectorStatus": d["collectorStatus"], "observations": d["observations"],
        "markets": d["universeSize"], "histories": d["historiesAvailable"], "feeBps": d["feeBps"],
        "metrics": {"cash": s["cash"], "realizedPnl": s["realizedPnl"], "referencePnl": s["referencePnl"],
            "referenceEquity": s["referenceEquity"], "liquidationPnl": s["liquidationPnl"],
            "equity": s["estimatedLiquidationEquity"], "fills": s["fills"], "fees": s["fees"],
            "drawdown": s["maxObservedDrawdown"]},
        "positions": positions, "trades": trades, "pending": pending,
        "performance": if d["performance"].is_null() { V::Null }
            else { json!({"closedRoundTrips": d["performance"]["closedRoundTrips"]}) },
    }))
}

pub fn wallet_connection(wallet: &V, now: Option<f64>) -> V {
    let now = now.unwrap_or_else(now_ms);
    if wallet["available"].as_bool() != Some(true) {
        return json!({"healthy": false,
            "label": if wallet["official"].as_bool() == Some(true) { "Account unavailable" } else { "Worker unavailable" }});
    }
    if !wallet["receiptPortfolio"].is_null() {
        return crate::frontend__app__receipt_portfolio_freshness::receipt_observation_health(wallet, Some(now));
    }
    if wallet["official"].as_bool() == Some(true) {
        let keys = ["positions", "orders", "fills", "pnl"];
        let ages: Vec<Option<f64>> = keys.iter().map(|k| {
            let t = &wallet["sourceTimes"][k];
            if t.is_null() { None } else { Some(now - parse_ms(t.as_str().unwrap_or(""))) }
        }).collect();
        let status = if ages.iter().any(|a| a.is_some_and(|x| !x.is_finite() || x < 0.0)) {
            "invalid_clock"
        } else if ages.iter().any(|a| a.is_none()) {
            "partial"
        } else if !wallet["errors"].as_object().cloned().unwrap_or_default().is_empty()
            || wallet["collectorStatus"].as_str() != Some("running") {
            "degraded"
        } else if ages.iter().any(|a| a.is_some_and(|x| x > 300000.0)) {
            "stale"
        } else {
            wallet["health"]["status"].as_str().unwrap_or("unverified")
        };
        let label = match status {
            "fresh" => "Account fresh", "partial" => "Account partially synced",
            "degraded" => "Account degraded", "invalid_clock" => "Account clock invalid",
            "stale" => "Account snapshot stale", "unavailable" => "Account unavailable",
            _ => "Account freshness unverified",
        };
        return json!({"healthy": status == "fresh", "label": label, "status": status});
    }
    let age = now - parse_ms(wallet["updatedAt"].as_str().unwrap_or(""));
    if !age.is_finite() || age < 0.0 {
        json!({"healthy": false, "label": "Worker clock invalid"})
    } else if age > 60000.0 {
        json!({"healthy": false, "label": "Worker stale"})
    } else {
        json!({"healthy": wallet["status"].as_str() == Some("running"),
            "label": format!("Worker {}", wallet["status"].as_str().unwrap_or(""))})
    }
}
