// live-paper/label-paths.mjs — per-instrument row streams.
// JS returns [{rows: <shared live stream>, index}] per row; the stream array is
// shared by reference so at use time it holds every row of that instrument.
// Rust: return the full index lists; caller uses (stream_indices, ordinal).
use indexmap::IndexMap;
use serde_json::Value as V;

pub struct LabelPaths {
    /// per-instrument indices into `rows`, in encounter order.
    pub streams: Vec<Vec<usize>>,
    /// paths[i] = (stream_id, ordinal in that stream)
    pub paths: Vec<(usize, usize)>,
}
pub fn instrument_label_paths(rows: &[V]) -> LabelPaths {
    let mut ids: IndexMap<String, usize> = IndexMap::new();
    let mut streams: Vec<Vec<usize>> = Vec::new();
    let mut paths = Vec::with_capacity(rows.len());
    for (i, row) in rows.iter().enumerate() {
        let key = row.get("instrument").and_then(V::as_str).unwrap_or("").to_string();
        let next = streams.len();
        let sid = *ids.entry(key).or_insert(next);
        if sid == streams.len() { streams.push(Vec::new()); }
        paths.push((sid, streams[sid].len()));
        streams[sid].push(i);
    }
    LabelPaths { streams, paths }
}
