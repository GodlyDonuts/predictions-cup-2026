// research/current-polls-v2/capture-comparators.mjs — freeze Emerson pages
// and the official Texas/Michigan ballot sources.
use crate::files::{
    exists, fetch, log_json, read_bytes, read_json, runtime_dir, sha256, write_json_pretty_wx,
    write_wx,
};
use crate::js::{now_iso, num, require_that};
use baal_service_runtime::json_text::js_stringify;
use serde_json::{json, Value as V};
use std::path::Path;

pub fn run() -> Result<V, String> {
    let dir = runtime_dir()?.join("research-sources-current-marist-20261005-v2");
    let sources: Vec<(&str, &str, Option<Vec<&str>>)> = vec![
        ("michigan-emerson.html","https://emersoncollegepolling.com/michigan-2026-poll-el-sayed-and-rogers-locked-in-close-election/",Some(vec!["El-Sayed","Rogers","48%","46%","September 12","1,000"])),
        ("texas-emerson.html","https://emersoncollegepolling.com/texas-2026-poll-talarico-and-paxton-in-dead-heat-abbott-maintains-edge/",Some(vec!["Talarico","Paxton","47%","46%","September 12","1,000"])),
        ("texas-official.pdf","https://www.sos.state.tx.us/elections/forms/2026-ballot-cert.pdf",None),
        ("michigan-official.html","https://mi-boe.entellitrak.com/etk-mi-boe-prod/page.request.do?electionType=GEN&electionYear=2026&page=page.miboePublicReport",Some(vec!["El-Sayed","Rogers","General Election"])),
    ];
    let mut captures: Vec<V> = Vec::new();
    for (file, url, phrases) in &sources {
        let out = dir.join(file);
        let receipt = Path::new(&format!("{}.receipt.json", out.display())).to_path_buf();
        if exists(&out) {
            let r = read_json(&receipt)?;
            require_that(
                r["url"].as_str() == Some(*url)
                    && r["contentHash"].as_str().unwrap_or("") == sha256(&read_bytes(&out)?),
                "SOURCE_CHANGED",
            )?;
            captures.push(r);
            continue;
        }
        let r = fetch(url, 30_000)?;
        require_that((200..300).contains(&r.status), format!("SOURCE_HTTP_{}", r.status))?;
        let bytes = r.bytes;
        let maximum: usize = if *file == "texas-official.pdf" { 32_000_000 } else { 4_000_000 };
        require_that(
            bytes.len() <= maximum && !bytes.is_empty(),
            format!("SOURCE_SIZE_{file}"),
        )?;
        match phrases {
            Some(list) => {
                let text = String::from_utf8_lossy(&bytes).to_string();
                for phrase in list {
                    require_that(
                        text.contains(phrase),
                        format!("SOURCE_FACT_CHANGED_{file}"),
                    )?;
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
            "contentType": r.content_type.clone().map_or(V::Null, V::String),
            "contentHash": sha256(&bytes),
        });
        write_wx(&out, &bytes)?;
        write_wx(&receipt, (serde_json::to_string(&record).map_err(|e| e.to_string())? + "\n").as_bytes())?;
        captures.push(record);
    }
    let manifest = json!({
        "schemaVersion": 1,
        "id": "current-primary-comparators-20261005-v2",
        "captures": captures,
        "accountApiRequests": 0,
        "rawSourcesRestrictedLocally": true,
    });
    let out = Path::new("research/current-polls-v2/comparator-sources.json");
    if exists(out) {
        require_that(
            js_stringify(&read_json(out)?) == js_stringify(&manifest),
            "MANIFEST_CHANGED",
        )?;
    } else {
        write_json_pretty_wx(out, &manifest)?;
    }
    let result = json!({
        "sources": num(captures.len() as f64),
        "accountApiRequests": 0,
    });
    log_json(&result);
    Ok(result)
}
