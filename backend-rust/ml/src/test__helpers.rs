// ml/test/helpers.mjs — shared fixture/config accessors for the ml test
// modules. Byte/semantic port.
use crate::core::{hash, js_stringify, ms_to_iso, parse_dataset, Dataset, R};
use crate::generate_fixture::fixture_default;
use serde_json::{json, Value as V};

/// config — JSON.parse(readFileSync('../fixtures/config.json')) bound at module
/// load. include_str! resolves relative to this source file the same way the
/// import.meta.url-relative read resolves relative to helpers.mjs.
pub fn config() -> V {
    serde_json::from_str(include_str!("../../../ml/fixtures/config.json")).unwrap()
}

/// runtime = { implementationSha256: hash('unit-test-runtime') }.
pub fn runtime() -> V {
    json!({ "implementationSha256": hash("unit-test-runtime") })
}

/// values() — fixture().trimEnd().split('\n').map(JSON.parse).
pub fn values() -> Vec<V> {
    fixture_default()
        .trim_end()
        .split('\n')
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

/// encode(rows) — rows.map(JSON.stringify).join('\n') + '\n'.
pub fn encode(rows: &[V]) -> String {
    rows.iter().map(js_stringify).collect::<Vec<_>>().join("\n") + "\n"
}

/// dataset(rows) — parseDataset(encode(rows)).
pub fn dataset(rows: &[V]) -> R<Dataset> {
    parse_dataset(&encode(rows))
}

/// at(ms) — new Date(Date.parse('2026-10-01T00:00:00.000Z') + ms).toISOString().
pub fn at(ms: i64) -> String {
    ms_to_iso(crate::core::days_from_civil(2026, 10, 1) * 86_400_000 + ms)
}

/// shortRows(n = 12) — values().slice(0, n + 1): a negative end counts back
/// from the tail, anything past the length clamps to the length.
pub fn short_rows(n: i64) -> Vec<V> {
    let v = values();
    let len = v.len() as i64;
    let raw = n.saturating_add(1);
    let end = if raw < 0 { (raw + len).max(0) } else { raw.min(len) };
    v.into_iter().take(end as usize).collect()
}
/// shortRows() — the default 12-row prefix plus header.
pub fn short_rows_default() -> Vec<V> { short_rows(12) }

/// shiftRow(row, ms) — rewrites the three timestamps in place and returns row.
/// Map::insert on an existing key updates in place, matching `row.k = v` key
/// order; absent keys append at the end, same as JS.
pub fn shift_row(row: &mut V, ms: i64) -> &mut V {
    if let Some(m) = row.as_object_mut() {
        m.insert("receivedAt".into(), json!(at(ms.saturating_add(10))));
        m.insert("availableAt".into(), json!(at(ms.saturating_add(20))));
        m.insert("sourceAt".into(), json!(at(ms)));
    }
    row
}

/// Keys destructured out of the row before { kind: 'barrier', reason }.
const BARRIER_DROPPED: [&str; 7] = ["kind", "sourceAt", "sequence", "quality", "nextExpiryAt", "bids", "asks"];

/// barrier(row, reason = 'gap') — { ...rest, kind: 'barrier', reason }:
/// surviving keys keep their original insertion order, kind/reason append last.
/// An existing `reason` in rest keeps its position (Map::insert replaces in
/// place), matching the JS literal spread overwrite.
pub fn barrier(row: &V, reason: &str) -> V {
    let mut m = serde_json::Map::new();
    if let Some(o) = row.as_object() {
        for (k, v) in o {
            if !BARRIER_DROPPED.contains(&k.as_str()) {
                m.insert(k.clone(), v.clone());
            }
        }
    }
    m.insert("kind".into(), json!("barrier"));
    m.insert("reason".into(), json!(reason));
    V::Object(m)
}
/// barrier(row) — the default 'gap' barrier.
pub fn barrier_default(row: &V) -> V { barrier(row, "gap") }
