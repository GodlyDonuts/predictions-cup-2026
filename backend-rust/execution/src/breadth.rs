// live-paper/breadth.mjs — bulk price universe, batching, response validation,
// and coverage updates. Ported into baal-execution because
// prepare-baal-combined-custody-v6.mjs imports ../live-paper/breadth.mjs and
// baal-execution does not depend on baal-collector.
use baal_service_runtime::json_text::{js_int, sha256};
use serde_json::{json, Value as V};
use std::collections::{BTreeSet, HashMap};

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn sget<'a>(v: &'a V, k: &str) -> Option<&'a str> {
    v.get(k).and_then(V::as_str)
}
fn id(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        !s.is_empty()
            && !s.starts_with('0')
            && s.bytes().all(|b| b.is_ascii_digit())
            && s.parse::<u64>().is_ok()
    })
}
fn iso_ms(v: &V) -> f64 {
    baal_service_runtime::contract::parse_iso_ms(v.as_str().unwrap_or(""))
        .unwrap_or(f64::NAN)
}

/// export function validateUniverse(u,tournamentId)
pub fn validate_universe(u: &V, tournament_id: &str) -> Result<V, String> {
    if sget(u, "tournamentId") != Some(tournament_id)
        || !u.get("exchanges").is_some_and(V::is_array)
        || u["exchanges"].as_array().unwrap().is_empty()
    {
        return Err("Invalid price universe".into());
    }
    let mut seen = BTreeSet::new();
    for x in u["exchanges"].as_array().unwrap() {
        if !id(get(x, "exchangeId"))
            || !id(get(x, "marketId"))
            || seen.contains(sget(x, "exchangeId").unwrap_or(""))
            || !x.get("option").is_some_and(|o| o.is_null() || o.is_string())
        {
            return Err("Invalid price universe exchange".into());
        }
        seen.insert(sget(x, "exchangeId").unwrap_or("").to_string());
    }
    let mut out = u.clone();
    out.as_object_mut()
        .unwrap()
        .insert("hash".into(), json!(sha256(baal_service_runtime::json_text::js_stringify(u).as_bytes())));
    Ok(out)
}

/// export function priceBatch(u,index)
pub fn price_batch(u: &V, index: i64) -> Vec<V> {
    let exchanges = u["exchanges"].as_array().cloned().unwrap_or_default();
    let batches = (exchanges.len() as i64 + 99) / 100;
    if batches <= 0 {
        return Vec::new();
    }
    let i = index % batches;
    let start = i * 100;
    let end = (i + 1) * 100;
    if start < 0 || start >= exchanges.len() as i64 {
        return Vec::new();
    }
    exchanges[start as usize..end.min(exchanges.len() as i64) as usize].to_vec()
}

/// export function priceEndpoint(batch,tournamentId)
pub fn price_endpoint(batch: &[V], tournament_id: &str) -> String {
    let ids: Vec<&str> = batch
        .iter()
        .filter_map(|x| sget(x, "exchangeId"))
        .collect();
    format!("/exchanges/prices?ids={}&tournamentId={tournament_id}", ids.join(","))
}

