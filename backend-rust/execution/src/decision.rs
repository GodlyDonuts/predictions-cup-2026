use baal_service_runtime::journal::Journal;
use serde_json::{json, Value as V};
use std::collections::BTreeMap;

// execution/decision-audit.mjs — durable entry-decision audit on top of the
// shared journal; no authority, no network.
const CODES: [&str; 10] = [
    "REFERENCE_BLOCKED", "WINDOW_BLOCKED", "UNKNOWN_IDENTITY",
    "STATE_CONCENTRATION", "EXTERNAL_EXPOSURE", "COOLDOWN", "SIGNAL_BLOCKED",
    "ZERO_EXECUTABLE_SIZE", "PREPARE_REJECTED", "INTENT_READY",
];
fn instant(s: &str) -> bool {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true) == s)
        .unwrap_or(false)
}
fn number(v: &V) -> Option<f64> {
    v.as_f64().filter(|x| x.is_finite())
}
fn lowercase_reason(s: &str) -> bool {
    !s.is_empty() && s.len() <= 100 && s.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
}

pub fn assess_entry_gates(reference: &V, ask: &V, bid: &V, policy: &V) -> Result<V, String> {
    if reference.get("ready") != Some(&V::Bool(true)) {
        let reason = reference
            .get("reason")
            .and_then(V::as_str)
            .filter(|s| lowercase_reason(s))
            .unwrap_or("unavailable");
        return Ok(json!({
            "referenceReady": false,
            "referenceReason": reason,
            "failed": ["reference"],
            "reference": null,
        }));
    }
    let tests = crate::gates::evaluate_entry_gates(reference, ask, bid, policy)?
        .get("tests")
        .cloned()
        .unwrap_or(V::Null);
    let ask_f = ask.as_f64().unwrap_or(f64::NAN);
    let bid_f = bid.as_f64().unwrap_or(f64::NAN);
    let center = reference.get("center").and_then(V::as_f64).unwrap_or(f64::NAN);
    let deviation = reference.get("robustDeviation").and_then(V::as_f64).unwrap_or(f64::NAN);
    let discount = center - ask_f;
    let relative = discount / center.max(0.001);
    let z = discount / deviation;
    let failed: Vec<String> = tests
        .as_object()
        .map(|m| m.iter().filter(|(_, v)| **v == V::Bool(false)).map(|(k, _)| k.clone()).collect())
        .unwrap_or_default();
    Ok(json!({
        "referenceReady": true,
        "referenceReason": null,
        "reference": {
            "center": number(reference.get("center").unwrap_or(&V::Null)),
            "robustDeviation": number(reference.get("robustDeviation").unwrap_or(&V::Null)),
            "samples": baal_service_runtime::json_text::js_int(reference.get("samples").unwrap_or(&V::Null)),
            "firstAt": reference.get("firstAt").and_then(V::as_str).filter(|s| instant(s)),
            "lastAt": reference.get("lastAt").and_then(V::as_str).filter(|s| instant(s)),
            "discount": if discount.is_finite() { json!(discount) } else { V::Null },
            "relativeDiscount": if relative.is_finite() { json!(relative) } else { V::Null },
            "z": if z.is_finite() { json!(z) } else { V::Null },
            "effectiveAsk": if ask_f.is_finite() { json!(ask_f) } else { V::Null },
            "effectiveBid": if bid_f.is_finite() { json!(bid_f) } else { V::Null },
        },
        "failed": failed,
        "tests": tests,
    }))
}
const ASSESSMENT_KEYS: [&str; 5] =
    ["referenceReady", "referenceReason", "reference", "failed", "tests"];
const GATE_NAMES: [&str; 6] =
    ["price", "spread", "minimumDiscount", "relativeDiscount", "maximumDiscount", "robustZ"];
