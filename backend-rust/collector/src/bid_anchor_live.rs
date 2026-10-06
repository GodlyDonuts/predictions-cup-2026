// live-paper/bid-anchor-live.mjs — bid-anchor replay gated by actual consumer
// availability (checkpoint journal stamps), with per-market consumer barriers.
use crate::bid_anchor::{advance_bid_anchor, bid_anchor_report};
use crate::model::invalidate;
use baal_paper::fixed::instant;
use serde_json::{json, Value as V};
use std::collections::BTreeSet;

fn need(ok: bool) -> Result<(), String> { if !ok { return Err("BID_ANCHOR_AVAILABILITY".to_string()); } Ok(()) }
fn ins(v: &V) -> Result<i64, String> { instant(v.as_str().unwrap_or("")) }

fn fence(s: &mut V, id: Option<&str>, at: &str) {
    let mut ids: BTreeSet<String> = BTreeSet::new();
    match id {
        None => {
            if let Some(h) = s["histories"].as_object() { ids.extend(h.keys().cloned()); }
            if let Some(ws) = s["wallets"].as_object() {
                for w in ws.values() {
                    if let Some(books) = w["state"]["books"].as_object() {
                        ids.extend(books.values().filter_map(|b| b["marketId"].as_str().map(String::from)));
                    }
                }
            }
        }
        Some(i) => { ids.insert(i.to_string()); }
    }
    for market in ids {
        s["live"]["barriers"][&market] = json!(at);
        s["histories"].as_object_mut().map(|h| h.remove(&market));
        s["seen"].as_object_mut().map(|h| h.remove(&market));
    }
    if let Some(ws) = s["wallets"].as_object() {
        let keys: Vec<String> = ws.keys().cloned().collect();
        for key in keys {
            invalidate(&mut s["wallets"][key.as_str()]["state"], id, at, "actual_consumer_boundary");
            let pending_keys: Vec<String> = s["wallets"][key.as_str()]["state"]["strategies"]["mean_reversion"]["pending"]
                .as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
            for pk in pending_keys {
                let pend = &s["wallets"][key.as_str()]["state"]["strategies"]["mean_reversion"]["pending"][pk.as_str()];
                if pend["action"].as_str() == Some("buy")
                    && (id.is_none()
                        || s["wallets"][key.as_str()]["state"]["books"][pk.as_str()]["marketId"].as_str() == id) {
                    s["wallets"][key.as_str()]["state"]["strategies"]["mean_reversion"]["pending"]
                        .as_object_mut().map(|o| o.remove(&pk));
                }
            }
        }
    }
}

/// advanceLiveBidAnchor(s, row, bank, availableAt) — mutates s.
pub fn advance_live_bid_anchor(s: &mut V, row: &V, bank: &V, available_at: &str) -> Result<(), String> {
    if s.get("live").is_none() || s["live"].is_null() {
        s["live"] = json!({"lastAvailableAt":bank["cutoff"],"barriers":{},"counts":{}});
    }
    need(ins(&row["at"])? <= instant(available_at)?
        && instant(available_at)? >= ins(&s["live"]["lastAvailableAt"])?)?;
    s["live"]["lastAvailableAt"] = json!(available_at);
    let inc = |s: &mut V, k: &str| {
        s["live"]["counts"][k] = json!(s["live"]["counts"][k].as_i64().unwrap_or(0) + 1);
    };
    let kind = row["kind"].as_str().unwrap_or("");
    if ["session_start", "session_stop", "invalidation"].contains(&kind) {
        advance_bid_anchor(s, row, bank)?;
        let id = if kind == "invalidation" { row["payload"]["marketId"].as_str() } else { None };
        fence(s, id, available_at);
        inc(s, "source_boundary");
        return Ok(());
    }
    if kind != "book" { return Ok(()); }
    let b = row["payload"]["book"].clone();
    let id = b["marketId"].as_str().unwrap_or("").to_string();
    if instant(available_at)? - ins(&b["receivedAt"])? > bank["config"]["maxDataAgeMs"].as_i64().unwrap_or(0) {
        if s["live"]["barriers"][&id].is_null() { fence(s, Some(&id), available_at); }
        inc(s, "stale_consumer_book");
        return Ok(());
    }
    if !s["live"]["barriers"][&id].is_null()
        && ins(&b["requestStartedAt"])? <= ins(&s["live"]["barriers"][&id])? {
        inc(s, "request_before_consumer_barrier");
        return Ok(());
    }
    s["live"]["barriers"].as_object_mut().map(|o| o.remove(&id));
    let mut before: serde_json::Map<String, V> = serde_json::Map::new();
    if let Some(ws) = s["wallets"].as_object() {
        for (key, w) in ws {
            let mut pendings = serde_json::Map::new();
            if let Some(p) = w["state"]["strategies"]["mean_reversion"]["pending"].as_object() {
                for (inst, i) in p {
                    pendings.insert(inst.clone(), json!({"id":i["id"],"createdAt":i["createdAt"]}));
                }
            }
            before.insert(key.clone(), V::Object(pendings));
        }
    }
    let prior_signals = s["signals"].as_array().map(|a| a.len()).unwrap_or(0);
    advance_bid_anchor(s, row, bank)?;
    let new_signals: Vec<usize> = match s["signals"].as_array() {
        Some(a) => (prior_signals..a.len()).collect(),
        None => Vec::new(),
    };
    for i in new_signals { s["signals"][i]["actualAvailableAt"] = json!(available_at); }
    if let Some(ws) = s["wallets"].as_object() {
        let keys: Vec<String> = ws.keys().cloned().collect();
        for key in keys {
            let insts: Vec<String> = s["wallets"][key.as_str()]["state"]["strategies"]["mean_reversion"]["pending"]
                .as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
            for instrument in insts {
                let i = &s["wallets"][key.as_str()]["state"]["strategies"]["mean_reversion"]["pending"][instrument.as_str()];
                let old = &before[key.as_str()][instrument.as_str()];
                let stale = old.is_null()
                    || old["id"] != i["id"]
                    || old["createdAt"] != i["createdAt"];
                if stale && !i.is_null() {
                    s["wallets"][key.as_str()]["state"]["strategies"]["mean_reversion"]["pending"][instrument.as_str()]["createdAt"] = json!(available_at);
                    s["wallets"][key.as_str()]["state"]["strategies"]["mean_reversion"]["pending"][instrument.as_str()]["actualAvailableAt"] = json!(available_at);
                }
            }
        }
    }
    inc(s, "fresh_consumer_book");
    Ok(())
}

/// liveBidAnchorReport(s, bank, at)
pub fn live_bid_anchor_report(s: &V, bank: &V, at: &str) -> V {
    let mut r = bid_anchor_report(s, bank, at);
    r["mode"] = json!("bid_anchor_actual_receipt_paper");
    r["updatedAt"] = json!(at);
    r["actualConsumer"] = s["live"].clone();
    r["limitations"] = json!([
        "Consumer availability is recorded in the original checkpoint journal; restarted backlogs do not receive retroactive entries.",
        "Fills remain local next-request counterfactuals with one-percent depth and adverse ticks, not contest orders.",
        "Prior executable bids are an unproven signal hypothesis.",
        "Correlated markets and latency wallets are not independent evidence."
    ]);
    r
}
