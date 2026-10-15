//! Verify legacy canonical JSON and wallet provenance without a JS interpreter.
use crate::{Result, engine::Engine};
use serde_json::Value;
use sha2::{Digest, Sha256};
fn index(k: &str) -> Option<u32> {
    k.parse::<u32>()
        .ok()
        .filter(|n| *n < u32::MAX && n.to_string() == k)
}
pub fn canonical(v: &Value) -> Result<String> {
    let mut out = String::new();
    write(v, &mut out)?;
    Ok(out)
}
fn write(v: &Value, out: &mut String) -> Result<()> {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(v) => out.push_str(if *v { "true" } else { "false" }),
        Value::String(v) => out.push_str(&serde_json::to_string(v).map_err(|_| "LEGACY_JSON")?),
        Value::Number(v) => {
            let f = v
                .as_f64()
                .filter(|f| f.is_finite())
                .ok_or("LEGACY_NUMBER")?;
            out.push_str(ryu_js::Buffer::new().format_finite(f));
        }
        Value::Array(v) => {
            out.push('[');
            for (i, v) in v.iter().enumerate() {
                if i > 0 {
                    out.push(',')
                }
                write(v, out)?;
            }
            out.push(']');
        }
        Value::Object(v) => {
            let mut keys = v.keys().collect::<Vec<_>>();
            keys.sort_by(|a, b| match (index(a), index(b)) {
                (Some(a), Some(b)) => a.cmp(&b),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => a.encode_utf16().cmp(b.encode_utf16()),
            });
            out.push('{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',')
                }
                out.push_str(&serde_json::to_string(k).map_err(|_| "LEGACY_KEY")?);
                out.push(':');
                write(&v[*k], out)?;
            }
            out.push('}');
        }
    }
    Ok(())
}
pub fn hash(v: &Value) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(canonical(v)?.as_bytes())))
}
pub fn verify_snapshot(v: &Value) -> Result<Engine> {
    let mut body = v.as_object().ok_or("LEGACY_SNAPSHOT")?.clone();
    let expected = body.remove("hash").ok_or("LEGACY_HASH")?;
    if expected.as_str() != Some(hash(&Value::Object(body))?.as_str()) {
        return Err("LEGACY_SNAPSHOT_HASH");
    }
    if v["state"]["policyHash"].as_str() != Some(hash(&v["state"]["policy"])?.as_str()) {
        return Err("LEGACY_POLICY_HASH");
    }
    Engine::from_seed_state(&v["state"])
}
pub fn load_snapshot(
    path: &std::path::Path,
) -> std::result::Result<(Engine, Value, String), Box<dyn std::error::Error>> {
    if std::fs::metadata(path)?.len() > 128 * 1024 * 1024 {
        return Err("LEGACY_SNAPSHOT_SIZE".into());
    }
    let bytes = std::fs::read(path)?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let v: Value = serde_json::from_slice(&bytes)?;
    let engine = verify_snapshot(&v)?;
    Ok((engine, v, digest))
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn canonical_matches_js_numeric_keys_and_number_spelling() {
        let v = json!({"10":1.0,"2":0.000001,"a":1e21,"01":1e-7,"0":-0.0});
        assert_eq!(
            canonical(&v).unwrap(),
            "{\"0\":0,\"2\":0.000001,\"10\":1,\"01\":1e-7,\"a\":1e+21}"
        );
    }
    #[test]
    fn counterfeit_snapshot_is_rejected() {
        assert!(verify_snapshot(&json!({"hash":"fake","state":{}})).is_err())
    }
}
