// live-paper/breadth-features.mjs — across-batch quote-feature extraction.
use baal_service_runtime::contract::parse_iso_ms;
use baal_service_runtime::json_text::{js_stringify, js_truthy};
use serde_json::{json, Value as V};
use std::collections::HashMap;

fn parse(v: &V) -> f64 { v.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN) }
/// `a === b` over JSON values (numbers compare by exact bit pattern here; NaN
/// cannot appear inside parsed JSON).
fn same(a: &V, b: &V) -> bool { a == b }

/// breadthFeatures(records, universe): `records` is a verified source record
/// list (e.g. `verify_source(...)` `Some(source).records`).
pub fn breadth_features(records: &[V], universe: &V) -> Result<Vec<V>, String> {
    let mut history: HashMap<String, V> = HashMap::new();
    let mut rows: Vec<V> = Vec::new();
    for r in records {
        let s = &r["event"]["bulkPrices"];
        if s.is_null() { continue; }
        if s["universeHash"] != universe["hash"] || s["tournamentId"] != universe["tournamentId"] {
            return Err("Breadth universe mismatch".into());
        }
        let clock = if js_truthy(r["event"].get("clockAt")) { &r["event"]["clockAt"] } else { &r["at"] };
        if parse(&s["receivedAt"]) > parse(clock) { return Err("Future breadth observation".into()); }
        let exchanges = universe["exchanges"].as_array().cloned().unwrap_or_default();
        let mut batch: Vec<V> = Vec::new();
        let mut bad = false;
        for q in s["quotes"].as_array().cloned().unwrap_or_default() {
            match exchanges.iter().find(|x| same(&x["exchangeId"], &q["exchangeId"])) {
                Some(x) => batch.push(x.clone()),
                None => bad = true,
            }
        }
        for id in s["missingIds"].as_array().cloned().unwrap_or_default() {
            match exchanges.iter().find(|x| same(&x["exchangeId"], &id)) {
                Some(x) => batch.push(x.clone()),
                None => bad = true,
            }
        }
        if bad || batch.len() > 100 { return Err("Invalid breadth batch".into()); }
        crate::breadth::normalize_prices(
            &json!({"endpoint": crate::breadth::price_endpoint(&batch, universe["tournamentId"].as_str().unwrap_or("")),
                "requestStartedAt": s["requestStartedAt"], "receivedAt": s["receivedAt"],
                "data": {"data": s["quotes"], "missingIds": s["missingIds"]}}),
            &batch, universe)?;
        for id in s["missingIds"].as_array().cloned().unwrap_or_default() {
            history.remove(&js_stringify(&id));
        }
        let receipt = parse(&s["receivedAt"]);
        for q in s["quotes"].as_array().cloned().unwrap_or_default() {
            let key = js_stringify(&q["exchangeId"]);
            let h = history.get(&key).cloned();
            let dt = h.as_ref().map(|h| receipt - parse(&h["at"]));
            if dt.is_some_and(|d| d <= 0.0) { return Err("Breadth collection time rollback".into()); }
            // `!== null`: a missing key satisfies it (undefined !== null), the
            // final `<` comparison then settles it.
            let nb = q.get("bestBid");
            let na = q.get("bestAsk");
            let bid = nb.and_then(V::as_f64).unwrap_or(f64::NAN);
            let ask = na.and_then(V::as_f64).unwrap_or(f64::NAN);
            let paired = !nb.is_some_and(V::is_null) && !na.is_some_and(V::is_null) && bid < ask;
            let mid = if paired { json!((bid + ask) / 2.0) } else { V::Null };
            let previous = if h.is_some() && dt.is_some_and(|d| d <= 600000.0) { h } else { None };
            let mid_change = if !mid.is_null()
                && previous.as_ref().is_some_and(|p| p.get("mid").is_some_and(|m| !m.is_null()))
            {
                json!(mid.as_f64().unwrap_or(f64::NAN) - previous.as_ref().unwrap()["mid"].as_f64().unwrap_or(f64::NAN))
            } else { V::Null };
            let quote_changed = previous.as_ref().map(|p| !same(&p["bestBid"], &q["bestBid"]) || !same(&p["bestAsk"], &q["bestAsk"]));
            rows.push(json!({
                "exchangeId": q["exchangeId"], "marketId": q["marketId"], "at": s["receivedAt"],
                "mid": mid,
                "spread": if paired { json!(ask - bid) } else { V::Null },
                "collectionIntervalMs": dt.map(V::from).unwrap_or(V::Null),
                "midChange": mid_change,
                "quoteChanged": quote_changed.map(|x| json!(x)).unwrap_or(V::Null),
                "executable": false,
            }));
            history.insert(key, json!({"at": s["receivedAt"], "mid": mid,
                "bestBid": q["bestBid"], "bestAsk": q["bestAsk"]}));
        }
    }
    Ok(rows)
}

