// execution/blocked-network-proof-v1.mjs — proof is emitted by the serialized
// client only when beforeNetwork threw, before the physical fetch expression.
// It cannot describe an API rejection.
use baal_service_runtime::contract::parse_iso_ms;
use baal_service_runtime::json_text::js_int;
use serde_json::{json, Map, Value as V};

use crate::core::payload_hash;

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}

fn sget<'a>(v: &'a V, k: &str) -> Option<&'a str> {
    v.get(k).and_then(V::as_str)
}

/// /^[A-Z][A-Z0-9_]{1,120}$/
fn code_pattern(raw: &V) -> String {
    match raw.as_str() {
        Some(s)
            if s.len() >= 2
                && s.len() <= 121
                && s.bytes().next().is_some_and(|b| b.is_ascii_uppercase())
                && s.bytes()
                    .skip(1)
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_') =>
        {
            s.to_string()
        }
        _ => "NETWORK_GUARD_BLOCKED".to_string(),
    }
}

/// blockedNetworkProofV1({spec,error,at})
pub fn blocked_network_proof_v1(input: &V) -> Result<V, String> {
    let spec = get(input, "spec");
    let error = get(input, "error");
    let at = get(input, "at");
    if !["POST", "DELETE"].contains(&sget(spec, "method").unwrap_or(""))
        || !get(spec, "bodyJson").is_string()
        || get(spec, "beforeNetwork").is_null()
        || !at
            .as_str()
            .and_then(parse_iso_ms)
            .is_some_and(|x| x.is_finite())
    {
        return Err("BLOCKED_NETWORK_PROOF_INPUT".into());
    }
    let body: V = serde_json::from_str(get(spec, "bodyJson").as_str().unwrap_or(""))
        .map_err(|e| e.to_string())?;
    let raw = if get(error, "code").is_null() {
        get(error, "message")
    } else {
        get(error, "code")
    };
    let mut out = Map::new();
    out.insert("schemaVersion".into(), json!(1));
    out.insert(
        "kind".into(),
        json!("serialized_client_before_network_block_v1"),
    );
    out.insert("method".into(), get(spec, "method").clone());
    out.insert("endpoint".into(), get(spec, "endpoint").clone());
    out.insert("requestBodyHash".into(), json!(payload_hash(&body)?));
    out.insert("at".into(), at.clone());
    out.insert("physicalFetchReached".into(), json!(false));
    out.insert("code".into(), json!(code_pattern(raw)));
    Ok(V::Object(out))
}

/// blockedNetworkProofMatchesV1(proof,intent)
pub fn blocked_network_proof_matches_v1(proof: &V, intent: &V) -> Result<bool, String> {
    if !(js_int(get(proof, "schemaVersion")) == Some(1)
        && sget(proof, "kind") == Some("serialized_client_before_network_block_v1")
        && sget(proof, "method") == Some("POST")
        && sget(proof, "endpoint") == Some("/orders")
        && get(proof, "physicalFetchReached") == &V::Bool(false)
        && get(proof, "requestBodyHash") == get(intent, "bodyHash"))
    {
        return Ok(false);
    }
    if !(get(proof, "requestBodyHash").as_str()
        == Some(payload_hash(get(intent, "body"))?.as_str()))
    {
        return Ok(false);
    }
    Ok(
        proof
            .get("at")
            .and_then(V::as_str)
            .and_then(parse_iso_ms)
            .map(|x| {
                x >= intent
                    .get("createdAt")
                    .and_then(V::as_str)
                    .and_then(parse_iso_ms)
                    .unwrap_or(f64::NAN)
            })
            .unwrap_or(false)
            && intent.get("orderId") == Some(&V::Null)
            && js_int(intent.get("actualFetchCount").unwrap_or(&V::Null)) == Some(0),
    )
}
