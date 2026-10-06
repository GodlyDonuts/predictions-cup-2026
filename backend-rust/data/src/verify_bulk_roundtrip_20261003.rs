// data/verify-bulk-roundtrip-20261003.mjs — first frozen bulk roundtrip verification.
use baal_paper::fixed::{big_amount, TICK};
use baal_service_runtime::atomic::atomic_json_canonical;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::js_stringify;
use num_bigint::BigInt;
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
fn sha(b: &[u8]) -> String { baal_service_runtime::json_text::sha256(b) }
fn digest(v: &V) -> String { baal_collector::model::digest(v) }
fn instant(v: &V) -> f64 {
    v.as_str().and_then(parse_iso_ms).unwrap_or_else(|| v.as_f64().unwrap_or(f64::NAN))
}

pub fn run(root: &Path) -> Result<V, String> {
    let rt_dir = rt();
    let active = read(&rt_dir.join("bulk-dip-active.json"))?;
    let dir = rt_dir.join("bulk-dip-cohorts").join(active["cohort"].as_str().unwrap_or(""));
    let bank = read(&dir.join("bank.json"))?;
    let stats = read(&dir.join("stats.json"))?;
    need(digest(&bank) == active["bankHash"].as_str().unwrap_or("")
        && stats["bankHash"] == active["bankHash"] && stats["failed"] == json!(false),
        "BANK_OR_REPORT")?;
    for (file, sha_v) in bank["codeIdentity"].as_object().cloned().unwrap_or_default() {
        need(sha(&std::fs::read(root.join("live-paper").join(&file)).map_err(|e| e.to_string())?)
            == sha_v.as_str().unwrap_or(""), "FROZEN_CODE")?;
    }
    let mut sequence = 0i64;
    let mut previous = digest(&bank);
    let mut checkpoint: Option<V> = None;
    let journal = std::fs::read(dir.join("journal.jsonl")).map_err(|e| e.to_string())?;
    need(journal.last() == Some(&10), "PARTIAL_JOURNAL")?;
    for line in String::from_utf8_lossy(&journal).trim_end().split('\n') {
        let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
        let hash = row["hash"].as_str().unwrap_or("").to_string();
        let mut body = row.clone();
        if let Some(o) = body.as_object_mut() { o.remove("hash"); }
        sequence += 1;
        need(body["sequence"].as_i64() == Some(sequence)
            && body["previous"].as_str().unwrap_or("") == previous
            && body["bankHash"].as_str().unwrap_or("") == active["bankHash"].as_str().unwrap_or("")
            && digest(&body) == hash, "JOURNAL_CHAIN")?;
        previous = hash;
        if sequence == stats["journalSequence"].as_i64().unwrap_or(-1) { checkpoint = Some(row); }
    }
    need(checkpoint.as_ref().map(|c| c["hash"].as_str().unwrap_or("")) == Some(stats["journalHash"].as_str().unwrap_or(""))
        && checkpoint.as_ref().map(|c| &c["stateHash"]).unwrap_or(&V::Null) == &stats["stateHash"],
        "REPORT_CHECKPOINT")?;
    let books: Vec<V> = String::from_utf8_lossy(
        &std::fs::read(rt_dir.join("public-books.jsonl")).map_err(|e| e.to_string())?)
        .trim_end().split('\n').map(|l| serde_json::from_str(l).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    let mut wallets = serde_json::Map::new();
    for (latency, w) in stats["wallets"].as_object().cloned().unwrap_or_default() {
        need(w["recentFills"].as_array().map(|a| a.len()).unwrap_or(0) == 2
            && w["closedTrades"].as_i64() == Some(1)
            && w["positions"].as_array().map(|a| a.is_empty()).unwrap_or(false)
            && w["pending"].as_array().map(|a| a.is_empty()).unwrap_or(false)
            && w["fees"].as_str() == Some("0.000000")
            && w["realizedPnl"].as_str() == Some("-0.015000"), "ROUNDTRIP_REPORT")?;
        let mut sources: Vec<V> = Vec::new();
        for fill in w["recentFills"].as_array().cloned().unwrap_or_default() {
            let row = books.iter().find(|x| x["kind"].as_str() == Some("book")
                && x["payload"]["book"]["versionKey"] == fill["sourceVersion"]
                && x["payload"]["book"]["receivedAt"] == fill["at"])
                .cloned();
            let row = match row { Some(r) => r, None => return Err("FILL_SOURCE".into()) };
            let b = &row["payload"]["book"];
            let level = if fill["action"].as_str() == Some("buy") {
                b["asks"][0].clone() } else { b["bids"][0].clone() };
            let tick = BigInt::from(TICK);
            let modeled = big_amount(level["price"].as_str().unwrap_or(""))?
                + if fill["action"].as_str() == Some("buy") { tick } else { -tick };
            need(modeled == big_amount(fill["price"].as_str().unwrap_or(""))?
                && fill["quantity"].as_i64() == Some(1)
                && (level["quantity"].as_f64().unwrap_or(f64::NAN) / 100.0).floor()
                    >= fill["quantity"].as_f64().unwrap_or(f64::NAN)
                && fill["fee"].as_str() == Some("0.000000")
                && fill["slippageTicks"].as_i64() == Some(1)
                && fill["depthFractionBps"].as_i64() == Some(100), "FILL_MODEL")?;
            sources.push(json!({"action": fill["action"], "sequence": row["sequence"],
                "hash": row["hash"], "versionKey": b["versionKey"],
                "requestStartedAt": b["requestStartedAt"], "receivedAt": b["receivedAt"],
                "availableAt": row["at"], "level": level}));
        }
        need(instant(&sources[0]["requestStartedAt"])
                >= instant(&stats["signals"][0]["actualAvailableAt"])
                    + latency.parse::<f64>().map_err(|e| e.to_string())?
            && instant(&sources[1]["requestStartedAt"]) > instant(&sources[0]["receivedAt"]),
            "POST_ADMISSION")?;
        wallets.insert(latency, json!({"closedTrades": w["closedTrades"],
            "realizedPnl": w["realizedPnl"], "fees": w["fees"], "fills": w["recentFills"],
            "sources": sources}));
    }
    let result = json!({"at": ms_to_iso(std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)),
        "statsAt": stats["updatedAt"], "mode": "first_frozen_bulk_prospective_paper_roundtrip",
        "bankHash": active["bankHash"], "cutoff": bank["cutoff"],
        "journalSequence": stats["journalSequence"], "journalHash": stats["journalHash"],
        "journalChainVerified": true, "frozenCodeVerified": true,
        "wallets": V::Object(wallets), "independentOpportunities": 1, "networkRequests": 0,
        "competitionOrders": 0, "profitabilityEstablished": false,
        "limitations": ["Two latency wallets simulate one correlated opportunity.",
            "Counterfactual next-snapshot taker fills with one-percent depth, one adverse tick per side and zero fees; no contest orders."]});
    atomic_json_canonical(&root.join("data/bulk-dip-first-roundtrip-20261003-v2.json"), &result)
        .map_err(|e| e.to_string())?;
    println!("{}", js_stringify(&json!({"at": result["at"], "bankHash": active["bankHash"],
        "journalChainVerified": true, "independentOpportunities": 1, "walletPnl": "-0.015000"})));
    Ok(result)
}
