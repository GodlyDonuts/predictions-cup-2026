// live-paper/ml-shadow.mjs — verified-source helpers (subset used downstream).
use baal_service_runtime::json_text::digest;
use baal_service_runtime::contract::parse_iso_ms;
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};

pub struct VerifiedSource { pub records: Vec<V>, pub prefix_hashes: Vec<String> }
/// verifySource(raw) — null when raw fails the whole-string shape, throw on
/// integrity/clock failures (JS semantics preserved: null vs Err).
pub fn verify_source(raw: &str) -> Result<Option<VerifiedSource>, String> {
    if raw.is_empty() || !raw.ends_with('\n') { return Ok(None); }
    let mut previous = String::new();
    let mut clock = f64::NEG_INFINITY;
    let mut records = Vec::new();
    let mut prefix_hashes = Vec::new();
    let mut prefix = Sha256::new();
    for (i, line) in raw[..raw.len() - 1].split('\n').enumerate() {
        let r: V = serde_json::from_str(line).map_err(|_| "Source JSON invalid".to_string())?;
        let hash = r["hash"].as_str().unwrap_or("").to_string();
        let mut body = r.clone();
        body.as_object_mut().unwrap().remove("hash");
        if r["sequence"].as_i64() != Some(i as i64 + 1)
            || r["previous"].as_str().unwrap_or("") != previous
            || hash != digest(&body) {
            return Err("Source integrity failure".into());
        }
        let next = r["event"]["clockAt"].as_str().or(r["at"].as_str()).and_then(parse_iso_ms)
            .ok_or("Source clock rollback")?;
        if next < clock { return Err("Source clock rollback".into()); }
        clock = next;
        prefix.update(format!("{}\n", line).as_bytes());
        prefix_hashes.push(format!("{:x}", prefix.clone().finalize()));
        previous = hash;
        records.push(r);
    }
    Ok(Some(VerifiedSource { records, prefix_hashes }))
}
