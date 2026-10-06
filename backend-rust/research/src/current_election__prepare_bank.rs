// research/current-election/prepare-bank.mjs — re-verify the Tennessee
// assessment reproduction, stage the next settlement bank (or apply it
// through the collector lock with --apply) and report.
use crate::current_election__assessment::assess_election;
use crate::files::{log_json, read_bytes, read_json, runtime_dir, sha256, to_fixed, write_json_pretty_wx};
use crate::js::{date_parse, js_string, num, require_that};
use baal_collector::breadth::validate_universe;
use baal_collector::model::digest;
use baal_execution::settlement_bank_transition::{
    apply_stopped_settlement_transition, validate_settlement_transition,
};
use serde_json::{json, Value as V};
use std::path::Path;

pub fn run(apply: bool) -> Result<V, String> {
    let runtime = runtime_dir()?;
    let base = Path::new("research/current-election");
    let a = read_json(&runtime.join("competition-activation.json"))?;
    let previous = read_json(&runtime.join("production-banks").join(format!(
        "{}.json",
        a["strategyBankHash"].as_str().unwrap_or("")
    )))?;
    let file = base.join("tennessee-20261004-v1.json");
    let record = read_json(&file)?;
    let d = read_json(&base.join("definition.json"))?;
    if sha256(&read_bytes(&base.join("definition.json"))?)
        != record["definitionHash"].as_str().unwrap_or("")
    {
        return Err("ASSESSMENT_DEFINITION_CHANGED".to_string());
    }
    let recomputed = assess_election(&json!({
        "poll": record["facts"]["poll"],
        "prior": record["prior"],
        "definition": d,
        "assessmentAt": record["assessment"]["assessedAt"],
        "electionAt": record["assessment"]["componentClocks"]["electionAt"],
    }))?;
    if digest(&recomputed) != digest(&record["assessment"]) {
        return Err("ASSESSMENT_NOT_REPRODUCIBLE".to_string());
    }
    let metadata = read_json(
        &runtime
            .join("production-metadata")
            .join("tennessee-current-assessment-20261004-v1")
            .join("363.json"),
    )?;
    if metadata["marketId"].as_str() != Some("363")
        || metadata["rootHash"] != previous["contracts"]["363"]["rootHash"]
        || metadata["root"]["contract_details"]["winnerName"].as_str() != Some("Republican Party")
        || metadata["root"]["contract_details"]["electionDate"].as_str() != Some("2026-11-03")
    {
        return Err("CONTRACT_BINDING_CHANGED".to_string());
    }
    let universe = validate_universe(
        &read_json(Path::new("live-paper/price-universe.json"))?,
        a["tournamentId"].as_str().unwrap_or(""),
    )?;
    let mut bank = previous.clone();
    let assessment = record["assessment"].clone();
    bank["cutoff"] = assessment["assessedAt"].clone();
    bank["parentBankHash"] = a["strategyBankHash"].clone();
    let mut contract = previous["contracts"]["363"].as_object().cloned().unwrap_or_default();
    contract.insert("receivedAt".into(), metadata["receivedAt"].clone());
    contract.insert("sourceHash".into(), metadata["responseHash"].clone());
    bank["contracts"]["363"] = V::Object(contract);
    let sources = V::Array(
        record["facts"]["sources"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|s| json!({"url": s["url"], "contentHash": s["contentHash"]}))
            .collect(),
    );
    let research = json!({
        "id": record["id"],
        "predicateHash": previous["contracts"]["363"]["predicateHash"],
        "side": "YES",
        "lower": to_fixed((crate::js::js_number(assessment["ordinary"].get("lower")) * 1e6).floor() / 1e6, 6),
        "upper": to_fixed((crate::js::js_number(assessment["ordinary"].get("upper")) * 1e6).ceil() / 1e6, 6),
        "recordedAtMs": num(date_parse(&js_string(assessment.get("assessedAt").unwrap_or(&V::Null)))),
        "sourceAtMs": num(date_parse(&js_string(assessment.get("assessedAt").unwrap_or(&V::Null)))),
        "expiresAtMs": num(date_parse(&js_string(assessment.get("expiresAt").unwrap_or(&V::Null)))),
        "method": "subjective_range",
        "settlementReviewed": true,
        "flags": [],
        "sources": sources,
    });
    let mut found = false;
    if let Some(candidates) = bank.get_mut("candidates").and_then(V::as_array_mut) {
        for c in candidates.iter_mut() {
            if c.get("marketId").and_then(V::as_str) == Some("363") {
                if let Some(obj) = c.as_object_mut() {
                    obj.insert("research".into(), research.clone());
                }
                found = true;
                break;
            }
        }
    }
    // JS TypeError when no candidate with marketId '363' exists.
    require_that(
        found,
        "Cannot set properties of undefined (setting 'research')",
    )?;
    let file_hash = sha256(&read_bytes(&file)?);
    bank["assessment"] = json!({
        "id": record["id"],
        "manifestSha256": file_hash,
        "definitionHash": record["definitionHash"],
        "modelHash": record["prior"]["modelHash"],
        "componentClocks": assessment["componentClocks"],
        "ordinary": [assessment["ordinary"]["lower"], assessment["ordinary"]["upper"]],
        "broaderStress": [assessment["stress"]["lower"], assessment["stress"]["upper"]],
        "sourceAtMeaning": record["sourceAtMeaning"],
        "assessmentCodeHash": sha256(&read_bytes(&base.join("assessment.mjs"))?),
    });
    bank["limitations"] = json!([
        "New TN assessment has its own six-hour clock, preserving the original August field dates and source hashes; WV stays expired and KS remains unresolved.",
        "Subjective scenario probabilities are uncalibrated; broader error stresses contradict a small positive ordinary edge. No guaranteed win or near-term profitable exit.",
        "The same run, window, engine, groups, limits, original journals, cumulative purchases and inventory are retained. Legacy account inventory is excluded."
    ]);
    validate_settlement_transition(
        &a,
        &previous,
        &bank,
        &universe,
        &json!({}),
        &json!({}),
        crate::js::now_ms(),
    )?;
    if apply {
        let local = std::env::var("LOCALAPPDATA").map_err(|_| "LOCALAPPDATA unset".to_string())?;
        let applied = apply_stopped_settlement_transition(
            &runtime,
            Path::new(&local).join("SigTradingCup").join("bot.lock").as_path(),
            &bank,
            &universe,
            &|| crate::js::now_ms(),
        )?;
        let transition = read_json(Path::new(
            applied["recordFile"].as_str().unwrap_or_default(),
        ))?;
        write_json_pretty_wx(
            Path::new("data/tennessee-production-transition-20261004.json"),
            &json!({
                "at": transition["at"],
                "recordHash": applied["recordHash"],
                "runId": transition["runId"],
                "previousBankHash": transition["previousBankHash"],
                "nextBankHash": transition["nextBankHash"],
                "previousActivationHash": transition["previousActivationHash"],
                "nextActivationHash": transition["nextActivationHash"],
                "coreSequence": transition["coreSequence"],
                "coreHash": transition["coreHash"],
                "ledgerSequence": transition["ledgerSequence"],
                "ledgerHash": transition["ledgerHash"],
                "accountingReset": false,
                "runChanged": false,
                "windowChanged": false,
                "policyChanged": false,
                "ordersSubmittedByTransition": 0,
                "profitabilityEstablished": false,
            }),
        )?;
        let out = json!({
            "nextBankHash": applied["nextActivation"]["strategyBankHash"],
            "runId": applied["nextActivation"]["runId"],
            "accountingReset": false,
            "ordersSubmitted": 0,
        });
        log_json(&out);
        Ok(out)
    } else {
        let out_file = base.join("production-bank-20261004-v2.json");
        write_json_pretty_wx(&out_file, &bank)?;
        let cand = bank["candidates"]
            .as_array()
            .and_then(|a| a.iter().find(|c| c.get("marketId").and_then(V::as_str) == Some("363")))
            .cloned()
            .unwrap_or(V::Null);
        let out = json!({
            "stagedBankHash": digest(&bank),
            "assessmentId": record["id"],
            "lower": cand["research"]["lower"],
            "upper": cand["research"]["upper"],
            "ordersSubmitted": 0,
        });
        log_json(&out);
        Ok(out)
    }
}
