// data/check-pair-anchor-prepared-20261004.mjs — verify the pair-anchor
// successor cohort is staged and its sealed sources are unchanged.
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};

fn ph(v: &V) -> Result<String, String> { baal_execution::core::payload_hash(v) }
fn read_json(p: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn localappdata() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).ok_or_else(|| "LOCALAPPDATA".to_string())
}
fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(format!("PAIR_PREPARED_{}", code)) }
}

pub fn run() -> Result<V, String> {
    let r = localappdata()?.join("SigTradingCup/paper-runtime");
    need(read_json(&r.join("COLLECTOR_STOP"))?["source"].as_str()
            == Some("pair-anchor-deployment-20261004")
        && !r.join("..").join("bot.lock").exists()
        && !Path::new("STOP").exists()
        && !r.join("STOP").exists(), "STOP_OR_LEASE")?;
    let preserved = read_json(Path::new("data/pair-anchor-preservation-20261004.json"))?;
    let active = read_json(&r.join("baal-pair-active.json"))?;
    let dir = r.join("baal-pair-cohorts").join(active["cohort"].as_str().unwrap_or(""));
    let bank = read_json(&dir.join("bank.json"))?;
    let stats = read_json(&dir.join("stats.json"))?;
    let journal = Journal::open(&dir.join("journal.jsonl"))?;
    need(active["cohort"] == preserved["cohort"]
        && active["bankHash"] == preserved["bankHash"]
        && active["hostRevision"].as_i64() == Some(4)
        && ph(&bank)? == active["bankHash"].as_str().unwrap_or("")
        && !baal_service_runtime::json_text::js_truthy(Some(&stats["failed"]))
        && journal.sequence == 1
        && journal.last.as_ref().map(|l| l["event"]["kind"].as_str() == Some("genesis")).unwrap_or(false)
        && stats["journalHash"].as_str().unwrap_or("") == journal.previous
        && stats["cash"] == preserved["opening"]["cash"]
        && stats["fills"] == preserved["opening"]["fills"], "UNSTARTED_SUCCESSOR")?;
    let _mod = baal_collector::frozen_paper_runtime::frozen_paper_module(
        &r, &bank, "live-paper/baal-pair-host-v4.mjs")?;
    need(ph(&baal_collector::baal_pair_host_v4::baal_pair_code_identity(
            Path::new("live-paper")))? == ph(&bank["codeIdentity"])?, "CODE_IDENTITY")?;
    let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .unwrap().as_millis() as f64;
    need(now_ms < baal_service_runtime::contract::parse_iso_ms(
            bank["review"]["expiresAt"].as_str().unwrap_or("")).unwrap_or(f64::NEG_INFINITY)
        && read_json(&r.join("competition-activation.json"))?["enabled"].as_bool() == Some(false),
        "REVIEW_OR_ACTIVATION")?;
    for (name, source) in [("public-books.jsonl", preserved["publicSource"].clone()),
        ("journal.jsonl", preserved["originalSource"].clone())] {
        let file = r.join(name);
        need(std::fs::metadata(&file).map_err(|e| e.to_string())?.len()
                == source["bytes"].as_u64().unwrap_or(0)
            && baal_collector::native_journal_restore::source_prefix_hash(
                &file, source["bytes"].as_u64().unwrap_or(0), None)?
                == source["hash"].as_str().unwrap_or(""), "SEALED_SOURCE_CHANGED")?;
    }
    let _ = sha256(b"");
    Ok(json!({"prepared": true, "cohort": active["cohort"],
        "bankHash": active["bankHash"], "bankAndAccountingPreserved": true}))
}
