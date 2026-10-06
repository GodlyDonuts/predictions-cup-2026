// live-paper/market-history-audit.mjs — read-only bootstrap audit of the paper runtime.
use baal_service_runtime::json_text::digest;
use serde_json::{json, Value as V};
use std::path::Path;

fn parse_at(v: &V) -> f64 { baal_service_runtime::contract::parse_iso_ms(v.as_str().unwrap_or("")).unwrap_or(f64::NAN) }
/// auditMarketHistory() — synchronous port. `root` is the repo root (the JS
/// module derives it from import.meta.url); the runtime dir still comes from
/// LOCALAPPDATA exactly like the Node script.
pub fn audit_market_history(root: &Path) -> Result<V, String> {
    let rt = Path::new(&std::env::var("LOCALAPPDATA").map_err(|e| e.to_string())?)
        .join("SigTradingCup/paper-runtime");
    let universe_raw: V = serde_json::from_slice(&std::fs::read(root.join("live-paper/price-universe.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let config: V = serde_json::from_slice(&std::fs::read(root.join("live-paper/config.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let u = crate::breadth::validate_universe(&universe_raw, config["tournamentId"].as_str().unwrap_or(""))?;
    let mut state = crate::market_history::create_market_history(&u);
    let mut sources = Vec::new();
    let mut observed: Vec<V> = Vec::new();
    for name in ["journal.jsonl", "market-prices.jsonl"] {
        let file = rt.join(name);
        if !file.exists() { continue; }
        let raw = std::fs::read(&file).map_err(|e| e.to_string())?;
        let size = raw.len();
        if size == 0 { continue; }
        let mut sequence: i64 = 0;
        let mut previous = String::new();
        let mut consumed: usize = 0;
        let mut observations: i64 = 0;
        for line_bytes in raw.split(|b| *b == b'\n') {
            let line_len = line_bytes.len();
            consumed += line_len + 1;
            if consumed > size {
                consumed -= line_len + 1;
                break;
            }
            let line = std::str::from_utf8(line_bytes).map_err(|_| "MARKET_HISTORY_SOURCE_CHAIN".to_string())?;
            let row: V = serde_json::from_str(line).map_err(|_| "MARKET_HISTORY_SOURCE_CHAIN".to_string())?;
            let h = row["hash"].as_str().unwrap_or("").to_string();
            let mut body = row.clone();
            body.as_object_mut().and_then(|m| m.shift_remove("hash"));
            if row["sequence"].as_i64() != Some(sequence + 1)
                || row["previous"].as_str().unwrap_or("") != previous
                || digest(&body) != h
            {
                return Err("MARKET_HISTORY_SOURCE_CHAIN".into());
            }
            sequence = row["sequence"].as_i64().unwrap_or(0);
            previous = h;
            let snapshot = if name == "journal.jsonl" {
                row["event"]["bulkPrices"].clone()
            } else {
                row["event"]["snapshot"].clone()
            };
            if baal_service_runtime::json_text::js_truthy(Some(&snapshot)) {
                observed.push(snapshot);
                observations += 1;
            }
        }
        sources.push(json!({"name": name, "sequence": sequence, "hash": previous,
            "observations": observations, "bytes": if consumed <= size { consumed } else { size }}));
    }
    let mut seen = std::collections::BTreeSet::new();
    observed.sort_by(|a, b| parse_at(&a["receivedAt"]).partial_cmp(&parse_at(&b["receivedAt"])).unwrap_or(std::cmp::Ordering::Equal));
    for snapshot in &observed {
        let id = digest(snapshot);
        if seen.contains(&id) { continue; }
        seen.insert(id);
        state = crate::market_history::observe_market_history(&state, snapshot, &u, &crate::market_history::market_history_policy())?;
    }
    let clock_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let rank = crate::market_history::rank_market_history(&state, &u, &clock_at, &crate::market_history::market_history_policy())?;
    let mut report = json!({"schemaVersion": 1});
    for (k, v) in rank.as_object().unwrap() { report[k] = v.clone(); }
    report["sources"] = json!(sources);
    report["sourceObservations"] = state["observations"].clone();
    report["meaning"] = json!("Read-only screening: historical bids are counterfactual recovery references, not forecasts or executable fills.");
    let out = format!("{}\n", serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?);
    std::fs::write(root.join("data/market-history-live-audit.json"), out).map_err(|e| e.to_string())?;
    Ok(json!({"state": state, "report": report, "universe": u}))
}
