// bot/strategy.mjs — opposing-party metadata pairing + package planning.
// Metadata matches are research hypotheses, not proof of exclusive settlement.
use serde_json::{json, Value as V};

fn falsy(v: &V) -> bool {
    v.is_null()
        || v.as_str() == Some("")
        || v.as_bool() == Some(false)
        || v.as_f64() == Some(0.0)
}

/// JSON.stringify of [raceId, stageId, electionDate, settlement_date, resolutionType]
/// for recognized General Party Winner contracts, else null.
pub fn race_identity(node: &V) -> Option<String> {
    let d = &node["contract_details"];
    if node["node_type"].as_str() != Some("contract")
        || node["contract_type"].as_str() != Some("Election Outcome")
        || d["resolutionType"].as_str() != Some("Party Winner")
        || d["raceStage"].as_str() != Some("General")
        || falsy(&d["raceId"])
        || falsy(&d["stageId"])
        || falsy(&d["electionDate"])
        || falsy(&d["winnerName"])
        || falsy(&node["settlement_date"])
        || baal_service_runtime::contract::parse_iso_ms(node["settlement_date"].as_str().unwrap_or("")).is_none()
    {
        return None;
    }
    Some(serde_json::to_string(&json!([
        d["raceId"], d["stageId"], d["electionDate"], node["settlement_date"], d["resolutionType"]
    ]))
    .unwrap())
}

/// Groups open single-exchange markets by race identity; returns only pairs
/// whose two legs name opposing recognized parties.
pub fn metadata_pairs(markets: &[V], nodes: &V) -> Vec<Vec<V>> {
    let mut groups: indexmap::IndexMap<String, Vec<V>> = indexmap::IndexMap::new();
    for m in markets {
        if m["status"].as_str() != Some("open")
            || m["isComposite"].as_bool().unwrap_or(false)
            || m["exchanges"].as_array().map(|e| e.len()) != Some(1)
        {
            continue;
        }
        let node = &nodes[m["id"].as_str().unwrap_or("")]["root"];
        let Some(key) = race_identity(node) else { continue };
        if !node["settled_with"].is_null() {
            continue;
        }
        let party = node["contract_details"]["winnerName"].as_str().unwrap_or("");
        // Conservative initial scope: recognized opposing parties only, distinct contracts.
        if party != "Democratic Party" && party != "Republican Party" {
            continue;
        }
        groups.entry(key).or_default().push(json!({
            "marketId": m["id"],
            "exchangeId": m["exchanges"][0]["id"],
            "title": m["title"],
            "party": party,
        }));
    }
    groups
        .into_values()
        .filter(|g| g.len() == 2 && g[0]["party"] != g[1]["party"])
        .collect()
}

/// Exact mills: nonnegative price/amount with at most 3 decimals -> i64.
pub fn to_mills(value: &V) -> Result<i64, String> {
    let text = match value {
        V::String(s) => s.clone(),
        V::Number(n) => n.to_string(),
        _ => return Err("Expected a nonnegative price or amount with at most three decimal places".into()),
    };
    let (whole, frac) = match text.split_once('.') {
        Some((w, f)) => (w, f),
        None => (text.as_str(), ""),
    };
    let valid = !whole.is_empty()
        && whole.bytes().all(|b| b.is_ascii_digit())
        && frac.len() <= 3
        && frac.bytes().all(|b| b.is_ascii_digit());
    if !valid {
        return Err("Expected a nonnegative price or amount with at most three decimal places".into());
    }
    let frac3 = format!("{:0<3}", frac);
    let result = whole
        .parse::<i64>()
        .ok()
        .and_then(|w| w.checked_mul(1000))
        .and_then(|w| frac3.parse::<i64>().ok().and_then(|f| w.checked_add(f)));
    match result {
        Some(r) if r <= 9_007_199_254_740_991 => Ok(r),
        _ => Err("Amount exceeds exact integer accounting range".into()),
    }
}

