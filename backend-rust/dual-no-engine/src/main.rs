use baal_dual_no_engine::{advance_frozen, assert_state, bank_check, create, hash, report};
use serde_json::{Value as V, json};
use std::io::{self, BufRead, Write};
fn main() {
    let mut bank = V::Null;
    let mut state = V::Null;
    let mut bank_hash = String::new();
    let stdin = io::stdin();
    let mut out = io::BufWriter::new(io::stdout().lock());
    for line in stdin.lock().lines() {
        let response = (|| -> Result<V, String> {
            let request: V = serde_json::from_str(&line.map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            match request["op"].as_str().unwrap_or("") {
                "init" => {
                    let b = request["bank"].clone();
                    let next = create(&b)?;
                    bank_hash = hash(&b);
                    bank = b;
                    state = next;
                    Ok(json!({"state":state}))
                }
                "advance" => {
                    let next = advance_frozen(&state, &request["event"], &bank, &bank_hash)?;
                    state = next;
                    Ok(json!({"state":state}))
                }
                "restore" => {
                    let b = request["bank"].clone();
                    bank_check(&b)?;
                    assert_state(&request["state"], &b)?;
                    bank_hash = hash(&b);
                    bank = b;
                    state = request["state"].clone();
                    Ok(json!({"state":state}))
                }
                "batch" => {
                    bank_check(&bank)?;
                    assert_state(&state, &bank)?;
                    let mut next = state.clone();
                    for event in request["events"].as_array().ok_or("EVENTS")? {
                        next = advance_frozen(&next, event, &bank, &bank_hash)?;
                    }
                    state = next;
                    Ok(json!({"state":state}))
                }
                "report" => Ok(json!({"report":report(&state,&bank,&request["at"])?})),
                _ => Err("UNKNOWN_OPERATION".into()),
            }
        })();
        let response = match response {
            Ok(v) => v,
            Err(e) => json!({"error":e}),
        };
        writeln!(out, "{response}").unwrap();
        out.flush().unwrap();
    }
}
