// live-paper/market-history.mjs — minute-bucket bid history + discount ranking.
use baal_service_runtime::json_text::js_truthy;
use serde_json::{json, Value as V};

/// MARKET_HISTORY_POLICY
pub fn market_history_policy() -> V {
    json!({"windowMs": 21600000, "minMinutes": 30, "minSpanMs": 1800000, "maxAgeMs": 120000,
        "maxSpread": 0.02, "minNetRecovery": 0.02, "lowerQuantile": 0.25})
}
fn parse_at(v: &V) -> f64 { baal_service_runtime::contract::parse_iso_ms(v.as_str().unwrap_or("")).unwrap_or(f64::NAN) }
/// createMarketHistory(universe)
pub fn create_market_history(universe: &V) -> V {
    json!({"universeHash": universe["hash"], "lastAt": V::Null, "observations": 0, "minutes": {}})
}
/// observeMarketHistory(state, snapshot, universe, policy)
pub fn observe_market_history(state: &V, snapshot: &V, universe: &V, policy: &V) -> Result<V, String> {
    crate::market_price_publication::validate_price_observation_pub(snapshot, universe)?;
    let now = parse_at(&snapshot["receivedAt"]);
    let mut s = state.clone();
    if s["universeHash"] != universe["hash"]
        || (!s["lastAt"].is_null() && now < parse_at(&s["lastAt"]))
    {
        return Err("MARKET_HISTORY_SCOPE_OR_CLOCK".into());
    }
    for id in snapshot["missingIds"].as_array().cloned().unwrap_or_default() {
        let key = id.as_str().unwrap_or("");
        if !s["minutes"][key].is_null() {
            s["minutes"][key]["unavailableAt"] = snapshot["receivedAt"].clone();
        }
    }
    for q in snapshot["quotes"].as_array().cloned().unwrap_or_default() {
        let valid = !q["bestBid"].is_null() && !q["bestAsk"].is_null() && !js_truthy(q.get("crossed"))
            && q["bestBid"].as_f64().unwrap_or(0.0) > 0.0 && q["bestAsk"].as_f64().unwrap_or(1.0) < 1.0;
        let eid = q["exchangeId"].as_str().unwrap_or("").to_string();
        let mut series = if s["minutes"][&eid].is_null() {
            json!({"marketId": q["marketId"], "buckets": [], "latest": V::Null, "unavailableAt": V::Null})
        } else {
            s["minutes"][&eid].clone()
        };
        if !valid {
            series["unavailableAt"] = snapshot["receivedAt"].clone();
            s["minutes"][&eid] = series;
            continue;
        }
        let bucket = (now / 60000.0).floor();
        let point = json!({"bucket": bucket, "at": snapshot["receivedAt"], "bid": q["bestBid"], "ask": q["bestAsk"]});
        let mut buckets = series["buckets"].as_array().cloned().unwrap_or_default();
        if buckets.last().map(|b| b["bucket"].as_f64() == Some(bucket)).unwrap_or(false) {
            *buckets.last_mut().unwrap() = point.clone();
        } else {
            buckets.push(point.clone());
        }
        let cutoff = now - policy["windowMs"].as_f64().unwrap_or(0.0);
        buckets.retain(|x| parse_at(&x["at"]) >= cutoff);
        series["buckets"] = json!(buckets);
        series["latest"] = point;
        series["unavailableAt"] = V::Null;
        s["minutes"][&eid] = series;
    }
    s["lastAt"] = snapshot["receivedAt"].clone();
    s["observations"] = json!(s["observations"].as_i64().unwrap_or(0) + 1);
    Ok(s)
}
/// rankMarketHistory(state, universe, clockAt, policy)
pub fn rank_market_history(state: &V, universe: &V, clock_at: &str, policy: &V) -> Result<V, String> {
    let now = baal_service_runtime::contract::parse_iso_ms(clock_at).unwrap_or(f64::NAN);
    if !now.is_finite() || state["universeHash"] != universe["hash"]
        || (!state["lastAt"].is_null() && now < parse_at(&state["lastAt"]))
    {
        return Err("MARKET_HISTORY_RANK_CLOCK".into());
    }
    let mut leads = Vec::new();
    let mut counts = json!({"observed": 0, "unavailable": 0, "stale": 0, "insufficientHistory": 0,
        "wideSpread": 0, "noNetDiscount": 0, "eligible": 0});
    let window = policy["windowMs"].as_f64().unwrap_or(0.0);
    let max_age = policy["maxAgeMs"].as_f64().unwrap_or(0.0);
    let min_minutes = policy["minMinutes"].as_i64().unwrap_or(0) as usize;
    let min_span = policy["minSpanMs"].as_f64().unwrap_or(0.0);
    let max_spread = policy["maxSpread"].as_f64().unwrap_or(0.0);
    let min_net = policy["minNetRecovery"].as_f64().unwrap_or(0.0);
    let lower_q = policy["lowerQuantile"].as_f64().unwrap_or(0.0);
    for spec in universe["exchanges"].as_array().cloned().unwrap_or_default() {
        let s = &state["minutes"][spec["exchangeId"].as_str().unwrap_or("")];
        if s.is_null() { continue; }
        counts["observed"] = json!(counts["observed"].as_i64().unwrap_or(0) + 1);
        if !s["unavailableAt"].is_null() || s["latest"].is_null() {
            counts["unavailable"] = json!(counts["unavailable"].as_i64().unwrap_or(0) + 1);
            continue;
        }
        let age = now - parse_at(&s["latest"]["at"]);
        if age < 0.0 || age > max_age {
            counts["stale"] = json!(counts["stale"].as_i64().unwrap_or(0) + 1);
            continue;
        }
        // Current minute and the immediately prior minute cannot set the reference.
        // Minute weighting prevents faster polling from manufacturing more evidence.
        let latest_bucket = s["latest"]["bucket"].as_f64().unwrap_or(f64::NAN);
        let past: Vec<V> = s["buckets"].as_array().cloned().unwrap_or_default().into_iter()
            .filter(|x| x["bucket"].as_f64().unwrap_or(f64::NAN) < latest_bucket - 1.0 && parse_at(&x["at"]) >= now - window)
            .collect();
        if past.len() < min_minutes
            || parse_at(&past.last().unwrap_or(&V::Null)["at"]) - parse_at(&past[0]["at"]) < min_span
        {
            counts["insufficientHistory"] = json!(counts["insufficientHistory"].as_i64().unwrap_or(0) + 1);
            continue;
        }
        let latest_ask = s["latest"]["ask"].as_f64().unwrap_or(0.0);
        let latest_bid = s["latest"]["bid"].as_f64().unwrap_or(0.0);
        if latest_ask - latest_bid > max_spread + 1e-9 {
            counts["wideSpread"] = json!(counts["wideSpread"].as_i64().unwrap_or(0) + 1);
            continue;
        }
        let mut bids: Vec<f64> = past.iter().map(|x| x["bid"].as_f64().unwrap_or(0.0)).collect();
        bids.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let index = ((bids.len() - 1) as f64 * lower_q).floor() as usize;
        let reference_bid = bids[index];
        let net_recovery = reference_bid - latest_ask;
        if net_recovery < min_net - 1e-9 {
            counts["noNetDiscount"] = json!(counts["noNetDiscount"].as_i64().unwrap_or(0) + 1);
            continue;
        }
        counts["eligible"] = json!(counts["eligible"].as_i64().unwrap_or(0) + 1);
        leads.push(json!({"marketId": spec["marketId"], "exchangeId": spec["exchangeId"],
            "receivedAt": s["latest"]["at"], "referenceBid": reference_bid, "entryAsk": latest_ask,
            "currentBid": latest_bid, "netRecovery": net_recovery, "minutes": past.len(),
            "historyThrough": past.last().unwrap()["at"], "executable": false,
            "reason": "prior_bid_discount_requires_fresh_depth"}));
    }
    leads.sort_by(|a: &V, b: &V| {
        b["netRecovery"].as_f64().unwrap_or(0.0).partial_cmp(&a["netRecovery"].as_f64().unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal)
            .then(a["marketId"].as_str().unwrap_or("").cmp(b["marketId"].as_str().unwrap_or("")))
    });
    Ok(json!({"clockAt": clock_at, "policy": policy, "counts": counts, "leads": leads,
        "executable": false, "networkRequests": 0}))
}
