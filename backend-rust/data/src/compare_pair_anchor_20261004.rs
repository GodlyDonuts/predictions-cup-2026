// data/compare-pair-anchor-20261004.mjs — legacy v2 vs corrected v3 replay of
// the diagnosed pair journal (development counterfactual).
use baal_collector::baal_pair_paper_v2 as legacy;
use baal_collector::baal_pair_paper_v3 as corrected;
use baal_service_runtime::journal::{journal_lines, JournalReadOptions};
use baal_service_runtime::json_text::{js_stringify, sha256};
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

fn sha(b: &[u8]) -> String { sha256(b) }
fn ph(v: &V) -> Result<String, String> { baal_execution::core::payload_hash(v) }
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
fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(format!("PAIR_COMPARE_{}", code)) }
}

pub fn run() -> Result<V, String> {
    let r = localappdata()?.join("SigTradingCup/paper-runtime");
    let diagnostic = read_json(Path::new("data/pair-bottleneck-20261004.json"))?;
    let dir = r.join("baal-pair-cohorts")
        .join(diagnostic["source"]["cohort"].as_str().unwrap_or(""));
    let bank = read_json(&dir.join("bank.json"))?;
    let mut hash = Sha256::new();
    let mut sequence = 0i64;
    let mut previous = String::new();
    let mut old = V::Null;
    let mut next = V::Null;
    let mut at = String::new();
    for line in journal_lines(&dir.join("journal.jsonl"), &JournalReadOptions {
        end_byte: diagnostic["source"]["bytes"].as_u64(), ..Default::default() })? {
        let row: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        let mut body = row.clone();
        let record_hash = row["hash"].as_str().unwrap_or("").to_string();
        if let V::Object(m) = &mut body { m.shift_remove("hash"); }
        sequence += 1;
        need(row["sequence"].as_i64() == Some(sequence)
            && row["previous"].as_str().unwrap_or("") == previous
            && sha(js_stringify(&body).as_bytes()) == record_hash, "CHAIN")?;
        hash.update(line.as_bytes());
        hash.update(b"\n");
        previous = record_hash;
        if row["event"]["kind"].as_str() == Some("genesis") {
            old = row["state"]["paper"].clone();
            next = old.clone();
            continue;
        }
        at = row["event"]["availableAt"].as_str().unwrap_or("").to_string();
        old = legacy::advance_baal_pair(&old, &row["event"], &bank)?;
        need(ph(&old)? == ph(&row["state"]["paper"])?, "LEGACY_REPLAY")?;
        next = corrected::advance_baal_pair(&next, &row["event"], &bank)?;
    }
    need(format!("{:x}", hash.finalize()) == diagnostic["source"]["hash"].as_str().unwrap_or("")
        && previous == diagnostic["source"]["tailHash"].as_str().unwrap_or("")
        && sequence == diagnostic["source"]["records"].as_i64().unwrap_or(0),
        "SOURCE_BINDING")?;
    let report = json!({"at": now_iso(), "source": diagnostic["source"],
        "originalExactStateReplay": true,
        "original": legacy::baal_pair_report(&old, &bank, &at)?,
        "correctedCounterfactual": corrected::baal_pair_report(&next, &bank, &at)?,
        "inputPolicyChanged": false, "parametersTuned": false,
        "comparisonKind": "Recorded causal decisions with only bounded confirmation-marker correction; development counterfactual, not untouched validation or actual execution",
        "sourceCodeHashes": V::Object(["live-paper/baal-pair-paper-v2.mjs",
            "live-paper/baal-pair-paper-v3.mjs", "data/compare-pair-anchor-20261004.mjs"]
            .iter().map(|f| -> Result<(String, V), String> {
                Ok((f.to_string(), json!(sha(&std::fs::read(f).map_err(|e| e.to_string())?))))
            }).collect::<Result<serde_json::Map<String, V>, String>>()?),
        "networkRequests": 0, "competitionOrders": 0, "profitabilityEstablished": false});
    baal_service_runtime::atomic::atomic_json_canonical(
        Path::new("data/pair-anchor-comparison-20261004.json"), &report)?;
    Ok(json!({"source": report["source"],
        "original": {"fills": report["original"]["fills"],
            "counts": report["original"]["counts"]},
        "corrected": {"fills": report["correctedCounterfactual"]["fills"],
            "counts": report["correctedCounterfactual"]["counts"],
            "cash": report["correctedCounterfactual"]["cash"],
            "realizedPnl": report["correctedCounterfactual"]["realizedPnl"],
            "liquidationPnl": report["correctedCounterfactual"]["estimatedLiquidationPnl"],
            "conditionalSurplus": report["correctedCounterfactual"]["ordinaryConditionalSettlementSurplus"]},
        "profitabilityEstablished": false}))
}
