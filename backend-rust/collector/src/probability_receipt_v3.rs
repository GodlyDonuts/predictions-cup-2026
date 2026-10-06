// live-paper/probability-receipt-v3.mjs — native-transport continuation adapter.
use baal_service_runtime::json_text::digest;
use serde_json::{json, Value as V};

pub use crate::probability_receipt_v2::{
    probability_account_eligibility, probability_account_gate, probability_receipt_policy,
    receipt_probability_gate, advance_receipt_probability, receipt_probability_report,
};

fn need(ok: bool, code: &str) -> Result<(), String> { if !ok { return Err(format!("PROBABILITY_NATIVE_{}", code)); } Ok(()) }
fn hash(v: &V) -> bool { v.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())) }
fn instant(v: &V) -> i64 { baal_paper::fixed::instant(v.as_str().unwrap_or("")).unwrap_or(i64::MIN) }
// This revision changes startup transport only. Economics, research, exposure,
// account projections and the original cash/basis/fill history are unchanged.
/// validateProbabilityReceiptBank(bank)
pub fn validate_probability_receipt_bank(bank: &V) -> Result<V, String> {
    need(bank["schemaVersion"].as_i64() == Some(4)
        && bank["mode"].as_str() == Some("source_probability_native_continuation_v3"), "BANK")?;
    let mut v2 = bank.clone();
    v2["schemaVersion"] = json!(3);
    v2["mode"] = json!("source_probability_owned_risk_continuation_v2");
    crate::probability_receipt_v2::validate_probability_receipt_bank(&v2)?;
    need(bank["nativeBackend"]["schemaVersion"].as_i64() == Some(1)
        && bank["nativeBackend"]["mode"].as_str() == Some("native_public_book_prefix_v1")
        && ["binaryHash", "engineHash", "runtimeLibraryHash"].iter().all(|k| hash(&bank["nativeBackend"][*k])), "NATIVE_IDENTITY")?;
    Ok(bank.clone())
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
        .all(|k| digest(&opening["priorBank"][*k]) == digest(&bank[*k]))
        && instant(&opening["state"]["lastAvailableAt"]) <= instant(&bank["cutoff"]), "OPENING_SCOPE")?;
    let mut wallet_keys: Vec<String> = opening["state"]["wallets"].as_object().map(|w| w.keys().cloned().collect()).unwrap_or_default();
    wallet_keys.sort();
    need(wallet_keys.join("|") == "1000|3000", "OPENING_WALLETS")?;
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
    let wallet_keys: Vec<String> = state["wallets"].as_object().map(|w| w.keys().cloned().collect()).unwrap_or_default();
    for k in wallet_keys {
        let mut w = state["wallets"][&k].clone();
        crate::model::invalidate(&mut w["state"], None, bank["cutoff"].as_str().unwrap_or(""), "native_transport_continuation_boundary");
        let st_keys: Vec<String> = w["state"]["strategies"].as_object().map(|s| s.keys().cloned().collect()).unwrap_or_default();
        for sk in st_keys {
            let pending_len = w["state"]["strategies"][&sk]["pending"].as_object().map(|p| p.len()).unwrap_or(0);
            w["state"]["strategies"][&sk]["expiredIntents"] =
                json!(w["state"]["strategies"][&sk]["expiredIntents"].as_i64().unwrap_or(0) + pending_len as i64);
            w["state"]["strategies"][&sk]["pending"] = json!({});
        }
        state["wallets"][&k] = w;
    }
    Ok(state)
}
