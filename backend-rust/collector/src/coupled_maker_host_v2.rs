// live-paper/coupled-maker-host-v2.mjs — coupled bulk host (tape + prices sources).
use baal_execution::core::payload_hash;
use baal_service_runtime::atomic::atomic_json_canonical;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::journal::{journal_lines, Journal, JournalReadOptions};
use baal_service_runtime::json_text::{digest, js_int, js_truthy, sha256};
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::coupled_maker_paper_v2::{advance_coupled_bulk, coupled_bulk_candidates, coupled_bulk_report, create_coupled_bulk_state};
use crate::quote_projection_recovery::quote_projection_recovery_requests;

fn need(ok: bool, code: &str) -> Result<(), String> { if !ok { return Err(format!("COUPLED_BULK_HOST_{}", code)); } Ok(()) }
fn workspace_dir() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..") }
fn cohort_ok(v: &V) -> bool {
    v.as_str().is_some_and(|s| s.len() >= 2 && s.len() <= 71
        && s.as_bytes()[0].is_ascii_lowercase()
        && s[1..].bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'))
}
fn iso_or(ms: f64) -> Result<String, String> {
    if !ms.is_finite() || ms.abs() > 8_640_000_000_000_000.0 { return Err("Invalid time value".into()); }
    Ok(ms_to_iso(ms))
}
fn js_max(a: f64, b: f64) -> f64 { if a.is_nan() || b.is_nan() { f64::NAN } else { a.max(b) } }
/// coupledBulkCodeIdentity() — frozen manifest with live-paper/-relative keys.
pub fn coupled_bulk_code_identity() -> Result<V, String> {
    let files = crate::frozen_paper_graph::collect_frozen_paper_files(
        &workspace_dir(), &["live-paper/coupled-maker-host-v2.mjs".to_string()])?;
    Ok(json!(files.into_iter()
        .map(|(f, h)| (f.trim_start_matches("live-paper/").to_string(), V::String(h)))
        .collect::<serde_json::Map<String, V>>()))
}
/// prefixHash(file, length) — 1MB chunks; a non-empty prefix must end at a newline.
fn prefix_hash(file: &Path, length: u64) -> Result<String, String> {
    let mut hash = Sha256::new();
    let mut fd = fs::File::open(file).map_err(|e| e.to_string())?;
    let mut chunk = vec![0u8; 1024 * 1024];
    let mut offset = 0u64;
    let mut last: Option<u8> = None;
    while offset < length {
        let want = (chunk.len() as u64).min(length - offset) as usize;
        let read = fd.read(&mut chunk[..want]).map_err(|e| e.to_string())?;
        need(read > 0, "SOURCE_PREFIX")?;
        hash.update(&chunk[..read]);
        last = Some(chunk[read - 1]);
        offset += read as u64;
    }
    need(length == 0 || last == Some(10), "SOURCE_PREFIX_LINE")?;
    Ok(format!("{:x}", hash.finalize()))
}
/// openCoupledBulkHost({runtimeDir, now, canOperate})
pub struct CoupledBulkHost {
    pub bank: V,
    pub dir: PathBuf,
    journal: Journal,
    state: V,
    stats_file: PathBuf,
    active_file: PathBuf,
    bank_file: PathBuf,
    active_bytes: Vec<u8>,
    bank_bytes: Vec<u8>,
    stop_file: PathBuf,
    failed: bool,
    now: Arc<dyn Fn() -> f64 + Send + Sync>,
    can_operate: Arc<dyn Fn() -> bool + Send + Sync>,
}
fn bulk_sources(runtime_dir: &Path, bank: &V) -> Vec<(String, PathBuf, V)> {
    vec![
        ("tape".to_string(), runtime_dir.join("anonymous-trades.jsonl"), bank["tapePrefix"].clone()),
        ("prices".to_string(), runtime_dir.join("market-prices.jsonl"), bank["pricePrefix"].clone()),
    ]
}
impl CoupledBulkHost {
    /// openCoupledBulkHost({runtimeDir, now, canOperate}) -> Option<Host>.
    pub fn open(runtime_dir: &Path, now: Arc<dyn Fn() -> f64 + Send + Sync>,
        can_operate: Arc<dyn Fn() -> bool + Send + Sync>) -> Result<Option<Self>, String> {
        let active_file = runtime_dir.join("coupled-bulk-active.json");
        if !active_file.exists() { return Ok(None); }
        let active_bytes = fs::read(&active_file).map_err(|e| e.to_string())?;
        let active: V = serde_json::from_slice(&active_bytes).map_err(|e| e.to_string())?;
        need(active["schemaVersion"].as_i64() == Some(1) && cohort_ok(&active["cohort"]), "ACTIVE")?;
        let cohort = active["cohort"].as_str().unwrap().to_string();
        let dir = runtime_dir.join("coupled-bulk-cohorts").join(&cohort);
        let bank_file = dir.join("bank.json");
        let bank_bytes = fs::read(&bank_file).map_err(|e| e.to_string())?;
        let bank: V = serde_json::from_slice(&bank_bytes).map_err(|e| e.to_string())?;
        let stop_file = dir.join("STOP");
        need(payload_hash(&bank).ok().as_deref() == active["bankHash"].as_str()
            && payload_hash(&bank["codeIdentity"]).ok() == payload_hash(&coupled_bulk_code_identity()?).ok(), "FROZEN")?;
        let sources = bulk_sources(runtime_dir, &bank);
        for (_k, file, prefix) in &sources {
            let bytes = js_int(&prefix["bytes"]);
            need(!prefix.is_null() && bytes.is_some_and(|b| b >= 0)
                && prefix_hash(file, bytes.unwrap() as u64)? == prefix["hash"].as_str().unwrap_or(""), "SOURCE_PREFIX")?;
        }
        let journal = Journal::open(&dir.join("journal.jsonl"))?;
        let stats_file = dir.join("stats.json");
        let state = match &journal.last {
            Some(last) if js_truthy(last.get("state")) => last["state"].clone(),
            _ => {
                let mut frontiers = serde_json::Map::new();
                for (key, _file, prefix) in &sources {
                    frontiers.insert(key.clone(), json!({"sequence": prefix["sequence"], "hash": prefix["tailHash"]}));
                }
                json!({"paper": create_coupled_bulk_state(&bank)?, "frontiers": frontiers})
            }
        };
        need(state["paper"]["bankHash"] == active["bankHash"], "STATE")?;
        for (key, _f, prefix) in &sources {
            let fs = js_int(&state["frontiers"][key.as_str()]["sequence"]);
            let ps = js_int(&prefix["sequence"]);
            need(fs.is_some() && ps.is_some() && fs >= ps, "FRONTIER")?;
        }
        if stats_file.exists() {
            let prior: V = serde_json::from_str(&fs::read_to_string(&stats_file).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            need(prior["bankHash"] == active["bankHash"]
                && js_int(&prior["journalSequence"]).unwrap_or(i64::MIN) <= journal.sequence as i64, "PUBLISHED_ROLLBACK")?;
            if js_int(&prior["journalSequence"]) == Some(journal.sequence as i64) {
                need(prior["journalHash"].as_str() == Some(journal.previous.as_str()), "PUBLISHED_TAIL")?;
            }
        }
        let mut host = CoupledBulkHost {
            bank, dir, journal, state, stats_file, active_file, bank_file,
            active_bytes, bank_bytes, stop_file, failed: false, now, can_operate,
        };
        for (key, file, _prefix) in &sources {
            let mut sequence = 0i64;
            let mut previous = String::new();
            let mut frontier_seen = js_int(&host.state["frontiers"][key.as_str()]["sequence"]) == Some(0)
                && host.state["frontiers"][key.as_str()]["hash"].as_str() == Some("");
            for line in journal_lines(file, &JournalReadOptions::default())? {
                let row: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
                let hash = row["hash"].as_str().unwrap_or("").to_string();
                let mut body = row.clone();
                body.as_object_mut().unwrap().remove("hash");
                sequence += 1;
                need(js_int(&row["sequence"]) == Some(sequence)
                    && row["previous"].as_str() == Some(previous.as_str())
                    && digest(&body) == hash, "SOURCE_CHAIN")?;
                previous = hash.clone();
                let frontier_seq = js_int(&host.state["frontiers"][key.as_str()]["sequence"]).unwrap_or(0);
                if js_int(&row["sequence"]) == Some(frontier_seq) {
                    need(row["hash"].as_str() == host.state["frontiers"][key.as_str()]["hash"].as_str(), "FRONTIER_CHANGED")?;
                    frontier_seen = true;
                }
                if js_int(&row["sequence"]).unwrap_or(0) > frontier_seq {
                    host.consume(key, &row)?;
                }
            }
            need(frontier_seen && sequence >= js_int(&host.state["frontiers"][key.as_str()]["sequence"]).unwrap_or(0), "SOURCE_TRUNCATED")?;
        }
        if host.journal.last.is_none() {
            let state = host.state.clone();
            host.append(state, &json!({"kind": "genesis", "cutoff": host.bank["cutoff"]}), false)?;
        }
        Ok(Some(host))
    }
    fn now_ms(&self) -> f64 { (self.now)() }
    fn time(&self) -> Result<String, String> {
        iso_or(js_max(self.now_ms(),
            parse_iso_ms(self.state["paper"]["lastAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN)))
    }
    /// status()
    pub fn status(&self) -> Result<V, String> {
        let at = self.time()?;
        let mut out = coupled_bulk_report(&self.state["paper"], &self.bank, &at)?;
        out["failed"] = json!(self.failed);
        out["lastEventAt"] = self.state["paper"]["lastAt"].clone();
        out["journalSequence"] = json!(self.journal.sequence);
        out["journalHash"] = json!(self.journal.previous);
        out["sourceFrontiers"] = self.state["frontiers"].clone();
        out["riskMarketIds"] = json!(risk_market_ids(&self.state));
        out["projectionRecoveryRequests"] = json!(quote_projection_recovery_requests(&self.state["paper"], &self.bank, parse_iso_ms(&at).unwrap_or(0.0) as i64));
        Ok(out)
    }
    fn allowed(&self) -> bool { !self.failed && (self.can_operate)() && !self.stop_file.exists() }
    fn verify(&self, allow_stopped: bool) -> Result<(), String> {
        need(!self.failed && (allow_stopped || self.allowed()), "STOPPED")?;
        need(sha256(&fs::read(&self.active_file).map_err(|e| e.to_string())?) == sha256(&self.active_bytes)
            && sha256(&fs::read(&self.bank_file).map_err(|e| e.to_string())?) == sha256(&self.bank_bytes), "BANK_CHANGED")?;
        Ok(())
    }
    fn append(&mut self, next: V, event: &V, allow_stopped: bool) -> Result<(), String> {
        self.verify(allow_stopped)?;
        let attempt: Result<(), String> = (|| {
            self.journal.commit(&next, event)?;
            self.state = next;
            let st = self.status()?;
            atomic_json_canonical(&self.stats_file, &st)?;
            Ok(())
        })();
        if attempt.is_err() {
            self.failed = true;
            return Err("COUPLED_BULK_HOST_DURABILITY".into());
        }
        Ok(())
    }
    /// consume(key, row) — verified-source row into the paper reducer.
    fn consume(&mut self, key: &str, row: &V) -> Result<(), String> {
        let frontier = &self.state["frontiers"][key];
        need(js_int(&row["sequence"]) == Some(js_int(&frontier["sequence"]).unwrap_or(0) + 1)
            && row["previous"].as_str() == frontier["hash"].as_str(), "FRONTIER")?;
        let hash = row["hash"].as_str().unwrap_or("").to_string();
        let mut body = row.clone();
        body.as_object_mut().unwrap().remove("hash");
        need(digest(&body) == hash
            && (key != "prices" || row.as_object().map(|m| m.get("state").map(|s| s.is_null()).unwrap_or(false)).unwrap_or(false)), "SOURCE_CHAIN")?;
        let available_at = self.time()?;
        let mut paper = self.state["paper"].clone();
        let mut relevant = true;
        if key == "prices" {
            need(row["event"]["type"].as_str() == Some("market_price_observation"), "PRICE_SOURCE")?;
            paper = advance_coupled_bulk(&paper,
                &json!({"kind": "prices", "availableAt": available_at, "snapshot": row["event"]["snapshot"]}), &self.bank)?;
        } else {
            relevant = self.bank["legs"].as_array().is_some_and(|legs| legs.iter()
                .any(|l| l["exchangeId"] == row["event"]["exchangeId"] && l["marketId"] == row["event"]["marketId"]));
            if relevant {
                paper = advance_coupled_bulk(&paper,
                    &json!({"kind": "trades", "availableAt": available_at, "page": row["event"]}), &self.bank)?;
            }
        }
        let mut next = self.state.clone();
        next["paper"] = paper;
        let mut frontiers = self.state["frontiers"].clone();
        frontiers[key] = json!({"sequence": row["sequence"], "hash": hash.clone()});
        next["frontiers"] = frontiers;
        self.append(next, &json!({"kind": format!("source_{}", key), "availableAt": available_at,
            "sourceSequence": row["sequence"], "sourceHash": hash, "relevant": relevant}), false)
    }
    /// focus() — status plus bulk candidate market ids.
    pub fn focus(&self) -> Result<V, String> {
        let s = self.status()?;
        let mut out = s;
        out["candidateMarketIds"] = if self.allowed()
            && self.now_ms() < parse_iso_ms(self.bank["entryExpiresAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN) {
            json!(coupled_bulk_candidates(&self.state["paper"], &self.bank, &self.time()?)?
                .iter().map(|x| x["marketId"].clone()).collect::<Vec<_>>())
        } else {
            json!([])
        };
        Ok(out)
    }
    fn callback(&mut self, key: &str, row: &V) -> Result<(), String> {
        self.verify(false)?;
        let frontier = &self.state["frontiers"][key];
        if let Some(rs) = js_int(&row["sequence"]) {
            if rs <= js_int(&frontier["sequence"]).unwrap_or(0) {
                need(rs == js_int(&frontier["sequence"]).unwrap_or(0)
                    && row["hash"].as_str() == frontier["hash"].as_str(), "DUPLICATE")?;
                return Ok(());
            }
        }
        self.consume(key, row)
    }
    /// onPage(row) — tape source row.
    pub fn on_page(&mut self, row: &V) -> Result<(), String> { self.callback("tape", row) }
    /// onPrice(row) — prices source row.
    pub fn on_price(&mut self, row: &V) -> Result<(), String> { self.callback("prices", row) }
    /// onBook(publication, marketId, accountEligibility)
    pub fn on_book(&mut self, publication: &V, market_id: &str, account_eligibility: &V) -> Result<(), String> {
        self.verify(false)?;
        let leg = self.bank["legs"].as_array().cloned().unwrap_or_default()
            .into_iter().find(|l| l["marketId"].as_str() == Some(market_id));
        let leg = match leg { Some(l) => l, None => return Ok(()) };
        let book = publication["books"][format!("{}:YES", leg["exchangeId"].as_str().unwrap_or("")).as_str()].clone();
        if book.is_null() { return Ok(()); }
        let at = self.time()?;
        let mut next = self.state.clone();
        next["paper"] = advance_coupled_bulk(&self.state["paper"],
            &json!({"kind": "book", "availableAt": at, "book": book, "accountEligibility": account_eligibility}),
            &self.bank)?;
        self.append(next, &json!({"kind": "book", "availableAt": at,
            "sourceSequence": publication["sequence"], "sourceHash": publication["hash"], "marketId": market_id}), false)
    }
    /// boundary(reason)
    pub fn boundary(&mut self, reason: &V) -> Result<(), String> {
        self.verify(true)?;
        let at = self.time()?;
        let mut next = self.state.clone();
        next["paper"] = advance_coupled_bulk(&self.state["paper"],
            &json!({"kind": "boundary", "availableAt": at}), &self.bank)?;
        self.append(next, &json!({"kind": "boundary", "reason": reason, "availableAt": at}), true)
    }
}
/// riskMarketIds — deduped market ids across open positions and unresolved quotes.
fn risk_market_ids(state: &V) -> Vec<V> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for w in state["paper"]["wallets"].as_object().map(|m| m.values().cloned().collect::<Vec<_>>()).unwrap_or_default() {
        for p in w["positions"].as_object().map(|m| m.values().cloned().collect::<Vec<_>>()).unwrap_or_default() {
            let id = baal_service_runtime::json_text::js_stringify(&p["marketId"]);
            if seen.insert(id) { out.push(p["marketId"].clone()); }
        }
        for q in w["quotes"].as_object().map(|m| m.values().cloned().collect::<Vec<_>>()).unwrap_or_default() {
            if !js_truthy(q.get("resolvedAt")) {
                let id = baal_service_runtime::json_text::js_stringify(&q["marketId"]);
                if seen.insert(id) { out.push(q["marketId"].clone()); }
            }
        }
    }
    out
}
