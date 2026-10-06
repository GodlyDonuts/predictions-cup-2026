// data/verify-coupled-bulk-live-20261005.mjs — coupled bulk live verification.
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::{js_stringify, sha256};
use baal_service_runtime::ledger::payload_hash;
use serde_json::{json, Value as V};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

fn rt() -> PathBuf {
    Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default()).join("SigTradingCup").join("paper-runtime")
}
fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(format!("BULK_LIVE_VERIFICATION_{}", code)) }
}
fn sha(b: &[u8]) -> String { sha256(b) }
fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
}
fn instant(s: &V) -> f64 { s.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN) }
fn prefix_hash(file: &Path, bytes: u64) -> Result<String, String> {
    let mut fd = std::fs::File::open(file).map_err(|e| e.to_string())?;
    let mut h = sha2::Sha256::new();
    use sha2::Digest;
    let mut buffer = vec![0u8; 1024 * 1024];
    let mut offset = 0u64;
    while offset < bytes {
        fd.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;
        let chunk = std::cmp::min(bytes - offset, buffer.len() as u64) as usize;
        let n = fd.read(&mut buffer[..chunk])
            .map_err(|e| e.to_string())?;
        need(n > 0, "PREFIX_TRUNCATED")?;
        h.update(&buffer[..n]);
        offset += n as u64;
    }
    Ok(h.finalize().iter().map(|b| format!("{:02x}", b)).collect())
}

