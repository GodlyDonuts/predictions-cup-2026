// data/verify-funded-settlement-paper-20261005.mjs — funded settlement paper verification.
use baal_service_runtime::atomic::atomic_json_canonical;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::{js_stringify, sha256};
use baal_service_runtime::ledger::payload_hash;
use serde_json::{json, Value as V};
use std::io::Read;
use std::path::{Path, PathBuf};

fn rt() -> PathBuf {
    Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default()).join("SigTradingCup").join("paper-runtime")
}
fn read(f: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(f).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(format!("FUNDED_VALUE_PROOF_{}", code)) }
}
fn sha(b: &[u8]) -> String { sha256(b) }
fn digest(v: &V) -> String { baal_collector::model::digest(v) }
fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
}
fn instant(s: &V) -> f64 { s.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN) }
fn pid_alive(pid: i64) -> bool { pid > 0 && Path::new(&format!("/proc/{}", pid)).exists() }
fn stream_sha_prefix(file: &Path, bytes: u64) -> Result<String, String> {
    let mut fd = std::fs::File::open(file).map_err(|e| e.to_string())?;
    let mut h = sha2::Sha256::new();
    use sha2::Digest;
    let mut remaining = bytes;
    let mut buf = vec![0u8; 65536];
    while remaining > 0 {
        let chunk = std::cmp::min(remaining, buf.len() as u64) as usize;
        let n = fd.read(&mut buf[..chunk])
            .map_err(|e| e.to_string())?;
        if n == 0 { break; }
        h.update(&buf[..n]);
        remaining -= n as u64;
    }
    Ok(h.finalize().iter().map(|b| format!("{:02x}", b)).collect())
}

