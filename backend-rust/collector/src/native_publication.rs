// live-paper/native-publication.mjs — frozen native publication binding/state.
use baal_execution::core::payload_hash;
use baal_service_runtime::json_text::sha256;
use serde_json::Value as V;
use std::path::{Path, PathBuf};

fn need(ok: bool, code: &str) -> Result<(), String> { if !ok { return Err(format!("NATIVE_PUBLICATION_{}", code)); } Ok(()) }
fn hash_str(v: &V) -> bool { v.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())) }
/// nativePublicationBinding(runtimeDir, binding) — validates + returns executable path.
pub fn native_publication_binding(runtime_dir: &Path, binding: &V) -> Result<PathBuf, String> {
    need(runtime_dir.is_absolute() && binding["schemaVersion"].as_i64() == Some(1)
        && binding["mode"].as_str() == Some("native_public_book_prefix_v1")
        && ["binaryHash", "engineHash", "runtimeLibraryHash"].iter().all(|k| hash_str(&binding[*k])), "BINDING")?;
    let root = runtime_dir.join("native-builds").join(binding["binaryHash"].as_str().unwrap());
    let executable = root.join("baal-backend.exe");
    need(sha256(&std::fs::read(&executable).map_err(|_| "NATIVE_PUBLICATION_BINARY_CHANGED".to_string())?)
            == binding["binaryHash"].as_str().unwrap()
        && sha256(&std::fs::read(root.join("libunwind.dll")).map_err(|_| "NATIVE_PUBLICATION_BINARY_CHANGED".to_string())?)
            == binding["runtimeLibraryHash"].as_str().unwrap(), "BINARY_CHANGED")?;
    Ok(executable)
}
/// nativePublicationState({runtimeDir, binding, universe, journal, endByte, expectedSha, receiptDir})
pub fn native_publication_state(runtime_dir: &Path, binding: &V, universe: &V, journal: &Path,
    end_byte: u64, expected_sha: &str, receipt_dir: &Path) -> Result<V, String> {
    let executable = native_publication_binding(runtime_dir, binding)?;
    let scope_bytes = serde_json::to_string(universe).map_err(|e| e.to_string())? + "\n";
    std::fs::create_dir_all(receipt_dir).map_err(|_| "NATIVE_PUBLICATION_RECEIPT_DIR".to_string())?;
    let id = uuid_v4();
    let scope = receipt_dir.join(format!("{}.scope.json", id));
    let out = receipt_dir.join(format!("{}.state.json", id));
    std::fs::OpenOptions::new().write(true).create_new(true).open(&scope)
        .and_then(|mut f| std::io::Write::write_all(&mut f, scope_bytes.as_bytes()))
        .map_err(|_| "NATIVE_PUBLICATION_RECEIPT_DIR".to_string())?;
    let result = crate::rust_backend::read_native_publication_prefix(journal, &scope, end_byte,
        expected_sha, &out, &executable, binding["binaryHash"].as_str().unwrap())?;
    native_publication_binding(runtime_dir, binding)?;
    let trusted = crate::book_publication_v2::create_book_publication_state(universe)?;
    need(result["engineHash"] == binding["engineHash"] && result["scopeHash"] == trusted["scopeHash"]
        && payload_hash(&result["state"]["scope"]).ok() == payload_hash(&trusted["scope"]).ok(), "ENGINE_OR_SCOPE_CHANGED")?;
    let mut state = result["state"].clone();
    state["scope"] = trusted["scope"].clone();
    let mut result = result;
    result["state"] = state;
    result["receiptFile"] = serde_json::json!(format!("{}.receipt.json", out.display()));
    Ok(result)
}
fn uuid_v4() -> String {
    let mut b = [0u8; 16];
    use std::time::{SystemTime, UNIX_EPOCH};
    let mut seed = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64 ^ std::process::id() as u64;
    for byte in &mut b {
        seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17;
        *byte = seed as u8;
    }
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    format!("{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15])
}
