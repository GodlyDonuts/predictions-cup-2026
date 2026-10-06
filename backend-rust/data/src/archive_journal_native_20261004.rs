// data/archive-journal-native-20261004.mjs — immutably archive the native
// journal-restore binary, library and source graph; write the binding manifest.
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};

fn sha(b: &[u8]) -> String { sha256(b) }
fn localappdata() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).ok_or_else(|| "LOCALAPPDATA".to_string())
}
fn mkdir_700(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|e| e.to_string())?;
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn immutable(file: &Path, bytes: &[u8], collision: &str) -> Result<(), String> {
    if file.exists() {
        if sha(&std::fs::read(file).map_err(|e| e.to_string())?) != sha(bytes) {
            return Err(collision.into());
        }
    } else {
        if let Some(d) = file.parent() { mkdir_700(d)?; }
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create_new(true);
        #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; o.mode(0o600); }
        let mut f = o.open(file).map_err(|e| e.to_string())?;
        std::io::Write::write_all(&mut f, bytes).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Script body; returns the binding.
pub fn run() -> Result<V, String> {
    let r = localappdata()?.join("SigTradingCup").join("paper-runtime");
    let source = std::env::current_dir().map_err(|e| e.to_string())?.join("backend-rust/journal-restore");
    let binary = std::fs::read(source.join("target/release/baal-journal-restore.exe")).map_err(|e| e.to_string())?;
    let library = std::fs::read(source.join("target/release/libunwind.dll")).map_err(|e| e.to_string())?;
    let engine_hash = baal_collector::native_journal_restore::journal_engine_identity(&source)?;
    let binding = json!({"schemaVersion": 1, "mode": "canonical_js_journal_restore_v1",
        "binaryHash": sha(&binary), "engineHash": engine_hash,
        "runtimeLibraryHash": sha(&library)});
    let build_dir = r.join("journal-native-builds").join(binding["binaryHash"].as_str().unwrap());
    let source_dir = r.join("journal-native-sources").join(binding["engineHash"].as_str().unwrap());
    mkdir_700(&build_dir)?;
    mkdir_700(&source_dir)?;
    for (name, bytes) in [("baal-journal-restore.exe", binary.clone()), ("libunwind.dll", library.clone())] {
        immutable(&build_dir.join(name), &bytes, "Archived journal binary changed")?;
    }
    let mut files = serde_json::Map::new();
    for name in baal_collector::native_journal_restore::JOURNAL_SOURCE_FILES {
        let bytes = std::fs::read(source.join(name)).map_err(|e| e.to_string())?;
        let file = source_dir.join(name);
        files.insert(name.to_string(), json!(sha(&bytes)));
        immutable(&file, &bytes, "Archived journal source changed")?;
    }
    baal_service_runtime::atomic::atomic_json_canonical(&source_dir.join("manifest.json"),
        &json!({"schemaVersion": 1, "engineHash": binding["engineHash"], "files": V::Object(files)}))?;
    baal_service_runtime::atomic::atomic_json_canonical(
        Path::new("data/journal-native-binding-20261004.json"), &binding)?;
    Ok(binding)
}
