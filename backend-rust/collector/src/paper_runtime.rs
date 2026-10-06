// live-paper/runtime.mjs — paper-worker v1 serialized read client + journal/lock.
// ReadClient itself is ported at baal_service_runtime::client::ReadClient
// (identical pacing/allowlist/cooldown/manualReview semantics); this module
// re-exports it and ports exclusionSet/atomicJson/Journal/lock.
use serde_json::Value as V;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub use baal_service_runtime::client::ReadClient;
pub use baal_service_runtime::journal::Journal;
pub use baal_service_runtime::atomic::atomic_json;
pub use baal_service_runtime::exclusions::exclusion_set;
pub use baal_service_runtime::history::allowed_history_endpoint;

/// lock(file) — exclusive create + pid write; unlink on drop.
pub struct PaperLock { file: PathBuf }
impl Drop for PaperLock { fn drop(&mut self) { let _ = std::fs::remove_file(&self.file); } }
pub fn lock(file: &Path) -> Result<PaperLock, String> {
    if let Some(dir) = file.parent() { std::fs::create_dir_all(dir).map_err(|e| e.to_string())?; }
    use std::io::Write;
    let mut fd = std::fs::OpenOptions::new().write(true).create_new(true).open(file).map_err(|e| e.to_string())?;
    write!(fd, "{}", std::process::id()).map_err(|e| e.to_string())?;
    fd.sync_all().ok();
    Ok(PaperLock { file: file.to_path_buf() })
}
/// BTreeSet helper retained for parity callers that want ordered ids.
pub fn ordered_exclusion_set(positions: &V, orders: &V) -> Result<BTreeSet<String>, String> {
    Ok(baal_service_runtime::exclusions::exclusion_set(positions, orders)?.into_iter().collect())
}
