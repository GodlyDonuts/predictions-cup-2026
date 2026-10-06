// research/profit-sprint-20261005/capture.mjs — parallel best-effort fetch of
// the broader Texas/Iowa/Michigan poll corpus. JS uses Promise.all; capture
// order is the source order either way, so this runs them sequentially.
use crate::files::{
    exists, fetch, log_json, mkdir, read_bytes, read_json, runtime_dir, sha256, write_json_pretty,
    write_wx,
};
use crate::js::{now_iso, num};
use serde_json::{json, Value as V};
use std::path::Path;

fn attempt(file: &str, url: &str, dir: &Path) -> Result<V, String> {
    let out = dir.join(file);
    let r = fetch(url, 20_000)?;
    let bytes = r.bytes;
    if !(200..300).contains(&r.status) {
        return Err(format!("SOURCE_HTTP_{}", r.status));
    }
    if bytes.len() < 100 || bytes.len() > 6_000_000 {
        return Err("SOURCE_SIZE".to_string());
    }
    if file.ends_with(".pdf") && !(bytes.len() >= 5 && &bytes[..5] == b"%PDF-") {
        return Err("NOT_PDF".to_string());
    }
    write_wx(&out, &bytes)?;
    Ok(json!({
        "file": file,
        "url": url,
        "status": "captured",
        "observedAt": now_iso(),
        "bytes": num(bytes.len() as f64),
        "contentHash": sha256(&bytes),
        "contentType": r.content_type.clone().map_or(V::Null, V::String),
    }))
}

fn reason_of(message: &str) -> String {
    for prefix in ["SOURCE_HTTP_", "SOURCE_SIZE", "NOT_PDF"] {
        if message.starts_with(prefix) {
            return message.to_string();
        }
    }
    "PUBLIC_FETCH_FAILED".to_string()
}

pub fn run() -> Result<V, String> {
    let runtime = runtime_dir()?.join("research-profit-sprint-20261005");
    mkdir(&runtime)?;
    let sources: Vec<(&str, &str)> = vec![
        ("fox-texas-topline.pdf","https://static.foxnews.com/foxnews.com/content/uploads/2026/10/fox_september-24-28-2026_texas_topline_october-1-release.pdf"),
        ("fox-texas-crosstabs.pdf","https://static.foxnews.com/foxnews.com/content/uploads/2026/10/fox_september-24-28-2026_texas_cross-tabs_october-1-release.pdf"),
        ("fox-iowa-topline.pdf","https://static.foxnews.com/foxnews.com/content/uploads/2026/10/fox_september-24-28-2026_iowa_topline_october-1-release.pdf"),
        ("fox-michigan-topline.pdf","https://static.foxnews.com/foxnews.com/content/uploads/2026/09/fox_september-24-28-2026_michigan_topline_september-30-release.pdf"),
        ("bigdata-texas.html","https://www.bigdatapoll.com/blog/lone-star-state-poll-talarico-paxton-in-close-race-us-senate-in-texas/"),
        ("ut-governor-tracker.html","https://texaspolitics.utexas.edu/blog/texas-2026-gubernatorial-poll-tracker"),
        ("tsu-september.pdf","https://static1.squarespace.com/static/67aa7aa3e284b002cd6a420e/t/6ab15cb39e4cc420b9ef07f5/1790008500236/TSUSept2026_TXStatewideRaces.pdf"),
        ("siena-governors.html","https://sri.siena.edu/2026/10/04/nytimes-siena-poll-of-likely-voters-gubernatorial-races-2/"),
        ("siena-senate.html","https://sri.siena.edu/2026/10/03/nytimes-siena-poll-of-likely-voters-senate-races/"),
        ("nyt-texas-toplines.html","https://www.nytimes.com/interactive/2026/10/03/polls/times-siena-texas-poll-toplines.html"),
        ("tpor-september.html","https://texaspublicopinionresearch.substack.com/p/new-poll-in-texas-general-election-662"),
    ];
    let mut captures: Vec<V> = Vec::new();
    for (file, url) in &sources {
        let out = runtime.join(file);
        let receipt = Path::new(&format!("{}.receipt.json", out.display())).to_path_buf();
        if exists(&receipt) {
            let r = read_json(&receipt)?;
            if r["status"].as_str() == Some("captured")
                && sha256(&read_bytes(&out)?) != r["contentHash"].as_str().unwrap_or("")
            {
                return Err("FROZEN_SOURCE_CHANGED".to_string());
            }
            captures.push(r);
            continue;
        }
        let record = match attempt(file, url, &runtime) {
            Ok(record) => record,
            Err(e) => json!({
                "file": file,
                "url": url,
                "status": "unavailable",
                "observedAt": now_iso(),
                "reason": reason_of(&e),
            }),
        };
        write_wx(
            &receipt,
            (serde_json::to_string_pretty(&record).map_err(|e| e.to_string())? + "\n").as_bytes(),
        )?;
        captures.push(record);
    }
    let manifest = json!({
        "schemaVersion": 1,
        "id": "profit-sprint-primary-sources-20261005",
        "at": now_iso(),
        "captures": captures,
        "accountApiRequests": 0,
        "rawSourcesRestrictedLocally": true,
        "executionAuthority": false,
    });
    write_json_pretty(Path::new("research/profit-sprint-20261005/sources.json"), &manifest)?;
    let out = V::Array(
        captures
            .iter()
            .map(|x| {
                json!({
                    "file": x["file"],
                    "status": x["status"],
                    "reason": x.get("reason").cloned().unwrap_or(V::Null),
                })
            })
            .collect(),
    );
    log_json(&out);
    Ok(out)
}
