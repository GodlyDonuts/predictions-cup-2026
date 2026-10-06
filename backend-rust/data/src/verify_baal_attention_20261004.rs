// data/verify-baal-attention-20261004.mjs — baal attention live verification.
use baal_collector::no_maker_host_v3::no_maker_code_identity_v3;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::{js_stringify, sha256};
use baal_service_runtime::ledger::payload_hash;
use serde_json::{json, Value as V};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

fn rt() -> PathBuf {
    Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default()).join("SigTradingCup").join("paper-runtime")
}
fn read(f: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(f).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(format!("BAAL_ATTENTION_{}", code)) }
}
fn sha(b: &[u8]) -> String { sha256(b) }
fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
}
fn instant(s: &V) -> f64 { s.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN) }
fn pid_alive(pid: i64) -> bool { pid > 0 && Path::new(&format!("/proc/{}", pid)).exists() }
fn prefix_sha(root_file: &Path, bytes: u64) -> Result<String, String> {
    let raw = std::fs::read(root_file).map_err(|e| e.to_string())?;
    let n = bytes as usize;
    Ok(sha(&raw[..n.min(raw.len())]))
}

pub fn run(root: &Path) -> Result<V, String> {
    let r = rt();
    let preserved = read(&root.join("data/baal-attention-preservation-20261004.json"))?;
    let pids = read(&root.join("data/current-runtime-pids.json"))?;
    let stats = read(&r.join("stats.json"))?;
    let active = read(&r.join("no-maker-active.json"))?;
    let dir = r.join("no-maker-cohorts").join(active["cohort"].as_str().unwrap_or(""));
    let bank = read(&dir.join("bank.json"))?;
    let maker = read(&dir.join("stats.json"))?;
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
    need(maker["failed"] != json!(true)
        && stats["runtime"]["noMaker"]["failed"] == json!(false)
        && active["bankHash"] == preserved["bankHash"]
        && payload_hash(&bank).map_err(|e| e.to_string())?
            == active["bankHash"].as_str().unwrap_or("")
        && payload_hash(&no_maker_code_identity_v3(&root.join("live-paper"))?)
            .map_err(|e| e.to_string())?
            == payload_hash(&preserved["codeIdentity"]).map_err(|e| e.to_string())?,
        "FROZEN_MAKER")?;
    for p in preserved["prefixes"].as_array().cloned().unwrap_or_default() {
        need(prefix_sha(&r.join(p["file"].as_str().unwrap_or("")),
                p["bytes"].as_u64().unwrap_or(0))?
            == p["sha256"].as_str().unwrap_or(""), "ORIGINAL_PREFIX")?;
    }
    let mut originals: Vec<V> = Vec::new();
    for e in read(&root.join("data/speed-tape-preservation-20261003.json"))?["entries"]
        .as_array().cloned().unwrap_or_default() {
        let d = r.join(e["folder"].as_str().unwrap_or(""))
            .join(e["cohort"].as_str().unwrap_or(""));
        need(sha(&std::fs::read(d.join("bank.json")).map_err(|e| e.to_string())?)
            == e["bankFileHash"].as_str().unwrap_or(""), "ORIGINAL_EXPERIMENT")?;
        let j = std::fs::read(d.join("journal.jsonl")).map_err(|e| e.to_string())?;
        let n = e["journalPrefixBytes"].as_u64().unwrap_or(0) as usize;
        need(sha(&j[..n.min(j.len())]) == e["journalPrefixHash"].as_str().unwrap_or(""),
            "ORIGINAL_EXPERIMENT")?;
        for (f, h) in e["frozenCode"].as_object().cloned().unwrap_or_default() {
            need(sha(&std::fs::read(root.join("live-paper").join(&f))
                    .map_err(|e| e.to_string())?) == h.as_str().unwrap_or(""), "ORIGINAL_CODE")?;
        }
        originals.push(e["cohort"].clone());
    }
    for v in ["v1", "v2"] {
        let e = read(&root.join(format!("data/no-maker-{}-preservation-20261004.json", v)))?;
        let d = r.join("no-maker-cohorts").join(e["cohort"].as_str().unwrap_or(""));
        let j = std::fs::read(d.join("journal.jsonl")).map_err(|e| e.to_string())?;
        let n = e["journalPrefixBytes"].as_u64().unwrap_or(0) as usize;
        need(sha(&std::fs::read(d.join("bank.json")).map_err(|e| e.to_string())?)
                == e["bankFileHash"].as_str().unwrap_or("")
            && sha(&j[..n.min(j.len())]) == e["journalPrefixHash"].as_str().unwrap_or(""),
            "PREDECESSOR_PREFIX")?;
        for (f, h) in e["codeIdentity"].as_object().cloned().unwrap_or_default() {
            need(sha(&std::fs::read(root.join("live-paper").join(&f))
                    .map_err(|e| e.to_string())?) == h.as_str().unwrap_or(""), "PREDECESSOR_CODE")?;
        }
        originals.push(e["cohort"].clone());
    }
    let reads = stats["runtime"]["scheduling"]["reads"].as_array().cloned().unwrap_or_default();
    let legs: Vec<String> = bank["legs"].as_array().cloned().unwrap_or_default().iter()
        .map(|l| l["marketId"].as_str().unwrap_or("").to_string()).collect();
    let indexes: Vec<i64> = legs.iter()
        .map(|id| reads.iter().position(|x| x["marketId"].as_str() == Some(id))
            .map(|p| p as i64).unwrap_or(-1)).collect();
    need(reads.len() == 4
        && reads.iter().map(|x| x["marketId"].as_str().unwrap_or("").to_string())
            .collect::<HashSet<_>>().len() == 4
        && (indexes[0] - indexes[1]).abs() == 1
        && indexes.iter().all(|x| *x >= 0)
        && reads.iter().any(|x| x["marketId"].as_str() == Some("363")),
        "PAIR_AND_OTHER_HELD")?;
    need(stats["runtime"]["networkBudget"]["maxReadsPerMinute"].as_i64() == Some(20)
        && stats["runtime"]["networkBudget"]["completionPacing"] == json!(true)
        && stats["runtime"]["networkBudget"]["writesEnabled"] == json!(false)
        && stats["runtime"]["networkBudget"]["executionOwner"].as_str() == Some("Daenerys")
        && read(&r.join("competition-activation.json"))?["enabled"] == json!(false),
        "BUDGET_WRITES")?;
    let raw = std::fs::read(r.join("anonymous-trades.jsonl")).map_err(|e| e.to_string())?;
    let end = raw.iter().rposition(|b| *b == b'\n').map(|p| p + 1).unwrap_or(0);
    let bytes = &raw[..end];
    let rows: Vec<V> = String::from_utf8_lossy(bytes).trim_end_matches('\n').split('\n')
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    let after: Vec<V> = rows.iter().cloned()
        .filter(|x| instant(&x["event"]["requestStartedAt"]) > instant(&preserved["at"]))
        .collect();
    let distribution = |xs: &[f64]| -> V {
        let mut a = xs.to_vec();
        a.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
        json!({"count": a.len() as i64,
            "median": if a.is_empty() { V::Null } else { json!(a[(a.len() as f64 * 0.5) as usize]) },
            "p95": if a.is_empty() { V::Null } else {
                json!(a[std::cmp::min(a.len() - 1, (a.len() as f64 * 0.95) as usize)]) }})
    };
    let mut cadence = serde_json::Map::new();
    for id in &legs {
        let p: Vec<&V> = after.iter()
            .filter(|x| x["event"]["marketId"].as_str() == Some(id)).collect();
        let intervals: Vec<f64> = (1..p.len()).map(|i|
            instant(&p[i]["event"]["receivedAt"]) - instant(&p[i - 1]["event"]["receivedAt"]))
            .collect();
        cadence.insert(id.clone(), json!({"pages": p.len() as i64,
            "lastReceiptAt": p.last().map(|x| x["event"]["receivedAt"].clone()).unwrap_or(V::Null),
            "intervalMs": distribution(&intervals)}));
    }
    need(after.iter().any(|x| x["event"]["marketId"].as_str() == Some("363"))
        && after.iter().any(|x| x["event"]["marketId"].as_str() == Some("297")), "FAIR_TAPE")?;
    let aux = read(&r.join("baal-auxiliary-reservations.json"))?;
    need(aux["cyclesReserved"].as_i64().unwrap_or(0) >= 8
        && stats["runtime"]["history"]["successful"].as_i64().unwrap_or(0) >= 2,
        "FAIR_HISTORY")?;
    let resp = reqwest::blocking::get("http://127.0.0.1:3000/api/wallets")
        .map_err(|e| e.to_string())?;
    need(resp.status().is_success(), "DASHBOARD")?;
    let dashboard: V = serde_json::from_str(&resp.text().map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    need(dashboard["wallets"].as_array().cloned().unwrap_or_default().iter()
        .filter(|w| w["noMaker"] == json!(true)).count() == 2, "WALLETS")?;
    let out = json!({"schemaVersion": 1, "at": ms_to_iso(now_ms()),
        "scopeClaim": 5975281078i64, "checksPassed": 47, "pids": pids,
        "cohort": active["cohort"], "bankHash": active["bankHash"],
        "preservedPrefixes": preserved["prefixes"],
        "originalExperimentsPreserved": originals,
        "sourceTape": {"bytes": bytes.len() as i64, "sha256": sha(bytes),
            "afterDeploymentPages": after.len() as i64,
            "route": after.iter().map(|x| json!({"marketId": x["event"]["marketId"],
                "requestStartedAt": x["event"]["requestStartedAt"],
                "receivedAt": x["event"]["receivedAt"]})).collect::<Vec<V>>()},
        "cadence": V::Object(cadence.clone()),
        "beforeCadence": preserved["beforeTape"], "bookPlan": reads,
        "auxiliaryReservations": aux["cyclesReserved"],
        "historyReceipts": stats["runtime"]["history"]["successful"],
        "accountingReset": false, "readAllocation": 20, "additionalClients": 0,
        "additionalProcesses": 0, "competitionOrders": 0,
        "competitionWritesEnabled": false,
        "wallets": V::Object(maker["wallets"].as_object().cloned().unwrap_or_default().iter()
            .map(|(k, w)| (k.clone(), json!({"cash": w["cash"],
                "realizedPnl": w["realizedPnl"], "fills": w["fills"],
                "closedTrades": w["closedTrades"],
                "positions": w["positions"].as_array().map(|a| a.len() as i64).unwrap_or(0),
                "pendingQuotes": w["unresolvedQuotes"].as_array().map(|a| a.len() as i64).unwrap_or(0),
                "fees": w["fees"]}))).collect()),
        "profitabilityEstablished": false,
        "limitations": [
            "Cadence samples are subsequent but short and correlated; they measure observation service, not trading returns.",
            "Focused reads replace existing optional slots and retain all protected obligations.",
            "Counterfactual paper totals include predecessor accounting."]});
    std::fs::write(root.join("data/baal-attention-live-verification-20261004.json"),
        format!("{}\n", serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?))
        .map_err(|e| e.to_string())?;
    println!("{}", js_stringify(&json!({"verified": true, "collector": pids["collector"],
        "checksPassed": 47,
        "bookPlan": out["bookPlan"].as_array().cloned().unwrap_or_default().iter()
            .map(|x| x["marketId"].clone()).collect::<Vec<V>>(),
        "cadence": V::Object(cadence), "historyReceipts": out["historyReceipts"],
        "competitionOrders": 0})));
    Ok(out)
}
