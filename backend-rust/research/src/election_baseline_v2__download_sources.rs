// research/election-baseline-v2/download-sources.mjs — fetch the manifest-pinned
// public FEC sources with byte/sha256 verification.
use crate::election_baseline_v1__model::hash;
use crate::files::{exists, fetch, log_json, mkdir, read_bytes, read_json, write_wx};
use crate::js::{num, require_that};
use serde_json::{json, Value as V};
use std::path::Path;

pub fn run() -> Result<V, String> {
    let base = Path::new("research/election-baseline-v2");
    let manifest = read_json(&base.join("source-manifest.json"))?;
    mkdir(&base.join("sources"))?;
    for s in manifest["sources"].as_array().cloned().unwrap_or_default() {
        let filename = s["filename"].as_str().unwrap_or_default();
        let target = base.join("sources").join(filename);
        if exists(&target) {
            let actual = hash(&read_bytes(&target)?);
            require_that(
                actual == s["sha256"].as_str().unwrap_or(""),
                format!("assert.equal(hash, '{}')", s["sha256"].as_str().unwrap_or("")),
            )?;
            continue;
        }
        let response = fetch(s["url"].as_str().unwrap_or(""), 0)?;
        require_that(
            (200..300).contains(&response.status),
            format!("Public FEC download {}", response.status),
        )?;
        let bytes = response.bytes;
        require_that(
            bytes.len() as f64 == crate::js::js_number(s.get("bytes")),
            "assert.equal(bytes.length, s.bytes)",
        )?;
        let actual = hash(&bytes);
        require_that(
            actual == s["sha256"].as_str().unwrap_or(""),
            "assert.equal(hash(bytes), s.sha256)",
        )?;
        write_wx(&target, &bytes)?;
    }
    let out = json!({
        "sourceIntegrityVerified": true,
        "files": num(manifest["sources"].as_array().map(|a| a.len() as f64).unwrap_or(0.0)),
    });
    log_json(&out);
    Ok(out)
}
