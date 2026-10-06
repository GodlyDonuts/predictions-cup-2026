// data/study-canonical-entry-exit-20261003.mjs — canonical entry/exit diagnostic.
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::{js_stringify, sha256};
use serde_json::{json, Value as V};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

fn sha(b: &[u8]) -> String { sha256(b) }
fn rt() -> PathBuf {
    Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default()).join("SigTradingCup").join("paper-runtime")
}
fn now_iso() -> String {
    ms_to_iso(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64).unwrap_or(0.0))
}
fn read_json(f: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(f).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn complete(file: &Path) -> Result<Vec<u8>, String> {
    let b = std::fs::read(file).map_err(|e| e.to_string())?;
    let end = b.iter().rposition(|x| *x == b'\n').map(|p| p + 1).unwrap_or(0);
    Ok(b[..end].to_vec())
}
fn rows(b: &[u8]) -> Result<Vec<V>, String> {
    if b.is_empty() { return Ok(Vec::new()); }
    String::from_utf8_lossy(b).trim_end().split('\n')
        .map(|l| serde_json::from_str(l).map_err(|e| e.to_string())).collect()
}

pub fn run(root: &Path, argv: &[String]) -> Result<V, String> {
    let r = rt();
    let a = read_json(&r.join("canonical-capacity-active.json"))?;
    let dir = r.join("canonical-capacity-cohorts").join(a["cohort"].as_str().unwrap_or(""));
    let bank_bytes = std::fs::read(dir.join("bank.json")).map_err(|e| e.to_string())?;
    let bank: V = serde_json::from_slice(&bank_bytes).map_err(|e| e.to_string())?;
    if baal_collector::model::digest(&bank) != a["bankHash"].as_str().unwrap_or("") {
        return Err("Bank identity mismatch".into());
    }
    let capacity_bytes = complete(&dir.join("journal.jsonl"))?;
    let capacity = rows(&capacity_bytes)?;
    let mut previous = a["bankHash"].as_str().unwrap_or("").to_string();
    let mut sequence = 0i64;
    for row in &capacity {
        let hash = row["hash"].as_str().unwrap_or("").to_string();
        let mut body = row.clone();
        if let Some(o) = body.as_object_mut() { o.remove("hash"); }
        sequence += 1;
        if body["sequence"].as_i64() != Some(sequence)
            || body["previous"].as_str().unwrap_or("") != previous
            || body["bankHash"].as_str().unwrap_or("") != a["bankHash"].as_str().unwrap_or("")
            || baal_collector::model::digest(&body) != hash {
            return Err("Capacity chain mismatch".into());
        }
        previous = hash;
    }
    let tape_bytes = complete(&r.join("anonymous-trades.jsonl"))?;
    let pages = rows(&tape_bytes)?;
    previous = String::new();
    sequence = 0;
    for row in &pages {
        let hash = row["hash"].as_str().unwrap_or("").to_string();
        let mut body = row.clone();
        if let Some(o) = body.as_object_mut() { o.remove("hash"); }
        sequence += 1;
        if body["sequence"].as_i64() != Some(sequence)
            || body["previous"].as_str().unwrap_or("") != previous
            || sha(js_stringify(&body).as_bytes()) != hash {
            return Err("Tape chain mismatch".into());
        }
        previous = hash;
    }
    let source_bytes = complete(&r.join("public-books.jsonl"))?;
    let prefix_len = bank["sourcePrefixBytes"].as_u64().unwrap_or(0) as usize;
    if sha(&source_bytes[..prefix_len]) != bank["sourcePrefixHash"].as_str().unwrap_or("") {
        return Err("Original prefix mismatch".into());
    }
    let wanted: HashSet<i64> = capacity.iter()
        .map(|x| x["pair"]["sourceSequence"].as_i64().unwrap_or(0)).collect();
    let mut frames = serde_json::Map::new();
    let mut s = baal_collector::book_publication_v2::create_book_publication_state(
        &read_json(&root.join("live-paper/price-universe.json"))?)?;
    for row in rows(&source_bytes)? {
        s = baal_collector::book_publication_v2::apply_book_publication(&s, &row)?;
        let sseq = s["sequence"].as_i64().unwrap_or(0);
        if wanted.contains(&sseq) {
            frames.insert(sseq.to_string(), json!({"hash": s["hash"],
                "books": bank["review"]["legs"].as_array().cloned().unwrap_or_default().iter()
                    .map(|l| s["books"][format!("{}:YES", l["exchangeId"].as_str().unwrap_or(""))].clone())
                    .collect::<Vec<V>>()}));
        }
    }
    let mut actual: Vec<V> = Vec::new();
    for row in &capacity {
        let frame = match frames.get(&row["pair"]["sourceSequence"].as_i64().unwrap_or(0).to_string()) {
            Some(f) => f.clone(),
            None => return Err("Frame binding mismatch".into()),
        };
        if frame["hash"] != row["pair"]["sourceHash"] {
            return Err("Frame binding mismatch".into());
        }
        let books = frame["books"].as_array().cloned().unwrap_or_default();
        let versions = row["pair"]["versions"].as_array().cloned().unwrap_or_default();
        for (i, b) in books.iter().enumerate() {
            // JS `b?.versionKey !== row.pair.versions[i]` — absent key on either side is
            // `undefined` in JS; undefined !== any JSON value but equals undefined.
            let bv = b.get("versionKey").cloned();
            let ev = versions.get(i).cloned();
            match (bv, ev) {
                (None, None) => {}
                (Some(x), Some(y)) if x == y => {}
                _ => return Err("Frame binding mismatch".into()),
            }
        }
        let expected = baal_collector::canonical_no_capacity::evaluate_canonical_no_capacity(
            &bank["review"], &books,
            if row["accountEligibility"].is_null() { None } else { Some(&row["accountEligibility"]) },
            row["at"].as_str().unwrap_or(""), &bank["policy"])?;
        if baal_collector::model::digest(&expected) != baal_collector::model::digest(&row["result"]) {
            return Err("Original frame result replay mismatch".into());
        }
        actual.push(json!({"at": row["at"], "sourceSequence": row["pair"]["sourceSequence"],
            "sourceHash": row["pair"]["sourceHash"], "versions": row["pair"]["versions"],
            "books": books, "accountEligibility": row["accountEligibility"],
            "result": row["result"]}));
    }
    let mut trades: Vec<V> = Vec::new();
    for row in &pages {
        for t in row["event"]["trades"].as_array().cloned().unwrap_or_default() {
            let mut o = t.as_object().cloned().unwrap_or_default();
            o.insert("tournamentId".into(), row["event"]["tournamentId"].clone());
            o.insert("marketId".into(), row["event"]["marketId"].clone());
            o.insert("exchangeId".into(), row["event"]["exchangeId"].clone());
            o.insert("firstAvailableAt".into(), row["event"]["receivedAt"].clone());
            trades.push(V::Object(o));
        }
    }
    let mut result = json!({"at": now_iso(), "cohort": a["cohort"], "bankHash": a["bankHash"],
        "sources": {"publicBooks": {"bytes": source_bytes.len(), "sha256": sha(&source_bytes),
            "sequence": s["sequence"], "tailHash": s["hash"]},
            "capacity": {"bytes": capacity_bytes.len(), "sha256": sha(&capacity_bytes),
                "rows": capacity.len()},
            "tradeTape": {"bytes": tape_bytes.len(), "sha256": sha(&tape_bytes),
                "pages": pages.len(), "trades": trades.len()}}});
    let spread = baal_collector::canonical_entry_exit_study::study_capacity_paths(&actual, &trades, None)?
        .as_object().cloned().unwrap_or_default();
    if let Some(ro) = result.as_object_mut() {
        for (k, v) in spread { ro.insert(k, v); }
    }
    let output = argv.first().cloned()
        .unwrap_or_else(|| "data/canonical-entry-exit-diagnostic-20261003.json".to_string());
    std::fs::write(root.join(&output),
        serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    println!("{}", js_stringify(&json!({"output": output, "at": result["at"],
        "frames": result["frames"], "usableFrames": result["usableFrames"],
        "positiveAnchors": result["positiveAnchors"],
        "anchorsWithLaterCompleteDepth": result["anchorsWithLaterCompleteDepth"],
        "positiveLaterDepthMarks": result["anchorsWithAnyPositiveLaterDepthMark"],
        "makerBothLegsThrough": result["makerBothLegsThrough"],
        "makerOneLegThrough": result["makerOneLegThrough"],
        "profitabilityEstablished": false})));
    Ok(result)
}
