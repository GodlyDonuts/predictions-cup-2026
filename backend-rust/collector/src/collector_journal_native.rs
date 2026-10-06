// live-paper/collector-journal-native.mjs — collector native journal config.
use baal_execution::core::payload_hash;
use serde_json::{json, Value as V};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// The workspace root the JS resolves from live-paper/*.mjs (repo root).
fn workspace_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
/// collectorJournalCodeIdentity() — frozen-file snapshot of this module.
pub fn collector_journal_code_identity() -> Result<V, String> {
    let files: BTreeMap<String, String> = crate::frozen_paper_graph::collect_frozen_paper_files(
        &workspace_dir(), &["live-paper/collector-journal-native.mjs".to_string()])?;
    Ok(json!(files.into_iter().map(|(k, v)| (k, V::String(v))).collect::<serde_json::Map<String, V>>()))
}
/// collectorNativeJournalConfig(runtimeDir) — null when absent; strict binding
/// and engine-source checks when present.
pub fn collector_native_journal_config(runtime_dir: &Path) -> Result<Option<V>, String> {
    let file = runtime_dir.join("collector-native-journal.json");
    if !file.exists() { return Ok(None); }
    let c: V = serde_json::from_str(&fs::read_to_string(&file).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let identity = collector_journal_code_identity()?;
    if !(c["schemaVersion"].as_i64() == Some(1)
        && c["mode"].as_str() == Some("native_original_journal_collector_v1")
        && payload_hash(&c["codeIdentity"])? == payload_hash(&identity)?
        && payload_hash(&c["codeIdentity"])? == c["sourceSnapshot"].as_str().unwrap_or("")) {
        return Err("COLLECTOR_JOURNAL_BINDING_CHANGED".into());
    }
    crate::native_journal_restore::native_journal_binding(runtime_dir, &c["nativeBackend"])?;
    if crate::native_journal_restore::journal_engine_identity(
        &runtime_dir.join("journal-native-sources").join(c["nativeBackend"]["engineHash"].as_str().unwrap_or("")))?
        != c["nativeBackend"]["engineHash"].as_str().unwrap_or("") {
        return Err("COLLECTOR_JOURNAL_SOURCE_CHANGED".into());
    }
    Ok(Some(c))
}
