// live-paper/cycle-wait.mjs
use serde_json::{json, Value as V};
fn safe_int(v: &V) -> Option<i64> { v.as_i64() }
fn is_numeric_id(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && s.as_bytes()[0] != b'0'
}
/// collectionCycleWait({cycleStartedAt, now, cycleIntervalMs, managedMarketIds})
/// Args as JSON so callers pass the raw config values unchanged.
pub fn collection_cycle_wait(args: &V) -> Result<V, String> {
    let cycle_started_at = args.get("cycleStartedAt").and_then(safe_int);
    let now = args.get("now").and_then(safe_int);
    let interval = args.get("cycleIntervalMs").and_then(safe_int);
    let managed = args.get("managedMarketIds").and_then(V::as_array).cloned().unwrap_or_default();
    let ok = match (cycle_started_at, now, interval) {
        (Some(cs), Some(n), Some(i)) => n >= cs && i >= 1000,
        _ => false,
    } && managed.iter().all(|x| {
        x.as_str().is_some_and(|s| is_numeric_id(s) && s.parse::<i64>().is_ok())
    });
    if !ok { return Err("INVALID_COLLECTION_CYCLE_CLOCK_OR_RISK".into()); }
    let owned = !managed.is_empty();
    Ok(json!({
        "mode": if owned { "owned_risk" } else { "normal" },
        "delayMs": if owned { 0 } else { std::cmp::max(1000i64, interval.unwrap() - (now.unwrap() - cycle_started_at.unwrap())) },
    }))
}
