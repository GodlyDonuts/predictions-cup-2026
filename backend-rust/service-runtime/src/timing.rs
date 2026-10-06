use crate::atomic::need;
use crate::json_text::sha256;
use serde_json::{json, Map, Value as V};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// latency-clock.mjs: monotonic durations, wall-clock persisted dates. The
// Node event-loop histogram is replaced by a sampler thread measuring timer
// scheduling delay at 20ms resolution — the same quantity.
pub struct LatencyClock {
    start: Instant,
    source: &'static str,
    sampler: Option<LatencySampler>,
}
struct LatencySampler {
    inner: Arc<Mutex<SamplerInner>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}
struct SamplerInner {
    samples: Vec<f64>,
}
impl LatencySampler {
    fn start() -> LatencySampler {
        let inner = Arc::new(Mutex::new(SamplerInner { samples: vec![] }));
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let i2 = inner.clone();
        let s2 = stop.clone();
        let handle = std::thread::spawn(move || {
            let resolution = Duration::from_millis(20);
            while !s2.load(std::sync::atomic::Ordering::Relaxed) {
                let t0 = Instant::now();
                std::thread::sleep(resolution);
                let delay = t0.elapsed().as_secs_f64() * 1000.0 - 20.0;
                if let Ok(mut g) = i2.lock() {
                    g.samples.push(delay.max(0.0));
                }
            }
        });
        LatencySampler { inner, stop, handle: Some(handle) }
    }
    fn take(&self) -> Vec<f64> {
        self.inner.lock().map(|mut g| std::mem::take(&mut g.samples)).unwrap_or_default()
    }
}
impl Drop for LatencySampler {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}
impl LatencyClock {
    pub fn new(monitor: bool) -> LatencyClock {
        LatencyClock {
            start: Instant::now(),
            source: "monotonic",
            sampler: if monitor { Some(LatencySampler::start()) } else { None },
        }
    }
    pub fn injected() -> LatencyClock {
        LatencyClock { start: Instant::now(), source: "injected", sampler: None }
    }
    // Milliseconds on the monotonic clock, matching performance.now().
    pub fn now(&self) -> f64 {
        self.start.elapsed().as_secs_f64() * 1000.0
    }
    pub fn source(&self) -> &'static str {
        self.source
    }
    pub fn snapshot(&mut self) -> V {
        let samples = self.sampler.as_ref().map(|s| s.take()).unwrap_or_default();
        let mut a = samples;
        a.sort_by(|x, y| x.partial_cmp(y).unwrap());
        let percentile = |q: f64| -> Option<f64> {
            if a.is_empty() { None } else { Some(a[(a.len() as f64 * q).ceil().max(1.0) as usize - 1]) }
        };
        json!({
            "eventLoopSamples": a.len(),
            "eventLoopP99Ms": percentile(0.99),
            "eventLoopMaxMs": a.last().copied(),
        })
    }
}

const STAGES: [&str; 6] = ["queueWaitMs", "admissionWaitMs", "headersMs", "bodyMs", "serviceMs", "totalMs"];
const LOOP_STAGES: [&str; 2] = ["eventLoopP99Ms", "eventLoopMaxMs"];

use crate::json_text::js_int;
const KINDS: [&str; 9] = [
    "tournament", "positions", "orders", "book", "market", "prices", "history", "fills", "pnl",
];
const OUTCOMES: [&str; 6] =
    ["success", "connection_error", "http_error", "body_error", "aborted", "blocked"];

fn parse_date(s: &str) -> Option<f64> {
    chrono::DateTime::parse_from_rfc3339(s).ok().map(|d| d.timestamp_millis() as f64)
}
fn finite_nonneg(v: &V) -> bool {
    v.is_null() || v.as_f64().is_some_and(|x| x.is_finite() && x >= 0.0)
}

