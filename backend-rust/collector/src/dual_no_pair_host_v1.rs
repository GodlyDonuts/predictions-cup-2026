// live-paper/dual-no-pair-host-v1.mjs — locked, journaled dual-NO paper host.
use baal_execution::core::payload_hash;
use baal_service_runtime::atomic::atomic_json_canonical;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::{js_truthy, sha256};
use serde_json::{json, Map, Value as V};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

fn need(ok: bool, code: &str) -> Result<(), String> { if !ok { return Err(format!("DUAL_NO_HOST_{}", code)); } Ok(()) }
fn hex(v: &V) -> bool { v.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))) }
fn cohort_ok(cohort: &str) -> bool {
    let b = cohort.as_bytes();
    b.len() >= 2 && b.len() <= 64 && b[0].is_ascii_lowercase() && b[1..].iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}
pub fn system_now() -> i64 { SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0) }
fn uuid_v4() -> String {
    let mut b = [0u8; 16];
    let mut seed = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64 ^ std::process::id() as u64;
    for byte in &mut b { seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17; *byte = seed as u8; }
    b[6] = (b[6] & 0x0f) | 0x40; b[8] = (b[8] & 0x3f) | 0x80;
    format!("{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15])
}
fn write_private_new(path: &Path, body: &[u8]) -> Result<(), String> {
    let mut o = fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; o.mode(0o600); }
    let mut f = o.open(path).map_err(|e| e.to_string())?;
    f.write_all(body).map_err(|e| e.to_string())
}
fn set_private_dir(path: &Path) {
    #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700)); }
}
fn sha_file(path: &Path) -> Result<String, String> { Ok(sha256(&fs::read(path).map_err(|e| e.to_string())?)) }
fn sha_empty() -> String { sha256(&[]) }

struct PrefixDescriptor { end: u64, hash: String, tail: V }
fn prefix_descriptor(file: &Path, end: u64) -> Result<PrefixDescriptor, String> {
    let mut fd = fs::File::open(file).map_err(|e| e.to_string())?;
    let result = (|| -> Result<PrefixDescriptor, String> {
        let before = fd.metadata().map_err(|e| e.to_string())?;
        let mut hash = Sha256::new();
        let mut offset = 0u64;
        let mut tail: Vec<u8> = Vec::new();
        need(before.len() >= end && end > 0, "SOURCE_PREFIX")?;
        while offset < end {
            let want = std::cmp::min(1048576u64, end - offset) as usize;
            let mut chunk = vec![0u8; want];
            fd.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;
            let n = fd.read(&mut chunk).map_err(|e| e.to_string())?;
            need(n > 0, "SOURCE_PREFIX")?;
            hash.update(&chunk[..n]);
            tail.extend_from_slice(&chunk[..n]);
            if tail.len() > 1048576 { tail.drain(0..tail.len() - 1048576); }
            offset += n as u64;
        }
        let after = fd.metadata().map_err(|e| e.to_string())?;
        let same_ino = {
            #[cfg(unix)] { use std::os::unix::fs::MetadataExt; after.ino() == before.ino() }
            #[cfg(not(unix))] { true }
        };
        need(same_ino && after.created().ok() == before.created().ok() && after.len() >= end && tail.last() == Some(&10), "SOURCE_PREFIX")?;
        let start = tail[..tail.len() - 1].iter().rposition(|b| *b == 10).map(|i| i + 1).unwrap_or(0);
        let tail_v: V = serde_json::from_slice(&tail[start..tail.len() - 1]).map_err(|e| e.to_string())?;
        Ok(PrefixDescriptor { end, hash: format!("{:x}", hash.finalize()), tail: tail_v })
    })();
    drop(fd);
    result
}
fn sealed_source(file: &Path) -> Result<PrefixDescriptor, String> {
    let size = fs::metadata(file).map_err(|e| e.to_string())?.len();
    let mut tail = vec![0u8; std::cmp::min(size, 1048576) as usize];
    {
        let mut fd = fs::File::open(file).map_err(|e| e.to_string())?;
        fd.seek(SeekFrom::Start(size - tail.len() as u64)).map_err(|e| e.to_string())?;
        let _ = fd.read(&mut tail).map_err(|e| e.to_string())?;
    }
    let index = tail.iter().rposition(|b| *b == 10);
    need(index.is_some(), "EMPTY_SOURCE")?;
    prefix_descriptor(file, size - tail.len() as u64 + index.unwrap() as u64 + 1)
}
/// dualNoPairCodeIdentity() — sha256 of each source file; workspace_dir is the repo root
/// that contains live-paper/, execution/, paper/ and feed/ (the JS import.meta.url base).
pub fn dual_no_pair_code_identity(workspace_dir: &Path) -> Result<V, String> {
    let mut m = Map::new();
    for file in ["./dual-no-pair-host-v1.mjs","./dual-no-pair-paper-v1.mjs","./dual-no-review-v1.mjs",
        "./canonical-no-capacity.mjs","./probability-receipt.mjs","./transport-runtime.mjs","./journal-lines.mjs",
        "../execution/ledger.mjs","../paper/fixed.mjs","../feed/contract.mjs"] {
        let path = if let Some(rest) = file.strip_prefix("./") { workspace_dir.join("live-paper").join(rest) }
            else { workspace_dir.join(file.trim_start_matches("../")) };
        m.insert(file.to_string(), json!(sha256(&fs::read(&path).map_err(|e| e.to_string())?)));
    }
    Ok(V::Object(m))
}

