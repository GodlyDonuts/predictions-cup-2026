// execution/proven-unfetched-fences-v1.mjs — a prepared intent is proven
// never to have reached the physical network only when the last serialized
// before-network block names this exact body at or after the fence.
//
// Port notes:
// - `payloadHash(order.body)` throws propagate as Err; a missing/null
//   blockedNetworkProofs entry yields false.
// - Date.parse NaN comparisons return false exactly like JavaScript.
use baal_service_runtime::contract::parse_iso_ms;
use serde_json::Value as V;

use crate::core::payload_hash;

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}

fn dparse(v: &V) -> f64 {
    v.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN)
}

// `v?.length === 0` in JS: arrays/strings use their own length; objects may
// carry a numeric `length` key equal to 0; every other value yields
// `undefined !== 0` (reject). JSON has no undefined/NaN distinction here.
fn len_eq_zero(v: &V) -> bool {
    match v {
        V::Array(a) => a.is_empty(),
        V::String(s) => s.encode_utf16().count() == 0,
        V::Object(m) => m.get("length").is_some_and(|x| x.as_f64() == Some(0.0)),
        _ => false,
    }
}

/// provenUnfetchedFencesV1(order)
pub fn proven_unfetched_fences_v1(order: &V) -> Result<bool, String> {
    let fences = get(order, "fences");
    if !fences.is_array()
        || fences.as_array().unwrap().len() > 1
        || !len_eq_zero(get(order, "fetchStarts"))
        || !len_eq_zero(get(order, "receipts"))
    {
        return Ok(false);
    }
    if fences.as_array().unwrap().is_empty() {
        return Ok(true);
    }
    let p = order
        .get("blockedNetworkProofs")
        .and_then(V::as_array)
        .and_then(|a| a.last().cloned())
        .unwrap_or(V::Null);
    let f = &fences.as_array().unwrap()[0];
    Ok(
        p.get("kind").and_then(V::as_str) == Some("serialized_client_before_network_block_v1")
            && p.get("method").and_then(V::as_str) == Some("POST")
            && p.get("endpoint").and_then(V::as_str) == Some("/orders")
            && p.get("physicalFetchReached") == Some(&V::Bool(false))
            && p.get("requestBodyHash") == Some(get(order, "bodyHash")).filter(|v| !v.is_null())
            && p.get("requestBodyHash").and_then(V::as_str)
                == Some(payload_hash(get(order, "body"))?.as_str())
            && dparse(p.get("at").unwrap_or(&V::Null)) >= dparse(f.get("at").unwrap_or(&V::Null)),
    )
}
