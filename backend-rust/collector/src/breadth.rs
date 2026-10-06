// live-paper/breadth.mjs — bulk price universe, batching, response validation,
// coverage rollup. Hash is sha256 of the JSON.stringify serialization
// (insertion order, not canonical).
use baal_service_runtime::contract::parse_iso_ms;
use baal_service_runtime::json_text::{js_stringify, sha256};
use serde_json::{json, Value as V};
fn is_id(v: &V) -> bool {
    v.as_str().is_some_and(|s| {
        !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && s.as_bytes()[0] != b'0' && s.parse::<i64>().is_ok()
    })
}
pub fn validate_universe(u: &V, tournament_id: &str) -> Result<V, String> {
    if u.get("tournamentId").and_then(V::as_str) != Some(tournament_id)
        || !u.get("exchanges").is_some_and(|e| e.as_array().is_some_and(|a| !a.is_empty())) {
        return Err("Invalid price universe".into());
    }
    let mut seen = std::collections::HashSet::new();
    for x in u["exchanges"].as_array().unwrap() {
        let eid = x.get("exchangeId").cloned().unwrap_or(V::Null);
        let opt = x.get("option");
        if !is_id(&eid) || !is_id(&x["marketId"]) || !seen.insert(eid.as_str().unwrap().to_string())
            || !opt.is_none_or(|o| o.is_null() || o.is_string()) {
            return Err("Invalid price universe exchange".into());
        }
    }
    let mut out = u.clone();
    out["hash"] = json!(sha256(js_stringify(u).as_bytes()));
    Ok(out)
}
pub fn price_batch(u: &V, index: usize) -> Vec<V> {
    let exchanges = u["exchanges"].as_array().map(|a| a.as_slice()).unwrap_or(&[]);
    let batches = (exchanges.len() + 99) / 100;
    if batches == 0 { return Vec::new(); }
    let i = index % batches;
    exchanges[i * 100..((i + 1) * 100).min(exchanges.len())].to_vec()
}
pub fn price_endpoint(batch: &[V], tournament_id: &str) -> String {
    let ids: Vec<&str> = batch.iter().filter_map(|x| x["exchangeId"].as_str()).collect();
    format!("/exchanges/prices?ids={}&tournamentId={}", ids.join(","), tournament_id)
}
pub fn normalize_prices(response: &V, batch: &[V], u: &V) -> Result<V, String> {
    let data = response.get("data").and_then(|d| d.get("data")).and_then(V::as_array).cloned().unwrap_or_default();
    let missing = response.get("data").and_then(|d| d.get("missingIds")).and_then(V::as_array).cloned().unwrap_or_default();
    let started = response.get("requestStartedAt").and_then(V::as_str).and_then(parse_iso_ms);
    let received = response.get("receivedAt").and_then(V::as_str).and_then(parse_iso_ms);
    if response.get("endpoint").and_then(V::as_str) != Some(price_endpoint(batch, u["tournamentId"].as_str().unwrap_or("")).as_str())
        || !response.get("data").and_then(|d| d.get("data")).is_some_and(V::is_array)
        || !response.get("data").and_then(|d| d.get("missingIds")).is_some_and(V::is_array)
        || started.is_none() || received.is_none() || received < started {
        return Err("Invalid bulk price response".into());
    }
    let mut seen = std::collections::HashSet::new();
    let mut quotes = Vec::new();
    for x in &data {
        let eid = x.get("exchangeId").and_then(V::as_str).unwrap_or("");
        let spec = batch.iter().find(|b| b["exchangeId"].as_str() == Some(eid));
        if spec.is_none() || !seen.insert(eid.to_string())
            || x.get("marketId") != spec.unwrap().get("marketId")
            || x.get("option") != spec.unwrap().get("option") {
            return Err("Bulk price identity mismatch".into());
        }
        for k in ["latestPrice", "bestBid", "bestAsk", "spread"] {
            match x.get(k) {
                None => return Err("Invalid bulk price value".into()),
                Some(V::Null) => {}
                Some(v) => {
                    let n = v.as_f64();
                    if !n.is_some_and(|n| n.is_finite() && n <= 1.0 && n >= if k == "spread" { -1.0 } else { 0.0 }) {
                        return Err("Invalid bulk price value".into());
                    }
                }
            }
        }
        let (bid, ask) = (x["bestBid"].as_f64(), x["bestAsk"].as_f64());
        quotes.push(json!({"exchangeId": x["exchangeId"], "marketId": x["marketId"], "option": x["option"],
            "latestPrice": x["latestPrice"], "bestBid": x["bestBid"], "bestAsk": x["bestAsk"], "spread": x["spread"],
            "crossed": bid.is_some() && ask.is_some() && bid.unwrap() >= ask.unwrap()}));
    }
    for x in &missing {
        let s = x.as_str().unwrap_or("");
        if !batch.iter().any(|b| b["exchangeId"].as_str() == Some(s)) || !seen.insert(s.to_string()) {
            return Err("Invalid bulk price missing IDs".into());
        }
    }
    if seen.len() != batch.len() { return Err("Incomplete bulk price coverage".into()); }
    Ok(json!({"tournamentId": u["tournamentId"], "universeHash": u["hash"],
        "requestStartedAt": response["requestStartedAt"], "receivedAt": response["receivedAt"],
        "sourceTimestamp": V::Null, "depthAvailable": false, "executable": false,
        "quotes": quotes, "missingIds": missing}))
}
pub fn update_coverage(previous: &V, snapshot: &V, u: &V, batch_index: usize) -> V {
    let mut quotes = if previous.get("universeHash") == u.get("hash") {
        previous.get("quotes").and_then(V::as_object).cloned().unwrap_or_default()
    } else { serde_json::Map::new() };
    for x in snapshot.get("missingIds").and_then(V::as_array).into_iter().flatten().filter_map(V::as_str) {
        quotes.remove(x);
    }
    for x in snapshot.get("quotes").and_then(V::as_array).into_iter().flatten() {
        let mut q = x.clone();
        q["receivedAt"] = snapshot["receivedAt"].clone();
        q["requestStartedAt"] = snapshot["requestStartedAt"].clone();
        if let Some(eid) = x.get("exchangeId").and_then(V::as_str) { quotes.insert(eid.to_string(), q); }
    }
    let n = u["exchanges"].as_array().map(|a| a.len()).unwrap_or(0);
    json!({"universeHash": u["hash"], "tournamentId": u["tournamentId"], "universeSize": n,
        "nextBatch": (batch_index + 1) % ((n + 99) / 100).max(1), "updatedAt": snapshot["receivedAt"],
        "executable": false, "sourceTimestamp": V::Null, "depthAvailable": false,
        "quotes": V::Object(quotes), "lastMissingIds": snapshot["missingIds"]})
}
