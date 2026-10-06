// data/prepare-native-receipt-observer-20261006.mjs — pin the settlement
// observer service binary and sources, then write the observer config.
use crate::publication_pack::{now_ms, read_json, runtime_dir, sha};
use baal_service_runtime::contract::ms_to_iso;
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};

fn pin(file: &Path, bytes: &[u8]) -> Result<V, String> {
    std::fs::create_dir_all(file.parent().unwrap()).map_err(|e| e.to_string())?;
    if file.exists() {
        if sha(&std::fs::read(file).map_err(|e| e.to_string())?) != sha(bytes) {
            return Err("Pinned artifact conflict".into());
        }
    } else {
        std::fs::OpenOptions::new().write(true).create_new(true).open(file)
            .and_then(|mut f| f.write_all(bytes)).map_err(|e| e.to_string())?;
    }
    Ok(json!({"path": file.to_string_lossy(), "sha256": sha(bytes)}))
}
pub fn run() -> Result<V, String> {
    let rt = runtime_dir();
    let name = "kansas-receipt-baseline-20261006-v3";
    let dir = format!("{rt}/settlement-portfolio-v3-cohorts/{name}");
    let prepared = read_json(Path::new("data/receipt-baseline-prepared-20261006-v3.json"))?;
    let krate = Path::new("backend-rust/baal-service").canonicalize().map_err(|e| e.to_string())?;
    let binary = krate.join("target/release/baal-service.exe");
    let bank_file = format!("{dir}/bank.json");
    let bank = read_json(Path::new(&bank_file))?;
    if sha(&std::fs::read(&bank_file).map_err(|e| e.to_string())?) != prepared["bankFileHash"].as_str().unwrap_or("") {
        return Err("Prospective bank changed".into());
    }
    for n in ["STOP", "COLLECTOR_STOP", "EXECUTION_STOP"] {
        if Path::new(&format!("{rt}/{n}")).exists() {
            return Err("STOP provenance review required".into());
        }
    }
    let build = std::fs::read_to_string(krate.join("build.rs")).map_err(|e| e.to_string())?;
    let start = build.find("for p in [").ok_or("Service source manifest unavailable")?;
    let rest = &build[start..];
    let end = rest.find(']').ok_or("Service source manifest unavailable")?;
    let list = &rest[..end];
    let files: Vec<String> = {
        let mut out = Vec::new();
        let mut chars = list.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '"' {
                let mut s = String::new();
                while let Some(&n) = chars.peek() {
                    chars.next();
                    if n == '"' { break; }
                    s.push(n);
                }
                out.push(s);
            }
        }
        out
    };
    let mut service_hash = Sha256::new();
    let mut sources: Vec<(String, Vec<u8>, String)> = Vec::new();
    let backend_root = Path::new("backend-rust").canonicalize().map_err(|e| e.to_string())?;
    for f in &files {
        let from = krate.join(f).canonicalize().map_err(|e| e.to_string())?;
        let b = std::fs::read(&from).map_err(|e| e.to_string())?;
        service_hash.update(f.as_bytes());
        service_hash.update((b.len() as u64).to_le_bytes());
        service_hash.update(&b);
        let rel = from.strip_prefix(&backend_root).map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| from.to_string_lossy().to_string());
        sources.push((rel, b.clone(), sha(&b)));
    }
    let engine_hash = format!("{:x}", service_hash.finalize());
    let binary_bytes = std::fs::read(&binary).map_err(|e| e.to_string())?;
    let binary_hash = sha(&binary_bytes);
    if !contains(&binary_bytes, engine_hash.as_bytes())
        || !contains(&binary_bytes, bank["codeIdentity"]["engineHash"].as_str().unwrap_or("").as_bytes())
    {
        return Err("Service binary does not match current compiled source/portfolio engine".into());
    }
    for (f, h) in bank["codeIdentity"]["sources"].as_object().unwrap() {
        let from = Path::new("backend-rust/portfolio-engine").join(f).canonicalize().map_err(|e| e.to_string())?;
        let b = std::fs::read(&from).map_err(|e| e.to_string())?;
        if sha(&b) != h.as_str().unwrap_or("") {
            return Err("Frozen portfolio source changed".into());
        }
        let rel = from.strip_prefix(&backend_root).map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| from.to_string_lossy().to_string());
        sources.push((rel, b, h.as_str().unwrap().to_string()));
    }
    let artifact = format!("{rt}/native-observer-builds/{binary_hash}");
    let archive = format!("{rt}/native-observer-sources/{engine_hash}");
    std::fs::create_dir_all(&artifact).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&archive).map_err(|e| e.to_string())?;
    let installed = format!("{artifact}/baal-service.exe");
    let portfolio_binary = format!("{rt}/{}", prepared["binaryRelative"].as_str().unwrap_or(""));
    if sha(&std::fs::read(&portfolio_binary).map_err(|e| e.to_string())?) != bank["codeIdentity"]["binaryHash"].as_str().unwrap_or("") {
        return Err("Frozen portfolio binary changed".into());
    }
    let libunwind = format!(
        "{}/SigTradingCup/rust-toolchain/rustup/toolchains/stable-x86_64-pc-windows-gnullvm/bin/libunwind.dll",
        std::env::var("LOCALAPPDATA").unwrap_or_default());
    let mut code_pins = vec![
        pin(Path::new(&installed), &binary_bytes)?,
        json!({"path": portfolio_binary, "sha256": bank["codeIdentity"]["binaryHash"]}),
        pin(Path::new(&libunwind), &std::fs::read(&libunwind).map_err(|e| e.to_string())?)?,
    ];
    let archive_abs = Path::new(&archive).canonicalize().map_err(|e| e.to_string())?;
    for (rel, bytes, _h) in &sources {
        let to = archive_abs.join(rel);
        if !to.starts_with(&archive_abs) {
            return Err("Archive source scope".into());
        }
        let p = pin(&to, bytes)?;
        if !code_pins.iter().any(|x| x["path"] == p["path"]) {
            code_pins.push(p);
        }
    }
    let config = json!({
        "cohortDir": dir, "bankPath": bank_file, "bankFileHash": prepared["bankFileHash"],
        "bankHash": prepared["bankHash"],
        "publicSourcePath": format!("{rt}/public-books.jsonl"),
        "accountSourcePath": format!("{rt}/account-eligibility.jsonl"),
        "accountUniverseHash": bank["universe"]["hash"],
        "stopPaths": (["STOP", "COLLECTOR_STOP", "EXECUTION_STOP"].iter()
            .map(|n| json!(format!("{rt}/{n}"))).collect::<Vec<V>>()),
        "codePins": code_pins, "entryEnabled": true,
        "entryDrainPath": format!("{dir}/ENTRY_DRAIN"),
    });
    let config_file = format!("{dir}/observer-config.json");
    if Path::new(&config_file).exists() {
        return Err("Never silently rebind an existing observer".into());
    }
    std::fs::OpenOptions::new().write(true).create_new(true).open(&config_file)
        .and_then(|mut f| f.write_all(format!("{}\n", serde_json::to_string_pretty(&config).unwrap()).as_bytes()))
        .map_err(|e| e.to_string())?;
    let out = json!({
        "schemaVersion": 1, "at": ms_to_iso(now_ms() as f64), "cohort": name,
        "bankHash": prepared["bankHash"], "serviceBinaryHash": binary_hash,
        "serviceEngineHash": engine_hash,
        "portfolioEngineHash": bank["codeIdentity"]["engineHash"],
        "sourcePins": config["codePins"].as_array().map(|a| a.len()).unwrap_or(0),
        "configFileRelative": PathBuf::from(&config_file).strip_prefix(&rt).unwrap()
            .to_string_lossy().replace('\\', "/"),
        "binaryRelative": PathBuf::from(&installed).strip_prefix(&rt).unwrap()
            .to_string_lossy().replace('\\', "/"),
        "prepared": true, "activated": false, "accountRequests": 0, "competitionOrders": 0,
        "persistentServicesAdded": 0,
    });
    std::fs::OpenOptions::new().write(true).create_new(true).open("data/native-receipt-observer-prepared-20261006.json")
        .and_then(|mut f| f.write_all(format!("{}\n", serde_json::to_string_pretty(&out).unwrap()).as_bytes()))
        .map_err(|e| e.to_string())?;
    Ok(out)
}
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    needle.is_empty()
        || haystack.windows(needle.len()).any(|w| w == needle)
}
