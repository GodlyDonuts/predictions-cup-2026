// live-paper/basket-pair-audit.mjs — replays the verified source journal
// through both the original basket projection and the complete-pair successor
// and reports the comparison. Read-only; writes nothing.
use crate::basket_pair_capture::advance_complete_basket_record;
use crate::basket_watch::{advance_basket_record, basket_code_identity};
use crate::model::digest;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::journal::{journal_lines, JournalReadOptions};
use serde_json::{json, Map, Value as V};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

fn js_str(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Number(n) => {
            let mut b = ryu_js::Buffer::new();
            b.format_finite(n.as_f64().unwrap_or(f64::NAN)).to_string()
        }
        V::Bool(b) => if *b { "true".into() } else { "false".into() },
        V::Null => "null".into(),
        _ => "[object Object]".into(),
    }
}

/// auditBasketPairs(runtime) — `live_paper_dir` supplies the frozen JS sources
/// that basketCodeIdentity digests (JS resolves its own directory).
pub fn audit_basket_pairs(runtime: &Path, live_paper_dir: &Path) -> Result<V, String> {
    let bank: V = serde_json::from_str(
        &std::fs::read_to_string(runtime.join("basket-bank.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if digest(&bank["codeIdentity"]) != digest(&basket_code_identity(live_paper_dir)?) {
        return Err("ORIGINAL_BASKET_CODE_CHANGED".into());
    }
    // bank.pairs.map(...) — a missing/non-array pairs is a TypeError in JS.
    let bank_pairs = bank["pairs"]
        .as_array()
        .cloned()
        .ok_or("Cannot read properties of undefined (reading 'map')")?;
    let make = |bank: &V| -> V {
        let mut pairs = Map::new();
        for p in &bank_pairs {
            pairs.insert(js_str(&p["id"]), p["initialState"].clone());
        }
        json!({
            "bankHash": digest(bank),
            "sourceSequence": bank["sourceRecords"],
            "lastRecordHash": V::Null,
            "pairs": V::Object(pairs),
        })
    };
    let mut original = make(&bank);
    let mut successor = make(&bank);
    let mut previous = String::new();
    let mut sequence = 0.0f64;
    let mut last = f64::NEG_INFINITY;
    let mut observations: Vec<V> = Vec::new();
    let mut prefix = Sha256::new();
    let file = runtime.join("journal.jsonl");
    let size = std::fs::metadata(&file).map_err(|e| e.to_string())?.len();
    let source_records = bank["sourceRecords"].as_f64().unwrap_or(f64::NAN);
    for line in journal_lines(&file, &JournalReadOptions { end_byte: Some(size), ..Default::default() })? {
        let row: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        let mut body = row.clone();
        if let Some(m) = body.as_object_mut() {
            m.shift_remove("hash");
        }
        let at = row["at"].as_str().and_then(parse_iso_ms);
        let hash_ok = row["hash"].as_str().is_some_and(|h| h == digest(&body));
        if !(row["sequence"].as_f64() == Some(sequence + 1.0)
            && row["previous"].as_str() == Some(previous.as_str())
            && hash_ok
            && at.is_some_and(|a| a.is_finite() && a >= last))
        {
            return Err("BASKET_PAIR_AUDIT_SOURCE".into());
        }
        sequence = row["sequence"].as_f64().unwrap();
        previous = row["hash"].as_str().unwrap_or_default().to_string();
        last = at.unwrap();
        prefix.update(line.as_bytes());
        prefix.update(b"\n");
        if sequence == source_records {
            if format!("{:x}", prefix.clone().finalize()) != bank["sourceHash"].as_str().unwrap_or("") {
                return Err("BASKET_PAIR_ORIGIN_CHANGED".into());
            }
            original["lastRecordHash"] = json!(previous);
            successor["lastRecordHash"] = json!(previous);
        }
        if sequence > source_records {
            original = advance_basket_record(&original, &row, &bank)?["state"].clone();
            let next = advance_complete_basket_record(&successor, &row, &bank)?;
            successor = next["state"].clone();
            for o in next["observations"].as_array().cloned().unwrap_or_default() {
                observations.push(o);
            }
        }
    }
    if sequence < source_records {
        return Err("BASKET_PAIR_ORIGIN_MISSING".into());
    }
    let mut pairs = Vec::new();
    for p in &bank_pairs {
        let legs = p["bank"]["legs"]
            .as_array()
            .ok_or("Cannot read properties of undefined (reading 'map')")?;
        let pid = js_str(&p["id"]);
        let o = original["pairs"].get(&pid).ok_or("Cannot read properties of undefined (reading 'counts')")?;
        let s = successor["pairs"].get(&pid).ok_or("Cannot read properties of undefined (reading 'counts')")?;
        // p.bank.settlementProof.status — absent settlementProof is a TypeError;
        // an absent status serializes as a dropped key (undefined), not null.
        let sp = &p["bank"]["settlementProof"];
        if !sp.is_object() {
            return Err("Cannot read properties of undefined (reading 'status')".into());
        }
        let mut m = Map::new();
        m.insert("pairId".into(), p["id"].clone());
        m.insert(
            "marketIds".into(),
            json!(legs.iter().map(|l| l["marketId"].clone()).collect::<Vec<_>>()),
        );
        if let Some(st) = sp.get("status") {
            m.insert("proofStatus".into(), st.clone());
        }
        if let Some(c) = o.get("counts") {
            m.insert("original".into(), c.clone());
        }
        if let Some(c) = s.get("counts") {
            m.insert("completePair".into(), c.clone());
        }
        if let Some(lo) = s.get("lastObservation") {
            m.insert("lastCompleteObservation".into(), lo.clone());
        }
        pairs.push(V::Object(m));
    }
    let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0);
    Ok(json!({
        "schemaVersion": 1,
        "checkedAt": ms_to_iso(now_ms),
        "originalBankHash": digest(&bank),
        "source": {"sequence": sequence, "hash": previous, "bytes": size},
        "pairs": pairs,
        "observations": observations,
        "executionAuthority": false,
        "ordersSubmitted": 0,
        "networkRequests": 0,
        "fills": V::Null,
        "pnl": V::Null,
        "meaning": "Historical complete-pair diagnostic with original source/cutoff/policy. No original projection or journal is changed; a prospective successor requires its own later cutoff.",
    }))
}
