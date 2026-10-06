// live-paper/coupled-maker-host-v1.mjs — coupled maker host (v1 tape only).
use baal_execution::core::payload_hash;
use baal_service_runtime::atomic::atomic_json_canonical;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::journal::{journal_lines, Journal, JournalReadOptions};
use baal_service_runtime::json_text::{digest, js_int, js_truthy, sha256};
use serde_json::{json, Value as V};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::coupled_maker_paper_v1::{advance_coupled_maker, coupled_maker_report, create_coupled_maker_state};
use crate::quote_projection_recovery::quote_projection_recovery_requests;

fn need(ok: bool, code: &str) -> Result<(), String> { if !ok { return Err(format!("COUPLED_MAKER_HOST_{}", code)); } Ok(()) }
/// The workspace root the JS resolves from live-paper/*.mjs (repo root).
fn workspace_dir() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..") }
/// /^[a-z][a-z0-9-]{1,70}$/ .test(v)
fn cohort_ok(v: &V) -> bool {
    v.as_str().is_some_and(|s| s.len() >= 2 && s.len() <= 71
        && s.as_bytes()[0].is_ascii_lowercase()
        && s[1..].bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'))
}
fn js_num(v: &V) -> f64 {
    match v {
        V::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        V::String(s) => { let t = s.trim(); if t.is_empty() { 0.0 } else { t.parse::<f64>().unwrap_or(f64::NAN) } }
        _ => f64::NAN,
    }
}
/// Math.max(now(), Date.parse(x)) — NaN poisons like JS; new Date(...).toISOString() throws RangeError.
fn iso_or(ms: f64) -> Result<String, String> {
    if !ms.is_finite() || ms.abs() > 8_640_000_000_000_000.0 { return Err("Invalid time value".into()); }
    Ok(ms_to_iso(ms))
}
fn js_max(a: f64, b: f64) -> f64 { if a.is_nan() || b.is_nan() { f64::NAN } else { a.max(b) } }
/// coupledMakerCodeIdentity() — frozen manifest with live-paper/-relative keys.
pub fn coupled_maker_code_identity() -> Result<V, String> {
    let files = crate::frozen_paper_graph::collect_frozen_paper_files(
        &workspace_dir(), &["live-paper/coupled-maker-host-v1.mjs".to_string()])?;
    Ok(json!(files.into_iter()
        .map(|(f, h)| (f.trim_start_matches("live-paper/").to_string(), V::String(h)))
        .collect::<serde_json::Map<String, V>>()))
}
/// mergeMakerTapeFocus(...focus)
pub fn merge_maker_tape_focus(focus: &[V]) -> V {
    let mut seen = BTreeSet::new();
    let mut risk_market_ids: Vec<V> = Vec::new();
    for x in focus.iter().filter(|x| js_truthy(Some(x))) {
        for id in x["riskMarketIds"].as_array().cloned().unwrap_or_default() {
            let k = baal_service_runtime::json_text::js_stringify(&id);
            if seen.insert(k) { risk_market_ids.push(id); }
        }
    }
    let mut requests = serde_json::Map::new();
    for x in focus.iter().filter(|x| js_truthy(Some(x))) {
        for r in x["projectionRecoveryRequests"].as_array().cloned().unwrap_or_default() {
            requests.insert(r["key"].as_str().unwrap_or("").to_string(), r);
        }
    }
    let mut reqs: Vec<V> = requests.into_values().collect();
    reqs.sort_by(|a, b| {
        parse_iso_ms(a["from"].as_str().unwrap_or("")).unwrap_or(f64::NAN)
            .partial_cmp(&parse_iso_ms(b["from"].as_str().unwrap_or("")).unwrap_or(f64::NAN))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| js_num(&a["marketId"]).partial_cmp(&js_num(&b["marketId"])).unwrap_or(std::cmp::Ordering::Equal))
    });
    reqs.truncate(4);
    json!({"riskMarketIds": risk_market_ids, "projectionRecoveryRequests": reqs})
}
/// openCoupledMakerHost({runtimeDir, now, canOperate, activeFileName})
pub struct CoupledMakerHost {
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
impl CoupledMakerHost {
    /// openCoupledMakerHost({runtimeDir, now, canOperate, activeFileName})
    /// -> Option<Host>: None when the active file does not exist.
    pub fn open(runtime_dir: &Path, now: Arc<dyn Fn() -> f64 + Send + Sync>,
        can_operate: Arc<dyn Fn() -> bool + Send + Sync>, active_file_name: &str) -> Result<Option<Self>, String> {
        need(active_file_name == "coupled-maker-active.json", "ACTIVE_NAME")?;
        let active_file = runtime_dir.join(active_file_name);
        if !active_file.exists() { return Ok(None); }
        let active_bytes = fs::read(&active_file).map_err(|e| e.to_string())?;
        let active: V = serde_json::from_slice(&active_bytes).map_err(|e| e.to_string())?;
        need(active["schemaVersion"].as_i64() == Some(1) && cohort_ok(&active["cohort"]), "ACTIVE")?;
        let cohort = active["cohort"].as_str().unwrap().to_string();
        let dir = runtime_dir.join("coupled-maker-cohorts").join(&cohort);
        let bank_file = dir.join("bank.json");
        let bank_bytes = fs::read(&bank_file).map_err(|e| e.to_string())?;
        let bank: V = serde_json::from_slice(&bank_bytes).map_err(|e| e.to_string())?;
        let stop_file = dir.join("STOP");
        need(payload_hash(&bank).ok().as_deref() == active["bankHash"].as_str()
            && payload_hash(&bank["codeIdentity"]).ok() == payload_hash(&coupled_maker_code_identity()?).ok(), "FROZEN")?;
        let source_file = runtime_dir.join("anonymous-trades.jsonl");
        let prefix_bytes = js_int(&bank["tapePrefixBytes"]).unwrap_or(-1).max(0) as u64;
        let prefix = {
            let f = fs::File::open(&source_file).map_err(|e| e.to_string())?;
            let take = f.metadata().map_err(|e| e.to_string())?.len().min(prefix_bytes);
            use std::io::Read;
            let mut buf = vec![0u8; take as usize];
            let n = f.take(take).read(&mut buf).map_err(|e| e.to_string())?;
            buf.truncate(n);
            buf
        };
        need(prefix.len() as u64 == prefix_bytes
            && (prefix.is_empty() || *prefix.last().unwrap() == b'\n')
            && sha256(&prefix) == bank["tapePrefixHash"].as_str().unwrap_or(""), "SOURCE_PREFIX")?;
        let journal = Journal::open(&dir.join("journal.jsonl"))?;
        let stats_file = dir.join("stats.json");
        let state = match &journal.last {
            Some(last) if js_truthy(last.get("state")) => last["state"].clone(),
            _ => json!({"paper": create_coupled_maker_state(&bank)?,
                "tapeSequence": bank["tapePrefixSequence"], "tapeHash": bank["tapePrefixTailHash"]}),
        };
        need(state["paper"]["bankHash"] == active["bankHash"]
            && js_int(&state["tapeSequence"]).unwrap_or(i64::MIN) >= js_int(&bank["tapePrefixSequence"]).unwrap_or(i64::MIN), "STATE")?;
        if stats_file.exists() {
            let prior: V = serde_json::from_str(&fs::read_to_string(&stats_file).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            need(prior["bankHash"] == active["bankHash"]
                && js_int(&prior["journalSequence"]).unwrap_or(i64::MIN) <= journal.sequence as i64, "PUBLISHED_ROLLBACK")?;
            if js_int(&prior["journalSequence"]) == Some(journal.sequence as i64) {
                need(prior["journalHash"].as_str() == Some(journal.previous.as_str()), "PUBLISHED_TAIL")?;
            }
        }
        let mut host = CoupledMakerHost {
            bank, dir, journal, state, stats_file, active_file, bank_file,
            active_bytes, bank_bytes, stop_file, failed: false, now, can_operate,
        };
        // Startup replay of the tape prefix beyond the stored frontier.
        let mut sequence = 0i64;
        let mut previous = String::new();
        for line in journal_lines(&source_file, &JournalReadOptions::default())? {
            let row: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
            let hash = row["hash"].as_str().unwrap_or("").to_string();
            let mut body = row.clone();
            body.as_object_mut().unwrap().remove("hash");
            sequence += 1;
            need(js_int(&row["sequence"]) == Some(sequence)
                && row["previous"].as_str() == Some(previous.as_str())
                && digest(&body) == hash, "SOURCE_CHAIN")?;
            previous = hash.clone();
            let tape_seq = js_int(&host.state["tapeSequence"]).unwrap_or(0);
            if js_int(&row["sequence"]) == Some(tape_seq) {
                need(row["hash"].as_str() == host.state["tapeHash"].as_str(), "FRONTIER_CHANGED")?;
            }
            if js_int(&row["sequence"]).unwrap_or(0) > tape_seq {
                let at = iso_or(js_max(host.now_ms(), parse_iso_ms(host.state["paper"]["lastAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN)))?;
                host.page(&row, &at)?;
            }
        }
        need(sequence >= js_int(&host.state["tapeSequence"]).unwrap_or(0), "SOURCE_TRUNCATED")?;
        if host.journal.last.is_none() {
            let state = host.state.clone();
            host.append(state, &json!({"kind": "genesis", "cutoff": host.bank["cutoff"]}), false)?;
        }
        Ok(Some(host))
    }
    fn now_ms(&self) -> f64 { (self.now)() }
    /// status()
    pub fn status(&self) -> Result<V, String> {
        let at = iso_or(self.now_ms())?;
        let mut out = coupled_maker_report(&self.state["paper"], &self.bank, &at)?;
        out["failed"] = json!(self.failed);
        out["lastEventAt"] = self.state["paper"]["lastAt"].clone();
        out["journalSequence"] = json!(self.journal.sequence);
        out["journalHash"] = json!(self.journal.previous);
        out["tapeSequence"] = self.state["tapeSequence"].clone();
        out["riskMarketIds"] = json!(risk_market_ids(&self.state));
        out["projectionRecoveryRequests"] = json!(quote_projection_recovery_requests(&self.state["paper"], &self.bank, self.now_ms() as i64));
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
            return Err("COUPLED_MAKER_HOST_DURABILITY".into());
        }
        Ok(())
    }
    fn page(&mut self, row: &V, available_at: &str) -> Result<(), String> {
        let tape_seq = js_int(&self.state["tapeSequence"]).unwrap_or(0);
        need(js_int(&row["sequence"]) == Some(tape_seq + 1)
            && row["previous"].as_str() == Some(self.state["tapeHash"].as_str().unwrap_or("")), "FRONTIER")?;
        let hash = row["hash"].as_str().unwrap_or("").to_string();
        let mut body = row.clone();
        body.as_object_mut().unwrap().remove("hash");
        need(digest(&body) == hash, "SOURCE_CHAIN")?;
        let relevant = self.bank["legs"].as_array().is_some_and(|legs| legs.iter()
            .any(|l| l["exchangeId"] == row["event"]["exchangeId"] && l["marketId"] == row["event"]["marketId"]));
        let paper = if relevant {
            advance_coupled_maker(&self.state["paper"],
                &json!({"kind": "trades", "availableAt": available_at, "page": row["event"]}), &self.bank)?
        } else {
            self.state["paper"].clone()
        };
        let mut next = self.state.clone();
        next["paper"] = paper;
        next["tapeSequence"] = row["sequence"].clone();
        next["tapeHash"] = json!(hash.clone());
        self.append(next, &json!({"kind": "source_page", "availableAt": available_at,
            "tapeSequence": row["sequence"], "tapeHash": hash, "relevant": relevant}), false)
    }
    /// focus() — status plus the market ids this host is a candidate for.
    pub fn focus(&self) -> Result<V, String> {
        let s = self.status()?;
        let mut out = s;
        out["candidateMarketIds"] = if self.allowed()
            && self.now_ms() < parse_iso_ms(self.bank["entryExpiresAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN) {
            json!(self.bank["legs"].as_array().cloned().unwrap_or_default().iter()
                .map(|l| l["marketId"].clone()).collect::<Vec<_>>())
        } else {
            json!([])
        };
        Ok(out)
    }
    /// onPage(row)
    pub fn on_page(&mut self, row: &V) -> Result<(), String> {
        self.verify(false)?;
        let tape_seq = js_int(&self.state["tapeSequence"]).unwrap_or(0);
        if js_int(&row["sequence"]).unwrap_or(0) <= tape_seq {
            need(js_int(&row["sequence"]) == Some(tape_seq)
                && row["hash"].as_str() == self.state["tapeHash"].as_str(), "DUPLICATE")?;
            return Ok(());
        }
        let at = iso_or(js_max(self.now_ms(), parse_iso_ms(self.state["paper"]["lastAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN)))?;
        self.page(row, &at)
    }
    /// onBook(publication, marketId, accountEligibility)
    pub fn on_book(&mut self, publication: &V, market_id: &str, account_eligibility: &V) -> Result<(), String> {
        self.verify(false)?;
        let leg = self.bank["legs"].as_array().cloned().unwrap_or_default()
            .into_iter().find(|l| l["marketId"].as_str() == Some(market_id));
        let leg = match leg { Some(l) => l, None => return Ok(()) };
        let book = publication["books"][format!("{}:YES", leg["exchangeId"].as_str().unwrap_or("")).as_str()].clone();
        if book.is_null() { return Ok(()); }
        let at = iso_or(self.now_ms())?;
        let mut next = self.state.clone();
        next["paper"] = advance_coupled_maker(&self.state["paper"],
            &json!({"kind": "book", "availableAt": at, "book": book, "accountEligibility": account_eligibility}),
            &self.bank)?;
        self.append(next, &json!({"kind": "book", "availableAt": at,
            "sourceSequence": publication["sequence"], "sourceHash": publication["hash"], "marketId": market_id}), false)
    }
    /// boundary(reason)
    pub fn boundary(&mut self, reason: &V) -> Result<(), String> {
        self.verify(true)?;
        let at = iso_or(js_max(self.now_ms(), parse_iso_ms(self.state["paper"]["lastAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN)))?;
        let mut next = self.state.clone();
        next["paper"] = advance_coupled_maker(&self.state["paper"],
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
