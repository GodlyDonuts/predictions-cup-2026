// live-paper/worker-health.mjs
use baal_service_runtime::contract::parse_iso_ms;
use serde_json::{json, Value as V};
/// workerHealth(report, now, maxHeartbeatAgeMs)
pub fn worker_health(report: &V, now: f64, max_heartbeat_age_ms: f64) -> V {
    let ts = report.get("updatedAt").or_else(|| report.get("reportedAt")).and_then(V::as_str);
    let at = ts.and_then(parse_iso_ms);
    let age = at.map(|a| now - a);
    match age {
        Some(a) if !(a.is_finite() && a >= 0.0) => json!({"status": "invalid_heartbeat", "heartbeatAgeMs": V::Null}),
        None => json!({"status": "invalid_heartbeat", "heartbeatAgeMs": V::Null}),
        Some(a) if a > max_heartbeat_age_ms => json!({"status": "stale_worker", "heartbeatAgeMs": a}),
        Some(a) => json!({"status": report.get("status").and_then(V::as_str).unwrap_or("unknown"), "heartbeatAgeMs": a}),
    }
}
fn assess_market_clocks(market_clocks: &[V], required: &[String], held: &[String], now: f64, max_age_ms: f64) -> V {
    let books: Vec<V> = market_clocks.iter().map(|b| {
        let age = b.get("receivedAt").and_then(V::as_str).and_then(parse_iso_ms).map(|a| now - a);
        let age_ms = age.filter(|a| a.is_finite() && *a >= 0.0);
        let status = if age_ms.is_none() { "invalid_clock" }
            else if b.get("invalidated").and_then(V::as_bool).unwrap_or(false) { "invalidated" }
            else if age_ms.unwrap() > max_age_ms { "stale" } else { "fresh" };
        let mut o = b.clone();
        o.as_object_mut().unwrap().insert("ageMs".into(), age_ms.map(|a| json!(a)).unwrap_or(V::Null));
        o.as_object_mut().unwrap().insert("status".into(), json!(status));
        o
    }).collect();
    let prio = |s: &str| match s { "invalid_clock" => 5, "invalidated" => 4, "stale" => 3, "missing" => 2, _ => 1 };
    let markets: Vec<V> = required.iter().map(|mid| {
        let mut matches: Vec<&V> = books.iter().filter(|b| b.get("marketId").and_then(V::as_str) == Some(mid.as_str())).collect();
        matches.sort_by_key(|b| -prio(b.get("status").and_then(V::as_str).unwrap_or("")));
        json!({"marketId": mid, "status": matches.first().map(|b| b.get("status").cloned().unwrap_or(json!("missing"))).unwrap_or(json!("missing"))})
    }).collect();
    let held_out: Vec<V> = held.iter().map(|inst| {
        let st = books.iter().find(|b| b.get("instrument").and_then(V::as_str) == Some(inst.as_str()))
            .and_then(|b| b.get("status").and_then(V::as_str)).unwrap_or("missing");
        json!({"instrument": inst, "status": st})
    }).collect();
    let ages: Vec<f64> = books.iter().filter_map(|b| b.get("ageMs").and_then(V::as_f64)).collect();
    let count = |s: &str| books.iter().filter(|b| b.get("status").and_then(V::as_str) == Some(s)).count();
    let mcount = |st: &str| markets.iter().filter(|m| m.get("status").and_then(V::as_str) == Some(st)).count();
    let hcount = |st: &str| held_out.iter().filter(|h| h.get("status").and_then(V::as_str) == Some(st)).count();
    let ever: std::collections::BTreeSet<&str> = books.iter().filter_map(|b| b.get("marketId").and_then(V::as_str)).collect();
    json!({
        "marketClocks": books, "requiredMarketIds": required, "heldInstrumentIds": held,
        "asOf": baal_service_runtime::contract::ms_to_iso(now), "maxDataAgeMs": max_age_ms,
        "markets": markets, "held": held_out,
        "invalidated": count("invalidated"), "invalidClocks": count("invalid_clock"),
        "coveredMarkets": mcount("fresh"), "everObservedMarkets": ever.len(),
        "expectedMarkets": required.len(), "staleMarkets": mcount("stale"),
        "missingMarkets": mcount("missing"), "staleHeldInstruments": hcount("stale"),
        "missingHeldInstruments": hcount("missing"),
        "oldestBookAgeMs": if ages.is_empty() { V::Null } else { json!(ages.iter().cloned().fold(f64::NEG_INFINITY, f64::max)) },
    })
}
/// marketDataHealth(wallet, config, at)
pub fn market_data_health(wallet: &V, config: &V, at: &V) -> V {
    let now = at.as_f64().or_else(|| at.as_str().and_then(parse_iso_ms)).unwrap_or(f64::NAN);
    let mut held: Vec<String> = Vec::new();
    if let Some(strategies) = wallet.get("strategies").and_then(V::as_object) {
        for s in strategies.values() {
            for inst in s.get("positions").and_then(V::as_object).map(|p| p.keys()).into_iter().flatten() {
                if !held.contains(inst) { held.push(inst.clone()); }
            }
        }
    }
    let books = wallet.get("books").and_then(V::as_object);
    let mut required: Vec<String> = Vec::new();
    for id in config.get("marketIds").and_then(V::as_array).into_iter().flatten().filter_map(V::as_str) {
        if !required.iter().any(|r| r == id) { required.push(id.to_string()); }
    }
    for inst in &held {
        if let Some(mid) = books.and_then(|b| b.get(inst)).and_then(|b| b.get("marketId")).and_then(V::as_str) {
            if !required.iter().any(|r| r == mid) { required.push(mid.to_string()); }
        }
    }
    let market_clocks: Vec<V> = books.map(|b| b.iter().filter(|(id, b)| {
            required.iter().any(|r| Some(r.as_str()) == b.get("marketId").and_then(V::as_str)) || held.contains(id)
        }).map(|(instrument, b)| json!({
            "instrument": instrument, "marketId": b.get("marketId"), "receivedAt": b.get("receivedAt"),
            "invalidated": wallet.get("invalidated").and_then(V::as_object).and_then(|i| i.get(instrument)).and_then(V::as_bool).unwrap_or(false),
        })).collect()).unwrap_or_default();
    assess_market_clocks(&market_clocks, &required, &held, now, config.get("maxDataAgeMs").and_then(V::as_f64).unwrap_or(f64::NAN))
}
/// baselineView(baseline, legacy, runtime, now)
pub fn baseline_view(baseline: &V, legacy: &V, runtime: &V, now: f64) -> Result<V, String> {
    if baseline.get("config").and_then(|c| c.get("feeBps")).and_then(V::as_f64) != Some(0.0)
        || baseline.get("mode").and_then(V::as_str) != Some("prospective_fee_free_paper_baseline") {
        return Err("Invalid fee-free baseline".into());
    }
    let health = worker_health(baseline, now, 60000.0);
    let book_age = baseline.get("lastObservationAt").and_then(V::as_str).and_then(parse_iso_ms).map(|a| now - a);
    let verified = baseline.get("dataHealth").and_then(|d| d.get("marketClocks")).is_some_and(V::is_array);
    let data_health = if verified {
        let dh = &baseline["dataHealth"];
        let clocks: Vec<V> = dh.get("marketClocks").and_then(V::as_array).cloned().unwrap_or_default();
        let req: Vec<String> = dh.get("requiredMarketIds").and_then(V::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        let held: Vec<String> = dh.get("heldInstrumentIds").and_then(V::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        assess_market_clocks(&clocks, &req, &held, now, baseline["config"]["maxDataAgeMs"].as_f64().unwrap_or(f64::NAN))
    } else { baseline.get("dataHealth").cloned().unwrap_or(V::Null) };
    let observations = baseline.get("observations").and_then(V::as_i64).unwrap_or(0);
    let dh = |k: &str| data_health.get(k).and_then(V::as_i64).unwrap_or(0);
    let data_status = if observations == 0 { "awaiting_first_accepted_book" }
        else if book_age.map(|a| !a.is_finite() || a < 0.0).unwrap_or(true) || dh("invalidClocks") > 0 { "invalid_market_clock" }
        else if dh("invalidated") > 0 { "invalidated_market_data" }
        else if book_age.unwrap_or(0.0) > baseline["config"]["maxDataAgeMs"].as_f64().unwrap_or(f64::NAN)
            || dh("staleMarkets") > 0 || dh("staleHeldInstruments") > 0 { "stale_market_data" }
        else if dh("missingHeldInstruments") > 0
            || (verified && dh("coveredMarkets") < dh("expectedMarkets")) { "partial_market_coverage" }
        else if verified { "fresh_market_data" } else { "unverified_market_coverage" };
    let mut out = baseline.clone();
    let o = out.as_object_mut().unwrap();
    o.insert("dataHealth".into(), data_health.clone());
    o.insert("baselineWorkerStatus".into(), health["status"].clone());
    o.insert("baselineHeartbeatAgeMs".into(), health["heartbeatAgeMs"].clone());
    o.insert("baselineBookAgeMs".into(), if observations > 0 {
        if verified { data_health.get("oldestBookAgeMs").cloned().unwrap_or(V::Null) }
        else { book_age.filter(|a| a.is_finite() && *a >= 0.0).map(|a| json!(a)).unwrap_or(V::Null) }
    } else { V::Null });
    o.insert("baselineDataStatus".into(), json!(data_status));
    o.insert("legacyFeeStress".into(), json!({"feeBps": legacy["config"]["feeBps"], "runId": legacy["runId"], "strategies": legacy["strategies"]}));
    o.insert("runtime".into(), runtime.clone());
    Ok(out)
}
