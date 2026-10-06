// live-paper/portfolio-replay-pool.mjs — shards the selected models across
// replay workers and merges per-candidate point paths.
// JS worker_threads become scoped std::threads over the shared source.
use serde_json::{json, Value as V};
use std::collections::BTreeSet;

use baal_service_runtime::json_text::{js_int, js_truthy};
use indexmap::IndexMap;

use crate::native_dip_host_v1::jstr;
use crate::portfolio_bank::{is_verified_portfolio_source, ProgressFn, ReplayJob};

fn scope(items: &[V]) -> Result<Vec<String>, String> {
    let mut set: BTreeSet<String> = BTreeSet::new();
    for m in items {
        let coverage: Vec<V> = match m.get("coverage") {
            None => Vec::new(),
            Some(V::Null) => Vec::new(),
            Some(V::Array(a)) => a.clone(),
            Some(v) if !js_truthy(Some(v)) => Vec::new(),
            Some(_) => return Err("(m.coverage || []).filter is not a function".into()),
        };
        for c in &coverage {
            if !js_truthy(c.get("eligible")) { continue; }
            let instruments: Vec<V> = match c.get("trainingInstruments") {
                None => Vec::new(),
                Some(V::Null) => Vec::new(),
                Some(V::Array(a)) => a.clone(),
                Some(v) if !js_truthy(Some(v)) => Vec::new(),
                Some(_) => return Err("(c.trainingInstruments || []).map is not a function".into()),
            };
            for id in &instruments {
                set.insert(format!("{}:{}", jstr(&c["marketId"]), jstr(id)));
            }
        }
    }
    Ok(set.into_iter().collect())
}

/// partitionReplayModels(models, workers) — round-robin shards; falls back to a
/// single shard when any shard's eligible instrument scope differs from the
// Histories are shared across candidate wallets. Every shard must advance the
// same supported instruments; otherwise use one worker to preserve causality.
pub fn partition_replay_models(models: &[V], workers: i64) -> Result<Vec<Vec<V>>, String> {
    if js_int(&json!(workers)).is_none() || !(1..=8).contains(&workers) {
        return Err("Invalid offline replay worker count".into());
    }
    let n = (workers as usize).min(models.len());
    let mut parts: Vec<Vec<V>> = (0..n).map(|_| Vec::new()).collect();
    for (i, m) in models.iter().enumerate() {
        parts[i % n].push(m.clone());
    }
    let full = scope(models)?;
    let same = parts.iter().try_fold(true, |ok, part| scope(part).map(|s| ok && s == full))?;
    Ok(if same { parts } else { vec![models.to_vec()] })
}

/// parallelPortfolioReplay(job, {workers}) — merged paths Map (insertion order).
pub fn parallel_portfolio_replay(job: &ReplayJob, workers: i64, on_progress: &ProgressFn)
    -> Result<IndexMap<String, Vec<V>>, String> {
    if js_int(&json!(workers)).is_none() || !(1..=8).contains(&workers) {
        return Err("Invalid offline replay worker count".into());
    }
    if !is_verified_portfolio_source(job.source) {
        return Err("Unverified offline portfolio source".into());
    }
    let models = job.model_bank["selected"].as_array().cloned()
        .ok_or("Cannot read properties of undefined (reading 'length')")?;
    let parts = partition_replay_models(&models, workers)?;
    if parts.is_empty() { return Ok(IndexMap::new()); }
    on_progress(&json!({"phase": "replay_worker_allocation", "requestedWorkers": workers,
        "activeWorkers": parts.len()}));
    let source = job.source;
    let config = &job.config;
    let results: Vec<Result<IndexMap<String, Vec<V>>, String>> = std::thread::scope(|scope_| {
        let handles: Vec<_> = parts.iter().enumerate().map(|(index, selected)| {
            let mut model_bank = job.model_bank.clone();
            model_bank["selected"] = json!(selected);
            scope_.spawn(move || {
                let progress = move |p: &V| {
                    let mut m = p.clone();
                    m["worker"] = json!(index);
                    on_progress(&m);
                };
                crate::portfolio_replay_worker::portfolio_replay_worker_body(&json!({
                    "records": source.records, "prefixHashes": source.prefix_hashes,
                    "config": config, "modelBank": model_bank,
                    "startCount": job.start_count, "endCounts": job.end_counts,
                }), &progress)
            })
        }).collect();
        handles.into_iter().map(|h| {
            h.join().unwrap_or_else(|_| Err("Offline replay worker failed".into()))
        }).collect()
    });
    let mut paths: IndexMap<String, Vec<V>> = IndexMap::new();
    for result in results {
        for (id, points) in result? {
            if paths.contains_key(&id) { return Err("Duplicate portfolio replay result".into()); }
            paths.insert(id, points);
        }
    }
    Ok(paths)
}
