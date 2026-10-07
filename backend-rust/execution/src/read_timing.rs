// live-paper/read-timing.mjs — serialized-client read latency stage metrics.
// Ported into this crate (collector dependency would be circular): identical
// implementation as backend-rust/collector/src/read_timing.rs.
use baal_service_runtime::json_text::{js_int, sha256};
use serde_json::{json, Map, Value as V};

const STAGES: [&str; 6] = ["queueWaitMs", "admissionWaitMs", "headersMs", "bodyMs", "serviceMs", "totalMs"];
const LOOP_STAGES: [&str; 2] = ["eventLoopP99Ms", "eventLoopMaxMs"];
const KINDS: [&str; 9] = ["tournament", "positions", "orders", "book", "market", "prices", "history", "fills", "pnl"];
const OUTCOMES: [&str; 6] = ["success", "connection_error", "http_error", "body_error", "aborted", "blocked"];

fn date_parse(s: &str) -> Option<f64> { baal_service_runtime::contract::parse_iso_ms(s) }
// JS `e[k] !== null` is also true when the key is absent (undefined !== null).
fn nonnull(e: &V, k: &str) -> bool { e.get(k) != Some(&V::Null) }

/// timingDistribution(values) — finite-only ascending nearest-rank percentiles.
pub fn timing_distribution(values: &[V]) -> V {
    let mut a: Vec<f64> = values.iter().filter_map(|v| v.as_f64().filter(|x| x.is_finite())).collect();
    a.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    let percentile = |q: f64| -> V {
        if a.is_empty() { V::Null } else { json!(a[(a.len() as f64 * q).ceil() as usize - 1]) }
    };
    json!({"count": a.len(), "median": percentile(0.5), "p95": percentile(0.95), "p99": percentile(0.99),
        "max": a.last().map(|x| json!(x)).unwrap_or(V::Null)})
}

/// validateTiming(e)
pub fn validate_timing(e: &V) -> Result<V, String> {
    let base_fields: Vec<&str> = ["schemaVersion", "kind", "marketId", "queuedAt", "admittedAt", "finishedAt"]
        .into_iter().chain(STAGES).chain(["statusCode", "outcome"]).collect();
    let fields: Vec<&str> = if js_int(&e["schemaVersion"]) == Some(2) {
        base_fields.iter().cloned().chain(["durationClock", "eventLoopSamples"]).chain(LOOP_STAGES).collect()
    } else {
        base_fields
    };
    if !e.is_object()
        || e.as_object().unwrap().keys().any(|k| !fields.contains(&k.as_str()))
        || ![1i64, 2i64].contains(&js_int(&e["schemaVersion"]).unwrap_or(-1))
        || !e["kind"].as_str().is_some_and(|k| KINDS.contains(&k))
        || !e["outcome"].as_str().is_some_and(|o| OUTCOMES.contains(&o))
    {
        return Err("Invalid timing schema".into());
    }
    match e.get("marketId") {
        Some(V::Null) => {}
        Some(V::String(s))
            if s.as_bytes().first().is_some_and(|b| (b'1'..=b'9').contains(b))
                && s.bytes().all(|b| b.is_ascii_digit()) => {}
        _ => return Err("Invalid timing market".into()),
    }
    for k in ["queuedAt", "finishedAt"] {
        if !e[k].as_str().and_then(date_parse).is_some_and(|x| x.is_finite()) {
            return Err("Invalid timing clock".into());
        }
    }
    if nonnull(e, "admittedAt")
        && !e["admittedAt"].as_str().and_then(date_parse).is_some_and(|x| x.is_finite())
    {
        return Err("Invalid timing admission".into());
    }
    for k in STAGES {
        if nonnull(e, k) && !(e[k].as_f64().is_some_and(|x| x.is_finite() && x >= 0.0)) {
            return Err("Invalid timing duration".into());
        }
    }
    if nonnull(e, "statusCode") && !js_int(&e["statusCode"]).is_some_and(|x| (100..=599).contains(&x)) {
        return Err("Invalid timing status".into());
    }
    if js_int(&e["schemaVersion"]) == Some(2) {
        if !e["durationClock"].as_str().is_some_and(|d| ["monotonic", "injected"].contains(&d))
            || !js_int(&e["eventLoopSamples"]).is_some_and(|x| x >= 0)
        {
            return Err("Invalid timing instrumentation".into());
        }
        let zero_samples = js_int(&e["eventLoopSamples"]) == Some(0);
        for k in LOOP_STAGES {
            let bad = if zero_samples {
                nonnull(e, k)
            } else {
                !(e[k].as_f64().is_some_and(|x| x.is_finite() && x >= 0.0))
            };
            if bad {
                return Err("Invalid event-loop duration".into());
            }
        }
        if js_int(&e["eventLoopSamples"]).unwrap_or(0) > 0
            && e["eventLoopP99Ms"].as_f64().unwrap_or(f64::NAN) > e["eventLoopMaxMs"].as_f64().unwrap_or(f64::NAN)
        {
            return Err("Invalid event-loop quantiles".into());
        }
    }
    Ok(e.clone())
}

