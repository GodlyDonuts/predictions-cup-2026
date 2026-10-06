// data/prepare-baal-pair-20261004.mjs — create the first baal-no-pair cohort
// bank and active pointer (collector stopped; no-maker is the frozen control).
use baal_collector::baal_pair_host::baal_pair_code_identity;
use baal_collector::baal_pair_paper::baal_pair_policy;
use baal_collector::no_maker_host_v3::no_maker_code_identity_v3;
use baal_execution::core::payload_hash;
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
fn parse_iso_ms(s: &str) -> Option<f64> {
    baal_service_runtime::contract::parse_iso_ms(s)
}
fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(code.to_string()) }
}
fn write_json_wx(p: &Path, v: &V) -> Result<(), String> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)] { std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600); }
    let mut f = o.open(p).map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut f,
        format!("{}\n", serde_json::to_string_pretty(v).map_err(|e| e.to_string())?)
            .as_bytes()).map_err(|e| e.to_string())
}
fn mkdir_700(p: &Path) -> Result<(), String> {
    #[cfg(unix)] {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(p)
            .map_err(|e| e.to_string())
    }
    #[cfg(not(unix))] { std::fs::create_dir_all(p).map_err(|e| e.to_string()) }
}

pub fn run() -> Result<V, String> {
    let r = localappdata()?.join("SigTradingCup/paper-runtime");
    need(read_json(&r.join("COLLECTOR_STOP"))?["source"].as_str()
            == Some("baal-pair-deployment-20261004")
        && !r.join("..").join("bot.lock").exists(), "STOP_OR_LEASE")?;
    let name = "baal-no-pair-20261004-v1";
    let dir = r.join("baal-pair-cohorts").join(name);
    need(!dir.exists() && !r.join("baal-pair-active.json").exists(), "NEW_COHORT_ONLY")?;
    let active = read_json(&r.join("no-maker-active.json"))?;
    let maker_dir = r.join("no-maker-cohorts")
        .join(active["cohort"].as_str().unwrap_or(""));
    let maker_bank = read_json(&maker_dir.join("bank.json"))?;
    let maker = read_json(&maker_dir.join("stats.json"))?;
    need(payload_hash(&maker_bank).ok() == active["bankHash"].as_str().map(|s| s.to_string())
        && maker["failed"].is_null()
        && payload_hash(&maker_bank["codeIdentity"]).ok()
            == payload_hash(&no_maker_code_identity_v3(Path::new("live-paper"))?).ok(),
        "FROZEN_CONTROL")?;
    let files: Vec<String> = vec!["public-books.jsonl".into(),
        "anonymous-trades.jsonl".into(),
        format!("no-maker-cohorts/{}/journal.jsonl",
            active["cohort"].as_str().unwrap_or(""))];
    let mut prefixes = Vec::new();
    for file in &files {
        let bytes = std::fs::read(r.join(file)).map_err(|e| e.to_string())?;
        need(!bytes.is_empty() && *bytes.last().unwrap() == b'\n', "SOURCE_COMPLETE")?;
        prefixes.push(json!({"file": file, "bytes": bytes.len(),
            "sha256": sha(&bytes)}));
    }
    let source = std::fs::read(r.join("public-books.jsonl")).map_err(|e| e.to_string())?;
    let tail_start = source[..source.len() - 1].iter().rposition(|c| *c == b'\n')
        .map(|i| i + 1).unwrap_or(0);
    let tail: V = serde_json::from_slice(&source[tail_start..source.len() - 1])
        .map_err(|e| e.to_string())?;
    let cutoff = now_iso();
    need(parse_iso_ms(&cutoff).unwrap_or(f64::NAN)
            < parse_iso_ms(maker_bank["review"]["expiresAt"].as_str().unwrap_or(""))
                .unwrap_or(f64::NAN), "CURRENT_REVIEW")?;
    let tournament = read_json(Path::new("tournaments-midterm-elections.json"))?;
    let bank: V = json!({"schemaVersion": 1, "name": name,
        "mode": "baal_sequential_no_pair_paper",
        "tournamentId": maker_bank["tournamentId"], "cutoff": cutoff,
        "policy": baal_pair_policy(),
        "review": maker_bank["review"], "capacityPolicy": maker_bank["capacityPolicy"],
        "codeIdentity": baal_pair_code_identity(Path::new("live-paper")),
        "sourcePrefixBytes": source.len(), "sourcePrefixHash": sha(&source),
        "sourcePrefixSequence": tail["sequence"],
        "sourcePrefixTailHash": tail["hash"],
        "tournamentContext": {"id": tournament["id"],
            "startDate": tournament["startDate"], "endDate": tournament["endDate"],
            "status": tournament["status"],
            "source": "historical_local_official_v1_capture",
            "freshnessVerified": false},
        "economics": "Fixed price-capped sequential NO pair; ordinary surplus is conditional, never booked as cash or P&L. Failed hedge/second sale creates an actual modeled orphan loss.",
        "executionAuthority": false, "competitionOrders": 0});
    mkdir_700(&dir)?;
    baal_service_runtime::atomic::atomic_json_canonical(&dir.join("bank.json"), &bank)?;
    baal_service_runtime::atomic::atomic_json_canonical(&r.join("baal-pair-active.json"),
        &json!({"schemaVersion": 1, "cohort": name,
            "bankHash": payload_hash(&bank)?, "executionAuthority": false}))?;
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    o.open("data/baal-pair-bank-20261004-v1.json")
        .and_then(|mut f| std::io::Write::write_all(&mut f,
            format!("{}\n", serde_json::to_string_pretty(&bank).unwrap()).as_bytes()))
        .map_err(|e| e.to_string())?;
    let mut original_wallets = serde_json::Map::new();
    for (k, w) in maker["wallets"].as_object().cloned().unwrap_or_default() {
        original_wallets.insert(k, json!({"cash": w["cash"],
            "realizedPnl": w["realizedPnl"], "fills": w["fills"],
            "closedTrades": w["closedTrades"],
            "positions": w["positions"].as_array().map(|a| a.len()).unwrap_or(0),
            "pendingQuotes": w["unresolvedQuotes"].as_array()
                .map(|a| a.len()).unwrap_or(0)}));
    }
    let preservation = json!({"at": cutoff, "cohort": active["cohort"],
        "bankHash": active["bankHash"], "prefixes": prefixes,
        "originalMakerWallets": V::Object(original_wallets),
        "newCohort": name, "newBankHash": payload_hash(&bank)?,
        "independentNewWallet": true, "originalAccountingReset": false,
        "competitionOrders": 0});
    let mut o2 = std::fs::OpenOptions::new();
    o2.write(true).create_new(true);
    o2.open("data/baal-pair-preservation-20261004.json")
        .and_then(|mut f| std::io::Write::write_all(&mut f,
            format!("{}\n", serde_json::to_string_pretty(&preservation).unwrap())
                .as_bytes())).map_err(|e| e.to_string())?;
    Ok(json!({"cohort": name, "cutoff": cutoff, "bankHash": payload_hash(&bank)?,
        "originalAccountingReset": false,
        "preservedPrefixes": preservation["prefixes"].as_array()
            .map(|a| a.len()).unwrap_or(0), "competitionOrders": 0}))
}
