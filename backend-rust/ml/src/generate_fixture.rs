// ml/generate-fixture.mjs — deterministic synthetic dataset writer.
// Deliberately manufactured signal, for the correctness path only.
use crate::core::{hash, js_stringify, ms_to_iso, FORMAT};
use crate::model::random;
use serde_json::{json, Value as V};

pub const FIXTURE_SEED: u64 = 20261001;
/// JS Math.min — NaN propagates, -0 < +0.
fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::NAN } else { a.min(b) }
}
fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::NAN } else { a.max(b) }
}
/// Number((x).toFixed(8)): round the exact decimal expansion to 8 fractional
/// digits (remainder >= .5 rounds up; an exact tie picks the larger n), then
/// re-parse the decimal string to binary64. {:.60} is the exact expansion —
/// every f64 terminates and the magnitudes here finish well under 60 digits.
fn to_fixed8(x: f64) -> f64 {
    if !x.is_finite() { return x; }
    let neg = x.is_sign_negative() && x != 0.0;
    let s = format!("{:.60}", x.abs());
    let frac_start = s.find('.').unwrap() + 1;
    let frac = s.as_bytes();
    let mut int_val: u128 = s[..frac_start - 1].parse().unwrap();
    let mut frac_val: u64 = frac[frac_start..frac_start + 8]
        .iter()
        .fold(0u64, |a, d| a * 10 + (d - b'0') as u64);
    if frac[frac_start + 8] >= b'5' {
        frac_val += 1;
        if frac_val == 100_000_000 { frac_val = 0; int_val += 1; }
    }
    format!("{}{}.{:08}", if neg { "-" } else { "" }, int_val, frac_val)
        .parse()
        .unwrap_or(f64::NAN)
}
/// fixture(seed = FIXTURE_SEED) — newline-terminated sig-offline-books JSONL.
pub fn fixture(seed: u64) -> String {
    let mut rng = random(seed);
    // Date.parse('2026-10-01T00:00:00.000Z')
    let start: i64 = crate::core::days_from_civil(2026, 10, 1) * 86_400_000;
    let timestamp = |ms: i64| ms_to_iso(start + ms);
    let mut rows: Vec<V> = vec![json!({
        "format": FORMAT,
        "schemaVersion": 1,
        "datasetId": "synthetic-correctness-v1",
        "dataKind": "synthetic",
        "normalization": "quality-segmented-books-v1",
        "provenance": {
            "sourceSha256": hash(format!("SYNTHETIC_GENERATOR_V1 seed={seed}; no market source")),
            "normalizerVersion": "synthetic-fixture-v1",
        },
    })];
    for s in 0..16u64 {
        let mut mid = 0.45f64;
        let mut prior_imbalance = 0.0f64;
        for i in 0..24u64 {
            let imbalance = rng() * 1.6 - 0.8;
            // Deliberately manufactured signal solely to exercise the fitting path.
            mid = js_max(0.2, js_min(0.8, mid + 0.002 * prior_imbalance + (rng() - 0.5) * 0.001));
            prior_imbalance = imbalance;
            let time = (s * 30000 + i * 1000) as i64;
            rows.push(json!({
                "kind": "book", "rowId": format!("synthetic-{s}-{i}"),
                "sessionId": format!("synthetic-session-{s}"),
                "tournamentId": "synthetic-cup", "marketId": "synthetic-market",
                "exchangeId": "synthetic-exchange", "segmentId": format!("synthetic-segment-{s}"),
                "receivedAt": timestamp(time + 10), "availableAt": timestamp(time + 20),
                "sourceAt": timestamp(time), "sequence": (s * 1000 + i) as u64,
                "quality": {"continuous": true, "recovering": false, "stale": false, "expiryKnown": true},
                "nextExpiryAt": V::Null,
                "bids": [{"price": to_fixed8(mid - 0.005), "quantity": to_fixed8(100.0 * (1.0 + imbalance))}],
                "asks": [{"price": to_fixed8(mid + 0.005), "quantity": to_fixed8(100.0 * (1.0 - imbalance))}],
            }));
        }
    }
    rows.iter().map(js_stringify).collect::<Vec<_>>().join("\n") + "\n"
}
/// fixture() with the default seed.
pub fn fixture_default() -> String { fixture(FIXTURE_SEED) }
