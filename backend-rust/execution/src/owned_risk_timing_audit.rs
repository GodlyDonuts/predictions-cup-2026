// execution/owned-risk-timing-audit.mjs — reconciled owned buy-fill exit timing
// over the verified core journal and public book receipts.
use crate::candidate_markouts_audit::book_publication::{apply_book_publication, create_book_publication_state};
use baal_service_runtime::contract::parse_iso_ms;
use baal_service_runtime::json_text::{digest, js_int, js_truthy};
use serde_json::{json, Map, Value as V};
use std::path::Path;

fn need(v: bool, m: &str) -> Result<(), String> {
    if v { Ok(()) } else { Err(m.to_string()) }
}
// complete(file): bytes through last '\n'; need(end>0,'TIMING_NO_COMPLETE_ROWS').
fn complete(file: &Path) -> Result<Vec<V>, String> {
    let bytes = std::fs::read(file).map_err(|e| e.to_string())?;
    let end = bytes.iter().rposition(|b| *b == b'\n').map(|i| i + 1).unwrap_or(0);
    need(end > 0, "TIMING_NO_COMPLETE_ROWS")?;
    let text = String::from_utf8_lossy(&bytes[..end]);
    text.trim()
        .split('\n')
        .map(|l| serde_json::from_str(l).map_err(|e| e.to_string()))
        .collect()
}
fn parse(v: &V) -> f64 {
    v.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN)
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
// JS property-key coercion for object lookup: strings unchanged, numbers to
// their decimal text, anything else won't match a key.
fn key_of(v: &V) -> Option<String> {
    match v {
        V::String(s) => Some(s.clone()),
        V::Number(_) => Some(baal_service_runtime::json_text::js_stringify(v)),
        _ => None,
    }
}
fn num(x: f64) -> V {
    if x.is_finite() && x.fract() == 0.0 && x.abs() <= 9.0e15 {
        json!(x as i64)
    } else {
        json!(x)
    }
}
/// ownedRiskTimingAudit(runtimeDir, universe)
pub fn owned_risk_timing_audit(runtime_dir: &Path, universe: &V) -> Result<V, String> {
    let core = complete(&runtime_dir.join("competition/core-journal.jsonl"))?;
    let mut sequence: i64 = 0;
    let mut previous = String::new();
    let mut clock = f64::NEG_INFINITY;
    for r in &core {
        let mut body = r.clone();
        let hash = body.as_object_mut().and_then(|b| b.remove("hash")).unwrap_or(V::Null);
        let t = parse(&r["at"]);
        need(
            js_int(&r["sequence"]) == Some(sequence + 1)
                && r["previous"].as_str().unwrap_or("\u{0}") == previous
                && hash.as_str().unwrap_or("") == digest(&body)
                && t.is_finite()
                && t >= clock,
            "TIMING_CORE_CHAIN",
        )?;
        sequence += 1;
        previous = hash.as_str().unwrap().to_string();
        clock = t;
    }
    let mut publication = create_book_publication_state(universe)?;
    let mut books: Vec<V> = Vec::new();
    for r in &complete(&runtime_dir.join("public-books.jsonl"))? {
        publication = apply_book_publication(&publication, r)?;
        if r["kind"] == json!("book") {
            books.push(r["payload"]["book"].clone());
        }
    }
    let final_state = core.last().map(|r| r["state"].clone()).unwrap_or(V::Null);
    // JS Object.values(final.intents): null/absent -> TypeError; string -> its
    // chars (all filtered out); other primitives -> no own enumerable props.
    let intent_map = final_state["intents"].as_object().cloned().unwrap_or_else(|| {
        match &final_state["intents"] {
            V::Null | V::Number(_) | V::Bool(_) => Map::new(),
            V::String(s) => s.chars().enumerate().map(|(k, c)| (k.to_string(), json!(c))).collect(),
            V::Array(a) => a.iter().enumerate().map(|(k, v)| (k.to_string(), v.clone())).collect(),
            V::Object(_) => Map::new(),
        }
    });
    if matches!(final_state.get("intents"), None | Some(V::Null)) {
        return Err("Cannot convert undefined or null to object".into());
    }
    let entries: Vec<V> = intent_map
        .values()
        .filter(|i| {
            i["action"] == json!("buy")
                && num_of(&i["filledQuantity"]) > 0.0
                && i["status"] == json!("closed")
        })
        .cloned()
        .collect();
    let intent_lookup = |state: &V, intent_id: &V| -> Option<V> {
        key_of(intent_id).and_then(|id| state["intents"].get(&id)).cloned()
    };
    let mut rows = Vec::new();
    for i in &entries {
        let exchange_id = i.get("exchangeId").cloned().unwrap_or(V::Null);
        let intent_id = i.get("intentId").cloned().unwrap_or(V::Null);
        let owned_key = key_of(&exchange_id);
        let reconciled = core.iter().find(|r| {
            r["event"]["type"] == json!("official_reconciliation")
                && r["event"].get("intentId") == i.get("intentId")
                && intent_lookup(&r["state"], &intent_id).is_some_and(|x| x["status"] == json!("closed"))
                && owned_key.as_ref().is_some_and(|k| js_truthy(r["state"]["owned"].get(k)))
        });
        let reconciled_at = reconciled.map(|r| parse(&r["at"])).unwrap_or(f64::NAN);
        let exit = core.iter().find(|r| {
            r["event"]["type"] == json!("exit_obligation_latched")
                && r["event"].get("exchangeId") == i.get("exchangeId")
                && parse(&r["at"]) > reconciled_at
        });
        let exit_at = exit.map(|r| parse(&r["at"])).unwrap_or(f64::NAN);
        let first = books.iter().find(|b| {
            b.get("exchangeId") == i.get("exchangeId")
                && parse(&b["receivedAt"]) >= reconciled_at
                && (exit.is_none() || parse(&b["receivedAt"]) <= exit_at)
        });
        let sell = exit.and_then(|_| {
            core.iter().find(|r| {
                let prepared = intent_lookup(&r["state"], &r["event"]["intentId"]);
                r["event"]["type"] == json!("intent_prepared")
                    && prepared.as_ref().is_some_and(|p| p["action"] == json!("sell"))
                    && prepared.as_ref().is_some_and(|p| p.get("exchangeId") == i.get("exchangeId"))
                    && parse(&r["at"]) >= exit_at
            })
        });
        let sale = sell.and_then(|r| intent_lookup(&final_state, &r["event"]["intentId"]));
        let first_book = first.map(|b| {
            json!({
                "receivedAt": b["receivedAt"],
                "requestStartedAt": b["requestStartedAt"],
                "versionKey": b["versionKey"],
                "bestBid": b["bids"].as_array().and_then(|a| a.first()).map(|l| l["price"].clone()).unwrap_or(V::Null),
            })
        });
        let exit_trigger = match exit {
            Some(r) => {
                let control = &r["state"]["control"];
                if !control.is_object() {
                    return Err(format!(
                        "Cannot read properties of {} (reading 'exits')",
                        if control.is_null() { "null" } else { "undefined" }
                    ));
                }
                owned_key
                    .as_ref()
                    .and_then(|k| control["exits"].get(k))
                    .and_then(|x| x.get("triggerBid"))
                    .cloned()
                    .unwrap_or(V::Null)
            }
            None => V::Null,
        };
        let latch_ms = if exit.is_some() && first.is_some() {
            num(exit_at - parse(&first.unwrap()["receivedAt"]))
        } else {
            V::Null
        };
        let entry_filled = i["reconciledFills"].as_array().and_then(|a| a.first()).and_then(|f| f.get("filledAt")).cloned().unwrap_or(V::Null);
        let sale_filled = sale
            .as_ref()
            .and_then(|s| s["reconciledFills"].as_array().and_then(|a| a.first()).and_then(|f| f.get("filledAt")).cloned())
            .unwrap_or(V::Null);
        let mut out = Map::new();
        out.insert("marketId".into(), i["marketId"].clone());
        out.insert("exchangeId".into(), exchange_id);
        out.insert("buyIntentId".into(), i["intentId"].clone());
        out.insert("entryFilledAt".into(), entry_filled);
        out.insert("buyClosedReconciledAt".into(), reconciled.map(|r| r["at"].clone()).unwrap_or(V::Null));
        out.insert("firstBookAfterClosedReconciliation".into(), first_book.unwrap_or(V::Null));
        out.insert("exitLatchedAt".into(), exit.map(|r| r["at"].clone()).unwrap_or(V::Null));
        out.insert("exitTriggerBid".into(), exit_trigger);
        out.insert("firstBookToExitLatchMs".into(), latch_ms);
        out.insert("exitPreparedAt".into(), sell.map(|r| r["at"].clone()).unwrap_or(V::Null));
        out.insert("exitFirstFillAt".into(), sale_filled);
        out.insert("evidence".into(), json!("verified_actual_ledger_and_public_receipts_not_counterfactual_fill"));
        rows.push(V::Object(out));
    }
    Ok(json!({
        "schemaVersion": 1,
        "at": baal_service_runtime::contract::ms_to_iso(
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
        ),
        "coreSequence": sequence,
        "coreHash": previous,
        "publicationSequence": publication["sequence"],
        "publicationHash": publication["hash"],
        "entries": rows,
        "networkRequests": 0,
        "competitionOrdersChanged": 0,
        "profitabilityEstablished": false,
        "limitations": [
            "Only observed quotes; gaps do not reveal intermediate prices.",
            "Earlier eligibility is not proof an earlier order would fill or improve profit.",
            "A take-profit exit need not have a risk latch.",
        ],
    }))
}
