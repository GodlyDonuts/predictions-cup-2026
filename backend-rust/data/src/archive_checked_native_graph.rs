// data/archive-checked-native-graph.mjs — preserve the original historical
// benchmark helper: immutably archive the checked native binary, runtime library
// and source graph under runtimeDir.
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

fn sha(b: &[u8]) -> String { sha256(b) }
fn mkdir_700(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|e| e.to_string())?;
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn immutable(file: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(d) = file.parent() { mkdir_700(d)?; }
    if file.exists() {
        if sha(&std::fs::read(file).map_err(|e| e.to_string())?) != sha(bytes) {
            return Err("NATIVE_ARCHIVE_COLLISION".into());
        }
    } else {
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create_new(true);
        #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; o.mode(0o600); }
        let mut f = o.open(file).map_err(|e| e.to_string())?;
        std::io::Write::write_all(&mut f, bytes).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// archiveCheckedNativeGraph(runtimeDir, {binaryHash, engineHash, exactStateParity}).
pub fn archive_checked_native_graph(runtime_dir: &Path, binary_hash: &str,
    engine_hash: &str, exact_state_parity: bool) -> Result<V, String> {
    let native_executable = baal_collector::rust_backend::native_executable();
    let binary = std::fs::read(&native_executable).map_err(|e| e.to_string())?;
    let dll_dir: PathBuf = native_executable.parent().unwrap_or(Path::new("")).to_path_buf();
    let library = std::fs::read(dll_dir.join("libunwind.dll")).map_err(|e| e.to_string())?;
    if sha(&binary) != binary_hash || !exact_state_parity {
        return Err("NATIVE_ARCHIVE_UNCHECKED_BINARY".into());
    }
    let build_rs = std::fs::read_to_string("backend-rust/build.rs").map_err(|e| e.to_string())?;
    // Lines of the form `  "name",`
    let mut files: Vec<String> = Vec::new();
    for line in build_rs.lines() {
        let t = line.trim_start();
        if t.starts_with('"') && t.ends_with("\",") && t.len() >= 3 {
            files.push(t[1..t.len() - 2].to_string());
        }
    }
    if files.is_empty() || files.iter().collect::<std::collections::HashSet<_>>().len() != files.len() {
        return Err("NATIVE_ARCHIVE_SOURCE_LIST".into());
    }
    let mut graph = Sha256::new();
    let mut sources = serde_json::Map::new();
    let mut bytes_by_name: std::collections::HashMap<String, Vec<u8>> = std::collections::HashMap::new();
    for name in &files {
        if Path::new(name).is_absolute() || name.contains("..") {
            return Err("NATIVE_ARCHIVE_PATH".into());
        }
        let bytes = std::fs::read(Path::new("backend-rust").join(name)).map_err(|e| e.to_string())?;
        graph.update((name.as_bytes().len() as u64).to_le_bytes());
        graph.update(name.as_bytes());
        graph.update((bytes.len() as u64).to_le_bytes());
        graph.update(&bytes);
        sources.insert(name.clone(), json!(sha(&bytes)));
        bytes_by_name.insert(name.clone(), bytes);
    }
    if format!("{:x}", graph.finalize()) != engine_hash {
        return Err("NATIVE_ARCHIVE_SOURCE_GRAPH_CHANGED".into());
    }
    let root = runtime_dir.join("native-builds").join(binary_hash);
    let source_root = runtime_dir.join("native-sources").join(engine_hash);
    immutable(&root.join("baal-backend.exe"), &binary)?;
    immutable(&root.join("libunwind.dll"), &library)?;
    for name in &files {
        immutable(&source_root.join(name), &bytes_by_name[name])?;
    }
    let manifest = json!({"schemaVersion": 1, "engineHash": engine_hash, "files": V::Object(sources)});
    immutable(&source_root.join("manifest.json"),
        format!("{}\n", serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?).as_bytes())?;
    Ok(json!({"schemaVersion": 1, "mode": "native_public_book_prefix_v1",
        "binaryHash": binary_hash, "engineHash": engine_hash,
        "runtimeLibraryHash": sha(&library)}))
}
