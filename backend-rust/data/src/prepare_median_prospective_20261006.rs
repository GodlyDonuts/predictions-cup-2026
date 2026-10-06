// data/prepare-median-prospective-20261006.mjs — pin the native median dip
// replay binary + sources and freeze the prospective paper bank.
use crate::publication_pack::{now_ms, read_json, runtime_dir, sha};
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::digest;
use serde_json::{json, Value as V};
use std::path::Path;

fn pin(file: &Path, bytes: &[u8]) -> Result<V, String> {
    if let Some(parent) = file.parent() { std::fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(file)
        .map_err(|e| e.to_string())?;
    f.write_all(bytes).map_err(|e| e.to_string())?;
    Ok(json!({"path": file.to_string_lossy(), "sha256": sha(bytes)}))
}
fn anchor(file: &Path) -> Result<V, String> {
    let b = std::fs::read(file).map_err(|e| e.to_string())?;
    let end = b.iter().rposition(|&c| c == b'\n').map(|i| i + 1).unwrap_or(0);
    if end == 0 { return Err("Missing complete capture".into()); }
    let prefix = &b[..end];
    let last_start = prefix[..end - 1].iter().rposition(|&c| c == b'\n').map(|i| i + 1).unwrap_or(0);
    let last: V = serde_json::from_slice(&prefix[last_start..end]).map_err(|e| e.to_string())?;
    Ok(json!({
        "bytes": end, "sha256": sha(prefix), "sequence": last["sequence"],
        "hash": last["hash"], "lastAt": last["at"],
    }))
}
pub fn run() -> Result<V, String> {
    let rt = runtime_dir();
    let release = read_json(Path::new("data/median-prospective-build-20261006.json"))?;
    let cohort = "baal-median-prospective-20261006-v1";
    let dir = format!("{rt}/median-paper-cohorts/{cohort}");
    if Path::new(&dir).exists() { return Err("Refuse to replace a frozen cohort".into()); }
    let binary = std::fs::read(release["binaryPath"].as_str().unwrap_or("")).map_err(|e| e.to_string())?;
    if sha(&binary) != release["binaryHash"].as_str().unwrap_or("")
        || !String::from_utf8_lossy(&binary).contains(release["engineHash"].as_str().unwrap_or(""))
    {
        return Err("Compiled identity mismatch".into());
    }
    let native_path = format!("{dir}/bin/baal-median-dip-replay.exe");
    std::fs::create_dir_all(Path::new(&native_path).parent().unwrap()).map_err(|e| e.to_string())?;
    let mut pins = vec![pin(Path::new(&native_path), &binary)?];
    let mut source_map = serde_json::Map::new();
    for p in release["sourcePins"].as_array().map(|a| a.iter()).unwrap_or_default() {
        let source = std::fs::canonicalize(p["relativePath"].as_str().unwrap_or("")).map_err(|e| e.to_string())?;
        let bytes = std::fs::read(&source).map_err(|e| e.to_string())?;
        if sha(&bytes) != p["sha256"].as_str().unwrap_or("") { return Err("Changed checked source".into()); }
        let target = format!("{dir}/sources/{}", p["relativePath"].as_str().unwrap_or(""));
        pins.push(pin(Path::new(&target), &bytes)?);
        source_map.insert(p["relativePath"].as_str().unwrap_or("").to_string(), p["sha256"].clone());
    }
    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let dll = format!("{local}/SigTradingCup/rust-toolchain/rustup/toolchains/stable-x86_64-pc-windows-gnullvm/bin/libunwind.dll");
    let dll_bytes = std::fs::read(&dll).map_err(|e| e.to_string())?;
    let dll_target = format!("{dir}/bin/libunwind.dll");
    pins.push(pin(Path::new(&dll_target), &dll_bytes)?);
    let public_source_path = format!("{rt}/public-books.jsonl");
    let account_source_path = format!("{rt}/account-eligibility.jsonl");
    let price_source_path = format!("{rt}/market-prices.jsonl");
    let mut public_prefix = anchor(Path::new(&public_source_path))?;
    let mut account_prefix = anchor(Path::new(&account_source_path))?;
    let mut feature_prefix = anchor(Path::new(&price_source_path))?;
    let cutoff = ms_to_iso(now_ms() as f64);
    let cutoff_ms = now_ms() as f64;
    for a in [&mut public_prefix, &mut account_prefix, &mut feature_prefix] {
        if baal_service_runtime::contract::parse_iso_ms(a["lastAt"].as_str().unwrap_or("")).unwrap_or(0.0) > cutoff_ms {
            return Err("Future capture anchor".into());
        }
        a.as_object_mut().unwrap().remove("lastAt");
    }
    let old = read_json(Path::new(&format!("{rt}/settlement-portfolio-v3-cohorts/kansas-receipt-baseline-20261006-v3/bank.json")))?;
    let bank = json!({
        "schemaVersion": 1, "mode": "fixed_rolling_median_prospective_paper_v1",
        "name": cohort, "cutoff": cutoff, "universe": old["universe"],
        "publicPrefix": public_prefix, "accountPrefix": account_prefix,
        "featurePrefix": feature_prefix, "featurePolicy": release["featurePolicy"],
        "thresholds": [1, 2, 4], "policy": release["fixedPolicy"],
        "codeIdentity": {
            "engineHash": release["engineHash"], "binaryHash": release["binaryHash"],
            "sources": V::Object(source_map),
        },
    });
    let bank_path = format!("{dir}/bank.json");
    let bank_bytes = format!("{}\n", serde_json::to_string_pretty(&bank).map_err(|e| e.to_string())?);
    {
        use std::io::Write;
        std::fs::OpenOptions::new().write(true).create_new(true).open(&bank_path)
            .map_err(|e| e.to_string())?.write_all(bank_bytes.as_bytes()).map_err(|e| e.to_string())?;
    }
    let bank_hash = sha(serde_json::to_string(&bank).map_err(|e| e.to_string())?.as_bytes());
    let config = json!({
        "cohortDir": dir, "bankPath": bank_path, "bankFileHash": sha(bank_bytes.as_bytes()),
        "bankHash": bank_hash, "publicSourcePath": public_source_path,
        "accountSourcePath": account_source_path, "priceSourcePath": price_source_path,
        "stopPaths": [format!("{rt}/STOP"), format!("{rt}/COLLECTOR_STOP")],
        "codePins": pins, "entryEnabled": true, "entryDrainPath": format!("{dir}/ENTRY_DRAIN"),
    });
    let config_path = format!("{dir}/observer-config.json");
    {
        use std::io::Write;
        std::fs::OpenOptions::new().write(true).create_new(true).open(&config_path)
            .map_err(|e| e.to_string())?
            .write_all((serde_json::to_string_pretty(&config).map_err(|e| e.to_string())? + "\n").as_bytes())
            .map_err(|e| e.to_string())?;
    }
    let evidence = json!({
        "schemaVersion": 1, "at": cutoff, "cohort": cohort, "bankHash": bank_hash,
        "engineHash": release["engineHash"], "binaryHash": release["binaryHash"],
        "publicPrefix": public_prefix, "accountPrefix": account_prefix,
        "featurePrefix": feature_prefix, "thresholds": [1, 2, 4],
        "configRelative": config_path.trim_start_matches(&format!("{rt}/")),
        "binaryRelative": native_path.trim_start_matches(&format!("{rt}/")),
        "pins": pins.len(), "fees": 0, "primedHistoryOnly": true,
        "oldFillsImported": false, "sessionLaunched": false, "competitionOrders": 0,
        "networkRequests": 0, "profitabilityEstablished": false,
    });
    std::fs::write("data/median-prospective-prepared-20261006.json",
        serde_json::to_string_pretty(&evidence).map_err(|e| e.to_string())? + "\n")
        .map_err(|e| e.to_string())?;
    Ok(evidence)
}
