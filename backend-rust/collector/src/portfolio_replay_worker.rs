// live-paper/portfolio-replay-worker.mjs — the worker-thread body. The JS worker
// receives workerData and posts {kind:'progress'} / {kind:'result'} messages;
// here the pool calls this body on a scoped thread and merges the map.
use serde_json::{json, Value as V};

use indexmap::IndexMap;

use crate::portfolio_bank::replay_portfolio_worker_job;

/// Runs replayPortfolioWorkerJob(payload) reporting each progress update through
/// `post_progress` (the JS {kind:'progress',progress} message payload is the
/// progress object itself; the pool adds the worker index).
pub fn portfolio_replay_worker_body(payload: &V,
    post_progress: &(dyn Fn(&V) + Send + Sync))
    -> Result<IndexMap<String, Vec<V>>, String> {
    replay_portfolio_worker_job(payload, post_progress)
}

/// The worker's result message entries: [...paths] — insertion-ordered pairs.
pub fn worker_result_message(paths: &IndexMap<String, Vec<V>>) -> V {
    json!({"kind": "result", "entries": paths.iter().map(|(k, v)| json!([k, v])).collect::<Vec<_>>()})
}
