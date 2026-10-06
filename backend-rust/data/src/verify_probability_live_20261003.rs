// data/verify-probability-live-20261003.mjs — probability live replay proof.
use baal_collector::probability_read_focus::read_probability_read_focus;
use baal_collector::probability_receipt::{
    advance_receipt_probability, create_probability_receipt_state, receipt_probability_report};
use baal_service_runtime::atomic::atomic_json_canonical;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::{canonical_json, js_stringify, sha256};
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
fn digest(v: &V) -> String { baal_collector::model::digest(v) }
fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
}
fn instant(s: &V) -> f64 { s.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN) }
fn pid_alive(pid: i64) -> bool { pid > 0 && Path::new(&format!("/proc/{}", pid)).exists() }

pub fn run(root: &Path) -> Result<V, String> {
    let rt_dir = rt();
    let pids = read(&root.join("data/current-runtime-pids.json"))?;
    let before = read(&root.join("data/probability-preservation-before-20261003.json"))?;
    let freeze = read(&root.join("data/probability-freeze-20261003-v1.json"))?;
    let dir = rt_dir.join("probability-receipt-cohorts")
        .join(freeze["cohort"].as_str().unwrap_or(""));
    let stats = read(&dir.join("stats.json"))?;
    let bank_bytes = std::fs::read(dir.join("bank.json")).map_err(|e| e.to_string())?;
    let bank: V = serde_json::from_slice(&bank_bytes).map_err(|e| e.to_string())?;
    for k in ["collector", "paper", "dashboard", "metrics"] {
        if !pid_alive(pids[k].as_i64().unwrap_or(0)) {
            return Err(format!("process.kill {}", pids[k]));
        }
    }
    need(pids["metrics"] == before["pids"]["metrics"], "METRICS_CHANGED")?;
    need(read(&rt_dir.join("competition-activation.json"))?["enabled"] == json!(false),
        "COMPETITION_ACTIVATED")?;
    need(sha(&bank_bytes) == freeze["bankSha256"].as_str().unwrap_or("")
        && digest(&bank) == freeze["bankHash"].as_str().unwrap_or("")
        && stats["bankHash"] == freeze["bankHash"], "BANK_CHANGED")?;
    let mut preserved: Vec<V> = Vec::new();
    for item in before["banks"].as_array().cloned().unwrap_or_default() {
        let d = rt_dir.join(item["cohort"].as_str().unwrap_or(""));
        let bytes = std::fs::read(d.join("bank.json")).map_err(|e| e.to_string())?;
        let journal = std::fs::read(d.join("journal.jsonl")).map_err(|e| e.to_string())?;
        let b: V = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        let jbytes = item["journalBytes"].as_u64().unwrap_or(0) as usize;
        need(sha(&bytes) == item["bankSha256"].as_str().unwrap_or("")
            && journal.len() >= jbytes
            && sha(&journal[..jbytes]) == item["journalSha256"].as_str().unwrap_or(""),
            "OLD_BANK_OR_JOURNAL_CHANGED")?;
        for (f, h) in b["codeIdentity"].as_object().cloned().unwrap_or_default() {
            let target = if f.starts_with('.') { root.join("live-paper").join(&f) }
                else { root.join(&f) };
            need(sha(&std::fs::read(target).map_err(|e| e.to_string())?)
                == h.as_str().unwrap_or(""), "OLD_FROZEN_CODE_CHANGED")?;
        }
        let mut seq = 0i64;
        let mut prev = digest(&b);
        for line in String::from_utf8_lossy(&journal).trim_end_matches('\n').split('\n') {
            if line.is_empty() { continue; }
            let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
            let hash = row["hash"].as_str().unwrap_or("").to_string();
            let mut body = row;
            if let Some(o) = body.as_object_mut() { o.remove("hash"); }
            seq += 1;
            need(body["sequence"].as_i64() == Some(seq)
                && body["previous"].as_str().unwrap_or("") == prev
                && body["bankHash"].as_str().unwrap_or("") == digest(&b)
                && digest(&body) == hash, "OLD_JOURNAL_CHAIN")?;
            prev = hash;
        }
        preserved.push(json!({"cohort": item["cohort"], "bankUnchanged": true,
            "frozenCodeUnchanged": true, "originalJournalPrefixUnchanged": true,
            "completeJournalChainValid": true,
            "retiredFromLiveLoop": !item["cohort"].as_str().unwrap_or("")
                .ends_with("bulk-dip-live-20261003-v1")}));
    }
    for x in before["protocols"].as_array().cloned().unwrap_or_default() {
        need(sha(&std::fs::read(root.join("data").join(x["file"].as_str().unwrap_or("")))
                .map_err(|e| e.to_string())?) == x["sha256"].as_str().unwrap_or(""),
            "FROZEN_PROTOCOL_CHANGED")?;
    }
    for (f, h) in bank["codeIdentity"].as_object().cloned().unwrap_or_default() {
        need(sha(&std::fs::read(root.join("live-paper").join(&f))
                .map_err(|e| e.to_string())?) == h.as_str().unwrap_or(""),
            "PROBABILITY_FROZEN_CODE_CHANGED")?;
    }
    let bytes = std::fs::read(rt_dir.join("public-books.jsonl")).map_err(|e| e.to_string())?;
    let end = bytes.iter().rposition(|b| *b == b'\n').map(|p| p + 1).unwrap_or(0);
    let raw = &bytes[..end];
    let records: Vec<V> = String::from_utf8_lossy(raw).trim_end_matches('\n').split('\n')
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str::<V>(l).map_err(|e| e.to_string()))
        .collect::<Result<Vec<V>, String>>()?;
    let journal_bytes = std::fs::read(dir.join("journal.jsonl")).map_err(|e| e.to_string())?;
    need(journal_bytes.last() == Some(&10), "PARTIAL_PAPER_JOURNAL")?;
    let prefix_n = freeze["originalJournalBytes"].as_u64().unwrap_or(0) as usize;
    need(sha(&journal_bytes[..prefix_n.min(journal_bytes.len())])
        == freeze["originalJournalSha256"].as_str().unwrap_or(""),
        "PROBABILITY_ORIGINAL_JOURNAL_CHANGED")?;
    let journal_limit = stats["journalSequence"].as_i64().unwrap_or(0);
    let journal: Vec<V> = String::from_utf8_lossy(&journal_bytes).trim_end_matches('\n')
        .split('\n').filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str::<V>(l).map_err(|e| e.to_string()))
        .collect::<Result<Vec<V>, String>>()?
        .into_iter()
        .filter(|x| x["sequence"].as_i64().unwrap_or(i64::MAX) <= journal_limit).collect();
    let mut publication = baal_collector::book_publication_v2::create_book_publication_state(
        &bank["universe"])?;
    let mut state = create_probability_receipt_state(&bank)?;
    let mut i = 0usize;
    let mut offset = 0usize;
    while publication["sequence"].as_i64().unwrap_or(0)
        < bank["sourcePrefixSequence"].as_i64().unwrap_or(0) {
        let rec = match records.get(i) { Some(r) => r.clone(), None => return Err("prefix".into()) };
        i += 1;
        publication = baal_collector::book_publication_v2::apply_book_publication(&publication, &rec)?;
        // offset = raw.indexOf('\n', offset) + 1
        offset = raw.iter().skip(offset).position(|b| *b == b'\n')
            .map(|p| offset + p + 1).unwrap_or(raw.len());
    }
    need(offset == bank["sourcePrefixBytes"].as_u64().unwrap_or(0) as usize
        && sha(&raw[..offset]) == bank["sourcePrefixHash"].as_str().unwrap_or("")
        && publication["hash"].as_str().unwrap_or("") == bank["sourcePrefixJournalHash"].as_str().unwrap_or(""),
        "SOURCE_PREFIX")?;
    let mut seq = 0i64;
    let mut prev = digest(&bank);
    for row in &journal {
        let hash = row["hash"].as_str().unwrap_or("").to_string();
        let mut body = row.clone();
        if let Some(o) = body.as_object_mut() { o.remove("hash"); }
        seq += 1;
        need(body["sequence"].as_i64() == Some(seq)
            && body["previous"].as_str().unwrap_or("") == prev
            && body["bankHash"].as_str().unwrap_or("") == digest(&bank)
            && digest(&body) == hash, "PROBABILITY_JOURNAL_CHAIN")?;
        if body["kind"].as_str() == Some("genesis") {
            need(seq == 1 && body["stateHash"].as_str().unwrap_or("") == digest(&state), "GENESIS")?;
        } else {
            need(body["kind"].as_str() == Some("batch")
                && body["sourceSequence"].as_i64().unwrap_or(0)
                    > publication["sequence"].as_i64().unwrap_or(0), "BATCH")?;
            while publication["sequence"].as_i64().unwrap_or(0)
                < body["sourceSequence"].as_i64().unwrap_or(0) {
                let source = match records.get(i) {
                    Some(r) => r.clone(), None => return Err("source".into()) };
                i += 1;
                publication = baal_collector::book_publication_v2::apply_book_publication(
                    &publication, &source)?;
                state = advance_receipt_probability(&state, &source, &bank,
                    body["availableAt"].as_str().unwrap_or(""), &body["accountEligibility"])?;
                offset = raw.iter().skip(offset).position(|b| *b == b'\n')
                    .map(|p| offset + p + 1).unwrap_or(raw.len());
            }
            need(body["sourceBytes"].as_u64().unwrap_or(0) as usize == offset
                && body["sourceHash"] == publication["hash"]
                && body["stateHash"].as_str().unwrap_or("") == digest(&state),
                "EXACT_FORWARD_REPLAY")?;
        }
        prev = hash;
    }
    need(seq == journal_limit && prev == stats["journalHash"].as_str().unwrap_or("")
        && digest(&state) == stats["stateHash"].as_str().unwrap_or("")
        && publication["sequence"] == stats["sourceSequence"], "LIVE_CHECKPOINT")?;
    let replay = receipt_probability_report(&state, &bank,
        stats["updatedAt"].as_str().unwrap_or(""));
    need(canonical_json(&replay["wallets"])? == canonical_json(&stats["wallets"])?,
        "WALLET_REPLAY")?;
    let current = read(&dir.join("stats.json"))?;
    let runtime: V = serde_json::from_str(
        &reqwest::blocking::get("http://127.0.0.1:8787/api/stats")
            .map_err(|e| e.to_string())?.text().map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let runtime = runtime["runtime"].clone();
    let budget = runtime["networkBudget"].clone();
    need(runtime["pid"] == pids["collector"] && runtime["status"].as_str() == Some("running")
        && runtime["cycles"].as_i64().unwrap_or(0) > 0
        && budget["schemaVersion"].as_i64() == Some(3)
        && budget["maxReadsPerMinute"].as_i64() == Some(20)
        && budget["executionOwner"].as_str() == Some("Daenerys")
        && budget["writesEnabled"] == json!(false)
        && budget["completionPacing"] == json!(true), "COLLECTOR_HEALTH_OR_BUDGET")?;
    need(current["failed"] == json!(false) && current["sourceClosed"] == json!(false)
        && now_ms() - instant(&current["updatedAt"]) < 20000.0, "PAPER_HEALTH")?;
    let u = baal_collector::breadth::validate_universe(&bank["universe"],
        bank["universe"]["tournamentId"].as_str().unwrap_or(""))?;
    let focus = read_probability_read_focus(&rt_dir, &u, now_ms(),
        "probability-receipt-active.json", root);
    need(focus.as_ref().map(|f| f["executionAuthority"] == json!(false)
            && f["bankHash"] == freeze["bankHash"]).unwrap_or(false), "FOCUS_HEALTH")?;
    let views_resp: V = serde_json::from_str(
        &reqwest::blocking::get("http://127.0.0.1:3000/api/wallets")
            .map_err(|e| e.to_string())?.text().map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let views = views_resp["wallets"].as_array().cloned().unwrap_or_default();
    let probability_views: Vec<V> = views.iter()
        .filter(|x| x["id"].as_str().map(|id| id.starts_with("probability-")).unwrap_or(false))
        .cloned().collect();
    need(probability_views.len() == 2 && probability_views.iter().all(|x| {
            let latency = x["id"].as_str().unwrap_or("")
                .rsplit('-').next().unwrap_or("");
            let w = &current["wallets"][latency];
            x["status"].as_str() == Some("running")
                && x["feeBps"].as_i64() == Some(0)
                && x["trades"].as_array().map(|a| a.len())
                    == w["recentFills"].as_array().map(|a| a.len())
                && x["positions"].as_array().map(|a| a.len())
                    == w["positions"].as_array().map(|a| a.len())
        }), "DASHBOARD")?;
    let result = json!({"at": ms_to_iso(now_ms()),
        "mode": "prospective_probability_live_replay_proof",
        "codeCommit": freeze["codeCommit"], "bankHash": freeze["bankHash"],
        "cutoff": freeze["cutoff"], "pids": pids, "preserved": preserved,
        "protocolsPreserved": before["protocols"].as_array().map(|a| a.len() as i64).unwrap_or(0),
        "exactReplay": {"sourceSequence": publication["sequence"], "journalSequence": seq,
            "journalHash": prev, "stateHash": digest(&state), "walletsIdentical": true,
            "originalJournalPrefixUnchanged": true, "prefixEconomicsSkipped": true},
        "current": {"at": current["updatedAt"], "counts": current["counts"],
            "wallets": V::Object(current["wallets"].as_object().cloned().unwrap_or_default()
                .iter().map(|(latency, w)| (latency.clone(), json!({"cash": w["cash"],
                    "fees": w["fees"], "fills": w["fills"], "closedTrades": w["closedTrades"],
                    "realizedPnl": w["realizedPnl"], "liquidationPnl": w["liquidationPnl"],
                    "positions": w["positions"].as_array().cloned().unwrap_or_default().iter()
                        .map(|x| json!({"instrument": x["instrument"], "quantity": x["quantity"],
                            "cost": x["cost"]})).collect::<Vec<V>>(),
                    "pending": w["pending"].as_array().map(|a| a.len() as i64).unwrap_or(0),
                    "recentFills": w["recentFills"]}))).collect())},
        "collector": {"pid": runtime["pid"], "cycles": runtime["cycles"],
            "lastCycleAt": runtime["lastCycleAt"], "networkBudget": budget,
            "probabilityFocus": focus},
        "dashboard": {"wallets": probability_views.iter().map(|x| json!({"id": x["id"],
                "status": x["status"],
                "trades": x["trades"].as_array().map(|a| a.len() as i64).unwrap_or(0),
                "positions": x["positions"].as_array().map(|a| a.len() as i64).unwrap_or(0)}))
                .collect::<Vec<V>>(),
            "oldBulkWallets": views.iter()
                .filter(|x| x["id"].as_str().map(|id| id.starts_with("bulk-dip-")).unwrap_or(false))
                .map(|x| json!({"id": x["id"], "status": x["status"],
                    "trades": x["trades"].as_array().map(|a| a.len() as i64).unwrap_or(0)}))
                .collect::<Vec<V>>()},
        "extraBackgroundProcesses": 0, "newApiClients": 0, "competitionOrders": 0,
        "competitionActivation": false, "profitabilityEstablished": false,
        "limitations": [
            "Two markets across duplicated latency wallets are correlated opportunities.",
            "Research bands are subjective stress scenarios, not calibrated floors.",
            "Counterfactual fills use sampled independent requests; fill at is receipt time, not an actual exchange confirmation.",
            "Source/account snapshots are not atomic; own-exposure exclusion is conservative sampled evidence.",
            "Immediate liquidation includes adverse ticks and crossed spreads; settlement priors do not promise two-day resale profits."]});
    atomic_json_canonical(&root.join("data/probability-live-verification-20261003.json"), &result)
        .map_err(|e| e.to_string())?;
    println!("{}", js_stringify(&json!({"at": result["at"], "exactReplay": result["exactReplay"],
        "current": result["current"],
        "preservedBanks": result["preserved"].as_array().map(|a| a.len() as i64).unwrap_or(0),
        "dashboard": true, "profitabilityEstablished": false})));
    Ok(result)
}
