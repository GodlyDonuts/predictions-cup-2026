// live-paper/depth-scope.mjs
use serde_json::Value as V;
fn is_numeric_id(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && s.as_bytes()[0] != b'0'
}
fn str_list(v: Option<&V>) -> Vec<String> {
    v.and_then(V::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default()
}
/// validateDepthPlan(plan, c, universe)
pub fn validate_depth_plan(plan: &V, c: &V, universe: &V) -> Result<V, String> {
    let market_ids: Vec<String> = str_list(plan.get("marketIds"));
    let cfg_market_ids: Vec<String> = str_list(c.get("marketIds"));
    let unique: std::collections::BTreeSet<&String> = market_ids.iter().collect();
    if plan.get("tournamentId") != c.get("tournamentId")
        || !plan.get("marketIds").is_some_and(V::is_array)
        || market_ids.len() > 2
        || unique.len() != market_ids.len()
    {
        return Err("Invalid supplemental depth plan".into());
    }
    let exchanges = universe.get("exchanges").and_then(V::as_array).cloned().unwrap_or_default();
    for id in &market_ids {
        let matching: Vec<&V> = exchanges.iter().filter(|x| x.get("marketId").and_then(V::as_str) == Some(id.as_str())).collect();
        if !is_numeric_id(id)
            || cfg_market_ids.contains(id)
            || matching.len() != 1
            || matching[0].get("option").and_then(V::as_str) != Some("YES")
        {
            return Err("Unsupported supplemental depth market".into());
        }
    }
    Ok(plan.clone())
}
/// scopedSnapshots(event, c)
pub fn scoped_snapshots(event: &V, c: &V) -> Result<Vec<V>, String> {
    let core = event.get("sourceSnapshots").and_then(V::as_array).cloned().unwrap_or_default();
    let extra = event.get("supplementalDepth").and_then(V::as_array).cloned().unwrap_or_default();
    if !event.get("sourceSnapshots").is_some_and(V::is_array) || !event.get("supplementalDepth").is_some_and(V::is_array) {
        return Err("Invalid depth capture arrays".into());
    }
    let market_ids = str_list(c.get("marketIds"));
    Ok(core.into_iter().chain(extra).filter(|s| {
        let id = if s.get("skipped").is_some_and(|x| x.is_boolean() && x.as_bool().unwrap()) {
            s.get("marketId").and_then(V::as_str).map(String::from)
        } else {
            s.get("endpoint").and_then(V::as_str).and_then(|e| {
                e.strip_prefix("/markets/").and_then(|rest| {
                    let digits: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
                    if rest[digits.len()..].starts_with("/orderbook?") && !digits.is_empty() { Some(digits) } else { None }
                })
            })
        };
        id.is_some_and(|i| market_ids.contains(&i))
    }).collect())
}
