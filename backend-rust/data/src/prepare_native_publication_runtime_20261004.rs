// data/prepare-native-publication-runtime-20261004.mjs — archive the
// benchmarked native publication binary and source graph into the runtime dir.
use crate::publication_pack::{read_json, sha};
use baal_collector::rust_backend::native_executable;
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};

fn immutable(file: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::create_dir_all(file.parent().unwrap()).map_err(|e| e.to_string())?;
    if file.exists() {
        if sha(&std::fs::read(file).map_err(|e| e.to_string())?) != sha(bytes) {
            return Err("Native archive collision".into());
        }
        Ok(())
    } else {
        std::fs::OpenOptions::new().write(true).create_new(true).open(file)
            .and_then(|mut f| f.write_all(bytes)).map_err(|e| e.to_string())
    }
}
/// archiveNativePublication(runtimeDir, {bindingEvidence})
pub fn archive_native_publication(runtime_dir: &Path, binding_evidence: Option<&V>) -> Result<V, String> {
    let benchmark = match binding_evidence {
        Some(v) => v.clone(),
        None => read_json(Path::new("data/baal-native-publication-benchmark-20261004.json"))?,
    };
    let exe = native_executable();
    let binary = std::fs::read(&exe).map_err(|e| e.to_string())?;
    let binary_hash = sha(&binary);
    let library = std::fs::read(exe.parent().unwrap().join("libunwind.dll")).map_err(|e| e.to_string())?;
    if binary_hash != benchmark["binaryHash"].as_str().unwrap_or("") || benchmark["exactStateParity"] != json!(true) {
        return Err("Benchmarked native binary changed".into());
    }
    let files = ["Cargo.toml", "Cargo.lock", "build.rs", ".cargo/config.toml", "src/main.rs",
        "src/money.rs", "src/types.rs", "src/capture.rs", "src/engine.rs", "src/replay.rs",
        "src/trace.rs", "src/sweep.rs", "src/evaluation.rs", "src/publication.rs"];
    let mut hash = Sha256::new();
    let mut sources = serde_json::Map::<String, V>::new();
    for name in &files {
        let bytes = std::fs::read(format!("backend-rust/{name}")).map_err(|e| e.to_string())?;
        hash.update((name.len() as u64).to_le_bytes());
        hash.update(name.as_bytes());
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(&bytes);
        sources.insert(name.to_string(), json!(sha(&bytes)));
    }
    let engine_hash = format!("{:x}", hash.finalize());
    if engine_hash != benchmark["engineHash"].as_str().unwrap_or("") {
        return Err("Benchmarked native source graph changed".into());
    }
    let root = runtime_dir.join("native-builds").join(&binary_hash);
    let source_root = runtime_dir.join("native-sources").join(&engine_hash);
    immutable(&root.join("baal-backend.exe"), &binary)?;
    immutable(&root.join("libunwind.dll"), &library)?;
    for name in &files {
        let bytes = std::fs::read(format!("backend-rust/{name}")).map_err(|e| e.to_string())?;
        immutable(&source_root.join(name), &bytes)?;
    }
    let manifest = json!({
        "schemaVersion": 1, "engineHash": engine_hash, "files": V::Object(sources),
    });
    immutable(&source_root.join("manifest.json"),
        format!("{}\n", serde_json::to_string_pretty(&manifest).unwrap()).as_bytes())?;
    Ok(json!({
        "schemaVersion": 1, "mode": "native_public_book_prefix_v1",
        "binaryHash": binary_hash, "engineHash": engine_hash,
        "runtimeLibraryHash": sha(&library),
    }))
}
pub fn run() -> Result<V, String> {
    let _ = PathBuf::new();
    archive_native_publication(Path::new(&crate::publication_pack::runtime_dir()), None)
}
