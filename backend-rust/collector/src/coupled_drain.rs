// live-paper/coupled-drain.mjs — host wrapper; port as free functions so the
// caller (a host trait object) keeps its own dispatch.
use serde_json::{json, Value as V};
pub const DRAIN_MODE: &str = "drain_existing_inventory_and_quotes";
pub const DRAIN_BOUNDARY: &str = "operator_drain_unowned_entry_features";
/// wrapCoupledDrain status() overlay.
pub fn drain_status(status: V, draining: bool) -> V {
    if !draining { return status; }
    let mut s = status;
    s.as_object_mut().map(|o| o.insert("operatorMode".into(), json!(DRAIN_MODE)));
    s
}
/// wrapCoupledDrain focus() overlay.
pub fn drain_focus(focus: V, draining: bool) -> V {
    if !draining { return focus; }
    let mut f = focus;
    if let Some(o) = f.as_object_mut() {
        o.insert("candidateMarketIds".into(), json!([]));
        o.insert("operatorMode".into(), json!(DRAIN_MODE));
    }
    f
}
/// Whether onBook must emit the boundary: draining and no wallet holds marketId.
pub fn drain_boundary_needed(status_wallets: &V, market_id: &str, draining: bool) -> bool {
    if !draining { return false; }
    let owned = status_wallets.as_object().map(|w| w.values().any(|w| {
        w.get("positions").and_then(V::as_array).map(|p| p.iter().any(|p| {
            p.get("marketId").and_then(V::as_str) == Some(market_id)
                && p.get("quantity").and_then(V::as_f64).unwrap_or(0.0) > 0.0
        })).unwrap_or(false)
    })).unwrap_or(false);
    !owned
}
