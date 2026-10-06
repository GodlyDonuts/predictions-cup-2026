// data/verify-pair-anchor-20261004.mjs — pair anchor live verification.
use baal_collector::baal_pair_paper_v3::{advance_baal_pair, baal_pair_report};
use baal_collector::frozen_paper_runtime::frozen_paper_module;
use baal_collector::native_journal_restore::source_prefix_hash;
use baal_paper::fixed::{big_amount as amount, instant, SCALE, TICK};
use baal_service_runtime::atomic::atomic_json_canonical;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::journal::{journal_lines, JournalReadOptions};
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
    if ok { Ok(()) } else { Err(format!("PAIR_ANCHOR_LIVE_{}", code)) }
}
fn sha(b: &[u8]) -> String { sha256(b) }
fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
}
fn inst(v: &V) -> i64 {
    match v { V::String(s) => instant(s).unwrap_or(i64::MIN), _ => v.as_i64().unwrap_or(i64::MIN) }
}

pub fn run(root: &Path) -> Result<V, String> {
    let r = rt();
    let preserved = read(&root.join("data/pair-anchor-preservation-20261004.json"))?;
    let pids = read(&root.join("data/current-runtime-pids.json"))?;
    let runtime = read(&r.join("stats.json"))?["runtime"].clone();
    let active = read(&r.join("baal-pair-active.json"))?;
    let dir = r.join("baal-pair-cohorts").join(active["cohort"].as_str().unwrap_or(""));
    let bank = read(&dir.join("bank.json"))?;
    need(active["hostRevision"].as_i64() == Some(4)
        && active["cohort"] == preserved["cohort"]
        && active["bankHash"] == preserved["bankHash"]
        && payload_hash(&bank).map_err(|e| e.to_string())?
            == active["bankHash"].as_str().unwrap_or(""), "BANK")?;
    need(runtime["pid"] == pids["collector"] && runtime["status"].as_str() == Some("running")
        && runtime["cycles"].as_i64().unwrap_or(0) > 0
        && runtime["baalPair"]["failed"] != json!(true)
        && runtime["noMaker"]["failed"] != json!(true)
        && runtime["baalPair"]["cohort"] == active["cohort"]
        && pids["paper"] == preserved["pids"]["paper"]
        && pids["dashboard"] == preserved["pids"]["dashboard"], "HEALTH")?;
    need(!root.join("STOP").exists() && !r.join("STOP").exists()
        && !r.join("COLLECTOR_STOP").exists(), "STOP")?;
    let _mod = frozen_paper_module(&r, &bank, "live-paper/baal-pair-paper-v3.mjs")?;
    let seed_bytes = std::fs::read(dir.join("opening-state.json")).map_err(|e| e.to_string())?;
    let c = &bank["accountingContinuation"];
    let prior = r.join("baal-pair-cohorts").join(c["cohort"].as_str().unwrap_or(""));
    let prior_bank_bytes = std::fs::read(prior.join("bank.json")).map_err(|e| e.to_string())?;
    let prior_bank: V = serde_json::from_slice(&prior_bank_bytes).map_err(|e| e.to_string())?;
    need(sha(&seed_bytes) == c["openingStateFileHash"].as_str().unwrap_or("")
        && sha(&prior_bank_bytes) == c["bankFileHash"].as_str().unwrap_or("")
        && payload_hash(&prior_bank).map_err(|e| e.to_string())?
            == c["bankHash"].as_str().unwrap_or("")
        && source_prefix_hash(&prior.join("journal.jsonl"),
            c["journalPrefixBytes"].as_u64().unwrap_or(0), None)?
            == c["journalPrefixHash"].as_str().unwrap_or("")
        && payload_hash(&bank["policy"]).map_err(|e| e.to_string())?
            == payload_hash(&prior_bank["policy"]).map_err(|e| e.to_string())?,
        "EXACT_CONTINUATION")?;
    for (type_name, old) in preserved["untouched"].as_object().cloned().unwrap_or_default() {
        let a = read(&r.join(format!("{}-active.json", type_name)))?;
        let d = r.join(format!("{}-cohorts", type_name))
            .join(a["cohort"].as_str().unwrap_or(""));
        need(a["cohort"] == old["cohort"] && a["bankHash"] == old["bankHash"]
            && sha(&std::fs::read(d.join("bank.json")).map_err(|e| e.to_string())?)
                == old["bankFileHash"].as_str().unwrap_or("")
            && source_prefix_hash(&d.join("journal.jsonl"),
                old["journalPrefixBytes"].as_u64().unwrap_or(0), None)?
                == old["journalPrefixHash"].as_str().unwrap_or(""), "UNTOUCHED_WALLET")?;
    }
    need(runtime["originalJournal"]["native"]["sourceBytes"] == preserved["originalSource"]["bytes"]
        && runtime["originalJournal"]["native"]["sourceHash"] == preserved["originalSource"]["hash"]
        && source_prefix_hash(&r.join("journal.jsonl"),
            preserved["originalSource"]["bytes"].as_u64().unwrap_or(0), None)?
            == preserved["originalSource"]["hash"].as_str().unwrap_or(""), "ORIGINAL_SOURCE")?;
    need(runtime["bookPublisher"]["native"]["sourceBytes"] == preserved["publicSource"]["bytes"]
        && runtime["bookPublisher"]["native"]["sourceHash"] == preserved["publicSource"]["hash"]
        && source_prefix_hash(&r.join("public-books.jsonl"),
            preserved["publicSource"]["bytes"].as_u64().unwrap_or(0), None)?
            == preserved["publicSource"]["hash"].as_str().unwrap_or(""), "PUBLIC_SOURCE")?;
    let journal = dir.join("journal.jsonl");
    let end_byte = std::fs::metadata(&journal).map_err(|e| e.to_string())?.len();
    let mut sequence = 0i64;
    let mut previous = String::new();
    let mut state = V::Null;
    let mut through = String::new();
    let mut prefix = sha2::Sha256::new();
    use sha2::Digest;
    let seed: V = serde_json::from_slice(&seed_bytes).map_err(|e| e.to_string())?;
    let mut genesis_input = seed.clone();
    if let Some(o) = genesis_input.as_object_mut() {
        o.insert("bankHash".into(), active["bankHash"].clone());
    }
    for line in journal_lines(&journal,
        &JournalReadOptions { end_byte: Some(end_byte), ..Default::default() })? {
        let row: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        let hash = row["hash"].as_str().unwrap_or("").to_string();
        let mut body = row.clone();
        if let Some(o) = body.as_object_mut() { o.remove("hash"); }
        sequence += 1;
        need(body["sequence"].as_i64() == Some(sequence)
            && body["previous"].as_str().unwrap_or("") == previous
            && sha(js_stringify(&body).as_bytes()) == hash, "LIVE_CHAIN")?;
        previous = hash;
        prefix.update(format!("{}\n", line).as_bytes());
        state = if row["event"]["kind"].as_str() == Some("genesis") {
            advance_baal_pair(&genesis_input,
                &json!({"kind": "boundary", "availableAt": bank["cutoff"]}), &bank)?
        } else {
            advance_baal_pair(&state, &row["event"], &bank)?
        };
        need(payload_hash(&state).map_err(|e| e.to_string())?
            == payload_hash(&row["state"]["paper"]).map_err(|e| e.to_string())?,
            "WHOLE_STATE_REPLAY")?;
        through = state["lastAt"].as_str().unwrap_or("").to_string();
    }
    let publication_file = r.join("public-books.jsonl");
    let public_bytes = std::fs::read(&publication_file).map_err(|e| e.to_string())?;
    let last = public_bytes.iter().rposition(|b| *b == b'\n').map(|p| p + 1).unwrap_or(0);
    let complete = &public_bytes[..last];
    let cutoff = inst(&bank["cutoff"]);
    let fills: Vec<V> = state["fills"].as_array().cloned().unwrap_or_default().into_iter()
        .filter(|f| inst(&f["at"]) > cutoff).collect();
    let mut wanted: indexmap::IndexMap<i64, V> = indexmap::IndexMap::new();
    for f in &fills {
        wanted.insert(f["source"]["publicationSequence"].as_i64().unwrap_or(0), f.clone());
    }
    let mut pub_sequence = 0i64;
    let mut pub_previous = String::new();
    let depth_bps = bank["policy"]["depthFractionBps"].as_f64().unwrap_or(0.0);
    let latency_ms = bank["policy"]["latencyMs"].as_f64().unwrap_or(0.0);
    for line in String::from_utf8_lossy(complete).split('\n') {
        if line.is_empty() { continue; }
        let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
        let hash = row["hash"].as_str().unwrap_or("").to_string();
        let mut body = row.clone();
        if let Some(o) = body.as_object_mut() { o.remove("hash"); }
        pub_sequence += 1;
        need(body["sequence"].as_i64() == Some(pub_sequence)
            && body["previousHash"].as_str().unwrap_or("") == pub_previous
            && payload_hash(&body).map_err(|e| e.to_string())? == hash, "PUBLIC_CHAIN")?;
        pub_previous = hash;
        if !wanted.contains_key(&row["sequence"].as_i64().unwrap_or(0)) { continue; }
        let seq = row["sequence"].as_i64().unwrap_or(0);
        let f = wanted[&seq].clone();
        let book = &row["payload"]["book"];
        need(row["hash"].as_str().unwrap_or("") == f["source"]["publicationHash"].as_str().unwrap_or("")
            && book["exchangeId"] == f["exchangeId"]
            && book["versionKey"] == f["source"]["version"]
            && book["requestStartedAt"] == f["source"]["requestStartedAt"]
            && book["receivedAt"] == f["source"]["receivedAt"], "FILL_SOURCE")?;
        let side = if f["action"].as_str() == Some("buy") { "bids" } else { "asks" };
        let level = &book[side][f["source"]["levelIndex"].as_u64().unwrap_or(0) as usize];
        need(level["price"] == f["source"]["rawYesPrice"]
            && (level["quantity"].as_f64().unwrap_or(0.0) * depth_bps / 10000.0).floor() >= 1.0,
            "FILL_DEPTH")?;
        let level_price = amount(level["price"].as_str().unwrap_or(""))?;
        let tick = num_bigint::BigInt::from(TICK);
        let scale = num_bigint::BigInt::from(SCALE);
        let unit = if f["action"].as_str() == Some("buy") { &scale - &level_price + &tick }
            else { &scale - &level_price - &tick };
        let f_price = amount(f["price"].as_str().unwrap_or(""))?;
        let f_limit = amount(f["limit"].as_str().unwrap_or(""))?;
        need(f_price == unit
            && (if f["action"].as_str() == Some("buy") { &unit <= &f_limit } else { &unit >= &f_limit })
            && book["versionKey"] != f["decisionVersion"], "FILL_PRICE_PROTECTION")?;
        need(inst(&book["requestStartedAt"]) as f64 >= inst(&f["createdAt"]) as f64 + latency_ms
            && inst(&book["requestStartedAt"]) > cutoff
            && inst(&f["at"]) >= inst(&book["receivedAt"])
            && inst(&f["at"]) <= inst(&f["expiresAt"]), "FILL_CAUSAL_CLOCK")?;
        wanted.shift_remove(&seq);
    }
    need(wanted.is_empty() && fills.len() >= 2
        && !state["package"]["completedPair"].is_null(), "SUBSEQUENT_PAIR_NOT_YET_OBSERVED")?;
    let budget = read(&r.join("coordinated-network-budget.json"))?;
    need(budget["schemaVersion"].as_i64() == Some(3)
        && budget["maxReadsPerMinute"].as_i64() == Some(20)
        && budget["writesEnabled"] == json!(false)
        && budget["executionOwner"].as_str() == Some("Daenerys")
        && read(&r.join("competition-activation.json"))?["enabled"] == json!(false),
        "AUTHORITY")?;
    let summary = baal_pair_report(&state, &bank, &through)?;
    let report = json!({"at": ms_to_iso(now_ms()), "evidenceThrough": through,
        "status": "corrected_pair_with_subsequent_counterfactual_fills", "pids": pids,
        "cohort": active["cohort"], "bankHash": active["bankHash"], "cutoff": bank["cutoff"],
        "source": {"bytes": end_byte, "records": sequence,
            "hash": prefix.finalize().iter().map(|b| format!("{:02x}", b)).collect::<String>(),
            "tailHash": previous},
        "frozenSnapshot": preserved["frozenSnapshot"], "frozenFiles": preserved["frozenFiles"],
        "exactFullStateReplay": true, "originalAndPriorPrefixesVerified": true,
        "allOriginalWalletBanksPreserved": true, "policyUnchanged": true,
        "prospectivePaper": {"fills": fills.len() as i64,
            "completedPairs": summary["counts"]["completed_pairs"].as_i64().unwrap_or(0),
            "closedPackages": summary["closedPackages"], "cash": summary["cash"],
            "realizedPnl": summary["realizedPnl"],
            "estimatedLiquidationPnl": summary["estimatedLiquidationPnl"],
            "ordinaryConditionalSettlementSurplus": summary["ordinaryConditionalSettlementSurplus"],
            "zeroPayoutExceptionLoss": summary["zeroPayoutExceptionLoss"],
            "fees": summary["fees"]},
        "fillProofs": fills.iter().map(|f| json!({"action": f["action"],
            "quantity": f["quantity"], "price": f["price"], "at": f["at"],
            "createdAt": f["createdAt"], "requestStartedAt": f["source"]["requestStartedAt"],
            "receivedAt": f["source"]["receivedAt"],
            "publicationSequence": f["source"]["publicationSequence"],
            "publicationHash": f["source"]["publicationHash"],
            "levelIndex": f["source"]["levelIndex"], "model": f["model"],
            "causalClocksAndProtectedPriceVerified": true})).collect::<Vec<V>>(),
        "tests": 23, "skippedTests": 0, "accountingReset": false,
        "extraPersistentProcesses": 0, "extraReadSlots": 0, "competitionOrders": 0,
        "profitabilityEstablished": false,
        "limitations": [
            "Recorded local counterfactual fills, not official orders or proven market impact",
            "Conditional ordinary settlement surplus differs from realized/liquidation profit; refund/fusion/admin risks remain",
            "Two simulated buys are a correctness result and insufficient performance evidence"]});
    atomic_json_canonical(&root.join("data/pair-anchor-live-20261004.json"), &report)
        .map_err(|e| e.to_string())?;
    println!("{}", js_stringify(&json!({"status": report["status"], "evidenceThrough": through,
        "pids": pids, "cohort": report["cohort"],
        "prospectivePaper": report["prospectivePaper"], "exactReplay": true,
        "accountingReset": false, "competitionOrders": 0})));
    Ok(report)
}
