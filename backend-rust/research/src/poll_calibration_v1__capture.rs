// research/poll-calibration-v1/capture.mjs — freeze the raw poll archive,
// codebook and paper PDF with receipts and git-blob pinning.
use crate::files::{
    exists, fetch, log_json, mkdir, read_bytes, read_json, runtime_dir, sha1, sha256, write_json_pretty_wx,
    write_wx,
};
use crate::js::{now_iso, num};
use baal_service_runtime::json_text::js_stringify;
use serde_json::{json, Value as V};
use std::path::Path;

pub fn run() -> Result<V, String> {
    let dir = runtime_dir()?.join("poll-calibration-sources-20261005");
    mkdir(&dir)?;
    let inputs: Vec<V> = vec![
        json!({"file":"raw_polls.csv","url":"https://raw.githubusercontent.com/fivethirtyeight/data/master/pollster-ratings/raw_polls.csv","gitBlob":"08735ea503ad64c67cfa285fd75c0a270fdca245","kind":"csv"}),
        json!({"file":"codebook.md","url":"https://raw.githubusercontent.com/fivethirtyeight/data/master/pollster-ratings/README.md","gitBlob":"78b95c4a883383322bf4c4b3f88d69ee4c1e2e48","kind":"text"}),
        json!({"file":"marist-iowa-tables.pdf","url":"https://maristpoll.marist.edu/wp-content/uploads/2026/09/Marist-Poll-IA-NOS-and-Tables_202609211631-1.pdf","kind":"pdf"}),
    ];
    let mut captures: Vec<V> = Vec::new();
    for input in &inputs {
        let name = input["file"].as_str().unwrap_or_default();
        let file = dir.join(name);
        let receipt = Path::new(&format!("{}.receipt.json", file.display())).to_path_buf();
        if exists(&file) || exists(&receipt) {
            crate::js::require_that(exists(&file) && exists(&receipt), "INCOMPLETE_CAPTURE")?;
            let old = read_json(&receipt)?;
            let blob_ok = match input.get("gitBlob") {
                Some(V::String(want)) => old.get("gitBlob").and_then(V::as_str) == Some(want.as_str()),
                _ => true,
            };
            crate::js::require_that(
                old.get("url").and_then(V::as_str)
                    == input.get("url").and_then(V::as_str)
                    && sha256(&read_bytes(&file)?) == old["sha256"].as_str().unwrap_or("")
                    && blob_ok,
                "CAPTURE_CHANGED",
            )?;
            captures.push(old);
            continue;
        }
        let response = fetch(input["url"].as_str().unwrap_or(""), 30_000)?;
        crate::js::require_that(
            (200..300).contains(&response.status),
            format!("SOURCE_HTTP_{}", response.status),
        )?;
        let bytes = response.bytes;
        crate::js::require_that(bytes.len() <= 8_000_000, "SOURCE_SIZE_LIMIT")?;
        let mut blob_bytes = format!("blob {}\0", bytes.len()).into_bytes();
        blob_bytes.extend_from_slice(&bytes);
        let git_blob = sha1(&blob_bytes);
        if let Some(V::String(want)) = input.get("gitBlob") {
            crate::js::require_that(&git_blob == want, "GITHUB_BLOB_CHANGED")?;
        }
        if input["kind"].as_str() == Some("csv") {
            let head = String::from_utf8_lossy(&bytes[..400.min(bytes.len())]);
            crate::js::require_that(head.contains("margin_actual"), "CSV_HEADER_CHANGED")?;
        }
        if input["kind"].as_str() == Some("pdf") {
            crate::js::require_that(
                bytes.len() >= 5 && &bytes[..5] == b"%PDF-",
                "PDF_TYPE_CHANGED",
            )?;
        }
        let record = json!({
            "file": input["file"],
            "url": input["url"],
            "observedAt": now_iso(),
            "bytes": num(bytes.len() as f64),
            "sha256": sha256(&bytes),
            "gitBlob": input.get("gitBlob").cloned().unwrap_or(V::Null),
            "contentType": response.content_type.clone().map_or(V::Null, V::String),
        });
        write_wx(&file, &bytes)?;
        write_wx(&receipt, (serde_json::to_string_pretty(&record).map_err(|e| e.to_string())? + "\n").as_bytes())?;
        captures.push(record);
    }
    let manifest = json!({
        "schemaVersion": 1,
        "id": "primary-poll-calibration-sources-20261005-v1",
        "captures": captures,
        "rawSourcesRestrictedLocally": true,
        "accountApiRequests": 0,
    });
    let file = Path::new("research/poll-calibration-v1/sources.json");
    if exists(file) {
        crate::js::require_that(
            js_stringify(&read_json(file)?) == js_stringify(&manifest),
            "MANIFEST_CHANGED",
        )?;
    } else {
        write_json_pretty_wx(file, &manifest)?;
    }
    let out = json!({
        "sources": captures.iter().map(|c| json!({
            "file": c["file"], "bytes": c["bytes"], "sha256": c["sha256"],
        })).collect::<Vec<V>>(),
        "accountApiRequests": 0,
    });
    log_json(&out);
    Ok(out)
}
