// data/repair-transport-bindings-20261005.mjs — frozen transport binding repair.
use baal_collector::collector_journal_native::{collector_journal_code_identity, collector_native_journal_config};
use baal_collector::collector_publication_native::{collector_native_publication_config, collector_publication_code_identity};
use baal_collector::frozen_paper_runtime::frozen_paper_module;
use baal_collector::model::digest;
use baal_collector::native_journal_restore::{journal_engine_identity, native_journal_binding};
use baal_collector::native_publication::native_publication_binding;
use baal_service_runtime::atomic::atomic_json_canonical as atomic_json;
use baal_service_runtime::ledger::payload_hash;
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Value as V};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn sha(b: &[u8]) -> String { sha256(b) }
fn read(f: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(f).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn now_iso() -> String {
    baal_service_runtime::contract::ms_to_iso(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0))
}
fn posix_norm(p: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for part in p.split('/') {
        match part { "" | "." => {} ".." => { out.pop(); } x => out.push(x) }
    }
    out.join("/")
}
fn mkdir_700(p: &Path) -> Result<(), String> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(p).map_err(|e| e.to_string())
}
fn write_excl_600(f: &Path, b: &[u8]) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(f)
        .and_then(|mut fd| fd.write_all(b)).map_err(|e| e.to_string())
}
struct Graph { files: BTreeMap<String, String>, bytes: BTreeMap<String, Vec<u8>> }

