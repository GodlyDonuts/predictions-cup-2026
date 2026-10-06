// research/current-polls-v2/capture.mjs — freeze Marist poll pages/PDFs.
use crate::files::{
    exists, fetch, log_json, mkdir, read_bytes, read_json, runtime_dir, sha256, write_json_pretty_wx,
    write_wx,
};
use crate::js::{now_iso, num, require_that};
use baal_service_runtime::json_text::js_stringify;
use serde_json::{json, Value as V};
use std::path::Path;

pub fn run() -> Result<V, String> {
    let dir = runtime_dir()?.join("research-sources-current-marist-20261005-v2");
    let sources: Vec<(&str, &str, Option<Vec<&str>>)> = vec![
        ("michigan-marist.html","https://maristpoll.marist.edu/polls/midterm-elections-in-michigan-september-2026/",Some(vec!["El-Sayed","Rogers","1,235","September 27"])),
        ("michigan-marist.pdf","https://maristpoll.marist.edu/wp-content/uploads/2026/09/Marist-Poll_MI-NOS-and-Tables_202609281401@9159_7e99y.pdf",None),
        ("texas-marist.html","https://maristpoll.marist.edu/polls/2026-midterm-elections-in-texas-september-2026/",Some(vec!["Talarico","Paxton","1,139","September 20"])),
        ("texas-marist.pdf","https://maristpoll.marist.edu/wp-content/uploads/2026/09/Marist-Poll_TX-NOS-and-Tables_202609221131.pdf",None),
        ("ohio-marist.html","https://maristpoll.marist.edu/polls/2026-midterm-elections-in-ohio-september-2026/",Some(vec!["Brown","Husted","1,298","September 27"])),
        ("ohio-marist.pdf","https://maristpoll.marist.edu/wp-content/uploads/2026/09/Marist-Poll_OH-NOS-and-Tables_202609281347aC909p6465.pdf",None),
    ];
    mkdir(&dir)?;
    let mut captures: Vec<V> = Vec::new();
    for (file, url, phrases) in &sources {
        let out = dir.join(file);
        let receipt = Path::new(&format!("{}.receipt.json", out.display())).to_path_buf();
        if exists(&out) {
            let record = read_json(&receipt)?;
            require_that(
                record["url"].as_str() == Some(*url)
                    && record["contentHash"].as_str().unwrap_or("")
                        == sha256(&read_bytes(&out)?),
                "FROZEN_SOURCE_CHANGED",
            )?;
            captures.push(record);
            continue;
        }
        let r = fetch(url, 30_000)?;
        require_that((200..300).contains(&r.status), format!("SOURCE_HTTP_{}", r.status))?;
        let bytes = r.bytes;
        require_that(
            bytes.len() <= 4_000_000 && !bytes.is_empty(),
            "SOURCE_SIZE",
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
        "id": "current-primary-marist-20261005-v2",
        "captures": captures,
        "rawSourcesRestrictedLocally": true,
        "accountApiRequests": 0,
    });
    let out = Path::new("research/current-polls-v2/sources.json");
    if exists(out) {
        require_that(
            js_stringify(&read_json(out)?) == js_stringify(&manifest),
            "FROZEN_MANIFEST_CHANGED",
        )?;
    } else {
        write_json_pretty_wx(out, &manifest)?;
    }
    let result = json!({
        "sources": num(captures.len() as f64),
        "accountApiRequests": 0,
        "rawPublished": false,
    });
    log_json(&result);
    Ok(result)
}
