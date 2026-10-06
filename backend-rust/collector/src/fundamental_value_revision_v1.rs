// live-paper/fundamental-value-revision-v1.mjs — operator-driven value revision reducer.
use baal_execution::core::payload_hash;
use baal_service_runtime::atomic::atomic_json_canonical;
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::sha256;
use num_bigint::BigInt;
use serde_json::{json, Map, Value as V};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use crate::settlement_paper_host::PaperHost;

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(format!("FUNDAMENTAL_REVISION_{}", code)); }
    Ok(())
}
fn digest(v: &V) -> String { crate::model::digest(v) }
fn instant(v: &V) -> Result<i64, String> { baal_paper::fixed::instant(v.as_str().unwrap_or("")) }
fn amount(v: &V) -> Result<BigInt, String> { baal_paper::fixed::big_amount(v.as_str().unwrap_or("")) }
fn money(v: &BigInt) -> String { baal_paper::fixed::money(v) }
fn tick() -> BigInt { BigInt::from(baal_paper::fixed::TICK) }
fn hex64(v: &V) -> bool { v.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())) }
fn safe_int(v: &V) -> Option<i64> { v.as_i64() }
fn uuid_v4() -> String {
    let mut b = [0u8; 16];
    use std::time::{SystemTime, UNIX_EPOCH};
    let mut seed = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64 ^ std::process::id() as u64;
    for byte in &mut b {
        seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17;
        *byte = seed as u8;
    }
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    format!("{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15])
}
fn source_code_hash(live_paper_dir: &Path) -> Result<String, String> {
    Ok(sha256(&std::fs::read(live_paper_dir.join("fundamental-value-revision-v1.mjs")).map_err(|e| e.to_string())?))
}

/// FUNDAMENTAL_REVISION_POLICY
pub fn fundamental_revision_policy() -> V {
    json!({"mode": "invalidated_forecast_robust_value_reduction", "maximumBookAgeMs": 20000,
        "depthFractionBps": 100, "adverseTicks": 1, "intentLifetimeMs": 180000,
        "fees": "0.000000", "newEntries": false})
}
/// validateFundamentalRevision(r, bank)
pub fn validate_fundamental_revision(r: &V, bank: &V) -> Result<(), String> {
    let market_id = bank["research"][0]["marketId"].as_str().unwrap_or("");
    need(r["schemaVersion"].as_i64() == Some(1) && r["owner"].as_str() == Some("Codex")
        && r["bankHash"].as_str() == Some(digest(bank).as_str())
        && r["cohort"].as_str() == bank["name"].as_str()
        && r["marketId"].as_str() == Some(market_id)
        && r["exchangeId"] == bank["identities"][market_id]["exchangeId"]
        && r["predicateHash"] == bank["identities"][market_id]["predicateHash"], "BINDING")?;
    need(r["policy"].as_str() == Some("invalidated_forecast_robust_value_reduction")
        && r["newEntries"] == V::Bool(false) && r["sourceClockRenewed"] == V::Bool(false), "AUTHORITY")?;
    need(instant(&r["sourceAt"])? <= instant(&r["recordedAt"])?
        && instant(&r["recordedAt"])? < instant(&r["expiresAt"])?, "CLOCK")?;
    need(amount(&r["lower"])? >= BigInt::from(0) && amount(&r["upper"])? <= BigInt::from(1_000_000i64)
        && amount(&r["lower"])? <= amount(&r["central"])? && amount(&r["central"])? <= amount(&r["upper"])?, "VALUES")?;
    need(r["sources"].as_array().is_some_and(|a| !a.is_empty() && a.iter().all(|s| {
        s["url"].as_str().is_some_and(|u| u.starts_with("https://")) && hex64(&s["contentHash"])
    })) && hex64(&r["sourceComparisonHash"]), "SOURCES")?;
    Ok(())
}
/// createFundamentalRevisionState({bank, originalStats, revision, cutoff})
pub fn create_fundamental_revision_state(bank: &V, original_stats: &V, revision: &V, cutoff: &str) -> Result<V, String> {
    validate_fundamental_revision(revision, bank)?;
    need(original_stats["bankHash"].as_str() == Some(digest(bank).as_str())
        && original_stats["failed"] == V::Bool(false)
        && baal_paper::fixed::instant(cutoff)? >= instant(&revision["recordedAt"])?, "OPENING")?;
    let id = revision["marketId"].as_str().unwrap_or("");
    let mut wallets = Map::new();
    for (latency, w) in original_stats["wallets"].as_object().cloned().unwrap_or_default() {
        let lb = latency.as_bytes();
        let lat_ok = !lb.is_empty() && lb[0] != b'0' && lb.iter().all(|c| c.is_ascii_digit());
        need(lat_ok
            && w["fills"].as_i64() == w["recentFills"].as_array().map(|a| a.len() as i64)
            && w["positions"].as_array().is_some_and(|p| p.len() == 1)
            && w["positions"][0]["marketId"].as_str() == Some(id)
            && w["positions"][0]["quantity"].as_f64() == Some(1.0)
            && w["pending"].as_array().is_some_and(|p| p.iter()
                .all(|p| p["action"].as_str() == Some("sell") && p["marketId"].as_str() == Some(id))), "FULL_HELD_HISTORY")?;
        let mut positions = Map::new();
        positions.insert(id.to_string(), json!({"quantity": 1, "cost": w["positions"][0]["cost"],
            "enteredAt": w["positions"][0]["enteredAt"]}));
        wallets.insert(latency, json!({"cash": w["cash"], "realizedPnl": w["realizedPnl"],
            "cumulativeBuys": w["cumulativeBuys"], "positions": positions,
            "fills": w["recentFills"], "pending": V::Null, "consumedVersions": {},
            "intents": 0,
            "discardedPriorSellIntents": w["pending"].as_array().map(|p| p.len()).unwrap_or(0)}));
    }
    let latencies = bank["fillPolicy"]["latenciesMs"].as_array().cloned().unwrap_or_default();
    need(wallets.len() == latencies.len()
        && latencies.iter().all(|ms| {
            let key = ms.as_i64().map(|i| i.to_string()).unwrap_or_else(|| ms.to_string());
            wallets.contains_key(&key)
        }), "LATENCY_SET")?;
    let state = json!({"schemaVersion": 1, "bankHash": digest(bank), "revisionHash": digest(revision),
        "cutoff": cutoff, "lastAvailableAt": cutoff, "barrier": cutoff,
        "sourceBytes": original_stats["sourceBytes"], "sourceSequence": original_stats["sourceSequence"],
        "sourceHash": original_stats["sourceHash"], "wallets": wallets, "counts": {}, "book": V::Null});
    assert_fundamental_revision_state(&state, bank, revision)?;
    Ok(state)
}
/// assertFundamentalRevisionState(s, bank, r)
pub fn assert_fundamental_revision_state(s: &V, bank: &V, r: &V) -> Result<(), String> {
    need(s["bankHash"].as_str() == Some(digest(bank).as_str())
        && s["revisionHash"].as_str() == Some(digest(r).as_str()), "STATE_BINDING")?;
    for (_latency, w) in s["wallets"].as_object().cloned().unwrap_or_default() {
        let mut cash = amount(&bank["fillPolicy"]["initialCash"])?;
        let mut realized = BigInt::from(0);
        let mut buys = BigInt::from(0);
        let mut positions = Map::new();
        for f in w["fills"].as_array().cloned().unwrap_or_default() {
            need(f["marketId"] == r["marketId"] && f["exchangeId"] == r["exchangeId"]
                && f["quantity"].as_f64() == Some(1.0) && f["fee"].as_str() == Some("0.000000"), "FILL_SCOPE")?;
            let v = amount(&f["price"])?;
            let market = f["marketId"].as_str().unwrap_or("").to_string();
            if f["action"].as_str() == Some("buy") {
                need(!positions.contains_key(&market), "DUPLICATE")?;
                positions.insert(market, json!({"quantity": 1, "cost": f["price"], "enteredAt": f["at"]}));
                cash -= &v;
                buys += &v;
            } else {
                need(f["action"].as_str() == Some("sell") && positions.contains_key(&market), "OVERSELL")?;
                cash += &v;
                realized += &v - amount(&positions[&market]["cost"])?;
                positions.shift_remove(&market);
            }
        }
        need(cash == amount(&w["cash"])? && realized == amount(&w["realizedPnl"])?
            && buys == amount(&w["cumulativeBuys"])?
            && digest(&V::Object(positions)) == digest(&w["positions"]), "CONSERVATION")?;
        let pending = &w["pending"];
        need(pending.is_null()
            || (pending["action"].as_str() == Some("sell")
                && pending["marketId"] == r["marketId"]
                && pending["quantity"].as_f64() == Some(1.0)
                && w["positions"].as_object().is_some_and(|p| p.contains_key(r["marketId"].as_str().unwrap_or("")))), "COVERED_INTENT")?;
    }
    Ok(())
}
/// advanceFundamentalRevision(input, event, bank, revision) — returns the next state.
pub fn advance_fundamental_revision(input: &V, event: &V, bank: &V, revision: &V) -> Result<V, String> {
    assert_fundamental_revision_state(input, bank, revision)?;
    let mut s = input.clone();
    let at = event["availableAt"].as_str().unwrap_or("");
    let now = baal_paper::fixed::instant(at)?;
    need(now >= instant(&s["lastAvailableAt"])? && now >= instant(&s["cutoff"])?, "CAUSAL")?;
    s["lastAvailableAt"] = json!(at);
    macro_rules! boundary {
        ($s:expr, $reason:expr) => {{
            let reason: &str = $reason;
            $s["barrier"] = json!(at);
            $s["book"] = V::Null;
            for (_k, w) in $s["wallets"].as_object_mut().map(|m| m.iter_mut().collect::<Vec<_>>()).unwrap_or_default() {
                w["pending"] = V::Null;
            }
            let n = $s["counts"][reason].as_i64().unwrap_or(0);
            $s["counts"][reason] = json!(n + 1);
        }};
    }
    if !event["sourceSequence"].is_null() {
        need(event["sourceSequence"].as_i64().unwrap_or(0) > s["sourceSequence"].as_i64().unwrap_or(0)
            && hex64(&event["sourceHash"]), "SOURCE")?;
        s["sourceSequence"] = event["sourceSequence"].clone();
        s["sourceHash"] = event["sourceHash"].clone();
        if !event["sourceBytes"].is_null() {
            s["sourceBytes"] = event["sourceBytes"].clone();
        }
    }
    if event["kind"].as_str() == Some("boundary") {
        let reason = event["reason"].as_str().unwrap_or("source_boundary");
        boundary!(s, reason);
        assert_fundamental_revision_state(&s, bank, revision)?;
        return Ok(s);
    }
    if event["kind"].as_str() == Some("cursor") {
        assert_fundamental_revision_state(&s, bank, revision)?;
        return Ok(s);
    }
    need(event["kind"].as_str() == Some("book"), "SOURCE_KIND")?;
    let b = &event["book"];
    let market_id = revision["marketId"].as_str().unwrap_or("");
    need(b["marketId"].as_str() == Some(market_id)
        && b["exchangeId"].as_str() == revision["exchangeId"].as_str()
        && b["instrument"].as_str() == Some(format!("{}:YES", revision["exchangeId"].as_str().unwrap_or("")).as_str()), "BOOK_SCOPE")?;
    let request = instant(&b["requestStartedAt"])?;
    let received = instant(&b["receivedAt"])?;
    if request <= instant(&s["barrier"])?
        || received < request || received > now
        || now - request > fundamental_revision_policy()["maximumBookAgeMs"].as_i64().unwrap_or(0)
        || b["freshnessPolicy"].as_str() != Some("authoritative_receipt_v1")
        || crate::probability_receipt::probability_account_gate(&event["accountEligibility"], b, at).is_some()
    {
        boundary!(s, "unavailable_or_noncausal");
        return Ok(s);
    }
    if request < instant(&revision["recordedAt"])? {
        boundary!(s, "revision_not_known");
        return Ok(s);
    }
    let revision_current = now < instant(&revision["expiresAt"])?;
    need(b["bids"].is_array() && b["asks"].is_array()
        && !b["bids"].as_array().unwrap().is_empty()
        && !b["asks"].as_array().unwrap().is_empty()
        && b["versionKey"].is_string(), "BOOK_DEPTH")?;
    let level = b["bids"].as_array().cloned().unwrap_or_default().into_iter().find(|l| {
        safe_int(&l["quantity"]).is_some()
            && (l["quantity"].as_f64().unwrap_or(0.0) * 100.0 / 10000.0).floor() >= 1.0
            && amount(&l["price"]).map(|p| p - tick()).map(|p| p >= tick()).unwrap_or(false)
    });
    let sale: Option<BigInt> = level.map(|l| amount(&l["price"]).map(|p| p - tick())).transpose()?;
    s["book"] = b.clone();
    let wallet_keys: Vec<String> = s["wallets"].as_object().map(|w| w.keys().cloned().collect()).unwrap_or_default();
    for latency in wallet_keys {
        let latency_n = latency.parse::<i64>().unwrap_or(0);
        let w = &mut s["wallets"][&latency];
        if w["positions"][market_id].is_null() {
            w["pending"] = V::Null;
            continue;
        }
        let lower = amount(&revision["lower"])?;
        if !w["pending"].is_null()
            && (now - instant(&w["pending"]["createdAt"])? > fundamental_revision_policy()["intentLifetimeMs"].as_i64().unwrap_or(0)
                || sale.is_none()
                || sale.as_ref().is_some_and(|p| *p < lower))
        {
            w["pending"] = V::Null;
        }
        let pending = w["pending"].clone();
        if !pending.is_null()
            && request >= instant(&pending["createdAt"])? + latency_n
            && request > instant(&pending["createdAt"])?
            && event["sourceSequence"].as_i64().unwrap_or(0) > pending["sourceSequence"].as_i64().unwrap_or(0)
            && b["versionKey"] != pending["sourceVersion"]
            && w["consumedVersions"][market_id] != b["versionKey"]
            && sale.as_ref().is_some_and(|p| *p >= amount(&pending["limit"]).unwrap_or_default())
        {
            let sale_v = sale.clone().unwrap();
            let old = w["positions"][market_id].clone();
            let fill = json!({"intentId": pending["id"], "marketId": market_id,
                "exchangeId": revision["exchangeId"], "instrument": format!("{}:YES", revision["exchangeId"].as_str().unwrap_or("")),
                "action": "sell", "quantity": 1, "price": money(&sale_v), "fee": "0.000000",
                "at": at, "requestStartedAt": b["requestStartedAt"], "receivedAt": b["receivedAt"],
                "sourceVersion": b["versionKey"], "sourceSequence": event["sourceSequence"],
                "sourceHash": event["sourceHash"], "revisionHash": digest(revision),
                "reason": revision["policy"], "model": "later_rest_request_counterfactual"});
            w["cash"] = json!(money(&(amount(&w["cash"])? + &sale_v)));
            w["realizedPnl"] = json!(money(&(amount(&w["realizedPnl"])? + &sale_v - amount(&old["cost"])?)));
            w["fills"].as_array_mut().map(|f| f.push(fill));
            w["positions"].as_object_mut().and_then(|p| p.shift_remove(market_id));
            w["pending"] = V::Null;
            w["consumedVersions"][market_id] = b["versionKey"].clone();
            let n = s["counts"]["held_risk_sale"].as_i64().unwrap_or(0);
            s["counts"]["held_risk_sale"] = json!(n + 1);
        } else if w["pending"].is_null() && revision_current
            && sale.as_ref().is_some_and(|p| *p >= lower)
        {
            let sale_v = sale.clone().unwrap();
            w["intents"] = json!(w["intents"].as_i64().unwrap_or(0) + 1);
            let intents = w["intents"].clone();
            w["pending"] = json!({"id": intents, "marketId": market_id, "action": "sell",
                "quantity": 1, "limit": money(&sale_v), "createdAt": at,
                "sourceSequence": event["sourceSequence"], "sourceVersion": b["versionKey"],
                "sourceHash": event["sourceHash"], "revisionHash": digest(revision)});
        }
    }
    assert_fundamental_revision_state(&s, bank, revision)?;
    Ok(s)
}
/// fundamentalRevisionReport(s, bank, r, at)
pub fn fundamental_revision_report(s: &V, bank: &V, r: &V, at: &str) -> Result<V, String> {
    assert_fundamental_revision_state(s, bank, r)?;
    let mut wallets = Map::new();
    for (latency, w) in s["wallets"].as_object().cloned().unwrap_or_default() {
        let positions: Vec<V> = w["positions"].as_object().cloned().unwrap_or_default().iter()
            .map(|(market_id, p)| {
                let mut o = json!({"marketId": market_id});
                for (k, v) in p.as_object().cloned().unwrap_or_default() { o[k] = v; }
                o
            }).collect();
        let fills = w["fills"].as_array().cloned().unwrap_or_default();
        let closed = fills.iter().filter(|f| f["action"].as_str() == Some("sell")).count();
        let recent = fills.iter().rev().take(25).rev().cloned().collect::<Vec<_>>();
        wallets.insert(latency, json!({"cash": w["cash"], "realizedPnl": w["realizedPnl"],
            "cumulativeBuys": w["cumulativeBuys"], "positions": positions,
            "pending": if w["pending"].is_null() { json!([]) } else { json!([w["pending"]]) },
            "fills": fills.len(), "closedTrades": closed, "recentFills": recent,
            "reservedCost": "0.000000", "newEntries": false}));
    }
    Ok(json!({"mode": "fundamental_held_value_revision_v1", "cohort": bank["name"],
        "bankHash": s["bankHash"], "revisionHash": s["revisionHash"], "updatedAt": at,
        "cutoff": s["cutoff"], "sourceSequence": s["sourceSequence"], "sourceHash": s["sourceHash"],
        "counts": s["counts"], "wallets": wallets, "policy": fundamental_revision_policy(),
        "revision": {"sourceAt": r["sourceAt"], "recordedAt": r["recordedAt"],
            "expiresAt": r["expiresAt"], "lower": r["lower"], "upper": r["upper"],
            "central": r["central"], "sourceComparisonHash": r["sourceComparisonHash"]},
        "executionAuthority": false, "competitionOrders": 0, "profitabilityEstablished": false,
        "limitations": ["An invalidated forecast prompts declared downside-risk reduction, not profit-taking or calibrated probability bounds.",
            "Original purchase costs and earlier cash/FIFO remain attributed; a sale may realize a loss.",
            "The original bank and journal remain immutable; only one reducer may own this inventory.",
            "Expiry neither refreshes sources nor settles inventory. Later receipt fills remain local counterfactuals."]}))
}

/// openFundamentalValueRevision({runtimeDir, originalHost, revision, initialize, now, canOperate})
pub struct FundamentalRevisionOperator {
    pub dir: PathBuf,
    pub bank: V,
    revision: V,
    opening: V,
    file: PathBuf,
    file_hash: String,
    journal: Journal,
    lock_file: PathBuf,
    token: String,
    runtime_dir: PathBuf,
    live_paper_dir: PathBuf,
    source: PathBuf,
    original_journal: PathBuf,
    original_bank_file: PathBuf,
    original_dir: PathBuf,
    state: V,
    operator_journal_hash: String,
    closed: bool,
    failed: bool,
    now: std::sync::Arc<dyn Fn() -> i64 + Send + Sync>,
    can_operate: std::sync::Arc<dyn Fn() -> bool + Send + Sync>,
}
impl FundamentalRevisionOperator {
    /// openFundamentalValueRevision({runtimeDir, originalHost, revision, initialize, now, canOperate})
    /// `live_paper_dir` stands in for import.meta.url (this module's own code hash).
    pub fn open(runtime_dir: &Path, original: &mut dyn PaperHost, revision: &V, initialize: bool,
        now: std::sync::Arc<dyn Fn() -> i64 + Send + Sync>,
        can_operate: std::sync::Arc<dyn Fn() -> bool + Send + Sync>,
        live_paper_dir: &Path) -> Result<Self, String> {
        let dir = original.dir().join("operator-value-revision");
        let file = dir.join("opening.json");
        let journal_file = dir.join("journal.jsonl");
        let lock_file = dir.join("worker.lock");
        if !dir.exists() {
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
        }
        let token = serde_json::to_string(&json!({"pid": std::process::id(), "id": uuid_v4()})).map_err(|e| e.to_string())?;
        let mut opts = std::fs::OpenOptions::new();
        let wrote = opts.write(true).create_new(true).mode(0o600).open(&lock_file);
        match wrote.and_then(|mut f| f.write_all(token.as_bytes())) {
            Ok(()) => {}
            Err(e) => return Err(e.to_string()),
        }
        let bank = original.bank().clone();
        let r = (|| -> Result<Self, String> {
            validate_fundamental_revision(revision, &bank)?;
            let original_journal = original.dir().join("journal.jsonl");
            let original_bank_file = original.dir().join("bank.json");
            let opening: V = if initialize {
                need(!file.exists() && !journal_file.exists(), "EXISTS")?;
                let original_stats = original.status()?;
                let original_journal_bytes = std::fs::read(&original_journal).map_err(|e| e.to_string())?;
                let bank_bytes = std::fs::read(&original_bank_file).map_err(|e| e.to_string())?;
                let raw = std::fs::read(runtime_dir.join("public-books.jsonl")).map_err(|e| e.to_string())?;
                let prefix_end = raw.iter().rposition(|b| *b == b'\n').map(|p| p + 1).unwrap_or(0);
                let prefix = &raw[..prefix_end];
                let tail_start = if prefix.len() >= 2 {
                    prefix[..prefix.len() - 1].iter().rposition(|b| *b == b'\n').map(|p| p + 1).unwrap_or(0)
                } else { 0 };
                let tail: V = serde_json::from_slice(&prefix[tail_start..prefix.len().saturating_sub(1)])
                    .map_err(|e| e.to_string())?;
                let cutoff = baal_service_runtime::contract::ms_to_iso((now)() as f64);
                need(tail["sequence"].as_i64().unwrap_or(0) >= original_stats["sourceSequence"].as_i64().unwrap_or(0)
                    && baal_paper::fixed::instant(tail["at"].as_str().unwrap_or(""))?
                        <= baal_paper::fixed::instant(&cutoff)?
                    && digest(&serde_json::from_slice::<V>(&bank_bytes).map_err(|e| e.to_string())?) == digest(&bank), "SOURCE_OPENING")?;
                let opening = json!({"revision": revision, "originalStats": original_stats,
                    "originalStatsHash": digest(&original_stats),
                    "originalJournalBytes": original_journal_bytes.len(),
                    "originalJournalHash": sha256(&original_journal_bytes),
                    "originalBankHash": digest(&bank), "originalBankFileHash": sha256(&bank_bytes),
                    "cutoff": cutoff, "sourcePrefixBytes": prefix.len(),
                    "sourcePrefixHash": sha256(prefix), "sourcePrefixSequence": tail["sequence"],
                    "sourcePrefixTailHash": tail["hash"],
                    "codeHash": source_code_hash(live_paper_dir)?});
                let mut fd = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&file)
                    .map_err(|e| e.to_string())?;
                fd.write_all((serde_json::to_string_pretty(&opening).map_err(|e| e.to_string())? + "\n").as_bytes())
                    .map_err(|e| e.to_string())?;
                opening
            } else {
                serde_json::from_slice(&std::fs::read(&file).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?
            };
            let file_hash = sha256(&std::fs::read(&file).map_err(|e| e.to_string())?);
            let verify_original = |bank: &V| -> Result<(), String> {
                let b = std::fs::read(&original_journal).map_err(|e| e.to_string())?;
                let bank_bytes = std::fs::read(&original_bank_file).map_err(|e| e.to_string())?;
                need(b.len() as u64 == opening["originalJournalBytes"].as_u64().unwrap_or(0)
                    && sha256(&b) == opening["originalJournalHash"].as_str().unwrap_or("")
                    && sha256(&bank_bytes) == opening["originalBankFileHash"].as_str().unwrap_or("")
                    && digest(&serde_json::from_slice::<V>(&bank_bytes).map_err(|e| e.to_string())?)
                        == opening["originalBankHash"].as_str().unwrap_or("")
                    && digest(bank) == opening["originalBankHash"].as_str().unwrap_or(""), "ORIGINAL_CHANGED")
            };
            verify_original(&bank)?;
            need(digest(&opening["revision"]) == digest(revision)
                && digest(&opening["originalStats"]) == opening["originalStatsHash"].as_str().unwrap_or("")
                && opening["codeHash"].as_str() == Some(&source_code_hash(live_paper_dir)?), "OPENING_CHANGED")?;
            let source = runtime_dir.join("public-books.jsonl");
            let verify_source = |source: &Path| -> Result<(), String> {
                let raw = std::fs::read(source).map_err(|e| e.to_string())?;
                let end = opening["sourcePrefixBytes"].as_u64().unwrap_or(0) as usize;
                need(raw.len() >= end && sha256(&raw[..end]) == opening["sourcePrefixHash"].as_str().unwrap_or(""),
                    "SOURCE_PREFIX")
            };
            verify_source(&source)?;
            let journal = Journal::open(&journal_file)?;
            let mut state = create_fundamental_revision_state(&bank, &opening["originalStats"], revision,
                opening["cutoff"].as_str().unwrap_or(""))?;
            state["sourceBytes"] = opening["sourcePrefixBytes"].clone();
            state["sourceSequence"] = opening["sourcePrefixSequence"].clone();
            state["sourceHash"] = opening["sourcePrefixTailHash"].clone();
            if journal.last.is_some() {
                let raw = std::fs::read_to_string(&journal_file).map_err(|e| e.to_string())?;
                for (i, line) in raw.trim_end().split('\n').enumerate() {
                    let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
                    if i == 0 {
                        need(row["event"]["kind"].as_str() == Some("genesis")
                            && row["event"]["openingHash"].as_str() == Some(file_hash.as_str()), "GENESIS")?;
                    } else {
                        state = advance_fundamental_revision(&state, &row["event"], &bank, revision)?;
                    }
                    need(digest(&row["state"]) == digest(&state), "ECONOMIC_REPLAY")?;
                }
            }
            let operator_journal_hash = if journal_file.exists() {
                sha256(&std::fs::read(&journal_file).map_err(|e| e.to_string())?)
            } else {
                sha256(&[])
            };
            let journal_empty = journal.last.is_none();
            let mut op = Self {
                dir, bank, revision: revision.clone(), opening, file, file_hash, journal,
                lock_file: lock_file.clone(), token: token.clone(), runtime_dir: runtime_dir.to_path_buf(),
                live_paper_dir: live_paper_dir.to_path_buf(), source,
                original_journal, original_bank_file, original_dir: original.dir().to_path_buf(),
                state, operator_journal_hash, closed: false, failed: false, now, can_operate,
            };
            if journal_empty {
                let ev = json!({"kind": "genesis", "openingHash": op.file_hash});
                let next = op.state.clone();
                op.append(&next, &ev, false)?;
            } else {
                let at = baal_service_runtime::contract::ms_to_iso(
                    (op.now)().max(baal_paper::fixed::instant(op.state["lastAvailableAt"].as_str().unwrap_or(""))?) as f64);
                let event = json!({"kind": "boundary", "reason": "operator_restart", "availableAt": at});
                let next = advance_fundamental_revision(&op.state, &event, &op.bank.clone(), &op.revision.clone())?;
                op.append(&next, &event, false)?;
            }
            Ok(op)
        })();
        match r {
            Ok(v) => Ok(v),
            Err(e) => {
                if lock_file.exists()
                    && std::fs::read_to_string(&lock_file).ok().as_deref() == Some(token.as_str())
                {
                    let _ = std::fs::remove_file(&lock_file);
                }
                Err(e)
            }
        }
    }
    fn verify_original(&self) -> Result<(), String> {
        let b = std::fs::read(&self.original_journal).map_err(|e| e.to_string())?;
        let bank_bytes = std::fs::read(&self.original_bank_file).map_err(|e| e.to_string())?;
        need(b.len() as u64 == self.opening["originalJournalBytes"].as_u64().unwrap_or(0)
            && sha256(&b) == self.opening["originalJournalHash"].as_str().unwrap_or("")
            && sha256(&bank_bytes) == self.opening["originalBankFileHash"].as_str().unwrap_or("")
            && digest(&serde_json::from_slice::<V>(&bank_bytes).map_err(|e| e.to_string())?)
                == self.opening["originalBankHash"].as_str().unwrap_or("")
            && digest(&self.bank) == self.opening["originalBankHash"].as_str().unwrap_or(""), "ORIGINAL_CHANGED")
    }
    fn verify_source(&self) -> Result<(), String> {
        let raw = std::fs::read(&self.source).map_err(|e| e.to_string())?;
        let end = self.opening["sourcePrefixBytes"].as_u64().unwrap_or(0) as usize;
        need(raw.len() >= end && sha256(&raw[..end]) == self.opening["sourcePrefixHash"].as_str().unwrap_or(""),
            "SOURCE_PREFIX")
    }
    fn live(&self, allow_stopped: bool) -> Result<(), String> {
        need(!self.closed && !self.failed
            && (allow_stopped || ((self.can_operate)() && !self.runtime_dir.join("STOP").exists()))
            && std::fs::read_to_string(&self.lock_file).map_err(|e| e.to_string())? == self.token
            && sha256(&std::fs::read(&self.file).map_err(|e| e.to_string())?) == self.file_hash
            && self.opening["codeHash"].as_str() == Some(source_code_hash(&self.live_paper_dir)?.as_str()), "OWNERSHIP")?;
        self.verify_original()?;
        self.verify_source()
    }
    /// status()
    pub fn status(&self) -> Result<V, String> {
        let mut out = fundamental_revision_report(&self.state, &self.bank, &self.revision,
            &baal_service_runtime::contract::ms_to_iso((self.now)() as f64))?;
        out["originalJournalHash"] = self.opening["originalJournalHash"].clone();
        out["originalJournalBytes"] = self.opening["originalJournalBytes"].clone();
        out["originalBankFileHash"] = self.opening["originalBankFileHash"].clone();
        out["sourcePrefixHash"] = self.opening["sourcePrefixHash"].clone();
        out["sourcePrefixBytes"] = self.opening["sourcePrefixBytes"].clone();
        out["operatorCodeHash"] = self.opening["codeHash"].clone();
        out["journalSequence"] = json!(self.journal.sequence);
        out["journalHash"] = json!(self.journal.previous);
        out["failed"] = json!(self.failed);
        Ok(out)
    }
    /// publish()
    pub fn publish(&self) -> Result<(), String> {
        atomic_json_canonical(&self.dir.join("stats.json"), &self.status()?)
    }
    fn append(&mut self, next: &V, event: &V, allow_stopped: bool) -> Result<(), String> {
        self.live(allow_stopped)?;
        let r = (|| -> Result<(), String> {
            let current = if self.journal.file.exists() {
                sha256(&std::fs::read(&self.journal.file).map_err(|e| e.to_string())?)
            } else {
                sha256(&[])
            };
            need(current == self.operator_journal_hash, "OPERATOR_JOURNAL_CHANGED")?;
            self.journal.commit(next, event)?;
            self.operator_journal_hash = sha256(&std::fs::read(&self.journal.file).map_err(|e| e.to_string())?);
            self.state = next.clone();
            self.publish()
        })();
        if r.is_err() { self.failed = true; }
        r
    }
    /// consume()
    pub fn consume(&mut self) -> Result<usize, String> {
        self.live(false)?;
        let raw = std::fs::read(&self.source).map_err(|e| e.to_string())?;
        let sealed_end = raw.iter().rposition(|b| *b == b'\n').map(|p| p + 1).unwrap_or(0);
        let sealed = &raw[..sealed_end];
        need(sealed.len() as u64 >= self.state["sourceBytes"].as_u64().unwrap_or(0), "SOURCE_TRUNCATED")?;
        let offset = self.state["sourceBytes"].as_u64().unwrap_or(0) as usize;
        let tail = String::from_utf8_lossy(&sealed[offset..]);
        let parts: Vec<&str> = tail.split('\n').collect();
        let lines: Vec<&str> = parts[..parts.len().saturating_sub(1)].to_vec();
        let mut account = V::Null;
        if let Ok(account_raw) = std::fs::read(self.runtime_dir.join("official-account.json")) {
            if let Ok(account_json) = serde_json::from_slice::<V>(&account_raw) {
                if let Some(a) = crate::probability_receipt::probability_account_eligibility(&account_json) {
                    account = a;
                }
            }
        }
        let n = lines.len();
        for line in lines {
            let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
            let hash = row["hash"].as_str().unwrap_or("").to_string();
            let mut body = row.clone();
            body.as_object_mut().and_then(|m| m.shift_remove("hash"));
            need(row["sequence"].as_i64() == Some(self.state["sourceSequence"].as_i64().unwrap_or(0) + 1)
                && row["previousHash"].as_str().unwrap_or("") == self.state["sourceHash"].as_str().unwrap_or("")
                && payload_hash(&body).ok().as_deref() == Some(hash.as_str()), "SOURCE_CHAIN")?;
            let available_at = baal_service_runtime::contract::ms_to_iso((self.now)() as f64);
            need(baal_paper::fixed::instant(row["at"].as_str().unwrap_or(""))?
                <= baal_paper::fixed::instant(&available_at)?, "FUTURE_SOURCE")?;
            let source_bytes = self.state["sourceBytes"].as_u64().unwrap_or(0) + line.len() as u64 + 1;
            let base = json!({"availableAt": available_at, "sourceSequence": row["sequence"],
                "sourceHash": hash, "sourceBytes": source_bytes});
            let event = if row["kind"].as_str() == Some("book")
                && row["payload"]["book"]["marketId"] == self.revision["marketId"]
            {
                let mut e = base.clone();
                e["kind"] = json!("book");
                e["book"] = row["payload"]["book"].clone();
                e["accountEligibility"] = account.clone();
                e
            } else if ["session_start", "session_stop"].contains(&row["kind"].as_str().unwrap_or(""))
                || (row["kind"].as_str() == Some("invalidation")
                    && (row["payload"]["marketId"].is_null()
                        || row["payload"]["marketId"] == self.revision["marketId"]))
            {
                let mut e = base.clone();
                e["kind"] = json!("boundary");
                e["reason"] = json!("source_boundary");
                e
            } else {
                let mut e = base;
                e["kind"] = json!("cursor");
                e
            };
            let next = advance_fundamental_revision(&self.state, &event, &self.bank.clone(), &self.revision.clone())?;
            self.append(&next, &event, false)?;
        }
        Ok(n)
    }
    /// onBook(publication, marketId, accountEligibility)
    pub fn on_book(&mut self, publication: &V, market_id: &str, account_eligibility: &V) -> Result<(), String> {
        if Some(market_id) != self.revision["marketId"].as_str() { return Ok(()); }
        self.live(false)?;
        let instrument = format!("{}:YES", self.revision["exchangeId"].as_str().unwrap_or(""));
        let book = publication["books"][&instrument].clone();
        if book.is_null() { return Ok(()); }
        let event = if baal_service_runtime::json_text::js_truthy(publication.get("closed")) {
            json!({"kind": "boundary", "reason": "source_closed",
                "availableAt": baal_service_runtime::contract::ms_to_iso((self.now)() as f64)})
        } else {
            json!({"kind": "book", "book": book, "accountEligibility": account_eligibility,
                "availableAt": baal_service_runtime::contract::ms_to_iso((self.now)() as f64),
                "sourceSequence": publication["sequence"], "sourceHash": publication["hash"]})
        };
        let next = advance_fundamental_revision(&self.state, &event, &self.bank.clone(), &self.revision.clone())?;
        self.append(&next, &event, false)
    }
    /// focus()
    pub fn focus(&self) -> Result<V, String> {
        let any_held = self.state["wallets"].as_object().cloned().unwrap_or_default().values()
            .any(|w| !w["positions"][self.revision["marketId"].as_str().unwrap_or("")].is_null()
                || !w["pending"].is_null());
        Ok(json!({"bankHash": self.opening["originalBankHash"], "revisionHash": digest(&self.revision),
            "markets": if any_held {
                vec![json!({"marketId": self.revision["marketId"], "phase": "held"})]
            } else { vec![] },
            "ownedRiskReads": true, "executionAuthority": false}))
    }
    /// boundary(reason)
    pub fn boundary(&mut self, reason: &str) -> Result<(), String> {
        let at = baal_service_runtime::contract::ms_to_iso(
            (self.now)().max(baal_paper::fixed::instant(self.state["lastAvailableAt"].as_str().unwrap_or(""))?) as f64);
        let event = json!({"kind": "boundary", "reason": reason, "availableAt": at});
        let next = advance_fundamental_revision(&self.state, &event, &self.bank.clone(), &self.revision.clone())?;
        self.append(&next, &event, true)
    }
    /// close()
    pub fn close(&mut self) -> Result<(), String> {
        if self.closed { return Ok(()); }
        need(std::fs::read_to_string(&self.lock_file).map_err(|e| e.to_string())? == self.token, "LOCK_CHANGED")?;
        self.closed = true;
        std::fs::remove_file(&self.lock_file).map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// wrapFundamentalValueRevision(host, {runtimeDir, now, canOperate}) — returns the
/// host unchanged when neither pointer nor opening exists, else a revision wrapper.
pub struct FundamentalValueRevisionWrapper {
    inner: Option<Box<dyn PaperHost>>,
    operator: Option<FundamentalRevisionOperator>,
}
impl FundamentalValueRevisionWrapper {
    pub fn wrap(inner: Box<dyn PaperHost>, runtime_dir: &Path,
        now: std::sync::Arc<dyn Fn() -> i64 + Send + Sync>,
        can_operate: std::sync::Arc<dyn Fn() -> bool + Send + Sync>,
        live_paper_dir: &Path) -> Result<Box<dyn PaperHost>, String> {
        let pointer = runtime_dir.join("FUNDAMENTAL_VALUE_REVISION");
        let opening = inner.dir().join("operator-value-revision").join("opening.json");
        if !pointer.exists() && !opening.exists() {
            return Ok(inner);
        }
        let revision: V = if pointer.exists() {
            let v: V = serde_json::from_slice(&std::fs::read(&pointer).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            need(v["schemaVersion"].as_i64() == Some(1) && v["owner"].as_str() == Some("Codex")
                && v["cohort"].as_str() == inner.bank()["name"].as_str()
                && v["bankHash"].as_str() == Some(digest(inner.bank()).as_str()), "POINTER")?;
            v["revision"].clone()
        } else {
            let o: V = serde_json::from_slice(&std::fs::read(&opening).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            o["revision"].clone()
        };
        let mut inner = inner;
        let operator = match FundamentalRevisionOperator::open(runtime_dir, inner.as_mut(), &revision,
            !opening.exists(), now.clone(), can_operate, live_paper_dir)
        {
            Ok(op) => op,
            Err(e) => {
                // The JS caller closes the original host when the wrap throws.
                let _ = inner.close();
                return Err(e);
            }
        };
        // Sticky ownership: absent or expired pointer never resumes the old forecast.
        Ok(Box::new(Self { inner: Some(inner), operator: Some(operator) }))
    }
}
impl PaperHost for FundamentalValueRevisionWrapper {
    fn dir(&self) -> &Path { self.operator.as_ref().map(|o| o.dir.as_path()).unwrap_or_else(|| self.inner.as_ref().unwrap().dir()) }
    fn bank(&self) -> &V { self.operator.as_ref().map(|o| &o.bank).unwrap_or_else(|| self.inner.as_ref().unwrap().bank()) }
    fn consume(&mut self) -> Result<usize, String> {
        if let Some(op) = &mut self.operator { op.consume() } else { self.inner.as_mut().unwrap().consume() }
    }
    fn status(&mut self) -> Result<V, String> {
        if let Some(op) = &self.operator { op.status() } else { self.inner.as_mut().unwrap().status() }
    }
    fn publish(&mut self) -> Result<(), String> {
        if let Some(op) = &self.operator { op.publish() } else { self.inner.as_mut().unwrap().publish() }
    }
    fn focus(&mut self) -> Result<V, String> {
        if let Some(op) = &self.operator { op.focus() } else { self.inner.as_mut().unwrap().focus() }
    }
    fn boundary(&mut self, reason: &str) -> Result<(), String> {
        if let Some(op) = &mut self.operator { op.boundary(reason) } else { self.inner.as_mut().unwrap().boundary(reason) }
    }
    fn on_book(&mut self, publication: &V, market_id: &str, account: &V) -> Result<(), String> {
        if let Some(op) = &mut self.operator { op.on_book(publication, market_id, account) }
        else { self.inner.as_mut().unwrap().on_book(publication, market_id, account) }
    }
    fn entry_draining(&self) -> Result<bool, String> {
        if self.operator.is_some() { Ok(true) } else { self.inner.as_ref().unwrap().entry_draining() }
    }
    fn close(&mut self) -> Result<(), String> {
        if let Some(op) = &mut self.operator { op.close()?; }
        if let Some(inner) = &mut self.inner { inner.close()?; }
        Ok(())
    }
    fn operator_revision_active(&self) -> bool { self.operator.is_some() }
}
