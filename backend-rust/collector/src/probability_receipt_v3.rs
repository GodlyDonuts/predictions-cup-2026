// live-paper/probability-receipt-v3.mjs — native-transport continuation revision.
// This revision changes startup transport only; economics are re-exported from v2.
use serde_json::{json, Value as V};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(format!("PROBABILITY_NATIVE_{}", code)); }
    Ok(())
}
fn digest(v: &V) -> String { baal_service_runtime::json_text::digest(v) }
fn instant_of(v: &V) -> i64 { crate::model::instant(v.as_str().unwrap_or("")).unwrap_or(i64::MIN) }
fn hash(v: &V) -> bool {
    v.as_str().is_some_and(|s| s.len() == 64
        && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
}

/// validateProbabilityReceiptBank(bank) — v4 native continuation.
pub fn validate_probability_receipt_bank(bank: &V) -> Result<(), String> {
    need(bank["schemaVersion"].as_i64() == Some(4)
        && bank["mode"].as_str() == Some("source_probability_native_continuation_v3"), "BANK")?;
    let mut rewritten = bank.clone();
    rewritten["schemaVersion"] = json!(3);
    rewritten["mode"] = json!("source_probability_owned_risk_continuation_v2");
    crate::probability_receipt_v2::validate_probability_receipt_bank(&rewritten)?;
    let nb = &bank["nativeBackend"];
    need(nb["schemaVersion"].as_i64() == Some(1)
        && nb["mode"].as_str() == Some("native_public_book_prefix_v1")
        && ["binaryHash", "engineHash", "runtimeLibraryHash"].iter().all(|k| hash(&nb[k])),
        "NATIVE_IDENTITY")?;
    Ok(())
}

/// validateProbabilityOpening(opening, bank)
pub fn validate_probability_opening(opening: &V, bank: &V) -> Result<V, String> {
    let c = &bank["accountingContinuation"];
    need(opening["schemaVersion"].as_i64() == Some(1)
        && digest(&opening["priorBank"]) == c["priorBankHash"].as_str().unwrap_or("")
        && opening["priorBankFileHash"] == c["priorBankFileHash"]
        && opening["priorBank"]["name"] == c["priorCohort"]
        && opening["state"]["bankHash"] == c["priorBankHash"]
        && digest(&opening["state"]) == c["priorStateHash"].as_str().unwrap_or(""), "OPENING_IDENTITY")?;
    crate::probability_receipt_v2::validate_probability_receipt_bank(&opening["priorBank"])?;
    need(opening["sourceBytes"] == bank["sourcePrefixBytes"]
        && opening["sourceSequence"] == bank["sourcePrefixSequence"]
        && opening["sourceHash"] == bank["sourcePrefixJournalHash"]
        && opening["sourcePrefixHash"] == bank["sourcePrefixHash"]
        && opening["journalHash"] == c["priorJournalHash"]
        && opening["journalFileHash"] == c["priorJournalFileHash"]
        && opening["journalBytes"] == c["priorJournalBytes"], "OPENING_FRONTIER")?;
    need(["config", "research", "identities", "universe", "policy"].iter()
        .all(|k| digest(&opening["priorBank"][k]) == digest(&bank[k]))
        && instant_of(&opening["state"]["lastAvailableAt"]) <= instant_of(&bank["cutoff"]), "OPENING_SCOPE")?;
    let wallet_keys: String = opening["state"]["wallets"].as_object().map(|m| {
        let mut v: Vec<String> = m.keys().cloned().collect();
        v.sort();
        v.join("|")
    }).unwrap_or_default();
    need(wallet_keys == "1000|3000", "OPENING_WALLETS")?;
    for (latency, w) in opening["state"]["wallets"].as_object().cloned().unwrap_or_default() {
        let mut cfg = bank["config"].clone();
        cfg["latencyMs"] = json!(latency.parse::<i64>().unwrap_or(0));
        crate::model::assert_state(&w["state"], &cfg)?;
    }
    Ok(opening.clone())
}

/// createProbabilityReceiptState(bank, opening)
pub fn create_probability_receipt_state(bank: &V, opening: &V) -> Result<V, String> {
    validate_probability_receipt_bank(bank)?;
    validate_probability_opening(opening, bank)?;
    let mut state = opening["state"].clone();
    state["bankHash"] = json!(digest(bank));
    state["lastAvailableAt"] = bank["cutoff"].clone();
    state["barriers"] = json!({"global": bank["cutoff"], "markets": {}});
    let wallets: Vec<String> = state["wallets"].as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
    for k in wallets {
        let mut w = state["wallets"][&k].clone();
        crate::model::invalidate(&mut w["state"], None, bank["cutoff"].as_str().unwrap_or(""), "native_transport_continuation_boundary");
        if let Some(strategies) = w["state"]["strategies"].as_object_mut() {
            for st in strategies.values_mut() {
                let n = st["pending"].as_object().map(|p| p.len()).unwrap_or(0) as i64;
                st["expiredIntents"] = json!(st["expiredIntents"].as_i64().unwrap_or(0) + n);
                st["pending"] = json!({});
            }
        }
        state["wallets"][&k] = w;
    }
    Ok(state)
}

// Re-exports (JS: `export {...} from './probability-receipt-v2.mjs'`).
pub use crate::probability_receipt_v2::{
    probability_receipt_policy_v2, probability_account_eligibility, probability_account_gate,
    receipt_probability_gate, advance_receipt_probability, receipt_probability_report,
};
