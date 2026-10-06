// research/current-polls-v1/capture.mjs — freeze the September 2026 poll
// pages and the official Kansas ballot notice. `ballots` selects the list.
use crate::files::{
    exists, fetch, log_json, mkdir, read_bytes, read_json, runtime_dir, sha256, write_json_pretty_wx,
    write_wx,
};
use crate::js::{now_iso, num, require_that};
use baal_service_runtime::json_text::js_stringify;
use serde_json::{json, Value as V};
use std::path::Path;

pub fn run(ballots: bool) -> Result<V, String> {
    let dir = runtime_dir()?.join(format!(
        "research-sources-current-polls-20261005{}",
        if ballots { "-ballots" } else { "" }
    ));
    let sources: Vec<(&str, &str, Vec<&str>)> = if ballots {
        vec![(
            "kansas-official.html",
            "https://meadeco.org/367/2026-General-Election-Information",
            vec!["2026", "Hamilton", "Marshall", "Graham"],
        )]
    } else {
        vec![
            ("kansas-emerson.html","https://emersoncollegepolling.com/kansas-2026/",vec!["Hamilton","Marshall","September 15","750"]),
            ("iowa-emerson.html","https://emersoncollegepolling.com/iowa-2026-poll-hinson-leads-turek/",vec!["Hinson","Turek","September 1","750"]),
            ("iowa-marist.html","https://maristpoll.marist.edu/polls/2026-midterm-elections-in-iowa-september-2026/",vec!["Turek","Hinson","registered voters","1,050"]),
            ("national-emerson.html","https://emersoncollegepolling.com/september-2026-national-poll-democrats/",vec!["53%","42%","September 21","1,000"]),
            ("iowa-official.html","https://elections.marshallcountyia.gov/elections/info/2026_general_election_2026_11_03/",vec!["2026","Hinson","Turek","Laehn"]),
        ]
    };
    mkdir(&dir)?;
    let mut captures: Vec<V> = Vec::new();
    for (file, url, required) in &sources {
        let out = dir.join(file);
        let receipt = Path::new(&format!("{}.receipt.json", out.display())).to_path_buf();
        if exists(&out) || exists(&receipt) {
            require_that(exists(&out) && exists(&receipt), "INCOMPLETE_SOURCE_CAPTURE")?;
            let r = read_json(&receipt)?;
            let hash = sha256(&read_bytes(&out)?);
            require_that(
                hash == r["contentHash"].as_str().unwrap_or("")
                    && r["url"].as_str() == Some(*url),
                "SOURCE_CAPTURE_CHANGED",
            )?;
            captures.push(r);
            continue;
        }
        let r = fetch(url, 30_000)?;
        require_that((200..300).contains(&r.status), format!("PUBLIC_SOURCE_HTTP_{}", r.status))?;
        let bytes = r.bytes;
        require_that(bytes.len() <= 4_000_000, "PUBLIC_SOURCE_TOO_LARGE")?;
        let text = String::from_utf8_lossy(&bytes).to_string();
        for phrase in required {
            require_that(
                text.contains(phrase),
                format!("PUBLIC_SOURCE_FACTS_CHANGED_{file}"),
            )?;
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
    let manifest = json!({
        "schemaVersion": 1,
        "id": if ballots { "current-primary-ballots-20261005-v1" } else { "current-primary-polls-20261005-v1" },
        "captures": captures,
        "rawSourcesRestrictedLocally": true,
        "accountApiRequests": 0,
    });
    let file = Path::new("research/current-polls-v1").join(if ballots {
        "ballot-sources.json"
    } else {
        "sources.json"
    });
    if exists(&file) {
        require_that(
            js_stringify(&read_json(&file)?) == js_stringify(&manifest),
            "MANIFEST_CHANGED",
        )?;
    } else {
        write_json_pretty_wx(&file, &manifest)?;
    }
    let out = json!({
        "sources": num(captures.len() as f64),
        "accountApiRequests": 0,
        "rawSourcesPublished": false,
    });
    log_json(&out);
    Ok(out)
}
