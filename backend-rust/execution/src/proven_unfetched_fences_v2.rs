// execution/proven-unfetched-fences-v2.mjs — v1's before-network proof plus
// the affirmative physical-fetch clock block: a prepared intent is proven
// never fetched when a recorded pre-physical rejection names this exact body
// at or after its single fence.
//
// Port notes:
// - provenUnfetchedFencesV1 throws propagate as Err first, exactly like JS.
// - `?.length!==0`/`!==1` accepts arrays, strings and objects carrying a
//   numeric `length` key — same helper semantics as the v1 port.
// - `o.physicalFetchBlockProofs?.some(...)===true`: missing/non-array -> false
//   (the JS TypeError path for non-array objects is not reproduced, matching
//   the v1 port's treatment of blockedNetworkProofs).
// - `p.requestBodyHash===o.bodyHash` is strict: absent on both sides equals,
//   like JS `undefined===undefined`.
use baal_service_runtime::contract::parse_iso_ms;
use baal_service_runtime::json_text::js_truthy;
use serde_json::Value as V;

use crate::core::payload_hash;
use crate::proven_unfetched_fences_v1::proven_unfetched_fences_v1;

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}

fn dparse(v: &V) -> f64 {
    v.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN)
}

// `v?.length === n` in JS: arrays/strings use their own length; objects may
// carry a numeric `length` key; every other value yields undefined !== n.
fn len_eq(v: &V, n: usize) -> bool {
    match v {
        V::Array(a) => a.len() == n,
        V::String(s) => s.encode_utf16().count() == n,
        V::Object(m) => m
            .get("length")
            .and_then(V::as_f64)
            .is_some_and(|x| x == n as f64),
        _ => false,
    }
}

/// provenUnfetchedFencesV2(o)
pub fn proven_unfetched_fences_v2(order: &V) -> Result<bool, String> {
    if proven_unfetched_fences_v1(order)? {
        return Ok(true);
    }
    if !js_truthy(Some(order))
        || order.get("orderId") != Some(&V::Null)
        || order.get("ack") != Some(&V::Null)
        || !len_eq(get(order, "fetchStarts"), 0)
        || !len_eq(get(order, "receipts"), 0)
        || !len_eq(get(order, "fences"), 1)
    {
        return Ok(false);
    }
    let fence_at = get(order, "fences")
        .as_array()
        .and_then(|a| a.first())
        .map(|f| dparse(get(f, "at")))
        .unwrap_or(f64::NAN);
    let body_hash = get(order, "bodyHash");
    let expected = payload_hash(get(order, "body"))?;
    Ok(order
        .get("physicalFetchBlockProofs")
        .and_then(V::as_array)
        .is_some_and(|proofs| {
            proofs.iter().any(|p| {
                get(p, "kind").as_str() == Some("affirmative_before_actual_fetch_clock_block_v1")
                    && get(p, "physicalFetchReached") == &V::Bool(false)
                    && get(p, "method").as_str() == Some("POST")
                    && get(p, "endpoint").as_str() == Some("/orders")
                    && get(p, "code").as_str() == Some("BAAL_SCALED_PHYSICAL_FETCH_FENCE")
                    && get(p, "requestBodyHash") == body_hash
                    && get(p, "requestBodyHash").as_str() == Some(expected.as_str())
                    && dparse(get(p, "at")) >= fence_at
            })
        }))
}
