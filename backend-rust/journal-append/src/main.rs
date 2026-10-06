use baal_journal_append::Journal;
use serde_json::{Value as V, json};
use std::io::{self, BufRead, Write};
fn main() {
    let mut journal: Option<Journal> = None;
    let mut out = io::BufWriter::new(io::stdout().lock());
    for line in io::stdin().lock().lines() {
        let result = (|| -> Result<V, String> {
            let request: V = serde_json::from_str(&line.map_err(|_| "APPEND_INPUT")?)
                .map_err(|_| "APPEND_INPUT")?;
            match request["op"].as_str().unwrap_or("") {
                "open" => {
                    if journal.is_some() {
                        return Err("APPEND_ALREADY_OPEN".into());
                    }
                    let opened = Journal::open(&request)?;
                    let status = opened.status();
                    journal = Some(opened);
                    Ok(json!({"opened":true,"receipt":status}))
                }
                "commit" => journal.as_mut().ok_or("APPEND_NOT_OPEN")?.commit(&request),
                "status" => {
                    Ok(json!({"receipt":journal.as_ref().ok_or("APPEND_NOT_OPEN")?.status()}))
                }
                "close" => {
                    journal = None;
                    Ok(json!({"closed":true}))
                }
                _ => Err("APPEND_OPERATION".into()),
            }
        })();
        let response = result.unwrap_or_else(|e| json!({"error":e}));
        writeln!(out, "{response}").unwrap();
        out.flush().unwrap();
    }
}
