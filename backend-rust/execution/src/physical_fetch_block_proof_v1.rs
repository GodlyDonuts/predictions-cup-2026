// execution/physical-fetch-block-proof-v1.mjs — affirmative proof recorded by
// the serialized client when the monotonic deadline check fired before the
// physical fetch expression. It cannot describe an API rejection.
use baal_service_runtime::contract::parse_iso_ms;
use serde_json::{json, Map, Value as V};

use crate::core::payload_hash;

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}

/// physicalFetchBlockProofV1({spec,code,at})
pub fn physical_fetch_block_proof_v1(input: &V) -> Result<V, String> {
    let spec = get(input, "spec");
    let code = get(input, "code");
    let at = get(input, "at");
    if !(get(spec, "method").as_str() == Some("POST")
        && get(spec, "endpoint").as_str() == Some("/orders")
        && get(spec, "bodyJson").is_string()
        && code.as_str() == Some("BAAL_SCALED_PHYSICAL_FETCH_FENCE")
        && at.as_str().and_then(parse_iso_ms).is_some_and(|x| x.is_finite()))
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
