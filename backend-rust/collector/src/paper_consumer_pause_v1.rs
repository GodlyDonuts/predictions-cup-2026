// live-paper/paper-consumer-pause-v1.mjs — pure engineering-pause predicate.
// The caller verifies/writes the boundary and retains the original frozen bank,
// ledger and source before using this result. Every failure is caught and
// returned as a rejection decision; this function never throws.
use baal_paper::fixed::{big_amount, instant};
use baal_service_runtime::json_text::{js_int, js_truthy};
use num_bigint::BigInt;
use serde_json::{json, Value as V};

// execution/ledger.mjs payloadHash: sorted-key canonical JSON sha256.
fn payload_hash(v: &V) -> Result<String, String> {
    baal_service_runtime::ledger::payload_hash(v).map_err(|e| e.message)
}
// JS === on JSON values: numbers compare by value; everything else structurally.
fn js_eq(a: &V, b: &V) -> bool {
    if let (Some(x), Some(y)) = (a.as_f64(), b.as_f64()) { return x == y; }
    a == b
}
// A hashable value that must exist: canonicalJson(undefined) throws
// 'Canonical JSON requires finite JSON values' in JS (non-prefixed -> INVALID).
fn req(v: Option<&V>) -> Result<V, String> {
    v.cloned().ok_or_else(|| "INVALID_INPUT: Canonical JSON requires finite JSON values".to_string())
}

