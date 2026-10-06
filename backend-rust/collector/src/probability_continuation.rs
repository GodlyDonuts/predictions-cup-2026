// live-paper/probability-continuation.mjs — replays a predecessor
// probability-receipt cohort with its recorded receipt clock and account
// exclusions, then exports the continuation opening. Today's account cache
// must never change yesterday's accounting.
use baal_service_runtime::json_text::js_truthy;
use serde_json::{json, Value as V};
use std::path::Path;

fn sha(b: &[u8]) -> String { baal_service_runtime::json_text::sha256(b) }
fn digest(v: &V) -> String { baal_service_runtime::json_text::digest(v) }
fn need(ok: bool, c: &str) -> Result<(), String> {
    if !ok { return Err(format!("PROBABILITY_CONTINUATION_{}", c)); }
    Ok(())
}
fn rows(b: &[u8]) -> Result<Vec<V>, String> {
    need(!b.is_empty() && *b.last().unwrap() == b'\n', "PROBABILITY_CONTINUATION_PARTIAL_JOURNAL")?;
    let text = String::from_utf8(b.to_vec()).map_err(|e| e.to_string())?;
    let mut parts: Vec<&str> = text.split('\n').collect();
    parts.pop();
    parts.iter().map(|x| serde_json::from_str(x).map_err(|e| e.to_string())).collect()
}
fn read_json(file: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(file).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn cohort_ok(cohort: &str) -> bool {
    // /^[a-z][a-z0-9-]{1,63}$/
    cohort.len() >= 2 && cohort.len() <= 64
        && cohort.as_bytes()[0].is_ascii_lowercase()
        && cohort.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}
/// originalFiles identity — sha256 of the predecessor code set.
fn identity(workspace_dir: &Path) -> Result<V, String> {
    let keys = ["./probability-receipt-host.mjs", "./probability-receipt.mjs",
        "./probability-paper.mjs", "./model.mjs", "./book-publication.mjs",
        "../paper/fixed.mjs", "../feed/contract.mjs", "../execution/ledger.mjs"];
    let files = ["live-paper/probability-receipt-host.mjs", "live-paper/probability-receipt.mjs",
        "live-paper/probability-paper.mjs", "live-paper/model.mjs", "live-paper/book-publication.mjs",
        "paper/fixed.mjs", "feed/contract.mjs", "execution/ledger.mjs"];
    let mut m = serde_json::Map::new();
    for (k, p) in keys.iter().zip(files.iter()) {
        m.insert(k.to_string(), json!(sha(&std::fs::read(workspace_dir.join(p)).map_err(|e| e.to_string())?)));
    }
    Ok(V::Object(m))
}

/// exportProbabilityOpening({runtimeDir, cohort}) — workspace_dir locates the
/// JS tree the code identity is hashed from (new URL(p, import.meta.url)).
pub fn export_probability_opening(runtime_dir: &Path, cohort: &str,
    workspace_dir: &Path) -> Result<V, String> {
    need(runtime_dir.is_absolute() && cohort_ok(cohort), "DIRECTORY")?;
    let dir = runtime_dir.join("probability-receipt-cohorts").join(cohort);
    need(!dir.join("worker.lock").exists(), "PREDECESSOR_RUNNING")?;
    let bank_bytes = std::fs::read(dir.join("bank.json")).map_err(|e| e.to_string())?;
    let prior_bank: V = serde_json::from_slice(&bank_bytes).map_err(|e| e.to_string())?;
    crate::probability_receipt::validate_probability_receipt_bank(&prior_bank)?;
    need(prior_bank["name"].as_str() == Some(cohort), "COHORT")?;
    need(digest(&prior_bank["codeIdentity"]) == digest(&identity(workspace_dir)?), "FROZEN_CODE")?;
    let source = runtime_dir.join("public-books.jsonl");
    let bytes = std::fs::read(&source).map_err(|e| e.to_string())?;
    let records = rows(&bytes)?;
    let journal = std::fs::read(dir.join("journal.jsonl")).map_err(|e| e.to_string())?;
    let mut publication = crate::book_publication::create_book_publication_state(&prior_bank["universe"])?;
    let mut state = crate::probability_receipt::create_probability_receipt_state(&prior_bank)?;
    let mut offset = 0usize;
    let mut index = 0usize;
    while publication["sequence"].as_i64().unwrap_or(0)
        < prior_bank["sourcePrefixSequence"].as_i64().unwrap_or(0) {
        need(index < records.len() && js_truthy(records.get(index)), "SOURCE_TRUNCATED")?;
        publication = crate::book_publication::apply_book_publication(&publication, &records[index])?;
        index += 1;
        offset = bytes[offset..].iter().position(|x| *x == b'\n').map(|p| p + offset + 1).unwrap_or(offset);
    }
    need(offset == prior_bank["sourcePrefixBytes"].as_i64().unwrap_or(-1) as usize
        && sha(&bytes[..offset]) == prior_bank["sourcePrefixHash"].as_str().unwrap_or("")
        && publication["hash"].as_str() == Some(prior_bank["sourcePrefixJournalHash"].as_str().unwrap_or("")),
        "ORIGINAL_PREFIX")?;
    let mut sequence = 0i64;
    let mut previous = digest(&prior_bank);
    for record in rows(&journal)? {
        let mut body = record;
        let hash = body.as_object_mut().and_then(|m| m.shift_remove("hash")).unwrap_or(V::Null);
        sequence += 1;
        need(body["sequence"].as_i64() == Some(sequence)
            && body["previous"].as_str() == Some(previous.as_str())
            && body["bankHash"].as_str() == Some(digest(&prior_bank).as_str())
            && hash.as_str() == Some(digest(&body).as_str()), "JOURNAL")?;
        if body["kind"].as_str() == Some("genesis") {
            need(sequence == 1 && body["stateHash"].as_str() == Some(digest(&state).as_str()),
                "GENESIS")?;
        } else {
            need(body["kind"].as_str() == Some("batch")
                && body["sourceSequence"].as_i64().unwrap_or(0)
                    > publication["sequence"].as_i64().unwrap_or(0), "CHECKPOINT")?;
            while publication["sequence"].as_i64().unwrap_or(0)
                < body["sourceSequence"].as_i64().unwrap_or(0) {
                need(index < records.len() && js_truthy(records.get(index)), "SOURCE_TRUNCATED")?;
                let row = &records[index];
                publication = crate::book_publication::apply_book_publication(&publication, row)?;
                state = crate::probability_receipt::advance_receipt_probability(&state, row,
                    &prior_bank, body["availableAt"].as_str().unwrap_or(""),
                    &body["accountEligibility"])?;
                index += 1;
                offset = bytes[offset..].iter().position(|x| *x == b'\n').map(|p| p + offset + 1).unwrap_or(offset);
            }
            need(body["sourceBytes"].as_i64() == Some(offset as i64)
                && body["sourceSequence"] == publication["sequence"]
                && body["sourceHash"] == publication["hash"]
                && body["stateHash"].as_str() == Some(digest(&state).as_str()), "REPLAY")?;
        }
        previous = hash.as_str().unwrap_or("").to_string();
    }
    need(sequence > 0 && offset == bytes.len(), "UNCONSUMED_SOURCE")?;
    Ok(json!({"schemaVersion":1,"priorBank":prior_bank,
        "priorBankFileHash":sha(&bank_bytes),"state":state,"sourceBytes":offset as i64,
        "sourceSequence":publication["sequence"],"sourceHash":publication["hash"],
        "sourcePrefixHash":sha(&bytes),"journalSequence":sequence,"journalHash":previous,
        "journalFileHash":sha(&journal),"journalBytes":journal.len() as i64}))
}

/// probabilityContinuationBundle(opening)
pub fn probability_continuation_bundle(opening: &V) -> Result<V, String> {
    let opening_bytes = format!("{}\n",
        serde_json::to_string_pretty(opening).map_err(|e| e.to_string())?).into_bytes();
    Ok(json!({"identities":opening["priorBank"]["identities"],
        "research":opening["priorBank"]["research"],
        "researchEvidence":opening["priorBank"]["researchEvidence"],
        "opening":opening,
        "accountingContinuation":{"accountingCarried":true,
            "priorCohort":opening["priorBank"]["name"],
            "priorBankHash":digest(&opening["priorBank"]),
            "priorBankFileHash":opening["priorBankFileHash"],
            "priorStateHash":digest(&opening["state"]),
            "priorJournalHash":opening["journalHash"],
            "priorJournalFileHash":opening["journalFileHash"],
            "priorJournalBytes":opening["journalBytes"],
            "openingFileHash":sha(&opening_bytes),
            "reason":"position_exposure_blocks_entry_but_not_reduce_only_paper_exit",
            "executionAuthority":false}}))
}
