// execution/proven-unfetched-fences-v2.mjs — v1's serialized before-network
// block proof, plus an affirmative monotonic-clock block recorded before the
// physical fetch expression for this exact body.
//
// Port notes:
// - `payloadHash(o.body)` throws propagate as Err, exactly like v1.
// - `o?.physicalFetchBlockProofs?.some(...)` over a non-array yields false.
// - Date.parse NaN comparisons return false exactly like JavaScript.
use baal_service_runtime::contract::parse_iso_ms;
use serde_json::Value as V;

use crate::core::payload_hash;
use crate::proven_unfetched_fences_v1::proven_unfetched_fences_v1;

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}

// Same `.length === 0` helper as proven_unfetched_fences_v1 (private there):
// arrays empty, strings empty, objects carrying `length` === 0.
fn len_eq_zero(v: &V) -> bool {
    match v {
        V::Array(a) => a.is_empty(),
        V::String(s) => s.encode_utf16().count() == 0,
        V::Object(m) => m.get("length").is_some_and(|x| x.as_f64() == Some(0.0)),
        _ => false,
    }
}

fn dparse(v: &V) -> f64 {
    v.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN)
}

/// provenUnfetchedFencesV2(o)
pub fn proven_unfetched_fences_v2(o: &V) -> Result<bool, String> {
    if proven_unfetched_fences_v1(o)? {
        return Ok(true);
    }
    if o.is_null()
        || !get(o, "orderId").is_null()
        || !get(o, "ack").is_null()
        || !len_eq_zero(get(o, "fetchStarts"))
        || !len_eq_zero(get(o, "receipts"))
        || !(get(o, "fences").is_array() && get(o, "fences").as_array().unwrap().len() == 1)
    {
        return Ok(false);
    }
    let proofs = match o.get("physicalFetchBlockProofs").and_then(V::as_array) {
        Some(a) => a,
        None => return Ok(false),
    };
    for p in proofs {
        if get(p, "kind").as_str() == Some("affirmative_before_actual_fetch_clock_block_v1")
            && get(p, "physicalFetchReached") == &V::Bool(false)
            && get(p, "method").as_str() == Some("POST")
            && get(p, "endpoint").as_str() == Some("/orders")
            && get(p, "code").as_str() == Some("BAAL_SCALED_PHYSICAL_FETCH_FENCE")
            && get(p, "requestBodyHash") == get(o, "bodyHash")
            && get(p, "requestBodyHash").as_str()
                == Some(payload_hash(get(o, "body"))?.as_str())
            && dparse(get(p, "at"))
                >= dparse(&get(o, "fences").as_array().unwrap()[0].get("at").cloned().unwrap_or(V::Null))
        {
            return Ok(true);
        }
    }
    Ok(false)
}
