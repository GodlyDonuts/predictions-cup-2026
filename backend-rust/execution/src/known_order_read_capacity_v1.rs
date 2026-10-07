// execution/known-order-read-capacity-v1.mjs — wait before the first fresh
// receipt; never consume requests to obtain capacity.
//
// Port notes:
// - async -> sync: `pause` is a blocking callback; callers pass
//   baal_service_runtime::wait::thread_sleep, mirroring the JS
//   `ms => new Promise(r=>setTimeout(r,ms))` default.
// - `hasReadHeadroom(11)` returning a non-boolean has no representation on the
//   Result<bool,String> channel — Err propagates exactly like the JS throw,
//   so the KNOWN_CAPACITY_CALLBACK arm is unreachable here.
use serde_json::{json, Value as V};

/// waitKnownOrderReadCapacityV1({orderId,hasReadHeadroom,canRead,now,pause})
pub fn wait_known_order_read_capacity_v1(
    order_id: &str,
    has_read_headroom: &dyn Fn(i64) -> Result<bool, String>,
    can_read: &dyn Fn() -> bool,
    now: &dyn Fn() -> f64,
    pause: &dyn Fn(f64),
) -> Result<V, String> {
    // typeof orderId === 'string' && /^[1-9]\d*$/ && Number.isSafeInteger(Number(orderId))
    let digit_ok = {
        let mut b = order_id.bytes();
        matches!(b.next(), Some(c) if (b'1'..=b'9').contains(&c))
            && b.all(|c| c.is_ascii_digit())
    };
    let safe_ok = order_id
        .parse::<u64>()
        .map(|n| n <= 9_007_199_254_740_991)
        .unwrap_or(false);
    if !digit_ok || !safe_ok {
        return Err("KNOWN_CAPACITY_ORDER".into());
    }
    let start = now();
    let mut waits = 0u64;
    loop {
        // canRead() !== true -> stopped (any non-true value).
        if !can_read() {
            return Err("KNOWN_CAPACITY_STOPPED".into());
        }
        let ready = has_read_headroom(11)?;
        if ready {
            return Ok(json!({
                "waits": waits,
                "waitedMs": now() - start,
                "readsReserved": 11,
                "networkRequests": 0,
            }));
        }
        if now() - start >= 60000.0 {
            return Err("KNOWN_CAPACITY_TIMEOUT".into());
        }
        pause(250.0);
        waits += 1;
    }
}
