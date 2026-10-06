// data/prepare-receipt-baseline-20261006-v3.mjs — declare the funded receipt
// baseline v3 cohort after a native genesis probe.
use crate::publication_pack::{now_ms, read_json, runtime_dir, sha};
use baal_collector::model::digest;
use baal_collector::settlement_portfolio_host_v2::verify_settlement_portfolio_source_prefix;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub fn run() -> Result<V, String> {
    let rt = runtime_dir();
    let name = "kansas-receipt-baseline-20261006-v3";
    let dir = format!("{rt}/settlement-portfolio-v3-cohorts/{name}");
    let krate = Path::new("backend-rust/portfolio-engine").canonicalize().map_err(|e| e.to_string())?;
    let release = read_json(&krate.join("release-binding.json"))?;
    let binary = krate.join("target/release/baal-portfolio-engine.exe");
    for n in ["STOP", "COLLECTOR_STOP", "EXECUTION_STOP"] {
        if Path::new(&format!("{rt}/{n}")).exists() {
            return Err("STOP provenance review required".into());
        }
    }
    if Path::new(&dir).exists() { return Err("Never overwrite a prospective bank".into()); }
    if sha(&std::fs::read(&binary).map_err(|e| e.to_string())?) != release["binaryHash"].as_str().unwrap_or("") {
        return Err("Native binary identity changed".into());
    }
    let mut engine = Sha256::new();
    for (f, h) in release["sourceFiles"].as_object().unwrap() {
        let b = std::fs::read(krate.join(f)).map_err(|e| e.to_string())?;
        if sha(&b) != h.as_str().unwrap_or("") {
            return Err(format!("Native source changed: {f}"));
        }
        engine.update(f.as_bytes());
        engine.update([0u8]);
        engine.update(&b);
        engine.update([0u8]);
    }
    let binary_bytes = std::fs::read(&binary).map_err(|e| e.to_string())?;
    if format!("{:x}", engine.finalize()) != release["engineHash"].as_str().unwrap_or("")
        || !binary_bytes.windows(64).any(|w| w == release["engineHash"].as_str().unwrap_or("").as_bytes())
    { return Err("Compiled source binding mismatch".into()); }
    let artifact_dir = format!("{rt}/portfolio-native-builds/{}", release["binaryHash"].as_str().unwrap_or(""));
    std::fs::create_dir_all(&artifact_dir).map_err(|e| e.to_string())?;
    let libunwind = format!(
        "{}/SigTradingCup/rust-toolchain/rustup/toolchains/stable-x86_64-pc-windows-gnullvm/bin/libunwind.dll",
        std::env::var("LOCALAPPDATA").unwrap_or_default());
    for from in [binary.to_string_lossy().to_string(), libunwind] {
        let to = format!("{artifact_dir}/{}", Path::new(&from).file_name().unwrap().to_string_lossy());
        let b = std::fs::read(&from).map_err(|e| e.to_string())?;
        if Path::new(&to).exists() {
            if sha(&std::fs::read(&to).map_err(|e| e.to_string())?) != sha(&b) {
                return Err("Pinned binary conflict".into());
            }
        } else {
            std::fs::OpenOptions::new().write(true).create_new(true).open(&to)
                .and_then(|mut f| f.write_all(&b)).map_err(|e| e.to_string())?;
        }
    }
    let old_pointer = read_json(Path::new(&format!("{rt}/settlement-portfolio-v2-active.json")))?;
    let old = read_json(Path::new(&format!("{rt}/settlement-portfolio-v2-cohorts/{}/bank.json",
        old_pointer["cohort"].as_str().unwrap_or(""))))?;
    if digest(&old) != old_pointer["bankHash"].as_str().unwrap_or("") {
        return Err("Reference identity changed".into());
    }
    let prefix = verify_settlement_portfolio_source_prefix(
        Path::new(&format!("{rt}/public-books.jsonl")), &old["universe"], None, now_ms())?;
    if old["research"].as_array().map(|a| a.iter().any(|r|
        parse_iso_ms(r["expiresAt"].as_str().unwrap_or("")).unwrap_or(0.0)
            <= parse_iso_ms(prefix["cutoff"].as_str().unwrap_or("")).unwrap_or(0.0))).unwrap_or(false)
    { return Err("Research expired; do not renew source clocks".into()); }
    let mut fill_policy = old["fillPolicy"].clone();
    fill_policy.as_object_mut().unwrap().insert("fillAdverseTicks".into(), json!(0));
    fill_policy.as_object_mut().unwrap().insert("liquidationAdverseTicks".into(), json!(0));
    let bank = json!({
        "schemaVersion": 3, "mode": "funded_settlement_portfolio_receipt_baseline_v3",
        "name": name, "cutoff": prefix["cutoff"], "universe": old["universe"],
        "identities": old["identities"], "research": old["research"],
        "researchEvidence": old["researchEvidence"], "riskGroups": old["riskGroups"],
        "valuationDeadlineMs": old["valuationDeadlineMs"], "policy": old["policy"],
        "fillPolicy": fill_policy,
        "sourcePrefixBytes": prefix["sourcePrefixBytes"],
        "sourcePrefixHash": prefix["sourcePrefixHash"],
        "sourcePrefixSequence": prefix["sourcePrefixSequence"],
        "sourcePrefixJournalHash": prefix["sourcePrefixJournalHash"],
        "codeIdentity": {
            "language": "Rust", "engineHash": release["engineHash"],
            "binaryHash": release["binaryHash"], "sources": release["sourceFiles"],
        },
        "experiment": {
            "referenceBankHash": old_pointer["bankHash"], "independentNewCashLedger": true,
            "existingWalletsImported": false, "pointForecastNotConfidenceBound": true,
            "originalResearchClocks": true, "protectiveLimitCushionTicks": 1,
            "fillResidualAdverseTicks": 0,
            "estimatedLiquidationSensitivityTicks": [0, 1, 2],
            "counterfactualNotOrderAcceptance": true,
        },
    });
    let input = format!(
        "{}\n{}\n",
        serde_json::to_string(&json!({"op": "init", "bank": bank})).unwrap(),
        serde_json::to_string(&json!({"op": "report", "at": bank["cutoff"]})).unwrap());
    let exe = format!("{artifact_dir}/baal-portfolio-engine.exe");
    let out = Command::new(&exe).stdin(Stdio::piped())
        .stdout(Stdio::piped()).stderr(Stdio::null()).spawn()
        .and_then(|mut child| {
            child.stdin.take().unwrap().write_all(input.as_bytes())?;
            child.wait_with_output()
        }).map_err(|e| e.to_string())?;
    if !out.status.success() { return Err("Native init failed".into()); }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let rows: Vec<V> = text.split('\n').filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    if rows.len() != 2 || rows.iter().any(|r| r["error"].is_object() || r["error"].is_string())
        || rows[0]["state"]["bankHash"] != json!(digest(&bank))
        || rows[1]["report"]["wallets"].as_object().map(|o| o.values().any(|w|
            w["cash"] != json!("100.000000") || w["realizedPnl"] != json!("0.000000")
            || w["positions"].as_array().map(|a| !a.is_empty()).unwrap_or(false))).unwrap_or(false)
    { return Err("Native genesis failed".into()); }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::OpenOptions::new().write(true).create_new(true).open(format!("{dir}/bank.json"))
        .and_then(|mut f| f.write_all(format!("{}\n", serde_json::to_string_pretty(&bank).unwrap()).as_bytes()))
        .map_err(|e| e.to_string())?;
    let out_doc = json!({
        "schemaVersion": 1, "at": ms_to_iso(now_ms() as f64), "cohort": name,
        "bankHash": digest(&bank),
        "bankFileHash": sha(&std::fs::read(format!("{dir}/bank.json")).map_err(|e| e.to_string())?),
        "bankMode": bank["mode"], "cutoff": bank["cutoff"],
        "sourcePrefixBytes": bank["sourcePrefixBytes"],
        "sourcePrefixHash": bank["sourcePrefixHash"],
        "sourcePrefixSequence": bank["sourcePrefixSequence"],
        "sourcePrefixJournalHash": bank["sourcePrefixJournalHash"],
        "compiledEngineHash": release["engineHash"], "binaryHash": release["binaryHash"],
        "binaryRelative": PathBuf::from(&exe).strip_prefix(&rt).unwrap()
            .to_string_lossy().replace('\\', "/"),
        "researchSourceAt": bank["research"][0]["sourceAt"],
        "researchExpiry": bank["research"][0]["expiresAt"],
        "pointForecast": bank["research"][0]["lower"],
        "pointIsNotConfidenceBound": true, "protectiveLimitCushionTicks": 1,
        "residualFillTicks": 0, "residualLiquidationTicks": 0, "fees": 0,
        "nativeInitialStateHash": digest(&rows[0]["state"]),
        "prepared": true, "activated": false, "accountRequests": 0, "contestOrders": 0,
        "originalBanksChanged": false, "profitabilityEstablished": false,
    });
    std::fs::OpenOptions::new().write(true).create_new(true).open("data/receipt-baseline-prepared-20261006-v3.json")
        .and_then(|mut f| f.write_all(format!("{}\n", serde_json::to_string_pretty(&out_doc).unwrap()).as_bytes()))
        .map_err(|e| e.to_string())?;
    Ok(out_doc)
}
