// live-paper/probability-native-continuation.mjs — native prefix transport + replay.
use serde_json::{json, Value as V};
use std::fs;
use std::path::Path;

use baal_service_runtime::json_text::digest;
use crate::probability_receipt_host_v2::{
    cohort_ok, js_eq, next_line_end, rows, sha,
};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok {
        return Err(format!("NATIVE_CONTINUATION_{}", code));
    }
    Ok(())
}

/// exportNativeProbabilityOpening({runtimeDir,cohort,nativeBackend}) — async in JS
/// (await nativePublicationState); synchronous here.
pub fn export_native_probability_opening(
    runtime_dir: &Path,
    cohort: &str,
    native_backend: &V,
    source_dir: &Path,
) -> Result<V, String> {
    need(runtime_dir.is_absolute() && cohort_ok(cohort), "DIRECTORY")?;
    let dir = runtime_dir.join("probability-receipt-cohorts").join(cohort);
    need(!dir.join("worker.lock").exists(), "PREDECESSOR_RUNNING")?;
    let bank_bytes = fs::read(dir.join("bank.json")).map_err(|e| e.to_string())?;
    let prior_bank: V = serde_json::from_slice(&bank_bytes).map_err(|e| e.to_string())?;
    crate::probability_receipt_v2::validate_probability_receipt_bank(&prior_bank)?;
    need(
        prior_bank["name"].as_str() == Some(cohort)
            && digest(&prior_bank["codeIdentity"])
                == digest(&crate::probability_receipt_host_v2::probability_code_identity_v2(
                    source_dir,
                )?),
        "FROZEN_CODE",
    )?;
    let previous_opening_bytes =
        fs::read(dir.join("opening-state.json")).map_err(|e| e.to_string())?;
    need(
        sha(&previous_opening_bytes)
            == prior_bank["accountingContinuation"]["openingFileHash"]
                .as_str()
                .unwrap_or(""),
        "OPENING_CHANGED",
    )?;
    let source = runtime_dir.join("public-books.jsonl");
    let bytes = fs::read(&source).map_err(|e| e.to_string())?;
    let records = rows(&bytes)?;
    let journal = fs::read(dir.join("journal.jsonl")).map_err(|e| e.to_string())?;
    let native = crate::native_publication::native_publication_state(
        runtime_dir,
        native_backend,
        &prior_bank["universe"],
        &source,
        prior_bank["sourcePrefixBytes"].as_u64().unwrap_or(0),
        prior_bank["sourcePrefixHash"].as_str().unwrap_or(""),
        &dir.join("native-export"),
    )?;
    let mut publication = native["state"].clone();
    let previous_opening: V =
        serde_json::from_slice(&previous_opening_bytes).map_err(|e| e.to_string())?;
    let mut state = crate::probability_receipt_v2::create_probability_receipt_state(
        &prior_bank,
        &previous_opening,
    )?;
    let mut offset = prior_bank["sourcePrefixBytes"].as_u64().unwrap_or(0) as usize;
    let mut index = prior_bank["sourcePrefixSequence"].as_u64().unwrap_or(0) as usize;
    need(
        js_eq(&publication["sequence"], &json!(index))
            && js_eq(&publication["hash"], &prior_bank["sourcePrefixJournalHash"])
            && sha(&bytes[..offset.min(bytes.len())])
                == prior_bank["sourcePrefixHash"].as_str().unwrap_or(""),
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
                    crate::book_publication_v2::apply_book_publication(&publication, &row)?;
                crate::probability_receipt_v2::advance_receipt_probability(
                    &mut state,
                    &row,
                    &prior_bank,
                    body["availableAt"].as_str().unwrap_or(""),
                    Some(&body["accountEligibility"]),
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
    need(
        sha(&fs::read(&source).map_err(|e| e.to_string())?) == sha(&bytes),
        "SOURCE_CHANGED",
    )?;
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

/// nativeProbabilityContinuationBundle(opening, nativeBackend)
pub fn native_probability_continuation_bundle(
    opening: &V,
    native_backend: &V,
) -> Result<V, String> {
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
        "nativeBackend": native_backend,
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
            "reason": "native_prefix_transport_only_original_wallet_accounting_carried",
            "executionAuthority": false,
        },
    }))
}
