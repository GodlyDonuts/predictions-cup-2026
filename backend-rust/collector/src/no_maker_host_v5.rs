// live-paper/no-maker-host-v5.mjs — durable no-maker host v5: projection recovery + accounting continuation frontier.
use baal_execution::core::payload_hash;
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::{digest, js_truthy, sha256};
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::no_maker_paper_v3::{advance_no_maker, create_no_maker_state, no_maker_report};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok {
        return Err(format!("NO_MAKER_HOST_{}", code));
    }
    Ok(())
}
fn js_eq(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Null, V::Null) => true,
        (V::Bool(x), V::Bool(y)) => x == y,
        (V::Number(_), V::Number(_)) => a.as_f64() == b.as_f64(),
        (V::String(x), V::String(y)) => x == y,
        _ => false,
    }
}
fn js_string(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Number(n) => {
            let x = n.as_f64().unwrap_or(0.0);
            if x.fract() == 0.0 && x.is_finite() && x.abs() < 1e21 {
                format!("{}", x as i64)
            } else {
                format!("{}", x)
            }
        }
        V::Bool(b) => b.to_string(),
        V::Null => "null".into(),
        _ => "[object Object]".into(),
    }
}
fn jnum(v: &V) -> f64 {
    match v {
        V::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        V::String(s) => s.trim().parse::<f64>().unwrap_or(f64::NAN),
        V::Bool(b) => i32::from(*b) as f64,
        V::Null => 0.0,
        _ => f64::NAN,
    }
}
// JS `a + b`: string concat when either side is a string, else numeric add.
fn js_plus(a: &V, b: &V) -> V {
    match (a, b) {
        (V::String(x), y) => json!(x.clone() + &js_string(y)),
        (x, V::String(y)) => json!(js_string(x) + y),
        _ => json!(jnum(a) + jnum(b)),
    }
}
fn js_keys(v: &V) -> Vec<String> {
    fn array_index(k: &str) -> Option<u64> {
        if k.is_empty() || k.len() > 10 || !k.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        if k.len() > 1 && k.starts_with('0') {
            return None;
        }
        let n: u64 = k.parse().ok()?;
        if n < 4_294_967_295 {
            Some(n)
        } else {
            None
        }
    }
    let m = match v.as_object() {
        Some(m) => m,
        None => return vec![],
    };
    let mut ints: Vec<(u64, String)> = vec![];
    let mut rest: Vec<String> = vec![];
    for k in m.keys() {
        match array_index(k) {
            Some(n) => ints.push((n, k.clone())),
            None => rest.push(k.clone()),
        }
    }
    ints.sort_by_key(|x| x.0);
    ints.into_iter().map(|x| x.1).chain(rest).collect()
}
fn js_values(v: &V) -> Vec<V> { js_keys(v).into_iter().map(|k| v[&k].clone()).collect() }
// /^[a-z][a-z0-9-]{1,63}$/
fn cohort_ok(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 2
        && b.len() <= 64
        && b[0].is_ascii_lowercase()
        && b[1..].iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}
// new Date(ms).toISOString() — throws on NaN/invalid (RangeError in JS).
fn to_iso(ms: f64) -> Result<String, String> {
    if !ms.is_finite() {
        return Err("Invalid time value".into());
    }
    Ok(ms_to_iso(ms))
}
// Math.max — NaN propagates like JS (f64::max does not).
fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.max(b)
    }
}

/// noMakerCodeIdentityV5(sourceDir) — sha256 of each listed module file.
/// `source_dir` is the live-paper/ directory (the JS `import.meta.url` base).
pub fn no_maker_code_identity_v5(source_dir: &Path) -> Result<V, String> {
    let mut out = serde_json::Map::new();
    for f in [
        "./no-maker-host-v5.mjs",
        "./no-maker-paper-v3.mjs",
        "./canonical-entry-exit-study.mjs",
        "./canonical-no-capacity-v2.mjs",
        "./canonical-no-capacity.mjs",
        "./quote-projection-recovery.mjs",
        "./probability-receipt.mjs",
        "./model.mjs",
        "./transport-runtime.mjs",
        "./journal-lines.mjs",
        "../execution/api-tick-price.mjs",
        "../execution/core.mjs",
        "../execution/ledger.mjs",
        "../paper/fixed.mjs",
        "../feed/contract.mjs",
    ] {
        let bytes = std::fs::read(source_dir.join(f)).map_err(|e| e.to_string())?;
        out.insert(f.to_string(), json!(sha256(&bytes)));
    }
    Ok(V::Object(out))
}

