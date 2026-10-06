// data/archive-checked-native-graph.mjs — checked native binary/source archiver.
// Preserve the original historical benchmark helper. Current callers must
// independently establish state parity on the exact binary they are archiving.
use baal_collector::rust_backend::native_executable;
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Value as V};
use std::fs;
use std::path::{Path, PathBuf};

fn sha(b: &[u8]) -> String { sha256(b) }

fn mkdir_private(p: &Path) -> Result<(), String> {
    fs::create_dir_all(p).map_err(|e| e.to_string())?;
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(p, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn write_private_new(f: &Path, b: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let mut o = fs::OpenOptions::new().write(true).create_new(true).open(f).map_err(|e| e.to_string())?;
    o.write_all(b).map_err(|e| e.to_string())?;
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(f, fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
    }
    o.sync_all().map_err(|e| e.to_string())
}

// The u64le-prefixed (name, bytes) graph digest used by every native archive.
pub fn graph_digest(files: &[(String, Vec<u8>)]) -> String {
    let mut buf = Vec::new();
    for (name, bytes) in files {
        buf.extend_from_slice(&(name.as_bytes().len() as u64).to_le_bytes());
        buf.extend_from_slice(name.as_bytes());
        buf.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        buf.extend_from_slice(bytes);
    }
    sha256(&buf)
}

// [...fs.readFileSync('backend-rust/build.rs').matchAll(/^\s*"([^"]+)",$/gm)].map(x=>x[1])
pub fn engine_graph_list(root: &Path) -> Result<Vec<String>, String> {
    let text = fs::read_to_string(root.join("backend-rust").join("build.rs")).map_err(|e| e.to_string())?;
    let mut files = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('"') && t.ends_with("\",") {
            files.push(t[1..t.len() - 2].to_string());
        }
    }
    if files.is_empty() || files.iter().collect::<std::collections::HashSet<_>>().len() != files.len() {
        return Err("NATIVE_ARCHIVE_SOURCE_LIST".into());
    }
    Ok(files)
}

// Read the listed backend-rust sources and produce (engineHash, sources{name:hash}, bytes).
pub fn engine_graph_files(root: &Path, files: &[String]) -> Result<(String, serde_json::Map<String, V>, Vec<(String, Vec<u8>)>), String> {
    let mut sources = serde_json::Map::new();
    let mut named = Vec::new();
    for name in files {
        if Path::new(name).is_absolute() || name.contains("..") {
            return Err("NATIVE_ARCHIVE_PATH".into());
        }
        let bytes = fs::read(root.join("backend-rust").join(name)).map_err(|e| e.to_string())?;
        sources.insert(name.clone(), json!(sha(&bytes)));
        named.push((name.clone(), bytes));
    }
    Ok((graph_digest(&named), sources, named))
}

pub fn archive_checked_native_graph(runtime_dir: &Path, binary_hash: &str, engine_hash: &str, exact_state_parity: bool) -> Result<V, String> {
    let executable = native_executable();
    let binary = fs::read(&executable).map_err(|e| e.to_string())?;
    let library = fs::read(executable.parent().unwrap().join("libunwind.dll")).map_err(|e| e.to_string())?;
    if sha(&binary) != binary_hash || !exact_state_parity {
        return Err("NATIVE_ARCHIVE_UNCHECKED_BINARY".into());
    }
    let files = engine_graph_list(Path::new("."))?;
    let (digest, sources, named) = engine_graph_files(Path::new("."), &files)?;
    if digest != engine_hash {
        return Err("NATIVE_ARCHIVE_SOURCE_GRAPH_CHANGED".into());
    }
    let immutable = |file: &PathBuf, bytes: &[u8]| -> Result<(), String> {
        mkdir_private(file.parent().unwrap())?;
        if file.exists() {
            if sha(&fs::read(file).map_err(|e| e.to_string())?) != sha(bytes) {
                return Err("NATIVE_ARCHIVE_COLLISION".into());
            }
        } else {
            write_private_new(file, bytes)?;
        }
        Ok(())
    };
    let root = runtime_dir.join("native-builds").join(binary_hash);
    let source_root = runtime_dir.join("native-sources").join(engine_hash);
    immutable(&root.join("baal-backend.exe"), &binary)?;
    immutable(&root.join("libunwind.dll"), &library)?;
    for (name, bytes) in &named {
        immutable(&source_root.join(name), bytes)?;
    }
    let manifest = format!("{}\n", serde_json::to_string_pretty(&json!({
        "schemaVersion": 1, "engineHash": engine_hash, "files": sources
    })).map_err(|e| e.to_string())?);
    immutable(&source_root.join("manifest.json"), manifest.as_bytes())?;
    Ok(json!({"schemaVersion": 1, "mode": "native_public_book_prefix_v1",
        "binaryHash": binary_hash, "engineHash": engine_hash, "runtimeLibraryHash": sha(&library)}))
}
