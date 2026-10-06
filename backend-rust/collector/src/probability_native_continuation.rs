// live-paper/probability-native-continuation.mjs — replays a v2 cohort through
// the native publication prefix to export a native continuation opening.
use baal_service_runtime::json_text::js_truthy;
use serde_json::{json, Value as V};
use std::path::Path;

fn sha(b: &[u8]) -> String { baal_service_runtime::json_text::sha256(b) }
fn digest(v: &V) -> String { baal_service_runtime::json_text::digest(v) }
fn need(ok: bool, c: &str) -> Result<(), String> {
    if !ok { return Err(format!("NATIVE_CONTINUATION_{}", c)); }
    Ok(())
}
fn rows(b: &[u8]) -> Result<Vec<V>, String> {
    need(!b.is_empty() && *b.last().unwrap() == b'\n', "NATIVE_CONTINUATION_PARTIAL_JOURNAL")?;
    let text = String::from_utf8(b.to_vec()).map_err(|e| e.to_string())?;
    let mut parts: Vec<&str> = text.split('\n').collect();
    parts.pop();
    parts.iter().map(|x| serde_json::from_str(x).map_err(|e| e.to_string())).collect()
}
fn cohort_ok(cohort: &str) -> bool {
    // /^[a-z][a-z0-9-]{1,63}$/
    cohort.len() >= 2 && cohort.len() <= 64
        && cohort.as_bytes()[0].is_ascii_lowercase()
        && cohort.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// exportNativeProbabilityOpening({runtimeDir, cohort, nativeBackend})
/// workspace_dir locates the JS tree the code identity is hashed from.
pub fn export_native_probability_opening(runtime_dir: &Path, cohort: &str,
    native_backend: &V, workspace_dir: &Path) -> Result<V, String> {
    need(runtime_dir.is_absolute() && cohort_ok(cohort), "DIRECTORY")?;
    let dir = runtime_dir.join("probability-receipt-cohorts").join(cohort);
    need(!dir.join("worker.lock").exists(), "PREDECESSOR_RUNNING")?;
    let bank_bytes = std::fs::read(dir.join("bank.json")).map_err(|e| e.to_string())?;
    let prior_bank: V = serde_json::from_slice(&bank_bytes).map_err(|e| e.to_string())?;
    crate::probability_receipt_v2::validate_probability_receipt_bank(&prior_bank)?;
    need(prior_bank["name"].as_str() == Some(cohort)
        && digest(&prior_bank["codeIdentity"])
            == digest(&crate::probability_receipt_host_v2::probability_code_identity_v2(workspace_dir)?),
        "FROZEN_CODE")?;
    let previous_opening_bytes = std::fs::read(dir.join("opening-state.json")).map_err(|e| e.to_string())?;
    need(sha(&previous_opening_bytes)
        == prior_bank["accountingContinuation"]["openingFileHash"].as_str().unwrap_or(""),
        "OPENING_CHANGED")?;
    let source = runtime_dir.join("public-books.jsonl");
    let bytes = std::fs::read(&source).map_err(|e| e.to_string())?;
    let records = rows(&bytes)?;
    let journal = std::fs::read(dir.join("journal.jsonl")).map_err(|e| e.to_string())?;
    let native = crate::native_publication::native_publication_state(runtime_dir, native_backend,
        &prior_bank["universe"], &source,
        prior_bank["sourcePrefixBytes"].as_i64().unwrap_or(0) as u64,
        prior_bank["sourcePrefixHash"].as_str().unwrap_or(""),
        &dir.join("native-export"))?;
    let mut publication = native["state"].clone();
    let opening: V = serde_json::from_slice(&previous_opening_bytes).map_err(|e| e.to_string())?;
    let mut state = crate::probability_receipt_v2::create_probability_receipt_state(&prior_bank, &opening)?;
    let mut offset = prior_bank["sourcePrefixBytes"].as_i64().unwrap_or(0) as usize;
    let mut index = prior_bank["sourcePrefixSequence"].as_i64().unwrap_or(0) as usize;
    need(publication["sequence"].as_i64().unwrap_or(-1) == index as i64
        && publication["hash"].as_str() == Some(prior_bank["sourcePrefixJournalHash"].as_str().unwrap_or(""))
        && sha(&bytes[..offset]) == prior_bank["sourcePrefixHash"].as_str().unwrap_or(""),
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
                publication = crate::book_publication_v2::apply_book_publication(&publication, row)?;
                crate::probability_receipt_v2::advance_receipt_probability(&mut state, row,
                    &prior_bank, body["availableAt"].as_str().unwrap_or(""),
                    if body["accountEligibility"].is_null() { None } else { Some(&body["accountEligibility"]) })?;
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
    need(sha(&std::fs::read(&source).map_err(|e| e.to_string())?) == sha(&bytes), "SOURCE_CHANGED")?;
    Ok(json!({"schemaVersion":1,"priorBank":prior_bank,
        "priorBankFileHash":sha(&bank_bytes),"state":state,"sourceBytes":offset as i64,
        "sourceSequence":publication["sequence"],"sourceHash":publication["hash"],
        "sourcePrefixHash":sha(&bytes),"journalSequence":sequence,"journalHash":previous,
        "journalFileHash":sha(&journal),"journalBytes":journal.len() as i64}))
}

/// nativeProbabilityContinuationBundle(opening, nativeBackend)
pub fn native_probability_continuation_bundle(opening: &V, native_backend: &V) -> Result<V, String> {
    let opening_bytes = format!("{}\n",
        serde_json::to_string_pretty(opening).map_err(|e| e.to_string())?).into_bytes();
    Ok(json!({"identities":opening["priorBank"]["identities"],
        "research":opening["priorBank"]["research"],
        "researchEvidence":opening["priorBank"]["researchEvidence"],
        "opening":opening,"nativeBackend":native_backend,
        "accountingContinuation":{"accountingCarried":true,
            "priorCohort":opening["priorBank"]["name"],
            "priorBankHash":digest(&opening["priorBank"]),
            "priorBankFileHash":opening["priorBankFileHash"],
            "priorStateHash":digest(&opening["state"]),
            "priorJournalHash":opening["journalHash"],
            "priorJournalFileHash":opening["journalFileHash"],
            "priorJournalBytes":opening["journalBytes"],
            "openingFileHash":sha(&opening_bytes),
            "reason":"native_prefix_transport_only_original_wallet_accounting_carried",
            "executionAuthority":false}}))
}