/// openDualNoPaperHost({runtimeDir,cohort,initialize,bundle,now,canOperate}).
/// `workspace_dir` replaces import.meta.url for the code-identity file hashes.
pub fn open_dual_no_paper_host(workspace_dir: &Path, runtime_dir: &Path, cohort: &str, initialize: bool, bundle: Option<&V>,
    now: Arc<dyn Fn() -> i64 + Send + Sync>, can_operate: Arc<dyn Fn() -> bool + Send + Sync>) -> Result<DualNoPaperHost, String> {
    need(runtime_dir.is_absolute() && cohort_ok(cohort), "SCOPE")?;
    let dir = runtime_dir.join("dual-no-pair-cohorts").join(cohort);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    set_private_dir(&dir);
    let bank_file = dir.join("bank.json");
    let stats_file = dir.join("stats.json");
    let source = runtime_dir.join("public-books.jsonl");
    let lock = dir.join("worker.lock");
    let stopped = |dir: &Path, runtime_dir: &Path| -> bool {
        runtime_dir.join("STOP").exists() || dir.join("STOP").exists() || dir.join("OBSERVER_STOP").exists()
    };
    need(can_operate() && !stopped(&dir, runtime_dir), "STOPPED")?;
    let token = serde_json::to_string(&json!({"pid":std::process::id(),"id":uuid_v4()})).map_err(|e| e.to_string())?;
    write_private_new(&lock, token.as_bytes())?;
    let result = open_locked(workspace_dir, runtime_dir, cohort, initialize, bundle, now, can_operate,
        dir, bank_file, stats_file, source, lock.clone(), token.clone());
    if let Err(e) = &result {
        if lock.exists() && fs::read_to_string(&lock).map(|t| t == token).unwrap_or(false) { let _ = fs::remove_file(&lock); }
        return Err(e.clone());
    }
    result
}

