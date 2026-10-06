// live-paper/collector-publication-native.mjs — collector's native-publication
// config gate: code identity + scope hash must match the frozen sources.
use baal_execution::core::payload_hash;
use baal_service_runtime::json_text::js_int;
use serde_json::Value as V;
use std::collections::BTreeMap;
use std::path::Path;

/// collectorPublicationCodeIdentity() — `workspace` is the repo root holding
/// the live-paper sources (path.dirname(path.dirname(import.meta)) analog).
pub fn collector_publication_code_identity(workspace: &Path) -> Result<BTreeMap<String, String>, String> {
    crate::frozen_paper_graph::collect_frozen_paper_files(workspace, &[
        "live-paper/collector-publication-native.mjs".to_string(),
        "live-paper/book-publication-v3.mjs".to_string(),
    ])
}
/// collectorNativePublicationConfig(runtimeDir, universe)
pub fn collector_native_publication_config(runtime_dir: &Path, universe: &V, workspace: &Path) -> Result<Option<V>, String> {
    let file = runtime_dir.join("collector-native-publication.json");
    if !file.exists() { return Ok(None); }
    let c: V = serde_json::from_slice(&std::fs::read(&file).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let identity = collector_publication_code_identity(workspace)?;
    let identity_value = serde_json::to_value(&identity).map_err(|e| e.to_string())?;
    if js_int(&c["schemaVersion"]) != Some(1)
        || c["mode"].as_str() != Some("native_publication_collector_v1")
        || c["scopeHash"] != crate::book_publication_v2::create_book_publication_state(universe)?["scopeHash"]
        || payload_hash(&c["codeIdentity"]).ok() != payload_hash(&identity_value).ok() {
        return Err("COLLECTOR_NATIVE_BINDING_CHANGED".into());
    }
    crate::native_publication::native_publication_binding(runtime_dir, &c["nativeBackend"])?;
    Ok(Some(c))
}