struct NoMakerInner {
    journal: Journal,
    state: V,
    failed: bool,
}
/// {bank,dir,status,onPage,onBook,boundary} — see openNoMakerHostV5.
pub struct NoMakerHostV5 {
    pub bank: V,
    pub dir: PathBuf,
    inner: Mutex<NoMakerInner>,
    now: Arc<dyn Fn() -> i64 + Send + Sync>,
    can_operate: Arc<dyn Fn() -> bool + Send + Sync>,
    bank_file: PathBuf,
    bank_bytes: Vec<u8>,
    stats_file: PathBuf,
    stop_file: PathBuf,
}
impl NoMakerHostV5 {
    fn stopped(&self) -> bool { !(self.can_operate)() || self.stop_file.exists() }
    fn verify(&self, allow_stopped: bool) -> Result<(), String> {
        let inner = self.inner.lock().unwrap();
        need(!inner.failed && (allow_stopped || !self.stopped()), "STOPPED")?;
        let current = std::fs::read(&self.bank_file).map_err(|e| e.to_string())?;
        need(sha256(&current) == sha256(&self.bank_bytes), "BANK_CHANGED")?;
        Ok(())
    }
    fn status_of(&self, inner: &NoMakerInner) -> Result<V, String> {
        let at = to_iso((self.now)() as f64)?;
        let mut st = no_maker_report(&inner.state["paper"], &self.bank, &at)?;
        st["failed"] = json!(inner.failed);
        st["hostRevision"] = json!(5);
        st["accountingContinuation"] = if js_truthy(self.bank.get("accountingContinuation")) {
            self.bank["accountingContinuation"].clone()
        } else {
            V::Null
        };
        st["lastAdmission"] = if js_truthy(inner.state.get("lastAdmission")) {
            inner.state["lastAdmission"].clone()
        } else {
            V::Null
        };
        st["journalSequence"] = json!(inner.journal.sequence);
        st["journalHash"] = json!(inner.journal.previous);
        st["tapeSequence"] = inner.state["tapeSequence"].clone();
        let at2 = to_iso((self.now)() as f64)?;
        st["projectionRecoveryRequests"] = json!(
            crate::quote_projection_recovery::quote_projection_recovery_requests(
                &inner.state["paper"],
                &self.bank,
                crate::model::instant(&at2)?,
            )
        );
        let mut risk: Vec<V> = Vec::new();
        for w in js_values(&inner.state["paper"]["wallets"]) {
            for p in js_values(&w["positions"]) {
                if !risk.contains(&p["marketId"]) {
                    risk.push(p["marketId"].clone());
                }
            }
            for q in js_values(&w["quotes"]) {
                if !js_truthy(q.get("resolvedAt")) && !risk.contains(&q["marketId"]) {
                    risk.push(q["marketId"].clone());
                }
            }
        }
        st["riskMarketIds"] = json!(risk);
        Ok(st)
    }
    fn append(&self, next: V, event: &V, allow_stopped: bool) -> Result<(), String> {
        self.verify(allow_stopped)?;
        let result = (|| -> Result<(), String> {
            let mut inner = self.inner.lock().unwrap();
            inner.journal.commit(&next, event)?;
            inner.state = next;
            let status = self.status_of(&inner)?;
            atomic_json(&self.stats_file, &status)?;
            Ok(())
        })();
        match result {
            Ok(()) => Ok(()),
            Err(_) => {
                self.inner.lock().unwrap().failed = true;
                Err("NO_MAKER_HOST_DURABILITY".into())
            }
        }
    }
    fn page(&self, row: &V, available_at: &str) -> Result<(), String> {
        let (tape_seq, tape_hash, mut next) = {
            let inner = self.inner.lock().unwrap();
            (
                inner.state["tapeSequence"].clone(),
                inner.state["tapeHash"].clone(),
                inner.state.clone(),
            )
        };
        need(
            js_eq(&row["sequence"], &js_plus(&tape_seq, &json!(1))) && js_eq(&row["previous"], &tape_hash),
            "TAPE_FRONTIER",
        )?;
        let hash = row["hash"].clone();
        let mut body = row.clone();
        del(&mut body, "hash");
        need(digest(&body) == js_string(&hash), "TAPE_HASH")?;
        let e = row["event"].clone();
        let relevant = self.bank["legs"].as_array().cloned().unwrap_or_default().iter().any(|l| {
            js_eq(&l["marketId"], &e["marketId"]) && js_eq(&l["exchangeId"], &e["exchangeId"])
        });
        let paper = next["paper"].clone();
        next["paper"] = if relevant {
            advance_no_maker(&paper, &json!({"kind": "trades", "availableAt": available_at, "page": e}), &self.bank)?
        } else {
            paper
        };
        next["tapeSequence"] = row["sequence"].clone();
        next["tapeHash"] = hash;
        self.append(
            next,
            &json!({"kind": if relevant { "trade_page" } else { "other_trade_page" },
                "availableAt": available_at, "tapeSequence": row["sequence"], "tapeHash": row["hash"],
                "event": if relevant { e } else { V::Null }}),
            false,
        )
    }
    /// status()
    pub fn status(&self) -> Result<V, String> {
        let inner = self.inner.lock().unwrap();
        self.status_of(&inner)
    }
    /// onPage(row)
    pub fn on_page(&self, row: &V) -> Result<(), String> {
        self.verify(false)?;
        let (tape_seq, tape_hash) = {
            let inner = self.inner.lock().unwrap();
            (inner.state["tapeSequence"].clone(), inner.state["tapeHash"].clone())
        };
        if jnum(&row["sequence"]) <= jnum(&tape_seq) {
            need(
                js_eq(&row["sequence"], &tape_seq) && js_eq(&row["hash"], &tape_hash),
                "SOURCE_DUPLICATE",
            )?;
            return Ok(());
        }
        let at = to_iso((self.now)() as f64)?;
        self.page(row, &at)
    }
    /// onBook(publication, marketId, accountEligibility)
    pub fn on_book(&self, publication: &V, market_id: &V, account_eligibility: &V) -> Result<(), String> {
        self.verify(false)?;
        let leg = self.bank["legs"].as_array().cloned().unwrap_or_default().into_iter().find(|l| {
            js_eq(&l["marketId"], market_id)
        });
        let leg = match leg {
            Some(l) => l,
            None => return Ok(()),
        };
        let b = publication["books"][format!("{}:YES", js_string(&leg["exchangeId"]))].clone();
        if !js_truthy(Some(&b)) {
            return Ok(());
        }
        let at = to_iso((self.now)() as f64)?;
        let books: Vec<V> = self.bank["legs"].as_array().cloned().unwrap_or_default().iter()
            .map(|l| publication["books"][format!("{}:YES", js_string(&l["exchangeId"]))].clone())
            .collect();
        let mut entry_allowed = false;
        // `reason` is undefined-able in JS: absent key serializes as nothing, not null.
        let mut reason: Option<V> = Some(json!("pair_incomplete_or_closed"));
        if !js_truthy(publication.get("closed")) && books.iter().all(js_truthy_on) {
            match crate::canonical_no_capacity_v2::evaluate_canonical_no_capacity(
                &self.bank["review"],
                &books,
                Some(account_eligibility),
                &at,
                &self.bank["capacityPolicy"],
            ) {
                Ok(frame) => {
                    entry_allowed = js_truthy(frame.get("positiveConditionalCapacity"));
                    reason = frame.get("reason").cloned();
                }
                Err(_) => reason = Some(json!("pair_validation_failed")),
            }
        }
        // A coherent pair becomes knowable at this consumer clock. Reconsider both
        // actual books, in canonical leg order, without changing their request,
        // receipt, version or publication clocks. A stale/incomplete pair only
        // advances the incoming leg for any already-owned risk bookkeeping.
        let evaluated: Vec<V> = if entry_allowed { books.clone() } else { vec![b.clone()] };
        let next = {
            let inner = self.inner.lock().unwrap();
            let mut paper = inner.state["paper"].clone();
            for book in &evaluated {
                paper = advance_no_maker(
                    &paper,
                    &json!({"kind": "book", "availableAt": at, "book": book,
                        "accountEligibility": account_eligibility, "entryAllowed": entry_allowed}),
                    &self.bank,
                )?;
            }
            let mut next = inner.state.clone();
            next["paper"] = paper;
            let mut adm = serde_json::Map::new();
            adm.insert("at".into(), json!(at));
            if let Some(r) = &reason {
                adm.insert("reason".into(), r.clone());
            }
            adm.insert("entryAllowed".into(), json!(entry_allowed));
            adm.insert("marketIds".into(), json!(evaluated.iter().map(|x| x["marketId"].clone()).collect::<Vec<V>>()));
            adm.insert("incomingMarketId".into(), market_id.clone());
            next["lastAdmission"] = V::Object(adm);
            next
        };
        let mut ev = serde_json::Map::new();
        ev.insert("kind".into(), json!(if entry_allowed { "paired_book" } else { "book" }));
        ev.insert("availableAt".into(), json!(at));
        ev.insert("books".into(), json!(evaluated));
        ev.insert("accountEligibility".into(), account_eligibility.clone());
        ev.insert("entryAllowed".into(), json!(entry_allowed));
        ev.insert("sourceSequence".into(), publication["sequence"].clone());
        ev.insert("sourceHash".into(), publication["hash"].clone());
        ev.insert("incomingMarketId".into(), market_id.clone());
        if let Some(r) = &reason {
            ev.insert("reason".into(), r.clone());
        }
        self.append(next, &V::Object(ev), false)
    }
    /// boundary(reason)
    pub fn boundary(&self, reason: &V) -> Result<(), String> {
        self.verify(true)?;
        let at = to_iso((self.now)() as f64)?;
        let next = {
            let inner = self.inner.lock().unwrap();
            let mut next = inner.state.clone();
            next["paper"] = advance_no_maker(
                &inner.state["paper"],
                &json!({"kind": "boundary", "availableAt": at}),
                &self.bank,
            )?;
            next
        };
        self.append(
            next,
            &json!({"kind": "boundary", "availableAt": at, "reason": reason}),
            true,
        )
    }
}
fn del(v: &mut V, k: &str) {
    if let Some(m) = v.as_object_mut() {
        m.remove(k);
    }
}
fn js_truthy_on(v: &V) -> bool { js_truthy(Some(v)) }