fn open_locked(workspace_dir: &Path, runtime_dir: &Path, cohort: &str, initialize: bool, bundle: Option<&V>,
    now: Arc<dyn Fn() -> i64 + Send + Sync>, can_operate: Arc<dyn Fn() -> bool + Send + Sync>,
    dir: PathBuf, bank_file: PathBuf, stats_file: PathBuf, #[allow(dead_code)] source: PathBuf, lock: PathBuf, token: String) -> Result<DualNoPaperHost, String> {
    let bank: V;
    if initialize {
        need(bundle.is_some_and(|x| js_truthy(Some(x))) && !bank_file.exists() && !dir.join("journal.jsonl").exists(), "EXISTS")?;
        crate::dual_no_review_v1::validate_dual_no_review(&bundle.unwrap()["review"])?;
        let sealed = sealed_source(&source)?;
        let end = sealed.end;
        let tail = sealed.tail.clone();
        let cutoff = ms_to_iso(now() as f64);
        let cutoff_ms = parse_iso_ms(&cutoff).unwrap_or(f64::NAN);
        need(parse_iso_ms(tail["at"].as_str().unwrap_or("")).unwrap_or(f64::NAN) <= cutoff_ms
            && parse_iso_ms(bundle.unwrap()["review"]["recordedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN) <= cutoff_ms
            && parse_iso_ms(bundle.unwrap()["review"]["expiresAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN) > cutoff_ms, "CUTOFF")?;
        let mut capacity_policy = crate::canonical_no_capacity::canonical_no_policy();
        capacity_policy["maxQuantity"] = json!(1); capacity_policy["maxCost"] = json!("1.000000"); capacity_policy["maxUnmatchedCost"] = json!("1.000000");
        bank = json!({"schemaVersion":1,"mode":"dual_no_sequential_paper_v1","name":cohort,"cutoff":cutoff,
            "review":bundle.unwrap()["review"].clone(),"policy":crate::dual_no_pair_paper_v1::dual_no_pair_policy(),
            "capacityPolicy":capacity_policy,"codeIdentity":dual_no_pair_code_identity(workspace_dir)?,
            "sourcePrefixBytes":end,"sourcePrefixHash":sealed.hash,"sourcePrefixSequence":tail["sequence"].clone(),
            "sourcePrefixTailHash":tail["hash"].clone()});
        write_private_new(&bank_file, format!("{}\n", serde_json::to_string_pretty(&bank).map_err(|e| e.to_string())?).as_bytes())?;
    } else {
        bank = serde_json::from_str(&fs::read_to_string(&bank_file).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    }
    crate::dual_no_review_v1::validate_dual_no_review(&bank["review"])?;
    need(payload_hash(&bank["codeIdentity"])? == payload_hash(&dual_no_pair_code_identity(workspace_dir)?)?, "CODE_IDENTITY")?;
    let bytes = fs::read(&bank_file).map_err(|e| e.to_string())?;
    let hash = payload_hash(&bank)?;
    let prefix = prefix_descriptor(&source, bank["sourcePrefixBytes"].as_u64().unwrap_or(0))?;
    need(prefix.hash == bank["sourcePrefixHash"].as_str().unwrap_or(""), "SOURCE_PREFIX")?;
    let tail = prefix.tail;
    need(tail["sequence"] == bank["sourcePrefixSequence"] && tail["hash"] == bank["sourcePrefixTailHash"], "PREFIX_TAIL")?;
    let journal_file = dir.join("journal.jsonl");
    let journal = Journal::open(&journal_file)?;
    let mut state = json!({"paper":crate::dual_no_pair_paper_v1::create_dual_no_pair_state(&bank)?,
        "sourceSequence":bank["sourcePrefixSequence"].clone(),"sourceHash":bank["sourcePrefixTailHash"].clone()});
    if journal.last.is_some() {
        let text = fs::read_to_string(&journal_file).map_err(|e| e.to_string())?;
        let records: Vec<V> = text.trim_end().split('\n').map(serde_json::from_str).collect::<Result<_,_>>().map_err(|e| e.to_string())?;
        for (i, row) in records.iter().enumerate() {
            let event = row["event"].clone();
            need(js_truthy(Some(&event)) && ["genesis","book","boundary"].contains(&event["kind"].as_str().unwrap_or("")), "REPLAY_EVENT")?;
            if i == 0 {
                need(event["kind"].as_str() == Some("genesis") && event["bankHash"].as_str() == Some(hash.as_str())
                    && event["cutoff"] == bank["cutoff"], "GENESIS")?;
            } else {
                need(event["kind"].as_str() != Some("genesis"), "REPLAY_GENESIS")?;
                let paper = crate::dual_no_pair_paper_v1::advance_dual_no_pair(&state["paper"], &event, &bank)?;
                if event["kind"].as_str() == Some("book") {
                    need(baal_service_runtime::json_text::js_int(&event["sourceSequence"]).unwrap_or(i64::MIN)
                        > baal_service_runtime::json_text::js_int(&state["sourceSequence"]).unwrap_or(i64::MIN)
                        && hex(&event["sourceHash"]), "REPLAY_FRONTIER")?;
                    state = json!({"paper":paper,"sourceSequence":event["sourceSequence"].clone(),"sourceHash":event["sourceHash"].clone()});
                } else { state["paper"] = paper; }
            }
            need(payload_hash(&state)? == payload_hash(&row["state"])?, "REPLAY_STATE")?;
            crate::dual_no_pair_paper_v1::assert_dual_no_pair(&state["paper"], &bank)?;
        }
    }
    let journal_digest = if journal_file.exists() { sha_file(&journal_file)? } else { sha_empty() };
    let bank_sha = sha256(&bytes);
    let mut host = DualNoPaperHost {
        bank, runtime_dir: runtime_dir.to_path_buf(), dir, bank_file, stats_file, source, journal_file, lock, token,
        bank_sha, bank_hash: hash.clone(), journal_digest, journal, state, closed: false, failed: false, now, can_operate,
    };
    if host.journal.last.is_none() {
        let event = json!({"kind":"genesis","cutoff":host.bank["cutoff"].clone(),"bankHash":hash});
        let next = host.state.clone();
        host.append(next, &event, false)?;
    } else {
        need(host.state["paper"]["bankHash"].as_str() == Some(hash.as_str()), "JOURNAL_BANK")?;
        let last_ms = parse_iso_ms(host.state["paper"]["lastAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN);
        let m = if last_ms.is_nan() { f64::NAN } else { ((host.now)() as f64).max(last_ms) };
        if !m.is_finite() { return Err("Invalid time value".into()); }
        let at = ms_to_iso(m);
        let paper = crate::dual_no_pair_paper_v1::advance_dual_no_pair(&host.state["paper"], &json!({"kind":"boundary","availableAt":at}), &host.bank)?;
        let mut next = host.state.clone();
        next["paper"] = paper;
        host.append(next, &json!({"kind":"boundary","availableAt":at,"reason":"restart"}), false)?;
    }
    Ok(host)
}

/// The object returned by openDualNoPaperHost.
pub struct DualNoPaperHost {
    pub bank: V,
    runtime_dir: PathBuf, dir: PathBuf, bank_file: PathBuf, stats_file: PathBuf, #[allow(dead_code)] source: PathBuf,
    journal_file: PathBuf, lock: PathBuf, token: String, bank_sha: String, bank_hash: String, journal_digest: String,
    journal: Journal, state: V, closed: bool, failed: bool,
    now: Arc<dyn Fn() -> i64 + Send + Sync>, can_operate: Arc<dyn Fn() -> bool + Send + Sync>,
}
impl DualNoPaperHost {
    fn stopped(&self) -> bool {
        self.runtime_dir.join("STOP").exists() || self.dir.join("STOP").exists() || self.dir.join("OBSERVER_STOP").exists()
    }
    fn live(&self, allow_stopped: bool) -> Result<(), String> {
        need(!self.closed && !self.failed && (allow_stopped || ((self.can_operate)() && !self.stopped()))
            && fs::read_to_string(&self.lock).map_err(|e| e.to_string())? == self.token
            && sha_file(&self.bank_file)? == self.bank_sha, "OWNERSHIP")
    }
    /// status()
    pub fn status(&self) -> Result<V, String> {
        let mut st = crate::dual_no_pair_paper_v1::dual_no_pair_report(&self.state["paper"], &self.bank, &ms_to_iso((self.now)() as f64))?;
        st["failed"] = json!(self.failed);
        st["journalSequence"] = json!(self.journal.sequence);
        st["journalHash"] = json!(self.journal.previous);
        st["sourceSequence"] = self.state["sourceSequence"].clone();
        st["sourceHash"] = self.state["sourceHash"].clone();
        st["workerPid"] = json!(std::process::id());
        Ok(st)
    }
    fn append(&mut self, next: V, event: &V, allow_stopped: bool) -> Result<(), String> {
        self.live(allow_stopped)?;
        let result = self.append_inner(next, event);
        if result.is_err() { self.failed = true; }
        result
    }
    fn append_inner(&mut self, next: V, event: &V) -> Result<(), String> {
        let current = if self.journal_file.exists() { sha_file(&self.journal_file)? } else { sha_empty() };
        need(current == self.journal_digest, "JOURNAL_CHANGED")?;
        self.journal.commit(&next, event)?;
        self.journal_digest = sha_file(&self.journal_file)?;
        self.state = next;
        let st = self.status()?;
        atomic_json_canonical(&self.stats_file, &st)?;
        Ok(())
    }
    /// publish()
    pub fn publish(&self) -> Result<(), String> { atomic_json_canonical(&self.stats_file, &self.status()?) }
    /// onBook(publication, marketId, accountEligibility)
    pub fn on_book(&mut self, publication: &V, market_id: &str, account_eligibility: &V) -> Result<(), String> {
        self.live(false)?;
        let legs = self.bank["review"]["legs"].as_array().cloned().unwrap_or_default();
        let leg = match legs.iter().find(|x| x["marketId"].as_str() == Some(market_id)) { Some(l) => l.clone(), None => return Ok(()) };
        let book_key = format!("{}:YES", leg["exchangeId"].as_str().unwrap_or(""));
        let book = publication["books"][&book_key].clone();
        if !js_truthy(Some(&book)) { return Ok(()); }
        let pub_seq = baal_service_runtime::json_text::js_int(&publication["sequence"]).unwrap_or(i64::MIN);
        let state_seq = baal_service_runtime::json_text::js_int(&self.state["sourceSequence"]).unwrap_or(i64::MIN);
        let prefix_seq = baal_service_runtime::json_text::js_int(&self.bank["sourcePrefixSequence"]).unwrap_or(i64::MIN);
        need(pub_seq > state_seq && hex(&publication["hash"])
            && baal_service_runtime::json_text::js_int(&book["publicationSequence"]).is_some_and(|x| x <= pub_seq && x > prefix_seq), "SOURCE_FRONTIER")?;
        let at = ms_to_iso((self.now)() as f64);
        let books: Vec<V> = legs.iter().map(|x| publication["books"][&format!("{}:YES", x["exchangeId"].as_str().unwrap_or(""))].clone()).collect();
        let closed = js_truthy(Some(&publication["closed"]));
        let mut admission = json!({"positiveConditionalCapacity":false});
        if !closed && books.iter().all(|b| js_truthy(Some(b))) {
            admission = crate::canonical_no_capacity::evaluate_canonical_no_capacity(&self.bank["review"], &books,
                if account_eligibility.is_null() { None } else { Some(account_eligibility) }, &at, &self.bank["capacityPolicy"])
                .unwrap_or_else(|_| json!({"positiveConditionalCapacity":false,"reason":"invalid_or_unavailable_pair"}));
        }
        let event = json!({"kind":"book","book":book,"pairBooks":if closed { Vec::<V>::new() } else { books.iter().filter(|b| js_truthy(Some(b))).cloned().collect::<Vec<_>>() },
            "availableAt":at,"accountEligibility":if closed { V::Null } else { account_eligibility.clone() },"admission":admission});
        let paper = crate::dual_no_pair_paper_v1::advance_dual_no_pair(&self.state["paper"], &event, &self.bank)?;
        let next = json!({"paper":paper,"sourceSequence":publication["sequence"].clone(),"sourceHash":publication["hash"].clone()});
        let mut journal_event = event;
        journal_event["sourceSequence"] = publication["sequence"].clone();
        journal_event["sourceHash"] = publication["hash"].clone();
        self.append(next, &journal_event, false)?;
        Ok(())
    }
    /// focus()
    pub fn focus(&self) -> Result<V, String> {
        let paper = &self.state["paper"];
        let owned = paper["positions"].as_object().is_some_and(|m| !m.is_empty())
            || js_truthy(Some(&paper["pending"])) || js_truthy(Some(&paper["package"]));
        let expires = parse_iso_ms(self.bank["review"]["expiresAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN);
        let markets: Vec<V> = self.bank["review"]["legs"].as_array().cloned().unwrap_or_default().iter()
            .filter(|_| owned || ((self.now)() as f64) < expires)
            .map(|leg| {
                let exid = leg["exchangeId"].as_str().unwrap_or("");
                let phase = if js_truthy(Some(&paper["positions"][exid])) { "held" }
                    else if paper["pending"]["exchangeId"] == leg["exchangeId"] {
                        if paper["pending"]["action"].as_str() == Some("sell") { "pending_sell" } else { "pending_buy" }
                    } else { "depth_required" };
                json!({"marketId":leg["marketId"].clone(),"phase":phase})
            }).collect();
        Ok(json!({"bankHash":self.bank_hash,"markets":markets,"ownedRiskReads":true,"executionAuthority":false}))
    }
    /// boundary(reason)
    pub fn boundary(&mut self, reason: Option<&str>) -> Result<(), String> {
        self.live(true)?;
        let last_ms = parse_iso_ms(self.state["paper"]["lastAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN);
        let m = if last_ms.is_nan() { f64::NAN } else { ((self.now)() as f64).max(last_ms) };
        if !m.is_finite() { return Err("Invalid time value".into()); }
        let at = ms_to_iso(m);
        let paper = crate::dual_no_pair_paper_v1::advance_dual_no_pair(&self.state["paper"], &json!({"kind":"boundary","availableAt":at}), &self.bank)?;
        let mut next = self.state.clone();
        next["paper"] = paper;
        let mut event = json!({"kind":"boundary","availableAt":at});
        if let Some(r) = reason { event["reason"] = json!(r); }
        self.append(next, &event, true)
    }
    /// close()
    pub fn close(&mut self) -> Result<(), String> {
        if self.closed { return Ok(()); }
        need(fs::read_to_string(&self.lock).map_err(|e| e.to_string())? == self.token, "LOCK_CHANGED")?;
        self.closed = true;
        fs::remove_file(&self.lock).map_err(|e| e.to_string())?;
        Ok(())
    }
}
