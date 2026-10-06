// live-paper/canonical-capacity-monitor.mjs — canonical-capacity cohort monitor.
// The JS resolves capacityCodeIdentity() files relative to import.meta.url (the
// live-paper directory); the port takes that directory as a parameter.
use baal_service_runtime::json_text::{js_int, js_stringify, js_truthy, sha256};
use serde_json::{json, Map, Value as V};
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn need(ok: bool, code: &str) -> Result<(), String> { if !ok { return Err(format!("CAPACITY_MONITOR_{}", code)); } Ok(()) }
fn digest(v: &V) -> String { crate::model::digest(v) }
fn instant(v: &V) -> Result<i64, String> { baal_service_runtime::fixed::instant(v.as_str().unwrap_or("")) }
fn iso_or_err(ms: f64) -> Result<String, String> {
    if !ms.is_finite() { return Err("Invalid time value".into()); }
    let s = baal_service_runtime::contract::ms_to_iso(ms);
    if s.is_empty() { Err("Invalid time value".into()) } else { Ok(s) }
}
// JS === (object identity for containers — separate parses never match).
fn strict_eq(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Null, V::Null) => true,
        (V::Bool(x), V::Bool(y)) => x == y,
        (V::Number(_), V::Number(_)) => a.as_f64() == b.as_f64(),
        (V::String(x), V::String(y)) => x == y,
        _ => false,
    }
}
// JS String(v) coercion for concatenation sites.
fn js_str(v: &V) -> String {
    match v {
        V::Null => "null".into(),
        V::Bool(b) => b.to_string(),
        V::String(s) => s.clone(),
        V::Object(_) => "[object Object]".into(),
        V::Array(a) => a.iter().map(js_str).collect::<Vec<_>>().join(","),
        other => js_stringify(other),
    }
}
fn js_str_opt(v: Option<&V>) -> String { v.map(js_str).unwrap_or_else(|| "undefined".into()) }
// JS Number(v) coercion; absent keys are undefined -> NaN.
fn js_num(v: &V) -> f64 {
    match v {
        V::Null => 0.0,
        V::Bool(b) => if *b { 1.0 } else { 0.0 },
        V::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        V::String(s) => { let t = s.trim(); if t.is_empty() { 0.0 } else { t.parse::<f64>().unwrap_or(f64::NAN) } }
        _ => f64::NAN,
    }
}
fn js_num_opt(v: Option<&V>) -> f64 { v.map(js_num).unwrap_or(f64::NAN) }
// JS property-key coercion for `counts[reason]`.
fn js_key(v: Option<&V>) -> String { v.map(js_str).unwrap_or_else(|| "undefined".into()) }
// /^[a-z][a-z0-9-]{1,63}$/
fn cohort_ok(s: &str) -> bool {
    let b = s.as_bytes();
    s.len() >= 2 && s.len() <= 64 && b[0].is_ascii_lowercase()
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// capacityCodeIdentity() — sha256 of each implementation file, keyed by the
/// literal relative path the JS uses (resolved against the live-paper dir).
pub fn capacity_code_identity(live_paper_dir: &Path) -> Result<V, String> {
    const FILES: [&str; 7] = ["./canonical-no-capacity.mjs", "./canonical-capacity-monitor.mjs",
        "./probability-receipt.mjs", "./model.mjs", "../paper/fixed.mjs",
        "../feed/contract.mjs", "../execution/ledger.mjs"];
    let mut out = Map::new();
    for p in FILES {
        out.insert(p.to_string(), json!(sha256(&fs::read(live_paper_dir.join(p)).map_err(|e| e.to_string())?)));
    }
    Ok(V::Object(out))
}

const ELIGIBLE: [&str; 3] = ["discovery", "comparison", "edge_observation"];

/// planCapacityPair(plan, review, {cycle, now, excludedExchangeIds=[]})
pub fn plan_capacity_pair(plan: &V, review: &V, cycle: &V, now: &V, excluded_exchange_ids: &[V]) -> Result<V, String> {
    crate::canonical_no_capacity::validate_capacity_review(review)?;
    need(js_int(cycle).is_some_and(|c| c >= 0) && js_int(now).is_some(), "PLAN_CLOCK")?;
    let now_ms = js_int(now).unwrap();
    if js_int(cycle).unwrap() % 4 != 0
        || now_ms < instant(&review["recordedAt"])?
        || now_ms >= instant(&review["expiresAt"])?
        || review["legs"].as_array().is_some_and(|legs| legs.iter()
            .any(|l| excluded_exchange_ids.iter().any(|x| strict_eq(x, &l["exchangeId"]))))
        || js_truthy(plan.get("comparisonSampling"))
        || plan["basketMeasurement"]["marketIds"].as_array().is_some_and(|a| !a.is_empty())
        || plan["paperPackageFocus"]["marketIds"].as_array().is_some_and(|a| !a.is_empty())
    {
        return Ok(plan.clone());
    }
    need(plan["reads"].as_array().is_some_and(|r| r.len() <= 4), "READ_BUDGET")?;
    let base_reads = plan["reads"].as_array().cloned().unwrap_or_default();
    let protected_reads: Vec<V> = base_reads.iter()
        .filter(|r| !ELIGIBLE.contains(&r["reason"].as_str().unwrap_or(""))).cloned().collect();
    let ids: Vec<V> = protected_reads.iter().map(|r| r["marketId"].clone()).collect();
    let legs = review["legs"].as_array().cloned().unwrap_or_default();
    if ids.iter().chain(legs.iter().map(|l| &l["marketId"]))
        .map(js_stringify).collect::<HashSet<_>>().len() > base_reads.len()
    {
        return Ok(plan.clone());
    }
    // Retain all risk reads and their order. Keep the selected pair contiguous
    // after protected reads; replace slots instead of appending network traffic.
    let pair: Vec<V> = legs.iter().filter(|l| !ids.iter().any(|i| strict_eq(i, &l["marketId"])))
        .map(|l| json!({"marketId": l["marketId"].clone(), "exchangeId": l["exchangeId"].clone(), "option": "YES",
            "path": "supplementalDepth", "reason": "conditional_capacity_pair"})).collect();
    let mut reads: Vec<V> = protected_reads;
    reads.extend(pair.iter().cloned());
    for r in &base_reads {
        if reads.len() < base_reads.len() && !reads.iter().any(|x| strict_eq(&x["marketId"], &r["marketId"])) {
            reads.push(r.clone());
        }
    }
    let selected: Vec<V> = legs.iter().map(|l| l["marketId"].clone()).collect();
    let mut out = plan.clone();
    out["entryMarketIds"] = json!(plan["entryMarketIds"].as_array().cloned().unwrap_or_default().iter()
        .filter(|id| reads.iter().any(|r| strict_eq(&r["marketId"], id)) && !selected.iter().any(|s| strict_eq(s, id)))
        .cloned().collect::<Vec<_>>());
    out["reads"] = json!(reads);
    out["canonicalCapacityFocus"] = json!({"marketIds": selected, "replacedSlots": pair.len(),
        "requestsAdded": 0, "executionAuthority": false});
    Ok(out)
}

pub struct CanonicalCapacityMonitor {
    pub bank: V,
    pub bank_hash: String,
    pub dir: PathBuf,
    runtime_dir: PathBuf,
    live_paper_dir: PathBuf,
    now: Arc<dyn Fn() -> f64 + Send + Sync>,
    bank_bytes: Vec<u8>,
    code_identity: V,
    file: PathBuf,
    journal: PathBuf,
    cohort: V,
    sequence: i64,
    previous: V,
    counts: Map<String, V>,
    last_pair: V,
    last: V,
    journal_bytes: u64,
    journal_hash: String,
    failed: bool,
}

/// openCanonicalCapacityMonitor({runtimeDir, universe, now=Date.now}) — plus the
/// live-paper directory whose .mjs files capacityCodeIdentity() hashes.
pub fn open_canonical_capacity_monitor(
    runtime_dir: &Path,
    universe: &V,
    live_paper_dir: &Path,
    now: Option<Arc<dyn Fn() -> f64 + Send + Sync>>,
) -> Result<Option<CanonicalCapacityMonitor>, String> {
    let active_file = runtime_dir.join("canonical-capacity-active.json");
    if !active_file.exists() {
        return Ok(None);
    }
    let active: V = serde_json::from_str(&fs::read_to_string(&active_file).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    need(cohort_ok(&js_str_opt(active.get("cohort"))), "DIRECTORY")?;
    // path.join requires strings: a non-string cohort that passed the coerced
    // regex above throws TypeError here in JS.
    if !active["cohort"].is_string() {
        return Err("The \"paths[2]\" argument must be of type string".into());
    }
    let dir = runtime_dir.join("canonical-capacity-cohorts").join(active["cohort"].as_str().unwrap());
    let file = dir.join("bank.json");
    let journal = dir.join("journal.jsonl");
    let bank_bytes = fs::read(&file).map_err(|e| e.to_string())?;
    let bank: V = serde_json::from_slice(&bank_bytes).map_err(|e| e.to_string())?;
    let bank_hash = digest(&bank);
    let code_identity = capacity_code_identity(live_paper_dir)?;
    need(strict_eq(&json!(bank_hash), &active["bankHash"])
        && digest(&code_identity) == digest(&bank["codeIdentity"])
        && digest(&bank["policy"]) == digest(&crate::canonical_no_capacity::canonical_no_policy()),
        "IDENTITY")?;
    crate::canonical_no_capacity::validate_capacity_review(&bank["review"])?;
    baal_service_runtime::fixed::instant(bank["cutoff"].as_str().unwrap_or(""))?;
    let source = fs::read(runtime_dir.join("public-books.jsonl")).map_err(|e| e.to_string())?;
    let spb = js_int(&bank["sourcePrefixBytes"]).unwrap_or(0);
    let prefix = &source[..(spb.max(0) as usize).min(source.len())];
    need(js_int(&bank["sourcePrefixBytes"]).is_some_and(|b| b > 0)
        && prefix.len() as i64 == spb
        && prefix.last() == Some(&10)
        && sha256(prefix) == bank["sourcePrefixHash"].as_str().unwrap_or(""), "SOURCE_PREFIX")?;
    let lines: Vec<&[u8]> = prefix.split(|b| *b == b'\n').collect();
    let last_prefix: V = serde_json::from_slice(lines[lines.len().saturating_sub(2)]).map_err(|e| e.to_string())?;
    need(strict_eq(&last_prefix["sequence"], &bank["sourcePrefixSequence"])
        && strict_eq(&last_prefix["hash"], &bank["sourcePrefixJournalHash"]), "SOURCE_PREFIX")?;
    need(strict_eq(&bank["tournamentId"], &universe["tournamentId"])
        && strict_eq(&bank["review"]["tournamentId"], &universe["tournamentId"])
        && bank["review"]["legs"].as_array().is_some_and(|legs| legs.iter()
            .all(|l| universe["exchanges"].as_array().is_some_and(|xs| xs.iter()
                .any(|x| strict_eq(&x["marketId"], &l["marketId"])
                    && strict_eq(&x["exchangeId"], &l["exchangeId"])
                    && x["option"].as_str() == Some("YES"))))), "SCOPE")?;
    let mut mon = CanonicalCapacityMonitor {
        bank, bank_hash, dir, runtime_dir: runtime_dir.to_path_buf(),
        live_paper_dir: live_paper_dir.to_path_buf(),
        now: now.unwrap_or_else(|| Arc::new(|| {
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as f64).unwrap_or(0.0)
        })),
        bank_bytes, code_identity, file, journal,
        cohort: active["cohort"].clone(),
        sequence: 0, previous: V::Null, counts: Map::new(),
        last_pair: V::Null, last: V::Null,
        journal_bytes: 0, journal_hash: String::new(), failed: false,
    };
    mon.previous = json!(mon.bank_hash.clone());
    let raw = fs::read(&mon.journal).map_err(|e| e.to_string())?;
    need(raw.is_empty() || raw.last() == Some(&10), "PARTIAL_JOURNAL")?;
    for line in raw.split(|b| *b == b'\n').filter(|l| !l.is_empty()) {
        let row: V = serde_json::from_slice(line).map_err(|e| e.to_string())?;
        mon.reduce(&row)?;
    }
    mon.journal_bytes = raw.len() as u64;
    mon.journal_hash = sha256(&raw);
    Ok(Some(mon))
}

impl CanonicalCapacityMonitor {
    fn reduce(&mut self, row: &V) -> Result<(), String> {
        let mut body = row.as_object().cloned().unwrap_or_default();
        let hash = body.remove("hash").unwrap_or(V::Null);
        let body = V::Object(body);
        need(js_int(&body["sequence"]) == Some(self.sequence + 1)
            && strict_eq(&body["previous"], &self.previous)
            && strict_eq(&body["bankHash"], &json!(self.bank_hash.clone()))
            && digest(&body) == js_str(&hash), "JOURNAL")?;
        need(body["kind"].as_str() == Some("capacity")
            && body["result"]["reviewHash"].as_str() == Some(digest(&self.bank["review"]).as_str())
            && body["result"]["policyHash"].as_str() == Some(digest(&self.bank["policy"]).as_str()),
            "RESULT")?;
        self.sequence = js_int(&body["sequence"]).unwrap_or(self.sequence + 1);
        self.previous = hash;
        self.last_pair = body["pair"].clone();
        self.last = body["result"].clone();
        let key = js_key(body["result"].as_object().and_then(|o| o.get("reason")));
        let n = self.counts.get(&key).and_then(|v| v.as_i64()).unwrap_or(0) + 1;
        self.counts.insert(key, json!(n));
        Ok(())
    }
    fn stopped(&self) -> bool {
        self.runtime_dir.join("STOP").exists()
            || self.runtime_dir.join("COLLECTOR_STOP").exists()
            || self.dir.join("STOP").exists()
    }
    fn verify(&self) -> Result<(), String> {
        need(!self.failed && !self.stopped(), "STOPPED")?;
        need(sha256(&fs::read(&self.file).map_err(|e| e.to_string())?) == sha256(&self.bank_bytes)
            && digest(&capacity_code_identity(&self.live_paper_dir)?) == digest(&self.code_identity),
            "IDENTITY_CHANGED")?;
        let bytes = fs::read(&self.journal).map_err(|e| e.to_string())?;
        need(bytes.len() as u64 == self.journal_bytes && sha256(&bytes) == self.journal_hash, "JOURNAL_CHANGED")?;
        Ok(())
    }
    /// status() — the published projection.
    pub fn status(&self) -> Result<V, String> {
        Ok(json!({"schemaVersion": 1, "mode": "canonical_no_capacity_observation",
            "cohort": self.cohort.clone(), "bankHash": self.bank_hash.clone(),
            "updatedAt": iso_or_err((self.now)())?, "failed": self.failed,
            "sourceSequence": if self.last_pair["sourceSequence"].is_null() {
                self.bank["sourcePrefixSequence"].clone()
            } else {
                self.last_pair["sourceSequence"].clone()
            },
            "journalSequence": self.sequence, "journalHash": self.previous.clone(),
            "counts": V::Object(self.counts.clone()), "last": self.last.clone(),
            "executionAuthority": false, "competitionOrders": 0, "networkRequests": 0,
            "profitabilityEstablished": false}))
    }
    /// inspect(publication) — observe a publication cycle; on failure the
    /// monitor marks itself failed and rethrows, exactly like the JS.
    pub fn inspect(&mut self, publication: &V) -> Result<bool, String> {
        match self.inspect_inner(publication) {
            Err(e) => {
                self.failed = true;
                Err(e)
            }
            ok => ok,
        }
    }
    fn inspect_inner(&mut self, publication: &V) -> Result<bool, String> {
        self.verify()?;
        if publication.is_null() {
            return Err("Cannot read properties of null (reading 'closed')".into());
        }
        if js_truthy(publication.get("closed")) {
            return Ok(false);
        }
        let legs = self.bank["review"]["legs"].as_array().cloned().unwrap_or_default();
        // publication.books[...] on a null/undefined books member throws in JS.
        let books_member = publication.get("books");
        if books_member.is_none() || books_member == Some(&V::Null) {
            let which = if books_member.is_none() { "undefined" } else { "null" };
            let key = legs.first()
                .map(|l| js_str_opt(l.as_object().and_then(|o| o.get("exchangeId"))))
                .unwrap_or_default();
            return Err(format!("Cannot read properties of {} (reading '{}:YES')", which, key));
        }
        let books: Vec<V> = legs.iter()
            .map(|l| publication["books"][format!("{}:YES", js_str_opt(l.as_object().and_then(|o| o.get("exchangeId"))))].clone())
            .collect();
        let source_prefix_sequence = js_num_opt(self.bank.get("sourcePrefixSequence"));
        let cutoff = instant(&self.bank["cutoff"])?;
        for b in &books {
            if !js_truthy(Some(b))
                || js_num_opt(b.get("publicationSequence")) <= source_prefix_sequence
                || instant(&b["requestStartedAt"])? < cutoff
            {
                return Ok(false);
            }
        }
        let prior = self.last_pair.clone();
        if js_truthy(Some(&prior)) {
            for (i, b) in books.iter().enumerate() {
                if strict_eq(&b["versionKey"], &prior["versions"][i])
                    || instant(&b["requestStartedAt"])? <= instant(&prior["lastReceiptAt"])?
                {
                    return Ok(false);
                }
            }
        }
        let at = iso_or_err((self.now)())?;
        let account = fs::read_to_string(self.runtime_dir.join("official-account.json")).ok()
            .and_then(|s| serde_json::from_str::<V>(&s).ok())
            .and_then(|v| crate::probability_receipt::probability_account_eligibility(&v))
            .unwrap_or(V::Null);
        let result = crate::canonical_no_capacity::evaluate_canonical_no_capacity(
            &self.bank["review"], &books, if account.is_null() { None } else { Some(&account) },
            &at, &self.bank["policy"])?;
        let mut receipts: Vec<V> = books.iter().map(|b| b["receivedAt"].clone()).collect();
        receipts.sort_by(|a, b| js_str(a).cmp(&js_str(b)));
        let pair = json!({"sourceSequence": publication["sequence"].clone(), "sourceHash": publication["hash"].clone(),
            "versions": books.iter().map(|b| b["versionKey"].clone()).collect::<Vec<_>>(),
            "lastReceiptAt": receipts.last().cloned().unwrap_or(V::Null)});
        let mut body = json!({"sequence": self.sequence + 1, "previous": self.previous.clone(),
            "bankHash": self.bank_hash.clone(), "kind": "capacity", "at": at, "pair": pair,
            "accountEligibility": account, "result": result});
        let hash = digest(&body);
        body.as_object_mut().unwrap().insert("hash".into(), json!(hash));
        let row = body;
        let bytes = format!("{}\n", js_stringify(&row)).into_bytes();
        {
            let mut o = fs::OpenOptions::new();
            o.append(true).create(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                o.mode(0o600);
            }
            let mut f = o.open(&self.journal).map_err(|e| e.to_string())?;
            let mut n = 0;
            while n < bytes.len() {
                let wrote = f.write(&bytes[n..]).map_err(|e| e.to_string())?;
                need(wrote > 0, "SHORT_WRITE")?;
                n += wrote;
            }
            f.sync_all().map_err(|e| e.to_string())?;
        }
        self.journal_bytes += bytes.len() as u64;
        self.journal_hash = sha256(&fs::read(&self.journal).map_err(|e| e.to_string())?);
        self.reduce(&row)?;
        baal_service_runtime::atomic::atomic_json_canonical(
            &self.dir.join("stats.json"), &self.status()?)?;
        Ok(true)
    }
    /// plan(base, context) — verify, then substitute the conditional pair.
    pub fn plan(&self, base: &V, context: &V) -> Result<V, String> {
        self.verify()?;
        plan_capacity_pair(base, &self.bank["review"], &context["cycle"], &context["now"],
            &context["excludedExchangeIds"].as_array().cloned().unwrap_or_default())
    }
}
