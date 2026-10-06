// live-paper/paper-book-continuation-frontier.mjs — seal unrelated-suffix proof.
use baal_execution::core::payload_hash;
use baal_service_runtime::journal::{journal_lines, JournalReadOptions};
use serde_json::Value as V;
use std::collections::BTreeSet;
use std::path::Path;

fn need(ok: bool, code: &str) -> Result<(), String> { if !ok { return Err(format!("PAPER_BOOK_CONTINUATION_{}", code)); } Ok(()) }
/// paperBookContinuationFrontier({file, endByte, priorState, legs})
pub fn paper_book_continuation_frontier(file: &Path, end_byte: u64, prior_state: &V, legs: &[V]) -> Result<V, String> {
    need(prior_state["sourceSequence"].as_i64().is_some_and(|x| x > 0), "FRONTIER")?;
    let prior_seq = prior_state["sourceSequence"].as_i64().unwrap();
    let mut sequence = 0i64;
    let mut previous = String::new();
    let mut found = false;
    let mut ignored = 0i64;
    let markets: BTreeSet<String> = legs.iter().filter_map(|l| l["marketId"].as_str().map(String::from)).collect();
    for line in journal_lines(file, &JournalReadOptions { end_byte: Some(end_byte), ..Default::default() })? {
        let row: V = serde_json::from_str(&line).map_err(|_| "PAPER_BOOK_CONTINUATION_CHAIN".to_string())?;
        let hash = row["hash"].as_str().unwrap_or("").to_string();
        let mut body = row.clone();
        body.as_object_mut().unwrap().remove("hash");
        sequence += 1;
        need(row["sequence"].as_i64() == Some(sequence) && row["previousHash"].as_str().unwrap_or("") == previous
            && payload_hash(&body).ok().as_deref() == Some(&hash), "CHAIN")?;
        previous = hash.clone();
        if sequence == prior_seq {
            need(hash == prior_state["sourceHash"].as_str().unwrap_or(""), "FRONTIER_CHANGED")?;
            found = true;
        }
        if sequence > prior_seq {
            let mid = &row["payload"]["book"]["marketId"];
            need(row["kind"].as_str() != Some("book") || !markets.contains(mid.as_str().unwrap_or("")), "UNCONSUMED_RELEVANT_BOOK")?;
            ignored += 1;
        }
    }
    need(found, "FRONTIER_TRUNCATED")?;
    Ok(serde_json::json!({"consumedSequence": prior_state["sourceSequence"], "consumedHash": prior_state["sourceHash"],
        "sealedSequence": sequence, "sealedHash": previous, "unrelatedOrBoundarySuffix": ignored}))
}
