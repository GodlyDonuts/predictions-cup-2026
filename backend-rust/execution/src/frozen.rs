use baal_service_runtime::json_text::sha256;
use serde_json::{json, Map, Value as V};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// live-paper/frozen-paper-graph.mjs: walks the relative .mjs import graph of a
// file set and hashes every reachable source — the code identity bound into
// strategy banks and activation receipts.
fn normalize_entry(entry: &str) -> Result<String, String> {
    if entry.is_empty()
        || entry.starts_with('/')
        || entry.contains('\\')
        || !entry.ends_with(".mjs")
    {
        return Err("INVALID_FROZEN_ENTRY".into());
    }
    let mut parts: Vec<&str> = Vec::new();
    for seg in entry.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err("INVALID_FROZEN_ENTRY".into());
                }
            }
            s => parts.push(s),
        }
    }
    if parts.is_empty() {
        return Err("INVALID_FROZEN_ENTRY".into());
    }
    Ok(parts.join("/"))
}
fn relative_imports(source: &str) -> Vec<String> {
    // (?:\bfrom\s*|\bimport\s*(?:\(\s*)?)['"](\.[^'"]+\.mjs)['"]
    let mut out = Vec::new();
    let bytes = source.as_bytes();
    let mut i = 0;
    while i + 4 < bytes.len() {
        let rest = &source[i..];
        let mut spec_start = None;
        if let Some(r) = rest.strip_prefix("from") {
            let r = r.trim_start();
            if r.starts_with('\'') || r.starts_with('"') {
                spec_start = Some(r);
            }
        } else if rest.starts_with("import") {
            let r = rest[6..].trim_start_matches('(').trim_start();
            if r.starts_with('\'') || r.starts_with('"') {
                spec_start = Some(r);
            }
        }
        if let Some(r) = spec_start {
            let q = r.chars().next().unwrap();
            if let Some(end) = r[1..].find(q) {
                let spec = &r[1..1 + end];
                if spec.starts_with('.') && spec.ends_with(".mjs") {
                    out.push(spec.to_string());
                }
                i += rest.len() - r.len() + end + 2;
                continue;
            }
            i += 1;
            continue;
        }
        i += 1;
    }
    out
}
fn join_relative(from: &str, spec: &str) -> Result<String, String> {
    let mut parts: Vec<String> = from
        .rsplit_once('/')
        .map(|(dir, _)| dir.to_string())
        .unwrap_or_default()
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();
    for seg in spec.split('/') {
        match seg {
            "." | "" => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err("INVALID_FROZEN_IMPORT".into());
                }
            }
            s => parts.push(s.to_string()),
        }
    }
    normalize_entry(&parts.join("/"))
}
fn collect_file(dir: &Path, entry: &str) -> Option<(String, Vec<String>)> {
    let path: PathBuf = dir.join(entry);
    let source = std::fs::read_to_string(&path).ok()?;
    let imports = relative_imports(&source)
        .iter()
        .map(|s| join_relative(entry, s))
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_default();
    Some((source, imports))
}
pub fn collect_frozen_paper_files(dir: &Path, entries: &[&str]) -> Result<V, String> {
    let mut normalized: Vec<String> = Vec::new();
    for e in entries {
        normalized.push(normalize_entry(e)?);
    }
    normalized.sort();
    normalized.dedup();
    let mut pending = normalized;
    let mut seen: Vec<String> = Vec::new();
    let mut files: Map<String, V> = Map::new();
    let mut hash_input: BTreeMap<String, String> = BTreeMap::new();
    while let Some(entry) = pending.pop() {
        if seen.contains(&entry) {
            continue;
        }
        seen.push(entry.clone());
        let (source, imports) = collect_file(dir, &entry)
            .ok_or_else(|| "MISSING_FROZEN_FILE".to_string())?;
        let hash = sha256(source.as_bytes());
        hash_input.insert(entry.clone(), hash.clone());
        files.insert(
            entry.clone(),
            json!({"sha256": hash, "bytes": source.len() as u64}),
        );
        pending.extend(imports);
    }
    let digested = serde_json::to_string(&hash_input).unwrap();
    let graph_hash = sha256(digested.as_bytes());
    let mut files_sorted = Map::new();
    for (k, v) in files {
        files_sorted.insert(k, v);
    }
    Ok(json!({
        "files": files_sorted,
        "hash": graph_hash,
    }))
}
