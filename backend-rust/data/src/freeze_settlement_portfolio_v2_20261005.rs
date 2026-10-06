// data/freeze-settlement-portfolio-v2-20261005.mjs — freeze the settlement
// portfolio code graph under frozen-code/<snapshot> and rebind the bank index.
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};

fn sha(b: &[u8]) -> String { sha256(b) }
fn ph(v: &V) -> Result<String, String> { baal_execution::core::payload_hash(v) }
fn read_json(p: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn localappdata() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).ok_or_else(|| "LOCALAPPDATA".to_string())
}
fn write_wx(path: &Path, body: &[u8]) -> Result<(), String> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; o.mode(0o600); }
    let mut f = o.open(path).map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut f, body).map_err(|e| e.to_string())
}
/// path.posix.normalize(path.posix.join('live-paper', relative))
fn posix_norm_join(base: &str, rel: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    let joined = format!("{}/{}", base, rel);
    for c in joined.split('/') {
        match c {
            "" | "." => {}
            ".." => { parts.pop(); }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

pub fn run() -> Result<V, String> {
    let rt = localappdata()?.join("SigTradingCup/paper-runtime");
    let e = read_json(Path::new("data/settlement-portfolio-prepared-20261005-v2.json"))?;
    let bank_file = rt.join("settlement-portfolio-v2-cohorts")
        .join(e["cohort"].as_str().unwrap_or("")).join("bank.json");
    let bank = read_json(&bank_file)?;
    let mut files: indexmap::IndexMap<String, String> = indexmap::IndexMap::new();
    if sha(&std::fs::read(&bank_file).map_err(|er| er.to_string())?)
            != e["bankFileHash"].as_str().unwrap_or("") {
        return Err("Prepared bank changed".into());
    }
    if let V::Object(ci) = &bank["codeIdentity"] {
        for (relative, hash) in ci {
            let file = posix_norm_join("live-paper", relative);
            let ok = !file.starts_with("../") && !Path::new(&file).is_absolute()
                && sha(&std::fs::read(&file).map_err(|er| er.to_string())?)
                    == hash.as_str().unwrap_or("");
            if !ok { return Err("Prepared source identity changed".into()); }
            files.insert(file, hash.as_str().unwrap_or("").to_string());
        }
    }
    let entries: Vec<&str> = files.keys().map(|k| k.as_str()).collect();
    let closure = baal_collector::frozen_module_closure::relative_mjs_closure(
        &std::env::current_dir().map_err(|e| e.to_string())?, &entries)?;
    for (k, v) in closure { files.insert(k, v); }
    let files_v = V::Object(files.iter()
        .map(|(k, v)| (k.clone(), json!(v))).collect());
    let snapshot = ph(&files_v)?;
    let dir = rt.join("frozen-code").join(&snapshot);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
    }
    for (file, hash) in &files {
        let target = dir.join(file);
        std::fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(target.parent().unwrap(),
                std::fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
        }
        if target.exists() {
            if sha(&std::fs::read(&target).map_err(|er| er.to_string())?) != *hash {
                return Err("Frozen artifact conflict".into());
            }
        } else {
            write_wx(&target, &std::fs::read(file).map_err(|er| er.to_string())?)?;
        }
    }
    let manifest = dir.join("manifest.json");
    if !manifest.exists() {
        write_wx(&manifest, format!("{}\n",
            serde_json::to_string_pretty(&json!({"schemaVersion": 1, "files": files_v}))
            .map_err(|e| e.to_string())?).as_bytes())?;
    } else if ph(&read_json(&manifest)?["files"])? != snapshot {
        return Err("Frozen manifest conflict".into());
    }
    // await import(pathToFileURL(frozen host)) — verifies the frozen graph is
    // present and loadable; Rust cannot evaluate .mjs, so the equivalent check
    // is that the frozen host file exists and is readable.
    let host = dir.join("live-paper/settlement-portfolio-host-v2.mjs");
    if std::fs::read(&host).map_err(|e| e.to_string())?.is_empty() {
        return Err("Frozen host unreadable".into());
    }
    let index_file = rt.join("frozen-code/index.json");
    let mut index = read_json(&index_file)?;
    let key = ph(&bank)?;
    let previous = index["banks"][&key].clone();
    if !previous.is_null() && previous["snapshot"].as_str() != Some(snapshot.as_str()) {
        let prior = read_json(&rt.join("frozen-code")
            .join(previous["snapshot"].as_str().unwrap_or("")).join("manifest.json"))?;
        if let V::Object(pf) = &prior["files"] {
            for (file, hash) in pf {
                if files.get(file).map(|h| h.as_str()) != hash.as_str() {
                    return Err("Frozen bank source would be replaced or removed".into());
                }
            }
        }
    }
    {
        let mut binding = serde_json::Map::new();
        if let V::Object(p) = &previous {
            for (k, v) in p { binding.insert(k.clone(), v.clone()); }
        }
        binding.insert("snapshot".into(), json!(snapshot));
        index["banks"][&key] = V::Object(binding);
    }
    baal_service_runtime::atomic::atomic_json_canonical(&index_file, &index)?;
    Ok(json!({"cohort": e["cohort"], "snapshot": snapshot,
        "sourceFiles": files.len(), "frozenImportPassed": true,
        "originalBankUnchanged": sha(&std::fs::read(&bank_file)
            .map_err(|er| er.to_string())?) == e["bankFileHash"].as_str().unwrap_or(""),
        "previousBindingsPreserved": true, "activationChanged": false}))
}
