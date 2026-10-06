// live-paper/trade-tape-focus.mjs
use serde_json::{json, Value as V};
fn dedup(v: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    v.into_iter().filter(|x| seen.insert(x.clone())).collect()
}
/// tradeTapeFocus(universe, {activeBanks, riskMarketIds, recoveryWindows})
pub fn trade_tape_focus(universe: &V, opts: &V) -> Result<V, String> {
    let scope: std::collections::HashMap<&str, &str> = universe.get("exchanges").and_then(V::as_array).map(|a| {
        a.iter().filter_map(|x| Some((x.get("marketId").and_then(V::as_str)?, x.get("exchangeId").and_then(V::as_str)?))).collect()
    }).unwrap_or_default();
    let bad = || "TRADE_TAPE_FOCUS_SCOPE".to_string();
    let empty = Vec::new();
    let banks = opts.get("activeBanks").and_then(V::as_array).unwrap_or(&empty);
    let risk = opts.get("riskMarketIds").and_then(V::as_array).unwrap_or(&empty);
    let recovery = opts.get("recoveryWindows").and_then(V::as_array).unwrap_or(&empty);
    let mut active = Vec::new();
    for bank in banks {
        for leg in bank.get("legs").and_then(V::as_array).unwrap_or(&empty) {
            let mid = leg.get("marketId").and_then(V::as_str);
            let eid = leg.get("exchangeId").and_then(V::as_str);
            if mid.and_then(|m| scope.get(m).copied()) != eid { return Err(bad()); }
            active.push(mid.unwrap().to_string());
        }
    }
    for id in risk {
        let s = id.as_str();
        if !s.is_some_and(|s| scope.contains_key(s)) { return Err(bad()); }
    }
    for r in recovery {
        let mid = r.get("marketId").and_then(V::as_str);
        if mid.and_then(|m| scope.get(m).copied()) != r.get("exchangeId").and_then(V::as_str) { return Err(bad()); }
    }
    let risk_ids: Vec<String> = risk.iter().filter_map(|x| x.as_str().map(String::from)).collect();
    let priority = if !risk_ids.is_empty() { dedup(risk_ids) } else { dedup(active.clone()) };
    let mut all = active;
    all.extend(risk.iter().filter_map(|x| x.as_str().map(String::from)));
    all.extend(recovery.iter().filter_map(|r| r.get("marketId").and_then(V::as_str).map(String::from)));
    let mut market_ids = dedup(all);
    if market_ids.is_empty() {
        market_ids = ["363", "297", "381", "382"].iter().filter(|id| scope.contains_key(**id)).map(|s| s.to_string()).collect();
    }
    if market_ids.is_empty() { return Err(bad()); }
    Ok(json!({"marketIds": market_ids, "priorityMarketIds": priority,
        "recoveryWindows": opts.get("recoveryWindows").cloned().unwrap_or(json!([]))}))
}
