// data/prepare-median-production-20261006.mjs — prepare (--activate) the
// median production bank and, on activation, hand off ownership.
use crate::publication_pack::{now_ms, read_json, runtime_dir, sha};
use baal_collector::model::digest;
use baal_execution::core::payload_hash;
use baal_execution::median_controller::{
    median_controller_identity, median_execution_policy, median_paper_policy,
};
use baal_execution::production::{validate_production_activation, validate_production_handoff};
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::canonical_json;
use baal_service_runtime::ledger::replay_journal;
use serde_json::{json, Value as V};
use std::path::Path;

const PAPER_HASH: &str = "fe61bfcee3e0fbc2bda5170de3fff0f3841c82d451529df2a20403a027098331";
fn assert_eq(a: &V, b: &V) -> Result<(), String> {
    if a == b { Ok(()) } else { Err("assert".into()) }
}
pub fn run(activate: bool) -> Result<V, String> {
    let runtime = runtime_dir();
    let read = |name: &str| read_json(Path::new(&format!("{runtime}/{name}")));
    let authority = read("median-human-activation-authority-20261006.json")?;
    let paper = read("median-paper-cohorts/baal-median-prospective-20261006-v1/bank.json")?;
    assert_eq(&json!(digest(&paper)), &json!(PAPER_HASH))?;
    assert_eq(&paper["policy"], &median_paper_policy())?;
    assert_eq(&authority["runId"], &json!("sig-pilot-median-20261006-human"))?;
    let prepared_name = "median-production-prepared-20261006.json";
    if !activate {
        let bank = json!({
            "schemaVersion": 1, "mode": "baal_historical_median_production_v1",
            "cutoff": ms_to_iso(now_ms() as f64), "tournamentId": paper["universe"]["tournamentId"],
            "universeHash": paper["universe"]["hash"],
            "paperBinding": {"bankHash": PAPER_HASH, "policyHash": digest(&median_paper_policy())},
            "sourcePrefix": paper["featurePrefix"], "policy": median_execution_policy(),
            "codeIdentity": median_controller_identity(Path::new("."))?,
        });
        let stored: V = serde_json::from_str(&canonical_json(&bank)?).map_err(|e| e.to_string())?;
        let bank_hash = digest(&stored);
        std::fs::create_dir_all(format!("{runtime}/production-banks")).map_err(|e| e.to_string())?;
        let file = format!("{runtime}/production-banks/{bank_hash}.json");
        if Path::new(&file).exists() { return Err("assert".into()); }
        atomic_json(Path::new(&file), &stored)?;
        assert_eq(&json!(digest(&read_json(Path::new(&file))?)), &json!(bank_hash))?;
        atomic_json(Path::new(&format!("{runtime}/{prepared_name}")), &json!({
            "schemaVersion": 1, "bankHash": bank_hash, "runId": authority["runId"],
            "authorityHash": payload_hash(&authority), "preparedAt": bank["cutoff"],
        }))?;
        return Ok(json!({"prepared": true, "bankHash": bank_hash, "executionEnabled": false}));
    }
    let prepared = read(prepared_name)?;
    let pointer = read("median-production-preflight-latest.json")?;
    assert_eq(&prepared["authorityHash"], &json!(payload_hash(&authority)))?;
    assert_eq(&pointer["activationAuthorityHash"], &prepared["authorityHash"])?;
    assert_eq(&pointer["runId"], &authority["runId"])?;
    let request = read("median-production-preflight-request.json")?;
    assert_eq(&pointer["requestHash"], &json!(payload_hash(&request)))?;
    assert_eq(&request["runId"], &authority["runId"])?;
    assert_eq(&request["activationAuthorityHash"], &prepared["authorityHash"])?;
    let result = read(pointer["file"].as_str().unwrap_or(""))?;
    let r = &result["receipt"];
    assert_eq(&result["runId"], &authority["runId"])?;
    assert_eq(&result["activationAuthorityHash"], &prepared["authorityHash"])?;
    assert_eq(&result["requestHash"], &pointer["requestHash"])?;
    assert_eq(&result["receiptHash"], &pointer["receiptHash"])?;
    assert_eq(&json!(payload_hash(r)), &pointer["receiptHash"])?;
    assert_eq(&r["complete"], &json!(true))?;
    let old = read("competition-activation.json")?;
    let old_handoff = read("production-handoff.json")?;
    validate_production_handoff(&old_handoff, &old)?;
    if old["runId"] == authority["runId"] { return Err("assert".into()); }
    let state_file = format!("{runtime}/competition-stats.json");
    if !Path::new(&state_file).exists() { return Err("Outgoing run status must be available".into()); }
    let stats = read_json(Path::new(&state_file))?;
    assert_eq(&stats["runId"], &old["runId"])?;
    assert_eq(&stats["openIntent"], &V::Null)?;
    assert_eq(&stats["unconfirmedIntents"], &json!(0))?;
    assert_eq(&json!(stats["positions"].as_object().map(|o| o.len()).unwrap_or(0)), &json!(0))?;
    let run_dir = format!("{}/competition-runs/{}", runtime, old["runId"].as_str().unwrap_or(""));
    let journal = Journal::open(Path::new(&format!("{run_dir}/core-journal.jsonl")))?;
    if let Some(last) = &journal.last {
        if !last["state"]["intents"].as_object().map(|m| m.values().all(|i| i["status"] == json!("closed"))).unwrap_or(false) {
            return Err("assert".into());
        }
        assert_eq(&json!(last["state"]["owned"].as_object().map(|o| o.len()).unwrap_or(0)), &json!(0))?;
    } else {
        let ledger = read(&format!("competition-runs/{}/execution-state.json", old["runId"].as_str().unwrap_or("")))?;
        let intent_file = format!("{run_dir}/execution-journal.jsonl");
        let raw = if Path::new(&intent_file).exists() {
            std::fs::read_to_string(&intent_file).map_err(|e| e.to_string())?
        } else {
            String::new()
        };
        let actual = replay_journal(&raw, old["tournamentId"].as_str()).map_err(|e| e.to_string())?;
        assert_eq(&ledger, &actual)?;
        assert_eq(&actual["sequence"], &json!(0))?;
        assert_eq(&actual["lastHash"], &json!(""))?;
        assert_eq(&json!(actual["intents"].as_object().map(|o| o.len()).unwrap_or(0)), &json!(0))?;
        assert_eq(&stats["ordersSubmitted"], &json!(0))?;
        assert_eq(&stats["dispatchAttempts"], &json!(0))?;
    }
    let at = ms_to_iso(now_ms() as f64);
    let at_ms = now_ms();
    if at_ms - baal_service_runtime::contract::parse_iso_ms(r["oldestReceivedAt"].as_str().unwrap_or("")).unwrap_or(0.0) as i64 > 120_000 {
        return Err("assert".into());
    }
    let h = json!({
        "schemaVersion": 3, "kind": "same_owner_explicit_human_activation",
        "executionOwner": "Codex", "ownershipTransferred": false,
        "handoffId": old["handoffId"], "authorityKind": old["authorityKind"],
        "assignmentCommentId": old["assignmentCommentId"], "ackCommentId": old["ackCommentId"],
        "inheritedOwnershipRecord": old_handoff, "inheritedActivation": old,
        "humanActivation": authority,
        "reconciliation": {
            "complete": true, "tournamentId": r["tournamentId"],
            "receiptHash": pointer["receiptHash"], "oldestReceivedAt": r["oldestReceivedAt"],
            "newestReceivedAt": r["newestReceivedAt"],
        },
        "reconciledAt": r["at"], "acknowledgedAt": at,
    });
    let mut a = old.clone();
    let ao = a.as_object_mut().unwrap();
    ao.insert("runId".into(), authority["runId"].clone());
    ao.insert("strategyId".into(), json!("historical-median-receipt-v1"));
    ao.insert("strategyBankHash".into(), prepared["bankHash"].clone());
    ao.insert("activationAuthority".into(), authority.clone());
    ao.insert("handoffHash".into(), json!(payload_hash(&h)));
    ao.insert("startedAt".into(), json!(at));
    ao.insert("entryExpiresAt".into(), json!(ms_to_iso(at_ms as f64 + 6.0 * 3_600_000.0)));
    validate_production_activation(&a)?;
    validate_production_handoff(&h, &a)?;
    let bank = read(&format!("production-banks/{}.json", prepared["bankHash"].as_str().unwrap_or("")))?;
    assert_eq(&bank["codeIdentity"], &median_controller_identity(Path::new("."))?)?;
    for (name, value) in [("competition-activation.json", &old), ("production-handoff.json", &old_handoff)] {
        let archive = format!("{runtime}/{name}.before-median-20261006");
        if Path::new(&archive).exists() { return Err("assert".into()); }
        atomic_json(Path::new(&archive), value)?;
    }
    atomic_json(Path::new(&format!("{runtime}/production-handoff.json")), &h)?;
    atomic_json(Path::new(&format!("{runtime}/competition-activation.json")), &a)?;
    Ok(json!({
        "activationConfigured": true, "runId": a["runId"],
        "entryExpiresAt": a["entryExpiresAt"], "bankHash": prepared["bankHash"],
        "ordersSubmitted": 0,
    }))
}
