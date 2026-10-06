// live-paper/network-speed-audit.mjs — descriptive pre/post deployment timing
// comparison over a read-timings JSONL capture. CLI entry omitted (no sibling
// Rust CLI pattern).
use crate::read_timing::{timing_summary, validate_timing};
use baal_service_runtime::contract::parse_iso_ms;
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Value as V};

/// auditNetworkSpeed(raw, installedAt, {beforeWindowMs})
pub fn audit_network_speed(raw: &str, installed_at: &str, before_window_ms: Option<i64>) -> Result<V, String> {
    let before_window_ms = before_window_ms.unwrap_or(600000);
    let cutoff = parse_iso_ms(installed_at).unwrap_or(f64::NAN);
    if !cutoff.is_finite() || before_window_ms <= 0 || before_window_ms > 9_007_199_254_740_991 {
        return Err("Invalid measurement window".into());
    }
    let end = raw.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let complete = &raw[..end];
    let mut events: Vec<V> = Vec::new();
    if !complete.trim().is_empty() {
        for line in complete.trim_end().split('\n') {
            events.push(validate_timing(&serde_json::from_str::<V>(line).map_err(|e| e.to_string())?)?);
        }
    }
    let queued = |e: &V| parse_iso_ms(e["queuedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN);
    let finished = |e: &V| parse_iso_ms(e["finishedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN);
    let admitted_at = |e: &V| parse_iso_ms(e["admittedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN);
    let before: Vec<V> = events.iter().filter(|e| queued(e) >= cutoff - before_window_ms as f64 && finished(e) < cutoff).cloned().collect();
    let after: Vec<V> = events.iter().filter(|e| queued(e) >= cutoff).cloned().collect();
    let mut admitted: Vec<V> = after.iter().filter(|e| !e["admittedAt"].is_null()).cloned().collect();
    admitted.sort_by(|a, b| admitted_at(a).partial_cmp(&admitted_at(b)).unwrap_or(std::cmp::Ordering::Equal));
    let mut maximum_requests_in_inclusive_minute = 0i64;
    let mut minimum_completion_gap_ms: Option<f64> = None;
    let mut j = 0usize;
    for i in 0..admitted.len() {
        let at = admitted_at(&admitted[i]);
        while at - admitted_at(&admitted[j]) > 60000.0 { j += 1; }
        maximum_requests_in_inclusive_minute = maximum_requests_in_inclusive_minute.max((i - j + 1) as i64);
        if i > 0 {
            let gap = at - finished(&admitted[i - 1]);
            minimum_completion_gap_ms = Some(match minimum_completion_gap_ms {
                None => gap,
                Some(m) => if gap.is_nan() || m.is_nan() { f64::NAN } else { gap.min(m) },
            });
        }
    }
    Ok(json!({
        "schemaVersion": 1,
        "sourceHash": sha256(complete.as_bytes()),
        "sourceBytes": complete.len(),
        "partialTailExcluded": end != raw.len(),
        "installedAt": installed_at,
        "beforeWindowMs": before_window_ms,
        "before": timing_summary(&before)?,
        "after": timing_summary(&after)?,
        "firstNewAdmissionAt": admitted.first().map(|e| e["admittedAt"].clone()).unwrap_or(V::Null),
        "lastFinishedAt": after.last().map(|e| e["finishedAt"].clone()).unwrap_or(V::Null),
        "maximumRequestsInInclusiveMinute": maximum_requests_in_inclusive_minute,
        "minimumCompletionGapMs": minimum_completion_gap_ms,
        "networkRequests": 0,
        "limitations": [
            "Descriptive windows differ in traffic mix and duration; this is not a controlled strategy-performance comparison.",
            "Timing records do not identify HTTP method; observed request totals cannot prove the write-specific budget.",
            "Write pacing is established by configuration and separate rolling-limit/restart tests.",
            "API admission delay is not signal-to-fill latency or evidence of profit."
        ],
    }))
}
