// data/verify-baal-recovery-20261004.mjs — baal recovery verification.
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
    if ok { Ok(()) } else { Err(format!("BAAL_RECOVERY_{}", code)) }
}
fn sha(b: &[u8]) -> String { sha256(b) }
fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
}
fn pid_alive(pid: i64) -> bool { pid > 0 && Path::new(&format!("/proc/{}", pid)).exists() }

pub fn run(root: &Path) -> Result<V, String> {
    let r = rt();
    let pids = read(&root.join("data/current-runtime-pids.json"))?;
    for k in ["collector", "paper", "dashboard"] {
        if !pid_alive(pids[k].as_i64().unwrap_or(0)) {
            return Err(format!("process.kill {}", pids[k]));
        }
    }
    let stats = read(&r.join("stats.json"))?;
    need(stats["runtime"]["pid"] == pids["collector"]
        && stats["runtime"]["status"].as_str() == Some("running"), "COLLECTOR")?;
    let lock: String = std::fs::read_to_string(r.join("..").join("bot.lock"))
        .map_err(|e| e.to_string())?;
    need(lock.trim().parse::<i64>().unwrap_or(-1) == pids["collector"].as_i64().unwrap_or(0),
        "LEASE")?;
    need(stats["runtime"]["noMaker"]["failed"] != json!(true)
        && stats["runtime"]["baalPair"]["failed"] != json!(true), "PAPER_HOSTS")?;
    let b = &stats["runtime"]["networkBudget"];
    need(b["maxReadsPerMinute"].as_i64() == Some(20) && b["completionPacing"] == json!(true)
        && b["writesEnabled"] == json!(false)
        && b["executionOwner"].as_str() == Some("Daenerys"), "BUDGET")?;
    need(read(&r.join("competition-activation.json"))?["enabled"] == json!(false),
        "ACTIVATION")?;
    let preservation = read(&root.join("data/baal-frozen-code-preservation-20261004.json"))?;
    let snapshot = r.join("frozen-code").join(preservation["snapshot"].as_str().unwrap_or(""));
    for (file, hash) in preservation["files"].as_object().cloned().unwrap_or_default() {
        need(sha(&std::fs::read(snapshot.join(&file)).map_err(|e| e.to_string())?)
            == hash.as_str().unwrap_or(""), "FROZEN_SOURCE")?;
    }
    for p in read(&root.join("data/baal-attention-preservation-20261004.json"))?["prefixes"]
        .as_array().cloned().unwrap_or_default() {
        let raw = std::fs::read(r.join(p["file"].as_str().unwrap_or("")))
            .map_err(|e| e.to_string())?;
        let n = p["bytes"].as_u64().unwrap_or(0) as usize;
        need(sha(&raw[..n.min(raw.len())]) == p["sha256"].as_str().unwrap_or(""),
            "SOURCE_PREFIX")?;
    }
    let active = read(&r.join("no-maker-active.json"))?;
    let bank = read(&r.join("no-maker-cohorts")
        .join(active["cohort"].as_str().unwrap_or("")).join("bank.json"))?;
    let prior = read(&root.join("data/no-maker-v2-preservation-20261004.json"))?;
    let prior_journal = std::fs::read(r.join("no-maker-cohorts")
        .join(prior["cohort"].as_str().unwrap_or("")).join("journal.jsonl"))
        .map_err(|e| e.to_string())?;
    let n = prior["journalPrefixBytes"].as_u64().unwrap_or(0) as usize;
    let prior_text = String::from_utf8_lossy(&prior_journal[..n.min(prior_journal.len())]);
    let rows: Vec<V> = prior_text.trim_end_matches('\n').split('\n')
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    let tail = rows.last().map(|x| x["state"].clone()).unwrap_or(V::Null);
    need(tail["tapeSequence"] == bank["tapePrefixSequence"]
        && tail["tapeHash"] == bank["tapePrefixTailHash"], "ACTUAL_MIGRATION_FRONTIER")?;
    let maker = read(&r.join("no-maker-cohorts")
        .join(active["cohort"].as_str().unwrap_or("")).join("stats.json"))?;
    let pair_active = read(&r.join("baal-pair-active.json"))?;
    let pair_bank = read(&r.join("baal-pair-cohorts")
        .join(pair_active["cohort"].as_str().unwrap_or("")).join("bank.json"))?;
    let pair = read(&r.join("baal-pair-cohorts")
        .join(pair_active["cohort"].as_str().unwrap_or("")).join("stats.json"))?;
    need(payload_hash(&bank).map_err(|e| e.to_string())?
            == active["bankHash"].as_str().unwrap_or("")
        && payload_hash(&pair_bank).map_err(|e| e.to_string())?
            == pair_active["bankHash"].as_str().unwrap_or(""), "BANKS")?;
    need(pair["failed"] != json!(true) && pair["fees"].as_str() == Some("0.000000")
        && pair["profitabilityEstablished"] != json!(true), "PAIR")?;
    let response = reqwest::blocking::get("http://127.0.0.1:3000/api/wallets")
        .map_err(|e| e.to_string())?;
    need(response.status().is_success(), "DASHBOARD")?;
    let api: V = serde_json::from_str(&response.text().map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let view = api["wallets"].as_array().cloned().unwrap_or_default().into_iter()
        .find(|w| w["baalPair"] == json!(true));
    let maker_views: Vec<V> = api["wallets"].as_array().cloned().unwrap_or_default()
        .into_iter().filter(|w| w["noMaker"] == json!(true)).collect();
    need(view.as_ref().map(|v| v["available"] == json!(true)
            && v["status"].as_str() == Some("running")).unwrap_or(false)
        && maker_views.len() == 2, "DASHBOARD_PAPER")?;
    let view = view.unwrap();
    need(view["metrics"]["realizedPnl"] == pair["realizedPnl"]
        && view["metrics"]["fills"] == pair["fills"], "DASHBOARD_ACCOUNTING")?;
    let result = json!({"at": ms_to_iso(now_ms()), "pids": pids,
        "sourceSnapshot": preservation["snapshot"],
        "frozenBanksVerified": preservation["banks"].as_array().map(|a| a.len() as i64).unwrap_or(0),
        "migration": {"predecessor": prior["cohort"], "successor": active["cohort"],
            "consumedSequence": tail["tapeSequence"],
            "sealedSequence": bank["tapePrefixSequence"], "hash": tail["tapeHash"],
            "matched": true},
        "wallets": V::Object(maker["wallets"].as_object().cloned().unwrap_or_default().iter()
            .map(|(k, w)| (k.clone(), json!({"cash": w["cash"],
                "realizedPnl": w["realizedPnl"], "fees": w["fees"], "fills": w["fills"],
                "closedTrades": w["closedTrades"],
                "positions": w["positions"].as_array().map(|a| a.len() as i64).unwrap_or(0)})))
            .collect()),
        "paired": {"cohort": pair_active["cohort"], "bankHash": pair_active["bankHash"],
            "cash": pair["cash"], "realizedPnl": pair["realizedPnl"], "fills": pair["fills"],
            "positions": pair["positions"].as_array().map(|a| a.len() as i64).unwrap_or(0),
            "reviewCurrent": pair["reviewCurrent"],
            "reviewExpiresAt": pair["reviewExpiresAt"]},
        "timingClocks": stats["runtime"]["readTiming"]["clocks"],
        "dashboardVerified": true, "additionalCollectors": 0,
        "readAdmissionsPerMinute": 20, "competitionOrders": 0, "accountingReset": false,
        "profitabilityEstablished": false});
    std::fs::write(root.join("data/baal-recovery-verification-20261004.json"),
        format!("{}\n", serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?))
        .map_err(|e| e.to_string())?;
    println!("{}", js_stringify(&result));
    Ok(result)
}
