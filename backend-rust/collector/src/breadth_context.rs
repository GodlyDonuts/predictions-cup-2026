// live-paper/breadth-context.mjs
use baal_service_runtime::contract::parse_iso_ms;
use serde_json::{json, Value as V};
pub const MAX_CONTEXT_AGE_MS: f64 = 300000.0;
fn digits(s: &str) -> bool { !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) }
/// updateContext(input, snapshot, clockAt)
pub fn update_context(input: Option<&V>, snapshot: Option<&V>, clock_at: Option<&str>) -> Result<V, String> {
    let mut state = input.cloned().unwrap_or_else(|| json!({"tournamentId": null, "quotes": {}}));
    let Some(snapshot) = snapshot else { return Ok(state); };
    if snapshot["executable"] != V::Bool(false) || snapshot["depthAvailable"] != V::Bool(false)
        || !snapshot.get("receivedAt").and_then(V::as_str).and_then(parse_iso_ms).is_some()
        || !snapshot.get("quotes").is_some_and(V::is_array)
        || !snapshot.get("missingIds").is_some_and(V::is_array) {
        return Err("Invalid breadth feature context".into());
    }
    let at = parse_iso_ms(snapshot["receivedAt"].as_str().unwrap()).unwrap();
    if clock_at.and_then(parse_iso_ms).is_some_and(|c| at > c) {
        return Err("Future breadth context".into());
    }
    if !state["tournamentId"].is_null() && state["tournamentId"] != snapshot["tournamentId"] {
        return Err("Breadth context scope changed".into());
    }
    state["tournamentId"] = snapshot["tournamentId"].clone();
    let mut seen = std::collections::HashSet::new();
    for id in snapshot["missingIds"].as_array().unwrap() {
        let s = id.as_str().ok_or("Invalid breadth context IDs")?;
        if !digits(s) || !seen.insert(s.to_string()) { return Err("Invalid breadth context IDs".into()); }
        state["quotes"].as_object_mut().map(|q| q.remove(s));
    }
    for q in snapshot["quotes"].as_array().unwrap() {
        let (eid, mid) = (q["exchangeId"].as_str().unwrap_or(""), q["marketId"].as_str().unwrap_or(""));
        if !digits(eid) || !digits(mid) || !seen.insert(eid.to_string()) {
            return Err("Invalid breadth context identity".into());
        }
        for k in ["bestBid", "bestAsk"] {
            match &q[k] { V::Null => {}, v => if !v.as_f64().is_some_and(|n| (0.0..=1.0).contains(&n)) { return Err("Invalid breadth context price".into()); } }
        }
        let previous = state["quotes"].get(eid).cloned();
        if let Some(p) = &previous {
            if parse_iso_ms(p["receivedAt"].as_str().unwrap_or("")).is_some_and(|pa| at <= pa) {
                return Err("Breadth context time rollback".into());
            }
        }
        let (bid, ask) = (q["bestBid"].as_f64(), q["bestAsk"].as_f64());
        let mid_price = if bid.is_some() && ask.is_some() && bid < ask { Some((bid.unwrap() + ask.unwrap()) / 2.0) } else { None };
        let prior = previous.filter(|p| at - parse_iso_ms(p["receivedAt"].as_str().unwrap_or("")).unwrap_or(f64::NEG_INFINITY) <= MAX_CONTEXT_AGE_MS);
        let change = match (mid_price, prior.as_ref().and_then(|p| p["mid"].as_f64())) {
            (Some(m), Some(pm)) => json!(m - pm), _ => V::Null,
        };
        state["quotes"][eid] = json!({"marketId": mid, "mid": mid_price,
            "spread": mid_price.map(|_| q["bestAsk"].as_f64().unwrap() - bid.unwrap()),
            "change": change, "receivedAt": snapshot["receivedAt"]});
    }
    Ok(state)
}
/// contextFeatures(state, book, tournamentId) -> [f64; 7]
pub fn context_features(state: &V, book: &V, tournament_id: &str) -> Result<[f64; 7], String> {
    if state.get("tournamentId").and_then(V::as_str).is_some_and(|t| t != tournament_id) {
        return Err("Breadth feature tournament mismatch".into());
    }
    let clock = book["requestStartedAt"].as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN);
    let available: Vec<(&String, &V)> = state.get("quotes").and_then(V::as_object).map(|o| o.iter().filter(|(_, q)| {
        let age = clock - parse_iso_ms(q["receivedAt"].as_str().unwrap_or("")).unwrap_or(f64::INFINITY);
        (0.0..=MAX_CONTEXT_AGE_MS).contains(&age) && !q["mid"].is_null()
    }).collect()).unwrap_or_default();
    let own = available.iter().find(|(id, q)| id.as_str() == book["exchangeId"].as_str().unwrap_or("") && q["marketId"] == book["marketId"]).map(|(_, q)| *q);
    let amt = |v: &V| baal_paper::fixed::big_amount(v.as_str().unwrap_or("")).ok()
        .and_then(|b| b.to_string().parse::<f64>().ok()).unwrap_or(f64::NAN);
    let depth_mid = (amt(&book["bids"][0]["price"]) + amt(&book["asks"][0]["price"])) / 2e6;
    let changes: Vec<f64> = available.iter().filter_map(|(_, q)| q["change"].as_f64()).collect();
    let absolute_change = if changes.is_empty() { 0.0 } else { changes.iter().map(|x| x.abs()).sum::<f64>() / changes.len() as f64 };
    Ok([
        own.map(|o| o["mid"].as_f64().unwrap_or(f64::NAN) - depth_mid).unwrap_or(0.0),
        own.and_then(|o| o["spread"].as_f64()).unwrap_or(0.0),
        absolute_change,
        if changes.is_empty() { 0.0 } else { changes.iter().filter(|x| **x != 0.0).count() as f64 / changes.len() as f64 },
        (available.len() as f64 / 237.0).min(1.0),
        own.map(|o| (clock - parse_iso_ms(o["receivedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN)) / MAX_CONTEXT_AGE_MS).unwrap_or(1.0),
        if own.is_some() { 0.0 } else { 1.0 },
    ])
}
