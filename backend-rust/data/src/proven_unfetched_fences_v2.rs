// execution/proven-unfetched-fences-v2.mjs — V1's serialized before-network
// block, OR a single fence plus an affirmative physical clock block proof
// naming this exact body at or after the fence.
//
// Port notes:
// - `o.orderId !== null` / `o.ack !== null` reject on a missing key too
//   (JS `undefined !== null`), so Option presence is compared, not the value.
// - `x?.length === n` mirrors the V1 port: arrays/strings use their own
//   length, objects may carry a numeric `length` key, everything else fails.
// - payloadHash(o.body) throws propagate as Err like the JS throw.
use baal_execution::core::payload_hash;
use baal_execution::proven_unfetched_fences_v1::proven_unfetched_fences_v1;
use baal_service_runtime::contract::parse_iso_ms;
use baal_service_runtime::json_text::js_truthy;
use serde_json::Value as V;

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn dparse(v: &V) -> f64 {
    v.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN)
}
fn len_eq(v: &V, n: usize) -> bool {
    match v {
        V::Array(a) => a.len() == n,
        V::String(s) => s.encode_utf16().count() == n,
        V::Object(m) => m.get("length").and_then(|x| x.as_f64()) == Some(n as f64),
        _ => false,
    }
}

/// provenUnfetchedFencesV2(order)
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
    let body_hash = payload_hash(get(order, "body"))?;
    let fence_at = dparse(get(order, "fences").get(0).map(|f| get(f, "at")).unwrap_or(&V::Null));
    let proofs = get(order, "physicalFetchBlockProofs");
    Ok(match proofs.as_array() {
        Some(a) => a.iter().any(|p| {
            get(p, "kind").as_str()
                == Some("affirmative_before_actual_fetch_clock_block_v1")
                && get(p, "physicalFetchReached") == &V::Bool(false)
                && get(p, "method").as_str() == Some("POST")
                && get(p, "endpoint").as_str() == Some("/orders")
                && get(p, "code").as_str()
                    == Some("BAAL_SCALED_PHYSICAL_FETCH_FENCE")
                && p.get("requestBodyHash") == Some(get(order, "bodyHash"))
                && get(p, "requestBodyHash").as_str() == Some(body_hash.as_str())
                && dparse(get(p, "at")) >= fence_at
        }),
        None => false,
    })
}
