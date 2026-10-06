// live-paper/diagnostics.mjs — replay-validated coverage/entry diagnostics.
use baal_paper::fixed::{big_amount, big_fee, TICK};
use baal_service_runtime::contract::parse_iso_ms;
use num_bigint::BigInt;
use serde_json::{json, Map, Value as V};
use std::collections::BTreeSet;

fn date_parse(v: &V) -> f64 { v.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN) }
/// /^\/markets\/(\d+)\/orderbook\?/ — market id from a book endpoint prefix.
fn endpoint_market(endpoint: &str) -> Option<String> {
    let rest = endpoint.strip_prefix("/markets/")?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() || !rest[digits.len()..].starts_with("/orderbook?") { return None; }
    Some(digits)
}
/// String(v) for the scalars the collector writes (id can be number or string).
fn js_scalar_string(v: &V) -> Option<String> {
    v.as_str().map(String::from).or_else(|| v.as_i64().map(|x| x.to_string()))
        .or_else(|| v.as_u64().map(|x| x.to_string()))
}
fn truthy(v: &V) -> bool { baal_service_runtime::json_text::js_truthy(Some(v)) }
/// JS object key order: integer-index keys ascend first, rest insertion order.
fn js_ordered(m: Map<String, V>) -> Map<String, V> {
    fn int_key(k: &str) -> Option<u64> {
        if k.is_empty() || k.len() > 10 || !k.bytes().all(|b| b.is_ascii_digit()) { return None; }
        if k.len() > 1 && k.starts_with('0') { return None; }
        k.parse::<u64>().ok().filter(|x| *x < 4_294_967_295)
    }
    let mut ints: Vec<(u64, String)> = m.keys().filter_map(|k| int_key(k).map(|i| (i, k.clone()))).collect();
    ints.sort_by_key(|x| x.0);
    let mut out = Map::new();
    for (_, k) in ints { out.insert(k.clone(), m[&k].clone()); }
    for (k, v) in m { if int_key(&k).is_none() { out.insert(k, v); } }
    out
}
fn increment(map: &mut V, key: &str) {
    let n = map[key].as_i64().unwrap_or(0);
    map[key] = json!(n + 1);
}
/// distribution(values) — finite-only ascending floor quantiles {count,min,median,p95,max}.
fn distribution(values: &[V]) -> V {
    let mut sorted: Vec<f64> = values.iter().filter_map(|v| v.as_f64().filter(|x| x.is_finite())).collect();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let q = |p: f64| -> V {
        if sorted.is_empty() { V::Null } else { json!(sorted[((sorted.len() - 1) as f64 * p).floor() as usize]) }
    };
    json!({"count": sorted.len(), "min": q(0.0), "median": q(0.5), "p95": q(0.95), "max": q(1.0)})
}
fn fresh_market_metrics() -> V {
    json!({"accepted": 0, "duplicates": 0, "uniqueVersions": 0, "gateCounts": {},
        "latencyMs": [], "sourceAgeMs": [], "samplingGapMs": [], "spread": [], "zScore": [],
        "entryEdge": [], "usableAskShares": [], "lastAt": V::Null})
}

