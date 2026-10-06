// live-paper/breadth-export.mjs — latest bulk-batch extraction + lead export.
use baal_service_runtime::contract::parse_iso_ms;
use baal_service_runtime::json_text::{js_stringify, js_truthy};
use serde_json::{json, Map, Value as V};
use std::collections::{BTreeMap, BTreeSet};

/// String(v) for scalars: strings as-is, numbers via their JS decimal form.
fn js_str(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Number(n) => n.to_string(),
        V::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}
fn checked(snapshot: &V, u: &V) -> Result<(usize, V), String> {
    if snapshot["tournamentId"] != u["tournamentId"] || snapshot["universeHash"] != u["hash"]
        || snapshot.get("sourceTimestamp") != Some(&V::Null)
        || snapshot.get("depthAvailable") != Some(&V::Bool(false))
        || snapshot.get("executable") != Some(&V::Bool(false))
        || !snapshot["quotes"].is_array() || !snapshot["missingIds"].is_array()
    {
        return Err("Invalid captured bulk snapshot".into());
    }
    let mut ids: Vec<String> = snapshot["quotes"].as_array().unwrap().iter().map(|q| js_str(&q["exchangeId"]))
        .chain(snapshot["missingIds"].as_array().unwrap().iter().map(js_str)).collect();
    ids.sort();
    let ids = ids.join(",");
    let mut index: i64 = -1;
    let batches = (u["exchanges"].as_array().map(|a| a.len()).unwrap_or(0) + 99) / 100;
    for i in 0..batches {
        let mut b: Vec<String> = crate::breadth::price_batch(u, i).iter().map(|q| js_str(&q["exchangeId"])).collect();
        b.sort();
        if b.join(",") == ids { index = i as i64; }
    }
    if index < 0 { return Err("Bulk snapshot does not match a catalogue batch".into()); }
    let batch = crate::breadth::price_batch(u, index as usize);
    let normalized = crate::breadth::normalize_prices(
        &json!({"endpoint": crate::breadth::price_endpoint(&batch, u["tournamentId"].as_str().unwrap_or("")),
            "requestStartedAt": snapshot["requestStartedAt"], "receivedAt": snapshot["receivedAt"],
            "data": {"data": snapshot["quotes"], "missingIds": snapshot["missingIds"]}}),
        &batch, u)?;
    Ok((index as usize, normalized))
}
/// extractLatestBulk(raw, universe)
pub fn extract_latest_bulk(raw: &str, universe: &V) -> Result<V, String> {
    let records = crate::ml_shadow::verify_source(raw)?;
    let records = match records { Some(v) if !v.records.is_empty() => v.records, _ => return Err("Complete source required".into()) };
    let u = crate::breadth::validate_universe(universe, universe["tournamentId"].as_str().unwrap_or(""))?;
    let mut latest: BTreeMap<usize, V> = BTreeMap::new();
    for r in &records {
        let b = &r["event"]["bulkPrices"];
        if b.is_null() { continue; }
        let (index, snapshot) = checked(b, &u)?;
        let clock = if js_truthy(r["event"].get("clockAt")) { &r["event"]["clockAt"] } else { &r["at"] };
        if parse_iso_ms(snapshot["receivedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN)
            > parse_iso_ms(clock.as_str().unwrap_or("")).unwrap_or(f64::NAN)
        {
            return Err("Bulk receipt after event clock".into());
        }
        if let Some(old) = latest.get(&index) {
            if parse_iso_ms(snapshot["receivedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN)
                < parse_iso_ms(old["receivedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN)
            {
                return Err("Bulk receipt rollback".into());
            }
        }
        latest.insert(index, snapshot);
    }
    let last = records.last().unwrap();
    let as_of = if js_truthy(last["event"].get("clockAt")) { last["event"]["clockAt"].clone() } else { last["at"].clone() };
    Ok(json!({"schemaVersion": 1, "kind": "derived_latest_market_bulk_batches", "asOf": as_of,
        "snapshots": latest.values().cloned().collect::<Vec<_>>()}))
}
/// exportBreadth(input, universe)
pub fn export_breadth(input: &V, universe: &V) -> Result<V, String> {
    if input["schemaVersion"].as_i64() != Some(1)
        || input["kind"].as_str() != Some("derived_latest_market_bulk_batches")
        || !input["asOf"].as_str().and_then(parse_iso_ms).is_some_and(|x| x.is_finite())
        || !input["snapshots"].is_array()
    {
        return Err("Invalid bulk export input".into());
    }
    let u = crate::breadth::validate_universe(universe, universe["tournamentId"].as_str().unwrap_or(""))?;
    // Map keys keep JS SameValueZero identity via the canonical JSON form.
    let mut quotes: Map<String, V> = Map::new();
    let mut seen: BTreeSet<usize> = BTreeSet::new();
    for raw in input["snapshots"].as_array().cloned().unwrap_or_default() {
        let (index, snapshot) = checked(&raw, &u)?;
        if !seen.insert(index)
            || parse_iso_ms(snapshot["receivedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN)
                > parse_iso_ms(input["asOf"].as_str().unwrap_or("")).unwrap_or(f64::NAN)
        {
            return Err("Duplicate/future bulk batch".into());
        }
        for q in snapshot["quotes"].as_array().cloned().unwrap_or_default() {
            quotes.insert(js_stringify(&q["exchangeId"]), json!({"quote": q,
                "requestStartedAt": snapshot["requestStartedAt"], "receivedAt": snapshot["receivedAt"],
                "status": "observed"}));
        }
        for id in snapshot["missingIds"].as_array().cloned().unwrap_or_default() {
            quotes.insert(js_stringify(&id), json!({"quote": V::Null,
                "requestStartedAt": snapshot["requestStartedAt"], "receivedAt": snapshot["receivedAt"],
                "status": "missing_in_latest_batch"}));
        }
    }
    let as_of_ms = parse_iso_ms(input["asOf"].as_str().unwrap_or("")).unwrap_or(f64::NAN);
    let mut exchanges = Vec::new();
    for spec in u["exchanges"].as_array().cloned().unwrap_or_default() {
        let o = quotes.get(&js_stringify(&spec["exchangeId"])).cloned().unwrap_or(V::Null);
        let q = &o["quote"];
        let status = if js_truthy(o.get("status")) { o["status"].clone() } else { json!("not_observed") };
        exchanges.push(json!({
            "marketId": spec["marketId"], "exchangeId": spec["exchangeId"], "option": spec["option"],
            "title": V::Null, "party": V::Null, "race": V::Null, "stage": V::Null,
            "settlementPredicate": V::Null,
            "identityMetadataStatus": "not_present_in_local_id_catalogue",
            "status": status,
            "latestPrice": q.get("latestPrice").cloned().unwrap_or(V::Null),
            "bestBid": q.get("bestBid").cloned().unwrap_or(V::Null),
            "bestAsk": q.get("bestAsk").cloned().unwrap_or(V::Null),
            "spread": q.get("spread").cloned().unwrap_or(V::Null),
            "crossed": q.get("crossed").cloned().unwrap_or(V::Null),
            "requestStartedAt": o.get("requestStartedAt").cloned().unwrap_or(V::Null),
            "receivedAt": o.get("receivedAt").cloned().unwrap_or(V::Null),
            "collectionAgeMs": if o.is_null() { V::Null } else {
                json!(as_of_ms - parse_iso_ms(o["receivedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN)) },
            "sourceTimestamp": V::Null, "sourceQuoteAgeMs": V::Null,
            "depthAvailable": false, "executable": false,
        }));
    }
    Ok(json!({"schemaVersion": 1, "kind": "bulk_price_lead_generation_only",
        "asOf": input["asOf"], "tournamentId": u["tournamentId"], "universeHash": u["hash"],
        "universeSize": exchanges.len(), "batchesObserved": seen.len(), "exchanges": exchanges,
        "limitations": [
            "Mixed collection ages; not a simultaneous snapshot",
            "Official bulk schema supplies neither engine quote time nor quantities; receipt age is not quote freshness",
            "Identity titles, party, race, stage and exact settlement predicates are unavailable in the local catalogue and remain null",
            "External comparisons require an independently verified exact contract join, fresh depth, rules and cost/risk checks",
            "Derived normalized snapshots cannot independently verify the unshared original journal or omitted account context",
            "No source prices are asserted as fair probabilities or executable fills",
        ],
        "networkRequests": 0, "competitionOrders": 0}))
}
