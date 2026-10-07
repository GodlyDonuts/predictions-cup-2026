// execution/known-order-read-capacity-v1.mjs — wait before the first fresh
// receipt; never consume requests to obtain capacity.
use serde_json::{json, Value as V};

/// waitKnownOrderReadCapacityV1({orderId,hasReadHeadroom,canRead,now,pause})
///
/// async -> sync: `pause` is the injected JS `setTimeout` sleep (callers pass
/// `baal_service_runtime::wait::thread_sleep` for the default). The JS
/// `ready !== false -> KNOWN_CAPACITY_CALLBACK` branch is unreachable here
/// because the headroom callback returns a real bool; a thrown callback error
/// propagates exactly as it would in JS.
pub fn wait_known_order_read_capacity_v1(
    order_id: &V,
    has_read_headroom: &dyn Fn(i64) -> Result<bool, String>,
    can_read: &dyn Fn() -> bool,
    now: &dyn Fn() -> f64,
    pause: &dyn Fn(f64),
) -> Result<V, String> {
    let id_ok = order_id.as_str().is_some_and(|s| {
        s.as_bytes().first().is_some_and(|b| (b'1'..=b'9').contains(b))
            && s.bytes().all(|b| b.is_ascii_digit())
    }) && order_id
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .is_some_and(|n| n.is_finite() && n.fract() == 0.0 && n <= 9_007_199_254_740_991.0);
    if !id_ok {
        return Err("KNOWN_CAPACITY_ORDER".into());
    }
    let start = now();
    let mut waits: i64 = 0;
    loop {
        if !can_read() {
            return Err("KNOWN_CAPACITY_STOPPED".into());
        }
        match has_read_headroom(11) {
            Ok(true) => {
                return Ok(json!({
                    "waits": waits,
                    "waitedMs": now() - start,
                    "readsReserved": 11,
                    "networkRequests": 0,
                }))
            }
            Ok(false) => {}
            Err(e) => return Err(e),
        }
        if now() - start >= 60000.0 {
            return Err("KNOWN_CAPACITY_TIMEOUT".into());
        }
        pause(250.0);
        waits += 1;
    }
}
