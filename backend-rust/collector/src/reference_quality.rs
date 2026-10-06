// live-paper/reference-quality.mjs — median-balanced minute quality metrics.
use baal_service_runtime::contract::parse_iso_ms;
use baal_service_runtime::history::historical_reference;
use serde_json::{json, Value as V};
use std::collections::BTreeMap;

fn parse(v: &V) -> f64 { v.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN) }
fn median(values: &[f64]) -> Option<f64> {
    let mut a = values.to_vec();
    a.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    let n = a.len();
    if n == 0 { None } else if n % 2 == 1 { Some(a[(n - 1) / 2]) } else { Some((a[n / 2 - 1] + a[n / 2]) / 2.0) }
}

// Diagnostic only: preserves the admitted legacy reference and does not change
// frozen wallets, actual orders or classify a falling price as an opportunity.
/// historicalReferenceQuality(history, books, at, policy)
pub fn historical_reference_quality(history: Option<&V>, books: &[V], at: &str, policy: &V) -> Result<V, String> {
    let reference = historical_reference(history, books, at, policy)?;
    let now = parse_iso_ms(at).unwrap_or(f64::NAN);
    if !baal_service_runtime::json_text::js_truthy(reference.get("ready")) {
        return Ok(json!({"reference": reference, "quality": V::Null}));
    }
    let candles = reference["source"].as_str() == Some("closed_trade_candle_vwap_median");
    let lookback = policy["lookbackMs"].as_f64().unwrap_or(f64::NAN);
    let source_rows: Vec<V> = if candles {
        history.map(|h| h["candles"].clone()).and_then(|c| c.as_array().cloned()).unwrap_or_default()
    } else {
        books.to_vec()
    };
    let mut points: Vec<(f64, f64)> = source_rows.iter().filter_map(|x| {
        let t = if candles { parse(&x["time"]) } else { parse(&x["at"]) };
        let keep = if candles {
            t + 60000.0 <= now && t >= now - lookback
        } else {
            t < now && parse(&x["receivedAt"]) <= now && t >= now - lookback
        };
        if keep { Some((if candles { t + 60000.0 } else { t }, x[if candles { "vwap" } else { "mid" }].as_f64().unwrap_or(f64::NAN))) } else { None }
    }).collect();
    if points.iter().any(|(t, price)| !price.is_finite() || *price < 0.0 || *price > 1.0 || !t.is_finite()) {
        return Err("INVALID_REFERENCE_POINTS".into());
    }
    points.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut minutes: BTreeMap<i64, Vec<f64>> = BTreeMap::new();
    for (t, price) in &points { minutes.entry((t / 60000.0).floor() as i64).or_default().push(*price); }
    let balanced: Vec<(i64, f64, usize)> = minutes.iter()
        .map(|(m, ps)| (*m, median(ps).unwrap_or(f64::NAN), ps.len())).collect();
    let edge = (balanced.len() as f64 / 4.0).ceil().max(1.0) as usize;
    let older = median(&balanced[..edge.min(balanced.len())].iter().map(|x| x.1).collect::<Vec<_>>());
    let recent = median(&balanced[balanced.len().saturating_sub(edge)..].iter().map(|x| x.1).collect::<Vec<_>>());
    let largest = balanced.iter().map(|x| x.2).max().unwrap_or(0) as f64;
    let largest_weight = if points.is_empty() { f64::NEG_INFINITY / 0.0 } else { largest / points.len() as f64 };
    // JS reads points.at(-1).at / points[0].at unconditionally — empty `points`
    // is a TypeError there; panic to mirror it rather than inventing a value.
    let last_at = points.last().expect("points.at(-1) is read unconditionally in the JS source").0;
    let first_at = points[0].0;
    let f = |o: Option<f64>| o.map(V::from).unwrap_or(V::Null);
    Ok(json!({
        "reference": reference,
        "quality": {
            "rawSamples": points.len(),
            "distinctMinutes": balanced.len(),
            "largestMinuteWeight": largest_weight,
            "minuteBalancedCenter": f(median(&balanced.iter().map(|x| x.1).collect::<Vec<_>>())),
            "firstQuartileCenter": f(older),
            "lastQuartileCenter": f(recent),
            "recentMinusOlder": json!(recent.unwrap_or(0.0) - older.unwrap_or(0.0)),
            "referenceMinusRecent": reference["center"].as_f64().map(|c| json!(c - recent.unwrap_or(0.0))).unwrap_or(V::Null),
            "lastObservationAgeMs": now - last_at,
            "spanMs": last_at - first_at,
            "interpretation": "Descriptive causal history only; repeated observations are not independent trials and a discount is not a probability of recovery.",
        }
    }))
}