pub fn format_mills(value: i64) -> String {
    format!("{}.{:03}", value.div_euclid(1000), value.rem_euclid(1000))
}

/// NO is bought against a YES bid at its complement; cost rounds up to tick (5 mills).
fn no_offer(book: &V) -> Option<V> {
    let bids: Vec<&V> = book["bids"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|b| {
                    b["price"].as_f64().is_some()
                        && b["quantity"].as_f64().unwrap_or(0.0) >= 1.0
                        && b["price"].as_f64().unwrap_or(0.0) > 0.0
                        && b["price"].as_f64().unwrap_or(0.0) < 1.0
                })
                .collect()
        })
        .unwrap_or_default();
    if bids.is_empty() {
        return None;
    }
    let bid = bids
        .iter()
        .max_by(|a, b| a["price"].as_f64().partial_cmp(&b["price"].as_f64()).unwrap())
        .unwrap();
    let price_mills = ((1000 - to_mills(&bid["price"]).ok()?) as f64 / 5.0).ceil() as i64 * 5;
    Some(json!({"priceMills": price_mills, "quantity": bid["quantity"].as_f64().unwrap_or(0.0).floor()}))
}

/// planPair: unverified metadata hypothesis candidate, never eligible for execution.
pub fn plan_pair(pair: &[V], books: &V, cfg: &V, cash: &V, positions: &[V], open_orders: &[V]) -> Option<V> {
    if pair.len() != 2 || pair[0]["exchangeId"] == pair[1]["exchangeId"] {
        return None;
    }
    // Avoid interacting with the user's existing holdings or previously filled packages.
    if positions.iter().any(|p| {
        pair.iter().any(|m| m["exchangeId"] == p["exchangeId"]) && p["quantity"].as_f64().unwrap_or(0.0) != 0.0
    }) {
        return None;
    }
    // Conservatively exclude the entire exchange, independent of order side/price.
    if open_orders.iter().any(|o| {
        o["exchangeId"].is_null()
            || pair.iter().any(|m| m["exchangeId"].to_string() == o["exchangeId"].to_string())
    }) {
        return None;
    }
    let offers: Vec<V> = pair
        .iter()
        .map(|m| no_offer(&books[m["exchangeId"].as_str().unwrap_or("")]))
        .collect::<Option<Vec<_>>>()?;
    let cost = offers[0]["priceMills"].as_i64().unwrap_or(0) + offers[1]["priceMills"].as_i64().unwrap_or(0);
    let edge = 1000 - cost;
    if edge < to_mills(&cfg["minimumEdge"]).ok()? || cost <= 0 {
        return None;
    }
    let quantity = [
        cfg["maxSharesPerLeg"].as_f64().unwrap_or(0.0),
        offers[0]["quantity"].as_f64().unwrap_or(0.0) * 0.25,
        offers[1]["quantity"].as_f64().unwrap_or(0.0) * 0.25,
        to_mills(&cfg["maxPackageCost"]).ok()? as f64 / cost as f64,
        (to_mills(cash).ok()? - to_mills(&cfg["cashReserve"]).ok()?).max(0) as f64 / cost as f64,
    ]
    .iter()
    .cloned()
    .fold(f64::INFINITY, f64::min)
    .floor() as i64;
    if quantity < 1 {
        return None;
    }
    Some(json!({
        "pair": pair,
        "quantity": quantity,
        "unitCost": format_mills(cost),
        "classification": "unverified_metadata_hypothesis",
        "settlementVerified": false,
        "eligibleForExecution": false,
        "quoteDifference": format_mills(edge),
        "worstCaseCost": format_mills(cost * quantity),
        "legs": pair.iter().enumerate().map(|(i, m)| json!({
            "exchangeId": m["exchangeId"],
            "side": "no",
            "action": "buy",
            "quantity": quantity,
            "price": format_mills(offers[i]["priceMills"].as_i64().unwrap_or(0)).parse::<f64>().unwrap(),
            "tournamentId": cfg["tournamentId"],
        })).collect::<Vec<_>>(),
    }))
}
