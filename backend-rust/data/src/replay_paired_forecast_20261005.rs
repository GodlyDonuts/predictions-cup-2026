// data/replay-paired-forecast-20261005.mjs — paired forecast replay.
use crate::paired_price_reversion_20261005::{paired_quote_packet, party_title_pairs};
use crate::research__paired_forecast_v1::{definition, fit_frozen_forecast, matured_labels, score_forecast};
use baal_collector::breadth::validate_universe;
use baal_collector::market_price_publication::validate_price_observation_pub;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::sha256;
use baal_service_runtime::journal::{journal_lines, JournalReadOptions};
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

fn need(v: bool, c: &str) -> Result<(), String> {
    if !v { return Err(format!("PAIR_FORECAST_REPLAY_{}", c)); }
    Ok(())
}
fn read(f: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(f).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn rt() -> PathBuf {
    Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default()).join("SigTradingCup").join("paper-runtime")
}
fn parse(s: &V) -> f64 { s.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN) }
fn now_iso() -> String {
    ms_to_iso(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64).unwrap_or(0.0))
}
fn write_pretty_excl(f: &Path, v: &V) -> Result<(), String> {
    use std::io::Write;
    let mut fd = std::fs::OpenOptions::new().write(true).create_new(true).open(f).map_err(|e| e.to_string())?;
    fd.write_all(format!("{}\n", serde_json::to_string_pretty(v).map_err(|e| e.to_string())?).as_bytes())
        .map_err(|e| e.to_string())
}

