// data/verify-primary-breadth-paper-20261005.mjs — primary breadth paper live verification.
use baal_collector::breadth::validate_universe;
use baal_collector::probability_read_focus::read_probability_read_focus;
use baal_service_runtime::atomic::atomic_json_canonical;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::{js_stringify, sha256};
use serde_json::{json, Value as V};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

fn rt() -> PathBuf {
    Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default()).join("SigTradingCup").join("paper-runtime")
}
fn sha(b: &[u8]) -> String { sha256(b) }
fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
}
fn instant(s: &V) -> f64 { s.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN) }
fn pid_alive(pid: i64) -> bool { pid > 0 && Path::new(&format!("/proc/{}", pid)).exists() }

pub fn run(root: &Path) -> Result<V, String> {
    let rt_dir = rt();
    let initial: V = serde_json::from_slice(
        &std::fs::read(root.join("data/primary-breadth-paper-initialization-20261005.json"))
            .map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let pids: V = serde_json::from_slice(
        &std::fs::read(root.join("data/current-runtime-pids.json")).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let universe_raw: V = serde_json::from_slice(
        &std::fs::read(root.join("live-paper/price-universe.json")).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let config_raw: V = serde_json::from_slice(
        &std::fs::read(root.join("live-paper/config.json")).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let u = validate_universe(&universe_raw, config_raw["tournamentId"].as_str().unwrap_or(""))?;
    let mut cohorts: Vec<V> = Vec::new();
    for file in ["probability-receipt-active.json", "calibrated-poll-paper-active.json",
        "primary-breadth-paper-active.json"] {
        let pointer: V = serde_json::from_slice(
            &std::fs::read(rt_dir.join(file)).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let dir = rt_dir.join("probability-receipt-cohorts")
            .join(pointer["cohort"].as_str().unwrap_or(""));
        let s: V = serde_json::from_slice(
            &std::fs::read(dir.join("stats.json")).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let lock: V = serde_json::from_slice(
            &std::fs::read(dir.join("worker.lock")).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        if !pid_alive(lock["pid"].as_i64().unwrap_or(0)) {
            return Err(format!("process.kill {}", lock["pid"]));
        }
        if lock["pid"] != pids["paper"] || s["failed"] == json!(true)
            || s["sourceClosed"] == json!(true)
            || now_ms() - instant(&s["updatedAt"]) > 20000.0
            || s["bankHash"] != pointer["bankHash"] {
            return Err("Paper worker not yet live/current".into());
        }
        let original = initial["preserved"].as_array().cloned().unwrap_or_default()
            .into_iter().find(|x| x["cohort"] == pointer["cohort"]);
        if let Some(original) = original {
            if sha(&std::fs::read(dir.join("bank.json")).map_err(|e| e.to_string())?)
                != original["bankFileHash"].as_str().unwrap_or("") {
                return Err("Prior bank changed".into());
            }
            let b = std::fs::read(dir.join("journal.jsonl")).map_err(|e| e.to_string())?;
            let prefix = original["journalPrefixBytes"].as_u64().unwrap_or(0) as usize;
            if b.len() < prefix || sha(&b[..prefix])
                != original["journalPrefixHash"].as_str().unwrap_or("") {
                return Err("Original journal prefix changed".into());
            }
        }
        let focus = read_probability_read_focus(&rt_dir, &u, now_ms(), file, root)
            .ok_or("Read focus not verified")?;
        let wallets: serde_json::Map<String, V> = s["wallets"].as_object()
            .cloned().unwrap_or_default().iter().map(|(k, v)| {
                (k.clone(), json!({"cash": v["cash"], "fills": v["fills"],
                    "closedTrades": v["closedTrades"], "realizedPnl": v["realizedPnl"],
                    "fees": v["fees"], "estimatedLiquidationPnl": v["liquidationPnl"],
                    "positions": v["positions"].as_array().cloned().unwrap_or_default().iter()
                        .map(|p| json!({"instrument": p["instrument"], "quantity": p["quantity"],
                            "cost": p["cost"]})).collect::<Vec<V>>(),
                    "pending": v["pending"].as_array().cloned().unwrap_or_default().iter()
                        .map(|p| json!({"action": p["action"], "createdAt": p["createdAt"]}))
                        .collect::<Vec<V>>()}))
            }).collect();
        cohorts.push(json!({"cohort": pointer["cohort"], "bankHash": s["bankHash"],
            "at": s["updatedAt"], "journalSequence": s["journalSequence"],
            "sourceSequence": s["sourceSequence"], "failed": s["failed"],
            "sourceClosed": s["sourceClosed"], "focus": focus["markets"],
            "wallets": V::Object(wallets)}));
    }
    let activation: V = serde_json::from_slice(
        &std::fs::read(rt_dir.join("competition-activation.json")).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let now = ms_to_iso(now_ms());
    if instant(&activation["entryExpiresAt"]) > now_ms() {
        return Err("Unexpected renewed official entry window".into());
    }
    let public_file = rt_dir.join("public-books.jsonl");
    let size = std::fs::metadata(&public_file).map_err(|e| e.to_string())?.len();
    let mut tail = vec![0u8; std::cmp::min(size, 3000000u64) as usize];
    {
        let mut fd = std::fs::File::open(&public_file).map_err(|e| e.to_string())?;
        fd.seek(SeekFrom::Start(size - tail.len() as u64)).map_err(|e| e.to_string())?;
        fd.read(&mut tail).map_err(|e| e.to_string())?;
    }
    let text = String::from_utf8_lossy(&tail).to_string();
    let mut lines: Vec<&str> = text.split('\n').collect();
    if size > tail.len() as u64 { lines.remove(0); }
    lines.pop(); // slice(..., -1) always drops the final element
    let governor_receipts: Vec<V> = lines.iter()
        .map(|l| serde_json::from_str::<V>(l).unwrap_or(V::Null))
        .filter(|r| r["kind"].as_str() == Some("book")
            && r["payload"]["book"]["marketId"].as_str() == Some("195")
            && instant(&r["at"]) > instant(&initial["cutoff"]))
        .collect::<Vec<V>>().iter().rev().take(5).rev().cloned()
        .map(|r| json!({"at": r["at"],
            "requestStartedAt": r["payload"]["book"]["requestStartedAt"],
            "receivedAt": r["payload"]["book"]["receivedAt"],
            "sourceAt": r["payload"]["book"]["sourceAt"],
            "sequence": r["payload"]["book"]["sequence"],
            "publicationSequence": r["sequence"],
            "bid": r["payload"]["book"]["bids"][0],
            "ask": r["payload"]["book"]["asks"][0]}))
        .collect();
    let collector: V = serde_json::from_slice(
        &std::fs::read(rt_dir.join("stats.json")).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let bulk = collector["runtime"]["coupledBulk"].clone();
    let drain: V = serde_json::from_slice(
        &std::fs::read(rt_dir.join("COUPLED_BULK_ENTRY_DRAIN")).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if collector["runtime"]["status"].as_str() != Some("running")
        || collector["runtime"]["lastCycleAt"].is_null()
        || now_ms() - instant(&collector["runtime"]["lastCycleAt"]) > 90000.0
        || drain["source"].as_str() != Some("human_profitability_focus_20261005")
        || drain["cohort"] != bulk["cohort"] || drain["bankHash"] != bulk["bankHash"] {
        return Err("Collector/drain not yet current".into());
    }
    let journal = rt_dir.join("coupled-bulk-cohorts")
        .join(bulk["cohort"].as_str().unwrap_or("")).join("journal.jsonl");
    let jsize = std::fs::metadata(&journal).map_err(|e| e.to_string())?.len();
    let mut jtail = vec![0u8; std::cmp::min(jsize, 3000000u64) as usize];
    {
        let mut jfd = std::fs::File::open(&journal).map_err(|e| e.to_string())?;
        jfd.seek(SeekFrom::Start(jsize - jtail.len() as u64)).map_err(|e| e.to_string())?;
        jfd.read(&mut jtail).map_err(|e| e.to_string())?;
    }
    let jtext = String::from_utf8_lossy(&jtail).to_string();
    let mut jlines: Vec<&str> = jtext.split('\n').collect();
    if jsize > jtail.len() as u64 { jlines.remove(0); }
    jlines.pop();
    let drain_events: Vec<V> = jlines.iter()
        .map(|l| serde_json::from_str::<V>(l).unwrap_or(V::Null))
        .filter(|r| r["event"]["reason"].as_str() == Some("operator_drain_unowned_entry_features"))
        .collect::<Vec<V>>().iter().rev().take(4).rev().cloned()
        .map(|r| json!({"at": r["at"], "reason": r["event"]["reason"], "sequence": r["sequence"]}))
        .collect();
    if drain_events.is_empty() {
        return Err("Durable drain event not observed".into());
    }
    let result = json!({"at": now,
        "pids": {"collector": pids["collector"], "paper": pids["paper"],
            "dashboard": pids["dashboard"]},
        "cohorts": cohorts, "governorReceipts": governor_receipts,
        "subsequentGovernorReceiptSeen": !governor_receipts.is_empty(),
        "bulkDrain": {"cohort": bulk["cohort"], "bankHash": bulk["bankHash"],
            "mode": drain["mode"], "durableEvents": drain_events,
            "riskMarkets": bulk["riskMarketIds"], "counts": bulk["counts"],
            "wallet": {"fills": bulk["wallets"]["COUPLED"]["fills"],
                "closedTrades": bulk["wallets"]["COUPLED"]["closedTrades"],
                "realizedPnl": bulk["wallets"]["COUPLED"]["realizedPnl"],
                "reserved": bulk["wallets"]["COUPLED"]["reserved"],
                "positions": bulk["wallets"]["COUPLED"]["positions"].as_array()
                    .cloned().unwrap_or_default().iter()
                    .map(|p| json!({"marketId": p["marketId"], "side": p["side"],
                        "cost": p["cost"], "quantity": p["quantity"]})).collect::<Vec<V>>()}},
        "officialEntryWindowClosed": true,
        "originalActivationExpiry": activation["entryExpiresAt"],
        "originalBanksAndJournalPrefixesPreserved": true, "fees": 0,
        "additionalPersistentServices": 0, "additionalAccountClients": 0,
        "profitabilityEstablished": false});
    atomic_json_canonical(&root.join("data/primary-breadth-paper-live-20261005.json"), &result)
        .map_err(|e| e.to_string())?;
    println!("{}", js_stringify(&result));
    Ok(result)
}
