// frontend/app/bulk-dip-view.mjs — bulk-dip paper wallet projection.
use crate::frontend__app__comparison_view::{age_comparison_wallet, comparison_name_valid};
use crate::frontend__app__wallet_view::paper_wallet_view;
use baal_service_runtime::json_text::{js_stringify, sha256};
use serde_json::{json, Value as V};

fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64).unwrap_or(0.0)
}
fn count(v: &V) -> i64 {
    if v.as_f64().is_some_and(|x| x.fract() == 0.0 && x >= 0.0 && x <= 9007199254740991.0) {
        v.as_f64().unwrap() as i64
    } else { 0 }
}

pub fn bulk_dip_wallet_views(report: &V, manifest: &V, bank: &V, worker_alive: bool, now: Option<f64>) -> Result<Vec<V>, String> {
    let now = now.unwrap_or_else(now_ms);
    let hash = sha256(js_stringify(bank).as_bytes());
    if !comparison_name_valid(&manifest["cohort"])
        || !manifest["bankHash"].as_str().is_some_and(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))
        || hash != manifest["bankHash"].as_str().unwrap_or("")
        || bank["schemaVersion"].as_i64() != Some(2) || bank["hostVersion"].as_i64() != Some(2)
        || bank["name"] != manifest["cohort"]
        || bank["mode"].as_str() != Some("market_wide_bulk_reference_paper")
        || bank["config"]["feeBps"].as_i64() != Some(0)
        || report["cohort"] != manifest["cohort"] || report["bankHash"].as_str().unwrap_or("") != hash
        || report["cutoff"] != bank["cutoff"]
        || report["hostVersion"].as_i64() != Some(2) || report["mode"] != bank["mode"]
        || report["networkRequests"].as_i64() != Some(0) || report["competitionOrders"].as_i64() != Some(0)
        || !report["failed"].is_boolean() || !report["sourceClosed"].is_boolean()
        || !baal_service_runtime::contract::parse_iso_ms(bank["cutoff"].as_str().unwrap_or("")).is_some_and(|x| x.is_finite())
        || !bank["config"]["maxDataAgeMs"].as_f64().is_some_and(|x| x.is_finite() && x > 0.0) {
        return Err("Bulk paper identity unavailable".into());
    }
    let universe = match bank["universe"]["exchanges"].as_array() {
        Some(u) if !u.is_empty() => u.clone(),
        _ => return Err("Bulk paper scope unavailable".into()),
    };
    if universe.iter().any(|x| {
        !x["marketId"].as_str().is_some_and(|t| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit()))
            || !x["exchangeId"].as_str().is_some_and(|t| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit()))
            || x["option"].as_str() != Some("YES")
    }) {
        return Err("Bulk paper scope unavailable".into());
    }
    let scope: std::collections::HashMap<String, V> = universe.iter()
        .map(|x| (format!("{}:YES", x["exchangeId"].as_str().unwrap_or("")), x.clone())).collect();
    if scope.len() != universe.len()
        || universe.iter().map(|x| x["exchangeId"].as_str().unwrap_or("")).collect::<std::collections::HashSet<_>>().len() != universe.len() {
        return Err("Bulk paper scope ambiguous".into());
    }
    let latencies = bank["policy"]["latenciesMs"].as_array().cloned().unwrap_or_default();
    if latencies.len() != 2 || latencies[0].as_i64() != Some(1000) || latencies[1].as_i64() != Some(3000) {
        return Err("Bulk paper wallets unavailable".into());
    }
    let label = |x: &V| -> Result<V, String> {
        let Some(instrument) = scope.get(x["instrument"].as_str().unwrap_or("")) else {
            return Err("Bulk paper instrument outside scope".into());
        };
        let mut v = x.clone();
        v["label"] = json!(format!("Market {} · YES", instrument["marketId"].as_str().unwrap_or("")));
        v["side"] = json!("yes");
        Ok(v)
    };
    let status = if report["failed"].as_bool() == Some(true) { "failed" }
        else if !worker_alive { "stopped" }
        else if report["sourceClosed"].as_bool() == Some(true) { "awaiting_source" }
        else { "running" };
    latencies.iter().map(|latency| {
        let latency_ms = latency.as_f64().unwrap_or(0.0);
        // JS `report.wallets?.[String(latency)]` — integer latencies stringify as "1000".
        let wallet = &report["wallets"][latency.as_f64().unwrap_or(0.0).to_string()];
        if wallet.is_null() || wallet["fees"].as_str() != Some("0.000000")
            || !wallet["positions"].is_array() || !wallet["pending"].is_array() || !wallet["recentFills"].is_array() {
            return Err("Bulk paper accounting unavailable".into());
        }
        let mut strategy = wallet.clone();
        let positions: Result<Vec<V>, String> = wallet["positions"].as_array().unwrap().iter().map(|x| label(x)).collect();
        let pending: Result<Vec<V>, String> = wallet["pending"].as_array().unwrap().iter().map(|x| label(x)).collect();
        let recent: Result<Vec<V>, String> = wallet["recentFills"].as_array().unwrap().iter().map(|x| label(x)).collect();
        strategy["positions"] = json!(positions?);
        strategy["pending"] = json!(pending?);
        strategy["recentFills"] = json!(recent?);
        let mut v = paper_wallet_view(&json!({
            "updatedAt": report["updatedAt"], "status": status, "feeBps": 0,
            "observations": report["bookSequence"], "universeSize": scope.len(), "strategy": strategy,
            "performance": {"closedRoundTrips": wallet["closedTrades"]}}),
            &format!("bulk-dip-{}", latency_ms as i64),
            &format!("Market-wide dip · {}s", latency_ms / 1000.0))?;
        v["comparison"] = json!({"kind": "bulk_dip", "cohort": manifest["cohort"], "cutoff": bank["cutoff"],
            "maxMarkAgeMs": bank["config"]["maxDataAgeMs"]});
        v["bulkDip"] = json!({"latencyMs": latency, "workerState": status,
            "trackedMarkets": count(&report["bulkReference"]["marketCount"]),
            "configuredMarkets": scope.len(),
            "candidateCount": count(&json!(report["bulkReference"]["candidates"].as_array().map(|a| a.len()))),
            "gates": report["counts"].clone(), "referenceCounts": report["bulkReference"]["counts"].clone(),
            "priceSequence": count(&report["priceSequence"]), "bookSequence": count(&report["bookSequence"]),
            "bankHash": hash});
        Ok(age_comparison_wallet(&v, Some(now)))
    }).collect()
}
