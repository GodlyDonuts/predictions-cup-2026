// feed/*.mjs -> Rust port: offline market-data collection, capture and replay.
pub mod capture;
pub mod cli;
pub mod collector;
pub mod contract;
pub mod replay;
pub mod runtime;

use serde_json::Value as V;

pub fn ms_to_iso(ms: f64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ms as i64)
        .map(|d| d.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
        .unwrap_or_default()
}

pub fn js_str(v: &V) -> Option<String> {
    v.as_str().map(|s| s.to_string())
}

pub fn obj() -> V {
    V::Object(serde_json::Map::new())
}