pub fn run(root: &Path) -> Result<V, String> {
    let rt_dir = rt();
    let now = now_ms();
    let init = read(&root.join("data/funded-settlement-paper-initialization-20261005.json"))?;
    let pids = read(&root.join("data/current-runtime-pids.json"))?;
    let dir = rt_dir.join("settlement-paper-cohorts").join(init["cohort"].as_str().unwrap_or(""));
    let bank = read(&dir.join("bank.json"))?;
    let stats = read(&dir.join("stats.json"))?;
    let lock = read(&dir.join("worker.lock"))?;
    let collector = read(&rt_dir.join("stats.json"))?;
    need(digest(&bank) == init["bankHash"].as_str().unwrap_or("")
        && stats["bankHash"] == init["bankHash"]
        && payload_hash(&bank).map_err(|e| e.to_string())? == init["canonicalIndexKey"].as_str().unwrap_or(""),
        "BANK")?;
    baal_collector::frozen_paper_runtime::frozen_paper_module(&rt_dir, &bank,
        "live-paper/settlement-paper-host.mjs")?;
    need(lock["pid"] == pids["collector"]
        && collector["runtime"]["pid"] == pids["collector"]
        && collector["runtime"]["status"].as_str() == Some("running")
        && now - instant(&collector["runtime"]["lastCycleAt"]) < 120000.0, "COLLECTOR")?;
    need(pid_alive(lock["pid"].as_i64().unwrap_or(0)), "COLLECTOR")?;
    need(stats["failed"] == json!(false) && stats["sourceClosed"] == json!(false)
        && now - instant(&stats["updatedAt"]) < 20000.0
        && stats["sourceSequence"].as_i64().unwrap_or(0)
            > bank["sourcePrefixSequence"].as_i64().unwrap_or(0), "FORWARD")?;
    need(collector["runtime"]["settlementPaper"]["bankHash"] == init["bankHash"]
        && collector["runtime"]["settlementPaper"]["failed"] == json!(false)
        && collector["runtime"]["coupledBulk"]["failed"] == json!(false), "INTEGRATION")?;
    need(sha(&std::fs::read(rt_dir.join("competition-activation.json")).map_err(|e| e.to_string())?)
        == init["activationFileHash"].as_str().unwrap_or(""), "ACTIVATION_CHANGED")?;
    for old in init["preserved"].as_array().cloned().unwrap_or_default() {
        let old_dir = rt_dir.join(old["folder"].as_str().unwrap_or(""))
            .join(old["cohort"].as_str().unwrap_or(""));
        let file = old_dir.join("journal.jsonl");
        need(sha(&std::fs::read(old_dir.join("bank.json")).map_err(|e| e.to_string())?)
            == old["bankFileHash"].as_str().unwrap_or(""), "OLD_BANK")?;
        need(std::fs::metadata(&file).map_err(|e| e.to_string())?.len()
            >= old["journalPrefixBytes"].as_u64().unwrap_or(0), "OLD_JOURNAL_TRUNCATED")?;
        need(stream_sha_prefix(&file, old["journalPrefixBytes"].as_u64().unwrap_or(0))?
            == old["journalPrefixHash"].as_str().unwrap_or(""), "OLD_JOURNAL_CHANGED")?;
    }
    let file = dir.join("journal.jsonl");
    let rows: Vec<V> = std::fs::read_to_string(&file).map_err(|e| e.to_string())?
        .trim_end().split('\n')
        .map(|l| serde_json::from_str(l).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    let tail = rows.last().cloned().unwrap_or(V::Null);
    let hash = tail["hash"].as_str().unwrap_or("").to_string();
    let mut body = tail.clone();
    if let Some(o) = body.as_object_mut() { o.remove("hash"); }
    need(digest(&body) == hash && hash == stats["journalHash"].as_str().unwrap_or("")
        && body["stateHash"] == stats["stateHash"] && body["sourceHash"] == stats["sourceHash"],
        "CHECKPOINT")?;
    let mut receipts: Vec<V> = Vec::new();
    let source_text = std::fs::read_to_string(rt_dir.join("public-books.jsonl")).map_err(|e| e.to_string())?;
    let source_lines: Vec<&str> = source_text.trim_end().split('\n').collect();
    for w in stats["wallets"].as_object().cloned().unwrap_or_default().values() {
        for fill in w["recentFills"].as_array().cloned().unwrap_or_default() {
            let idx = fill["sourceSequence"].as_i64().unwrap_or(0);
            let row: V = serde_json::from_str(source_lines.get((idx - 1) as usize).copied().unwrap_or(""))
                .map_err(|e| e.to_string())?;
            need(row["hash"] == fill["sourceHash"] && row["kind"].as_str() == Some("book")
                && row["payload"]["book"]["marketId"] == fill["marketId"]
                && instant(&fill["requestStartedAt"]) >= instant(&bank["cutoff"]),
                "FILL_SOURCE")?;
            receipts.push(json!({"latencyMs": fill["latencyMs"], "action": fill["action"],
                "at": fill["at"], "price": fill["price"], "quantity": fill["quantity"],
                "sourceSequence": fill["sourceSequence"], "sourceHash": fill["sourceHash"],
                "researchHash": fill["researchHash"]}));
        }
    }
    let result = json!({"at": ms_to_iso(now), "cohort": bank["name"], "bankHash": stats["bankHash"],
        "cutoff": bank["cutoff"], "sourceSequence": stats["sourceSequence"],
        "sourceHash": stats["sourceHash"], "journalSequence": stats["journalSequence"],
        "journalHash": stats["journalHash"], "stateHash": stats["stateHash"],
        "collectorPid": pids["collector"], "retainedPaperPid": pids["paper"],
        "retainedDashboardPid": pids["dashboard"], "workerSharesCollectorPid": true,
        "startup": stats["startup"], "lower": bank["research"][0]["lower"],
        "upper": bank["research"][0]["upper"], "researchSourceAt": bank["research"][0]["sourceAt"],
        "researchRecordedAt": bank["research"][0]["recordedAt"],
        "researchExpiresAt": bank["research"][0]["expiresAt"],
        "severeOpponentAllocationStress": init["severeOpponentAllocationStress"],
        "wallets": stats["wallets"], "receipts": receipts,
        "originalBanksAndJournalPrefixesPreserved": true, "activationUnchanged": true,
        "additionalAccountClients": 0, "additionalReadSlots": 0, "additionalPersistentServices": 0,
        "fees": 0, "competitionOrders": 0, "profitabilityEstablished": false,
        "limitations": stats["limitations"]});
    atomic_json_canonical(&root.join("data/funded-settlement-paper-live-20261005.json"), &result)
        .map_err(|e| e.to_string())?;
    let mut wallet_summary = serde_json::Map::new();
    for (k, w) in stats["wallets"].as_object().cloned().unwrap_or_default() {
        wallet_summary.insert(k, json!({"fills": w["fills"], "closedTrades": w["closedTrades"],
            "realizedPnl": w["realizedPnl"], "liquidationPnl": w["liquidationPnl"],
            "pending": w["pending"].as_array().map(|a| a.len() as i64).unwrap_or(0)}));
    }
    println!("{}", js_stringify(&json!({"at": result["at"], "collector": pids["collector"],
        "paper": pids["paper"], "dashboard": pids["dashboard"], "cohort": bank["name"],
        "originalAccountingPreserved": true, "sourceSequence": stats["sourceSequence"],
        "wallets": V::Object(wallet_summary), "competitionOrders": 0})));
    Ok(result)
}