fn curated_assessment(value: &V) -> Result<V, String> {
    if value.is_null() {
        return Ok(V::Null);
    }
    let m = value.as_object();
    let valid = m.is_some()
        && m.unwrap().keys().all(|k| ASSESSMENT_KEYS.contains(&k.as_str()))
        && value.get("referenceReady").and_then(V::as_bool).is_some()
        && value.get("failed").and_then(V::as_array).is_some_and(|f| {
            f.iter().all(|x| {
                x.as_str().is_some_and(|s| {
                    s == "reference" || GATE_NAMES.contains(&s)
                })
            })
        });
    if !valid {
        return Err("INVALID_ENTRY_ASSESSMENT".into());
    }
    let reason = value.get("referenceReason").unwrap_or(&V::Null);
    if !reason.is_null()
        && !reason.as_str().is_some_and(|s| lowercase_reason(s))
    {
        return Err("INVALID_ENTRY_ASSESSMENT".into());
    }
    if !value.get("reference").unwrap_or(&V::Null).is_null() {
        let fields = [
            "center", "robustDeviation", "samples", "firstAt", "lastAt",
            "discount", "relativeDiscount", "z", "effectiveAsk", "effectiveBid",
        ];
        let r = value.get("reference").unwrap();
        let ok = r.as_object().is_some_and(|m| {
            m.len() == fields.len() && m.keys().all(|k| fields.contains(&k.as_str()))
        });
        if !ok {
            return Err("INVALID_ENTRY_ASSESSMENT".into());
        }
        for (k, v) in r.as_object().unwrap() {
            let field_ok = if v.is_null() {
                true
            } else if ["firstAt", "lastAt"].contains(&k.as_str()) {
                v.as_str().is_some_and(|s| instant(s))
            } else {
                v.as_f64().is_some_and(|x| x.is_finite())
            };
            if !field_ok {
                return Err("INVALID_ENTRY_ASSESSMENT".into());
            }
        }
    }
    if let Some(t) = value.get("tests") {
        if !t.is_null() {
            let ok = t.as_object().is_some_and(|m| {
                m.len() == GATE_NAMES.len()
                    && m.iter().all(|(k, v)| GATE_NAMES.contains(&k.as_str()) && v.is_boolean())
            });
            if !ok {
                return Err("INVALID_ENTRY_ASSESSMENT".into());
            }
        }
    }
    Ok(value.clone())
}
pub fn normalized_entry_decision(input: &V) -> Result<V, String> {
    let run_id = input.get("runId").and_then(V::as_str).unwrap_or("");
    let book = input.get("book").unwrap_or(&V::Null);
    let clock_at = input.get("clockAt").and_then(V::as_str).unwrap_or("");
    let code = input.get("code").and_then(V::as_str).unwrap_or("");
    let run_ok = run_id.strip_prefix("sig-pilot-").is_some_and(|rest| {
        !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    });
    let book_ok = instant(book.get("receivedAt").and_then(V::as_str).unwrap_or(""))
        && instant(book.get("requestStartedAt").and_then(V::as_str).unwrap_or(""))
        && chrono::DateTime::parse_from_rfc3339(book.get("requestStartedAt").and_then(V::as_str).unwrap()).unwrap()
            <= chrono::DateTime::parse_from_rfc3339(book.get("receivedAt").and_then(V::as_str).unwrap()).unwrap()
        && chrono::DateTime::parse_from_rfc3339(book.get("receivedAt").and_then(V::as_str).unwrap()).unwrap()
            <= chrono::DateTime::parse_from_rfc3339(clock_at).unwrap_or_default()
        && book.get("marketId").and_then(V::as_str).is_some_and(|s| !s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit()))
        && book.get("exchangeId").and_then(V::as_str).is_some_and(|s| !s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit()))
        && book.get("versionKey").and_then(V::as_str).is_some_and(|s| s.len() <= 256);
    let model_hash = input.get("modelHash").unwrap_or(&V::Null);
    if !(run_ok
        && CODES.contains(&code)
        && instant(clock_at)
        && book_ok
        && (model_hash.is_null()
            || model_hash.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))))
    {
        return Err("INVALID_ENTRY_DECISION".into());
    }
    let bid = book.pointer("/bids/0/price").and_then(V::as_str).unwrap_or("");
    let ask = book.pointer("/asks/0/price").and_then(V::as_str).unwrap_or("");
    let price_ok = |p: &str| {
        p == "1.000000" || (p.starts_with("0.") && p.len() == 8 && p[2..].bytes().all(|b| b.is_ascii_digit()))
    };
    if !(price_ok(bid) && price_ok(ask)) {
        return Err("INVALID_ENTRY_DECISION_PRICE".into());
    }
    let best_bid = baal_service_runtime::json_text::js_int(book.pointer("/bids/0/quantity").unwrap_or(&V::Null));
    let best_ask = baal_service_runtime::json_text::js_int(book.pointer("/asks/0/quantity").unwrap_or(&V::Null));
    if !(best_bid.is_some_and(|q| (1..=2_147_483_647).contains(&q))
        && best_ask.is_some_and(|q| (1..=2_147_483_647).contains(&q)))
    {
        return Err("INVALID_ENTRY_DECISION_DEPTH".into());
    }
    let proof_in = input.get("referenceProof").unwrap_or(&V::Null);
    let proof = if proof_in.is_null() {
        V::Null
    } else {
        let hex64 = |v: &V| v.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
        let seq = baal_service_runtime::json_text::js_int(proof_in.get("sourceSequence").unwrap_or(&V::Null));
        let ok = seq.is_some_and(|n| n >= 1)
            && ["bankHash", "sourceHash", "referenceHash"]
                .iter()
                .all(|k| hex64(proof_in.get(k).unwrap_or(&V::Null)));
        if !ok {
            return Err("INVALID_ENTRY_DECISION_PROOF".into());
        }
        json!({
            "bankHash": proof_in.get("bankHash").unwrap().clone(),
            "sourceSequence": seq.unwrap(),
            "sourceHash": proof_in.get("sourceHash").unwrap().clone(),
            "referenceHash": proof_in.get("referenceHash").unwrap().clone(),
        })
    };
    let detail = input.get("detailCode").unwrap_or(&V::Null);
    if !detail.is_null()
        && !detail.as_str().is_some_and(|s| {
            !s.is_empty()
                && s.len() <= 101
                && s.bytes().nth(0).is_some_and(|b| b.is_ascii_uppercase())
                && s.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        })
    {
        return Err("INVALID_ENTRY_DECISION_DETAIL".into());
    }
    Ok(json!({
        "schemaVersion": 1,
        "runId": run_id,
        "clockAt": clock_at,
        "code": code,
        "marketId": book.get("marketId").unwrap().clone(),
        "exchangeId": book.get("exchangeId").unwrap().clone(),
        "requestStartedAt": book.get("requestStartedAt").unwrap().clone(),
        "receivedAt": book.get("receivedAt").unwrap().clone(),
        "versionKey": book.get("versionKey").unwrap().clone(),
        "bid": bid,
        "ask": ask,
        "bestBidQuantity": best_bid.unwrap(),
        "bestAskQuantity": best_ask.unwrap(),
        "modelHash": model_hash.clone(),
        "referenceProof": proof,
        "assessment": curated_assessment(input.get("assessment").unwrap_or(&V::Null))?,
        "detailCode": detail.clone(),
        "eligibleForExecution": false,
    }))
}

