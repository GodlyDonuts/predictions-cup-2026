// data/verify-canonical-capacity-live-20261003.mjs — canonical capacity live verification.
use baal_service_runtime::atomic::atomic_json_canonical;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::{canonical_json, js_stringify, sha256};
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
    if ok { Ok(()) } else { Err(code.to_string()) }
}
fn sha(b: &[u8]) -> String { sha256(b) }
fn digest(v: &V) -> String { baal_collector::model::digest(v) }
fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
}
fn pid_alive(pid: i64) -> bool {
    pid > 0 && Path::new(&format!("/proc/{}", pid)).exists()
}
fn instant(s: &V) -> f64 { s.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN) }

pub fn run(root: &Path) -> Result<V, String> {
    let rt_dir = rt();
    let active = read(&rt_dir.join("canonical-capacity-active.json"))?;
    let dir = rt_dir.join("canonical-capacity-cohorts").join(active["cohort"].as_str().unwrap_or(""));
    let bank = read(&dir.join("bank.json"))?;
    let stats = read(&dir.join("stats.json"))?;
    let universe = read(&root.join("live-paper/price-universe.json"))?;
    let pids = read(&root.join("data/current-runtime-pids.json"))?;
    need(digest(&bank) == active["bankHash"].as_str().unwrap_or("")
        && digest(&bank["codeIdentity"])
            == digest(&baal_collector::canonical_capacity_monitor::capacity_code_identity(
                &root.join("live-paper"))?), "BANK_IDENTITY")?;
    let source = std::fs::read(rt_dir.join("public-books.jsonl")).map_err(|e| e.to_string())?;
    let end = source.iter().rposition(|b| *b == b'\n').map(|p| p + 1).unwrap_or(0);
    let complete = &source[..end];
    let mut records: Vec<V> = Vec::new();
    for line in String::from_utf8_lossy(complete).split('\n') {
        if line.is_empty() { continue; }
        records.push(serde_json::from_str(line).map_err(|e| e.to_string())?);
    }
    // JS `.split('\n').slice(0,-1)` drops the final empty line — equivalent to skipping empty.
    let raw = std::fs::read(dir.join("journal.jsonl")).map_err(|e| e.to_string())?;
    need(raw.is_empty() || raw.last() == Some(&10), "PARTIAL_JOURNAL")?;
    let journal_limit = stats["journalSequence"].as_i64().unwrap_or(0);
    let journal: Vec<V> = String::from_utf8_lossy(&raw).split('\n')
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str::<V>(l).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    let journal: Vec<V> = journal.into_iter()
        .filter(|r| r["sequence"].as_i64().unwrap_or(i64::MAX) <= journal_limit).collect();
    let prefix_len = bank["sourcePrefixBytes"].as_u64().unwrap_or(0) as usize;
    need(sha(&complete[..prefix_len.min(complete.len())])
        == bank["sourcePrefixHash"].as_str().unwrap_or(""), "SOURCE_PREFIX")?;
    let mut publication = baal_collector::book_publication_v2::create_book_publication_state(&universe)?;
    let mut index = 0usize;
    let mut sequence = 0i64;
    let mut previous = digest(&bank);
    let mut results: Vec<V> = Vec::new();
    let instruments: HashSet<String> = bank["review"]["legs"].as_array().cloned().unwrap_or_default()
        .iter().map(|l| format!("{}:YES", l["exchangeId"].as_str().unwrap_or(""))).collect();
    for row in &journal {
        let hash = row["hash"].as_str().unwrap_or("").to_string();
        let mut body = row.clone();
        if let Some(o) = body.as_object_mut() { o.remove("hash"); }
        sequence += 1;
        need(body["sequence"].as_i64() == Some(sequence)
            && body["previous"].as_str().unwrap_or("") == previous
            && body["bankHash"].as_str().unwrap_or("") == active["bankHash"].as_str().unwrap_or("")
            && digest(&body) == hash, "CAPACITY_JOURNAL_CHAIN")?;
        while publication["sequence"].as_i64().unwrap_or(0)
            < body["pair"]["sourceSequence"].as_i64().unwrap_or(0) {
            let rec = match records.get(index) { Some(r) => r.clone(), None => return Err("SOURCE_TRUNCATED".into()) };
            index += 1;
            publication = baal_collector::book_publication_v2::apply_book_publication(&publication, &rec)?;
            // Keep only instrument books (JS drops the rest between replays).
            let kept: serde_json::Map<String, V> = publication["books"].as_object().cloned()
                .unwrap_or_default().into_iter()
                .filter(|(id, _)| instruments.contains(id)).collect();
            publication["books"] = V::Object(kept);
        }
        need(publication["hash"] == body["pair"]["sourceHash"], "ORIGINAL_SOURCE_IDENTITY")?;
        let books: Vec<V> = bank["review"]["legs"].as_array().cloned().unwrap_or_default().iter()
            .map(|l| publication["books"][format!("{}:YES", l["exchangeId"].as_str().unwrap_or(""))].clone())
            .collect();
        let versions = body["pair"]["versions"].as_array().cloned().unwrap_or_default();
        let prospective = books.iter().enumerate().all(|(i, b)|
            b["versionKey"] == versions.get(i).cloned().unwrap_or(V::Null)
                && b["publicationSequence"].as_i64().unwrap_or(0)
                    > bank["sourcePrefixSequence"].as_i64().unwrap_or(0)
                && instant(&b["requestStartedAt"]) >= instant(&bank["cutoff"]));
        need(prospective, "PROSPECTIVE_PAIR")?;
        let replay = baal_collector::canonical_no_capacity::evaluate_canonical_no_capacity(
            &bank["review"], &books,
            if body["accountEligibility"].is_null() { None } else { Some(&body["accountEligibility"]) },
            body["at"].as_str().unwrap_or(""), &bank["policy"])?;
        need(canonical_json(&replay)? == canonical_json(&body["result"])?, "EXACT_CAPACITY_REPLAY")?;
        let max_received = books.iter().map(|b| instant(&b["receivedAt"])).fold(f64::NEG_INFINITY, f64::max);
        let min_requested = books.iter().map(|b| instant(&b["requestStartedAt"])).fold(f64::INFINITY, f64::min);
        results.push(json!({"at": body["at"], "reason": replay["reason"],
            "matchedQuantity": replay["matchedQuantity"],
            "acquisitionCost": replay["acquisitionCost"],
            "ordinaryProfit": replay["conditionalOrdinaryMinimumProfit"],
            "exceptionProfit": replay["stressedMinimumProfit"],
            "requestWindowMs": max_received - min_requested,
            "versions": body["pair"]["versions"]}));
        previous = hash;
    }
    need(sequence > 0 && sequence == journal_limit && previous == stats["journalHash"].as_str().unwrap_or(""),
        "LIVE_CHECKPOINT")?;
    let saved = read(&root.join("data/canonical-capacity-probability-preservation-20261003.json"))?;
    let pd = rt_dir.join("probability-receipt-cohorts/source-probability-20261003-v1");
    let pj = std::fs::read(pd.join("journal.jsonl")).map_err(|e| e.to_string())?;
    let pj_len = saved["journalBytes"].as_u64().unwrap_or(0) as usize;
    need(sha(&std::fs::read(pd.join("bank.json")).map_err(|e| e.to_string())?)
            == saved["bankSha256"].as_str().unwrap_or("")
        && sha(&pj[..pj_len.min(pj.len())]) == saved["journalSha256"].as_str().unwrap_or(""),
        "PROBABILITY_ACCOUNTING_CHANGED")?;
    for k in ["collector", "paper", "dashboard", "metrics"] {
        if !pid_alive(pids[k].as_i64().unwrap_or(0)) {
            return Err(format!("process.kill {} failed", pids[k]));
        }
    }
    let runtime = {
        let body = reqwest::blocking::get("http://127.0.0.1:8787/api/stats")
            .map_err(|e| e.to_string())?.text().map_err(|e| e.to_string())?;
        serde_json::from_str::<V>(&body).map_err(|e| e.to_string())?["runtime"].clone()
    };
    need(runtime["pid"] == pids["collector"] && runtime["status"].as_str() == Some("running")
        && runtime["networkBudget"]["maxReadsPerMinute"].as_i64() == Some(20)
        && runtime["networkBudget"]["writesEnabled"] == json!(false)
        && runtime["networkBudget"]["executionOwner"].as_str() == Some("Daenerys"),
        "COLLECTOR_OR_AUTHORITY")?;
    need(read(&rt_dir.join("competition-activation.json"))?["enabled"] == json!(false),
        "CONTEST_ACTIVATION")?;
    let result = json!({"at": ms_to_iso(now_ms()), "cohort": active["cohort"],
        "bankHash": active["bankHash"], "cutoff": bank["cutoff"],
        "codeCommit": "0f205146801a150fcf9dbabc1caa4b02c946e4c2",
        "sourceSequence": publication["sequence"], "journalSequence": sequence,
        "journalHash": previous, "exactOriginalSourceAndAccountReplay": true,
        "probabilityBankAndOriginalJournalPrefixPreserved": true,
        "observations": results, "latest": stats["last"], "pids": pids,
        "networkBudget": runtime["networkBudget"],
        "fourBookSlots": runtime["collectionPlan"]["fullBookBudget"],
        "extraBackgroundProcesses": 0, "newApiClients": 0, "competitionOrders": 0,
        "profitabilityEstablished": false});
    atomic_json_canonical(&root.join("data/canonical-capacity-live-verification-20261003.json"), &result)
        .map_err(|e| e.to_string())?;
    println!("{}", js_stringify(&json!({"at": result["at"], "journalSequence": sequence,
        "exactReplay": true, "probabilityAccountingPreserved": true,
        "observations": result["observations"], "extraBackgroundProcesses": 0,
        "competitionOrders": 0, "profitabilityEstablished": false})));
    Ok(result)
}
