// live-paper/frozen-paper-graph.mjs — transitive frozen-source manifest hashing.
use baal_service_runtime::json_text::sha256;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn relative(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).map(|r| r.to_string_lossy().replace('\\', "/")).unwrap_or_default()
}
/// collectFrozenPaperFiles(workspaceDir, entries)
pub fn collect_frozen_paper_files(workspace_dir: &Path, entries: &[String]) -> Result<BTreeMap<String, String>, String> {
    let root = workspace_dir.canonicalize().map_err(|e| e.to_string())?;
    let mut files = BTreeMap::new();
    let mut pending: Vec<String> = entries.to_vec();
    // Scanners for `from './x.mjs'`, `import './x.mjs'`, `import('./x.mjs')`,
    // `new URL('./x.mjs', import.meta.url)` — local .mjs specifiers only.
    let scan = |source: &str, dir: &Path, pending: &mut Vec<String>| {
        let bytes = source.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'\'' || bytes[i] == b'"' {
                let q = bytes[i];
                let start = i + 1;
                let mut end = start;
                while end < bytes.len() && bytes[end] != q { end += 1; }
                let spec = &source[start..end];
                if spec.starts_with('.') && spec.ends_with(".mjs") {
                    // Check the specifier is in an import/from/URL context.
                    let before = &source[..i];
                    let tail = before.trim_end();
                    let ok = tail.ends_with("from") || tail.ends_with("import")
                        || tail.ends_with("import(") || tail.ends_with("import (")
                        || tail.ends_with("URL(") || tail.ends_with("URL (");
                    if ok {
                        let dep = dir.join(spec);
                        pending.push(dep.to_string_lossy().replace('\\', "/").replace(&format!("{}/", root.to_string_lossy().replace('\\', "/")), ""));
                    }
                }
                i = end + 1;
            } else { i += 1; }
        }
    };
    while let Some(name) = pending.pop() {
        let file = std::fs::canonicalize(root.join(&name)).map_err(|_| "FROZEN_GRAPH_SCOPE".to_string())?;
        if !file.starts_with(&root) || !file.to_string_lossy().ends_with(".mjs") {
            return Err("FROZEN_GRAPH_SCOPE".into());
        }
        let key = relative(&root, &file);
        if files.contains_key(&key) { continue; }
        let bytes = std::fs::read(&file).map_err(|_| "FROZEN_GRAPH_SCOPE".to_string())?;
        let source = String::from_utf8_lossy(&bytes).to_string();
        files.insert(key.clone(), sha256(&bytes));
        let dir = file.parent().unwrap_or(&root).to_path_buf();
        scan(&source, &dir, &mut pending);
    }
    Ok(files)
}
