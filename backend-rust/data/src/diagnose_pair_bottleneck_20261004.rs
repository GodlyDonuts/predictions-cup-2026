// data/diagnose-pair-bottleneck-20261004.mjs — replay the active pair journal
// counting why each book event did not produce an eligible anchor.
use baal_collector::baal_pair_paper_v2::advance_baal_pair;
use baal_collector::probability_receipt::probability_account_gate;
use baal_service_runtime::journal::{journal_lines, JournalReadOptions};
use baal_service_runtime::json_text::{js_stringify, sha256};
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

fn sha(b: &[u8]) -> String { sha256(b) }
fn ph(v: &V) -> Result<String, String> { baal_execution::core::payload_hash(v) }
fn read_json(p: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn localappdata() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).ok_or_else(|| "LOCALAPPDATA".to_string())
}
fn now_iso() -> String {
    baal_service_runtime::contract::ms_to_iso(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as f64)
}
fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(format!("PAIR_DIAG_{}", code)) }
}
fn parse_iso(s: &str) -> f64 {
    baal_service_runtime::contract::parse_iso_ms(s).unwrap_or(f64::NAN)
}
fn inc(m: &mut serde_json::Map<String, V>, k: &str) {
    let n = m.get(k).and_then(|v| v.as_i64()).unwrap_or(0);
    m.insert(k.to_string(), json!(n + 1));
}

