// execution/pinned-code-guard-v1.mjs — preliminary synchronous guards check
// pinned file identity/change metadata. The physical write fence MUST request
// full bytes immediately before fetch.
//
// Port notes:
// - `stamp` = dev:ino:size:mtimeNs:ctimeNs:birthtimeNs, each a bigint decimal
//   string; nanoseconds are whole-epoch totals like Node's bigint statSync.
// - The `filesystem` injectable collapses to std::fs (Node fs-compatible
//   subset used here: statSync/lstatSync/readFileSync).
// - `path.resolve` is lexical normalization of the (already absolute)
//   workspace; a relative workspace resolves against the process cwd.
use baal_service_runtime::json_text::sha256;
use serde_json::Value as V;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

fn sha(b: &[u8]) -> String {
    sha256(b)
}

fn ns(t: std::io::Result<std::time::SystemTime>) -> i128 {
    t.ok()
        .map(|s| match s.duration_since(std::time::UNIX_EPOCH) {
            Ok(d) => d.as_nanos() as i128,
            Err(e) => -(e.duration().as_nanos() as i128),
        })
        .unwrap_or(0)
}

fn stamp(s: &fs::Metadata) -> String {
    format!(
        "{}:{}:{}:{}:{}:{}",
        s.dev(),
        s.ino(),
        s.len(),
        s.mtime() as i128 * 1_000_000_000 + s.mtime_nsec() as i128,
        s.ctime() as i128 * 1_000_000_000 + s.ctime_nsec() as i128,
        ns(s.created()),
    )
}

fn resolve_path(base: &Path, rel: &str) -> PathBuf {
    let rel_p = Path::new(rel);
    let joined = if rel_p.is_absolute() {
        rel_p.to_path_buf()
    } else {
        base.join(rel_p)
    };
    let mut comps: Vec<String> = Vec::new();
    for c in joined.components() {
        match c {
            Component::Normal(s) => comps.push(s.to_string_lossy().into_owned()),
            Component::ParentDir => {
                comps.pop();
            }
            _ => {}
        }
    }
    PathBuf::from(format!("/{}", comps.join("/")))
}

struct Record {
    file: PathBuf,
    hash: String,
    stamp: String,
}

/// The returned verifier: ({full=false}={}) => true | throw.
pub struct PinnedCodeGuardV1 {
    records: Vec<Record>,
}

impl PinnedCodeGuardV1 {
    pub fn check(&self, full: bool) -> Result<bool, String> {
        for r in &self.records {
            let before = fs::metadata(&r.file).map_err(|e| e.to_string())?;
            if stamp(&before) != r.stamp {
                return Err("PINNED_CODE_CHANGED".to_string());
            }
            if full {
                let bytes = fs::read(&r.file).map_err(|e| e.to_string())?;
                let after = fs::metadata(&r.file).map_err(|e| e.to_string())?;
                if sha(&bytes) != r.hash || stamp(&after) != r.stamp {
                    return Err("PINNED_CODE_BYTES_CHANGED".to_string());
                }
            }
        }
        Ok(true)
    }
}

/// createPinnedCodeGuardV1({workspace,identity,filesystem=fs})
pub fn create_pinned_code_guard_v1(
    workspace: &Path,
    identity: &V,
) -> Result<PinnedCodeGuardV1, String> {
    let ws = if workspace.is_absolute() {
        workspace.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| e.to_string())?
            .join(workspace)
    };
    let root = resolve_path(Path::new("/"), ws.to_string_lossy().as_ref());
    let root_prefix = format!("{}/", root.to_string_lossy());
    if identity.is_null() {
        // Object.entries(null|undefined) throws a TypeError in JS.
        return Err("Cannot convert undefined or null to object".to_string());
    }
    let mut records = Vec::new();
    for (relative, hash_v) in identity.as_object().into_iter().flatten() {
        let hash = hash_v.as_str().unwrap_or("");
        let file = resolve_path(&root, relative);
        if !file.to_string_lossy().starts_with(&root_prefix)
            || !(hash.len() == 64
                && hash.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
        {
            return Err("PINNED_CODE_SCOPE".to_string());
        }
        let stat = fs::metadata(&file).map_err(|e| e.to_string())?;
        let stat_stamp = stamp(&stat);
        let is_symlink = fs::symlink_metadata(&file)
            .map_err(|e| e.to_string())?
            .file_type()
            .is_symlink();
        let bytes = fs::read(&file).map_err(|e| e.to_string())?;
        let again = fs::metadata(&file).map_err(|e| e.to_string())?;
        if !stat.is_file() || is_symlink || sha(&bytes) != hash || stamp(&again) != stat_stamp {
            return Err("PINNED_CODE_INITIAL".to_string());
        }
        records.push(Record {
            file,
            hash: hash.to_string(),
            stamp: stat_stamp,
        });
    }
    Ok(PinnedCodeGuardV1 { records })
}