/// openingPaper() — accounting continuation seed validation + boundary advance.
fn opening_paper(runtime_dir: &Path, dir: &Path, bank: &V, active: &V) -> Result<V, String> {
    let initial = create_no_maker_state(bank)?;
    let c = &bank["accountingContinuation"];
    if !js_truthy(Some(c)) {
        return Ok(initial);
    }
    let cohort = c["cohort"].as_str().unwrap_or("");
    need(cohort_ok(cohort), "CONTINUATION_SCOPE")?;
    let predecessor = runtime_dir.join("no-maker-cohorts").join(cohort);
    let prior_bank_bytes = std::fs::read(predecessor.join("bank.json")).map_err(|e| e.to_string())?;
    let prior_bank: V = serde_json::from_slice(&prior_bank_bytes).map_err(|e| e.to_string())?;
    need(
        sha256(&prior_bank_bytes) == js_string(&c["bankFileHash"])
            && js_eq(&json!(payload_hash(&prior_bank)?), &c["bankHash"]),
        "CONTINUATION_BANK",
    )?;
    let n = jnum(&c["journalPrefixBytes"]).max(0.0) as usize;
    let raw_journal = std::fs::read(predecessor.join("journal.jsonl")).map_err(|e| e.to_string())?;
    let prior_journal = &raw_journal[..raw_journal.len().min(n)];
    need(
        prior_journal.len() == n && !prior_journal.is_empty() && *prior_journal.last().unwrap() == 10
            && sha256(prior_journal) == js_string(&c["journalPrefixHash"]),
        "CONTINUATION_JOURNAL",
    )?;
    let mut prior_rows: Vec<V> = Vec::new();
    for line in String::from_utf8_lossy(prior_journal).trim_end().split('\n') {
        prior_rows.push(serde_json::from_str(line).map_err(|e| e.to_string())?);
    }
    let mut previous = String::new();
    for (i, row) in prior_rows.iter().enumerate() {
        let hash = row["hash"].clone();
        let mut body = row.clone();
        del(&mut body, "hash");
        need(
            js_eq(&row["sequence"], &json!(i as i64 + 1))
                && js_eq(&row["previous"], &json!(previous))
                && digest(&body) == js_string(&hash),
            "CONTINUATION_CHAIN",
        )?;
        previous = js_string(&hash);
    }
    // The capture prefix must be exactly the frontier consumed by the carried wallet.
    // Otherwise a committed source page could disappear between generations.
    let last = prior_rows
        .last()
        .cloned()
        .ok_or_else(|| "Cannot read properties of undefined (reading 'state')".to_string())?;
    need(
        js_eq(&last["state"]["tapeSequence"], &bank["tapePrefixSequence"])
            && js_eq(&last["state"]["tapeHash"], &bank["tapePrefixTailHash"]),
        "CONTINUATION_FRONTIER",
    )?;
    let seed_bytes = std::fs::read(dir.join("opening-state.json")).map_err(|e| e.to_string())?;
    let seed: V = serde_json::from_slice(&seed_bytes).map_err(|e| e.to_string())?;
    need(
        sha256(&seed_bytes) == js_string(&c["openingStateFileHash"])
            && js_eq(&json!(payload_hash(&seed)?), &json!(payload_hash(&last["state"]["paper"])?))
            && js_eq(&seed["bankHash"], &c["bankHash"]),
        "CONTINUATION_SEED",
    )?;
    let mut paper = initial.clone();
    if let (Some(p), Some(s)) = (paper.as_object_mut(), seed.as_object()) {
        for (k, v) in s {
            p.insert(k.clone(), v.clone());
        }
    }
    paper["bankHash"] = active["bankHash"].clone();
    paper["coverage"] = json!({});
    for wk in js_keys(&paper["wallets"]) {
        for qk in js_keys(&paper["wallets"][&wk]["quotes"]) {
            let mut q = paper["wallets"][&wk]["quotes"][&qk].clone();
            if !js_truthy(q.get("resolvedAt")) {
                q["coverageBarrierSequence"] = V::Null;
            }
            paper["wallets"][&wk]["quotes"][&qk] = q;
        }
    }
    advance_no_maker(&paper, &json!({"kind": "boundary", "availableAt": bank["cutoff"]}), bank)
}

