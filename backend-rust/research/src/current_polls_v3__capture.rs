// research/current-polls-v3/capture.mjs — freeze Siena breadth pages and the
// ReconMR Texas PDF.
use crate::files::{
    exists, fetch, log_json, mkdir, read_bytes, read_json, runtime_dir, sha256, write_json_pretty_wx,
    write_wx,
};
use crate::js::{now_iso, num, require_that};
use baal_service_runtime::json_text::js_stringify;
use serde_json::{json, Value as V};
use std::path::Path;

pub fn run() -> Result<V, String> {
    let dir = runtime_dir()?.join("research-sources-siena-breadth-20261005-v3");
    mkdir(&dir)?;
    let sources: Vec<(&str, &str, Option<Vec<&str>>)> = vec![
        ("siena-senate.html","https://sri.siena.edu/2026/10/03/nytimes-siena-poll-of-likely-voters-senate-races/",Some(vec!["Peltola","Hinson","Marshall","Talarico"])),
        ("siena-governors.html","https://sri.siena.edu/2026/10/04/nytimes-siena-poll-of-likely-voters-gubernatorial-races-2/",Some(vec!["Hinojosa","Abbott","Holscher"])),
        ("reconmr-texas.pdf","https://reconmr.com/wp-content/uploads/2026/09/ReconMR-Texas-Poll-September-Release-Final.pdf",None),
    ];
    let mut captures: Vec<V> = Vec::new();
    for (file, url, required) in &sources {
        let out = dir.join(file);
        let receipt = Path::new(&format!("{}.receipt.json", out.display())).to_path_buf();
        if exists(&out) {
            let r = read_json(&receipt)?;
            require_that(
                r["contentHash"].as_str().unwrap_or("") == sha256(&read_bytes(&out)?)
                    && r["url"].as_str() == Some(*url),
                "FROZEN_SOURCE_CHANGED",
            )?;
            captures.push(r);
            continue;
        }
        let r = fetch(url, 30_000)?;
        require_that((200..300).contains(&r.status), format!("SOURCE_HTTP_{}", r.status))?;
        let bytes = r.bytes;
        require_that(
            !bytes.is_empty() && bytes.len() <= 4_000_000,
            "SOURCE_SIZE",
        )?;
        match required {
            Some(list) => {
                let text = String::from_utf8_lossy(&bytes).to_string();
                for phrase in list {
                    require_that(text.contains(phrase), "SOURCE_FACT_CHANGED")?;
                }
            }
            None => {
                require_that(
                    bytes.len() >= 5 && &bytes[..5] == b"%PDF-",
                    "SOURCE_NOT_PDF",
                )?;
            }
        }
        let record = json!({
            "file": file,
            "url": url,
            "observedAt": now_iso(),
            "bytes": num(bytes.len() as f64),
            "contentHash": sha256(&bytes),
            "contentType": r.content_type.clone().map_or(V::Null, V::String),
        });
        write_wx(&out, &bytes)?;
        write_wx(&receipt, (serde_json::to_string(&record).map_err(|e| e.to_string())? + "\n").as_bytes())?;
        captures.push(record);
    }
    let out = Path::new("research/current-polls-v3/sources.json");
    let manifest = json!({
        "schemaVersion": 1,
        "id": "siena-primary-breadth-20261005-v3",
        "captures": captures,
        "accountApiRequests": 0,
        "rawSourcesRestrictedLocally": true,
    });
    if exists(out) {
        require_that(
            js_stringify(&read_json(out)?) == js_stringify(&manifest),
            "MANIFEST_CHANGED",
        )?;
    } else {
        write_json_pretty_wx(out, &manifest)?;
    }
    let result = json!({
        "captured": num(captures.len() as f64),
        "accountApiRequests": 0,
    });
    log_json(&result);
    Ok(result)
}
