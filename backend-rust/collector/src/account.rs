// live-paper/account.mjs — accountView aggregator (row validators live in
// baal_service_runtime::market::{account_positions,account_orders,...}).
use baal_service_runtime::contract::parse_iso_ms;
use serde_json::{json, Value as V};
const SECTIONS: [&str; 4] = ["positions", "orders", "fills", "pnl"];
/// accountView(cache, collectorStatus, now, identities)
pub fn account_view(cache: &V, collector_status: &str, now: &str, identities: &V) -> V {
    let clock = parse_iso_ms(now);
    let max_age_ms = 300000.0f64;
    let mut sections = serde_json::Map::new();
    for k in SECTIONS {
        let at = cache.get(k).and_then(|s| s.get("at")).cloned().unwrap_or(V::Null);
        let age = at.as_str().and_then(parse_iso_ms).zip(clock).map(|(a, c)| c - a);
        let valid = age.is_some_and(|a| a.is_finite() && a >= 0.0);
        let status = if at.is_null() { "missing" }
            else if !valid { "invalid_clock" }
            else if cache.get("errors").and_then(|e| e.get(k)).is_some_and(|e| !e.is_null()) { "degraded" }
            else if age.unwrap() > max_age_ms { "stale" } else { "fresh" };
        sections.insert(k.into(), json!({"at": at, "ageMs": if valid { json!(age.unwrap()) } else { V::Null }, "status": status}));
    }
    let ages: V = SECTIONS.iter().map(|k| (k.to_string(), sections.get(*k).unwrap()["ageMs"].clone())).collect::<serde_json::Map<_, _>>().into();
    let list = |st: &str| SECTIONS.iter().filter(|k| sections.get(**k).unwrap()["status"] == st).map(|s| s.to_string()).collect::<Vec<_>>();
    let missing = list("missing");
    let invalid_clock = list("invalid_clock");
    let stale = list("stale");
    let errors: Vec<String> = cache.get("errors").and_then(V::as_object).map(|e| e.keys().cloned().collect()).unwrap_or_default();
    let status = if !invalid_clock.is_empty() || clock.is_none() { "invalid_clock" }
        else if missing.len() == SECTIONS.len() { "unavailable" }
        else if !missing.is_empty() { "partial" }
        else if !errors.is_empty() || collector_status != "running" { "degraded" }
        else if !stale.is_empty() { "stale" } else { "fresh" };
    let health = json!({"status": status, "sections": sections, "missingSections": missing,
        "invalidClockSections": invalid_clock, "staleSections": stale, "errorSections": errors, "maxAgeMs": max_age_ms});
    // Labels: exchangeId -> market title from positions, else identities[marketId].title.
    let mut labels: std::collections::HashMap<String, V> = std::collections::HashMap::new();
    for x in cache.get("positions").and_then(|p| p.get("rows")).and_then(V::as_array).into_iter().flatten() {
        if let Some(e) = x.get("exchangeId").and_then(V::as_str) {
            labels.insert(e.to_string(), x.get("label").cloned().unwrap_or(V::Null));
        }
    }
    let label = |x: &V| -> V {
        let eid = x.get("exchangeId").and_then(V::as_str).unwrap_or("");
        if labels.get(eid).is_some_and(|l| !l.is_null()) { return labels[eid].clone(); }
        let mid = x.get("marketId").and_then(V::as_str).unwrap_or("");
        identities.get(mid).and_then(|i| i.get("title")).cloned().filter(|t| !t.is_null())
            .unwrap_or_else(|| json!(format!("Exchange {}", eid)))
    };
    let mut order_map: std::collections::HashMap<i64, &V> = std::collections::HashMap::new();
    for src in ["orderHistory", "orders"] {
        for x in cache.get(src).and_then(|s| s.get("rows")).and_then(V::as_array).into_iter().flatten() {
            if let Some(id) = x.get("id").and_then(V::as_i64) { order_map.insert(id, x); }
        }
    }
    let with_label = |k: &str| -> Vec<V> {
        cache.get(k).and_then(|s| s.get("rows")).and_then(V::as_array).into_iter().flatten()
            .map(|x| { let mut o = x.clone(); o["label"] = label(x); o }).collect()
    };
    let positions = with_label("positions");
    let pending = with_label("orders");
    let trades: Vec<V> = cache.get("fills").and_then(|s| s.get("rows")).and_then(V::as_array).into_iter().flatten()
        .map(|x| {
            let mut o = x.clone(); o["label"] = label(x);
            o["action"] = x.get("orderId").and_then(V::as_i64).and_then(|id| order_map.get(&id))
                .and_then(|o| o.get("action")).cloned().unwrap_or(V::Null);
            o
        }).collect();
    json!({"mode": "official_account",
        "available": SECTIONS.iter().any(|k| cache.get(*k).is_some_and(|s| !s.is_null())),
        "updatedAt": now, "collectorStatus": collector_status, "ages": ages, "health": health,
        "sourceTimes": SECTIONS.iter().map(|k| (k.to_string(), sections.get(*k).unwrap()["at"].clone())).collect::<serde_json::Map<_,_>>(),
        "positions": positions, "pending": pending, "trades": trades,
        "metrics": cache.get("pnl").cloned().unwrap_or(V::Null),
        "fillHistory": {"displayed": cache.get("fills").and_then(|f| f.get("rows")).and_then(V::as_array).map(|r| r.len()).unwrap_or(0),
            "hasMore": cache.get("fills").and_then(|f| f.get("hasMore")).cloned().unwrap_or(V::Null)},
        "errors": cache.get("errors").cloned().unwrap_or(json!({}))})
}
