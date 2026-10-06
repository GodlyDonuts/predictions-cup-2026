// data/verify-nh-paper-v5-20261004.mjs — NH paper v5 live verification.
use baal_paper::fixed::{big_amount as amount, money};
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::journal::{journal_lines, JournalReadOptions};
use baal_service_runtime::json_text::{js_stringify, sha256};
use baal_service_runtime::ledger::payload_hash;
use num_bigint::BigInt;
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
    if ok { Ok(()) } else { Err(format!("NH_LIVE_{}", code)) }
}
fn sha(b: &[u8]) -> String { sha256(b) }
fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
}
fn prefix_hash(file: &Path, end: u64) -> Result<String, String> {
    let mut fd = std::fs::File::open(file).map_err(|e| e.to_string())?;
    let mut bytes = 0u64;
    let mut h = sha2::Sha256::new();
    use sha2::Digest;
    let mut buf = vec![0u8; 65536];
    while bytes < end {
        let chunk = std::cmp::min(end - bytes, buf.len() as u64) as usize;
        let n = fd.read(&mut buf[..chunk])
            .map_err(|e| e.to_string())?;
        if n == 0 { break; }
        h.update(&buf[..n]);
        bytes += n as u64;
    }
    need(bytes == end, "PREFIX_TRUNCATED")?;
    Ok(h.finalize().iter().map(|b| format!("{:02x}", b)).collect())
}
fn bound_state(file: &Path, stats: &V) -> Result<V, String> {
    let mut previous = String::new();
    let mut sequence = 0i64;
    let mut matched = V::Null;
    for line in journal_lines(file, &JournalReadOptions::default())? {
        let row: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        let hash = row["hash"].as_str().unwrap_or("").to_string();
        let mut body = row.clone();
        if let Some(o) = body.as_object_mut() { o.remove("hash"); }
        sequence += 1;
        need(row["sequence"].as_i64() == Some(sequence)
            && row["previous"].as_str().unwrap_or("") == previous
            && sha(js_stringify(&body).as_bytes()) == hash, "CHAIN")?;
        previous = hash.clone();
        if row["sequence"].as_i64() == stats["journalSequence"].as_i64() {
            need(hash == stats["journalHash"].as_str().unwrap_or(""), "PUBLISHED_TAIL")?;
            matched = row["state"].clone();
            break;
        }
    }
    need(!matched.is_null(), "PUBLISHED_TRUNCATED")?;
    Ok(matched)
}
fn parse_bigint(v: &V) -> Result<BigInt, String> {
    match v {
        V::String(s) => s.parse::<BigInt>().map_err(|e| e.to_string()),
        V::Number(n) => Ok(BigInt::from(n.as_i64().unwrap_or(0))),
        _ => Err("bigint".into()),
    }
}