pub struct EntryDecisionAudit {
    journal: Journal,
    counts: BTreeMap<String, i64>,
    last: Option<V>,
    run_id: String,
}
pub fn open_entry_decision_audit(file: &std::path::Path, run_id: &str) -> Result<EntryDecisionAudit, String> {
    let journal = Journal::open(file)?;
    if let Some(last) = &journal.last {
        if last.pointer("/event/decision/runId").and_then(V::as_str) != Some(run_id) {
            return Err("ENTRY_DECISION_RUN_CHANGED".into());
        }
    }
    let mut counts: BTreeMap<String, i64> = BTreeMap::new();
    let mut last: Option<V> = None;
    if file.exists() {
        let raw = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
        for line in raw.split('\n').filter(|l| !l.is_empty()) {
            let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
            let d = row.pointer("/event/decision");
            if row.pointer("/event/type").and_then(V::as_str) != Some("entry_decision")
                || d.and_then(|d| d.get("runId")).and_then(V::as_str) != Some(run_id)
                || !CODES.contains(&d.and_then(|d| d.get("code")).and_then(V::as_str).unwrap_or(""))
            {
                return Err("ENTRY_DECISION_JOURNAL_SCHEMA".into());
            }
            *counts.entry(d.unwrap().get("code").and_then(V::as_str).unwrap().to_string()).or_insert(0) += 1;
            last = Some(d.unwrap().clone());
        }
    }
    Ok(EntryDecisionAudit { journal, counts, last, run_id: run_id.to_string() })
}
impl EntryDecisionAudit {
    pub fn record(&mut self, input: &V) -> Result<V, String> {
        let mut input = input.clone();
        input
            .as_object_mut()
            .unwrap()
            .insert("runId".into(), json!(self.run_id));
        let decision = normalized_entry_decision(&input)?;
        self.journal
            .commit(&V::Null, &json!({"type": "entry_decision", "decision": decision.clone()}))?;
        *self
            .counts
            .entry(decision.get("code").and_then(V::as_str).unwrap().to_string())
            .or_insert(0) += 1;
        self.last = Some(decision.clone());
        Ok(decision)
    }
    pub fn status(&self) -> V {
        json!({
            "sequence": self.journal.sequence,
            "hash": self.journal.previous.clone(),
            "counts": self.counts,
            "lastDecision": self.last.clone().unwrap_or(V::Null),
            "networkRequests": 0,
            "executionAuthority": false,
        })
    }
}
