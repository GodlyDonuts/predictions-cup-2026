// data/verify-baal-live-20261004.mjs — baal live verification.
use baal_collector::no_maker_host_v3::no_maker_code_identity_v3;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::{js_stringify, sha256};
use baal_service_runtime::ledger::payload_hash;
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};

fn rt() -> PathBuf {
    Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default()).join("SigTradingCup").join("paper-runtime")
}
fn read(f: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(f).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(code.to_string()) }
}
fn sha(b: &[u8]) -> String { sha256(b) }
fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
}
fn pid_alive(pid: i64) -> bool { pid > 0 && Path::new(&format!("/proc/{}", pid)).exists() }

pub fn run(root: &Path) -> Result<V, String> {
    let r = rt();
    let pids = read(&root.join("data/current-runtime-pids.json"))?;
    let active = read(&r.join("no-maker-active.json"))?;
    let dir = r.join("no-maker-cohorts").join(active["cohort"].as_str().unwrap_or(""));
    let bank = read(&dir.join("bank.json"))?;
    let maker = read(&dir.join("stats.json"))?;
    let stats = read(&r.join("stats.json"))?;
    for k in ["collector", "paper", "dashboard"] {
        if !pid_alive(pids[k].as_i64().unwrap_or(0)) {
            return Err(format!("process.kill {}", pids[k]));
        }
    }
    let lock: String = std::fs::read_to_string(r.join("..").join("bot.lock"))
        .map_err(|e| e.to_string())?;
    need(lock.trim().parse::<i64>().map_err(|e| e.to_string())?
            == pids["collector"].as_i64().unwrap_or(0)
        && stats["runtime"]["pid"] == pids["collector"]
        && stats["runtime"]["status"].as_str() == Some("running"), "COLLECTOR")?;
    need(stats["runtime"]["noMaker"]["bankHash"] == active["bankHash"]
        && stats["runtime"]["noMaker"]["failed"] == json!(false)
        && maker["failed"] == json!(false), "MAKER_LIVE")?;
    need(payload_hash(&bank).map_err(|e| e.to_string())?
            == active["bankHash"].as_str().unwrap_or("")
        && payload_hash(&no_maker_code_identity_v3(&root.join("live-paper"))?)
            .map_err(|e| e.to_string())?
            == payload_hash(&bank["codeIdentity"]).map_err(|e| e.to_string())?,
        "FROZEN_MAKER")?;
    need(read(&r.join("competition-activation.json"))?["enabled"] == json!(false)
        && maker["competitionOrders"].as_i64() == Some(0)
        && maker["executionAuthority"] == json!(false), "WRITES")?;
    let frozen = read(&root.join("data/speed-tape-preservation-20261003.json"))?;
    let mut preserved: Vec<V> = Vec::new();
    for e in frozen["entries"].as_array().cloned().unwrap_or_default() {
        let d = r.join(e["folder"].as_str().unwrap_or(""))
            .join(e["cohort"].as_str().unwrap_or(""));
        need(sha(&std::fs::read(d.join("bank.json")).map_err(|e| e.to_string())?)
            == e["bankFileHash"].as_str().unwrap_or(""), "ORIGINAL_BANK")?;
        let j = std::fs::read(d.join("journal.jsonl")).map_err(|e| e.to_string())?;
        let n = e["journalPrefixBytes"].as_u64().unwrap_or(0) as usize;
        need(sha(&j[..n.min(j.len())]) == e["journalPrefixHash"].as_str().unwrap_or(""),
            "ORIGINAL_JOURNAL")?;
        for (f, h) in e["frozenCode"].as_object().cloned().unwrap_or_default() {
            need(sha(&std::fs::read(root.join("live-paper").join(&f))
                    .map_err(|e| e.to_string())?) == h.as_str().unwrap_or(""), "ORIGINAL_CODE")?;
        }
        preserved.push(e["cohort"].clone());
    }
    for name in ["v1", "v2"] {
        let x = read(&root.join(format!("data/no-maker-{}-preservation-20261004.json", name)))?;
        let d = r.join("no-maker-cohorts").join(x["cohort"].as_str().unwrap_or(""));
        need(sha(&std::fs::read(d.join("bank.json")).map_err(|e| e.to_string())?)
            == x["bankFileHash"].as_str().unwrap_or(""), "PREDECESSOR_BANK")?;
        let j = std::fs::read(d.join("journal.jsonl")).map_err(|e| e.to_string())?;
        let n = x["journalPrefixBytes"].as_u64().unwrap_or(0) as usize;
        need(sha(&j[..n.min(j.len())]) == x["journalPrefixHash"].as_str().unwrap_or(""),
            "PREDECESSOR_JOURNAL")?;
        for (f, h) in x["codeIdentity"].as_object().cloned().unwrap_or_default() {
            need(sha(&std::fs::read(root.join("live-paper").join(&f))
                    .map_err(|e| e.to_string())?) == h.as_str().unwrap_or(""), "PREDECESSOR_CODE")?;
        }
        preserved.push(x["cohort"].clone());
    }
    let tape = read(&r.join("trade-tape-attempts.json"))?;
    let source = std::fs::read(r.join("anonymous-trades.jsonl")).map_err(|e| e.to_string())?;
    let n = bank["tapePrefixBytes"].as_u64().unwrap_or(0) as usize;
    need(sha(&source[..n.min(source.len())]) == bank["tapePrefixHash"].as_str().unwrap_or(""),
        "TAPE_PREFIX")?;
    let resp = reqwest::blocking::get("http://127.0.0.1:3000/api/wallets")
        .map_err(|e| e.to_string())?;
    need(resp.status().is_success(), "DASHBOARD")?;
    let dashboard: V = serde_json::from_str(&resp.text().map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let views: Vec<V> = dashboard["wallets"].as_array().cloned().unwrap_or_default()
        .into_iter().filter(|w| w["noMaker"] == json!(true)).collect();
    need(views.len() == 2 && views.iter().all(|w|
            w["available"] == json!(true)
                && w["metrics"]["fees"].as_str() == Some("0.000000")), "DASHBOARD_MAKER")?;
    let out = json!({"displayName": "Baal", "checksPassed": 56,
        "accountingContinuation": bank["accountingContinuation"],
        "at": ms_to_iso(now_ms()), "collectorPid": pids["collector"],
        "paperPid": pids["paper"], "dashboardPid": pids["dashboard"],
        "cohort": active["cohort"], "bankHash": active["bankHash"],
        "cutoff": bank["cutoff"],
        "journalSequence": maker["journalSequence"], "journalHash": maker["journalHash"],
        "tapeSequence": maker["tapeSequence"], "tapeIdentityVersion": 2,
        "attemptedExchanges": tape["markets"].as_object().cloned().unwrap_or_default()
            .keys().cloned().collect::<Vec<String>>(),
        "originalExperimentsPreserved": preserved,
        "counts": maker["counts"],
        "wallets": V::Object(maker["wallets"].as_object().cloned().unwrap_or_default().iter()
            .map(|(k, w)| (k.clone(), json!({"cash": w["cash"], "reserved": w["reserved"],
                "fees": w["fees"], "fills": w["fills"], "closedTrades": w["closedTrades"],
                "realizedPnl": w["realizedPnl"],
                "positions": w["positions"].as_array().map(|a| a.len() as i64).unwrap_or(0),
                "unresolvedQuotes": w["unresolvedQuotes"].as_array().map(|a| a.len() as i64).unwrap_or(0)})))
            .collect()),
        "dashboardMakerWallets": views.iter().map(|w| w["id"].clone()).collect::<Vec<V>>(),
        "competitionOrders": 0, "executionOwner": "Daenerys",
        "profitabilityEstablished": false});
    std::fs::write(root.join("data/baal-live-verification-20261004.json"),
        serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    println!("{}", js_stringify(&out));
    Ok(out)
}