// No fitting, order submission or credential access. The replay first validates
// the complete captured prefix; diagnostics then follow the same causal model.
/// diagnoseJournal(raw, c)
pub fn diagnose_journal(raw: &str, c: &V) -> Result<V, String> {
    let replay = crate::replay::replay_journal(Some(raw), None, c, crate::snapshot_order::LEGACY_SNAPSHOT_ORDER)?;
    let mut state: Option<V> = None;
    let mut markets = Map::new();
    let mut skip_reasons = json!({});
    let mut samples_latency: Vec<V> = Vec::new();
    let mut samples_age: Vec<V> = Vec::new();
    for line in raw.trim_end().split('\n') {
        let r: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
        let event = &r["event"];
        let clock = if truthy(&event["clockAt"]) { &event["clockAt"] } else { &r["at"] };
        let clock = clock.as_str().unwrap_or("").to_string();
        if event["type"].as_str() == Some("account_read_outage") {
            if let Some(st) = state.as_mut() {
                crate::model::invalidate(st, None, &clock, event["reason"].as_str().unwrap_or(""));
            }
            continue;
        }
        if event["type"].as_str() != Some("cycle") { continue; }
        let excluded: BTreeSet<String> = event["accountExclusions"].as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        let snapshots = crate::depth_scope::scoped_snapshots(event, c)?;
        for (index, e) in snapshots.iter().enumerate() {
            if !e["skipped"].is_null() {
                increment(&mut skip_reasons, e["skipped"].as_str().unwrap_or(""));
                if let Some(st) = state.as_mut() {
                    let mid_key = js_scalar_string(&e["marketId"]).unwrap_or_default();
                    let at = (if truthy(&e["at"]) { e["at"].as_str() } else { None })
                        .map(String::from)
                        .or_else(|| r["state"]["skips"][mid_key.as_str()]["at"].as_str().map(String::from))
                        .unwrap_or_else(|| clock.clone());
                    crate::model::invalidate(st, Some(&mid_key), &at, e["skipped"].as_str().unwrap_or(""));
                }
                continue;
            }
            let Some(mid) = endpoint_market(e["endpoint"].as_str().unwrap_or("")) else { continue };
            if !c["marketIds"].as_array().map(|a| a.iter().any(|x| x.as_str() == Some(mid.as_str()))).unwrap_or(false) { continue; }
            if state.is_none() {
                state = Some(crate::model::initial_state(c, "diagnostics", e["requestStartedAt"].as_str().unwrap_or(""))?);
            }
            let b = match crate::model::normalize_book(&e["data"], &mid, c["tournamentId"].as_str().unwrap_or(""),
                e["receivedAt"].as_str().unwrap_or(""), e["requestStartedAt"].as_str().unwrap_or(""),
                c["maxDataAgeMs"].as_i64().unwrap_or(0), &excluded, "event_age_v1") {
                Ok(b) => b,
                Err(err) => {
                    // The collector appends a raw successful-HTTP response followed by its
                    // validation skip. That adjacent event is the authoritative skip count.
                    let adjacent = snapshots.get(index + 1)
                        .is_some_and(|n| !n["skipped"].is_null() && js_scalar_string(&n["marketId"]).as_deref() == Some(mid.as_str()));
                    if !adjacent { increment(&mut skip_reasons, &err); }
                    crate::model::invalidate(state.as_mut().unwrap(), Some(&mid), e["receivedAt"].as_str().unwrap_or(""), &err);
                    continue;
                }
            };
            let market_id = b["marketId"].as_str().unwrap_or("").to_string();
            if !markets.contains_key(&market_id) { markets.insert(market_id.clone(), fresh_market_metrics()); }
            let m = &mut markets[&market_id];
            let inst = b["instrument"].as_str().unwrap_or("").to_string();
            let st_ref = state.as_ref().unwrap();
            let prev = st_ref["books"][&inst].clone();
            let prior_intents = st_ref["strategies"]["mean_reversion"]["intents"].as_i64().unwrap_or(0);
            let history: Vec<f64> = st_ref["histories"][&inst].as_array().cloned().unwrap_or_default()
                .iter().filter_map(|x| x.as_f64()).collect::<Vec<_>>()
                .into_iter().rev().take(c["lookback"].as_f64().unwrap_or(0.0).max(0.0) as usize).rev().collect();
            let st = state.take().unwrap();
            state = Some(crate::model::observe(&st, &b, c)?);
            m["accepted"] = json!(m["accepted"].as_i64().unwrap_or(0) + 1);
            let latency = date_parse(&b["receivedAt"]) - date_parse(&b["requestStartedAt"]);
            let age = date_parse(&b["receivedAt"]) - date_parse(&b["sourceAt"]);
            m["latencyMs"].as_array_mut().unwrap().push(json!(latency));
            m["sourceAgeMs"].as_array_mut().unwrap().push(json!(age));
            samples_latency.push(json!(latency));
            samples_age.push(json!(age));
            if truthy(&m["lastAt"]) {
                let gap = date_parse(&b["receivedAt"]) - date_parse(&m["lastAt"]);
                m["samplingGapMs"].as_array_mut().unwrap().push(json!(gap));
            }
            m["lastAt"] = b["receivedAt"].clone();
            let new_book = &state.as_ref().unwrap()["books"][&inst];
            if !prev.is_null() && prev["sequence"] == b["sequence"] && prev["versionKey"] == new_book["versionKey"] {
                m["duplicates"] = json!(m["duplicates"].as_i64().unwrap_or(0) + 1);
                continue;
            }
            m["uniqueVersions"] = json!(m["uniqueVersions"].as_i64().unwrap_or(0) + 1);
            let signal = state.as_ref().unwrap()["signals"][&inst].clone();
            let spread = (big_amount(b["asks"][0]["price"].as_str().unwrap_or(""))?
                - big_amount(b["bids"][0]["price"].as_str().unwrap_or(""))?)
                .to_string().parse::<f64>().unwrap_or(0.0) / 1e6;
            m["spread"].as_array_mut().unwrap().push(json!(spread));
            if !signal["zScore"].is_null() { m["zScore"].as_array_mut().unwrap().push(signal["zScore"].clone()); }
            let ask = big_amount(b["asks"][0]["price"].as_str().unwrap_or(""))? + BigInt::from(TICK) * BigInt::from(c["slippageTicks"].as_i64().unwrap_or(0));
            let mean = if !history.is_empty() {
                history.iter().sum::<f64>() / history.len() as f64
            } else {
                (big_amount(b["bids"][0]["price"].as_str().unwrap_or(""))?.to_string().parse::<f64>().unwrap_or(0.0)
                    + big_amount(b["asks"][0]["price"].as_str().unwrap_or(""))?.to_string().parse::<f64>().unwrap_or(0.0)) / 2.0
            };
            let fee = big_fee(&ask, &BigInt::from(1), &json!({"feeBps": c["feeBps"], "feePerShare": "0.000000"}))?;
            let exact_edge = mean - ask.to_string().parse::<f64>().unwrap_or(0.0) - fee.to_string().parse::<f64>().unwrap_or(0.0);
            m["entryEdge"].as_array_mut().unwrap().push(json!(exact_edge / 1e6));
            let capacity = (b["asks"][0]["quantity"].as_f64().unwrap_or(0.0) * c["depthFractionBps"].as_f64().unwrap_or(0.0) / 10000.0).floor() as i64;
            m["usableAskShares"].as_array_mut().unwrap().push(json!(capacity));
            let warm = signal["warmupRemaining"].as_f64() == Some(0.0);
            let variance = !signal["zScore"].is_null();
            let z = variance && signal["zScore"].as_f64().unwrap_or(0.0) >= c["entryZ"].as_f64().unwrap_or(0.0);
            let edge = exact_edge >= big_amount(c["minEdge"].as_str().unwrap_or(""))?.to_string().parse::<f64>().unwrap_or(0.0);
            if !warm {
                increment(&mut m["gateCounts"], "warmup");
            } else {
                increment(&mut m["gateCounts"], "postWarmup");
                if !variance { increment(&mut m["gateCounts"], "zeroVariance"); }
                if !z { increment(&mut m["gateCounts"], "zBelowThreshold"); }
                if !edge { increment(&mut m["gateCounts"], "edgeBelowThreshold"); }
                if z { increment(&mut m["gateCounts"], "zPass"); }
                if edge { increment(&mut m["gateCounts"], "edgePass"); }
                if z && edge { increment(&mut m["gateCounts"], "jointSignalPass"); }
            }
            if capacity == 0 { increment(&mut m["gateCounts"], "zeroTopAskCapacity"); }
            let now_intents = state.as_ref().unwrap()["strategies"]["mean_reversion"]["intents"].as_i64().unwrap_or(0);
            if now_intents > prior_intents { increment(&mut m["gateCounts"], "actualNewIntents"); }
        }
    }
    for (_, m) in markets.iter_mut() {
        for key in ["latencyMs", "sourceAgeMs", "samplingGapMs", "spread", "zScore", "entryEdge", "usableAskShares"] {
            let values = m[key].as_array().cloned().unwrap_or_default();
            m[key] = distribution(&values);
        }
    }
    Ok(json!({"schemaVersion": 1,
        "sourceHash": replay["replay"]["sourceHash"],
        "configHash": replay["configHash"],
        "sourceRecords": replay["replay"]["sourceRecords"],
        "finalEventClock": replay["replay"]["finalEventClock"],
        "observations": replay["observations"],
        "markets": V::Object(js_ordered(markets)),
        "skipReasons": skip_reasons,
        "latencyMs": distribution(&samples_latency),
        "sourceAgeMs": distribution(&samples_age),
        "interpretation": {
            "networkRequests": 0,
            "parametersChanged": false,
            "gateCountsOverlap": true,
            "edgeUnits": "SUSQies per share; lagged mean less ask, adverse tick and assumed entry fee",
            "scope": "Exploratory coverage and entry diagnostics, not profitability evidence",
            "limitations": [
                "Four-market sequential REST sampling misses intracycle changes",
                "Entry edge excludes subsequent exit costs and is an uncalibrated hypothesis",
                "Depth capacity is current top ask only; it is not a future fill guarantee",
                "Duplicate versions are accepted for freshness but do not extend signal history",
            ],
        }}))
}
