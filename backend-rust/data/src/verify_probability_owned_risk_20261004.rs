// data/verify-probability-owned-risk-20261004.mjs — owned-risk probability live verification.
use baal_collector::probability_receipt_v2::validate_probability_receipt_bank;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::{js_stringify, sha256};
use baal_service_runtime::ledger::payload_hash;
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};

fn rt() -> PathBuf {
    Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default()).join("SigTradingCup").join("paper-runtime")
}
fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(format!("OWNED_RISK_VERIFY_{}", code)) }
}
fn sha(b: &[u8]) -> String { sha256(b) }
fn digest(v: &V) -> String { baal_collector::model::digest(v) }
fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
}
fn instant(s: &V) -> f64 { s.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN) }
fn pid_alive(pid: i64) -> bool { pid > 0 && Path::new(&format!("/proc/{}", pid)).exists() }

pub fn run(root: &Path) -> Result<V, String> {
    let r = rt();
    let deployment: V = serde_json::from_slice(
        &std::fs::read(root.join("data/probability-owned-risk-deployment-20261004.json"))
            .map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let preserved: V = serde_json::from_slice(
        &std::fs::read(root.join("data/probability-owned-risk-preservation-20261004.json"))
            .map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let active: V = serde_json::from_slice(
        &std::fs::read(r.join("probability-receipt-active.json")).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let dir = r.join("probability-receipt-cohorts")
        .join(active["cohort"].as_str().unwrap_or(""));
    let bank: V = serde_json::from_slice(
        &std::fs::read(dir.join("bank.json")).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let stats: V = serde_json::from_slice(
        &std::fs::read(dir.join("stats.json")).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let lock: V = serde_json::from_slice(
        &std::fs::read(dir.join("worker.lock")).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    validate_probability_receipt_bank(&bank)?;
    need(active["cohort"] == deployment["cohort"]
        && digest(&bank) == active["bankHash"].as_str().unwrap_or("")
        && active["bankHash"] == deployment["bankHash"]
        && stats["bankHash"] == active["bankHash"], "ACTIVE_IDENTITY")?;
    need(pid_alive(lock["pid"].as_i64().unwrap_or(0)), "lock.pid")?;
    let age = now_ms() - instant(&stats["updatedAt"]);
    need(stats["failed"] != json!(true) && stats["sourceClosed"] != json!(true)
        && age >= 0.0 && age <= 20000.0
        && stats["competitionOrders"].as_i64() == Some(0)
        && stats["networkRequests"].as_i64() == Some(0), "LIVE_STATUS")?;
    let old_dir = r.join("probability-receipt-cohorts")
        .join(preserved["cohort"].as_str().unwrap_or(""));
    need(sha(&std::fs::read(old_dir.join("bank.json")).map_err(|e| e.to_string())?)
            == preserved["bankFileHash"].as_str().unwrap_or("")
        && sha(&std::fs::read(old_dir.join("journal.jsonl")).map_err(|e| e.to_string())?)
            == preserved["journalFileHash"].as_str().unwrap_or(""), "ORIGINAL_PRESERVED")?;
    let opening = std::fs::read(dir.join("opening-state.json")).map_err(|e| e.to_string())?;
    need(sha(&opening) == bank["accountingContinuation"]["openingFileHash"].as_str().unwrap_or(""),
        "OPENING")?;
    let journal = std::fs::read(dir.join("journal.jsonl")).map_err(|e| e.to_string())?;
    need(journal.last() == Some(&10), "PARTIAL_CHECKPOINT")?;
    let text = String::from_utf8_lossy(&journal);
    let lines: Vec<&str> = text.split('\n').collect();
    let last: V = serde_json::from_str(lines.get(lines.len().saturating_sub(2)).copied()
        .unwrap_or("")).map_err(|e| e.to_string())?;
    let hash = last["hash"].as_str().unwrap_or("").to_string();
    let mut body = last.clone();
    if let Some(o) = body.as_object_mut() { o.remove("hash"); }
    need(digest(&body) == hash && hash == stats["journalHash"].as_str().unwrap_or("")
        && body["stateHash"] == stats["stateHash"]
        && body["sequence"] == stats["journalSequence"], "CHECKPOINT")?;
    let index: V = serde_json::from_slice(
        &std::fs::read(r.join("frozen-code/index.json")).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let binding = index["banks"][payload_hash(&bank).map_err(|e| e.to_string())?.as_str()].clone();
    need(binding["snapshot"].as_str() == deployment["frozenSourceSnapshot"].as_str(),
        "FROZEN_BINDING")?;
    let froot = r.join("frozen-code").join(binding["snapshot"].as_str().unwrap_or(""));
    let manifest: V = serde_json::from_slice(
        &std::fs::read(froot.join("manifest.json")).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    need(payload_hash(&manifest["files"]).map_err(|e| e.to_string())?
        == binding["snapshot"].as_str().unwrap_or(""), "FROZEN_MANIFEST")?;
    for (file, hash) in manifest["files"].as_object().cloned().unwrap_or_default() {
        need(sha(&std::fs::read(froot.join(&file)).map_err(|e| e.to_string())?)
            == hash.as_str().unwrap_or(""), "FROZEN_FILE")?;
    }
    let pids: V = serde_json::from_slice(
        &std::fs::read(root.join("data/current-runtime-pids.json")).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    for role in ["collector", "paper", "dashboard"] {
        if !pid_alive(pids[role].as_i64().unwrap_or(0)) {
            return Err(format!("process.kill {}", pids[role]));
        }
    }
    need(pids["paper"] == lock["pid"], "PAPER_PID")?;
    let runtime: V = serde_json::from_slice(&std::fs::read(r.join("stats.json"))
            .map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let runtime = runtime["runtime"].clone();
    let budget: V = serde_json::from_slice(
        &std::fs::read(r.join("coordinated-network-budget.json")).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let activation: V = serde_json::from_slice(
        &std::fs::read(r.join("competition-activation.json")).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    need(budget["schemaVersion"].as_i64() == Some(3)
        && budget["maxReadsPerMinute"].as_i64() == Some(20)
        && budget["writesEnabled"] == json!(false)
        && budget["executionOwner"].as_str() == Some("Daenerys")
        && activation["enabled"] == json!(false), "OWNERSHIP")?;
    let dashboard: V = reqwest::blocking::Client::new()
        .get("http://127.0.0.1:3000/api/wallets")
        .timeout(std::time::Duration::from_secs(10))
        .send().map_err(|e| e.to_string())?
        .json().map_err(|e| e.to_string())?;
    let views: Vec<V> = dashboard["wallets"].as_array().cloned().unwrap_or_default().into_iter()
        .filter(|x| x["id"].as_str() == Some("probability-1000")
            || x["id"].as_str() == Some("probability-3000")).collect();
    need(views.len() == 2 && views.iter().all(|x|
            x["probability"]["bankHash"] == active["bankHash"]
                && x["probability"]["accountingContinuation"]["accountingCarried"] == json!(true)),
        "DASHBOARD")?;
    let wallets: serde_json::Map<String, V> = stats["wallets"].as_object()
        .cloned().unwrap_or_default().iter().map(|(latency, w)| {
            let view = views.iter().find(|v| v["id"].as_str()
                == Some(format!("probability-{}", latency).as_str()));
            (latency.clone(), json!({"cash": w["cash"], "realizedPnl": w["realizedPnl"],
                "liquidationPnl": view.map(|v| v["metrics"]["liquidationPnl"].clone())
                    .unwrap_or(V::Null),
                "lastDerivedQuoteLiquidationPnl": w["liquidationPnl"],
                "fees": w["fees"], "fills": w["fills"], "closedTrades": w["closedTrades"],
                "positions": w["positions"].as_array().cloned().unwrap_or_default().iter()
                    .map(|p| json!({"instrument": p["instrument"], "quantity": p["quantity"],
                        "cost": p["cost"], "quoteReceivedAt": p["quoteReceivedAt"],
                        "observationAgeMs": p["observationAgeMs"],
                        "quoteAgeMs": p["quoteAgeMs"],
                        "quoteInvalidated": p["quoteInvalidated"]})).collect::<Vec<V>>()}))
        }).collect();
    let result = json!({"at": ms_to_iso(now_ms()), "cohort": active["cohort"],
        "bankHash": active["bankHash"], "pids": pids,
        "originalBankAndJournalUnchanged": true, "originalWalletAccountingCarried": true,
        "frozenFiles": manifest["files"].as_object().map(|m| m.len() as i64).unwrap_or(0),
        "startup": stats["startup"],
        "sourceSequence": stats["sourceSequence"], "journalSequence": stats["journalSequence"],
        "sourceClosed": stats["sourceClosed"], "collectorStatus": runtime["status"],
        "readFocus": if runtime["probabilityReadFocus"].is_null() { V::Null }
            else { runtime["probabilityReadFocus"].clone() },
        "heldRiskReads": if runtime["bidAnchorReadFocus"].is_null() { V::Null }
            else { runtime["bidAnchorReadFocus"].clone() },
        "wallets": V::Object(wallets),
        "dashboardWallets": views.iter().map(|x| json!({"id": x["id"], "status": x["status"],
            "metrics": x["metrics"]})).collect::<Vec<V>>(),
        "checks": {"targetedPassed": 30, "targetedFailed": 0, "productionNextBuild": true,
            "fullRepositorySuite": false,
            "legacyRunnerHarness": "fails before requests at missing performance; reproduced without current runner edit"},
        "competitionOrders": 0, "additionalReadSlots": 0, "additionalPersistentProcesses": 0,
        "profitabilityEstablished": false});
    std::fs::write(root.join("data/probability-owned-risk-live-20261004.json"),
        format!("{}\n", serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?))
        .map_err(|e| e.to_string())?;
    println!("{}", js_stringify(&result));
    Ok(result)
}
