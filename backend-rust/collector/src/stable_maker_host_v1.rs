// live-paper/stable-maker-host-v1.mjs — stable two-side tape host.
use baal_execution::core::payload_hash;
use baal_service_runtime::atomic::atomic_json_canonical;
use baal_service_runtime::journal::{journal_lines, Journal, JournalReadOptions};
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Map, Value as V};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(format!("STABLE_MAKER_HOST_{}", code)); }
    Ok(())
}
fn sha(b: &[u8]) -> String { sha256(b) }
fn instant(v: &V) -> Result<i64, String> { baal_paper::fixed::instant(v.as_str().unwrap_or("")) }
fn parse_ms(v: &V) -> f64 {
    baal_service_runtime::contract::parse_iso_ms(v.as_str().unwrap_or("")).unwrap_or(f64::NAN)
}
/// stableMakerCodeIdentity() — frozen paper files rooted at the repo workspace,
/// keyed relative to 'live-paper'. `workspace` = parent of live_paper_dir.
pub fn stable_maker_code_identity(workspace: &Path) -> Result<V, String> {
    let files = crate::frozen_paper_graph::collect_frozen_paper_files(workspace,
        &["live-paper/stable-maker-host-v1.mjs".to_string()])?;
    let mut out = Map::new();
    for (f, h) in files {
        // path.posix.relative('live-paper', f)
        let key = if let Some(rest) = f.strip_prefix("live-paper/") {
            rest.to_string()
        } else {
            format!("../{}", f)
        };
        out.insert(key, json!(h));
    }
    Ok(V::Object(out))
}
/// mergeMakerTapeFocus(...focus)
pub fn merge_maker_tape_focus(focus: &[V]) -> Result<V, String> {
    let mut risk: Vec<V> = Vec::new();
    for x in focus.iter().filter(|x| !x.is_null()) {
        for r in x["riskMarketIds"].as_array().cloned().unwrap_or_default() {
            if !risk.contains(&r) { risk.push(r); }
        }
    }
    let mut requests: indexmap::IndexMap<String, V> = Default::default();
    for x in focus.iter().filter(|x| !x.is_null()) {
        for r in x["projectionRecoveryRequests"].as_array().cloned().unwrap_or_default() {
            let key = r["key"].as_str().unwrap_or("").to_string();
            if !requests.contains_key(&key) { requests.insert(key, r); }
        }
    }
    let mut reqs: Vec<V> = requests.into_values().collect();
    reqs.sort_by(|a, b| {
        parse_ms(&a["from"]).partial_cmp(&parse_ms(&b["from"])).unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                a["marketId"].as_str().unwrap_or("").parse::<f64>().unwrap_or(0.0)
                    .partial_cmp(&b["marketId"].as_str().unwrap_or("").parse::<f64>().unwrap_or(0.0))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    reqs.truncate(4);
    Ok(json!({"riskMarketIds": risk, "projectionRecoveryRequests": reqs}))
}
/// openStableMakerHost({runtimeDir, now, canOperate, activeFileName})
/// Ok(None) mirrors the JS null when the active file is absent.
/// `workspace` = repo workspace (parent of live_paper_dir) for code identity.
pub struct StableMakerHost {
    pub bank: V,
    pub dir: PathBuf,
    journal: Journal,
    state: V,
    failed: bool,
    active_file: PathBuf,
    active_bytes: Vec<u8>,
    active_hash: String,
    bank_file: PathBuf,
    bank_bytes: Vec<u8>,
    stats_file: PathBuf,
    stop_file: PathBuf,
    now: Arc<dyn Fn() -> i64 + Send + Sync>,
    can_operate: Arc<dyn Fn() -> bool + Send + Sync>,
}
impl StableMakerHost {
    pub fn open(runtime_dir: &Path, now: Arc<dyn Fn() -> i64 + Send + Sync>,
        can_operate: Arc<dyn Fn() -> bool + Send + Sync>, active_file_name: &str,
        workspace: &Path) -> Result<Option<Self>, String> {
        need(["stable-maker-active.json", "stable-maker-broad-active.json"].contains(&active_file_name), "ACTIVE_NAME")?;
        let active_file = runtime_dir.join(active_file_name);
        if !active_file.exists() { return Ok(None); }
        let active_bytes = std::fs::read(&active_file).map_err(|e| e.to_string())?;
        let active: V = serde_json::from_slice(&active_bytes).map_err(|e| e.to_string())?;
        let cohort = active["cohort"].as_str().unwrap_or("");
        let cb = cohort.as_bytes();
        need(active["schemaVersion"].as_i64() == Some(1)
            && cb.len() >= 2 && cb.len() <= 71 && cb[0].is_ascii_lowercase()
            && cb[1..].iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-'), "ACTIVE")?;
        let dir = runtime_dir.join("stable-maker-cohorts").join(cohort);
        let bank_file = dir.join("bank.json");
        let bank_bytes = std::fs::read(&bank_file).map_err(|e| e.to_string())?;
        let bank: V = serde_json::from_slice(&bank_bytes).map_err(|e| e.to_string())?;
        let stop_file = dir.join("STOP");
        need(payload_hash(&bank)? == active["bankHash"].as_str().unwrap_or("")
            && payload_hash(&bank["codeIdentity"])? == payload_hash(&stable_maker_code_identity(workspace)?)?, "FROZEN")?;
        let source_file = runtime_dir.join("anonymous-trades.jsonl");
        let raw = std::fs::read(&source_file).map_err(|e| e.to_string())?;
        let tape_prefix = bank["tapePrefixBytes"].as_u64().unwrap_or(0) as usize;
        let prefix = &raw[..tape_prefix.min(raw.len())];
        need(prefix.len() == tape_prefix
            && (prefix.is_empty() || *prefix.last().unwrap() == b'\n')
            && sha(prefix) == bank["tapePrefixHash"].as_str().unwrap_or(""), "SOURCE_PREFIX")?;
        let journal = Journal::open(&dir.join("journal.jsonl"))?;
        let stats_file = dir.join("stats.json");
        let mut state = if let Some(last) = &journal.last {
            last["state"].clone()
        } else {
            json!({"paper": crate::stable_maker_paper_v2::create_stable_maker_state(&bank)?,
                "tapeSequence": bank["tapePrefixSequence"], "tapeHash": bank["tapePrefixTailHash"]})
        };
        need(state["paper"]["bankHash"].as_str() == active["bankHash"].as_str()
            && state["tapeSequence"].as_i64().unwrap_or(0) >= bank["tapePrefixSequence"].as_i64().unwrap_or(0), "STATE")?;
        if stats_file.exists() {
            let prior: V = serde_json::from_slice(&std::fs::read(&stats_file).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            need(prior["bankHash"].as_str() == active["bankHash"].as_str()
                && prior["journalSequence"].as_i64().unwrap_or(0) <= journal.sequence as i64, "PUBLISHED_ROLLBACK")?;
            if prior["journalSequence"].as_i64() == Some(journal.sequence as i64) {
                need(prior["journalHash"].as_str() == Some(journal.previous.as_str()), "PUBLISHED_TAIL")?;
            }
        }
        let mut host = Self {
            bank, dir, journal, state: state.clone(), failed: false, active_file,
            active_bytes, active_hash: active["bankHash"].as_str().unwrap_or("").to_string(),
            bank_file, bank_bytes, stats_file, stop_file, now, can_operate,
        };
        // Replay the tape source from the last frontier.
        let mut sequence = 0i64;
        let mut previous = String::new();
        for line in journal_lines(&source_file, &JournalReadOptions::default())? {
            let row: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
            let hash = row["hash"].as_str().unwrap_or("").to_string();
            let mut body = row.clone();
            body.as_object_mut().and_then(|m| m.shift_remove("hash"));
            sequence += 1;
            need(row["sequence"].as_i64() == Some(sequence)
                && row["previous"].as_str().unwrap_or("") == previous
                && sha256(baal_service_runtime::json_text::js_stringify(&body).as_bytes()) == hash, "SOURCE_CHAIN")?;
            previous = hash.clone();
            if row["sequence"].as_i64() == Some(state["tapeSequence"].as_i64().unwrap_or(0)) {
                need(row["hash"].as_str() == state["tapeHash"].as_str(), "FRONTIER_CHANGED")?;
            }
            if row["sequence"].as_i64().unwrap_or(0) > state["tapeSequence"].as_i64().unwrap_or(0) {
                let at = baal_service_runtime::contract::ms_to_iso(
                    (host.now)().max(state["paper"]["lastAt"].as_str()
                        .and_then(|s| baal_paper::fixed::instant(s).ok()).unwrap_or(0)) as f64);
                host.page(&row, &at)?;
            }
            state = host.state.clone();
        }
        need(sequence >= state["tapeSequence"].as_i64().unwrap_or(0), "SOURCE_TRUNCATED")?;
        if host.journal.last.is_none() {
            let next = host.state.clone();
            host.append(&next, &json!({"kind": "genesis", "cutoff": host.bank["cutoff"]}), false)?;
        }
        Ok(Some(host))
    }
    fn allowed(&self) -> bool {
        !self.failed && (self.can_operate)() && !self.stop_file.exists()
    }
    fn verify(&self, allow_stopped: bool) -> Result<(), String> {
        need(!self.failed && (allow_stopped || self.allowed()), "STOPPED")?;
        need(sha(&std::fs::read(&self.active_file).map_err(|e| e.to_string())?) == sha(&self.active_bytes)
            && sha(&std::fs::read(&self.bank_file).map_err(|e| e.to_string())?) == sha(&self.bank_bytes), "BANK_CHANGED")
    }
    fn append(&mut self, next: &V, event: &V, allow_stopped: bool) -> Result<(), String> {
        self.verify(allow_stopped)?;
        let r = (|| -> Result<(), String> {
            self.journal.commit(next, event)?;
            self.state = next.clone();
            atomic_json_canonical(&self.stats_file, &self.status()?)
        })();
        if r.is_err() {
            self.failed = true;
            return Err("STABLE_MAKER_HOST_DURABILITY".into());
        }
        r
    }
    /// status()
    pub fn status(&self) -> Result<V, String> {
        let at = baal_service_runtime::contract::ms_to_iso((self.now)() as f64);
        let mut out = crate::stable_maker_paper_v2::stable_maker_report(&self.state["paper"], &self.bank, &at)?;
        out["failed"] = json!(self.failed);
        out["lastEventAt"] = self.state["paper"]["lastAt"].clone();
        out["journalSequence"] = json!(self.journal.sequence);
        out["journalHash"] = json!(self.journal.previous);
        out["tapeSequence"] = self.state["tapeSequence"].clone();
        let mut risk: Vec<V> = Vec::new();
        for (_k, w) in self.state["paper"]["wallets"].as_object().cloned().unwrap_or_default() {
            for (_pk, p) in w["positions"].as_object().cloned().unwrap_or_default() {
                if !risk.contains(&p["marketId"]) { risk.push(p["marketId"].clone()); }
            }
            for (_qk, q) in w["quotes"].as_object().cloned().unwrap_or_default() {
                if q["resolvedAt"].is_null() && !risk.contains(&q["marketId"]) {
                    risk.push(q["marketId"].clone());
                }
            }
        }
        out["riskMarketIds"] = json!(risk);
        out["projectionRecoveryRequests"] = json!(crate::quote_projection_recovery::quote_projection_recovery_requests(
            &self.state["paper"], &self.bank, (self.now)()));
        Ok(out)
    }
    fn page(&mut self, row: &V, available_at: &str) -> Result<(), String> {
        need(row["sequence"].as_i64() == Some(self.state["tapeSequence"].as_i64().unwrap_or(0) + 1)
            && row["previous"].as_str() == self.state["tapeHash"].as_str(), "FRONTIER")?;
        let hash = row["hash"].as_str().unwrap_or("").to_string();
        let mut body = row.clone();
        body.as_object_mut().and_then(|m| m.shift_remove("hash"));
        need(sha256(baal_service_runtime::json_text::js_stringify(&body).as_bytes()) == hash, "SOURCE_CHAIN")?;
        let relevant = self.bank["legs"].as_array().is_some_and(|l| l.iter()
            .any(|l| l["exchangeId"] == row["event"]["exchangeId"] && l["marketId"] == row["event"]["marketId"]));
        let paper = if relevant {
            crate::stable_maker_paper_v2::advance_stable_maker(&self.state["paper"],
                &json!({"kind": "trades", "availableAt": available_at, "page": row["event"]}), &self.bank)?
        } else {
            self.state["paper"].clone()
        };
        let mut next = self.state.clone();
        next["paper"] = paper;
        next["tapeSequence"] = row["sequence"].clone();
        next["tapeHash"] = json!(hash);
        self.append(&next, &json!({"kind": "source_page", "availableAt": available_at,
            "tapeSequence": row["sequence"], "tapeHash": hash, "relevant": relevant}), false)
    }
    /// focus()
    pub fn focus(&self) -> Result<V, String> {
        let mut s = self.status()?;
        s["candidateMarketIds"] = if self.allowed()
            && ((self.now)() as f64) < parse_ms(&self.bank["entryExpiresAt"])
        {
            json!(self.bank["legs"].as_array().cloned().unwrap_or_default().iter()
                .map(|l| l["marketId"].clone()).collect::<Vec<_>>())
        } else {
            json!([])
        };
        Ok(s)
    }
    /// onPage(row)
    pub fn on_page(&mut self, row: &V) -> Result<(), String> {
        self.verify(false)?;
        if row["sequence"].as_i64().unwrap_or(0) <= self.state["tapeSequence"].as_i64().unwrap_or(0) {
            need(row["sequence"].as_i64() == Some(self.state["tapeSequence"].as_i64().unwrap_or(0))
                && row["hash"].as_str() == self.state["tapeHash"].as_str(), "DUPLICATE")?;
            return Ok(());
        }
        let at = baal_service_runtime::contract::ms_to_iso(
            (self.now)().max(instant(&self.state["paper"]["lastAt"])?) as f64);
        self.page(row, &at)
    }
    /// onBook(publication, marketId, accountEligibility)
    pub fn on_book(&mut self, publication: &V, market_id: &str, account_eligibility: &V) -> Result<(), String> {
        self.verify(false)?;
        let leg = self.bank["legs"].as_array().cloned().unwrap_or_default().into_iter()
            .find(|l| l["marketId"].as_str() == Some(market_id));
        let Some(leg) = leg else { return Ok(()); };
        let instrument = format!("{}:YES", leg["exchangeId"].as_str().unwrap_or(""));
        let b = publication["books"][&instrument].clone();
        if b.is_null() { return Ok(()); }
        let at = baal_service_runtime::contract::ms_to_iso((self.now)() as f64);
        let paper = crate::stable_maker_paper_v2::advance_stable_maker(&self.state["paper"],
            &json!({"kind": "book", "availableAt": at, "book": b, "accountEligibility": account_eligibility}),
            &self.bank)?;
        let mut next = self.state.clone();
        next["paper"] = paper;
        self.append(&next, &json!({"kind": "book", "availableAt": at,
            "sourceSequence": publication["sequence"], "sourceHash": publication["hash"],
            "marketId": market_id}), false)
    }
    /// boundary(reason)
    pub fn boundary(&mut self, reason: &str) -> Result<(), String> {
        self.verify(true)?;
        let at = baal_service_runtime::contract::ms_to_iso(
            (self.now)().max(instant(&self.state["paper"]["lastAt"])?) as f64);
        let paper = crate::stable_maker_paper_v2::advance_stable_maker(&self.state["paper"],
            &json!({"kind": "boundary", "availableAt": at}), &self.bank)?;
        let mut next = self.state.clone();
        next["paper"] = paper;
        self.append(&next, &json!({"kind": "boundary", "reason": reason, "availableAt": at}), true)
    }
}
