// data/verify-observer-retry-20261003.mjs — observer retry live verification.
use crate::frontend__app__bulk_dip_view::bulk_dip_wallet_views;
use baal_service_runtime::atomic::atomic_json_canonical;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::{js_stringify, sha256};
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};

fn rt() -> PathBuf {
    Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default()).join("SigTradingCup").join("paper-runtime")
}
fn json_file(p: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(code.to_string()) }
}
fn sha(b: &[u8]) -> String { sha256(b) }
fn digest(v: &V) -> String { baal_collector::model::digest(v) }
fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
}
fn instant(s: &V) -> f64 { s.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN) }
fn pid_alive(pid: i64) -> bool { pid > 0 && Path::new(&format!("/proc/{}", pid)).exists() }

pub fn run(root: &Path) -> Result<V, String> {
    let runtime = rt();
    let before = json_file(&root.join("data/observer-retry-runtime-before-20261003.json"))?;
    let pids = json_file(&root.join("data/current-runtime-pids.json"))?;
    for key in ["collector", "paper", "dashboard", "metrics"] {
        if !pid_alive(pids[key].as_i64().unwrap_or(0)) {
            return Err(format!("process.kill {}", pids[key]));
        }
    }
    let mut preserved: Vec<V> = Vec::new();
    for c in before["cohorts"].as_array().cloned().unwrap_or_default() {
        let dir = runtime.join(c["cohort"].as_str().unwrap_or(""));
        let bank_bytes = std::fs::read(dir.join("bank.json")).map_err(|e| e.to_string())?;
        let bank: V = serde_json::from_slice(&bank_bytes).map_err(|e| e.to_string())?;
        let journal = std::fs::read(dir.join("journal.jsonl")).map_err(|e| e.to_string())?;
        need(sha(&bank_bytes) == c["bankHash"].as_str().unwrap_or(""), "BANK_CHANGED")?;
        let jbytes = c["journalBytes"].as_u64().unwrap_or(0) as usize;
        need(journal.len() >= jbytes
            && sha(&journal[..jbytes]) == c["journalHash"].as_str().unwrap_or(""),
            "JOURNAL_PREFIX_CHANGED")?;
        for (module, hash) in bank["codeIdentity"].as_object().cloned().unwrap_or_default() {
            let resolved = if module.starts_with("./") || module.starts_with("../") {
                root.join("live-paper").join(&module)
            } else {
                root.join(&module)
            };
            need(sha(&std::fs::read(resolved).map_err(|e| e.to_string())?)
                == hash.as_str().unwrap_or(""), "FROZEN_CODE_CHANGED")?;
        }
        let retired = c["cohort"].as_str().unwrap_or("")
            .ends_with("bulk-dip-live-20261003-v1");
        let mut live = V::Null;
        if !retired {
            let stats = json_file(&dir.join("stats.json"))?;
            let lock = json_file(&dir.join("worker.lock"))?;
            let age_ms = now_ms() - instant(&stats["updatedAt"]);
            need(stats["failed"] != json!(true) && stats["sourceClosed"] != json!(true)
                && age_ms >= 0.0 && age_ms < 20000.0, "WORKER_NOT_FRESH")?;
            need(lock["pid"] == pids["paper"]
                && stats["bankHash"].as_str() == Some(digest(&bank).as_str()),
                "WORKER_IDENTITY_CHANGED")?;
            live = json!({"updatedAt": stats["updatedAt"], "ageMs": age_ms,
                "failed": stats["failed"], "sourceClosed": stats["sourceClosed"],
                "journalSequence": stats["journalSequence"],
                "sourceSequence": if stats["sourceSequence"].is_null() { stats["bookSequence"].clone() }
                    else { stats["sourceSequence"].clone() },
                "priceSequence": if stats["priceSequence"].is_null() { V::Null }
                    else { stats["priceSequence"].clone() }});
        }
        preserved.push(json!({"cohort": c["cohort"], "bankHash": digest(&bank),
            "bankPreserved": true, "originalJournalBytes": c["journalBytes"],
            "journalPrefixPreserved": true, "frozenCodePreserved": true, "live": live}));
    }
    let activation = json_file(&runtime.join("competition-activation.json"))?;
    need(activation["enabled"] == json!(false), "CONTEST_ACTIVATION_CHANGED")?;
    let collector: V = serde_json::from_str(
        &reqwest::blocking::get("http://127.0.0.1:8787/api/stats")
            .map_err(|e| e.to_string())?.text().map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let budget = collector["runtime"]["networkBudget"].clone();
    need(collector["runtime"]["pid"] == pids["collector"]
        && collector["runtime"]["status"].as_str() == Some("running"), "COLLECTOR_UNAVAILABLE")?;
    need(budget["schemaVersion"].as_i64() == Some(3)
        && budget["maxReadsPerMinute"].as_i64() == Some(20)
        && budget["writesEnabled"] == json!(false)
        && budget["executionOwner"].as_str() == Some("Daenerys"), "BUDGET_CHANGED")?;
    let cohort = "bulk-dip-live-20261003-v2";
    let dir = runtime.join("bulk-dip-cohorts").join(cohort);
    let bank = json_file(&dir.join("bank.json"))?;
    let manifest = json_file(&runtime.join("bulk-dip-active.json"))?;
    let stats = json_file(&dir.join("stats.json"))?;
    let views = bulk_dip_wallet_views(&stats, &manifest, &bank, true, None)?;
    let text = std::fs::read_to_string(runtime.join("public-books.jsonl")).map_err(|e| e.to_string())?;
    let last_book = text.trim_end().split('\n')
        .map(|x| serde_json::from_str::<V>(x).unwrap_or(V::Null)).collect::<Vec<V>>()
        .into_iter().rev().find(|x| x["kind"].as_str() == Some("book"))
        .unwrap_or(V::Null)["payload"]["book"].clone();
    let instrument = bank["universe"]["exchanges"].as_array().cloned().unwrap_or_default()
        .iter().find(|x| x["exchangeId"] == last_book["exchangeId"]
            && x["marketId"] == last_book["marketId"]).cloned().unwrap_or(V::Null);
    need(!instrument.is_null()
        && last_book["instrument"].as_str()
            == Some(format!("{}:YES", instrument["exchangeId"].as_str().unwrap_or("")).as_str()),
        "EXCHANGE_INSTRUMENT_MISMATCH")?;
    let companion = json_file(&runtime.join("executable-edge-cohorts/executable-edge-20261003-v2/companion-status.json"))?;
    need(companion["mode"].as_str() == Some("bid_anchor_companion_running"), "COMPANION_UNAVAILABLE")?;
    let result = json!({"at": ms_to_iso(now_ms()), "pids": pids, "preserved": preserved,
        "networkBudget": budget, "competitionActivation": false,
        "profitabilityEstablished": false,
        "historicalAnchorFailure": before["historicalAnchorFailure"],
        "companion": companion,
        "dashboard": {"productionBuild": "frontend-build-20261003-exchange",
            "exchangeInstrumentVerified": true,
            "publicExample": {"marketId": instrument["marketId"],
                "exchangeId": instrument["exchangeId"], "instrument": last_book["instrument"]},
            "walletViews": views.len() as i64},
        "bulk": {"cohort": cohort, "bankHash": stats["bankHash"],
            "updatedAt": stats["updatedAt"],
            "marketCount": stats["bulkReference"]["marketCount"],
            "candidates": stats["bulkReference"]["candidates"].as_array().map(|a| a.len() as i64).unwrap_or(0),
            "wallets": V::Object(stats["wallets"].as_object().cloned().unwrap_or_default().iter()
                .map(|(latency, w)| (latency.clone(), json!({"fills": w["fills"],
                    "pending": w["pending"].as_array().map(|a| a.len() as i64).unwrap_or(0),
                    "positions": w["positions"].as_array().map(|a| a.len() as i64).unwrap_or(0),
                    "realizedPnl": w["realizedPnl"], "fees": w["fees"]})))
                .collect())}});
    atomic_json_canonical(&root.join("data/observer-retry-live-verification-20261003.json"), &result)
        .map_err(|e| e.to_string())?;
    println!("{}", js_stringify(&result));
    Ok(result)
}
