// live-paper/coverage-checkpoint.mjs
use crate::paper_persistence::{atomic_paper_json, PaperPersistenceError};
use serde_json::Value as V;
use std::path::Path;
/// checkpointCoverage(file, value, {canCommit, onFault})
pub fn checkpoint_coverage(file: &Path, value: &V, can_commit: &dyn Fn() -> bool, on_fault: &dyn Fn(&V)) -> Result<V, PaperPersistenceError> {
    match atomic_paper_json(file, value, can_commit) {
        Ok(r) => Ok(r),
        Err(e) => {
            let d = &e.details;
            on_fault(&serde_json::json!({"phase": d.get("phase").cloned().unwrap_or(serde_json::json!("unknown")),
                "osCode": d.get("osCode").cloned().unwrap_or(serde_json::json!("UNKNOWN")),
                "attempts": d.get("attempts").cloned().unwrap_or(serde_json::json!(0)),
                "target": "price-coverage.json",
                "payloadHash": d.get("payloadHash").cloned().unwrap_or(V::Null)}));
            Err(e)
        }
    }
}
