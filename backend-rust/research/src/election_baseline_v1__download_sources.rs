// research/election-baseline-v1/download-sources.mjs — fetch the Harvard
// Dataverse election datasets (senate/president 1976-2024 + codebooks) with
// published MD5 verification and manifest receipts.
use crate::files::{
    encode_uri_component, exists, fetch, log_json, md5, mkdir, read_bytes, sha256, write_wx,
};
use crate::js::{now_iso, num, require_that};
use serde_json::{json, Value as V};
use std::path::Path;

// /codebook|1976.*senate|1976.*president|1976.*house/.test(filename)
fn wanted(filename: &str) -> bool {
    if filename.contains("codebook") {
        return true;
    }
    if let Some(pos) = filename.find("1976") {
        let tail = &filename[pos..];
        return tail.contains("senate") || tail.contains("president") || tail.contains("house");
    }
    false
}
// originalFilename.replace(/[^a-zA-Z0-9._-]/g, '_')
fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

pub fn run() -> Result<V, String> {
    let dir = Path::new("research/election-baseline-v1/sources");
    mkdir(dir)?;
    for doi in ["doi:10.7910/DVN/PEJ5QU", "doi:10.7910/DVN/42MVDX"] {
        let metadata_url = format!(
            "https://dataverse.harvard.edu/api/datasets/:persistentId/?persistentId={}",
            encode_uri_component(doi)
        );
        let response = fetch(&metadata_url, 0)?;
        require_that(
            (200..300).contains(&response.status),
            format!("Metadata HTTP {}", response.status),
        )?;
        let metadata: V =
            serde_json::from_slice(&response.bytes).map_err(|e| e.to_string())?;
        let version = &metadata["data"]["latestVersion"];
        let mut source = serde_json::Map::new();
        source.insert("doi".into(), json!(doi));
        source.insert("metadataUrl".into(), json!(metadata_url));
        source.insert("version".into(), version["versionNumber"].clone());
        source.insert("minorVersion".into(), version["versionMinorNumber"].clone());
        source.insert("publicationDate".into(), version["releaseTime"].clone());
        // license: undefined is omitted by JSON.stringify.
        if let Some(name) = version["license"].get("name") {
            source.insert("license".into(), name.clone());
        }
        source.insert("files".into(), V::Array(vec![]));
        let mut files: Vec<V> = Vec::new();
        for item in version["files"].as_array().cloned().unwrap_or_default() {
            let f = &item["dataFile"];
            let fname = f["filename"].as_str().unwrap_or_default();
            if !wanted(fname) {
                continue;
            }
            let id = f["id"].clone();
            let url = format!(
                "https://dataverse.harvard.edu/api/access/datafile/{}?format=original",
                crate::js::js_string(&id)
            );
            let result = fetch(&url, 0)?;
            require_that(
                (200..300).contains(&result.status),
                format!("File HTTP {}", result.status),
            )?;
            let bytes = result.bytes;
            require_that(bytes.len() <= 15_000_000, "Unexpected dataset size")?;
            let digest = md5(&bytes);
            if f["checksum"].get("type").and_then(V::as_str) == Some("MD5")
                && digest != f["checksum"]["value"].as_str().unwrap_or("")
            {
                return Err("Published checksum mismatch".to_string());
            }
            let original = f.get("originalFileName").and_then(V::as_str).unwrap_or("");
            let original_filename = if original.is_empty() { fname.to_string() } else { original.to_string() };
            let filename = sanitize(&original_filename);
            let local = dir.join(&filename);
            if exists(&local) && read_bytes(&local)? != bytes {
                return Err("Immutable source differs".to_string());
            }
            if !exists(&local) {
                write_wx(&local, &bytes)?;
            }
            let mut entry = serde_json::Map::new();
            entry.insert("id".into(), f["id"].clone());
            entry.insert("filename".into(), json!(filename));
            entry.insert("originalFilename".into(), json!(original_filename));
            entry.insert("url".into(), json!(url));
            entry.insert("bytes".into(), num(bytes.len() as f64));
            entry.insert("sha256".into(), json!(sha256(&bytes)));
            if let Some(cs) = f.get("checksum") {
                entry.insert("publishedChecksum".into(), cs.clone());
            }
            entry.insert("downloadedAt".into(), json!(now_iso()));
            files.push(V::Object(entry));
            let head: Vec<String> = String::from_utf8_lossy(&bytes)
                .split('\n')
                .take(2)
                .map(|s| s.to_string())
                .collect();
            log_json(&json!({
                "doi": doi,
                "filename": filename,
                "bytes": num(bytes.len() as f64),
                "header": head,
            }));
        }
        source.insert("files".into(), V::Array(files));
        let last = doi.split('/').last().unwrap_or_default();
        let filename = dir.join(format!("{last}-manifest.json"));
        if !exists(&filename) {
            write_wx(
                &filename,
                (serde_json::to_string_pretty(&V::Object(source)).map_err(|e| e.to_string())?
                    + "\n")
                    .as_bytes(),
            )?;
        }
    }
    Ok(V::Null)
}
