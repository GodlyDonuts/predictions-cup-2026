// live-paper/stable-market-screen.mjs — stable-quote screening over price
// snapshots.
use serde_json::{json, Value as V};

/// STABLE_SCREEN_POLICY
pub fn stable_screen_policy() -> V {
    json!({"historyMs":300000,"activityMs":3600000,"minimumSpanMs":240000,
        "maximumGapMs":120000,"maximumAgeMs":90000,"maximumMidpointRangeTicks":2,
        "minimumSpreadTicks":1,"maximumSpreadTicks":10,"maximumCandidates":16})
}
/// createStableScreen()
pub fn create_stable_screen() -> V {
    json!({"lastAt": f64::NEG_INFINITY, "markets": {}, "invalidQuotes": 0})
}
/// observeStableScreen(state, snapshot, availableAt)
pub fn observe_stable_screen(state: &V, snapshot: &V, available_at: &str) -> Result<V, String> {
    let mut next = state.clone();
    let p = stable_screen_policy();
    let at = baal_service_runtime::contract::parse_iso_ms(available_at).unwrap_or(f64::NAN);
    let request = baal_service_runtime::contract::parse_iso_ms(
        snapshot["requestStartedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN);
    let received = baal_service_runtime::contract::parse_iso_ms(
        snapshot["receivedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN);
    if !at.is_finite() || at < state["lastAt"].as_f64().unwrap_or(f64::NEG_INFINITY)
        || !request.is_finite() || request > received || received > at {
        return Err("STABLE_SCREEN_CLOCK".into());
    }
    next["lastAt"] = json!(at);
    let mut markets = state["markets"].as_object().cloned().unwrap_or_default();
    let mut invalid = state["invalidQuotes"].as_i64().unwrap_or(0);
    for q in snapshot["quotes"].as_array().cloned().unwrap_or_default() {
        let eid = q["exchangeId"].as_str().unwrap_or("").to_string();
        let bid = match baal_execution::gates::normalize_official_fill_tick(&q["bestBid"]) {
            Ok(t) => t["tick"].as_i64().unwrap_or(0),
            Err(_) => { markets.remove(&eid); invalid += 1; continue; }
        };
        let ask = match baal_execution::gates::normalize_official_fill_tick(&q["bestAsk"]) {
            Ok(t) => t["tick"].as_i64().unwrap_or(0),
            Err(_) => { markets.remove(&eid); invalid += 1; continue; }
        };
        if bid < 10 || ask > 190 || ask - bid < 1 || ask - bid > 10 {
            markets.remove(&eid);
            continue;
        }
        let mut rows: Vec<V> = markets.get(&eid)
            .map(|m| m["rows"].as_array().cloned().unwrap_or_default()).unwrap_or_default();
        if let Some(last) = rows.last() {
            if received - last["received"].as_f64().unwrap_or(0.0)
                > p["maximumGapMs"].as_i64().unwrap_or(0) as f64 {
                rows.clear();
            }
        }
        if rows.is_empty()
            || request > rows.last().map(|r| r["request"].as_f64().unwrap_or(0.0)).unwrap_or(0.0) + 2000.0 {
            rows.push(json!({"at":at,"request":request,"received":received,"bid":bid,"ask":ask}));
        }
        let activity_ms = p["activityMs"].as_i64().unwrap_or(0) as f64;
        rows.retain(|r| at - r["at"].as_f64().unwrap_or(0.0) <= activity_ms);
        markets.insert(eid, json!({"marketId": q["marketId"], "exchangeId": q["exchangeId"],
            "rows": rows}));
    }
    for id in snapshot["missingIds"].as_array().cloned().unwrap_or_default() {
        if let Some(eid) = id.as_str() { markets.remove(eid); }
    }
    next["markets"] = V::Object(markets);
    next["invalidQuotes"] = json!(invalid);
    Ok(next)
}
/// stableScreenCandidates(state, now, {excludedMarketIds, excludedExchangeIds})
pub fn stable_screen_candidates(state: &V, now: f64, excluded_market_ids: &[String],
    excluded_exchange_ids: &[String]) -> V {
    let p = stable_screen_policy();
    let mut out: Vec<V> = Vec::new();
    for m in state["markets"].as_object().cloned().unwrap_or_default().values() {
        let mid = m["marketId"].as_str().unwrap_or("");
        let eid = m["exchangeId"].as_str().unwrap_or("");
        if excluded_market_ids.iter().any(|x| x == mid)
            || excluded_exchange_ids.iter().any(|x| x == eid) {
            continue;
        }
        let rows = m["rows"].as_array().cloned().unwrap_or_default();
        let Some(last) = rows.last() else { continue };
        let window: Vec<&V> = rows.iter()
            .filter(|r| now - r["at"].as_f64().unwrap_or(0.0) <= p["historyMs"].as_f64().unwrap_or(0.0))
            .collect();
        let minute_buckets: std::collections::BTreeSet<i64> = window.iter()
            .map(|r| (r["request"].as_f64().unwrap_or(0.0) / 60000.0).floor() as i64).collect();
        if now - last["received"].as_f64().unwrap_or(0.0) > p["maximumAgeMs"].as_f64().unwrap_or(0.0)
            || now < last["at"].as_f64().unwrap_or(0.0)
            || window.len() < 4
            || last["at"].as_f64().unwrap_or(0.0) - window[0]["at"].as_f64().unwrap_or(0.0)
                < p["minimumSpanMs"].as_f64().unwrap_or(0.0)
            || minute_buckets.len() < 4 {
            continue;
        }
        let mids: Vec<i64> = window.iter()
            .map(|r| r["bid"].as_i64().unwrap_or(0) + r["ask"].as_i64().unwrap_or(0)).collect();
        if mids.iter().max().unwrap_or(&0) - mids.iter().min().unwrap_or(&0)
            > p["maximumMidpointRangeTicks"].as_i64().unwrap_or(0) {
            continue;
        }
        let activity = rows.iter().enumerate().skip(1)
            .filter(|(i, r)| r["bid"] != rows[*i - 1]["bid"] || r["ask"] != rows[*i - 1]["ask"])
            .count();
        out.push(json!({"marketId": m["marketId"], "exchangeId": m["exchangeId"],
            "priceChangesLastHour": activity,
            "latestReceivedAt": baal_service_runtime::contract::ms_to_iso(
                last["received"].as_f64().unwrap_or(0.0)),
            "historySpanMs": last["at"].as_f64().unwrap_or(0.0)
                - window[0]["at"].as_f64().unwrap_or(0.0),
            "spreadTicks": last["ask"].as_i64().unwrap_or(0) - last["bid"].as_i64().unwrap_or(0),
            "indicativeBidTicks": last["bid"], "indicativeAskTicks": last["ask"],
            "executionEligible": false}));
    }
    out.sort_by(|a, b| {
        b["priceChangesLastHour"].as_i64().unwrap_or(0).cmp(&a["priceChangesLastHour"].as_i64().unwrap_or(0))
            .then(b["spreadTicks"].as_i64().unwrap_or(0).cmp(&a["spreadTicks"].as_i64().unwrap_or(0)))
            .then(a["marketId"].as_str().unwrap_or("0").parse::<i64>().unwrap_or(0)
                .cmp(&b["marketId"].as_str().unwrap_or("0").parse::<i64>().unwrap_or(0)))
    });
    out.truncate(p["maximumCandidates"].as_i64().unwrap_or(0) as usize);
    json!(out)
}