const BASE_FIELD_LIST: [&str; 14] = [
    "schemaVersion", "kind", "marketId", "queuedAt", "admittedAt", "finishedAt",
    "queueWaitMs", "admissionWaitMs", "headersMs", "bodyMs", "serviceMs", "totalMs",
    "statusCode", "outcome",
];
const V2_FIELD_LIST: [&str; 4] = ["durationClock", "eventLoopSamples", "eventLoopP99Ms", "eventLoopMaxMs"];

// read-timing.mjs validateTiming: extra keys reject; missing keys fall
// through to the per-field checks which then fail on the undefined value.
pub fn validate_timing(e: &V) -> Result<V, String> {
    let sv = e.get("schemaVersion").and_then(V::as_i64).unwrap_or(-1);
    let keys_ok = e
        .as_object()
        .map(|m| {
            m.keys().all(|k| {
                BASE_FIELD_LIST.contains(&k.as_str())
                    || (sv == 2 && V2_FIELD_LIST.contains(&k.as_str()))
            })
        })
        .unwrap_or(false);
    need(
        e.is_object() && !e.is_array() && keys_ok && [1, 2].contains(&sv),
        "Invalid timing schema",
    )?;
    need(
        KINDS.contains(&e.get("kind").and_then(V::as_str).unwrap_or("")),
        "Invalid timing schema",
    )?;
    need(
        OUTCOMES.contains(&e.get("outcome").and_then(V::as_str).unwrap_or("")),
        "Invalid timing schema",
    )?;
    let market = e.get("marketId").unwrap_or(&V::Null);
    if !market.is_null() {
        let ok = market
            .as_str()
            .is_some_and(|m| !m.is_empty() && !m.starts_with('0') && m.bytes().all(|b| b.is_ascii_digit()));
        need(ok, "Invalid timing market")?;
    }
    for k in ["queuedAt", "finishedAt"] {
        need(
            e.get(k).and_then(V::as_str).and_then(parse_date).is_some(),
            "Invalid timing clock",
        )?;
    }
    let admitted = e.get("admittedAt").unwrap_or(&V::Null);
    need(
        admitted.is_null() || admitted.as_str().and_then(parse_date).is_some(),
        "Invalid timing admission",
    )?;
    for k in STAGES {
        need(finite_nonneg(e.get(k).unwrap_or(&V::Null)), "Invalid timing duration")?;
    }
    let status = e.get("statusCode").unwrap_or(&V::Null);
    need(
        status.is_null() || js_int(status).is_some_and(|s| (100..=599).contains(&s)),
        "Invalid timing status",
    )?;
    if sv == 2 {
        need(
            ["monotonic", "injected"].contains(&e.get("durationClock").and_then(V::as_str).unwrap_or("")),
            "Invalid timing instrumentation",
        )?;
        let samples = e.get("eventLoopSamples").map(|v| js_int(v)).flatten();
        need(samples.is_some_and(|n| n >= 0), "Invalid timing instrumentation")?;
        for k in LOOP_STAGES {
            let v = e.get(k).unwrap_or(&V::Null);
            need(
                if samples == Some(0) { v.is_null() } else { v.as_f64().is_some_and(|x| x.is_finite() && x >= 0.0) },
                "Invalid event-loop duration",
            )?;
        }
        if samples.unwrap_or(0) > 0 {
            let p99 = e.get("eventLoopP99Ms").and_then(V::as_f64);
            let max = e.get("eventLoopMaxMs").and_then(V::as_f64);
            need(!(p99.is_some() && max.is_some()) || p99 <= max, "Invalid event-loop quantiles")?;
        }
    }
    Ok(e.clone())
}

// read-timing.mjs timingDistribution: nearest-rank percentiles.
pub fn timing_distribution(values: &[f64]) -> V {
    let mut a: Vec<f64> = values.iter().copied().filter(|x| x.is_finite()).collect();
    a.sort_by(|x, y| x.partial_cmp(y).unwrap());
    let percentile = |q: f64| -> V {
        if a.is_empty() {
            V::Null
        } else {
            json!(a[(a.len() as f64 * q).ceil().max(1.0) as usize - 1])
        }
    };
    json!({
        "count": a.len(),
        "median": percentile(0.5),
        "p95": percentile(0.95),
        "p99": percentile(0.99),
        "max": a.last().copied().map_or(V::Null, |x| json!(x)),
    })
}