fn graph(workspace: &Path, entry: &str, original_ledger: &[u8]) -> Result<Graph, String> {
    let mut files: BTreeMap<String, String> = BTreeMap::new();
    let mut bytes: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut pending = vec![entry.to_string()];
    let ws = workspace.canonicalize().map_err(|e| e.to_string())?;
    let re_import = regex::Regex::new(r#"(?:\bfrom\s*|\bimport\s*(?:\(\s*)?)['"](\.[^'"]+\.mjs)['"]"#).unwrap();
    let re_url = regex::Regex::new(r#"\bnew\s+URL\(\s*['"](\.[^'"]+\.mjs)['"]\s*,\s*import\.meta\.url"#).unwrap();
    while let Some(raw) = pending.pop() {
        let key = posix_norm(&raw);
        let file = ws.join(&key);
        let canon = file.canonicalize().unwrap_or(file.clone());
        if !canon.starts_with(&ws) || !key.ends_with(".mjs") {
            return Err("Frozen graph scope".into());
        }
        if files.contains_key(&key) { continue; }
        let b = if key == "execution/ledger.mjs" {
            original_ledger.to_vec()
        } else {
            std::fs::read(&file).map_err(|e| e.to_string())?
        };
        let source = String::from_utf8_lossy(&b).to_string();
        files.insert(key.clone(), sha(&b));
        bytes.insert(key.clone(), b);
        for re in [&re_import, &re_url] {
            for m in re.captures_iter(&source) {
                let dir = posix_norm(&key);
                let parent = dir.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
                pending.push(posix_norm(&format!("{}/{}", parent, &m[1])));
            }
        }
    }
    Ok(Graph { files, bytes })
}

pub fn run(root: &Path) -> Result<V, String> {
    let workspace = std::env::current_dir().map_err(|e| e.to_string())?;
    let runtime_dir = Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default())
        .join("SigTradingCup").join("paper-runtime");
    let marker = read(&runtime_dir.join("COLLECTOR_STOP"))?;
    if marker["source"].as_str() != Some("transport_binding_repair_20261005") {
        return Err("Transport repair provenance missing".into());
    }
    let original_ledger = std::fs::read(root.join("data/frozen-ledger-before-rename-20261005.mjs")).map_err(|e| e.to_string())?;
    if sha(&original_ledger) != "523ee3fd04f995983ee3ffc03b5ee8dbdf629a718ba91c17d08026fe2411b5c4" {
        return Err("Original immutable ledger bytes differ".into());
    }
    let index_file = runtime_dir.join("frozen-code").join("index.json");
    let mut index = read(&index_file)?;
    let mut archives: Vec<V> = Vec::new();
    for (pointer, entry) in [("probability-receipt-active.json", "live-paper/probability-receipt-host-v3.mjs"),
        ("calibrated-poll-paper-active.json", "live-paper/probability-receipt-host.mjs")] {
        let active = read(&runtime_dir.join(pointer))?;
        let dir = runtime_dir.join("probability-receipt-cohorts").join(active["cohort"].as_str().unwrap_or(""));
        let bank = read(&dir.join("bank.json"))?;
        let bank_hash = payload_hash(&bank).map_err(|e| e.to_string())?;
        let g = graph(&workspace, entry, &original_ledger)?;
        if digest(&bank) != active["bankHash"].as_str().unwrap_or("") {
            return Err("Active model identity differs".into());
        }
        if !index["banks"][&bank_hash].is_null() {
            frozen_paper_module(&runtime_dir, &bank, entry)?;
            archives.push(json!({"cohort": active["cohort"], "bankHash": active["bankHash"],
                "canonicalIndexKey": bank_hash, "snapshot": index["banks"][&bank_hash]["snapshot"],
                "existingArchiveVerified": true, "originalCodePreserved": true, "bankChanged": false,
                "accountingReset": false}));
            continue;
        }
        for (relative, expected) in bank["codeIdentity"].as_object().cloned().unwrap_or_default() {
            let key = posix_norm(&format!("live-paper/{}", relative));
            if g.files.get(&key).map(String::as_str) != expected.as_str() {
                return Err("Original model identity not preserved".into());
            }
        }
        let files_v = V::Object(g.files.iter().map(|(k, v)| (k.clone(), json!(v))).collect());
        let snapshot = payload_hash(&files_v).map_err(|e| e.to_string())?;
        let aroot = runtime_dir.join("frozen-code").join(&snapshot);
        if aroot.exists() { return Err("Unexpected archive exists".into()); }
        mkdir_700(&aroot)?;
        for (name, b) in &g.bytes {
            let target = aroot.join(name);
            if let Some(parent) = target.parent() { mkdir_700(parent)?; }
            write_excl_600(&target, b)?;
        }
        write_excl_600(&aroot.join("manifest.json"),
            format!("{}\n", serde_json::to_string_pretty(&json!({"schemaVersion": 1, "files": files_v})).map_err(|e| e.to_string())?).as_bytes())?;
        index["banks"][bank_hash.clone()] = json!({"snapshot": snapshot});
        archives.push(json!({"cohort": active["cohort"], "bankHash": active["bankHash"],
            "canonicalIndexKey": bank_hash, "snapshot": snapshot, "files": g.files.len() as i64,
            "originalCodePreserved": true, "bankChanged": false, "accountingReset": false}));
    }
    atomic_json(&index_file, &index)?;
    for x in &archives {
        let bank = read(&runtime_dir.join("probability-receipt-cohorts")
            .join(x["cohort"].as_str().unwrap_or("")).join("bank.json"))?;
        frozen_paper_module(&runtime_dir, &bank, if bank["schemaVersion"].as_i64() == Some(4) {
            "live-paper/probability-receipt-host-v3.mjs"
        } else { "live-paper/probability-receipt-host.mjs" })?;
    }
    let universe = read(&root.join("live-paper/price-universe.json"))?;
    let mut transport: Vec<V> = Vec::new();
    for (name, identity) in [("collector-native-journal.json", collector_journal_code_identity()?),
        ("collector-native-publication.json", V::Object(collector_publication_code_identity(&workspace)?
            .iter().map(|(k, v)| (k.clone(), json!(v))).collect()))] {
        let file = runtime_dir.join(name);
        let bytes = std::fs::read(&file).map_err(|e| e.to_string())?;
        let prior: V = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if name.contains("journal") {
            native_journal_binding(&runtime_dir, &prior["nativeBackend"])?;
            if journal_engine_identity(&runtime_dir.join("journal-native-sources")
                    .join(prior["nativeBackend"]["engineHash"].as_str().unwrap_or("")))?
                != prior["nativeBackend"]["engineHash"].as_str().unwrap_or("") {
                return Err("Native source changed".into());
            }
        } else {
            native_publication_binding(&runtime_dir, &prior["nativeBackend"])?;
        }
        write_excl_600(&PathBuf::from(format!("{}.before-rename-20261005", file.to_string_lossy())), &bytes)?;
        let mut next = prior.clone();
        next["codeIdentity"] = identity.clone();
        if name.contains("journal") {
            next["sourceSnapshot"] = json!(payload_hash(&identity).map_err(|e| e.to_string())?);
        }
        atomic_json(&file, &next)?;
        transport.push(json!({"file": name, "previousHash": sha(&bytes),
            "updatedHash": sha(&std::fs::read(&file).map_err(|e| e.to_string())?),
            "compiledBindingUnchanged": payload_hash(&prior["nativeBackend"]).map_err(|e| e.to_string())?
                == payload_hash(&next["nativeBackend"]).map_err(|e| e.to_string())?,
            "identityFiles": identity.as_object().map(|o| o.len() as i64).unwrap_or(0)}));
    }
    collector_native_journal_config(&runtime_dir)?;
    collector_native_publication_config(&runtime_dir, &universe, &workspace)?;
    let report = json!({"at": now_iso(), "archives": archives, "transport": transport,
        "activationChanged": false, "originalJournalRewritten": false, "modelBanksChanged": false,
        "accountingReset": false, "networkRequests": 0, "competitionOrders": 0});
    let out_file = root.join("data/transport-binding-repair-20261005.json");
    let mut fd = std::fs::OpenOptions::new().write(true).create_new(true).open(&out_file).map_err(|e| e.to_string())?;
    use std::io::Write;
    fd.write_all(format!("{}\n", serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?).as_bytes()).map_err(|e| e.to_string())?;
    println!("{}", baal_service_runtime::json_text::js_stringify(&json!({"archives": archives_len(&report),
        "transportBindings": report["transport"].as_array().map(|a| a.len() as i64).unwrap_or(0),
        "modelBanksChanged": false, "accountingReset": false})));
    Ok(report)
}
fn archives_len(r: &V) -> i64 { r["archives"].as_array().map(|a| a.len() as i64).unwrap_or(0) }