fn is_hex(v: &V) -> bool {
    v.as_str().is_some_and(|x| x.len() == 64
        && x.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
}
fn need(ok: bool, c: &str) -> Result<(), String> {
    if !ok { return Err(format!("PAPER_CONSUMER_PAUSE_{}", c)); } Ok(())
}
// rows=v=>Array.isArray(v)?v:Object.values(v||{}) — JS Object.values also
// accepts strings (char per index); numbers/bools give [].
fn rows(v: &V) -> Vec<V> {
    match v {
        V::Array(a) => a.clone(),
        V::Object(m) => m.values().cloned().collect(),
        V::String(s) => s.chars().map(|c| json!(c.to_string())).collect(),
        _ => Vec::new(),
    }
}
// Object.entries: object members in order; array/string indexed members; else
// []. Null/undefined throw in JS but are rejected by earlier checks here.
fn entries(v: &V) -> Vec<(String, V)> {
    match v {
        V::Object(m) => m.iter().map(|(k, x)| (k.clone(), x.clone())).collect(),
        V::Array(a) => a.iter().enumerate().map(|(i, x)| (i.to_string(), x.clone())).collect(),
        V::String(s) => s.chars().enumerate().map(|(i, c)| (i.to_string(), json!(c.to_string()))).collect(),
        _ => Vec::new(),
    }
}
// Object.keys(v).length for every JSON shape Object.keys accepts.
fn key_count(v: &V) -> usize {
    match v {
        V::Object(m) => m.len(),
        V::Array(a) => a.len(),
        V::String(s) => s.chars().count(),
        _ => 0,
    }
}
fn normal() -> V {
    json!({"schemaVersion":1,"mode":"paper_consumer_engineering_pause_predicate_v1",
        "paused":false,"consumeEnabled":true,"focusEnabled":true,
        "executionAuthority":false,"officialReconciliationAffected":false,
        "marketCaptureAffected":false,"accountingReset":false})
}

fn decide(input: &V) -> Result<V, String> {
    let p = &input["pauseRecord"];
    let consumer_kind = &input["consumerKind"];
    let cohort = &input["cohort"];
    let bank_hash = &input["bankHash"];
    let report = &input["report"];
    let journal_anchor = &input["journalAnchor"];
    let source_anchor = &input["sourceAnchor"];
    let accounting_verified = input["accountingVerified"].as_bool().unwrap_or(false);
    let now = &input["now"];

    need(consumer_kind.as_str() == Some("counterfactual_paper"), "COUNTERFACTUAL_ONLY")?;
    need(js_int(&p["schemaVersion"]) == Some(1)
        && p["owner"].as_str() == Some("Codex")
        && js_eq(&p["cohort"], cohort)
        && js_eq(&p["bankHash"], bank_hash)
        && is_hex(bank_hash)
        && p["reason"].as_str().is_some_and(|s| !s.is_empty() && s.encode_utf16().count() <= 256)
        && ["retire_verified_flat", "pause_retain_inventory"].contains(&p["policy"].as_str().unwrap_or("")),
        "OWNER_SCOPE")?;
    need(instant(p["createdAt"].as_str().unwrap_or(""))? <= instant(now.as_str().unwrap_or(""))?
        && p["bankPreserved"] == V::Bool(true)
        && p["journalPreserved"] == V::Bool(true)
        && p["sourcePreserved"] == V::Bool(true)
        && p["resumeRequiresFullOriginalReconciliation"] == V::Bool(true),
        "PRESERVATION")?;
    need(accounting_verified
        && js_eq(&report["bankHash"], bank_hash)
        && is_hex(&p["reportHash"])
        && payload_hash(report)? == p["reportHash"].as_str().unwrap_or("")
        && report["executionAuthority"] == V::Bool(false)
        && report["unknownAppendAcceptance"] != V::Bool(true),
        "VERIFIED_ACCOUNTING")?;
    need(js_truthy(Some(journal_anchor))
        && js_truthy(Some(source_anchor))
        && payload_hash(journal_anchor)? == payload_hash(&req(p.get("journalAnchor"))?)?
        && payload_hash(source_anchor)? == payload_hash(&req(p.get("sourceAnchor"))?)?
        && js_int(&journal_anchor["sequence"]).is_some_and(|s| s > 0)
        && is_hex(&journal_anchor["hash"])
        && js_int(&source_anchor["bytes"]).is_some_and(|b| b > 0)
        && is_hex(&source_anchor["prefixHash"]),
        "ORIGINAL_FRONTIERS")?;
    // report.wallets && Object.keys(report.wallets).length>0
    need(js_truthy(report.get("wallets")) && key_count(&report["wallets"]) > 0,
        "WALLETS")?;

    let mut retained_risk: Vec<V> = Vec::new();
    for (case_id, w) in entries(&report["wallets"]) {
        need(js_truthy(Some(&w))
            && w.get("positions").is_some()
            && w.get("pending").is_some(),
            "ACCOUNTING_FIELDS")?;
        let positions = rows(&w["positions"]);
        let pending = rows(&w["pending"]);
        need(positions.iter().all(|v| js_truthy(Some(v))
            && js_int(&v["quantity"]).is_some_and(|q| q > 0))
            && pending.iter().all(|v| js_truthy(Some(v)) && (v.is_object() || v.is_array())),
            "RISK_SCHEMA")?;
        // reserved=w.reservedCost??w.reserved??null
        let reserved = if w.get("reservedCost").is_some_and(|v| !v.is_null()) {
            w["reservedCost"].clone()
        } else if w.get("reserved").is_some_and(|v| !v.is_null()) {
            w["reserved"].clone()
        } else { V::Null };
        need(!reserved.is_null()
            && big_amount(reserved.as_str().unwrap_or(""))? >= BigInt::from(0),
            "RESERVATION")?;
        retained_risk.push(json!({
            "caseId":case_id,
            "positionCount":positions.len(),
            "pendingCount":pending.len(),
            "reservedCost":reserved,
            "positionsHash":payload_hash(&w["positions"])?,
            "pendingHash":payload_hash(&w["pending"])?,
            "walletHash":payload_hash(&w)?}));
    }
    if p["policy"].as_str() == Some("retire_verified_flat") {
        for w in &retained_risk {
            let ok = w["positionCount"].as_i64() == Some(0)
                && w["pendingCount"].as_i64() == Some(0)
                && big_amount(w["reservedCost"].as_str().unwrap_or(""))? == BigInt::from(0);
            need(ok, "FLAT_REQUIRED")?;
        }
    }
    let mut out = normal().as_object().unwrap().clone();
    out.insert("paused".into(), V::Bool(true));
    out.insert("consumeEnabled".into(), V::Bool(false));
    out.insert("focusEnabled".into(), V::Bool(false));
    out.insert("reason".into(), p["reason"].clone());
    out.insert("policy".into(), p["policy"].clone());
    out.insert("pauseHash".into(), json!(payload_hash(p)?));
    out.insert("bankHash".into(), bank_hash.clone());
    out.insert("reportHash".into(), p["reportHash"].clone());
    out.insert("journalAnchor".into(), journal_anchor.clone());
    out.insert("sourceAnchor".into(), source_anchor.clone());
    out.insert("retainedRisk".into(), V::Array(retained_risk));
    out.insert("valuationStatus".into(), json!("frozen_paper_snapshot_not_current"));
    out.insert("riskRecordsRetained".into(), V::Bool(true));
    out.insert("resumeRequiresFullOriginalReconciliation".into(), V::Bool(true));
    Ok(V::Object(out))
}

/// paperConsumerPauseDecision({pauseRecord,consumerKind,cohort,bankHash,report,
/// journalAnchor,sourceAnchor,accountingVerified,now})
pub fn paper_consumer_pause_decision(input: &V) -> V {
    if input["pauseRecord"].is_null() {
        let mut out = normal().as_object().unwrap().clone();
        out.insert("reason".into(), json!("NO_EXPLICIT_PAPER_PAUSE"));
        return V::Object(out);
    }
    match decide(input) {
        Ok(v) => v,
        Err(e) => {
            // /^PAPER_CONSUMER_PAUSE_[A-Z_]+$/.test(e.message) ? e.message : 'PAPER_CONSUMER_PAUSE_INVALID'
            let code = if e.starts_with("PAPER_CONSUMER_PAUSE_") {
                let tail = &e["PAPER_CONSUMER_PAUSE_".len()..];
                if !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_uppercase() || b == b'_') { e } else { "PAPER_CONSUMER_PAUSE_INVALID".to_string() }
            } else { "PAPER_CONSUMER_PAUSE_INVALID".to_string() };
            let mut out = normal().as_object().unwrap().clone();
            out.insert("reason".into(), json!(code));
            out.insert("pauseRejected".into(), V::Bool(true));
            V::Object(out)
        }
    }
}
