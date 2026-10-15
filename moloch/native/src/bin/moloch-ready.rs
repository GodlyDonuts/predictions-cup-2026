//! Offline deployment audit. No network client or credentials are opened.
#[path = "../capture_scope.rs"]
mod capture_scope;
use serde_json::{Value, json};
use std::{env, fs, path::PathBuf};
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.len() == 3 && args[0] == "scope-check" {
        let pairs: Value = serde_json::from_slice(&fs::read(&args[1])?)?;
        let policy: Value = serde_json::from_slice(&fs::read(&args[2])?)?;
        let engine = moloch_replay::engine::Engine::new(&policy)?;
        capture_scope::validate(&pairs, engine.policy(), &engine.required_exchanges())?;
        println!(
            "{}",
            json!({"runtime":"rust","captureScopesVerified":true,"pairs":pairs,"networkRequests":0,"contestOrders":0})
        );
        return Ok(());
    }
    if !args.is_empty() {
        return Err("USAGE: moloch-ready [scope-check PAIRS POLICY]".into());
    }
    let root = PathBuf::from(env::var("MOLOCH_STATE_DIR")?);
    let c: Value = serde_json::from_slice(&fs::read(root.join("deployment.json"))?)?;
    let bin = env::current_exe()?
        .parent()
        .ok_or("BIN_DIRECTORY")?
        .to_owned();
    let bins = [
        "moloch-collector",
        "moloch-paper",
        "moloch-replay",
        "moloch-seed",
        "moloch-session",
        "moloch-source",
    ];
    let binaries = bins.iter().all(|name| bin.join(name).is_file());
    let config = c.get("paperSession").ok_or("PAPER_SESSION_CONFIG")?;
    let policy = PathBuf::from(config["policyFile"].as_str().ok_or("POLICY_PATH")?);
    let seed = PathBuf::from(config["seedSnapshotFile"].as_str().ok_or("SEED_PATH")?);
    let bank = PathBuf::from(config["directory"].as_str().ok_or("BANK_PATH")?);
    if !bank.join("head.json").is_file() || !bank.join("manifest.json").is_file() {
        return Err("PAPER_SESSION_MISSING".into());
    }
    let policy_value: Value = serde_json::from_slice(&fs::read(&policy)?)?;
    let session = moloch_replay::session::Session::open(&bank, Some(&seed), &policy_value)?;
    let pairs: Value =
        serde_json::from_slice(&fs::read(c["pairsFile"].as_str().ok_or("PAIRS_PATH")?)?)?;
    capture_scope::validate(
        &pairs,
        session.engine().policy(),
        &session.engine().required_exchanges(),
    )?;
    let financial_report = session.engine().report()?;
    let network_stop = root.join("NETWORK_DISABLED").is_file();
    let execution_stop = root.join("EXECUTION_STOP").is_file();
    let credential = root.join("credentials/sig-api.key").exists();
    let valid = c["networkEnabled"] == false
        && c["tradingEnabled"] == false
        && c["readsPerMinute"] == 100
        && network_stop
        && execution_stop
        && !credential
        && binaries
        && policy.is_file()
        && seed.is_file()
        && bank.join("head.json").is_file()
        && bank.join("manifest.json").is_file();
    println!(
        "{}",
        json!({"captureScopesVerified":true,"capturePairs":pairs,"financialReport":financial_report,"runtime":"rust","platform":env::consts::OS,"architecture":env::consts::ARCH,"disabledDeploymentReady":valid,"networkEnabled":c["networkEnabled"],"tradingEnabled":c["tradingEnabled"],"configuredReadsPerMinute":c["readsPerMinute"],"networkStopMarker":network_stop,"executionStopMarker":execution_stop,"credentialPresent":credential,"rustBinariesPresent":binaries,"paperSessionPresent":bank.join("head.json").is_file(),"networkRequests":0,"contestOrders":0})
    );
    if !valid {
        return Err("DISABLED_DEPLOYMENT_AUDIT_FAILED".into());
    }
    Ok(())
}
fn main() {
    if run().is_err() {
        eprintln!("MOLOCH_READY_FAILED");
        std::process::exit(1)
    }
}