/// openNoMakerHostV5({runtimeDir, now, canOperate, sourceDir})
/// `source_dir` is the live-paper/ module directory used by noMakerCodeIdentityV5.
/// `now` yields epoch milliseconds (JS `Date.now`).
pub fn open_no_maker_host_v5(
    runtime_dir: &Path,
    source_dir: &Path,
    now: Arc<dyn Fn() -> i64 + Send + Sync>,
    can_operate: Arc<dyn Fn() -> bool + Send + Sync>,
) -> Result<Option<NoMakerHostV5>, String> {
    let active_file = runtime_dir.join("no-maker-active.json");
    if !active_file.exists() {
        return Ok(None);
    }
    let active: V = serde_json::from_slice(&std::fs::read(&active_file).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let cohort = active["cohort"].as_str().unwrap_or("");
    need(cohort_ok(cohort), "COHORT")?;
    let dir = runtime_dir.join("no-maker-cohorts").join(cohort);
    let bank_file = dir.join("bank.json");
    let bank_bytes = std::fs::read(&bank_file).map_err(|e| e.to_string())?;
    let bank: V = serde_json::from_slice(&bank_bytes).map_err(|e| e.to_string())?;
    need(
        js_eq(&active["hostRevision"], &json!(5)) && js_eq(&bank["hostRevision"], &json!(5)),
        "HOST_REVISION",
    )?;
    need(
        js_eq(&active["bankHash"], &json!(payload_hash(&bank)?))
            && js_eq(&json!(payload_hash(&no_maker_code_identity_v5(source_dir)?)), &json!(payload_hash(&bank["codeIdentity"])?)),
        "FROZEN_IDENTITY",
    )?;
    let tape_file = runtime_dir.join("anonymous-trades.jsonl");
    let tape_prefix_bytes = jnum(&bank["tapePrefixBytes"]).max(0.0) as usize;
    let all = std::fs::read(&tape_file).map_err(|e| e.to_string())?;
    let prefix = &all[..all.len().min(tape_prefix_bytes)];
    need(
        prefix.len() == tape_prefix_bytes && (prefix.is_empty() || *prefix.last().unwrap() == 10)
            && sha256(prefix) == bank["tapePrefixHash"].as_str().unwrap_or(""),
        "TAPE_PREFIX",
    )?;
    let journal = Journal::open(&dir.join("journal.jsonl"))?;
    let stats_file = dir.join("stats.json");
    let stop_file = dir.join("STOP");
    let state = match journal.last.as_ref().map(|l| l["state"].clone()) {
        Some(s) if js_truthy(Some(&s)) => s,
        _ => json!({"paper": opening_paper(runtime_dir, &dir, &bank, &active)?,
            "tapeSequence": bank["tapePrefixSequence"], "tapeHash": bank["tapePrefixTailHash"]}),
    };
    need(js_eq(&state["paper"]["bankHash"], &active["bankHash"]), "STATE_BANK")?;
    if stats_file.exists() {
        let old: V = serde_json::from_slice(&std::fs::read(&stats_file).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        need(
            js_eq(&old["bankHash"], &active["bankHash"])
                && jnum(&old["journalSequence"]) <= journal.sequence as f64,
            "PUBLISHED_JOURNAL_ROLLBACK",
        )?;
        if jnum(&old["journalSequence"]) == journal.sequence as f64 {
            need(js_eq(&old["journalHash"], &json!(journal.previous)), "PUBLISHED_JOURNAL_TAIL")?;
        }
    }
    let host = NoMakerHostV5 {
        bank: bank.clone(),
        dir: dir.clone(),
        inner: Mutex::new(NoMakerInner { journal, state, failed: false }),
        now: now.clone(),
        can_operate,
        bank_file,
        bank_bytes,
        stats_file,
        stop_file,
    };
    // If the source page committed just before a crash, resume it before creating
    // another quote. Original receipt time is retained in the source binding.
    need(all.is_empty() || *all.last().unwrap() == 10, "PARTIAL_SOURCE")?;
    let mut tape_previous = String::new();
    let mut sequence = 0i64;
    for line in String::from_utf8_lossy(&all).trim_end().split('\n').filter(|l| !l.is_empty()) {
        let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
        sequence += 1;
        let hash = row["hash"].clone();
        let mut body = row.clone();
        del(&mut body, "hash");
        need(
            js_eq(&row["sequence"], &json!(sequence))
                && js_eq(&row["previous"], &json!(tape_previous))
                && digest(&body) == js_string(&hash),
            "SOURCE_CHAIN",
        )?;
        tape_previous = js_string(&hash);
        let tape_seq = host.inner.lock().unwrap().state["tapeSequence"].clone();
        if js_eq(&row["sequence"], &tape_seq) {
            need(js_eq(&row["hash"], &host.inner.lock().unwrap().state["tapeHash"]), "SOURCE_FRONTIER_CHANGED")?;
        }
        if jnum(&row["sequence"]) > jnum(&tape_seq) {
            let last_at = host.inner.lock().unwrap().state["paper"]["lastAt"].clone();
            let parsed = parse_iso_ms(last_at.as_str().unwrap_or("")).unwrap_or(f64::NAN);
            let at = to_iso(js_max((host.now)() as f64, parsed))?;
            host.page(&row, &at)?;
        }
    }
    {
        let inner = host.inner.lock().unwrap();
        need(sequence as f64 >= jnum(&inner.state["tapeSequence"]), "SOURCE_TRUNCATED")?;
        if inner.journal.last.is_none() {
            drop(inner);
            host.append(
                host.inner.lock().unwrap().state.clone(),
                &json!({"kind": "genesis", "cutoff": bank["cutoff"]}),
                false,
            )?;
        }
    }
    Ok(Some(host))
}
