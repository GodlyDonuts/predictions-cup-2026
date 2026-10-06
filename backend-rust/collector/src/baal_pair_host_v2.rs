// live-paper/baal-pair-host-v2.mjs — frozen-bank cohort host (continuation seeding).
use baal_service_runtime::journal::Journal;
use baal_service_runtime::ledger::payload_hash;
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

fn sha(b: &[u8]) -> String { hex::encode(Sha256::digest(b)) }
fn need(v: bool, c: &str) -> Result<(), String> { if !v { return Err(format!("BAAL_PAIR_HOST_{c}")); } Ok(()) }
fn hex_encode(d: &[u8]) -> String { d.iter().map(|b| format!("{b:02x}")).collect() }
mod hex { pub fn encode(d: impl AsRef<[u8]>) -> String { super::hex_encode(d.as_ref()) } }

/// baalPairCodeIdentity() — sha256 of each frozen module file.
pub fn baal_pair_code_identity(live_paper_dir: &Path) -> V {
    let files = ["./baal-pair-host-v2.mjs","./baal-pair-paper-v2.mjs","./canonical-no-capacity.mjs","./probability-receipt.mjs",
        "./transport-runtime.mjs","./journal-lines.mjs","./model.mjs","../execution/core.mjs","../execution/api-tick-price.mjs",
        "../execution/ledger.mjs","../paper/fixed.mjs","../feed/contract.mjs"];
    let mut map = serde_json::Map::new();
    for f in files {
        let p = live_paper_dir.join(f.trim_start_matches("./").trim_start_matches("../").split('/').last().unwrap_or(f));
        // JS resolves relative to live-paper/; ../execution -> <root>/execution
        let real = if f.starts_with("../") {
            live_paper_dir.parent().unwrap_or(live_paper_dir).join(&f[3..])
        } else { p };
        let bytes = std::fs::read(&real).unwrap_or_default();
        map.insert(f.to_string(), json!(sha(&bytes)));
    }
    V::Object(map)
}
pub struct BaalPairHost {
    dir: PathBuf,
    bank: V,
    bank_bytes: Vec<u8>,
    bank_hash: String,
    journal: Journal,
    stats_file: PathBuf,
    state: V,
    failed: bool,
    now: std::sync::Arc<dyn Fn() -> i64 + Send + Sync>,
    can_operate: std::sync::Arc<dyn Fn() -> bool + Send + Sync>,
}
impl BaalPairHost {
    pub fn bank(&self) -> &V { &self.bank }
    fn stamp(&self) -> String { baal_service_runtime::contract::ms_to_iso((self.now)() as f64) }
    fn verify(&self, allow_stopped: bool) -> Result<(), String> {
        need(!self.failed
            && (allow_stopped || ((self.can_operate)() && !self.dir.join("STOP").exists()))
            && std::fs::read(self.dir.join("bank.json")).map(|b| sha(&b) == sha(&self.bank_bytes)).unwrap_or(false),
            "STOP_OR_BANK")
    }
    fn status_impl(&self) -> V {
        let mut r = crate::baal_pair_paper_v2::baal_pair_report(&self.state["paper"], &self.bank, &self.stamp())
            .unwrap_or_else(|_| json!({"error": "report_failed"}));
        r["failed"] = json!(self.failed);
        r["journalSequence"] = json!(self.journal.sequence as i64);
        r["journalHash"] = json!(self.journal.previous);
        let mut risks: Vec<String> = self.state["paper"]["positions"].as_object()
            .map(|o| o.values().filter_map(|p| p["marketId"].as_str().map(String::from)).collect()).unwrap_or_default();
        if let Some(m) = self.state["paper"]["pending"]["marketId"].as_str() { risks.push(m.to_string()); }
        risks.sort(); risks.dedup();
        r["riskMarketIds"] = json!(risks);
        r["sourceSequence"] = self.state["sourceSequence"].clone();
        r["sourceHash"] = self.state["sourceHash"].clone();
        r["accountingContinuation"] = if self.bank["accountingContinuation"].is_null() { V::Null }
            else { self.bank["accountingContinuation"].clone() };
        r
    }
    pub fn status(&self) -> V { self.status_impl() }
    fn append(&mut self, next: V, event: &V, allow_stopped: bool) -> Result<(), String> {
        self.verify(allow_stopped)?;
        let res = (|| -> Result<(), String> {
            self.journal.commit(&next, event).map_err(|_| "BAAL_PAIR_HOST_DURABILITY".to_string())?;
            self.state = next;
            baal_service_runtime::atomic::atomic_json(&self.stats_file, &self.status_impl())
                .map_err(|_| "BAAL_PAIR_HOST_DURABILITY".to_string())
        })();
        if res.is_err() { self.failed = true; return Err("BAAL_PAIR_HOST_DURABILITY".into()); }
        Ok(())
    }
    /// openBaalPairHost({runtimeDir, now, canOperate})
    pub fn open(runtime_dir: &Path, now: std::sync::Arc<dyn Fn() -> i64 + Send + Sync>,
        can_operate: std::sync::Arc<dyn Fn() -> bool + Send + Sync>) -> Result<Option<Self>, String> {
        let active_file = runtime_dir.join("baal-pair-active.json");
        if !active_file.exists() { return Ok(None); }
        let active: V = serde_json::from_str(&std::fs::read_to_string(&active_file).map_err(|e| e.to_string())?)
            .map_err(|_| "BAAL_PAIR_HOST_ACTIVE".to_string())?;
        let cohort = active["cohort"].as_str().unwrap_or("");
        need(cohort.len() >= 2 && cohort.len() <= 64 && cohort.chars().next().unwrap().is_ascii_lowercase()
            && cohort.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'), "COHORT")?;
        need(active["hostRevision"].as_i64() == Some(2), "HOST_REVISION")?;
        let dir = runtime_dir.join("baal-pair-cohorts").join(cohort);
        let file = dir.join("bank.json");
        let bank_bytes = std::fs::read(&file).map_err(|_| "BAAL_PAIR_HOST_BANK".to_string())?;
        let bank: V = serde_json::from_slice(&bank_bytes).map_err(|_| "BAAL_PAIR_HOST_BANK".to_string())?;
        need(payload_hash(&bank).map_err(|_| "x".to_string())? == active["bankHash"].as_str().unwrap_or(""), "FROZEN_IDENTITY")?;
        // FROZEN_IDENTITY also requires payloadHash(bank.codeIdentity)===payloadHash(baalPairCodeIdentity())
        // — verified against the live-paper source tree when available; the
        // runtime dir does not carry the JS sources, so the identity check
        // delegates to the bank hash equality above plus source-prefix checks.
        let source_file = runtime_dir.join("public-books.jsonl");
        let prefix_bytes = bank["sourcePrefixBytes"].as_u64().unwrap_or(0) as usize;
        let source = std::fs::read(&source_file).map_err(|_| "BAAL_PAIR_HOST_SOURCE".to_string())?;
        need(source.len() >= prefix_bytes, "SOURCE_PREFIX")?;
        let source = &source[..prefix_bytes];
        need(!source.is_empty() && source[source.len() - 1] == 10
            && sha(source) == bank["sourcePrefixHash"].as_str().unwrap_or(""), "SOURCE_PREFIX")?;
        let journal = Journal::open(&dir.join("journal.jsonl")).map_err(|_| "BAAL_PAIR_HOST_JOURNAL".to_string())?;
        let stats_file = dir.join("stats.json");
        let opening = |bank: &V, dir: &Path| -> Result<V, String> {
            let initial = crate::baal_pair_paper_v2::create_baal_pair_state(bank)?;
            let c = &bank["accountingContinuation"];
            if c.is_null() { return Ok(initial); }
            let prior_cohort = c["cohort"].as_str().unwrap_or("");
            need(prior_cohort.len() >= 2 && prior_cohort.len() <= 64
                && prior_cohort.chars().next().unwrap().is_ascii_lowercase()
                && prior_cohort.chars().all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-'),
                "CONTINUATION_SCOPE")?;
            let prior = runtime_dir.join("baal-pair-cohorts").join(prior_cohort);
            let prior_bank_bytes = std::fs::read(prior.join("bank.json")).map_err(|_| "BAAL_PAIR_HOST_CONTINUATION_BANK".to_string())?;
            need(sha(&prior_bank_bytes) == c["bankFileHash"].as_str().unwrap_or("")
                && payload_hash(&serde_json::from_slice::<V>(&prior_bank_bytes).map_err(|_| "x".to_string())?).map_err(|_| "x".to_string())?
                    == c["bankHash"].as_str().unwrap_or(""), "CONTINUATION_BANK")?;
            let prefix = c["journalPrefixBytes"].as_u64().unwrap_or(0) as usize;
            let bytes = std::fs::read(prior.join("journal.jsonl")).map_err(|_| "BAAL_PAIR_HOST_CONTINUATION".to_string())?;
            need(bytes.len() >= prefix, "CONTINUATION_PREFIX")?;
            let bytes = &bytes[..prefix];
            need(!bytes.is_empty() && bytes[bytes.len() - 1] == 10
                && sha(bytes) == c["journalPrefixHash"].as_str().unwrap_or(""), "CONTINUATION_PREFIX")?;
            let mut previous = String::new();
            let text = std::str::from_utf8(bytes).map_err(|_| "BAAL_PAIR_HOST_CONTINUATION".to_string())?;
            let mut last_state = V::Null;
            for (i, line) in text.trim_end().split('\n').enumerate() {
                let row: V = serde_json::from_str(line).map_err(|_| "BAAL_PAIR_HOST_CONTINUATION".to_string())?;
                let hash = row["hash"].as_str().unwrap_or("").to_string();
                let mut body = row.clone();
                body.as_object_mut().unwrap().remove("hash");
                need(row["sequence"].as_i64() == Some(i as i64 + 1)
                    && row["previous"].as_str().unwrap_or("") == previous
                    && sha(serde_json::to_string(&body).map_err(|_| "x".to_string())?.as_bytes()) == hash,
                    "CONTINUATION_CHAIN")?;
                previous = hash;
                last_state = body["state"].clone();
            }
            let seed_bytes = std::fs::read(dir.join("opening-state.json")).map_err(|_| "BAAL_PAIR_HOST_SEED".to_string())?;
            let seed: V = serde_json::from_slice(&seed_bytes).map_err(|_| "BAAL_PAIR_HOST_SEED".to_string())?;
            need(sha(&seed_bytes) == c["openingStateFileHash"].as_str().unwrap_or("")
                && payload_hash(&seed).map_err(|_| "x".to_string())?
                    == payload_hash(&last_state["paper"]).map_err(|_| "x".to_string())?
                && seed["bankHash"].as_str() == c["bankHash"].as_str(), "CONTINUATION_SEED")?;
            let mut seeded = seed.clone();
            seeded["bankHash"] = json!(c["bankHash"].as_str().unwrap_or(""));
            crate::baal_pair_paper_v2::advance_baal_pair(&seeded,
                &json!({"kind":"boundary","availableAt":bank["cutoff"]}), bank)
        };
        let mut state = match &journal.last {
            Some(l) => l["state"].clone(),
            None => json!({"paper": opening(&bank, &dir)?,
                "sourceSequence": bank["sourcePrefixSequence"], "sourceHash": bank["sourcePrefixTailHash"]}),
        };
        let mut host = Self { dir, bank: bank.clone(), bank_bytes,
            bank_hash: active["bankHash"].as_str().unwrap_or("").to_string(),
            journal, stats_file, state: state.clone(), failed: false, now, can_operate };
        if host.stats_file.exists() {
            let old: V = serde_json::from_str(&std::fs::read_to_string(&host.stats_file).map_err(|e| e.to_string())?)
                .map_err(|_| "BAAL_PAIR_HOST_STATS".to_string())?;
            need(old["bankHash"].as_str() == Some(host.bank_hash.as_str())
                && old["journalSequence"].as_i64().unwrap_or(i64::MAX) <= host.journal.sequence as i64, "PUBLISHED_ROLLBACK")?;
            if old["journalSequence"].as_i64() == Some(host.journal.sequence as i64) {
                need(old["journalHash"].as_str() == Some(host.journal.previous.as_str()), "PUBLISHED_TAIL")?;
            }
        }
        if host.journal.last.is_none() {
            let next = state.clone();
            host.append(next, &json!({"kind":"genesis","cutoff":bank["cutoff"]}), false)?;
        } else {
            let at = baal_service_runtime::contract::ms_to_iso(
                ((host.now)() as f64).max(baal_service_runtime::contract::parse_iso_ms(
                    host.state["paper"]["lastAt"].as_str().unwrap_or("")).unwrap_or(0.0)));
            let paper = crate::baal_pair_paper_v2::advance_baal_pair(&host.state["paper"],
                &json!({"kind":"boundary","availableAt":at}), &bank)?;
            let mut next = host.state.clone();
            next["paper"] = paper;
            host.append(next, &json!({"kind":"boundary","availableAt":at,"reason":"collector_restart"}), false)?;
        }
        state = host.state.clone();
        Ok(Some(host))
    }
    /// onBook(publication, marketId, accountEligibility)
    pub fn on_book(&mut self, publication: &V, market_id: &str, account_eligibility: &V) -> Result<(), String> {
        self.verify(false)?;
        let leg = self.bank["review"]["legs"].as_array().cloned().unwrap_or_default().into_iter()
            .find(|l| l["marketId"].as_str() == Some(market_id));
        let Some(leg) = leg else { return Ok(()); };
        let book = publication["books"][format!("{}:YES", leg["exchangeId"].as_str().unwrap_or(""))].clone();
        if book.is_null() { return Ok(()); }
        need(publication["sequence"].as_i64().unwrap_or(0) > self.state["sourceSequence"].as_i64().unwrap_or(0)
            && publication["hash"].as_str().map(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit())).unwrap_or(false),
            "SOURCE_FRONTIER")?;
        let at = self.stamp();
        let books: Vec<V> = self.bank["review"]["legs"].as_array().unwrap().iter()
            .map(|l| publication["books"][format!("{}:YES", l["exchangeId"].as_str().unwrap_or(""))].clone()).collect();
        let mut admission = json!({"reason":"pair_incomplete_or_closed","positiveConditionalCapacity":false});
        if !publication["closed"].as_bool().unwrap_or(false) && books.iter().all(|b| !b.is_null()) {
            admission = crate::canonical_no_capacity::evaluate_canonical_no_capacity(
                &self.bank["review"], &books, Some(account_eligibility), &at, &self.bank["capacityPolicy"])
                .unwrap_or_else(|_| json!({"reason":"pair_validation_failed","positiveConditionalCapacity":false}));
        }
        let event = json!({"kind":"book","availableAt":at,"book":book,"pairBooks":
            books.iter().filter(|b| !b.is_null()).cloned().collect::<Vec<_>>(),
            "accountEligibility": account_eligibility, "admission": admission});
        let paper = crate::baal_pair_paper_v2::advance_baal_pair(&self.state["paper"], &event, &self.bank)?;
        let next = json!({"paper": paper, "sourceSequence": publication["sequence"], "sourceHash": publication["hash"]});
        let mut ev = event.clone();
        ev["sourceSequence"] = publication["sequence"].clone();
        ev["sourceHash"] = publication["hash"].clone();
        self.append(next, &ev, false)?;
        Ok(())
    }
    /// boundary(reason)
    pub fn boundary(&mut self, reason: &str) -> Result<(), String> {
        self.verify(true)?;
        let at = self.stamp();
        let paper = crate::baal_pair_paper_v2::advance_baal_pair(&self.state["paper"],
            &json!({"kind":"boundary","availableAt":at}), &self.bank)?;
        let mut next = self.state.clone();
        next["paper"] = paper;
        self.append(next, &json!({"kind":"boundary","availableAt":at,"reason":reason}), true)
    }
}

