// live-paper/settlement-paper-host.mjs — owned settlement-value paper worker host.
use baal_service_runtime::atomic::atomic_json_canonical;
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Map, Value as V};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(format!("SETTLEMENT_HOST_{}", code)); }
    Ok(())
}
fn digest(v: &V) -> String { crate::model::digest(v) }
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

/// settlementPaperCodeIdentity() — sha256 over file bytes, keyed by the literal
/// './'-/'../'-prefixed paths. `live_paper_dir` stands in for import.meta.url.
pub fn settlement_paper_code_identity(live_paper_dir: &Path) -> Result<V, String> {
    let files = ["./settlement-paper-host.mjs", "./settlement-paper.mjs", "./settlement-value-native.mjs",
        "./probability-receipt.mjs", "./probability-paper.mjs", "./model.mjs", "./book-publication-v2.mjs",
        "./native-publication.mjs", "./rust-backend.mjs", "../paper/fixed.mjs", "../feed/contract.mjs",
        "../execution/ledger.mjs"];
    let mut out = Map::new();
    for p in files {
        let bytes = std::fs::read(live_paper_dir.join(p)).map_err(|e| e.to_string())?;
        out.insert(p.to_string(), json!(sha256(&bytes)));
    }
    Ok(V::Object(out))
}
fn complete(file: &Path) -> Result<Vec<u8>, String> {
    let b = std::fs::read(file).map_err(|e| e.to_string())?;
    let end = b.iter().rposition(|x| *x == b'\n').map(|p| p + 1).unwrap_or(0);
    Ok(b[..end].to_vec())
}
fn rows(bytes: &[u8]) -> Result<Vec<V>, String> {
    need(bytes.is_empty() || *bytes.last().unwrap() == b'\n', "PARTIAL")?;
    let text = String::from_utf8_lossy(bytes);
    let mut out = Vec::new();
    let trimmed: Vec<&str> = text.split('\n').collect();
    for line in trimmed.iter().take(trimmed.len().saturating_sub(1)) {
        out.push(serde_json::from_str(line).map_err(|e| e.to_string())?);
    }
    Ok(out)
}
fn write(file: &Path, bytes: &[u8], append: bool) -> Result<(), String> {
    let mut opts = std::fs::OpenOptions::new();
    if append { opts.append(true); } else { opts.write(true).create_new(true); }
    let mut fd = opts.mode(0o600).open(file).map_err(|e| e.to_string())?;
    let mut offset = 0;
    while offset < bytes.len() {
        let n = fd.write(&bytes[offset..]).map_err(|e| e.to_string())?;
        need(n > 0, "WRITE")?;
        offset += n;
    }
    fd.sync_all().map_err(|e| e.to_string())?;
    Ok(())
}