/// timingSummary(events)
pub fn timing_summary(events: &[V]) -> Result<V, String> {
    let mut groups = Map::new();
    let mut clocks = Map::new();
    for e in events {
        validate_timing(e)?;
        let clock = if js_int(&e["schemaVersion"]) == Some(1) {
            "legacy-wall-clock".to_string()
        } else {
            e["durationClock"].as_str().unwrap_or("").to_string()
        };
        let c = clocks.entry(clock).or_insert_with(|| json!(0));
        *c = json!(c.as_i64().unwrap_or(0) + 1);
        let kind = e["kind"].as_str().unwrap_or("").to_string();
        if !groups.contains_key(&kind) {
            let mut g = Map::new();
            g.insert("requests".to_string(), json!(0));
            g.insert("outcomes".to_string(), json!({}));
            for k in STAGES.iter().chain(LOOP_STAGES.iter()) {
                g.insert(k.to_string(), json!([]));
            }
            groups.insert(kind.clone(), V::Object(g));
        }
        let g = groups.get_mut(&kind).unwrap();
        g["requests"] = json!(g["requests"].as_i64().unwrap_or(0) + 1);
        let outcome = e["outcome"].as_str().unwrap_or("").to_string();
        g["outcomes"][outcome.as_str()] = json!(g["outcomes"][outcome.as_str()].as_i64().unwrap_or(0) + 1);
        for k in STAGES.iter().chain(LOOP_STAGES.iter()) {
            g[k].as_array_mut().unwrap().push(e[k].clone());
        }
    }
    for (_, g) in groups.iter_mut() {
        for k in STAGES.iter().chain(LOOP_STAGES.iter()) {
            let values = g[k].as_array().cloned().unwrap_or_default();
            g[k] = timing_distribution(&values);
        }
    }
    Ok(json!({"records": events.len(), "clocks": V::Object(clocks),
        "percentileMethod": "nearest-rank", "groups": V::Object(groups)}))
}

/// analyzeReadTimings(raw) — complete lines only; a partial tail is excluded.
pub fn analyze_read_timings(raw: &str) -> Result<V, String> {
    let complete = match raw.rfind('\n') {
        Some(i) => &raw[..i + 1],
        None => "",
    };
    let mut events = Vec::new();
    if !complete.is_empty() {
        for line in complete.trim_end().split('\n') {
            let parsed: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
            events.push(validate_timing(&parsed)?);
        }
    }
    let mut gaps = Vec::new();
    for i in 1..events.len() {
        let queued = date_parse(events[i]["queuedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN);
        let finished = date_parse(events[i - 1]["finishedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN);
        gaps.push(json!((queued - finished).max(0.0)));
    }
    let summary = timing_summary(&events)?;
    let mut out = json!({"schemaVersion": 2,
        "sourceHash": sha256(complete.as_bytes()),
        "sourceBytes": complete.len(),
        "partialTailExcluded": complete.len() != raw.len(),
        "firstQueuedAt": events.first().map(|e| e["queuedAt"].clone()).unwrap_or(V::Null),
        "lastFinishedAt": events.last().map(|e| e["finishedAt"].clone()).unwrap_or(V::Null)});
    for k in ["records", "clocks", "percentileMethod", "groups"] {
        out[k] = summary[k].clone();
    }
    out["outsideClientGapMs"] = timing_distribution(&gaps);
    out["interpretation"] = json!({
        "networkRequests": 0,
        "headersMs": "Time to headers or failed/cancelled header wait",
        "bodyMs": "Body reading and JSON parsing, including failures; v2 transport also measures HTTP-error body reads",
        "queueWaitMs": "Waiting behind earlier calls on this same serialized client",
        "admissionWaitMs": "Pacing/cooldown wait plus admission publication",
        "outsideClientGapMs": "Wall-clock caller dependency/processing/explicit sleep or restart gap; not an isolated CPU measurement",
        "eventLoopP99Ms": "Node event-loop timer delay in the interval since the preceding timing record; 20ms sampling resolution, not per-request CPU time or pure blocking overhead",
        "rateOrConcurrencyChanged": false,
        "limitations": [
            "V1 durations use wall time. V2 production durations use a monotonic clock; persisted dates and outside-client gaps still use wall time.",
            "Injected clocks are marked separately. Mixed historical/production samples require care when comparing latencies.",
            "Empty event-loop samples are null, not proof of zero delay. Event-loop statistics are process-wide window measurements.",
            "Small correlated samples do not justify a higher request budget or reliable tail extrapolation.",
            "No credentials, URLs, cursors, headers, account bodies or error messages are recorded.",
            "These measurements are not exchange matching latency, complete signal-to-fill tracing, or evidence of profit.",
        ]});
    Ok(out)
}
