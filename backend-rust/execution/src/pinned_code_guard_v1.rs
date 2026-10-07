// execution/pinned-code-guard-v1.mjs — preliminary synchronous guards check
// pinned file identity/change metadata. The physical write fence MUST request
// full bytes immediately before fetch.
//
// Port notes:
// - `filesystem=fs` collapses to std::fs (statSync -> fs::metadata,
//   lstatSync -> fs::symlink_metadata, readFileSync -> fs::read).
// - {bigint:true} stat fields render as decimal strings inside `stamp`;
//   mtimeNs/ctimeNs are sec*1e9+nsec, birthtimeNs comes from created().
// - Errors are the bare PINNED_CODE_* strings (JS `throw Error(code)`).
// - The returned checker is a PinnedCodeGuardV1::check(full) method, the
//   ({full=false}={}) closure; check() with no argument maps to check(false)
//   at the call site.
use baal_service_runtime::json_text::sha256;
use serde_json::Value as V;
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

fn sha(b: &[u8]) -> String {
    sha256(b)
}

fn stamp(m: &fs::Metadata) -> String {
    let birth_ns = m
        .created()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as i128)
        .unwrap_or(0);
    [
        m.dev().to_string(),
        m.ino().to_string(),
        m.size().to_string(),
        (m.mtime() as i128 * 1_000_000_000 + m.mtime_nsec() as i128).to_string(),
        (m.ctime() as i128 * 1_000_000_000 + m.ctime_nsec() as i128).to_string(),
        birth_ns.to_string(),
    ]
    .join(":")
}

// path.resolve(root, rel): join, then lexically collapse '.'/'..' segments.
fn resolve(root: &Path, rel: &str) -> PathBuf {
    let mut out: Vec<Component> = root.components().collect();
    for c in Path::new(rel).components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !matches!(out.last(), Some(Component::Normal(_))) {
                    out.push(c);
                } else {
                    out.pop();
                }
            }
            other => out.push(other),
        }
    }
    out.iter().collect()
}

struct Record {
    file: PathBuf,
    hash: String,
    stamp: String,
}

pub struct PinnedCodeGuardV1 {
    records: Vec<Record>,
}

/// createPinnedCodeGuardV1({workspace,identity})
/// identity is the Object.entries map of relative path -> 64-hex sha256.
pub fn create_pinned_code_guard_v1(
    workspace: &Path,
    identity: &BTreeMap<String, String>,
) -> Result<PinnedCodeGuardV1, String> {
    let root: PathBuf = workspace.components().collect();
    let mut records = Vec::new();
    for (relative, hash) in identity {
        let file = resolve(&root, relative);
        let is_hex = hash.len() == 64
            && hash
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase());
        if !file.starts_with(&root) || !is_hex {
            return Err("PINNED_CODE_SCOPE".to_string());
        }
        let stat = fs::metadata(&file).map_err(|e| e.to_string())?;
        let stable = fs::metadata(&file).map_err(|e| e.to_string())?;
        if !stat.is_file()
            || fs::symlink_metadata(&file)
                .map_err(|e| e.to_string())?
                .file_type()
                .is_symlink()
            || sha(&fs::read(&file).map_err(|e| e.to_string())?) != *hash
            || stamp(&stable) != stamp(&stat)
        {
            return Err("PINNED_CODE_INITIAL".to_string());
        }
        records.push(Record {
            file,
            hash: hash.clone(),
            stamp: stamp(&stat),
        });
    }
    Ok(PinnedCodeGuardV1 { records })
}

impl PinnedCodeGuardV1 {
    /// The returned checker: `({full=false}={})`.
    pub fn check(&self, full: bool) -> Result<bool, String> {
        for r in &self.records {
            let before = fs::metadata(&r.file).map_err(|e| e.to_string())?;
            if stamp(&before) != r.stamp {
                return Err("PINNED_CODE_CHANGED".to_string());
            }
            if full {
                let after_bytes = fs::read(&r.file).map_err(|e| e.to_string())?;
                let after = fs::metadata(&r.file).map_err(|e| e.to_string())?;
                if sha(&after_bytes) != r.hash || stamp(&after) != r.stamp {
                    return Err("PINNED_CODE_BYTES_CHANGED".to_string());
                }
            }
        }
        Ok(true)
    }
}

// Keep V imported for parity with sibling error adapters (unused otherwise).
#[allow(dead_code)]
fn _unused(_: &V) {}
