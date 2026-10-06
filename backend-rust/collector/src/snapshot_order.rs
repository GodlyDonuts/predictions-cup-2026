// live-paper/snapshot-order.mjs
use crate::depth_scope::scoped_snapshots;
use baal_service_runtime::contract::parse_iso_ms;
use serde_json::Value as V;
pub const LEGACY_SNAPSHOT_ORDER: &str = "core_then_supplemental_v1";
pub const RECEIPT_SNAPSHOT_ORDER: &str = "actual_receipt_order_v1";
pub fn validate_snapshot_order(value: &str) -> Result<&str, String> {
    match value { LEGACY_SNAPSHOT_ORDER | RECEIPT_SNAPSHOT_ORDER => Ok(value), _ => Err("Unknown snapshot order policy".into()) }
}
/// orderedSnapshots(event, config, policy, fallbackAt)
pub fn ordered_snapshots(event: &V, config: &V, policy: &str, fallback_at: Option<&str>) -> Result<Vec<V>, String> {
    validate_snapshot_order(policy)?;
    let rows = scoped_snapshots(event, config)?;
    if policy == LEGACY_SNAPSHOT_ORDER { return Ok(rows); }
    let mut keyed: Vec<(usize, f64, V)> = Vec::with_capacity(rows.len());
    for (index, snapshot) in rows.into_iter().enumerate() {
        let s = if snapshot.get("skipped").and_then(V::as_bool).unwrap_or(false) {
            snapshot.get("at").and_then(V::as_str)
                .or_else(|| event.get("clockAt").and_then(V::as_str))
                .or(fallback_at)
        } else { snapshot.get("receivedAt").and_then(V::as_str) };
        let at = s.and_then(parse_iso_ms).ok_or("Invalid snapshot receipt clock")?;
        keyed.push((index, at, snapshot));
    }
    keyed.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap().then(a.0.cmp(&b.0)));
    Ok(keyed.into_iter().map(|(_, _, s)| s).collect())
}
