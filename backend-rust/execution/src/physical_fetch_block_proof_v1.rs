// execution/physical-fetch-block-proof-v1.mjs — affirmative producer-side
// proof that a place-order spec was rejected on the physical-fetch clock
// fence before any actual network invocation.
//
// Port notes:
// - `spec.bodyJson` must be a string; JSON.parse errors propagate raw.
// - `Number.isFinite(Date.parse(at))` accepts any parseable date string; a
//   non-string/unparseable `at` is NaN -> PHYSICAL_FETCH_PROOF_INPUT.
// - The returned `at` is the input value verbatim (not normalized).
use baal_service_runtime::contract::parse_iso_ms;
use serde_json::{json, Value as V};

use crate::core::payload_hash;

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}

/// physicalFetchBlockProofV1({spec,code,at})
pub fn physical_fetch_block_proof_v1(input: &V) -> Result<V, String> {
    let spec = get(input, "spec");
    let code = get(input, "code").as_str();
    let at = get(input, "at");
    let body_json = get(spec, "bodyJson").as_str();
    if !(get(spec, "method").as_str() == Some("POST")
        && get(spec, "endpoint").as_str() == Some("/orders")
        && body_json.is_some()
        && code == Some("BAAL_SCALED_PHYSICAL_FETCH_FENCE")
        && at.as_str().and_then(parse_iso_ms).is_some())
    {
        return Err("PHYSICAL_FETCH_PROOF_INPUT".into());
    }
    let body: V = serde_json::from_str(body_json.unwrap()).map_err(|e| e.to_string())?;
    Ok(json!({
        "schemaVersion": 1,
        "kind": "affirmative_before_actual_fetch_clock_block_v1",
        "method": "POST",
        "endpoint": "/orders",
        "requestBodyHash": payload_hash(&body)?,
        "at": at,
        "physicalFetchReached": false,
        "code": code.unwrap(),
    }))
}