pub fn run(root: &Path) -> Result<V, String> {
    let runtime_dir = rt();
    let read = |file: &str| -> Result<V, String> {
        serde_json::from_slice(&std::fs::read(runtime_dir.join(file)).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())
    };
    let active = read("coupled-bulk-active.json")?;
    let bank = read(&format!("coupled-bulk-cohorts/{}/bank.json",
        active["cohort"].as_str().unwrap_or("")))?;
    let stats = read("stats.json")?;
    let bulk = stats["runtime"]["coupledBulk"].clone();
    let baseline: V = serde_json::from_slice(
        &std::fs::read(root.join("data/coupled-bulk-deployment-20261005.json")).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    need(active["bankHash"].as_str() == Some(payload_hash(&bank).map_err(|e| e.to_string())?.as_str())
        && active["bankHash"].as_str() == Some("d9e27c27bed40738f7526fdeb256633398dea60eeea42457c5dc40315ca3b2cf"),
        "BANK")?;
    need(stats["runtime"]["status"].as_str() == Some("running")
        && bulk["failed"] == json!(false)
        && bulk["bankHash"] == active["bankHash"]
        && now_ms() - instant(&bulk["lastEventAt"]) < 90000.0, "FRESH_COLLECTOR")?;
    need(bulk["sourceFrontiers"]["tape"]["sequence"].as_i64().unwrap_or(0)
            > bank["tapePrefix"]["sequence"].as_i64().unwrap_or(0)
        && bulk["sourceFrontiers"]["prices"]["sequence"].as_i64().unwrap_or(0)
            > bank["pricePrefix"]["sequence"].as_i64().unwrap_or(0)
        && bulk["counts"]["bulk_feature_observation"].as_i64().unwrap_or(0)
            >= bank["legs"].as_array().map(|a| a.len() as i64).unwrap_or(0),
        "LIVE_BOTH_SOURCES")?;
    let fixed: Vec<V> = baseline["fixedArtifactHashes"].as_object().cloned().unwrap_or_default()
        .iter().map(|(file, expected)| {
            let unchanged = std::fs::read(runtime_dir.join(file))
                .map(|b| sha(&b) == expected.as_str().unwrap_or("").to_lowercase()).unwrap_or(false);
            json!({"file": file, "unchanged": unchanged})
        }).collect();
    need(fixed.iter().all(|x| x["unchanged"] == json!(true)), "FIXED_ARTIFACT")?;
    let prefixes: Vec<V> = baseline["priorWalletPrefixes"].as_array().cloned().unwrap_or_default()
        .iter().map(|p| {
            let mut o = p.as_object().cloned().unwrap_or_default();
            let preserved = prefix_hash(
                &runtime_dir.join(p["relative"].as_str().unwrap_or("")),
                p["bytes"].as_u64().unwrap_or(0))
                .map(|h| h == p["hash"].as_str().unwrap_or("")).unwrap_or(false);
            o.insert("preserved".into(), json!(preserved));
            V::Object(o)
        }).collect();
    need(prefixes.iter().all(|x| x["preserved"] == json!(true)), "WALLET_PREFIX")?;
    let source_prefixes: Vec<V> = [
        ("anonymous-trades.jsonl", bank["tapePrefix"].clone()),
        ("market-prices.jsonl", bank["pricePrefix"].clone())]
        .iter().map(|(file, p)| {
            let preserved = prefix_hash(&runtime_dir.join(file), p["bytes"].as_u64().unwrap_or(0))
                .map(|h| h == p["hash"].as_str().unwrap_or("")).unwrap_or(false);
            json!({"file": file, "bytes": p["bytes"], "hash": p["hash"], "preserved": preserved})
        }).collect();
    need(source_prefixes.iter().all(|x| x["preserved"] == json!(true)), "SOURCE_PREFIX")?;
    let official = read("competition-stats.json")?;
    let activation = read("competition-activation.json")?;
    need(official["ordersSubmitted"].as_i64() == Some(0)
        && instant(&activation["entryExpiresAt"]) < now_ms(), "OFFICIAL_STATE")?;
    let pids: V = serde_json::from_slice(
        &std::fs::read(root.join("data/current-runtime-pids.json")).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let record = json!({"at": ms_to_iso(now_ms()), "pids": pids,
        "collector": {"status": stats["runtime"]["status"], "cycles": stats["runtime"]["cycles"],
            "errors": stats["runtime"]["errors"]},
        "bulk": {"bankHash": bulk["bankHash"], "lastEventAt": bulk["lastEventAt"],
            "failed": bulk["failed"], "featureScope": bulk["featureScope"],
            "candidates": bulk["bulkStableCandidates"], "counts": bulk["counts"],
            "sourceFrontiers": bulk["sourceFrontiers"], "wallet": bulk["wallets"]["COUPLED"]},
        "fixed": fixed, "priorWalletPrefixes": prefixes, "sourcePrefixes": source_prefixes,
        "official": {"status": official["status"], "ordersSubmitted": official["ordersSubmitted"],
            "dispatchAttempts": official["dispatchAttempts"],
            "unconfirmedIntents": official["unconfirmedIntents"],
            "entryExpiresAt": activation["entryExpiresAt"], "entryAuthorityExtended": false},
        "priorFailedBanksPreserved": json!(
            ["baal-coupled-bulk-20261005-v2-live", "baal-coupled-bulk-20261005-v2b-live"].iter()
            .all(|cohort| runtime_dir.join("coupled-bulk-cohorts").join(cohort).join("STOP").exists())),
        "networkRequests": 0, "accountingReset": false, "additionalServices": 0,
        "profitabilityEstablished": false});
    std::fs::write(root.join("data/coupled-bulk-live-verification-20261005.json"),
        format!("{}\n", serde_json::to_string_pretty(&record).map_err(|e| e.to_string())?))
        .map_err(|e| e.to_string())?;
    println!("{}", js_stringify(&json!({"at": record["at"], "bankHash": bulk["bankHash"],
        "scope": bulk["featureScope"], "candidates": bulk["bulkStableCandidates"],
        "sourceFrontiers": bulk["sourceFrontiers"],
        "walletPrefixProofs": record["priorWalletPrefixes"].as_array().map(|a| a.len() as i64),
        "sourcePrefixProofs": record["sourcePrefixes"].as_array().map(|a| a.len() as i64),
        "official": record["official"],
        "paperFills": bulk["wallets"]["COUPLED"]["fills"],
        "paperRealized": bulk["wallets"]["COUPLED"]["realizedPnl"]})));
    Ok(record)
}
