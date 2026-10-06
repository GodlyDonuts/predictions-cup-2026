// data/profile-profit-sprint-read-cadence-20261005.mjs — read-timings baseline
// over a fixed window; writes the baseline profile receipt.
use crate::publication_pack::runtime_dir;
use baal_collector::read_timing::{timing_distribution, validate_timing};
use baal_service_runtime::contract::parse_iso_ms;
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::io::Write;

fn group_into(groups: &mut serde_json::Map<String, V>, e: &V) {
    let kind = e["kind"].as_str().unwrap_or("");
    let g = groups.entry(kind.to_string()).or_insert_with(|| json!({
        "requests": 0, "totalClientMs": 0.0, "serviceMs": 0.0, "admissionMs": 0.0,
    }));
    g["requests"] = json!(g["requests"].as_u64().unwrap_or(0) + 1);
    for (out, key) in [("totalClientMs", "totalMs"), ("serviceMs", "serviceMs"), ("admissionMs", "admissionWaitMs")] {
        g[out] = json!(g[out].as_f64().unwrap_or(0.0) + e[key].as_f64().unwrap_or(0.0));
    }
}
pub fn run() -> Result<V, String> {
    let rt = runtime_dir();
    let start = "2026-10-05T20:35:00.000Z";
    let end = "2026-10-05T20:57:15.000Z";
    let raw = std::fs::read_to_string(format!("{rt}/read-timings.jsonl")).map_err(|e| e.to_string())?;
    let complete = &raw[..raw.rfind('\n').map(|i| i + 1).unwrap_or(0)];
    let mut rows: Vec<(String, V)> = Vec::new();
    for line in complete.trim_end().split('\n') {
        if line.is_empty() { continue; }
        let event = validate_timing(&serde_json::from_str::<V>(line).map_err(|e| e.to_string())?)?;
        if parse_iso_ms(event["queuedAt"].as_str().unwrap_or("")).unwrap_or(0.0) >= parse_iso_ms(start).unwrap_or(0.0)
            && parse_iso_ms(event["finishedAt"].as_str().unwrap_or("")).unwrap_or(0.0) < parse_iso_ms(end).unwrap_or(0.0)
        { rows.push((line.to_string(), event)); }
    }
    let events: Vec<&V> = rows.iter().map(|(_, e)| e).collect();
    let mut groups = serde_json::Map::<String, V>::new();
    for e in &events { group_into(&mut groups, e); }
    let mut book_recurrence = serde_json::Map::<String, V>::new();
    for market_id in ["195", "356"] {
        let b: Vec<&&V> = events.iter().filter(|e| e["kind"] == json!("book") && e["marketId"] == json!(market_id)).collect();
        let vals: Vec<V> = (1..b.len()).map(|i| json!(
            parse_iso_ms(b[i]["finishedAt"].as_str().unwrap_or("")).unwrap_or(0.0)
                - parse_iso_ms(b[i - 1]["finishedAt"].as_str().unwrap_or("")).unwrap_or(0.0)
        )).collect();
        book_recurrence.insert(market_id.to_string(), timing_distribution(&vals));
    }
    let mut window = Sha256::new();
    window.update(rows.iter().map(|(l, _)| format!("{l}\n")).collect::<String>().as_bytes());
    let result = json!({
        "schemaVersion": 1, "observedFrom": start, "observedThrough": end,
        "source": "existing serialized collector read-timings.jsonl; complete successful and unsuccessful receipts",
        "windowSha256": format!("{:x}", window.finalize()),
        "requests": events.len(), "groups": V::Object(groups),
        "bookRecurrence": V::Object(book_recurrence.clone()),
        "implementation": {
            "rateBudgetChanged": false, "extraClients": 0, "bookDepth": 20,
            "ordinaryBookCap": 4, "officialWrites": 0, "sprintLifetimeMaxMs": 3600000,
            "expectedRemovedRequestsPerOwnedSourceCycle": 5,
        },
        "limits": [
            "Baseline observation; postdeployment speed is not established",
            "Five removed calls assumes two unrelated discovery books, two optional breadth batches and one auxiliary read; metadata/liabilities reduce savings",
            "No observed strategy profitability is implied",
        ],
    });
    std::fs::OpenOptions::new().write(true).create(true).open("data/profit-sprint-read-baseline-20261005.json")
        .and_then(|mut f| f.write_all(format!("{}\n", serde_json::to_string_pretty(&result).unwrap()).as_bytes()))
        .map_err(|e| e.to_string())?;
    Ok(json!({
        "requests": result["requests"], "bookRecurrence": V::Object(book_recurrence),
        "groups": result["groups"],
    }))
}
