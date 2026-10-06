// live-paper/stable-maker-read-focus.mjs — mirrors coupled_bulk (10min group).
use baal_service_runtime::contract::parse_iso_ms;
use serde_json::{json, Value as V};
use std::collections::{HashMap, HashSet};
/// focusStableMakerReads(plan, focus, universe, {now, protectedMarketIds, excludedMarketIds})
pub fn focus_stable_maker_reads(plan: &V, focus: &V, universe: &V, now: f64,
    protected_market_ids: &[String], excluded_market_ids: &[String]) -> Result<V, String> {
    let candidates = focus.get("candidateMarketIds").and_then(V::as_array);
    if focus.is_null() || focus.get("failed").and_then(V::as_bool).unwrap_or(false)
        || !candidates.is_some_and(|c| !c.is_empty()) {
        return Ok(plan.clone());
    }
    let scope: HashMap<&str, &V> = universe["exchanges"].as_array().map(|a| {
        a.iter().filter_map(|x| Some((x["marketId"].as_str()?, x))).collect()
    }).unwrap_or_default();
    let protected_set: HashSet<&str> = protected_market_ids.iter().map(String::as_str).collect();
    let excluded: HashSet<&str> = excluded_market_ids.iter().map(String::as_str).collect();
    let available: Vec<&str> = candidates.unwrap().iter().filter_map(|c| c.as_str())
        .filter(|id| scope.contains_key(id) && !protected_set.contains(id) && !excluded.contains(id)).collect();
    let reads = plan["reads"].as_array().cloned().unwrap_or_default();
    let slots: Vec<usize> = reads.iter().enumerate()
        .filter(|(_, r)| !protected_set.contains(r["marketId"].as_str().unwrap_or(""))).map(|(i, _)| i).collect();
    if slots.is_empty() || available.is_empty() { return Ok(plan.clone()); }
    let cutoff = focus.get("cutoff").and_then(V::as_str).and_then(parse_iso_ms).ok_or("STABLE_READ_CLOCK")?;
    if !now.is_finite() || now < cutoff { return Err("STABLE_READ_CLOCK".into()); }
    let offset = (((now - cutoff) / 600000.0).floor() as usize * slots.len()) % available.len();
    let selected: Vec<&str> = (0..slots.len().min(available.len())).map(|i| available[(offset + i) % available.len()]).collect();
    let mut out_reads = reads.clone();
    for (i, sel) in selected.iter().enumerate() {
        let spec = scope[sel];
        out_reads[slots[i]] = json!({"marketId": spec["marketId"], "exchangeId": spec["exchangeId"],
            "option": "YES", "path": "supplementalDepth", "reason": "stable_paper_history"});
    }
    let mut seen = HashSet::new();
    for i in 0..out_reads.len() {
        let mid = out_reads[i]["marketId"].as_str().unwrap_or("").to_string();
        if seen.contains(&mid) {
            let original = reads.iter().find(|r| {
                let m = r["marketId"].as_str().unwrap_or("");
                !seen.contains(m) && !selected.contains(&m)
            }).ok_or("STABLE_READ_DUPLICATE")?;
            out_reads[i] = original.clone();
        }
        seen.insert(out_reads[i]["marketId"].as_str().unwrap_or("").to_string());
    }
    let mut out = plan.clone();
    let o = out.as_object_mut().unwrap();
    o.insert("reads".into(), json!(out_reads));
    o.insert("entryMarketIds".into(), json!(selected));
    o.insert("stableMakerReadFocus".into(), json!({"groupMs": 600000,
        "selectedMarketIds": selected, "protectedMarketIds": protected_set.iter().collect::<Vec<_>>(),
        "readsAdded": 0, "executionAuthority": false}));
    Ok(out)
}
