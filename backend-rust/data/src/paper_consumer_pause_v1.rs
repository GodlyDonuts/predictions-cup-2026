// live-paper/paper-consumer-pause-v1.mjs — pure engineering-pause predicate.
use baal_execution::core::payload_hash;
use baal_paper::fixed::{big_amount, instant};
use num_bigint::BigInt;
use serde_json::{json, Value as V};

fn hex(v: &V) -> bool {
    v.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
}
fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(format!("PAPER_CONSUMER_PAUSE_{code}")); }
    Ok(())
}
fn rows(v: &V) -> Vec<V> {
    if let Some(a) = v.as_array() { a.clone() } else { v.as_object().map(|o| o.values().cloned().collect()).unwrap_or_default() }
}
fn safe_int(v: &V) -> Option<i64> { baal_service_runtime::json_text::js_int(v) }

/// paperConsumerPauseDecision({pauseRecord=null,consumerKind,cohort,bankHash,report,journalAnchor,sourceAnchor,accountingVerified=false,now})
pub fn paper_consumer_pause_decision(o: &V) -> V {
    let pause_record = o.get("pauseRecord").cloned().unwrap_or(V::Null);
    let consumer_kind = o["consumerKind"].as_str().unwrap_or("");
    let cohort = o["cohort"].as_str().unwrap_or("");
    let bank_hash = o["bankHash"].as_str().unwrap_or("");
    let report = &o["report"];
    let journal_anchor = &o["journalAnchor"];
    let source_anchor = &o["sourceAnchor"];
    let accounting_verified = o["accountingVerified"].as_bool().unwrap_or(false);
    let now = o["now"].as_str().unwrap_or("");
    let normal = || {
        json!({"schemaVersion":1,"mode":"paper_consumer_engineering_pause_predicate_v1","paused":false,"consumeEnabled":true,"focusEnabled":true,"executionAuthority":false,"officialReconciliationAffected":false,"marketCaptureAffected":false,"accountingReset":false})
    };
    if pause_record.is_null() {
        let mut n = normal();
        n["reason"] = json!("NO_EXPLICIT_PAPER_PAUSE");
        return n;
    }
    let result = (|| -> Result<V, String> {
        need(consumer_kind == "counterfactual_paper", "COUNTERFACTUAL_ONLY")?;
        let p = &pause_record;
        need(
            p["schemaVersion"] == json!(1)
                && p["owner"] == json!("Codex")
                && p["cohort"] == json!(cohort)
                && p["bankHash"] == json!(bank_hash)
                && hex(&json!(bank_hash))
                && p["reason"].as_str().is_some_and(|r| !r.is_empty() && r.len() <= 256)
                && ["retire_verified_flat", "pause_retain_inventory"].contains(&p["policy"].as_str().unwrap_or("")),
            "OWNER_SCOPE",
        )?;
        // JS instant() throws a non-prefixed Error -> caught -> PAUSE_INVALID.
        let created_at = instant(p["createdAt"].as_str().unwrap_or("")).map_err(|e| e)?;
        let now_ms = instant(now).map_err(|e| e)?;
        need(
            created_at <= now_ms
                && p["bankPreserved"] == json!(true)
                && p["journalPreserved"] == json!(true)
                && p["sourcePreserved"] == json!(true)
                && p["resumeRequiresFullOriginalReconciliation"] == json!(true),
            "PRESERVATION",
        )?;
        need(
            accounting_verified
                && report["bankHash"] == json!(bank_hash)
                && hex(&p["reportHash"])
                && payload_hash(report).ok() == p["reportHash"].as_str().map(String::from)
                && report["executionAuthority"] == json!(false)
                && report["unknownAppendAcceptance"] != json!(true),
            "VERIFIED_ACCOUNTING",
        )?;
        need(
            journal_anchor.is_object()
                && source_anchor.is_object()
                && payload_hash(journal_anchor).ok() == payload_hash(&p["journalAnchor"]).ok()
                && payload_hash(source_anchor).ok() == payload_hash(&p["sourceAnchor"]).ok()
                && safe_int(&journal_anchor["sequence"]).is_some_and(|s| s > 0)
                && hex(&journal_anchor["hash"])
                && safe_int(&source_anchor["bytes"]).is_some_and(|b| b > 0)
                && hex(&source_anchor["prefixHash"]),
            "ORIGINAL_FRONTIERS",
        )?;
        need(report["wallets"].as_object().is_some_and(|w| !w.is_empty()), "WALLETS")?;
        let mut retained_risk = Vec::new();
        for (case_id, w) in report["wallets"].as_object().unwrap() {
            need(w.is_object() && !w.get("positions").is_none() && !w.get("pending").is_none(), "ACCOUNTING_FIELDS")?;
            let positions = rows(&w["positions"]);
            let pending = rows(&w["pending"]);
            need(
                positions.iter().all(|v| safe_int(&v["quantity"]).is_some_and(|q| q > 0))
                    && pending.iter().all(|v| v.is_object()),
                "RISK_SCHEMA",
            )?;
            let reserved = if !w["reservedCost"].is_null() { w["reservedCost"].clone() } else { w["reserved"].clone() };
            let reserved_amount = big_amount(reserved.as_str().unwrap_or(""))?;
            need(!reserved.is_null() && reserved_amount >= BigInt::from(0), "RESERVATION")?;
            retained_risk.push(json!({
                "caseId": case_id,
                "positionCount": positions.len(),
                "pendingCount": pending.len(),
                "reservedCost": reserved,
                "positionsHash": payload_hash(&w["positions"]).ok(),
                "pendingHash": payload_hash(&w["pending"]).ok(),
                "walletHash": payload_hash(w).ok(),
            }));
        }
        if p["policy"] == json!("retire_verified_flat") {
            need(
                retained_risk.iter().all(|w| w["positionCount"] == json!(0) && w["pendingCount"] == json!(0) && big_amount(w["reservedCost"].as_str().unwrap_or("")) == Ok(BigInt::from(0))),
                "FLAT_REQUIRED",
            )?;
        }
        let mut n = normal();
        for (k, v) in [
            ("paused", json!(true)),
            ("consumeEnabled", json!(false)),
            ("focusEnabled", json!(false)),
            ("reason", p["reason"].clone()),
            ("policy", p["policy"].clone()),
            ("pauseHash", json!(payload_hash(p)?)),
            ("bankHash", json!(bank_hash)),
            ("reportHash", p["reportHash"].clone()),
            ("journalAnchor", journal_anchor.clone()),
            ("sourceAnchor", source_anchor.clone()),
            ("retainedRisk", V::Array(retained_risk)),
            ("valuationStatus", json!("frozen_paper_snapshot_not_current")),
            ("riskRecordsRetained", json!(true)),
            ("resumeRequiresFullOriginalReconciliation", json!(true)),
        ] {
            n[k] = v;
        }
        Ok(n)
    })();
    match result {
        Ok(v) => v,
        Err(e) => {
            let mut n = normal();
            let re = regex_like_code(&e);
            n["reason"] = json!(if re { e } else { "PAPER_CONSUMER_PAUSE_INVALID".to_string() });
            n["pauseRejected"] = json!(true);
            n
        }
    }
}

fn regex_like_code(e: &str) -> bool {
    e.starts_with("PAPER_CONSUMER_PAUSE_")
        && e["PAPER_CONSUMER_PAUSE_".len()..].bytes().all(|b| b.is_ascii_uppercase() || b == b'_')
        && e.len() > "PAPER_CONSUMER_PAUSE_".len()
}
