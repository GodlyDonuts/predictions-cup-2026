// data/prepare-baal-pair-v2-20261004.mjs — baal-pair host revision 2 cohort
// carrying forward the v1 accounting continuation.
use baal_collector::baal_pair_host::baal_pair_code_identity as prior_code_identity;
use baal_collector::baal_pair_host_v2::baal_pair_code_identity;
use baal_collector::baal_pair_paper_v2::baal_pair_policy;
use baal_execution::core::payload_hash;
use baal_service_runtime::json_text::{js_stringify, sha256};
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
fn write_pretty(p: &str, v: &V, exclusive: bool) -> Result<(), String> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true);
    if exclusive { o.create_new(true); } else { o.create(true).truncate(true); }
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
            == Some("baal-pair-depth-deployment-20261004")
        && !r.join("..").join("bot.lock").exists(), "STOP_OR_LEASE")?;
    let name = "baal-no-pair-20261004-v2";
    let dir = r.join("baal-pair-cohorts").join(name);
    let active = read_json(&r.join("baal-pair-active.json"))?;
    let prior = r.join("baal-pair-cohorts").join(active["cohort"].as_str().unwrap_or(""));
    need(active["cohort"].as_str() == Some("baal-no-pair-20261004-v1")
        && !dir.exists(), "NEW_COHORT_ONLY")?;
    let bank_bytes = std::fs::read(prior.join("bank.json")).map_err(|e| e.to_string())?;
    let prior_bank: V = serde_json::from_slice(&bank_bytes).map_err(|e| e.to_string())?;
    let prior_journal = std::fs::read(prior.join("journal.jsonl")).map_err(|e| e.to_string())?;
    need(prior_journal.last() == Some(&b'\n')
        && payload_hash(&prior_bank).ok() == active["bankHash"].as_str().map(|s| s.to_string())
        && payload_hash(&prior_bank["codeIdentity"]).ok()
            == payload_hash(&prior_code_identity(Path::new("live-paper"))).ok(),
        "PRIOR_IDENTITY")?;
    let text = String::from_utf8(prior_journal.clone()).map_err(|e| e.to_string())?;
    let rows: Vec<V> = text.trim_end().split('\n')
        .map(|l| serde_json::from_str(l).unwrap_or(V::Null)).collect();
    let mut previous = String::new();
    for (i, row) in rows.iter().enumerate() {
        let mut body = row.clone();
        let hash = row["hash"].as_str().unwrap_or("").to_string();
        if let V::Object(m) = &mut body { m.shift_remove("hash"); }
        need(row["sequence"].as_i64() == Some(i as i64 + 1)
            && row["previous"].as_str().unwrap_or("") == previous
            && sha(js_stringify(&body).as_bytes()) == hash, "PRIOR_CHAIN")?;
        previous = hash;
    }
    let seed = rows.last().map(|r| r["state"]["paper"].clone()).unwrap_or(V::Null);
    let seed_bytes = serde_json::to_string(&seed).map_err(|e| e.to_string())?.into_bytes();
    let source = std::fs::read(r.join("public-books.jsonl")).map_err(|e| e.to_string())?;
    need(source.last() == Some(&b'\n'), "SOURCE_COMPLETE")?;
    let tail_start = source[..source.len() - 1].iter().rposition(|c| *c == b'\n')
        .map(|i| i + 1).unwrap_or(0);
    let tail: V = serde_json::from_slice(&source[tail_start..source.len() - 1])
        .map_err(|e| e.to_string())?;
    let cutoff = now_iso();
    need(parse_iso_ms(&cutoff).unwrap_or(f64::NAN)
            < parse_iso_ms(prior_bank["review"]["expiresAt"].as_str().unwrap_or(""))
                .unwrap_or(f64::NAN), "CURRENT_REVIEW")?;
    let continuation = json!({"cohort": active["cohort"],
        "bankHash": active["bankHash"], "bankFileHash": sha(&bank_bytes),
        "journalPrefixBytes": prior_journal.len(),
        "journalPrefixHash": sha(&prior_journal),
        "openingStateFileHash": sha(&seed_bytes),
        "reason": "full_depth_participation_execution_correction",
        "accountingCarried": true});
    // bank = {...priorBank, name, hostRevision:2, cutoff, policy, codeIdentity,
    //   sourcePrefix*, accountingContinuation} — spread then overrides in order.
    let mut bank = prior_bank.clone();
    if let V::Object(m) = &mut bank {
        m.insert("name".into(), json!(name));
        m.insert("hostRevision".into(), json!(2));
        m.insert("cutoff".into(), json!(cutoff));
        m.insert("policy".into(), baal_pair_policy());
        m.insert("codeIdentity".into(), baal_pair_code_identity(Path::new("live-paper")));
        m.insert("sourcePrefixBytes".into(), json!(source.len()));
        m.insert("sourcePrefixHash".into(), json!(sha(&source)));
        m.insert("sourcePrefixSequence".into(), tail["sequence"].clone());
        m.insert("sourcePrefixTailHash".into(), tail["hash"].clone());
        m.insert("accountingContinuation".into(), continuation.clone());
    }
    mkdir_700(&dir)?;
    {
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create(true).truncate(true);
        #[cfg(unix)] { std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600); }
        let mut f = o.open(dir.join("opening-state.json")).map_err(|e| e.to_string())?;
        std::io::Write::write_all(&mut f, &seed_bytes).map_err(|e| e.to_string())?;
    }
    baal_service_runtime::atomic::atomic_json_canonical(&dir.join("bank.json"), &bank)?;
    baal_service_runtime::atomic::atomic_json_canonical(&r.join("baal-pair-active.json"),
        &json!({"schemaVersion": 1, "cohort": name, "hostRevision": 2,
            "bankHash": payload_hash(&bank)?, "executionAuthority": false}))?;
    write_pretty("data/baal-pair-bank-20261004-v2.json", &bank, false)?;
    let preserved = json!({"at": cutoff, "cohort": continuation["cohort"],
        "bankHash": continuation["bankHash"],
        "bankFileHash": continuation["bankFileHash"],
        "journalPrefixBytes": continuation["journalPrefixBytes"],
        "journalPrefixHash": continuation["journalPrefixHash"],
        "openingStateFileHash": continuation["openingStateFileHash"],
        "reason": continuation["reason"],
        "accountingCarried": continuation["accountingCarried"],
        "codeIdentity": prior_bank["codeIdentity"],
        "openingWallet": {"cash": seed["cash"], "realizedPnl": seed["realizedPnl"],
            "fills": seed["fills"].as_array().map(|a| a.len()).unwrap_or(0),
            "positions": seed["positions"].as_object().map(|m| m.len()).unwrap_or(0),
            "pending": seed["pending"]["stage"].clone()},
        "newCohort": name, "newBankHash": payload_hash(&bank)?,
        "accountingReset": false, "competitionOrders": 0});
    write_pretty("data/baal-pair-v1-preservation-20261004.json", &preserved, false)?;
    Ok(json!({"cohort": name, "cutoff": cutoff, "bankHash": payload_hash(&bank)?,
        "accountingCarried": true, "openingWallet": preserved["openingWallet"],
        "competitionOrders": 0}))
}