pub fn run() -> Result<V, String> {
    let r = localappdata()?.join("SigTradingCup/paper-runtime");
    let active = read_json(&r.join("baal-pair-active.json"))?;
    let dir = r.join("baal-pair-cohorts").join(active["cohort"].as_str().unwrap_or(""));
    let bank = read_json(&dir.join("bank.json"))?;
    let file = dir.join("journal.jsonl");
    let end_byte = std::fs::metadata(&file).map_err(|e| e.to_string())?.len();
    need(ph(&bank)? == active["bankHash"].as_str().unwrap_or(""), "BANK")?;
    let mut hash = Sha256::new();
    let mut counts = serde_json::Map::new();
    let mut examples: Vec<V> = Vec::new();
    let mut reasons = serde_json::Map::new();
    let mut sequence = 0i64;
    let mut previous = String::new();
    let mut state = V::Null;
    let mut first_at = V::Null;
    let mut last_at = V::Null;
    let mut best: Option<num_bigint::BigInt> = None;
    for line in journal_lines(&file, &JournalReadOptions { end_byte: Some(end_byte), ..Default::default() })? {
        if line.is_empty() { continue; }
        let row: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        let mut body = row.clone();
        let record_hash = row["hash"].as_str().unwrap_or("").to_string();
        if let V::Object(m) = &mut body { m.shift_remove("hash"); }
        sequence += 1;
        need(body["sequence"].as_i64() == Some(sequence)
            && body["previous"].as_str().unwrap_or("") == previous
            && sha(js_stringify(&body).as_bytes()) == record_hash, "CHAIN")?;
        hash.update(line.as_bytes());
        hash.update(b"\n");
        previous = record_hash;
        let event = row["event"].clone();
        if event["kind"].as_str() == Some("genesis") {
            state = row["state"]["paper"].clone();
            first_at = state["lastAt"].clone();
            continue;
        }
        state = advance_baal_pair(&state, &event, &bank)?;
        need(ph(&state)? == ph(&row["state"]["paper"])?, "EXACT_REPLAY")?;
        last_at = event["availableAt"].clone();
        if event["kind"].as_str() != Some("book") { inc(&mut counts, "boundary"); continue; }
        inc(&mut counts, "book");
        let at = parse_iso(event["availableAt"].as_str().unwrap_or(""));
        let books: Vec<V> = bank["review"]["legs"].as_array().cloned().unwrap_or_default()
            .iter().map(|l| state["books"][l["exchangeId"].as_str().unwrap_or("")].clone())
            .collect();
        let admission = event["admission"]["reason"].as_str().unwrap_or("absent").to_string();
        inc(&mut reasons, &admission);
        if books.iter().any(|b| b.is_null()) { inc(&mut counts, "missing_cached_pair"); continue; }
        let account_gate: Vec<Option<String>> = books.iter()
            .map(|b| probability_account_gate(&event["accountEligibility"], b,
                event["availableAt"].as_str().unwrap_or("")))
            .collect();
        if account_gate.iter().any(|g| g.is_some()) { inc(&mut counts, "account_gate"); continue; }
        if books.iter().any(|b| at - parse_iso(b["requestStartedAt"].as_str().unwrap_or(""))
                > bank["policy"]["maxAgeMs"].as_f64().unwrap_or(f64::NAN)) {
            inc(&mut counts, "stale_pair"); continue;
        }
        let skew = books.iter().map(|b| parse_iso(b["receivedAt"].as_str().unwrap_or("")))
            .fold(f64::NEG_INFINITY, f64::max)
            - books.iter().map(|b| parse_iso(b["requestStartedAt"].as_str().unwrap_or("")))
                .fold(f64::INFINITY, f64::min);
        if skew > bank["policy"]["maxSkewMs"].as_f64().unwrap_or(f64::NAN) {
            inc(&mut counts, "pair_window_skew"); continue;
        }
        inc(&mut counts, "fresh_pair");
        if !baal_service_runtime::json_text::js_truthy(
                Some(&event["admission"]["positiveConditionalCapacity"])) {
            inc(&mut counts, "capacity_not_positive"); continue;
        }
        inc(&mut counts, "positive_capacity_fresh_pair");
        let depth_fraction = bank["policy"]["depthFractionBps"].as_f64().unwrap_or(0.0);
        let levels: Vec<Option<V>> = books.iter().map(|b| {
            b["bids"].as_array().cloned().unwrap_or_default().into_iter()
                .find(|l| (l["quantity"].as_f64().unwrap_or(0.0) * depth_fraction / 10000.0)
                    .floor() >= 1.0)
        }).collect();
        if levels.iter().any(|l| l.is_none()) { inc(&mut counts, "no_whole_share_depth"); continue; }
        let scale = num_bigint::BigInt::from(baal_paper::fixed::SCALE);
        let tick = num_bigint::BigInt::from(baal_paper::fixed::TICK);
        let mut sum = num_bigint::BigInt::from(0);
        for l in &levels {
            let l = l.clone().unwrap();
            sum += &scale - baal_paper::fixed::big_amount(
                l["price"].as_str().unwrap_or(""))? + &tick;
        }
        if best.is_none() || sum < *best.as_ref().unwrap() { best = Some(sum.clone()); }
        let minimum = baal_paper::fixed::big_amount(
            bank["policy"]["minimumPackageMargin"].as_str().unwrap_or(""))?;
        if sum > &scale - &minimum { inc(&mut counts, "package_cost_gate"); continue; }
        inc(&mut counts, "eligible_package_cost");
        if state["anchor"].is_null() && state["pending"].is_null() && state["package"].is_null() {
            inc(&mut counts, "eligible_but_no_anchor");
        } else if !state["anchor"].is_null() {
            inc(&mut counts, "anchor_wait");
        } else if !state["pending"].is_null() {
            inc(&mut counts, "pending");
        } else {
            inc(&mut counts, "owned_package");
        }
        if examples.len() < 10 {
            examples.push(json!({"at": event["availableAt"],
                "sourceSequence": event["sourceSequence"], "skewMs": skew,
                "cost": baal_paper::fixed::money(&sum),
                "anchor": if state["anchor"].is_null() { V::Null } else {
                    json!({"at": state["anchor"]["at"],
                        "versions": state["anchor"]["versions"]})},
                "pendingStage": state["pending"]["stage"],
                "versions": books.iter().map(|b| b["versionKey"].clone()).collect::<Vec<_>>()}));
        }
    }
    let report = json!({"at": now_iso(),
        "source": {"cohort": active["cohort"], "bankHash": active["bankHash"],
            "bytes": end_byte, "records": sequence, "hash": format!("{:x}", hash.finalize()),
            "tailHash": previous, "firstAt": first_at, "lastAt": last_at},
        "exactStateReplay": true, "counts": V::Object(counts),
        "admissionReasons": V::Object(reasons),
        "bestEligibleOneShareCost": match &best {
            Some(b) => json!(baal_paper::fixed::money(b)), None => V::Null},
        "examples": examples,
        "ending": {"fills": state["fills"].as_array().map(|a| a.len()).unwrap_or(0),
            "pending": state["pending"]["stage"], "anchor": state["anchor"],
            "counts": state["counts"]},
        "networkRequests": 0, "competitionOrders": 0, "profitabilityEstablished": false});
    baal_service_runtime::atomic::atomic_json_canonical(
        Path::new("data/pair-bottleneck-20261004.json"), &report)?;
    Ok(report)
}
