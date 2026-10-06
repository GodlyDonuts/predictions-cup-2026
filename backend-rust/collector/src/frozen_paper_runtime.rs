// live-paper/frozen-paper-runtime.mjs — frozen code snapshot binding.
// In the Rust port there is no dynamic module load: resolve_frozen_module returns
// the verified manifest + module bytes; hosts call the matching crate function.
use baal_execution::core::payload_hash;
use baal_service_runtime::json_text::sha256;
use serde_json::Value as V;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn need(ok: bool, code: &str) -> Result<(), String> { if !ok { return Err(format!("FROZEN_PAPER_{}", code)); } Ok(()) }
fn safe_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    need(!relative.is_empty() && relative.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'/' | b'.' | b'_' | b'-'))
        && relative.to_lowercase().ends_with(".mjs") && !relative.contains(".."), "PATH")?;
    let target = root.join(relative);
    need(target.starts_with(root), "PATH_SCOPE")?;
    Ok(target)
}
/// frozenPaperModule({runtimeDir, bank, modulePath, fallback}) — synchronous:
/// returns the module's verified bytes (caller maps them to the crate fn).
pub fn frozen_paper_module(runtime_dir: &Path, bank: &V, module_path: &str) -> Result<Option<V>, String> {
    let index_file = runtime_dir.join("frozen-code").join("index.json");
    if !index_file.exists() { return Ok(None); }
    let index: V = serde_json::from_str(&std::fs::read_to_string(&index_file).map_err(|e| e.to_string())?)
        .map_err(|_| "FROZEN_PAPER_INDEX".to_string())?;
    let binding = &index["banks"][payload_hash(bank).unwrap_or_default()];
    if binding.is_null() { return Ok(None); }
    need(binding["snapshot"].as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())), "SNAPSHOT")?;
    let root = runtime_dir.join("frozen-code").join(binding["snapshot"].as_str().unwrap());
    let manifest: V = serde_json::from_str(&std::fs::read_to_string(root.join("manifest.json")).map_err(|_| "FROZEN_PAPER_MANIFEST".to_string())?)
        .map_err(|_| "FROZEN_PAPER_MANIFEST".to_string())?;
    need(payload_hash(&manifest["files"]).ok().as_deref() == binding["snapshot"].as_str(), "MANIFEST")?;
    for (file, hash) in manifest["files"].as_object().cloned().unwrap_or_default() {
        need(sha256(&std::fs::read(safe_path(&root, &file)?).map_err(|_| "FROZEN_PAPER_FILE_CHANGED".to_string())?)
            == hash.as_str().unwrap_or(""), "FILE_CHANGED")?;
    }
    need(manifest["files"].get(module_path).is_some(), "MODULE")?;
    for (relative, hash) in bank["codeIdentity"].as_object().cloned().unwrap_or_default() {
        let file = format!("live-paper/{}", relative.trim_start_matches("./"));
        let norm = file.replace("//", "/");
        need(manifest["files"][&norm] == hash, "BANK_IDENTITY")?;
    }
    Ok(Some(serde_json::json!({"modulePath": module_path, "snapshot": binding["snapshot"],
        "verified": true, "files": manifest["files"].clone()})))
}
