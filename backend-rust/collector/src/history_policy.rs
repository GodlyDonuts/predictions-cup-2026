// live-paper/history-policy.mjs
use serde_json::Value as V;
use serde_json::map::Map;
pub const LEGACY: &str = "global-reset-v1";
pub const ISOLATED: &str = "market-isolated-v2";
pub fn history_policy(value: &str) -> Result<&'static str, String> {
    match value { LEGACY => Ok(LEGACY), ISOLATED => Ok(ISOLATED), _ => Err("Unknown ML history policy".into()) }
}
/// resetHistory(histories, marketId, policy)
pub fn reset_history(histories: &mut Map<String, V>, market_id: Option<&str>, policy: &str) -> Result<(), String> {
    history_policy(policy)?;
    let keys: Vec<String> = histories.keys().cloned().collect();
    for instrument in keys {
        let rows = histories.get(&instrument).and_then(V::as_array);
        let row = rows.and_then(|r| r.last());
        let market = row.and_then(|r| r.get("marketId").or_else(|| r.get("book").and_then(|b| b.get("marketId")))).and_then(V::as_str);
        if policy == LEGACY || market_id.is_none() || market == market_id {
            histories.remove(&instrument);
        }
    }
    Ok(())
}
