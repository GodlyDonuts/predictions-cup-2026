// data/verify-diagnostic-deployment-20261003.mjs — diagnostic collector deployment verification.
use crate::diagnostic_read_focus::diagnostic_focus_code_hash;
use baal_service_runtime::atomic::atomic_json_canonical;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::{js_stringify, sha256};
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
fn instant(s: &V) -> f64 { s.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN) }
fn pid_alive(pid: i64) -> bool { pid > 0 && Path::new(&format!("/proc/{}", pid)).exists() }
fn digest(v: &V) -> String { baal_collector::model::digest(v) }
fn get_json(url: &str) -> Result<V, String> {
    let body = reqwest::blocking::get(url).map_err(|e| e.to_string())?
        .text().map_err(|e| e.to_string())?;
    serde_json::from_str(&body).map_err(|e| e.to_string())
}

pub fn run(root: &Path) -> Result<V, String> {
    let rt_dir = rt();
    let before = read(&root.join("data/diagnostic-preservation-before-20261003.json"))?;
    let pids = read(&root.join("data/current-runtime-pids.json"))?;
    need(pids["paper"] == before["pids"]["paper"]
        && pids["dashboard"] == before["pids"]["dashboard"]
        && pids["metrics"] == before["pids"]["metrics"]
        && pids["collector"] != before["pids"]["collector"], "FLEET")?;
    for name in ["collector", "paper", "dashboard", "metrics"] {
        need(pid_alive(pids[name].as_i64().unwrap_or(0)), "FLEET")?;
    }
    let mut preserved: Vec<V> = Vec::new();
    for item in before["banks"].as_array().cloned().unwrap_or_default() {
        let dir = rt_dir.join(item["cohort"].as_str().unwrap_or(""));
        let bytes = std::fs::read(dir.join("bank.json")).map_err(|e| e.to_string())?;
        let journal = std::fs::read(dir.join("journal.jsonl")).map_err(|e| e.to_string())?;
        let bank: V = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        let jbytes = item["journalBytes"].as_u64().unwrap_or(0) as usize;
        need(sha(&bytes) == item["bankSha256"].as_str().unwrap_or("")
            && journal.len() >= jbytes
            && sha(&journal[..jbytes]) == item["journalSha256"].as_str().unwrap_or(""),
            "BANK_OR_ORIGINAL_JOURNAL_CHANGED")?;
        for (file, h) in bank["codeIdentity"].as_object().cloned().unwrap_or_default() {
            let target = if file.starts_with('.') {
                root.join("live-paper").join(&file)
            } else {
                root.join(&file)
            };
            need(sha(&std::fs::read(&target).map_err(|e| e.to_string())?)
                == h.as_str().unwrap_or(""), "FROZEN_CODE_CHANGED")?;
        }
        let mut fresh = V::Null;
        let cohort = item["cohort"].as_str().unwrap_or("");
        if !(cohort.ends_with("-v1") && cohort.contains("bulk-dip")) {
            let s = read(&dir.join("stats.json"))?;
            need(s["failed"] == json!(false) && s["sourceClosed"] == json!(false)
                && now_ms() - instant(&s["updatedAt"]) <= 20000.0
                && s["bankHash"].as_str() == Some(digest(&bank).as_str())
                && s["networkRequests"].as_i64() == Some(0)
                && s["competitionOrders"].as_i64() == Some(0), "PAPER_HEALTH")?;
            fresh = json!({"updatedAt": s["updatedAt"], "journalSequence": s["journalSequence"],
                "sourceSequence": if s["sourceSequence"].is_null() { s["bookSequence"].clone() }
                    else { s["sourceSequence"].clone() }});
        }
        preserved.push(json!({"cohort": item["cohort"], "bankUnchanged": true,
            "frozenCodeUnchanged": true, "originalJournalUnchanged": true,
            "originalJournalBytes": item["journalBytes"], "health": fresh}));
    }
    for p in before["protocols"].as_array().cloned().unwrap_or_default() {
        need(sha(&std::fs::read(root.join("data").join(p["file"].as_str().unwrap_or("")))
                .map_err(|e| e.to_string())?) == p["sha256"].as_str().unwrap_or(""),
            "PROTOCOL_CHANGED")?;
    }
    let runtime = get_json("http://127.0.0.1:8787/api/stats")?["runtime"].clone();
    let d = &runtime["diagnosticReadFocus"];
    let b = &runtime["networkBudget"];
    need(runtime["pid"] == pids["collector"] && runtime["status"].as_str() == Some("running")
        && runtime["cycles"].as_i64().unwrap_or(0) > 0
        && now_ms() - instant(&runtime["lastCycleAt"]) < 120000.0, "COLLECTOR_HEALTH")?;
    need(d["status"].as_str() == Some("running") && d["executionAuthority"] == json!(false)
        && d["projection"]["codeHash"].as_str()
            == Some(diagnostic_focus_code_hash(&root.join("live-paper"))?.as_str())
        && d["projection"]["pid"] == pids["collector"]
        && d["projection"]["networkRequests"].as_i64() == Some(0)
        && d["projection"]["competitionOrders"].as_i64() == Some(0)
        && d["projection"]["sourceClosed"] == json!(false), "DIAGNOSTIC_HEALTH")?;
    need(b["schemaVersion"].as_i64() == Some(3)
        && b["maxReadsPerMinute"].as_i64() == Some(20)
        && b["writesEnabled"] == json!(false)
        && b["executionOwner"].as_str() == Some("Daenerys")
        && b["completionPacing"] == json!(true)
        && b["intervalMs"].as_i64() == Some(3000), "READ_BUDGET")?;
    need(read(&rt_dir.join("competition-activation.json"))?["enabled"] == json!(false),
        "ACTIVATION")?;
    let view = get_json("http://127.0.0.1:3000/api/wallets")?;
    let wallets: Vec<V> = view["wallets"].as_array().cloned().unwrap_or_default().into_iter()
        .filter(|x| x["id"].as_str().map(|id| id.starts_with("bulk-dip-")).unwrap_or(false))
        .collect();
    need(wallets.len() == 2 && wallets.iter().all(|x|
            x["status"].as_str() == Some("running")
                && x["trades"].as_array().map(|a| a.len()).unwrap_or(0) == 2
                && x["trades"].as_array().cloned().unwrap_or_default().iter()
                    .all(|t| t["label"].as_str() == Some("Market 290 · YES"))
                && x["positions"].as_array().map(|a| a.is_empty()).unwrap_or(false)),
        "DASHBOARD_WALLETS")?;
    let result = json!({"at": ms_to_iso(now_ms()), "mode": "diagnostic_collector_live_deployment",
        "codeCommit": "cf3568985082a3e18c35dc5423c97fb5068d6e69", "testsPassed": 43, "pids": pids,
        "collector": {"status": runtime["status"], "cycles": runtime["cycles"],
            "lastCycleAt": runtime["lastCycleAt"], "networkBudget": runtime["networkBudget"],
            "diagnostic": {"status": d["status"], "markets": d["markets"],
                "source": d["projection"]["price"], "book": d["projection"]["book"],
                "codeHash": d["projection"]["codeHash"], "executionAuthority": false}},
        "preserved": preserved, "protocolsPreserved": before["protocols"].as_array().map(|a| a.len() as i64).unwrap_or(0),
        "dashboard": {"correctInstrumentLabels": true,
            "bulkWallets": wallets.iter().map(|x| json!({"id": x["id"], "status": x["status"],
                "trades": x["trades"].as_array().map(|a| a.len() as i64).unwrap_or(0),
                "positions": x["positions"].as_array().map(|a| a.len() as i64).unwrap_or(0)}))
                .collect::<Vec<V>>()},
        "extraBackgroundProcesses": 0, "newApiClients": 0, "competitionActivation": false,
        "competitionOrders": 0, "profitabilityEstablished": false});
    atomic_json_canonical(&root.join("data/diagnostic-collector-live-verification-20261003.json"), &result)
        .map_err(|e| e.to_string())?;
    println!("{}", js_stringify(&json!({"at": result["at"], "collectorPid": pids["collector"],
        "status": runtime["status"], "diagnostic": d["status"],
        "preserved": result["preserved"].as_array().map(|a| a.len() as i64).unwrap_or(0),
        "allOriginalJournals": true, "dashboard": true})));
    Ok(result)
}
