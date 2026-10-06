// live-paper/label-paths.mjs — group rows by instrument preserving order.
use serde_json::Value as V;
use indexmap::IndexMap;
/// instrumentLabelPaths(rows) -> Vec<(stream_rows, index)> as pairs of row indexes.
/// JS returns {rows: stream, index}; we return row indices into the input to avoid cloning.
pub fn instrument_label_paths(rows: &[V]) -> Vec<(usize, usize)> {
    let mut instruments: IndexMap<String, usize> = IndexMap::new();
    let mut paths = Vec::with_capacity(rows.len());
    for (i, row) in rows.iter().enumerate() {
        let key = row.get("instrument").and_then(V::as_str).unwrap_or("").to_string();
        let next = instruments.len();
        let stream = *instruments.entry(key).or_insert(next);
        let _ = stream;
        paths.push((i, 0));
    }
    // JS path entries share the per-instrument stream; replicate as (first_index, ordinal).
    let mut counts: IndexMap<String, usize> = IndexMap::new();
    for (i, row) in rows.iter().enumerate() {
        let key = row.get("instrument").and_then(V::as_str).unwrap_or("").to_string();
        let ord = *counts.get(&key).unwrap_or(&0);
        counts.insert(key, ord + 1);
        paths[i] = (i, ord);
    }
    paths
}