fn median(x: &[f64]) -> Option<f64> {
    if x.is_empty() { return None; }
    let mut v = x.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some((v[(v.len() - 1) / 2] + v[v.len() / 2]) / 2.0)
}

/// activityReport(rows)
pub fn activity_report(rows: &[V]) -> V {
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, V> = HashMap::new();
    for r in rows {
        let key = js_stringify(&r["exchangeId"]);
        if !groups.contains_key(&key) {
            groups.insert(key.clone(), json!({"exchangeId": r["exchangeId"], "marketId": r["marketId"],
                "observations": 0, "quoteChanges": 0, "pairedObservations": 0,
                "spreads": [], "absoluteMidChanges": []}));
            order.push(key.clone());
        }
        let g = groups.get_mut(&key).unwrap();
        g["observations"] = json!(g["observations"].as_i64().unwrap_or(0) + 1);
        if r["quoteChanged"] == V::Bool(true) {
            g["quoteChanges"] = json!(g["quoteChanges"].as_i64().unwrap_or(0) + 1);
        }
        if !r["spread"].is_null() {
            g["pairedObservations"] = json!(g["pairedObservations"].as_i64().unwrap_or(0) + 1);
            g["spreads"].as_array_mut().unwrap().push(r["spread"].clone());
        }
        if !r["midChange"].is_null() {
            g["absoluteMidChanges"].as_array_mut().unwrap()
                .push(json!(r["midChange"].as_f64().unwrap_or(f64::NAN).abs()));
        }
        g["lastCollectionAt"] = r["at"].clone();
    }
    let mut exchanges: Vec<V> = order.iter().map(|key| {
        let g = &groups[key];
        let spreads: Vec<f64> = g["spreads"].as_array().map(|a| a.iter().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect()).unwrap_or_default();
        let changes: Vec<f64> = g["absoluteMidChanges"].as_array().map(|a| a.iter().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect()).unwrap_or_default();
        let mean_change = if changes.is_empty() { V::Null } else {
            json!(changes.iter().sum::<f64>() / changes.len() as f64)
        };
        let mut o = g.as_object().cloned().unwrap_or_default();
        o.remove("spreads");
        o.remove("absoluteMidChanges");
        o.insert("medianSpread".into(), median(&spreads).map(V::from).unwrap_or(V::Null));
        o.insert("meanAbsoluteMidChange".into(), mean_change);
        V::Object(o)
    }).collect();
    exchanges.sort_by(|a, b| {
        b["quoteChanges"].as_i64().unwrap_or(0).cmp(&a["quoteChanges"].as_i64().unwrap_or(0))
            .then(a["medianSpread"].as_f64().unwrap_or(f64::INFINITY)
                .partial_cmp(&b["medianSpread"].as_f64().unwrap_or(f64::INFINITY))
                .unwrap_or(std::cmp::Ordering::Equal))
    });
    json!({
        "executable": false,
        "observationMeaning": "Collection polls; quote-source age and depth are unavailable. Quote changes are descriptive, not independent trades or profitability.",
        "exchanges": exchanges,
    })
}
