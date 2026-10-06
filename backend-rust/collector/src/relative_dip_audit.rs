// live-paper/relative-dip-audit.mjs — frozen relative-dip recovery study.
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::journal::{journal_lines, JournalReadOptions};
use baal_service_runtime::json_text::{digest, js_stringify, js_truthy};
use serde_json::{json, Map, Value as V};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::relative_dip::{create_relative_history, observe_relative_prices, relative_dip_features,
    MovementCache};

fn hash(v: &V) -> String { digest(v) }
fn js_num_string(v: &V) -> String {
    // String(x): strings pass through; numbers round-trip via JSON formatting.
    match v {
        V::String(s) => s.clone(),
        V::Number(n) => n.to_string(),
        V::Bool(b) => b.to_string(),
        V::Null => "null".into(),
        other => js_stringify(other),
    }
}
fn basename(file: &Path) -> String {
    file.file_name().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default()
}
/// Emit a JS number: integral values serialize as integers; non-finite -> null.
fn num_key(x: f64) -> V {
    if x.fract() == 0.0 && x.is_finite() { json!(x as i64) } else { json!(x) }
}

/// verifiedRelativeInputs(files, universe, requiredPrefixes=[])
pub fn verified_relative_inputs(files: &[PathBuf], universe: &V, required_prefixes: &[V]) -> Result<V, String> {
    let mut snapshots: Vec<V> = Vec::new();
    let mut sources: Vec<V> = Vec::new();
    for file in files {
        if !file.exists() { continue; }
        let size = fs::metadata(file).map_err(|e| e.to_string())?.len();
        let mut sequence = json!(0);
        let mut seq_f = 0.0f64;
        let mut previous = String::new();
        let mut bytes: u64 = 0;
        let mut last_at = f64::NEG_INFINITY;
        let base = basename(file);
        let required = required_prefixes.iter().find(|x| x["file"].as_str() == Some(base.as_str())).cloned();
        let mut prefix_seen = required.as_ref().map(|r| r["sequence"].as_f64() == Some(0.0)).unwrap_or(true);
        for line in journal_lines(file, &JournalReadOptions { end_byte: Some(size), ..Default::default() })? {
            let row: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
            let d = row["hash"].clone();
            let mut body = row.clone();
            body.as_object_mut().unwrap().remove("hash");
            let at = parse_iso_ms(row["at"].as_str().unwrap_or("")).unwrap_or(f64::NAN);
            if row["sequence"].as_f64() != Some(seq_f + 1.0)
                || row["previous"].as_str() != Some(previous.as_str())
                || d.as_str() != Some(hash(&body).as_str())
                || !at.is_finite() || at < last_at {
                return Err("RELATIVE_SOURCE_CHAIN".into());
            }
            sequence = row["sequence"].clone();
            seq_f = sequence.as_f64().unwrap_or(f64::NAN);
            previous = d.as_str().unwrap_or("").to_string();
            last_at = at;
            bytes += line.len() as u64 + 1;
            if let Some(req) = required.as_ref() {
                let seq_eq = match (row["sequence"].as_f64(), req["sequence"].as_f64()) {
                    (Some(a), Some(b)) => a == b,
                    _ => row["sequence"] == req["sequence"],
                };
                if seq_eq {
                    if d.as_str() != req["hash"].as_str() { return Err("RELATIVE_FROZEN_PREFIX_CHANGED".into()); }
                    prefix_seen = true;
                }
            }
            let snapshot = row["event"].get("bulkPrices").filter(|v| !v.is_null()).cloned()
                .unwrap_or_else(|| if row["event"]["type"].as_str() == Some("market_price_observation") {
                    row["event"]["snapshot"].clone()
                } else { V::Null });
            if js_truthy(Some(&snapshot)) {
                crate::market_price_publication::validate_price_observation_pub(&snapshot, universe)?;
                snapshots.push(json!({"snapshot": snapshot,
                    "proof": {"source": base, "sequence": sequence, "hash": d}}));
            }
        }
        if !prefix_seen { return Err("RELATIVE_FROZEN_PREFIX_MISSING".into()); }
        sources.push(json!({"file": base, "sequence": sequence, "hash": previous, "bytes": bytes}));
    }
    if required_prefixes.iter().any(|p| !sources.iter().any(|s| s["file"] == p["file"])) {
        return Err("RELATIVE_FROZEN_SOURCE_MISSING".into());
    }
    snapshots.sort_by(|a, b| {
        let ka = parse_iso_ms(a["snapshot"]["receivedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN);
        let kb = parse_iso_ms(b["snapshot"]["receivedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN);
        ka.partial_cmp(&kb).unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut seen = HashSet::new();
    let snapshots: Vec<V> = snapshots.into_iter()
        .filter(|x| seen.insert(hash(&x["snapshot"]))).collect();
    Ok(json!({"sources": sources, "snapshots": snapshots}))
}

/// relativeEpisodeLabels(episodes, quotes, {horizonsMinutes, maxDelayMs, maxLatencyMs, tickAllowance})
pub fn relative_episode_labels(episodes: &[V], quotes: &V, horizons_minutes: &[f64],
    max_delay_ms: f64, max_latency_ms: f64, tick_allowance: f64) -> Vec<V> {
    episodes.iter().map(|e| {
        let marks: Vec<V> = horizons_minutes.iter().map(|&minutes| {
            let target = parse_iso_ms(e["quoteAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN) + minutes * 60000.0;
            let series = quotes[e["exchangeId"].as_str().unwrap_or("")].as_array().cloned().unwrap_or_default();
            let q = series.iter().find(|x| parse_iso_ms(x["at"].as_str().unwrap_or("")).unwrap_or(f64::NAN) >= target);
            let minutes_out = num_key(minutes);
            let Some(q) = q else {
                return json!({"minutes": minutes_out, "status": "pending",
                    "grossPerShare": V::Null, "bufferedPerShare": V::Null});
            };
            let q_at = parse_iso_ms(q["at"].as_str().unwrap_or("")).unwrap_or(f64::NAN);
            let q_started = parse_iso_ms(q["startedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN);
            if q_at > target + max_delay_ms {
                return json!({"minutes": minutes_out, "status": "missing_quote",
                    "grossPerShare": V::Null, "bufferedPerShare": V::Null});
            }
            let bid = q["bid"].as_f64().unwrap_or(f64::NAN);
            let ask = q["ask"].as_f64().unwrap_or(f64::NAN);
            // JS uses q.bid===null / q.ask===null — strict null, so a missing key
            // is undefined (not null) and falls through to unrepresentable.
            if q.get("bid").map(|v| v.is_null()).unwrap_or(false)
                || q.get("ask").map(|v| v.is_null()).unwrap_or(false)
                || bid <= 0.0 || ask >= 1.0 || bid >= ask
                || q_started < target || q_at - q_started > max_latency_ms {
                return json!({"minutes": minutes_out, "status": "unusable_first_quote",
                    "grossPerShare": V::Null, "bufferedPerShare": V::Null});
            }
            let micros = |x: &V| baal_execution::core::decimal_micros(&json!(js_num_string(x)), "exact");
            match micros(&q["bid"]).and_then(|g| micros(&e["entryAsk"]).map(|a| g - a))
                .and_then(|g| micros(&V::from(tick_allowance)).map(|t| (g, g - 2i128 * t))) {
                Ok((gross, buffered)) => json!({"minutes": minutes_out, "status": "observed_quote_only",
                    "at": q["at"], "grossMicros": gross.to_string(), "bufferedMicros": buffered.to_string(),
                    "grossPerShare": num_key(gross as f64 / 1e6), "bufferedPerShare": num_key(buffered as f64 / 1e6),
                    "proof": q["proof"]}),
                Err(_) => json!({"minutes": minutes_out, "status": "unrepresentable_quote_decimal",
                    "grossPerShare": V::Null, "bufferedPerShare": V::Null}),
            }
        }).collect();
        let mut out = e.as_object().cloned().unwrap_or_default();
        out.insert("marks".into(), json!(marks));
        V::Object(out)
    }).collect()
}

/// runRelativeStudy(inputs, universe, identities, policy=RELATIVE_DIP_POLICY, {candidateCutoffAt})
pub fn run_relative_study(inputs: &V, universe: &V, identities: &V, policy: &V,
    candidate_cutoff_at: Option<&str>) -> Result<V, String> {
    if let Some(c) = candidate_cutoff_at {
        match parse_iso_ms(c) {
            Some(t) if ms_to_iso(t) == c => {}
            _ => return Err("RELATIVE_CUTOFF_INVALID".into()),
        }
    }
    let cutoff = candidate_cutoff_at.and_then(parse_iso_ms).unwrap_or(f64::NAN);
    let mut state = create_relative_history(universe, identities);
    let mut cache = MovementCache::new();
    let mut episodes: Vec<V> = Vec::new();
    let mut last_race: std::collections::HashMap<String, f64> = Default::default();
    let mut last_minute: std::collections::HashMap<String, i64> = Default::default();
    let mut quotes = Map::new();
    let mut counts = json!({"features": 0, "ready": 0, "localDips": 0, "residualDips": 0, "reasons": {}});
    let spacing = policy["episodeSpacingMs"].as_f64().unwrap_or(f64::NAN);
    for item in inputs["snapshots"].as_array().cloned().unwrap_or_default() {
        let snapshot = item["snapshot"].clone();
        let proof = item["proof"].clone();
        observe_relative_prices(&mut state, &snapshot, universe, policy, &mut cache)?;
        let received = parse_iso_ms(snapshot["receivedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN);
        for q in snapshot["quotes"].as_array().cloned().unwrap_or_default() {
            let eid = q["exchangeId"].as_str().unwrap_or("").to_string();
            {
                let arr = quotes.entry(eid.clone()).or_insert_with(|| json!([]));
                arr.as_array_mut().unwrap().push(json!({"at": snapshot["receivedAt"],
                    "startedAt": snapshot["requestStartedAt"], "bid": q["bestBid"], "ask": q["bestAsk"],
                    "proof": proof}));
            }
            if candidate_cutoff_at.is_some() && received < cutoff { continue; }
            let minute = (received / 60000.0).floor() as i64;
            if last_minute.get(&eid) == Some(&minute) { continue; }
            last_minute.insert(eid.clone(), minute);
            let f = relative_dip_features(&state, &eid,
                snapshot["receivedAt"].as_str().unwrap_or(""), policy, &mut cache)?;
            counts["features"] = json!(counts["features"].as_i64().unwrap_or(0) + 1);
            {
                let reason = f["reason"].as_str().unwrap_or("").to_string();
                let n = counts["reasons"][&reason].as_i64().unwrap_or(0);
                counts["reasons"].as_object_mut().unwrap().insert(reason, json!(n + 1));
            }
            if f["ready"] != V::Bool(true) { continue; }
            counts["ready"] = json!(counts["ready"].as_i64().unwrap_or(0) + 1);
            if f["localDip"] != V::Bool(true) { continue; }
            counts["localDips"] = json!(counts["localDips"].as_i64().unwrap_or(0) + 1);
            if f["residualDip"] == V::Bool(true) {
                counts["residualDips"] = json!(counts["residualDips"].as_i64().unwrap_or(0) + 1);
            }
            let race = state["identities"][&eid]["race"].clone();
            let now = parse_iso_ms(f["quoteAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN);
            let race_key = race.as_str().unwrap_or("").to_string();
            if now - last_race.get(&race_key).copied().unwrap_or(f64::NEG_INFINITY) < spacing { continue; }
            last_race.insert(race_key, now);
            let mut e = f.as_object().cloned().unwrap_or_default();
            e.insert("race".into(), race);
            e.insert("proof".into(), proof.clone());
            episodes.push(V::Object(e));
        }
    }
    let snap = inputs["snapshots"].as_array().cloned().unwrap_or_default();
    let start = parse_iso_ms(snap.first().map(|x| x["snapshot"]["receivedAt"].as_str().unwrap_or("")).unwrap_or(""))
        .unwrap_or(f64::NAN);
    let end = parse_iso_ms(snap.last().map(|x| x["snapshot"]["receivedAt"].as_str().unwrap_or("")).unwrap_or(""))
        .unwrap_or(f64::NAN);
    let split = start + (end - start) * 0.7;
    let labels = relative_episode_labels(&episodes, &V::Object(quotes), &[5.0, 15.0], 60000.0, 20000.0, 0.005);
    let period_of = |quote_at: &V| -> &'static str {
        if candidate_cutoff_at.is_some() { return "prospective"; }
        let t = parse_iso_ms(quote_at.as_str().unwrap_or("")).unwrap_or(f64::NAN);
        if t + 900000.0 < split { "development" }
        else if t >= split { "subsequent" }
        else { "purged_boundary" }
    };
    let rows: Vec<V> = labels.iter().map(|e| {
        let mut o = e.as_object().cloned().unwrap_or_default();
        o.insert("period".into(), json!(period_of(&e["quoteAt"])));
        V::Object(o)
    }).collect();
    let periods: &[&str] = if candidate_cutoff_at.is_some() { &["prospective"] }
        else { &["development", "subsequent", "purged_boundary"] };
    let mut summaries: Vec<V> = Vec::new();
    for period in periods {
        for cohort in ["local_dip", "market_relative_dip"] {
            for horizon in [5.0_f64, 15.0] {
                let eligible: Vec<&V> = rows.iter()
                    .filter(|e| e["period"].as_str() == Some(period)
                        && (cohort == "local_dip" || e["residualDip"] == V::Bool(true))).collect();
                let marks: Vec<V> = eligible.iter()
                    .filter_map(|e| e["marks"].as_array().unwrap_or(&vec![]).iter()
                        .find(|m| m["minutes"].as_f64() == Some(horizon)).cloned()).collect();
                let observed: Vec<&V> = marks.iter()
                    .filter(|m| m["status"].as_str() == Some("observed_quote_only")).collect();
                let sum = |key: &str| -> i128 {
                    observed.iter()
                        .map(|m| m[key].as_str().unwrap_or("0").parse::<i128>().unwrap_or(0)).sum()
                };
                let gross = sum("grossMicros");
                let buffered = sum("bufferedMicros");
                let positive = observed.iter()
                    .filter(|m| m["bufferedMicros"].as_str().unwrap_or("0").parse::<i128>().unwrap_or(0) > 0)
                    .count();
                let races: HashSet<String> = eligible.iter()
                    .filter_map(|e| e["race"].as_str().map(String::from)).collect();
                summaries.push(json!({
                    "period": period, "cohort": cohort, "horizonMinutes": num_key(horizon),
                    "episodes": eligible.len(), "races": races.len(),
                    "observed": observed.len(), "unobserved": marks.len() - observed.len(),
                    "meanGrossPerShare": if observed.is_empty() { V::Null }
                        else { num_key(gross as f64 / 1e6 / observed.len() as f64) },
                    "meanBufferedPerShare": if observed.is_empty() { V::Null }
                        else { num_key(buffered as f64 / 1e6 / observed.len() as f64) },
                    "positiveBuffered": positive,
                }));
            }
        }
    }
    Ok(json!({
        "schemaVersion": 1, "policy": policy, "counts": counts, "sources": inputs["sources"],
        "snapshotCount": snap.len(),
        "candidateCutoffAt": candidate_cutoff_at.map(|c| json!(c)).unwrap_or(V::Null),
        "chronologicalSplitAt": if candidate_cutoff_at.is_some() { V::Null }
            else if split.is_finite() { json!(ms_to_iso(split.trunc())) } else { V::Null },
        "rows": rows, "summaries": summaries,
        "executionAuthority": false, "networkRequests": 0, "ordersSubmitted": 0,
        "profitabilityEstablished": false,
        "limitations": [
            "Bulk quotes have no depth or quote-source timestamp; these are recovery labels, not fills or executable P&L.",
            "Party orientation is inferred from explicit cached titles; no resolution equivalence or calibrated factor loading is asserted.",
            if candidate_cutoff_at.is_none() {
                "Fixed unit factor loading and thresholds are hypotheses; this historical diagnostic was designed after prior observations and is not untouched prospective proof."
            } else {
                "Prospective labels use a frozen future cutoff and definition; adequate independent outcomes and executable depth are still required."
            },
            "Overlapping races and broad events may correlate observations; repeated polling is not independent evidence.",
        ],
    }))
}
