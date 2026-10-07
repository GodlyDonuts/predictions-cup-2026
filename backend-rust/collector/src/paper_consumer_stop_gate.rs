// live-paper/paper-consumer-stop-gate.mjs — operator STOP markers pause a
// counterfactual paper consumer while its accounting snapshot is retained.
use serde_json::{json, Map, Value as V};
use std::path::Path;

/// paperConsumerStopped(runtimeDir, filesystem=fs). `exists` is the injected
/// `filesystem.existsSync` seam; `paper_consumer_stopped` binds the real fs.
pub fn paper_consumer_stopped_with(runtime_dir: &Path, exists: impl Fn(&Path) -> bool) -> bool {
    ["EXECUTION_STOP", "COLLECTOR_STOP", "STOP"]
        .iter()
        .any(|name| exists(&runtime_dir.join(name)))
}
pub fn paper_consumer_stopped(runtime_dir: &Path) -> bool {
    paper_consumer_stopped_with(runtime_dir, |p| p.exists())
}

/// frozenPaperStopView(previous) — `{...previous, status:'paused', ...}`.
/// Object spread copies prior members in order; keys overwritten by the
/// literal keep their position, and new keys append at the end — serde_json
/// preserve_order Map::insert has the same ordering semantics.
pub fn frozen_paper_stop_view(previous: &V) -> V {
    // JS spreads: object -> own members; array/string -> indexed members;
    // null, undefined, numbers, booleans -> {}. (String spread is per UTF-16
    // code unit in JS; `chars` differs only on lone-surrogate astral input,
    // which cannot occur in a settlement-paper status view.)
    let mut out: Map<String, V> = match previous {
        V::Object(m) => m.clone(),
        V::String(s) => s
            .chars()
            .enumerate()
            .map(|(i, c)| (i.to_string(), json!(c.to_string())))
            .collect(),
        V::Array(a) => a
            .iter()
            .enumerate()
            .map(|(i, x)| (i.to_string(), x.clone()))
            .collect(),
        _ => Map::new(),
    };
    out.insert("status".into(), json!("paused"));
    out.insert("reason".into(), json!("OPERATOR_STOP"));
    out.insert("valuationFrozen".into(), V::Bool(true));
    out.insert("accountingRetained".into(), V::Bool(true));
    out.insert("executionAuthority".into(), V::Bool(false));
    V::Object(out)
}
