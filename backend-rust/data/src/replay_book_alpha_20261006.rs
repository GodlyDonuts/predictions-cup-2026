// data/replay-book-alpha-20261006.mjs — book alpha replay diagnostic.
use crate::replay_dual_no_timelapse_20261005::sealed_end;
use crate::research__book_alpha_v1::{book_alpha_policy, book_features, causal_book_labels, fit_book_alpha,
    reset_book_epochs, score_book_alpha};
use baal_collector::book_publication_v2::{apply_book_publication, create_book_publication_state};
use baal_service_runtime::journal::{journal_lines, JournalReadOptions};
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Instant;

fn read(f: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(f).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn rt() -> PathBuf {
    Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default()).join("SigTradingCup").join("paper-runtime")
}
fn parse(s: &V) -> f64 { s.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN) }
fn now_iso() -> String {
    ms_to_iso(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64).unwrap_or(0.0))
}
fn write_pretty_excl(f: &Path, v: &V) -> Result<(), String> {
    use std::io::Write;
    let mut fd = std::fs::OpenOptions::new().write(true).create_new(true).open(f).map_err(|e| e.to_string())?;
    fd.write_all(format!("{}\n", serde_json::to_string_pretty(v).map_err(|e| e.to_string())?).as_bytes())
        .map_err(|e| e.to_string())
}

pub fn run(root: &Path) -> Result<V, String> {
    let output = root.join("data/book-alpha-results-20261006-v1b.json");
    if output.exists() { return Err("Frozen diagnostic already exists; never overwrite for tuning".into()); }
    let prior = read(&root.join("data/book-alpha-results-20261006-v1.json"))?;
    let file = rt().join("public-books.jsonl");
    let end = prior["source"]["prefixBytes"].as_u64().unwrap_or(0);
    let mut hash = Sha256::new();
    let universe = read(&root.join("live-paper/price-universe.json"))?;
    if sealed_end(&file)? < end { return Err("Original source truncated".into()); }
    let policy = book_alpha_policy();
    let mut publication = create_book_publication_state(&universe)?;
    let mut streams: Vec<Vec<V>> = Vec::new();
    // JS `active` is Map<marketId, streamArray> where each stream is also pushed to
    // `streams`; clearing the map leaves the old streams in `streams`.
    let mut active: indexmap::IndexMap<String, usize> = indexmap::IndexMap::new();
    let mut first: Option<f64> = None;
    let mut last = f64::NAN;
    let mut book_rows = 0i64;
    let started = Instant::now();
    for line in journal_lines(&file, &JournalReadOptions { end_byte: Some(end), ..Default::default() })? {
        hash.update(format!("{}\n", line).as_bytes());
        let row: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        publication = apply_book_publication(&publication, &row)?;
        if reset_book_epochs(&mut active, &row) { continue; }
        let b = &row["payload"]["book"];
        let at = parse(&row["at"]);
        let request_at = parse(&b["requestStartedAt"]);
        if at - parse(&b["receivedAt"]) > 20000.0 || request_at > at { continue; }
        let market_id = b["marketId"].as_str().unwrap_or("").to_string();
        let idx = match active.get(&market_id) {
            Some(&i) => i,
            None => {
                streams.push(Vec::new());
                active.insert(market_id.clone(), streams.len() - 1);
                streams.len() - 1
            }
        };
        let prior_row = streams[idx].last().cloned();
        let Some(features) = book_features(b, prior_row.as_ref()) else { continue; };
        let mut rec = features.as_object().cloned().unwrap_or_default();
        rec.insert("marketId".into(), json!(market_id));
        rec.insert("exchangeId".into(), b["exchangeId"].clone());
        rec.insert("at".into(), json!(at));
        rec.insert("requestAt".into(), json!(request_at));
        rec.insert("sequence".into(), row["sequence"].clone());
        streams[idx].push(V::Object(rec));
        if first.is_none() { first = Some(at); }
        last = at;
        book_rows += 1;
    }
    let first = match first { Some(f) => f, None => return Err("Missing source chronology".into()) };
    if last <= first { return Err("Missing source chronology".into()); }
    let cutoff = (first + (last - first) * policy["trainFraction"].as_f64().unwrap_or(0.0)).floor();
    let mut results: Vec<V> = Vec::new();
    for horizon_ms in policy["horizonsMs"].as_array().cloned().unwrap_or_default() {
        let labels = causal_book_labels(&streams, horizon_ms.as_f64().unwrap_or(0.0), &policy);
        let flat: Vec<V> = streams.iter().flatten().cloned().collect();
        match fit_book_alpha(&labels, cutoff, &policy) {
            Ok(model) => results.push(json!({"horizonMs": horizon_ms, "labels": labels.len() as i64,
                "model": model, "score": score_book_alpha(&model, &labels, Some(&flat))})),
            Err(e) => {
                if !e.contains("INSUFFICIENT_CHRONOLOGY") { return Err(e); }
                results.push(json!({"horizonMs": horizon_ms, "labels": labels.len() as i64,
                    "unavailable": "insufficient_chronological_labels"}));
            }
        }
    }
    let source_sha = hex_encode(&hash.finalize());
    if source_sha != prior["source"]["sha256"].as_str().unwrap_or("") {
        return Err("Original diagnostic source changed".into());
    }
    let out = json!({"schemaVersion": 1, "at": now_iso(), "policy": policy,
        "correction": {"priorResult": "book-alpha-results-20261006-v1.json", "priorUsable": false,
            "reason": "Global invalidation must clear every feature/label epoch; earlier driver deleted only a null map key.",
            "sameSourcePrefix": true, "policyTuned": false},
        "source": {"prefixBytes": end, "sha256": source_sha, "sequence": publication["sequence"],
            "tailHash": publication["hash"], "first": ms_to_iso(first), "last": ms_to_iso(last),
            "hours": (last - first) / 3600000.0, "bookRows": book_rows, "streams": streams.len() as i64},
        "cutoff": ms_to_iso(cutoff), "wallMs": started.elapsed().as_secs_f64() * 1000.0,
        "results": results, "accountRequests": 0, "acceptedOrders": 0, "realizedPnl": V::Null,
        "profitabilityEstablished": false,
        "limitations": ["Quote-only diagnostic has no funded wallet, order acceptance or queue proof; observed margins are not P&L.",
            "Only later independently requested depth-qualified receipts label outcomes; missing horizons/depth/limit fills are censored.",
            "Training uses labels already available at cutoff; subsequent fitting segment is chronological but this capture has been examined in other development research.",
            "Repeated receipts and overlapping labels/related elections are correlated, not independent statistical samples.",
            "All three fixed horizons are retained, not a selected winner; new prospective frozen evidence is required."]});
    write_pretty_excl(&output, &out)?;
    let summary: Vec<V> = out["results"].as_array().cloned().unwrap_or_default().iter().map(|r| {
        json!({"horizonMs": r["horizonMs"], "labels": r["labels"],
            "training": r["model"]["trainLabels"], "later": r["score"]["laterLabels"],
            "rmse": r["score"]["rmse"], "baseline": r["score"]["unchangedBidRmse"],
            "selected": r["score"]["selectedQuoteSignals"], "unavailable": r["unavailable"]})
    }).collect();
    println!("{}", baal_service_runtime::json_text::js_stringify(&json!({"hours": out["source"]["hours"],
        "bookRows": book_rows, "wallMs": out["wallMs"], "results": summary,
        "profitabilityEstablished": false})));
    Ok(out)
}
fn hex_encode(d: &[u8]) -> String { d.iter().map(|b| format!("{b:02x}")).collect() }