/// Structural host contract shared by the settlement host and its operator
/// wrappers (fundamental entry drain / value revision). Mirrors the JS
/// `{...host}` spread: consumers call whichever overrides exist.
pub trait PaperHost: Send {
    fn dir(&self) -> &Path;
    fn bank(&self) -> &V;
    fn consume(&mut self) -> Result<usize, String>;
    fn status(&mut self) -> Result<V, String>;
    fn publish(&mut self) -> Result<(), String>;
    fn close(&mut self) -> Result<(), String>;
    fn entry_draining(&self) -> Result<bool, String> { Ok(false) }
    fn focus(&mut self) -> Result<V, String> { Err("HOST_FOCUS_UNDEFINED".into()) }
    fn boundary(&mut self, _reason: &str) -> Result<(), String> { Err("HOST_BOUNDARY_UNDEFINED".into()) }
    fn on_book(&mut self, _publication: &V, _market_id: &str, _account: &V) -> Result<(), String> {
        Err("HOST_ONBOOK_UNDEFINED".into())
    }
    fn operator_revision_active(&self) -> bool { false }
}
/// openSettlementPaper({runtimeDir, cohort, initialize, bundle, now, workspaceDir})
pub struct SettlementPaperHost {
    pub dir: PathBuf,
    pub bank: V,
    runtime_dir: PathBuf,
    workspace_dir: PathBuf,
    live_paper_dir: PathBuf,
    source: PathBuf,
    journal_file: PathBuf,
    bank_file: PathBuf,
    lock_file: PathBuf,
    token: String,
    source_ino: u64,
    source_btime: Option<std::time::SystemTime>,
    code_identity: V,
    executable: PathBuf,
    bank_bytes_hash: String,
    prefix_hash: String,
    publication: V,
    state: V,
    bytes: Vec<u8>,
    offset: usize,
    decision_serial: u64,
    sequence: i64,
    previous: String,
    journal_bytes: u64,
    journal_hash: String,
    startup: V,
    closed: bool,
    failed: bool,
    now: std::sync::Arc<dyn Fn() -> i64 + Send + Sync>,
}
impl SettlementPaperHost {
    #[allow(clippy::too_many_arguments)]
    pub fn open(runtime_dir: &Path, cohort: &str, initialize: bool, bundle: Option<&V>,
        workspace_dir: &Path, now: std::sync::Arc<dyn Fn() -> i64 + Send + Sync>,
        live_paper_dir: &Path) -> Result<Self, String> {
        let cb = cohort.as_bytes();
        need(runtime_dir.is_absolute()
            && cb.len() >= 2 && cb.len() <= 64 && cb[0].is_ascii_lowercase()
            && cb[1..].iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-'), "DIRECTORY")?;
        let dir = runtime_dir.join("settlement-paper-cohorts").join(cohort);
        let bank_file = dir.join("bank.json");
        let journal_file = dir.join("journal.jsonl");
        let source = runtime_dir.join("public-books.jsonl");
        let lock_file = dir.join("worker.lock");
        let stopped_files = |workspace_dir: &Path, dir: &Path| -> bool {
            [workspace_dir.join("STOP"), runtime_dir.join("STOP"), dir.join("STOP"), dir.join("OBSERVER_STOP")]
                .iter().any(|f| f.exists())
        };
        need(!stopped_files(workspace_dir, &dir), "STOPPED")?;
        if !dir.exists() {
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
        }
        let token = serde_json::to_string(&json!({"pid": std::process::id(), "id": uuid_v4()})).map_err(|e| e.to_string())?;
        let mut lock_ok = false;
        let opened = Self::open_inner(runtime_dir, workspace_dir, cohort, initialize, bundle, now,
            dir.clone(), bank_file.clone(), journal_file.clone(), source.clone(), lock_file.clone(),
            token.clone(), live_paper_dir, &mut lock_ok);
        match opened {
            Ok(h) => Ok(h),
            Err(e) => {
                if lock_ok && std::fs::read_to_string(&lock_file).ok().as_deref() == Some(token.as_str()) {
                    let _ = std::fs::remove_file(&lock_file);
                }
                Err(e)
            }
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn open_inner(runtime_dir: &Path, workspace_dir: &Path, cohort: &str, initialize: bool, bundle: Option<&V>,
        now: std::sync::Arc<dyn Fn() -> i64 + Send + Sync>, dir: PathBuf, bank_file: PathBuf,
        journal_file: PathBuf, source: PathBuf, lock_file: PathBuf, token: String,
        live_paper_dir: &Path, lock_ok: &mut bool) -> Result<Self, String> {
        write(&lock_file, token.as_bytes(), false)?;
        *lock_ok = true;
        let closed = false;
        let failed = false;
        let stopped = |dir: &Path| -> bool {
            [workspace_dir.join("STOP"), runtime_dir.join("STOP"), dir.join("STOP"), dir.join("OBSERVER_STOP")]
                .iter().any(|f| f.exists())
        };
        let live = |closed: bool, failed: bool, dir: &Path| -> Result<(), String> {
            need(!closed && !failed && !stopped(dir)
                && std::fs::read_to_string(&lock_file).map_err(|e| e.to_string())? == token, "OWNERSHIP")
        };
        let start = std::time::Instant::now();
        let bytes = complete(&source)?;
        let records = rows(&bytes)?;
        let source_stat = std::fs::metadata(&source).map_err(|e| e.to_string())?;
        let code_identity = settlement_paper_code_identity(live_paper_dir)?;
        need(!records.is_empty(), "EMPTY")?;
        let bank: V = if initialize {
            need(bundle.is_some() && !bank_file.exists() && !journal_file.exists(), "EXISTS")?;
            let bundle = bundle.unwrap();
            let last = records.last().cloned().unwrap_or(V::Null);
            let cutoff = baal_service_runtime::contract::ms_to_iso((now)() as f64);
            need(baal_service_runtime::contract::parse_iso_ms(last["at"].as_str().unwrap_or(""))
                .unwrap_or(f64::NAN) <= (now)() as f64, "FUTURE_SOURCE")?;
            json!({"schemaVersion": 1, "mode": "funded_settlement_value_paper_v1", "name": cohort,
                "cutoff": cutoff, "universe": bundle["universe"], "identities": bundle["identities"],
                "research": bundle["research"], "researchEvidence": bundle["researchEvidence"],
                "riskGroups": bundle["riskGroups"], "valuationDeadlineMs": bundle["valuationDeadlineMs"],
                "policy": crate::settlement_paper::settlement_paper_policy(),
                "fillPolicy": crate::settlement_paper::settlement_paper_fills(),
                "sourcePrefixBytes": bytes.len(), "sourcePrefixHash": sha256(&bytes),
                "sourcePrefixSequence": last["sequence"], "sourcePrefixJournalHash": last["hash"],
                "nativeBackend": bundle["nativeBackend"], "codeIdentity": code_identity})
        } else {
            let bank: V = serde_json::from_slice(&std::fs::read(&bank_file).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            need(digest(&bank["codeIdentity"]) == digest(&code_identity), "FROZEN_CODE")?;
            bank
        };
        crate::settlement_paper::validate_settlement_paper_bank(&bank)?;
        let executable = crate::native_publication::native_publication_binding(runtime_dir, &bank["nativeBackend"])?;
        let native = crate::native_publication::native_publication_state(runtime_dir, &bank["nativeBackend"],
            &bank["universe"], &source, bank["sourcePrefixBytes"].as_u64().unwrap_or(0),
            bank["sourcePrefixHash"].as_str().unwrap_or(""), &dir.join("native-startup"))?;
        live(closed, failed, &dir)?;
        let mut publication = native["state"].clone();
        let mut state = crate::settlement_paper::create_settlement_paper_state(&bank)?;
        let mut offset = bank["sourcePrefixBytes"].as_u64().unwrap_or(0) as usize;
        let mut index = bank["sourcePrefixSequence"].as_u64().unwrap_or(0) as usize;
        let mut decision_serial = 0u64;
        need(native["sourceRecords"].as_u64().map(|u| u as usize) == Some(index)
            && if index == 0 { publication["hash"].is_null() } else {
                records.get(index - 1).map(|r| &r["hash"]) == Some(&publication["hash"])
            }
            && sha256(&bytes[..offset]) == bank["sourcePrefixHash"].as_str().unwrap_or("")
            && publication["hash"] == bank["sourcePrefixJournalHash"], "PREFIX")?;
        let dir_for_eval = dir.clone();
        let bank_for_eval = bank.clone();
        let exec_for_eval = executable.clone();
        let mut evaluate = move |input: &V| -> Result<V, String> {
            decision_serial += 1;
            let ddir = dir_for_eval.join("native-decisions")
                .join(format!("{}-{}", uuid_v4(), decision_serial));
            let r = crate::settlement_value_native::run_settlement_value(input, &ddir, &exec_for_eval)?;
            need(r["binaryHash"] == bank_for_eval["nativeBackend"]["binaryHash"]
                && r["result"]["engineHash"] == bank_for_eval["nativeBackend"]["engineHash"], "NATIVE_IDENTITY")?;
            Ok(r["result"].clone())
        };
        let mut sequence = 0i64;
        let mut previous = digest(&bank);
        let mut journal_bytes = 0u64;
        let mut journal_hash = sha256(&[]);
        if !initialize {
            let raw = std::fs::read(&journal_file).map_err(|e| e.to_string())?;
            for record in rows(&raw)? {
                let hash = record["hash"].as_str().unwrap_or("").to_string();
                let mut body = record.clone();
                body.as_object_mut().and_then(|m| m.shift_remove("hash"));
                sequence += 1;
                need(body["sequence"].as_i64() == Some(sequence)
                    && body["previous"].as_str().unwrap_or("") == previous
                    && body["bankHash"].as_str() == Some(&digest(&bank))
                    && digest(&body) == hash, "JOURNAL")?;
                if body["kind"].as_str() == Some("genesis") {
                    need(sequence == 1 && body["stateHash"].as_str() == Some(&digest(&state)), "GENESIS")?;
                } else {
                    need(body["kind"].as_str() == Some("batch")
                        && body["sourceSequence"].as_i64().unwrap_or(0) > publication["sequence"].as_i64().unwrap_or(0), "CHECKPOINT")?;
                    let mut batch = Vec::new();
                    while (publication["sequence"].as_i64().unwrap_or(0) + batch.len() as i64)
                        < body["sourceSequence"].as_i64().unwrap_or(0)
                    {
                        need(index < records.len(), "TRUNCATED")?;
                        batch.push(records[index].clone());
                        index += 1;
                    }
                    let at = body["availableAt"].as_str().unwrap_or("");
                    let account = &body["accountEligibility"];
                    for row in &batch {
                        publication = crate::book_publication_v2::apply_book_publication(&publication, row)?;
                        crate::settlement_paper::advance_settlement_paper(&mut state, row, &bank, at, account,
                            &mut evaluate)?;
                    }
                    for _ in 0..batch.len() {
                        offset = bytes[offset..].iter().position(|b| *b == b'\n')
                            .map(|p| offset + p + 1).unwrap_or(0);
                    }
                    let expected = json!({"sourceBytes": offset, "sourceSequence": publication["sequence"],
                        "sourceHash": publication["hash"], "stateHash": digest(&state)});
                    need(expected.as_object().unwrap().iter().all(|(k, v)| {
                        let got = &body[k.as_str()];
                        if v.is_number() || got.is_number() {
                            got.as_f64() == v.as_f64()
                        } else {
                            got == v
                        }
                    }), "REPLAY")?;
                }
                previous = hash;
            }
            need(sequence > 0, "MISSING_GENESIS")?;
            journal_bytes = raw.len() as u64;
            journal_hash = sha256(&raw);
        }
        let mut host = Self {
            dir, bank, runtime_dir: runtime_dir.to_path_buf(), workspace_dir: workspace_dir.to_path_buf(),
            live_paper_dir: live_paper_dir.to_path_buf(), source, journal_file, bank_file, lock_file,
            token, source_ino: source_stat.ino(), source_btime: source_stat.created().ok(),
            code_identity, executable, bank_bytes_hash: String::new(),
            prefix_hash: String::new(), publication, state, bytes, offset,
            decision_serial, sequence, previous, journal_bytes, journal_hash,
            startup: V::Null, closed, failed, now,
        };
        if initialize {
            write(&host.bank_file, (serde_json::to_string_pretty(&host.bank).map_err(|e| e.to_string())? + "\n").as_bytes(), false)?;
            write(&host.journal_file, &[], false)?;
            host.append(&json!({"kind": "genesis", "stateHash": digest(&host.state)}))?;
        }
        host.startup = json!({"coldBootMs": start.elapsed().as_secs_f64() * 1000.0,
            "nativeVerifyMs": native["verifyMs"],
            "sourcePrefixBytes": host.bank["sourcePrefixBytes"]});
        host.bank_bytes_hash = sha256(&std::fs::read(&host.bank_file).map_err(|e| e.to_string())?);
        host.prefix_hash = sha256(&host.bytes[..host.offset]);
        Ok(host)
    }
    fn stopped(&self) -> bool {
        [self.workspace_dir.join("STOP"), self.runtime_dir.join("STOP"), self.dir.join("STOP"), self.dir.join("OBSERVER_STOP")]
            .iter().any(|f| f.exists())
    }
    fn live(&self) -> Result<(), String> {
        need(!self.closed && !self.failed && !self.stopped()
            && std::fs::read_to_string(&self.lock_file).map_err(|e| e.to_string())? == self.token, "OWNERSHIP")
    }
    fn expected(&self) -> V {
        json!({"sourceBytes": self.offset, "sourceSequence": self.publication["sequence"],
            "sourceHash": self.publication["hash"], "stateHash": digest(&self.state)})
    }
    fn verify_journal(&self) -> Result<(), String> {
        let b = std::fs::read(&self.journal_file).map_err(|e| e.to_string())?;
        need(b.len() as u64 == self.journal_bytes && sha256(&b) == self.journal_hash, "JOURNAL_CHANGED")
    }
    fn evaluate(&mut self, input: &V) -> Result<V, String> {
        self.decision_serial += 1;
        let ddir = self.dir.join("native-decisions")
            .join(format!("{}-{}", uuid_v4(), self.decision_serial));
        let r = crate::settlement_value_native::run_settlement_value(input, &ddir, &self.executable)?;
        need(r["binaryHash"] == self.bank["nativeBackend"]["binaryHash"]
            && r["result"]["engineHash"] == self.bank["nativeBackend"]["engineHash"], "NATIVE_IDENTITY")?;
        Ok(r["result"].clone())
    }
    fn reduce(&mut self, batch: &[V], at: &str, account: &V) -> Result<(), String> {
        let mut serial = self.decision_serial;
        let dir = self.dir.clone();
        let bank = self.bank.clone();
        let executable = self.executable.clone();
        let mut evaluate = move |input: &V| -> Result<V, String> {
            serial += 1;
            let ddir = dir.join("native-decisions").join(format!("{}-{}", uuid_v4(), serial));
            let r = crate::settlement_value_native::run_settlement_value(input, &ddir, &executable)?;
            need(r["binaryHash"] == bank["nativeBackend"]["binaryHash"]
                && r["result"]["engineHash"] == bank["nativeBackend"]["engineHash"], "NATIVE_IDENTITY")?;
            Ok(r["result"].clone())
        };
        for row in batch {
            self.publication = crate::book_publication_v2::apply_book_publication(&self.publication, row)?;
            crate::settlement_paper::advance_settlement_paper(&mut self.state, row, &self.bank, at, account,
                &mut evaluate)?;
        }
        self.decision_serial = serial;
        Ok(())
    }
    fn append(&mut self, body: &V) -> Result<(), String> {
        self.live()?;
        self.verify_journal()?;
        let mut item = json!({"sequence": self.sequence + 1, "previous": self.previous,
            "bankHash": digest(&self.bank)});
        for (k, v) in body.as_object().cloned().unwrap_or_default() { item[k] = v; }
        let hash = digest(&item);
        item["hash"] = json!(hash);
        write(&self.journal_file,
            (serde_json::to_string(&item).map_err(|e| e.to_string())? + "\n").as_bytes(), true)?;
        self.sequence += 1;
        self.previous = hash;
        self.journal_bytes = std::fs::metadata(&self.journal_file).map_err(|e| e.to_string())?.len();
        self.journal_hash = sha256(&std::fs::read(&self.journal_file).map_err(|e| e.to_string())?);
        Ok(())
    }
    /// consume()
    pub fn consume(&mut self) -> Result<usize, String> {
        self.live()?;
        let r = (|| -> Result<usize, String> {
            crate::native_publication::native_publication_binding(&self.runtime_dir, &self.bank["nativeBackend"])?;
            need(sha256(&std::fs::read(&self.bank_file).map_err(|e| e.to_string())?) == self.bank_bytes_hash
                && digest(&settlement_paper_code_identity(&self.live_paper_dir)?) == digest(&self.code_identity), "IDENTITY_CHANGED")?;
            self.verify_journal()?;
            let stat = std::fs::metadata(&self.source).map_err(|e| e.to_string())?;
            need(stat.ino() == self.source_ino && stat.created().ok() == self.source_btime
                && stat.len() >= self.offset as u64, "SOURCE_REPLACED")?;
            let b = complete(&self.source)?;
            need(sha256(&b[..self.offset]) == self.prefix_hash, "SOURCE_CHANGED")?;
            if b.len() == self.offset { return Ok(0); }
            let batch = rows(&b[self.offset..])?;
            let at = baal_service_runtime::contract::ms_to_iso((self.now)() as f64);
            let mut account = V::Null;
            if let Ok(account_raw) = std::fs::read(self.runtime_dir.join("official-account.json")) {
                if let Ok(account_json) = serde_json::from_slice::<V>(&account_raw) {
                    if let Some(a) = crate::probability_receipt::probability_account_eligibility(&account_json) {
                        account = a;
                    }
                }
            }
            let n = batch.len();
            self.reduce(&batch, &at, &account)?;
            self.append(&json!({"kind": "batch", "availableAt": at, "accountEligibility": account,
                "sourceBytes": b.len(), "sourceSequence": self.publication["sequence"],
                "sourceHash": self.publication["hash"], "stateHash": digest(&self.state)}))?;
            self.offset = b.len();
            self.prefix_hash = sha256(&b);
            Ok(n)
        })();
        if r.is_err() { self.failed = true; }
        r
    }
    /// status()
    pub fn status(&self) -> Result<V, String> {
        let mut out = crate::settlement_paper::settlement_paper_report(&self.state, &self.bank,
            &baal_service_runtime::contract::ms_to_iso((self.now)() as f64))?;
        for (k, v) in self.startup.as_object().cloned().unwrap_or_default() { out[k] = v; }
        for (k, v) in self.expected().as_object().cloned().unwrap_or_default() { out[k] = v; }
        out["journalSequence"] = json!(self.sequence);
        out["journalHash"] = json!(self.previous);
        out["sourceClosed"] = self.publication["closed"].clone();
        out["failed"] = json!(self.failed);
        Ok(out)
    }
    /// publish()
    pub fn publish(&self) -> Result<(), String> {
        atomic_json_canonical(&self.dir.join("stats.json"), &self.status()?)
    }
    /// close()
    pub fn close(&mut self) -> Result<(), String> {
        if self.closed { return Ok(()); }
        self.closed = true;
        need(std::fs::read_to_string(&self.lock_file).map_err(|e| e.to_string())? == self.token, "LOCK_CHANGED")?;
        std::fs::remove_file(&self.lock_file).map_err(|e| e.to_string())?;
        Ok(())
    }
}
impl PaperHost for SettlementPaperHost {
    fn dir(&self) -> &Path { &self.dir }
    fn bank(&self) -> &V { &self.bank }
    fn consume(&mut self) -> Result<usize, String> { SettlementPaperHost::consume(self) }
    fn status(&mut self) -> Result<V, String> { SettlementPaperHost::status(self) }
    fn publish(&mut self) -> Result<(), String> { SettlementPaperHost::publish(self) }
    fn close(&mut self) -> Result<(), String> { SettlementPaperHost::close(self) }
}
