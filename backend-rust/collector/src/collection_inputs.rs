// live-paper/collection-inputs.mjs — collector risk membership inputs.
// A research wallet never contributes execution leads.

use crate::dip_shadow::dip_depth_plan;
use crate::history_allocation::history_allocation;
use crate::model::digest;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::history::historical_reference;
use baal_service_runtime::json_text::js_truthy;
use serde_json::{json, Value as V};

/// JS `===` for JSON values: scalars by value, objects/arrays never equal.
fn strict_eq(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Null, V::Null) => true,
        (V::Bool(x), V::Bool(y)) => x == y,
        (V::Number(x), V::Number(y)) => x.as_f64() == y.as_f64(),
        (V::String(x), V::String(y)) => x == y,
        _ => false,
    }
}
fn to_key(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Null => "null".into(),
        V::Bool(b) => b.to_string(),
        V::Number(n) => {
            let f = n.as_f64().unwrap_or(f64::NAN);
            if f.is_finite() && f.fract() == 0.0 { (f as i64).to_string() } else { n.to_string() }
        }
        V::Array(a) => a.iter().map(|x| match x { V::Null => String::new(), other => to_key(other) })
            .collect::<Vec<_>>().join(","),
        V::Object(_) => "[object Object]".into(),
    }
}
fn push_unique(list: &mut Vec<V>, v: V) {
    if !list.iter().any(|x| strict_eq(x, &v)) { list.push(v); }
}

// Risk membership only: a research wallet never contributes execution leads.
pub fn shock_collection_risk(state: &V, bank: &V, universe: &V) -> Result<V, String> {
    if !(state["bankHash"].as_str() == Some(digest(bank).as_str())
        && bank["name"].as_str() == Some("shock-rebound-shadow-v1")
        && strict_eq(&bank["config"]["tournamentId"], &universe["tournamentId"])
        && bank["markets"].is_array()) {
        return Err("Shock collection identity mismatch".into());
    }
    let mut official: std::collections::HashMap<String, V> = Default::default();
    for x in universe["exchanges"].as_array().cloned().unwrap_or_default() {
        official.insert(to_key(&x["marketId"]), x);
    }
    let mut members: std::collections::HashMap<String, V> = Default::default();
    for m in bank["markets"].as_array().cloned().unwrap_or_default() {
        let key = to_key(&m["marketId"]);
        let o = official.get(&key);
        if o.is_none() || !strict_eq(&o.unwrap()["exchangeId"], &m["exchangeId"])
            || !strict_eq(&o.unwrap()["option"], &json!("YES")) || members.contains_key(&key) {
            return Err("Shock collection scope mismatch".into());
        }
        members.insert(key, m["exchangeId"].clone());
    }
    let st = &state["wallet"]["strategies"]["mean_reversion"];
    let collect = |rows: &V| -> Result<Vec<V>, String> {
        if !js_truthy(Some(rows)) || !rows.is_object() {
            return Err("Invalid shock holdings".into());
        }
        let mut out: Vec<V> = Vec::new();
        for id in rows.as_object().map(|o| o.keys().cloned().collect::<Vec<String>>()).unwrap_or_default() {
            let b = &state["wallet"]["books"][&id];
            let exchange = if js_truthy(Some(b)) {
                members.get(&to_key(&b["marketId"])).cloned()
            } else { None };
            match exchange {
                None => return Err("Unmapped shock managed market".into()),
                Some(ex) => {
                    if !strict_eq(&b["exchangeId"], &ex)
                        || !strict_eq(&json!(id.clone()), &json!(format!("{}:YES", to_key(&ex)))) {
                        return Err("Unmapped shock managed market".into());
                    }
                }
            }
            push_unique(&mut out, b["marketId"].clone());
        }
        Ok(out)
    };
    Ok(json!({
        "heldMarketIds": collect(&st["positions"])?,
        "pendingMarketIds": collect(&st["pending"])?,
    }))
}

// A stopped worker may still hold inventory; keep servicing it while suppressing
// new candidates. Source identity mismatches must not silently hide positions.
pub fn collection_inputs(sources: &[V], universe: &V, coverage: Option<&V>, now: f64) -> Result<V, String> {
    let valid: Vec<V> = universe["exchanges"].as_array().cloned().unwrap_or_default()
        .iter().map(|x| x["marketId"].clone()).collect();
    let mut held: Vec<V> = Vec::new();
    let mut pending: Vec<V> = Vec::new();
    let mut candidates: Vec<V> = Vec::new();
    let mut usable: Vec<V> = Vec::new();
    for src in sources {
        let state = &src["state"];
        let bank = &src["bank"];
        let healthy = js_truthy(src.get("healthy"));
        if !(state["bankHash"].as_str() == Some(digest(bank).as_str())
            && strict_eq(&bank["universe"]["hash"], &universe["hash"])) {
            return Err("Collection worker identity mismatch".into());
        }
        let st = &state["wallet"]["strategies"]["mean_reversion"];
        if st.is_null() { return Err("TypeError: Cannot read properties of undefined".into()); }
        for (rows, is_held) in [(&st["positions"], true), (&st["pending"], false)] {
            if rows.is_null() {
                return Err("TypeError: Cannot convert undefined or null to object".into());
            }
            for id in rows.as_object().map(|o| o.keys().cloned().collect::<Vec<String>>()).unwrap_or_default() {
                if !state["wallet"]["books"].is_object() {
                    return Err("TypeError: Cannot read properties of undefined".into());
                }
                let market = state["wallet"]["books"][&id].get("marketId").cloned().unwrap_or(V::Null);
                if !valid.iter().any(|v| strict_eq(v, &market)) {
                    return Err("Unmapped managed market".into());
                }
                if is_held { push_unique(&mut held, market); } else { push_unique(&mut pending, market); }
            }
        }
        if !healthy { continue; }
        let cov = coverage.cloned().unwrap_or(V::Null);
        for lead in dip_depth_plan(state, bank, &cov, now)?
            .as_array().cloned().unwrap_or_default() {
            if lead["reason"].as_str() == Some("historical_discount_lead") {
                push_unique(&mut candidates, lead["marketId"].clone());
            }
        }
        for spec in universe["exchanges"].as_array().cloned().unwrap_or_default() {
            let eid = spec["exchangeId"].as_str().unwrap_or("");
            let books = state["bookHistory"].get(eid).and_then(|b| b.as_array()).cloned().unwrap_or_default();
            let reference = historical_reference(state["histories"].get(eid), &books,
                &ms_to_iso(now), &bank["policy"])?;
            if js_truthy(reference.get("ready")) {
                push_unique(&mut usable, spec["marketId"].clone());
            }
        }
    }
    Ok(json!({
        "heldMarketIds": held,
        "pendingMarketIds": pending,
        "candidateMarketIds": candidates,
        "usableMarketIds": usable,
        "historyAllocation": history_allocation(sources, universe, coverage, now as i64)?,
    }))
}
