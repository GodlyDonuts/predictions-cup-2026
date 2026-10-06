// data/smoke-settlement-portfolio-v2-20261005.mjs — settlement portfolio v2 genesis smoke.
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::js_stringify;
use serde_json::{json, Value as V};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

fn read(f: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(f).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn rt() -> PathBuf {
    Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default()).join("SigTradingCup").join("paper-runtime")
}

pub fn run(root: &Path) -> Result<V, String> {
    let rt_dir = rt();
    let e = read(&root.join("data/settlement-portfolio-prepared-20261005-v2.json"))?;
    let bank = read(&rt_dir.join("settlement-portfolio-v2-cohorts")
        .join(e["cohort"].as_str().unwrap_or("")).join("bank.json"))?;
    let start = Instant::now();
    let mut host = baal_collector::settlement_portfolio_host_v2::SettlementPortfolioHostV2::open(
        &rt_dir, e["cohort"].as_str().unwrap_or(""),
        &json!({"binaryPath": rt_dir.join(e["nativeAppend"]["binaryPathRelative"].as_str().unwrap_or("")),
            "sourceRoot": rt_dir.join(e["nativeAppend"]["sourceRootRelative"].as_str().unwrap_or("")),
            "binding": bank["appendBackend"]}),
        Arc::new(|| {
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64).unwrap_or(0)
        }),
        Arc::new(|| true),
        root, &root.join("live-paper"), 128)?;
    let result = (|| -> Result<V, String> {
        let s = host.status()?;
        let bad_wallet = s["wallets"].as_object().cloned().unwrap_or_default().values().any(|w|
            w["cash"].as_str() != Some("100.000000") || w["fills"].as_i64() != Some(0)
                || w["positions"].as_array().map(|a| !a.is_empty()).unwrap_or(false)
                || w["pending"].as_array().map(|a| !a.is_empty()).unwrap_or(false));
        if s["bankHash"] != e["bankHash"] || s["failed"].as_bool() == Some(true) || bad_wallet {
            return Err("Unexpected initial funded state".into());
        }
        let proof = json!({"schemaVersion": 1,
            "at": ms_to_iso(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as f64).unwrap_or(0.0)),
            "cohort": e["cohort"], "bankHash": s["bankHash"], "sourceBytes": s["sourceBytes"],
            "sourceSequence": s["sourceSequence"], "sourceHash": s["sourceHash"],
            "journalSequence": s["journalSequence"], "journalHash": s["journalHash"],
            "startupMs": start.elapsed().as_secs_f64() * 1000.0,
            "realOriginalPrefixVerified": true, "genesisOnly": true,
            "initialCashPerCorrelatedLatency": "100.000000", "networkRequests": 0,
            "competitionOrders": 0, "sourceClockRenewal": false,
            "profitabilityEstablished": false});
        let mut fd = std::fs::OpenOptions::new().write(true).create_new(true)
            .open(root.join("data/settlement-portfolio-genesis-proof-20261005-v2.json"))
            .map_err(|e| e.to_string())?;
        fd.write_all(format!("{}\n",
            serde_json::to_string_pretty(&proof).map_err(|e| e.to_string())?).as_bytes())
            .map_err(|e| e.to_string())?;
        println!("{}", js_stringify(&proof));
        Ok(proof)
    })();
    // JS `finally{await host?.close()}` — a close error propagates, replacing any earlier one.
    match host.close() {
        Err(e) => Err(e),
        Ok(()) => result,
    }
}