fn iso_ms(v: &V, key: &str) -> Option<f64> {
    v.get(key).and_then(V::as_str).and_then(parse_date)
}

// read-timing.mjs analyzeReadTimings.
pub fn analyze_read_timings(raw: &str) -> Result<V, String> {
    let last_newline = raw.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let complete = &raw[..last_newline];
    let mut events = Vec::new();
    if !complete.is_empty() {
        for line in complete.trim_end().split('\n') {
            let v: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
            events.push(validate_timing(&v)?);
        }
    }
    let mut outside: Vec<f64> = Vec::new();
    for i in 1..events.len() {
        let a = iso_ms(&events[i - 1], "finishedAt");
        let b = iso_ms(&events[i], "queuedAt");
        if let (Some(a), Some(b)) = (a, b) {
            outside.push((b - a).max(0.0));
        }
    }
    let mut groups: Map<String, V> = Map::new();
    let mut clocks: Map<String, V> = Map::new();
    let mut group_state: std::collections::BTreeMap<String, (u64, Map<String, V>, Vec<Vec<f64>>)> =
        std::collections::BTreeMap::new();
    for e in &events {
        let clock = if e.get("schemaVersion").and_then(V::as_i64) == Some(1) {
            "legacy-wall-clock".to_string()
        } else {
            e.get("durationClock").and_then(V::as_str).unwrap_or("").to_string()
        };
        let c = clocks.entry(clock).or_insert(json!(0));
        *c = json!(c.as_i64().unwrap_or(0) + 1);
        let kind = e.get("kind").and_then(V::as_str).unwrap_or("").to_string();
        let entry = group_state.entry(kind.clone()).or_insert_with(|| {
            let mut outcomes = Map::new();
            outcomes.insert(String::new(), V::Null);
            outcomes.clear();
            (0, outcomes, vec![Vec::new(); STAGES.len() + LOOP_STAGES.len()])
        });
        entry.0 += 1;
        let outcome = e.get("outcome").and_then(V::as_str).unwrap_or("").to_string();
        let o = entry.1.entry(outcome).or_insert(json!(0));
        *o = json!(o.as_i64().unwrap_or(0) + 1);
        for (i, k) in STAGES.iter().chain(LOOP_STAGES.iter()).enumerate() {
            if let Some(x) = e.get(*k).and_then(V::as_f64) {
                entry.2[i].push(x);
            }
        }
    }
    for (kind, (requests, outcomes, columns)) in group_state {
        let mut g = Map::new();
        g.insert("requests".into(), json!(requests));
        g.insert("outcomes".into(), V::Object(outcomes));
        for (i, k) in STAGES.iter().chain(LOOP_STAGES.iter()).enumerate() {
            g.insert((*k).to_string(), timing_distribution(&columns[i]));
        }
        groups.insert(kind, V::Object(g));
    }
    Ok(json!({
        "schemaVersion": 2,
        "sourceHash": sha256(complete.as_bytes()),
        "sourceBytes": complete.len(),
        "partialTailExcluded": complete.len() != raw.len(),
        "firstQueuedAt": events.first().and_then(|e| e.get("queuedAt").cloned()).unwrap_or(V::Null),
        "lastFinishedAt": events.last().and_then(|e| e.get("finishedAt").cloned()).unwrap_or(V::Null),
        "records": events.len(),
        "clocks": V::Object(clocks),
        "percentileMethod": "nearest-rank",
        "groups": V::Object(groups),
        "outsideClientGapMs": timing_distribution(&outside),
        "interpretation": {
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
                "These measurements are not exchange matching latency, complete signal-to-fill tracing, or evidence of profit."
            ]
        }
    }))
}
