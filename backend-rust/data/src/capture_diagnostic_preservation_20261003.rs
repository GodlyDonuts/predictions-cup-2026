// data/capture-diagnostic-preservation-20261003.mjs — hash-snapshot the live
// diagnostic cohorts and protocols before later work.
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};

fn sha(b: &[u8]) -> String { sha256(b) }
fn read_json(p: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn localappdata() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).ok_or_else(|| "LOCALAPPDATA".to_string())
}
fn now_iso() -> String {
    baal_service_runtime::contract::ms_to_iso(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as f64)
}

pub fn run() -> Result<V, String> {
    let rt = localappdata()?.join("SigTradingCup").join("paper-runtime");
    let cohorts = [
        "executable-edge-cohorts/executable-edge-20261003-v2",
        "bid-anchor-live-cohorts/bid-anchor-live-20261003-v1",
        "bulk-dip-cohorts/bulk-dip-live-20261003-v1",
        "bulk-dip-cohorts/bulk-dip-live-20261003-v2"];
    let mut banks: Vec<V> = Vec::new();
    for cohort in cohorts {
        let dir = rt.join(cohort);
        let bank = std::fs::read(dir.join("bank.json")).map_err(|e| e.to_string())?;
        let journal = std::fs::read(dir.join("journal.jsonl")).map_err(|e| e.to_string())?;
        banks.push(json!({"cohort": cohort, "bankBytes": bank.len(),
            "bankSha256": sha(&bank), "journalBytes": journal.len(),
            "journalSha256": sha(&journal)}));
    }
    let mut protocols: Vec<V> = Vec::new();
    for file in ["dip-markout-protocol-20261003-v1.json",
        "rebound-path-protocol-20261003-v1.json", "rebound-path-protocol-20261003-v2.json",
        "passive-entry-protocol-20261003-v1.json"] {
        protocols.push(json!({"file": file,
            "sha256": sha(&std::fs::read(Path::new("data").join(file)).map_err(|e| e.to_string())?)}));
    }
    let out = json!({"at": now_iso(), "banks": banks, "protocols": protocols,
        "pids": read_json(Path::new("data/current-runtime-pids.json"))?,
        "competitionOrders": 0});
    baal_service_runtime::atomic::atomic_json_canonical(
        Path::new("data/diagnostic-preservation-before-20261003.json"), &out)?;
    Ok(json!({"preservedBanks": out["banks"].as_array().map(|a| a.len()).unwrap_or(0),
        "protocols": out["protocols"].as_array().map(|a| a.len()).unwrap_or(0)}))
}
