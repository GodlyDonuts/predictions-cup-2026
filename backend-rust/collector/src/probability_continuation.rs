// live-paper/probability-continuation.mjs — predecessor replay -> opening export.
use serde_json::{json, Value as V};
use std::fs;
use std::path::Path;

use baal_service_runtime::json_text::digest;
use crate::probability_receipt_host_v2::{cohort_ok, js_eq, next_line_end, rows, sha};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok {
        return Err(format!("PROBABILITY_CONTINUATION_{code}"));
    }
    Ok(())
}

const ORIGINAL_FILES: [&str; 8] = [
    "./probability-receipt-host.mjs",
    "./probability-receipt.mjs",
    "./probability-paper.mjs",
    "./model.mjs",
    "./book-publication.mjs",
    "../paper/fixed.mjs",
    "../feed/contract.mjs",
    "../execution/ledger.mjs",
];

// Replay the predecessor with its recorded receipt clock and account exclusions.
// Today's account cache must never change yesterday's accounting.
/// exportProbabilityOpening({runtimeDir,cohort}) — the `bank.json`/`journal.jsonl`
/// replay is read-only; `source_dir` is the live-paper module directory used by
/// the JS `new URL(p, import.meta.url)` code-identity reads.
pub fn export_probability_opening(
    runtime_dir: &Path,
    cohort: &str,
    source_dir: &Path,
) -> Result<V, String> {
    need(runtime_dir.is_absolute() && cohort_ok(cohort), "DIRECTORY")?;
    let dir = runtime_dir.join("probability-receipt-cohorts").join(cohort);
    need(!dir.join("worker.lock").exists(), "PREDECESSOR_RUNNING")?;
    let bank_bytes = fs::read(dir.join("bank.json")).map_err(|e| e.to_string())?;
    let prior_bank: V = serde_json::from_slice(&bank_bytes).map_err(|e| e.to_string())?;
    crate::probability_receipt::validate_probability_receipt_bank(&prior_bank)?;
    need(prior_bank["name"].as_str() == Some(cohort), "COHORT")?;
    let mut identity = serde_json::Map::new();
    for p in ORIGINAL_FILES {
        identity.insert(
            p.to_string(),
            json!(sha(&fs::read(source_dir.join(p)).map_err(|e| e.to_string())?)),
        );
    }
    need(
        digest(&prior_bank["codeIdentity"]) == digest(&V::Object(identity)),
        "FROZEN_CODE",
    )?;
    let bytes = fs::read(runtime_dir.join("public-books.jsonl")).map_err(|e| e.to_string())?;
    let records = rows(&bytes)?;
    let journal = fs::read(dir.join("journal.jsonl")).map_err(|e| e.to_string())?;
    let mut publication =
        crate::book_publication::create_book_publication_state(&prior_bank["universe"])?;
    let mut state = crate::probability_receipt::create_probability_receipt_state(&prior_bank)?;
    let mut offset = 0usize;
    let mut index = 0usize;
    while publication["sequence"].as_i64().unwrap_or(0)
        < prior_bank["sourcePrefixSequence"].as_i64().unwrap_or(0)
    {
        need(records.get(index).is_some(), "SOURCE_TRUNCATED")?;
        publication = crate::book_publication::apply_book_publication(&publication, &records[index])?;
        index += 1;
        offset = next_line_end(&bytes, offset);
    }
    need(
        offset as u64 == prior_bank["sourcePrefixBytes"].as_u64().unwrap_or(0)
            && sha(&bytes[..offset]) == prior_bank["sourcePrefixHash"].as_str().unwrap_or("")
            && publication["hash"].as_str() == prior_bank["sourcePrefixJournalHash"].as_str(),
        "ORIGINAL_PREFIX",
    )?;
    let mut sequence = 0u64;
    let mut previous = digest(&prior_bank);
    for record in rows(&journal)? {
        let mut body = record.clone();
        let hash = body
            .as_object_mut()
            .and_then(|m| m.shift_remove("hash"))
            .unwrap_or(V::Null);
        sequence += 1;
        need(
            body["sequence"].as_u64() == Some(sequence)
                && body["previous"].as_str() == Some(previous.as_str())
                && body["bankHash"].as_str() == Some(digest(&prior_bank).as_str())
                && digest(&body) == hash.as_str().unwrap_or(""),
            "JOURNAL",
        )?;
        if body["kind"].as_str() == Some("genesis") {
            need(
                sequence == 1 && body["stateHash"].as_str() == Some(digest(&state).as_str()),
                "GENESIS",
            )?;
        } else {
            need(
                body["kind"].as_str() == Some("batch")
                    && body["sourceSequence"].as_i64().unwrap_or(0)
                        > publication["sequence"].as_i64().unwrap_or(0),
                "CHECKPOINT",
            )?;
            while publication["sequence"].as_i64().unwrap_or(0)
                < body["sourceSequence"].as_i64().unwrap_or(0)
            {
                need(records.get(index).is_some(), "SOURCE_TRUNCATED")?;
                let row = records[index].clone();
                index += 1;
                publication =
                    crate::book_publication::apply_book_publication(&publication, &row)?;
                state = crate::probability_receipt::advance_receipt_probability(
                    &state,
                    &row,
                    &prior_bank,
                    body["availableAt"].as_str().unwrap_or(""),
                    &body["accountEligibility"],
                )?;
                offset = next_line_end(&bytes, offset);
            }
            need(
                js_eq(&body["sourceBytes"], &json!(offset))
                    && js_eq(&body["sourceSequence"], &publication["sequence"])
                    && js_eq(&body["sourceHash"], &publication["hash"])
                    && js_eq(&body["stateHash"], &json!(digest(&state))),
                "REPLAY",
            )?;
        }
        previous = hash.as_str().unwrap_or("").to_string();
    }
    need(sequence > 0 && offset == bytes.len(), "UNCONSUMED_SOURCE")?;
    Ok(json!({
        "schemaVersion": 1,
        "priorBank": prior_bank,
        "priorBankFileHash": sha(&bank_bytes),
        "state": state,
        "sourceBytes": offset,
        "sourceSequence": publication["sequence"],
        "sourceHash": publication["hash"],
        "sourcePrefixHash": sha(&bytes),
        "journalSequence": sequence,
        "journalHash": previous,
        "journalFileHash": sha(&journal),
        "journalBytes": journal.len(),
    }))
}

/// probabilityContinuationBundle(opening)
pub fn probability_continuation_bundle(opening: &V) -> Result<V, String> {
    let opening_bytes = format!(
        "{}\n",
        serde_json::to_string_pretty(opening).map_err(|e| e.to_string())?
    )
    .into_bytes();
    Ok(json!({
        "identities": opening["priorBank"]["identities"],
        "research": opening["priorBank"]["research"],
        "researchEvidence": opening["priorBank"]["researchEvidence"],
        "opening": opening,
        "accountingContinuation": {
            "accountingCarried": true,
            "priorCohort": opening["priorBank"]["name"],
            "priorBankHash": digest(&opening["priorBank"]),
            "priorBankFileHash": opening["priorBankFileHash"],
            "priorStateHash": digest(&opening["state"]),
            "priorJournalHash": opening["journalHash"],
            "priorJournalFileHash": opening["journalFileHash"],
            "priorJournalBytes": opening["journalBytes"],
            "openingFileHash": sha(&opening_bytes),
            "reason": "position_exposure_blocks_entry_but_not_reduce_only_paper_exit",
            "executionAuthority": false,
        },
    }))
}
