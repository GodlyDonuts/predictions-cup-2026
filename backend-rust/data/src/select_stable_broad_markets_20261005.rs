// data/select-stable-broad-markets-20261005.mjs — stable broad market selection.
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::{js_stringify, sha256};
use baal_service_runtime::journal::{journal_lines, JournalReadOptions};
use serde_json::{json, Value as V};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

fn read(f: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(f).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn rt() -> PathBuf {
    Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default()).join("SigTradingCup").join("paper-runtime")
}
fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
}
fn sha(b: &[u8]) -> String { sha256(b) }

pub fn run(root: &Path) -> Result<V, String> {
    let rt_dir = rt();
    let u = read(&root.join("live-paper/price-universe.json"))?;
    let universe = baal_collector::breadth::validate_universe(&u, u["tournamentId"].as_str().unwrap_or(""))?;
    let file = rt_dir.join("market-prices.jsonl");
    let size = std::fs::metadata(&file).map_err(|e| e.to_string())?.len();
    let n = size.min(1048576);
    let mut tail = vec![0u8; n as usize];
    {
        let mut fd = std::fs::File::open(&file).map_err(|e| e.to_string())?;
        fd.seek(SeekFrom::Start(size - n)).map_err(|e| e.to_string())?;
        let _ = fd.read(&mut tail).map_err(|e| e.to_string())?;
    }
    let end = size - n + tail.iter().rposition(|b| *b == b'\n').map(|p| p as u64 + 1).unwrap_or(0);
    if end == 0 { return Err("assertion failed".into()); }
    let screen_after = now_ms()
        - baal_collector::stable_market_screen::stable_screen_policy()["activityMs"].as_f64().unwrap_or(0.0);
    let mut seq = 0i64;
    let mut previous = String::new();
    let mut state = baal_collector::stable_market_screen::create_stable_screen();
    let mut bytes = 0u64;
    let mut h = sha2::Sha256::new();
    use sha2::Digest;
    for line in journal_lines(&file, &JournalReadOptions { end_byte: Some(end), ..Default::default() })? {
        let row: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        let hash = row["hash"].as_str().unwrap_or("").to_string();
        let mut body = row.clone();
        if let Some(o) = body.as_object_mut() { o.remove("hash"); }
        seq += 1;
        if row["sequence"].as_i64() != Some(seq) { return Err("assert.equal sequence".into()); }
        if row["previous"].as_str().unwrap_or("") != previous { return Err("assert.equal previous".into()); }
        if hash != sha256(js_stringify(&body).as_bytes()) { return Err("assert.equal hash".into()); }
        previous = hash;
        h.update(format!("{}\n", line).as_bytes());
        bytes += line.len() as u64 + 1;
        baal_collector::market_price_publication::validate_price_observation_pub(
            &row["event"]["snapshot"], &universe)?;
        if parse_iso_ms(row["at"].as_str().unwrap_or("")).unwrap_or(f64::NEG_INFINITY) >= screen_after {
            state = baal_collector::stable_market_screen::observe_stable_screen(
                &state, &row["event"]["snapshot"], row["at"].as_str().unwrap_or(""))?;
        }
    }
    if bytes != end { return Err(format!("assert.equal bytes {} != {}", bytes, end)); }
    let now = now_ms();
    let original = read(&rt_dir.join("stable-maker-cohorts")
        .join("baal-stable-spread-20261005-v2-live").join("bank.json"))?;
    let excluded: Vec<String> = original["legs"].as_array().cloned().unwrap_or_default().iter()
        .filter_map(|x| x["marketId"].as_str().map(String::from)).collect();
    let candidates = baal_collector::stable_market_screen::stable_screen_candidates(
        &state, now, &excluded, &[]);
    let report = json!({"schemaVersion": 1, "at": ms_to_iso(now),
        "claimComment": 5996357048i64,
        "selection": "fixed_stable_window_rank_price_changes_not_fitted_returns",
        "policy": baal_collector::stable_market_screen::stable_screen_policy(),
        "tournamentId": universe["tournamentId"],
        "universeExchanges": universe["exchanges"].as_array().map(|a| a.len() as i64).unwrap_or(0),
        "source": {"bytes": bytes, "sha256": h.finalize().iter().map(|b| format!("{:02x}", b)).collect::<String>(),
            "sequence": seq,
            "tailHash": previous},
        "screenedMarkets": state["markets"].as_object().map(|o| o.len() as i64).unwrap_or(0),
        "candidates": candidates, "invalidQuotes": state["invalidQuotes"],
        "oldScopeExcluded": true, "networkRequests": 0, "executionAuthority": false,
        "profitabilityEstablished": false,
        "limitations": ["Indicative quotes lack depth and source timestamps; fresh canonical books still gate every future paper entry.",
            "Price changes are an activity proxy, not trade volume or forecast profitability.",
            "Candidate selection uses past observed prices; a separately named future cohort is needed to measure subsequent outcomes."]});
    let mut fd = std::fs::OpenOptions::new().write(true).create_new(true)
        .open(root.join("data/stable-broad-selection-20261005.json")).map_err(|e| e.to_string())?;
    fd.write_all(format!("{}\n", serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?).as_bytes())
        .map_err(|e| e.to_string())?;
    println!("{}", js_stringify(&json!({"universe": report["universeExchanges"],
        "screened": report["screenedMarkets"], "candidates": report["candidates"],
        "bytes": bytes, "sourceRows": seq, "invalidQuotes": state["invalidQuotes"]})));
    Ok(report)
}
