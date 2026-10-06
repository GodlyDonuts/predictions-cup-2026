// execution/decision-audit.mjs — normalized entry-decision journal records.
use baal_service_runtime::journal::{journal_lines, Journal, JournalReadOptions};
use baal_service_runtime::json_text::{js_int, js_truthy};
use serde_json::{json, Map, Value as V};
use std::path::Path;

const CODES: [&str; 10] = [
    "REFERENCE_BLOCKED", "WINDOW_BLOCKED", "UNKNOWN_IDENTITY", "STATE_CONCENTRATION",
    "EXTERNAL_EXPOSURE", "COOLDOWN", "SIGNAL_BLOCKED", "ZERO_EXECUTABLE_SIZE",
    "PREPARE_REJECTED", "INTENT_READY",
];
const GATES: [&str; 6] = ["price", "spread", "minimumDiscount", "relativeDiscount", "maximumDiscount", "robustZ"];

fn check(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(code.to_string()) }
}
// instant: string that round-trips through new Date(x).toISOString() === x.
fn is_instant(v: &V) -> bool {
    v.as_str().is_some_and(|s| {
        chrono::DateTime::parse_from_rfc3339(s)
            .ok()
            .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true) == s)
            .unwrap_or(false)
    })
}
fn number(v: &V) -> V {
    v.as_f64().filter(|x| x.is_finite()).map(V::from).unwrap_or(V::Null)
}
// JS Number(x) coercion.
fn num_of(v: &V) -> f64 {
    match v {
        V::Null => 0.0,
        V::Bool(b) => i32::from(*b) as f64,
        V::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        V::String(s) => {
            let t = s.trim();
            if t.is_empty() { 0.0 } else { t.parse::<f64>().unwrap_or(f64::NAN) }
        }
        _ => f64::NAN,
    }
}
// JS String(x) coercion for regex .test().
fn js_str(v: &V) -> String {
    match v {
        V::Null => "null".into(),
        V::Bool(b) => b.to_string(),
        V::Number(_) => baal_service_runtime::json_text::js_stringify(v),
        V::String(s) => s.clone(),
        V::Array(a) => a.iter().map(|x| if x.is_null() { String::new() } else { js_str(x) }).collect::<Vec<_>>().join(","),
        V::Object(_) => "[object Object]".into(),
    }
}
fn digit_id(v: &V) -> bool {
    // /^([1-9]\d*)$/ with RegExp.test string coercion.
    let s = js_str(v);
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && s.as_bytes()[0] != b'0'
}
fn lowercase_reason(s: &str) -> bool {
    !s.is_empty() && s.len() <= 100 && s.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
}
/// assessEntryGates(reference, ask, bid, policy)
pub fn assess_entry_gates(reference: &V, ask: &V, bid: &V, policy: &V) -> Result<V, String> {
    if !js_truthy(reference.get("ready")) {
        let reason = reference.get("reason").and_then(V::as_str);
        return Ok(json!({
            "referenceReady": false,
            "referenceReason": reason.filter(|r| lowercase_reason(r)).unwrap_or("unavailable"),
            "failed": ["reference"],
            "reference": V::Null,
        }));
    }
    let evaluated = crate::entry_gates::evaluate_entry_gates(reference, ask, bid, policy)?;
    let tests = evaluated.get("tests").cloned().unwrap_or(V::Null);
    let ask_f = num_of(ask);
    let bid_f = num_of(bid);
    let center = num_of(reference.get("center").unwrap_or(&V::Null));
    let deviation = num_of(reference.get("robustDeviation").unwrap_or(&V::Null));
    let discount = center - ask_f;
    let relative = discount / f64::max(center, 0.001);
    let z = discount / deviation;
    let failed: Vec<V> = tests
        .as_object()
        .map(|m| m.iter().filter(|(_, v)| !js_truthy(Some(v))).map(|(k, _)| json!(k)).collect())
        .unwrap_or_default();
    Ok(json!({
        "referenceReady": true,
        "referenceReason": V::Null,
        "reference": {
            "center": number(reference.get("center").unwrap_or(&V::Null)),
            "robustDeviation": number(reference.get("robustDeviation").unwrap_or(&V::Null)),
            "samples": baal_service_runtime::json_text::js_int(reference.get("samples").unwrap_or(&V::Null)).map(V::from).unwrap_or(V::Null),
            "firstAt": if is_instant(reference.get("firstAt").unwrap_or(&V::Null)) { reference["firstAt"].clone() } else { V::Null },
            "lastAt": if is_instant(reference.get("lastAt").unwrap_or(&V::Null)) { reference["lastAt"].clone() } else { V::Null },
            "discount": number(&json!(discount)),
            "relativeDiscount": number(&json!(relative)),
            "z": number(&json!(z)),
            "effectiveAsk": number(&json!(ask_f)),
            "effectiveBid": number(&json!(bid_f)),
        },
        "failed": failed,
        "tests": tests,
    }))
}
/// structuredClone of a validated curated assessment.
fn curated_assessment(value: &V) -> Result<V, String> {
    if value.is_null() {
        return Ok(V::Null);
    }
    let allowed = ["referenceReady", "referenceReason", "reference", "failed", "tests"];
    let gate_or_reference: Vec<&str> = ["reference"].iter().chain(GATES.iter()).cloned().collect();
    let m = value.as_object();
    check(
        m.is_some()
            && m.unwrap().keys().all(|k| allowed.contains(&k.as_str()))
            && value["referenceReady"].is_boolean()
            && value["failed"].as_array().is_some_and(|f| f.iter().all(|k| k.as_str().is_some_and(|s| gate_or_reference.contains(&s)))),
        "INVALID_ENTRY_ASSESSMENT",
    )?;
    check(
        value["referenceReason"].is_null()
            || value["referenceReason"].as_str().is_some_and(|s| lowercase_reason(s)),
        "INVALID_ENTRY_ASSESSMENT",
    )?;
    // JS: value.reference!==null (absent counts as present-then-invalid).
    if value.as_object().unwrap().get("reference") != Some(&V::Null) {
        let fields = ["center", "robustDeviation", "samples", "firstAt", "lastAt", "discount", "relativeDiscount", "z", "effectiveAsk", "effectiveBid"];
        let r = value["reference"].as_object();
        check(
            r.is_some() && r.unwrap().len() == fields.len() && r.unwrap().keys().all(|k| fields.contains(&k.as_str())),
            "INVALID_ENTRY_ASSESSMENT",
        )?;
        for (k, v) in r.unwrap() {
            if !v.is_null() {
                let ok = if ["firstAt", "lastAt"].contains(&k.as_str()) { is_instant(v) } else { v.as_f64().is_some_and(|x| x.is_finite()) };
                check(ok, "INVALID_ENTRY_ASSESSMENT")?;
            }
        }
    }
    if let Some(tests) = value.as_object().unwrap().get("tests") {
        let t = tests.as_object();
        check(
            t.is_some() && t.unwrap().len() == GATES.len()
                && t.unwrap().iter().all(|(k, v)| GATES.contains(&k.as_str()) && v.is_boolean()),
            "INVALID_ENTRY_ASSESSMENT",
        )?;
    }
    Ok(value.clone())
}
/// normalizedEntryDecision({runId, book, clockAt, code, modelHash, referenceProof, assessment, detailCode})
pub fn normalized_entry_decision(input: &V) -> Result<V, String> {
    let run_id = input.get("runId").and_then(V::as_str);
    let book = input.get("book").cloned().unwrap_or(V::Null);
    let clock_at = input.get("clockAt").unwrap_or(&V::Null);
    let code = input.get("code").and_then(V::as_str).unwrap_or("");
    let model_hash = input.get("modelHash").cloned().unwrap_or(V::Null);
    let reference_proof = input.get("referenceProof").cloned().unwrap_or(V::Null);
    let detail_code = input.get("detailCode").cloned().unwrap_or(V::Null);
    let run_ok = run_id.is_some_and(|s| {
        s.len() > 10 && s.starts_with("sig-pilot-") && s[10..].bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    });
    check(
        run_ok && CODES.contains(&code)
            && is_instant(clock_at) && is_instant(book.get("receivedAt").unwrap_or(&V::Null)) && is_instant(book.get("requestStartedAt").unwrap_or(&V::Null))
            && baal_service_runtime::contract::parse_iso_ms(book["requestStartedAt"].as_str().unwrap_or("")) <= baal_service_runtime::contract::parse_iso_ms(book["receivedAt"].as_str().unwrap_or(""))
            && baal_service_runtime::contract::parse_iso_ms(book["receivedAt"].as_str().unwrap_or("")) <= baal_service_runtime::contract::parse_iso_ms(clock_at.as_str().unwrap_or(""))
            && digit_id(book.get("marketId").unwrap_or(&V::Null)) && digit_id(book.get("exchangeId").unwrap_or(&V::Null))
            && book.get("versionKey").and_then(V::as_str).is_some_and(|s| s.encode_utf16().count() <= 256)
            && (model_hash.is_null() || model_hash.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))),
        "INVALID_ENTRY_DECISION",
    )?;
    let prices = [
        ("bid", book.get("bids").and_then(|a| a.as_array()).and_then(|a| a.first()).and_then(|x| x.get("price"))),
        ("ask", book.get("asks").and_then(|a| a.as_array()).and_then(|a| a.first()).and_then(|x| x.get("price"))),
    ];
    for (_, p) in &prices {
        check(
            p.and_then(|v| v.as_str()).is_some_and(|s| {
                s == "1.000000"
                    || (s.len() == 8 && s.starts_with("0.") && s[2..].bytes().all(|b| b.is_ascii_digit()))
            }),
            "INVALID_ENTRY_DECISION_PRICE",
        )?;
    }
    let quantities = [
        ("bestBidQuantity", book.get("bids").and_then(|a| a.as_array()).and_then(|a| a.first()).and_then(|x| x.get("quantity")).cloned().unwrap_or(V::Null)),
        ("bestAskQuantity", book.get("asks").and_then(|a| a.as_array()).and_then(|a| a.first()).and_then(|x| x.get("quantity")).cloned().unwrap_or(V::Null)),
    ];
    for (_, q) in &quantities {
        check(js_int(q).is_some_and(|x| (1..=2_147_483_647).contains(&x)), "INVALID_ENTRY_DECISION_DEPTH")?;
    }
    let proof = if reference_proof.is_null() {
        V::Null
    } else {
        let hex64 = |v: &V| v.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
        let p = json!({
            "bankHash": reference_proof.get("bankHash"),
            "sourceSequence": reference_proof.get("sourceSequence"),
            "sourceHash": reference_proof.get("sourceHash"),
            "referenceHash": reference_proof.get("referenceHash"),
        });
        check(
            js_int(&p["sourceSequence"]).is_some_and(|x| x >= 1)
                && hex64(&p["bankHash"]) && hex64(&p["sourceHash"]) && hex64(&p["referenceHash"]),
            "INVALID_ENTRY_DECISION_PROOF",
        )?;
        p
    };
    check(
        detail_code.is_null() || detail_code.as_str().is_some_and(|s| {
            !s.is_empty() && s.len() <= 101 && s.as_bytes()[0].is_ascii_uppercase()
                && s[1..].bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        }),
        "INVALID_ENTRY_DECISION_DETAIL",
    )?;
    let assessment = curated_assessment(input.get("assessment").unwrap_or(&V::Null))?;
    let mut out = Map::new();
    out.insert("schemaVersion".into(), json!(1));
    out.insert("runId".into(), json!(run_id.unwrap()));
    out.insert("clockAt".into(), clock_at.clone());
    out.insert("code".into(), json!(code));
    out.insert("marketId".into(), book["marketId"].clone());
    out.insert("exchangeId".into(), book["exchangeId"].clone());
    out.insert("requestStartedAt".into(), book["requestStartedAt"].clone());
    out.insert("receivedAt".into(), book["receivedAt"].clone());
    out.insert("versionKey".into(), book["versionKey"].clone());
    out.insert("bid".into(), prices[0].1.unwrap().clone());
    out.insert("ask".into(), prices[1].1.unwrap().clone());
    out.insert("bestBidQuantity".into(), quantities[0].1.clone());
    out.insert("bestAskQuantity".into(), quantities[1].1.clone());
    out.insert("modelHash".into(), model_hash);
    out.insert("referenceProof".into(), proof);
    out.insert("assessment".into(), assessment);
    out.insert("detailCode".into(), detail_code);
    out.insert("eligibleForExecution".into(), json!(false));
    Ok(V::Object(out))
}
/// openEntryDecisionAudit(file, runId) — verified decision journal writer.
pub struct EntryDecisionAudit {
    journal: Journal,
    run_id: String,
    counts: Map<String, V>,
    last: Option<V>,
}
pub fn open_entry_decision_audit(file: &Path, run_id: &str) -> Result<EntryDecisionAudit, String> {
    let journal = Journal::open(file).map_err(|e| {
        e.strip_prefix("JOURNAL_INCOMPLETE_TAIL: ").map(str::to_string).unwrap_or(e)
    })?;
    if let Some(last) = &journal.last {
        if last.get("event").and_then(|e| e.get("decision")).and_then(|d| d.get("runId")).and_then(V::as_str) != Some(run_id) {
            return Err("ENTRY_DECISION_RUN_CHANGED".into());
        }
    }
    let mut counts: Map<String, V> = Map::new();
    let mut last_decision = None;
    if file.exists() {
        for line in journal_lines(file, &JournalReadOptions::default())? {
            if line.is_empty() {
                continue;
            }
            let row: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
            let d = row.get("event").and_then(|e| e.get("decision"));
            if row.get("event").and_then(|e| e.get("type")).and_then(V::as_str) != Some("entry_decision")
                || d.and_then(|d| d.get("runId")).and_then(V::as_str) != Some(run_id)
                || !CODES.contains(&d.and_then(|d| d.get("code")).and_then(V::as_str).unwrap_or(""))
            {
                return Err("ENTRY_DECISION_JOURNAL_SCHEMA".into());
            }
            let code = d.unwrap()["code"].as_str().unwrap().to_string();
            let c = counts.get(&code).and_then(V::as_i64).unwrap_or(0);
            counts.insert(code, json!(c + 1));
            last_decision = d.cloned();
        }
    }
    Ok(EntryDecisionAudit { journal, run_id: run_id.to_string(), counts, last: last_decision })
}
impl EntryDecisionAudit {
    /// record(input): normalizedEntryDecision({...input, runId}) then durable commit.
    pub fn record(&mut self, input: &V) -> Result<V, String> {
        let mut merged = input.as_object().cloned().unwrap_or_default();
        merged.insert("runId".into(), json!(self.run_id));
        let decision = normalized_entry_decision(&V::Object(merged))?;
        self.journal.commit(&V::Null, &json!({"type": "entry_decision", "decision": decision}))?;
        let c = self.counts.get(decision["code"].as_str().unwrap()).and_then(V::as_i64).unwrap_or(0);
        self.counts.insert(decision["code"].as_str().unwrap().to_string(), json!(c + 1));
        self.last = Some(decision.clone());
        Ok(decision)
    }
    pub fn status(&self) -> V {
        json!({
            "sequence": self.journal.sequence,
            "hash": self.journal.previous,
            "counts": V::Object(self.counts.clone()),
            "lastDecision": self.last.clone().unwrap_or(V::Null),
            "networkRequests": 0,
            "executionAuthority": false,
        })
    }
}
