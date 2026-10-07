// data/measure-owned-exit-hints-20261007.mjs — replay the v9 controller
// journal, bind the captured quote context, and evaluate the advisory
// owned-exit hint over every remaining owned inventory slot. Prints the
// sweep report; performs no network requests and no orders.
use crate::baal_generic_no_controller_v9::{
    baal_generic_no_controller_v9_identity, replay_baal_generic_no_journal_v9,
};
use crate::optimistic_owned_no_exit_hint_v1::optimistic_owned_no_exit_hint_v1;
use crate::publication_pack::{read_json, runtime_dir};
use baal_execution::baal_generic_quote_context_v1::bind_baal_generic_quote_context_v1;
use baal_execution::core::payload_hash;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::fixed::{amount, money};
use baal_service_runtime::json_text::{js_stringify, js_truthy};
use serde_json::{json, Value as V};
use std::path::Path;

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn now_iso() -> String {
    ms_to_iso(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as f64,
    )
}

pub fn run() -> Result<V, String> {
    let rt = runtime_dir();
    let r = read_json(Path::new(&format!("{rt}/baal-expansion-live-request-v3.json")))?;
    let file = Path::new(&rt)
        .join("baal-generic-live-runs")
        .join(r["runId"].as_str().unwrap_or(""))
        .join(r["epoch"].as_str().unwrap_or(""))
        .join("controller")
        .join("baal-generic-no-v9-journal.jsonl");
    let raw = String::from_utf8(std::fs::read(&file).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    // raw.indexOf('\n') === -1 -> slice(0,-1): unreachable on a sealed journal.
    let head_end = raw.find('\n').unwrap_or_else(|| raw.len().saturating_sub(1));
    let root: V = serde_json::from_str(&raw[..head_end]).map_err(|e| e.to_string())?;
    let replay = replay_baal_generic_no_journal_v9(
        &raw,
        &baal_generic_no_controller_v9_identity()?,
        &root["event"]["input"]["custodySeed"],
    )?;
    let coverage = read_json(Path::new(&format!("{rt}/price-coverage.json")))?;
    let mut quote_captures = Vec::new();
    for q in coverage["quotes"]
        .as_object()
        .map(|m| m.values().cloned().collect::<Vec<V>>())
        .unwrap_or_default()
    {
        let mut row = q.as_object().cloned().unwrap_or_default();
        row.insert("knownAt".into(), q["receivedAt"].clone());
        row.insert(
            "sourceHash".into(),
            json!(payload_hash(&json!({
                "universeHash": coverage["universeHash"],
                "quote": q,
            }))?),
        );
        quote_captures.push(V::Object(row));
    }
    let mut hints = json!({
        "tournamentId": r["tournamentId"],
        "universeHash": r["universeHash"],
        "quoteCaptures": quote_captures,
    });
    hints["quoteCaptures"] = json!(bind_baal_generic_quote_context_v1(
        &hints,
        r["tournamentId"].as_str().unwrap_or(""),
        r["universeHash"].as_str().unwrap_or(""),
    )?);
    let mut realized: i128 = 0;
    for p in get(&replay["state"], "inventory")
        .as_object()
        .map(|m| m.values().cloned().collect::<Vec<V>>())
        .unwrap_or_default()
    {
        realized += amount(p["proof"]["cumulativeRealizedPnl"].as_str().unwrap_or(""))?;
    }
    let realized_profit = money(realized);
    let now = now_iso();
    let mut rows = Vec::new();
    for (exchange_id, slot) in get(&replay["state"], "inventory")
        .as_object()
        .cloned()
        .unwrap_or_default()
    {
        if get(get(&slot, "proof"), "remainingOwnedQuantity")
            .as_f64()
            .unwrap_or(0.0)
            <= 0.0
        {
            continue;
        }
        let hint = optimistic_owned_no_exit_hint_v1(
            &r["scope"],
            r["universeHash"].as_str().unwrap_or(""),
            &hints,
            &slot,
            &json!(realized_profit),
            &now,
        );
        let mut row = serde_json::Map::new();
        row.insert("exchangeId".into(), json!(exchange_id));
        if let Some(m) = hint.as_object() {
            for (k, v) in m.iter() {
                row.insert(k.clone(), v.clone());
            }
        }
        rows.push(V::Object(row));
    }
    let deferred = rows
        .iter()
        .filter(|row| js_truthy(row.get("defer")))
        .count() as i64;
    let out = json!({
        "observedAt": now,
        "actorAnchor": replay["anchor"],
        "rows": rows,
        "networkRequests": 0,
        "potentialSavedReadsThisSweep": deferred * 5,
        "actualSkippedReads": 0,
        "actualOrders": 0,
    });
    println!("{}", js_stringify(&out));
    Ok(out)
}
