// execution/physical-fetch-block-proof-v1.mjs — affirmative clock-side proof
// that the physical fetch expression was never reached: emitted when the
// serialized physical fence expired before invocation.
//
// Port notes:
// - JSON.parse(spec.bodyJson) failure propagates as a serde error message,
//   like the JS SyntaxError (not a need() code).
// - Field order of the proof object follows the JS literal exactly.
use baal_execution::core::payload_hash;
use baal_service_runtime::contract::parse_iso_ms;
use serde_json::{json, Map, Value as V};

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn sget<'a>(v: &'a V, k: &str) -> Option<&'a str> {
    v.get(k).and_then(V::as_str)
}

/// physicalFetchBlockProofV1({spec,code,at})
pub fn physical_fetch_block_proof_v1(input: &V) -> Result<V, String> {
    let spec = get(input, "spec");
    let code = get(input, "code");
    let at = get(input, "at");
    if sget(spec, "method") != Some("POST")
        || sget(spec, "endpoint") != Some("/orders")
        || !get(spec, "bodyJson").is_string()
        || code.as_str() != Some("BAAL_SCALED_PHYSICAL_FETCH_FENCE")
        || !at
            .as_str()
            .and_then(parse_iso_ms)
            .is_some_and(|x| x.is_finite())
    {
        return Err("PHYSICAL_FETCH_PROOF_INPUT".into());
    }
    let body: V = serde_json::from_str(get(spec, "bodyJson").as_str().unwrap_or(""))
        .map_err(|e| e.to_string())?;
    let mut out = Map::new();
    out.insert("schemaVersion".into(), json!(1));
    out.insert(
        "kind".into(),
        json!("affirmative_before_actual_fetch_clock_block_v1"),
    );
    out.insert("method".into(), json!("POST"));
    out.insert("endpoint".into(), json!("/orders"));
    out.insert("requestBodyHash".into(), json!(payload_hash(&body)?));
    out.insert("at".into(), at.clone());
    out.insert("physicalFetchReached".into(), json!(false));
    out.insert("code".into(), code.clone());
    Ok(V::Object(out))
}