/// export function normalizePrices(response,batch,u)
pub fn normalize_prices(response: &V, batch: &[V], u: &V) -> Result<V, String> {
    let data = get(get(response, "data"), "data");
    let missing_ids = get(get(response, "data"), "missingIds");
    if sget(response, "endpoint") != Some(price_endpoint(batch, sget(u, "tournamentId").unwrap_or("")).as_str())
        || !data.is_array()
        || !missing_ids.is_array()
        || !iso_ms(get(response, "requestStartedAt")).is_finite()
        || !iso_ms(get(response, "receivedAt")).is_finite()
        || iso_ms(get(response, "receivedAt")) < iso_ms(get(response, "requestStartedAt"))
    {
        return Err("Invalid bulk price response".into());
    }
    let expected: HashMap<String, V> = batch
        .iter()
        .filter_map(|x| sget(x, "exchangeId").map(|e| (e.to_string(), x.clone())))
        .collect();
    let mut seen = BTreeSet::new();
    let mut quotes = Vec::new();
    for x in data.as_array().unwrap() {
        let ex = sget(x, "exchangeId").unwrap_or("");
        let spec = expected.get(ex);
        if spec.is_none()
            || seen.contains(ex)
            || sget(x, "marketId") != sget(spec.unwrap(), "marketId")
            || get(x, "option") != get(spec.unwrap(), "option")
        {
            return Err("Bulk price identity mismatch".into());
        }
        for k in ["latestPrice", "bestBid", "bestAsk", "spread"] {
            let v = get(x, k);
            if !v.is_null() {
                let ok = v.as_f64().is_some_and(|n| {
                    n.is_finite() && n <= 1.0 && n >= if k == "spread" { -1.0 } else { 0.0 }
                });
                if !ok {
                    return Err("Invalid bulk price value".into());
                }
            }
        }
        seen.insert(ex.to_string());
        let bid = x.get("bestBid").and_then(V::as_f64);
        let ask = x.get("bestAsk").and_then(V::as_f64);
        quotes.push(json!({
            "exchangeId": x["exchangeId"],
            "marketId": x["marketId"],
            "option": x["option"],
            "latestPrice": x["latestPrice"],
            "bestBid": x["bestBid"],
            "bestAsk": x["bestAsk"],
            "spread": x["spread"],
            "crossed": bid.is_some() && ask.is_some() && bid.unwrap() >= ask.unwrap(),
        }));
    }
    for x in missing_ids.as_array().unwrap() {
        if !x.as_str().is_some_and(|s| expected.contains_key(s)) || seen.contains(x.as_str().unwrap_or("")) {
            return Err("Invalid bulk price missing IDs".into());
        }
        seen.insert(x.as_str().unwrap_or("").to_string());
    }
    if seen.len() != expected.len() {
        return Err("Incomplete bulk price coverage".into());
    }
    // The official bulk schema supplies no quote timestamp or depth. Receipt
    // time measures collection age only; these observations cannot authorize
    // fills.
    Ok(json!({
        "tournamentId": get(u, "tournamentId"),
        "universeHash": get(u, "hash"),
        "requestStartedAt": get(response, "requestStartedAt"),
        "receivedAt": get(response, "receivedAt"),
        "sourceTimestamp": V::Null,
        "depthAvailable": false,
        "executable": false,
        "quotes": quotes,
        "missingIds": missing_ids,
    }))
}

/// export function updateCoverage(previous,snapshot,u,batchIndex)
pub fn update_coverage(previous: &V, snapshot: &V, u: &V, batch_index: i64) -> V {
    let mut quotes = if sget(previous, "universeHash") == sget(u, "hash") {
        get(previous, "quotes").as_object().cloned().unwrap_or_default()
    } else {
        Default::default()
    };
    for x in get(snapshot, "missingIds").as_array().cloned().unwrap_or_default() {
        if let Some(s) = x.as_str() {
            quotes.shift_remove(s);
        }
    }
    for x in get(snapshot, "quotes").as_array().cloned().unwrap_or_default() {
        if let Some(ex) = sget(&x, "exchangeId") {
            let mut q = x.clone();
            if let V::Object(m) = &mut q {
                m.insert("receivedAt".into(), get(snapshot, "receivedAt").clone());
                m.insert("requestStartedAt".into(), get(snapshot, "requestStartedAt").clone());
            }
            quotes.insert(ex.to_string(), q);
        }
    }
    let size = u["exchanges"].as_array().map(|a| a.len() as i64).unwrap_or(0);
    json!({
        "universeHash": get(u, "hash"),
        "tournamentId": get(u, "tournamentId"),
        "universeSize": size,
        "nextBatch": (batch_index + 1) % ((size + 99) / 100).max(1),
        "updatedAt": get(snapshot, "receivedAt"),
        "executable": false,
        "sourceTimestamp": V::Null,
        "depthAvailable": false,
        "quotes": quotes,
        "lastMissingIds": get(snapshot, "missingIds"),
    })
}
