// execution/proven-unfetched-fences-v1.mjs — true only for an intent that
// provably never reached a physical fetch: zero recorded fetches and zero
// receipts, at most one fence, and (when fenced) a sealed serialized-client
// before-network block proof bound to this exact request body.
use baal_service_runtime::contract::parse_iso_ms;
use serde_json::Value as V;

use crate::core::payload_hash;

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}

fn sget<'a>(v: &'a V, k: &str) -> Option<&'a str> {
    v.get(k).and_then(V::as_str)
}

fn dparse(v: &V) -> f64 {
    v.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN)
}

/// provenUnfetchedFencesV1(order) — payloadHash(order.body) propagates like
/// the JS throw if the body cannot be hashed.
pub fn proven_unfetched_fences_v1(order: &V) -> Result<bool, String> {
    let fences = match get(order, "fences").as_array() {
        Some(a) if a.len() <= 1 => a.clone(),
        _ => return Ok(false),
    };
    if get(order, "fetchStarts").as_array().map(|a| a.len()).unwrap_or(usize::MAX) != 0 {
        return Ok(false);
    }
    if get(order, "receipts").as_array().map(|a| a.len()).unwrap_or(usize::MAX) != 0 {
        return Ok(false);
    }
    if fences.is_empty() {
        return Ok(true);
    }
    // order.blockedNetworkProofs?.at(-1) — undefined unless a non-empty array.
    let p = get(order, "blockedNetworkProofs")
        .as_array()
        .and_then(|a| a.last())
        .cloned()
        .unwrap_or(V::Null);
    let f = &fences[0];
    Ok(sget(&p, "kind") == Some("serialized_client_before_network_block_v1")
        && sget(&p, "method") == Some("POST")
        && sget(&p, "endpoint") == Some("/orders")
        && get(&p, "physicalFetchReached") == &V::Bool(false)
        && get(&p, "requestBodyHash") == get(order, "bodyHash")
        && get(&p, "requestBodyHash").as_str() == Some(payload_hash(get(order, "body"))?.as_str())
        && dparse(get(&p, "at")) >= dparse(get(f, "at")))
}
