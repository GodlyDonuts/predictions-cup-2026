// live-paper/bid-anchor-replay.mjs — frozen bid-anchor cohort replay over the
// complete prefix of the durable book publication stream.
// (The module's argv CLI entry is not ported; no sibling .rs CLI pattern exists.)

use baal_service_runtime::json_text::{digest, sha256};
use crate::bid_anchor::{advance_bid_anchor, bid_anchor_report, create_bid_anchor_state};
use crate::book_publication::{apply_book_publication, create_book_publication_state};
use serde_json::{json, Value as V};
use std::fs;
use std::path::Path;

fn need(ok: bool, reason: &str) -> Result<(), String> {
    if !ok { return Err(format!("BID_ANCHOR_REPLAY_{reason}")); }
    Ok(())
}
fn js_num(v: &V) -> f64 {
    match v {
        V::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        V::Bool(b) => if *b { 1.0 } else { 0.0 },
        V::Null => 0.0,
        V::String(s) => {
            let t = s.trim();
            if t.is_empty() { 0.0 } else { t.parse::<f64>().unwrap_or(f64::NAN) }
        }
        _ => f64::NAN,
    }
}
/// JS Buffer.subarray end coercion: undefined→len, null/NaN→0, negative→len+i,
/// positive→min(len,i). `v` is the raw bank field (None = absent/undefined).
fn subarray_end(v: Option<&V>, len: usize) -> usize {
    match v {
        None => len,
        Some(V::Null) => 0,
        Some(x) => {
            let n = js_num(x);
            if n.is_nan() { return 0; }
            if !n.is_finite() { return if n > 0.0 { len } else { 0 }; }
            let i = n.trunc() as i64;
            if i < 0 { (len as i64 + i).max(0) as usize } else { (i as usize).min(len) }
        }
    }
}
/// JS `===` for JSON scalars (objects/arrays never equal).
fn strict_eq(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Null, V::Null) => true,
        (V::Bool(x), V::Bool(y)) => x == y,
        (V::Number(x), V::Number(y)) => x.as_f64() == y.as_f64(),
        (V::String(x), V::String(y)) => x == y,
        _ => false,
    }
}
fn js_str(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Number(n) => {
            let mut b = ryu_js::Buffer::new();
            b.format_finite(n.as_f64().unwrap_or(f64::NAN)).to_string()
        }
        V::Bool(b) => if *b { "true".into() } else { "false".into() },
        V::Null => "null".into(),
        V::Array(a) => a.iter().map(|x| match x { V::Null => String::new(), other => js_str(other) })
            .collect::<Vec<_>>().join(","),
        V::Object(_) => "[object Object]".into(),
    }
}

/// identity() — sha256 of each frozen source file, keyed by its literal path
/// relative to the live-paper directory supplied by the caller.
pub fn bid_anchor_replay_identity(live_paper_dir: &Path) -> Result<V, String> {
    let mut m = serde_json::Map::new();
    for (key, rel) in [
        ("./bid-anchor.mjs", "bid-anchor.mjs"),
        ("./bid-anchor-replay.mjs", "bid-anchor-replay.mjs"),
        ("./edge-depth-audit.mjs", "edge-depth-audit.mjs"),
        ("./model.mjs", "model.mjs"),
        ("./book-publication.mjs", "book-publication.mjs"),
        ("../paper/fixed.mjs", "../paper/fixed.mjs"),
        ("../feed/contract.mjs", "../feed/contract.mjs"),
        ("../execution/ledger.mjs", "../execution/ledger.mjs"),
    ] {
        m.insert(key.to_string(), json!(sha256(&fs::read(live_paper_dir.join(rel)).map_err(|e| e.to_string())?)));
    }
    Ok(V::Object(m))
}

pub fn replay_bid_anchor(bank: &V, bytes: &[u8]) -> Result<V, String> {
    let complete = bytes.iter().rposition(|b| *b == b'\n').map(|i| i + 1).unwrap_or(0);
    let prefix_len = subarray_end(bank.get("sourcePrefixBytes"), bytes.len());
    need((complete as f64) >= js_num(&bank["sourcePrefixBytes"])
        && sha256(&bytes[..prefix_len]) == bank["sourcePrefixHash"].as_str().unwrap_or(""), "PREFIX")?;
    let mut publication = create_book_publication_state(&bank["universe"])?;
    let mut state = create_bid_anchor_state(bank)?;
    let mut offset: usize = 0;
    let mut prefix_checked = false;
    let text = String::from_utf8_lossy(&bytes[..complete]).into_owned();
    let mut lines: Vec<&str> = text.split('\n').collect();
    lines.pop(); // .slice(0,-1)
    for line in lines {
        let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
        publication = apply_book_publication(&publication, &row)?;
        offset += line.len() + 1;
        if baal_service_runtime::json_text::js_int(&bank["sourcePrefixBytes"]) == Some(offset as i64) {
            need(strict_eq(&row["sequence"], &bank["sourcePrefixSequence"])
                && strict_eq(&row["hash"], &bank["sourcePrefixJournalHash"]), "PREFIX_CHAIN")?;
            prefix_checked = true;
        }
        advance_bid_anchor(&mut state, &row, bank)?;
    }
    need(prefix_checked
        && js_num(&publication["sequence"]) >= js_num(&bank["sourcePrefixSequence"]), "TRUNCATED")?;
    // Clock comes from the original last publication; wall time cannot relabel data.
    let last_text = text.trim_end();
    let last_line = last_text.split('\n').last().unwrap_or("");
    let last: V = serde_json::from_str(last_line).map_err(|e| e.to_string())?;
    let mut out = bid_anchor_report(&state, bank, &js_str(&last["at"]));
    out["stateHash"] = json!(digest(&state));
    out["sourceSequence"] = publication["sequence"].clone();
    out["sourceHash"] = publication["hash"].clone();
    out["sourceBytes"] = json!(complete);
    out["sourceBytesHash"] = json!(sha256(&bytes[..complete]));
    out["sourceClosed"] = publication["closed"].clone();
    out["incompleteTailBytes"] = json!(bytes.len() - complete);
    Ok(out)
}
