// data/preserve-frozen-paper-code-20261004.mjs — capture the transitive .mjs
// source closure of every existing bank codeIdentity into frozen-code/.
use crate::publication_pack::{now_ms, runtime_dir, sha};
use baal_execution::core::payload_hash;
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::contract::ms_to_iso;
use serde_json::{json, Value as V};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

fn norm(relative: &str) -> String {
    let replaced = relative.replace('\\', "/");
    let mut parts: Vec<&str> = Vec::new();
    for p in replaced.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                if matches!(parts.last(), Some(&s) if s != "..") { parts.pop(); }
                else { parts.push(p); }
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}
fn import_sources(source: &str) -> Vec<String> {
    // /(?:from\s*|import\s*\()\s*['"](\.{1,2}\/[^'"]+\.mjs)['"]/g
    let mut out = Vec::new();
    let bytes = source.as_bytes();
    for i in 0..bytes.len() {
        let rest = &source[i..];
        let after = if let Some(r) = rest.strip_prefix("from") {
            let t = r.trim_start();
            if t.starts_with('\'') || t.starts_with('"') { Some(t) }
            else if t.is_empty() || t.chars().next().is_some_and(|c| c.is_alphabetic()) { None }
            else { Some(t) }
        } else if rest.starts_with("import") && rest[6..].trim_start().starts_with('(') {
            Some(rest[6..].trim_start()[1..].trim_start())
        } else { None };
        if let Some(t) = after {
            let q = t.chars().next();
            if let Some(q) = q.filter(|q| *q == '\'' || *q == '"') {
                let inner = &t[1..];
                if let Some(end) = inner.find(q) {
                    let spec = &inner[..end];
                    if spec.starts_with("./") || spec.starts_with("../") {
                        if spec.ends_with(".mjs") { out.push(spec.to_string()); }
                    }
                }
            }
        }
    }
    out
}
pub fn run() -> Result<V, String> {
    let rt = runtime_dir();
    let workspace = Path::new(".").canonicalize().map_err(|e| e.to_string())?;
    let mut banks: Vec<V> = Vec::new();
    for folder in ["no-maker-cohorts", "baal-pair-cohorts", "probability-receipt-cohorts"] {
        let dir = format!("{rt}/{folder}");
        if !Path::new(&dir).exists() { continue; }
        for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
            let name = entry.map_err(|e| e.to_string())?.file_name().to_string_lossy().to_string();
            let file = format!("{dir}/{name}/bank.json");
            if Path::new(&file).exists() {
                banks.push(serde_json::from_slice(&std::fs::read(&file).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?);
            }
        }
    }
    let mut files: BTreeMap<String, String> = BTreeMap::new();
    let mut bytes: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut stack: Vec<String> = Vec::new();
    for bank in &banks {
        for (relative, hash) in bank["codeIdentity"].as_object().cloned().unwrap_or_default() {
            let file = norm(&format!("live-paper/{relative}"));
            stack.push(file.clone());
            while let Some(rel) = stack.pop() {
                if files.contains_key(&rel) { continue; }
                if !rel.ends_with(".mjs") || rel.starts_with("../") || rel.starts_with('/') {
                    return Err("Source scope".into());
                }
                let data = std::fs::read(workspace.join(&rel)).map_err(|e| e.to_string())?;
                files.insert(rel.clone(), sha(&data));
                bytes.insert(rel.clone(), data.clone());
                let text = String::from_utf8_lossy(&data).to_string();
                for m in import_sources(&text) {
                    let dir = rel.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
                    stack.push(norm(&format!("{dir}/{m}")));
                }
            }
            if files[&file] != hash.as_str().unwrap_or("") {
                return Err(format!("Existing frozen bank source mismatch: {}", bank["name"].as_str().unwrap_or("")));
            }
        }
    }
    let files_v = json!(files.iter().map(|(k, v)| (k.clone(), V::String(v.clone()))).collect::<serde_json::Map<String, V>>());
    let snapshot = payload_hash(&files_v)?;
    let root = format!("{rt}/frozen-code/{snapshot}");
    if !Path::new(&root).exists() {
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        for (file, data) in &bytes {
            let target = PathBuf::from(format!("{root}/{file}"));
            std::fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
            std::fs::OpenOptions::new().write(true).create_new(true).open(&target)
                .and_then(|mut f| f.write_all(data)).map_err(|e| e.to_string())?;
        }
        atomic_json(Path::new(&format!("{root}/manifest.json")), &json!({
            "schemaVersion": 1, "files": files_v,
        }))?;
    }
    for (file, hash) in &files {
        if sha(&std::fs::read(format!("{root}/{file}")).map_err(|e| e.to_string())?) != *hash {
            return Err("Snapshot verification".into());
        }
    }
    let index_file = format!("{rt}/frozen-code/index.json");
    let mut index: V = if Path::new(&index_file).exists() {
        serde_json::from_slice(&std::fs::read(&index_file).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?
    } else {
        json!({"schemaVersion": 1, "banks": {}})
    };
    for bank in &banks {
        let hash = payload_hash(bank)?;
        if index["banks"][&hash].is_object()
            && index["banks"][&hash]["snapshot"] != json!(snapshot)
        { return Err("Existing binding differs".into()); }
        index["banks"][hash] = json!({"snapshot": snapshot});
    }
    atomic_json(Path::new(&index_file), &index)?;
    let receipt = json!({
        "at": ms_to_iso(now_ms() as f64), "snapshot": snapshot, "files": files_v,
        "banks": banks.iter().map(|b| json!({
            "cohort": b["name"], "bankHash": payload_hash(b).unwrap_or_default(),
        })).collect::<Vec<V>>(),
        "accountingReset": false, "competitionOrders": 0,
    });
    std::fs::OpenOptions::new().write(true).create(true).open("data/baal-frozen-code-preservation-20261004.json")
        .and_then(|mut f| f.write_all(format!("{}\n", serde_json::to_string_pretty(&receipt).unwrap()).as_bytes()))
        .map_err(|e| e.to_string())?;
    Ok(json!({
        "snapshot": snapshot, "files": files.len(), "banks": banks.len(), "verified": true,
    }))
}