pub fn run(root: &Path) -> Result<V, String> {
    let source = read(&root.join("data/paired-price-reversion-results-20261005.json"))?["source"].clone();
    let file = rt().join("market-prices.jsonl");
    let raw_universe = read(&root.join("live-paper/price-universe.json"))?;
    let universe = validate_universe(&raw_universe, raw_universe["tournamentId"].as_str().unwrap_or(""))?;
    let pairs = party_title_pairs(
        read(&root.join("data/markets.json"))?.as_array().cloned().unwrap_or_default().as_slice(), &universe);
    let mut cache: HashMap<String, V> = HashMap::new();
    let mut samples: Vec<Vec<V>> = pairs.iter().map(|_| Vec::new()).collect();
    let mut last_minutes: Vec<i64> = pairs.iter().map(|_| -1).collect();
    let mut raw = Sha256::new();
    let mut previous = String::new();
    let mut sequence = 0i64;
    let start = Instant::now();
    for line in journal_lines(&file, &JournalReadOptions {
        end_byte: source["prefixBytes"].as_u64(), ..Default::default() })? {
        raw.update(format!("{}\n", line).as_bytes());
        let r: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        let mut body = r.as_object().cloned().unwrap_or_default();
        let hash = body.shift_remove("hash").unwrap_or(V::Null);
        sequence += 1;
        if r["sequence"].as_i64() != Some(sequence)
            || r["previous"].as_str().unwrap_or("") != previous
            || sha256(baal_service_runtime::json_text::js_stringify(&V::Object(body)).as_bytes()) != hash.as_str().unwrap_or("") {
            return Err("SOURCE_CHAIN".into());
        }
        previous = hash.as_str().unwrap_or("").to_string();
        let s = validate_price_observation_pub(&r["event"]["snapshot"], &universe)?;
        let at = parse(&s["receivedAt"]);
        let request_started_at = parse(&s["requestStartedAt"]);
        let mut changed: std::collections::HashSet<String> = std::collections::HashSet::new();
        for id in s["missingIds"].as_array().cloned().unwrap_or_default() {
            cache.remove(id.as_str().unwrap_or(""));
        }
        for q in s["quotes"].as_array().cloned().unwrap_or_default() {
            let mut qq = q.as_object().cloned().unwrap_or_default();
            qq.insert("requestStartedAt".into(), json!(request_started_at));
            qq.insert("receivedAt".into(), json!(at));
            cache.insert(q["exchangeId"].as_str().unwrap_or("").to_string(), V::Object(qq));
            changed.insert(q["exchangeId"].as_str().unwrap_or("").to_string());
        }
        let minute = (at / 60000.0).floor() as i64;
        for i in 0..pairs.len() {
            if last_minutes[i] == minute { continue; }
            let touched = pairs[i]["legs"].as_array().cloned().unwrap_or_default().iter()
                .any(|l| changed.contains(l["exchangeId"].as_str().unwrap_or("")));
            if !touched { continue; }
            if let Some(p) = paired_quote_packet(&pairs[i], &cache, at as i64) {
                let min_req = p["requests"].as_array().cloned().unwrap_or_default().iter()
                    .map(|x| x.as_f64().unwrap_or(f64::INFINITY)).fold(f64::INFINITY, f64::min);
                samples[i].push(json!({"pair": i as i64, "at": at,
                    "buy": p["buy"].as_f64().unwrap_or(f64::NAN) / 1e6,
                    "sell": p["sell"].as_f64().unwrap_or(f64::NAN) / 1e6,
                    "minRequestAt": min_req}));
                last_minutes[i] = minute;
            }
        }
    }
    need(hex_encode(&raw.finalize()) == source["sha256"].as_str().unwrap_or("")
        && sequence == source["records"].as_i64().unwrap_or(-1)
        && previous == source["tailHash"].as_str().unwrap_or(""), "SOURCE_BINDING")?;
    let def = definition();
    let cutoff = (parse(&source["first"])
        + def["trainingFraction"].as_f64().unwrap_or(0.0) * (parse(&source["last"]) - parse(&source["first"]))).floor();
    let mut results: Vec<V> = Vec::new();
    for horizon_ms in def["horizonsMs"].as_array().cloned().unwrap_or_default() {
        let labels: Vec<V> = samples.iter()
            .flat_map(|r| matured_labels(r, horizon_ms.as_f64().unwrap_or(0.0), f64::INFINITY))
            .collect();
        let model = fit_frozen_forecast(&labels, cutoff)?;
        results.push(json!({"horizonMs": horizon_ms, "model": model, "score": score_forecast(&model, &labels),
            "censoredSamples": samples.iter().map(|r| r.len() as i64).sum::<i64>() - labels.len() as i64}));
    }
    let out = json!({"schemaVersion": 1, "at": now_iso(), "mode": "frozen_pooled_liquidation_change_diagnostics",
        "source": source, "cutoff": ms_to_iso(cutoff), "definition": def, "pairs": pairs.len() as i64,
        "samples": samples.iter().map(|r| r.len() as i64).sum::<i64>(),
        "wallMs": start.elapsed().as_secs_f64() * 1000.0, "results": results,
        "allAttemptsRetained": true, "networkRequests": 0, "competitionOrders": 0, "simulatedFills": 0,
        "profitabilityEstablished": false,
        "limitations": ["Quote labels lack executable depth and are not actual trades or funded returns.",
            "Contemporary title discovery does not backfill historical exact predicate knowledge.",
            "Labels crossing the training cutoff are excluded from fitting; later feature scaling is frozen.",
            "Overlapping horizons and correlated election/pair observations are not independent samples; UTC-day results are exposed without iid confidence claims.",
            "Three horizons and ridge strength were fixed before fitting; no best-of-history production winner is selected.",
            "The same original source prefix is development data; only the chronological later segment is model-held-out, not untouched by all prior research."]});
    write_pretty_excl(&root.join("data/paired-forecast-results-20261005-v1.json"), &out)?;
    let summary: Vec<V> = out["results"].as_array().cloned().unwrap_or_default().iter().map(|r| {
        let mut m = serde_json::Map::new();
        m.insert("horizonMs".into(), r["horizonMs"].clone());
        m.insert("train".into(), r["model"]["trainingRows"].clone());
        if let Some(s) = r["score"].as_object() { for (k, v) in s { m.insert(k.clone(), v.clone()); } }
        V::Object(m)
    }).collect();
    println!("{}", baal_service_runtime::json_text::js_stringify(&json!({"samples": out["samples"],
        "wallMs": out["wallMs"], "cutoff": out["cutoff"], "results": summary,
        "profitabilityEstablished": false})));
    Ok(out)
}
fn hex_encode(d: &[u8]) -> String { d.iter().map(|b| format!("{b:02x}")).collect() }