pub fn run(root: &Path) -> Result<V, String> {
    let r = rt();
    let preservation = read(&root.join("data/nh-paper-v5-preservation-20261004.json"))?;
    let budget = read(&r.join("coordinated-network-budget.json"))?;
    need(!root.join("STOP").exists() && !r.join("STOP").exists()
        && !r.join("COLLECTOR_STOP").exists(), "STOP")?;
    need(budget["maxReadsPerMinute"].as_i64() == Some(20)
        && budget["writesEnabled"] == json!(false)
        && budget["executionOwner"].as_str() == Some("Daenerys")
        && read(&r.join("competition-activation.json"))?["enabled"] == json!(false),
        "AUTHORITY")?;
    let index = read(&r.join("frozen-code/index.json"))?;
    let mut wallets = serde_json::Map::new();
    let mut states = serde_json::Map::new();
    let mut tape_prefix_sequence = 0i64;
    for (type_name, description) in [
        ("no-maker", preservation["maker"].clone()),
        ("baal-pair", preservation["pair"].clone())] {
        let active = read(&r.join(format!("{}-active.json", type_name)))?;
        let dir = r.join(format!("{}-cohorts", type_name))
            .join(active["cohort"].as_str().unwrap_or(""));
        let bank = read(&dir.join("bank.json"))?;
        let stats = read(&dir.join("stats.json"))?;
        need(active["cohort"] == description["cohort"]
            && payload_hash(&bank).map_err(|e| e.to_string())? == active["bankHash"].as_str().unwrap_or("")
            && active["bankHash"] == description["bankHash"]
            && stats["failed"] != json!(true), "ACTIVE")?;
        if type_name == "no-maker" {
            tape_prefix_sequence = bank["tapePrefixSequence"].as_i64().unwrap_or(0);
        }
        let c = &bank["accountingContinuation"];
        let prior = r.join(format!("{}-cohorts", type_name))
            .join(c["cohort"].as_str().unwrap_or(""));
        need(sha(&std::fs::read(prior.join("bank.json")).map_err(|e| e.to_string())?)
                == c["bankFileHash"].as_str().unwrap_or("")
            && prefix_hash(&prior.join("journal.jsonl"),
                c["journalPrefixBytes"].as_u64().unwrap_or(0))?
                == c["journalPrefixHash"].as_str().unwrap_or(""), "PRESERVATION")?;
        need(std::fs::metadata(prior.join("journal.jsonl")).map_err(|e| e.to_string())?.len()
            == c["journalPrefixBytes"].as_u64().unwrap_or(0), "PREDECESSOR_APPENDED")?;
        let binding = index["banks"][payload_hash(&bank).map_err(|e| e.to_string())?.as_str()].clone();
        let froot = r.join("frozen-code").join(binding["snapshot"].as_str().unwrap_or(""));
        let manifest = read(&froot.join("manifest.json"))?;
        need(binding["snapshot"] == preservation["frozenSourceSnapshot"]
            && payload_hash(&manifest["files"]).map_err(|e| e.to_string())?
                == binding["snapshot"].as_str().unwrap_or(""), "SNAPSHOT")?;
        for (file, hash) in manifest["files"].as_object().cloned().unwrap_or_default() {
            need(sha(&std::fs::read(froot.join(&file)).map_err(|e| e.to_string())?)
                == hash.as_str().unwrap_or(""), "FROZEN_FILE")?;
        }
        let state = bound_state(&dir.join("journal.jsonl"), &stats)?;
        states.insert(type_name.to_string(), state.clone());
        wallets.insert(type_name.to_string(), if type_name == "no-maker" {
            V::Object(stats["wallets"].as_object().cloned().unwrap_or_default().iter()
                .map(|(k, w)| (k.clone(), json!({"cash": w["cash"], "reserved": w["reserved"],
                    "fills": w["fills"], "closedTrades": w["closedTrades"],
                    "realizedPnl": w["realizedPnl"],
                    "positions": w["positions"].as_array().map(|a| a.len() as i64).unwrap_or(0),
                    "unresolvedQuotes": w["unresolvedQuotes"].as_array().map(|a| a.len() as i64).unwrap_or(0),
                    "fees": w["fees"]}))).collect())
        } else {
            json!({"cash": stats["cash"], "fills": stats["fills"],
                "realizedPnl": stats["realizedPnl"],
                "positions": state["paper"]["positions"].as_object().map(|m| m.len() as i64).unwrap_or(0)})
        });
    }
    let mut recovered: Vec<V> = Vec::new();
    for line in journal_lines(&r.join("anonymous-trades.jsonl"), &JournalReadOptions::default())? {
        let row: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        let e = &row["event"];
        if row["sequence"].as_i64().unwrap_or(0) <= tape_prefix_sequence
            || e["queryPurpose"].as_str() != Some("expired_paper_quote_coverage_recovery") {
            continue;
        }
        let quote = states["no-maker"]["paper"]["wallets"].as_object()
            .cloned().unwrap_or_default().values()
            .flat_map(|w| w["quotes"].as_object().cloned().unwrap_or_default()
                .values().cloned().collect::<Vec<V>>())
            .find(|q| q["exchangeId"] == e["exchangeId"]
                && q["activationAt"] == e["from"] && q["expiresAt"] == e["to"]);
        let quote = match quote { Some(q) => q, None => continue };
        need(e["coverage"]["complete"] == json!(true)
            && e["pagination"]["hasMore"] != json!(true)
            && parse_bigint(&e["coverage"]["projectedThroughSequence"])?
                >= parse_bigint(&quote["coverageBarrierSequence"])?,
            "RECOVERY_COMPLETENESS")?;
        recovered.push(json!({"sourceSequence": row["sequence"], "sourceHash": row["hash"],
            "marketId": e["marketId"], "exchangeId": e["exchangeId"],
            "from": e["from"], "to": e["to"],
            "requestStartedAt": e["requestStartedAt"], "receivedAt": e["receivedAt"],
            "requiredCheckpoint": quote["coverageBarrierSequence"],
            "projectedThroughSequence": e["coverage"]["projectedThroughSequence"],
            "receivedCount": e["receivedCount"],
            "novelTrades": e["trades"].as_array().map(|a| a.len() as i64).unwrap_or(0),
            "queryPurpose": e["queryPurpose"]}));
    }
    need(recovered.iter().map(|x| x["exchangeId"].as_str().unwrap_or("").to_string())
        .collect::<std::collections::HashSet<_>>().len() == 2, "RECOVERY_NOT_FINISHED")?;
    let old_ids: std::collections::HashSet<i64> = [8i64, 9, 14, 15].into_iter().collect();
    let resolved: Vec<V> = states["no-maker"]["paper"]["wallets"].as_object()
        .cloned().unwrap_or_default().iter()
        .flat_map(|(family, w)| w["quotes"].as_object().cloned().unwrap_or_default()
            .values().cloned().collect::<Vec<V>>().into_iter()
            .filter(|q| q["id"].as_i64().map(|id| old_ids.contains(&id)).unwrap_or(false))
            .map(move |q| json!({"family": family, "id": q["id"], "marketId": q["marketId"],
                "resolvedAt": q["resolvedAt"], "resolution": q["resolution"],
                "checkpoint": q["coverageBarrierSequence"]}))
            .collect::<Vec<V>>())
        .collect();
    need(resolved.len() == 4 && resolved.iter().all(|q|
            !q["resolvedAt"].is_null()
                && q["resolution"].as_str() == Some("projection_observed_without_qualifying_trade")),
        "STUCK_QUOTE")?;
    let mut completion = V::Null;
    for line in journal_lines(&r.join("no-maker-cohorts")
        .join(preservation["maker"]["cohort"].as_str().unwrap_or(""))
        .join("journal.jsonl"), &JournalReadOptions::default())? {
        let row: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        let paper = &row["state"]["paper"];
        let old: Vec<V> = paper["wallets"].as_object().cloned().unwrap_or_default().values()
            .flat_map(|w| w["quotes"].as_object().cloned().unwrap_or_default()
                .values().cloned().collect::<Vec<V>>())
            .filter(|q| q["id"].as_i64().map(|id| old_ids.contains(&id)).unwrap_or(false))
            .collect();
        if old.len() == 4 && old.iter().all(|q| !q["resolvedAt"].is_null()) {
            let wallet_map: serde_json::Map<String, V> = paper["wallets"].as_object()
                .cloned().unwrap_or_default().iter().map(|(k, w)| {
                    let reserved: BigInt = w["quotes"].as_object().cloned().unwrap_or_default()
                        .values()
                        .filter(|q| q["resolvedAt"].is_null() && q["action"].as_str() == Some("buy"))
                        .map(|q| {
                            let limit = amount(q["limit"].as_str().unwrap_or("0")).unwrap_or_default();
                            let qty = parse_bigint(&q["quantity"]).unwrap_or_default();
                            limit * qty
                        }).sum();
                    (k.clone(), json!({"cash": w["cash"],
                        "fills": w["fills"].as_array().map(|a| a.len() as i64).unwrap_or(0),
                        "realizedPnl": w["realized"],
                        "reserved": money(&reserved)}))
                }).collect();
            completion = json!({"journalSequence": row["sequence"], "journalHash": row["hash"],
                "at": paper["lastAt"], "wallets": V::Object(wallet_map)});
            break;
        }
    }
    need(!completion.is_null(), "RECOVERY_CHECKPOINT")?;
    for (family, w) in completion["wallets"].as_object().cloned().unwrap_or_default() {
        let opening = preservation["maker"]["openingWallets"][family.as_str()].clone();
        need(w["cash"] == opening["cash"] && w["fills"] == opening["fills"]
            && w["realizedPnl"] == opening["realizedPnl"]
            && w["reserved"].as_str() == Some("0.000000"), "RECOVERY_CHANGED_ECONOMICS")?;
    }
    let runtime = read(&r.join("stats.json"))?["runtime"].clone();
    need(runtime["status"].as_str() == Some("running")
        && runtime["cycles"].as_i64().unwrap_or(0) > 0
        && runtime["noMaker"]["failed"] != json!(true)
        && runtime["baalPair"]["failed"] != json!(true), "COLLECTOR")?;
    let pids = read(&root.join("data/current-runtime-pids.json"))?;
    let review_bank = read(&r.join("no-maker-cohorts")
        .join(preservation["maker"]["cohort"].as_str().unwrap_or(""))
        .join("bank.json"))?;
    let report = json!({"at": ms_to_iso(now_ms()),
        "cohorts": {"maker": preservation["maker"]["cohort"],
            "pair": preservation["pair"]["cohort"]},
        "pids": pids,
        "collector": {"status": runtime["status"], "cycles": runtime["cycles"],
            "lastCycleAt": runtime["lastCycleAt"]},
        "recovery": {"fixedCheckpoints": true, "resolved": resolved,
            "sourcePages": recovered, "completion": completion,
            "releasedReservationPerWallet": "0.945000", "cashOrFillReset": false},
        "wallets": V::Object(wallets),
        "reviewExpiresAt": review_bank["review"]["expiresAt"],
        "frozenFiles": 40, "tests": 42, "competitionOrders": 0,
        "additionalReadSlots": 0, "additionalPersistentProcesses": 0,
        "profitabilityEstablished": false});
    baal_service_runtime::atomic::atomic_json_canonical(
        &root.join("data/nh-paper-v5-live-20261004.json"), &report)
        .map_err(|e| e.to_string())?;
    println!("{}", js_stringify(&report));
    Ok(report)
}
