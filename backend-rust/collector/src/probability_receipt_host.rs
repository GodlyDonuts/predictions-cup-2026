// live-paper/probability-receipt-host.mjs — locked single-writer host over
// public-books.jsonl for probability-receipt cohorts.
use serde_json::{json, Value as V};
use std::fs::OpenOptions;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

fn need(ok: bool, c: &str) -> Result<(), String> {
    if !ok { return Err(format!("PROBABILITY_HOST_{}", c)); }
    Ok(())
}
fn sha(b: &[u8]) -> String { baal_service_runtime::json_text::sha256(b) }
fn digest(v: &V) -> String { baal_service_runtime::json_text::digest(v) }
fn complete(file: &Path) -> Result<Vec<u8>, String> {
    let b = std::fs::read(file).map_err(|e| e.to_string())?;
    let end = b.iter().rposition(|x| *x == b'\n').map(|p| p + 1).unwrap_or(0);
    Ok(b[..end].to_vec())
}
fn rows(b: &[u8]) -> Result<Vec<V>, String> {
    need(b.is_empty() || *b.last().unwrap() == b'\n', "PROBABILITY_HOST_PARTIAL_JOURNAL")?;
    if b.is_empty() { return Ok(vec![]); }
    let text = String::from_utf8(b.to_vec()).map_err(|e| e.to_string())?;
    let mut parts: Vec<&str> = text.split('\n').collect();
    parts.pop();
    parts.iter().map(|x| serde_json::from_str(x).map_err(|e| e.to_string())).collect()
}
fn write(file: &Path, bytes: &[u8], append: bool, create_new: bool) -> Result<(), String> {
    let mut o = OpenOptions::new();
    if create_new { o.write(true).create_new(true); } else if append { o.append(true); } else { o.write(true).truncate(true); }
    #[cfg(unix)]
    o.mode(0o600);
    let mut fd = o.open(file).map_err(|e| e.to_string())?;
    fd.write_all(bytes).map_err(|_| "PROBABILITY_HOST_WRITE".to_string())?;
    fd.sync_all().map_err(|e| e.to_string())
}
fn read_json(file: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(file).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn uuid_like() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let r = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos()
        ^ ((std::process::id() as u128) << 64);
    format!("{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        (r & 0xffffffff) as u32, (r >> 32 & 0xffff) as u16, (r >> 48 & 0xffff) as u16,
        (r >> 64 & 0xffff) as u16, (r >> 80 & 0xffffffffffff) as u64)
}
fn identity(workspace_dir: &Path) -> Result<V, String> {
    let keys = ["./probability-receipt-host.mjs", "./probability-receipt.mjs",
        "./probability-paper.mjs", "./model.mjs", "./book-publication.mjs",
        "../paper/fixed.mjs", "../feed/contract.mjs", "../execution/ledger.mjs"];
    let files = ["live-paper/probability-receipt-host.mjs", "live-paper/probability-receipt.mjs",
        "live-paper/probability-paper.mjs", "live-paper/model.mjs", "live-paper/book-publication.mjs",
        "paper/fixed.mjs", "feed/contract.mjs", "execution/ledger.mjs"];
    let mut m = serde_json::Map::new();
    for (k, p) in keys.iter().zip(files.iter()) {
        m.insert(k.to_string(), json!(sha(&std::fs::read(workspace_dir.join(p)).map_err(|e| e.to_string())?)));
    }
    Ok(V::Object(m))
}
fn stat_of(file: &Path) -> Result<(u64, f64, u64), String> {
    let m = std::fs::metadata(file).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    return Ok((m.ino(), m.created().map_err(|e| e.to_string())?
        .duration_since(std::time::UNIX_EPOCH).map_err(|e| e.to_string())?.as_secs_f64() * 1000.0, m.len()));
    #[cfg(not(unix))]
    return Ok((0, 0.0, m.len()));
}

/// openProbabilityReceipt host — JS closure object as struct.
pub struct ProbabilityReceiptHost {
    pub dir: PathBuf,
    pub bank: V,
    state: V,
    runtime_dir: PathBuf,
    workspace_dir: PathBuf,
    cohort: String,
    code_identity: V,
    publication: V,
    sequence: i64,
    previous: String,
    journal_bytes: u64,
    journal_hash_bytes: String,
    offset: usize,
    bank_bytes_hash: String,
    prefix_hash: String,
    source_stat: (u64, f64, u64),
    lock_file: PathBuf,
    token: String,
    closed: bool,
    failed: bool,
    now: Box<dyn Fn() -> f64>,
}
impl ProbabilityReceiptHost {
    fn source_file(&self) -> PathBuf { self.runtime_dir.join("public-books.jsonl") }
    fn bank_file(&self) -> PathBuf { self.dir.join("bank.json") }
    fn journal_file(&self) -> PathBuf { self.dir.join("journal.jsonl") }
    pub fn stopped(&self) -> bool {
        [self.workspace_dir.join("STOP"), self.runtime_dir.join("STOP"), self.dir.join("STOP")]
            .iter().any(|p| p.exists())
    }
    fn live(&self) -> Result<(), String> {
        need(!self.closed && !self.failed && !self.stopped()
            && std::fs::read_to_string(&self.lock_file).map(|s| s == self.token).unwrap_or(false),
            "OWNERSHIP")
    }
    fn expected(&self) -> V {
        json!({"sourceBytes": self.offset, "sourceSequence": self.publication["sequence"],
            "sourceHash": self.publication["hash"], "stateHash": digest(&self.state)})
    }
    fn verify_journal(&self) -> Result<(), String> {
        let b = std::fs::read(self.journal_file()).map_err(|e| e.to_string())?;
        need(b.len() as u64 == self.journal_bytes && sha(&b) == self.journal_hash_bytes,
            "JOURNAL_CHANGED")
    }
    /// reduce(batch, at, account) — clone-apply-commit like JS reduce.
    fn reduce(&self, batch: &[V], at: &str, account: Option<&V>) -> Result<(V, V), String> {
        let mut next_publication = self.publication.clone();
        let mut next_state = self.state.clone();
        for row in batch {
            next_publication = crate::book_publication_v2::apply_book_publication(&next_publication, row)?;
            let account_v = account.cloned().unwrap_or(V::Null);
            next_state = crate::probability_receipt::advance_receipt_probability(&next_state, row,
                &self.bank, at, &account_v)?;
        }
        Ok((next_publication, next_state))
    }
    fn append(&mut self, body: V) -> Result<(), String> {
        self.live()?;
        self.verify_journal()?;
        let mut item = json!({"sequence": self.sequence + 1, "previous": self.previous,
            "bankHash": digest(&self.bank)});
        for (k, v) in body.as_object().unwrap() { item[k] = v.clone(); }
        let hash = digest(&item);
        item["hash"] = json!(hash);
        let b = format!("{}\n", baal_service_runtime::json_text::js_stringify(&item));
        write(&self.journal_file(), b.as_bytes(), true, false)?;
        self.sequence += 1;
        self.previous = hash;
        self.journal_bytes += b.len() as u64;
        self.journal_hash_bytes = sha(&std::fs::read(self.journal_file()).map_err(|e| e.to_string())?);
        Ok(())
    }
    /// consume() — number of source records consumed this pass.
    pub fn consume(&mut self) -> Result<usize, String> {
        self.live()?;
        let r = (|h: &mut Self| -> Result<usize, String> {
            need(sha(&std::fs::read(h.bank_file()).map_err(|e| e.to_string())?) == h.bank_bytes_hash
                && digest(&identity(&h.workspace_dir)?) == digest(&h.code_identity), "IDENTITY_CHANGED")?;
            h.verify_journal()?;
            let stat = stat_of(&h.source_file())?;
            need(stat.0 == h.source_stat.0 && stat.1 == h.source_stat.1
                && stat.2 >= h.offset as u64, "SOURCE_REPLACED")?;
            let b = complete(&h.source_file())?;
            need(sha(&b[..h.offset]) == h.prefix_hash, "SOURCE_CHANGED")?;
            if b.len() == h.offset { return Ok(0); }
            let batch = rows(&b[h.offset..])?;
            let at = baal_service_runtime::contract::ms_to_iso((h.now)());
            let account = read_json(&h.runtime_dir.join("official-account.json")).ok()
                .and_then(|cache| crate::probability_receipt::probability_account_eligibility(&cache));
            let (next_pub, next_state) = h.reduce(&batch, &at, account.as_ref())?;
            let mut body = json!({"kind": "batch", "availableAt": at,
                "accountEligibility": account});
            body["sourceBytes"] = json!(b.len());
            body["sourceSequence"] = next_pub["sequence"].clone();
            body["sourceHash"] = next_pub["hash"].clone();
            body["stateHash"] = json!(digest(&next_state));
            h.append(body)?;
            h.publication = next_pub;
            h.state = next_state;
            h.offset = b.len();
            h.prefix_hash = sha(&b);
            Ok(batch.len())
        })(self);
        if r.is_err() { self.failed = true; }
        r
    }
    /// status()
    pub fn status(&self) -> V {
        let mut out = crate::probability_receipt::receipt_probability_report(&self.state, &self.bank,
            &baal_service_runtime::contract::ms_to_iso((self.now)()));
        out["cohort"] = json!(self.cohort);
        for (k, v) in self.expected().as_object().unwrap() { out[k] = v.clone(); }
        out["journalSequence"] = json!(self.sequence);
        out["journalHash"] = json!(self.previous);
        out["sourceClosed"] = self.publication["closed"].clone();
        out["failed"] = json!(self.failed);
        out
    }
    pub fn close(&mut self) -> Result<(), String> {
        if self.closed { return Ok(()); }
        self.closed = true;
        need(std::fs::read_to_string(&self.lock_file).map(|s| s == self.token).unwrap_or(false),
            "PROBABILITY_HOST_LOCK_CHANGED")?;
        std::fs::remove_file(&self.lock_file).map_err(|e| e.to_string())
    }
}
/// openProbabilityReceipt({runtimeDir, cohort, initialize, bundle, now, workspaceDir})
pub fn open_probability_receipt(runtime_dir: &Path, cohort: &str, initialize: bool,
    bundle: Option<&V>, now: impl Fn() -> f64 + 'static,
    workspace_dir: &Path) -> Result<ProbabilityReceiptHost, String> {
    let cohort_ok = cohort.len() >= 2 && cohort.len() <= 64
        && cohort.as_bytes()[0].is_ascii_lowercase()
        && cohort.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    need(runtime_dir.is_absolute() && cohort_ok, "DIRECTORY")?;
    let dir = runtime_dir.join("probability-receipt-cohorts").join(cohort);
    let bank_file = dir.join("bank.json");
    let journal_file = dir.join("journal.jsonl");
    let lock_file = dir.join("worker.lock");
    let source_file = runtime_dir.join("public-books.jsonl");
    let stopped = |workspace_dir: &Path| -> bool {
        [workspace_dir.join("STOP"), runtime_dir.join("STOP"), dir.join("STOP")]
            .iter().any(|p| p.exists())
    };
    need(!stopped(workspace_dir), "STOPPED")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let token = format!("{}", json!({"pid": std::process::id(), "id": uuid_like()}));
    let r = (|| -> Result<ProbabilityReceiptHost, String> {
        write(&lock_file, token.as_bytes(), false, true)?;
        let code_identity = identity(workspace_dir)?;
        let source_stat = stat_of(&source_file)?;
        let bytes = complete(&source_file)?;
        let records = rows(&bytes)?;
        need(!records.is_empty(), "EMPTY")?;
        let bank: V;
        if initialize {
            need(bundle.is_some() && !bank_file.exists() && !journal_file.exists(), "BUNDLE_OR_EXISTS")?;
            let bundle = bundle.unwrap();
            let universe = read_json(&workspace_dir.join("live-paper/price-universe.json"))?;
            let template = read_json(&workspace_dir.join("live-paper/config.json"))?;
            let last = records.last().unwrap();
            let cutoff = baal_service_runtime::contract::ms_to_iso(now());
            need(baal_service_runtime::contract::parse_iso_ms(last["at"].as_str().unwrap_or(""))
                .unwrap_or(f64::NAN)
                <= baal_service_runtime::contract::parse_iso_ms(&cutoff).unwrap_or(f64::NAN), "FUTURE_SOURCE")?;
            let mut config = template.clone();
            config["strategyUniverse"] = json!("diversified-dip-v1");
            config["marketIds"] = json!(bundle["identities"].as_object().cloned().unwrap_or_default()
                .keys().cloned().collect::<Vec<String>>());
            config["initialCash"] = json!("100.000000");
            config["cashReserve"] = json!("90.000000");
            config["maxExposure"] = json!("10.000000");
            config["feeBps"] = json!(0);
            config["slippageTicks"] = json!(1);
            config["depthFractionBps"] = json!(100);
            config["orderShares"] = json!(1);
            config["maxPositionShares"] = json!(1);
            config["stopLoss"] = json!("0.050000");
            config["exitAfterMs"] = crate::probability_receipt::probability_receipt_policy()["maxHoldMs"].clone();
            bank = json!({"schemaVersion":2,"mode":"source_probability_actual_receipt_paper",
                "name":cohort,"cutoff":cutoff,"universe":universe,
                "identities":bundle["identities"],"research":bundle["research"],
                "researchEvidence":bundle["researchEvidence"].as_array().cloned().unwrap_or_default(),
                "policy":crate::probability_receipt::probability_receipt_policy(),"config":config,
                "sourcePrefixBytes":bytes.len() as i64,"sourcePrefixHash":sha(&bytes),
                "sourcePrefixSequence":last["sequence"],"sourcePrefixJournalHash":last["hash"],
                "codeIdentity":code_identity});
        } else {
            bank = read_json(&bank_file)?;
            need(digest(&bank["codeIdentity"]) == digest(&code_identity), "FROZEN_CODE")?;
        }
        crate::probability_receipt::validate_probability_receipt_bank(&bank)?;
        let mut publication = crate::book_publication_v2::create_book_publication_state(&bank["universe"])?;
        let state = crate::probability_receipt::create_probability_receipt_state(&bank)?;
        let mut offset = 0usize;
        let mut index = 0usize;
        while publication["sequence"].as_i64().unwrap_or(0) < bank["sourcePrefixSequence"].as_i64().unwrap_or(0) {
            need(std::fs::read_to_string(&lock_file).map(|s| s == token).unwrap_or(false)
                && !stopped(workspace_dir), "OWNERSHIP")?;
            need(index < records.len(), "TRUNCATED")?;
            let row = &records[index];
            index += 1;
            publication = crate::book_publication_v2::apply_book_publication(&publication, row)?;
            offset = bytes[offset..].iter().position(|x| *x == b'\n').map(|p| p + offset + 1).unwrap_or(offset);
        }
        need(offset == bank["sourcePrefixBytes"].as_i64().unwrap_or(-1) as usize
            && sha(&bytes[..offset]) == bank["sourcePrefixHash"].as_str().unwrap_or("")
            && publication["hash"].as_str() == Some(bank["sourcePrefixJournalHash"].as_str().unwrap_or("")),
            "PREFIX")?;
        let mut sequence = 0i64;
        let mut previous = digest(&bank);
        let mut journal_bytes = 0u64;
        let mut journal_bytes_hash = sha(&[]);
        let mut host = ProbabilityReceiptHost {
            dir: dir.clone(), bank: bank.clone(), state, runtime_dir: runtime_dir.to_path_buf(),
            workspace_dir: workspace_dir.to_path_buf(), cohort: cohort.to_string(),
            code_identity, publication, sequence, previous: previous.clone(), journal_bytes,
            journal_hash_bytes: journal_bytes_hash.clone(),
            offset, bank_bytes_hash: String::new(), prefix_hash: String::new(), source_stat,
            lock_file: lock_file.clone(), token: token.clone(), closed: false, failed: false,
            now: Box::new(now),
        };
        if !initialize {
            let raw = std::fs::read(&journal_file).map_err(|e| e.to_string())?;
            for record in rows(&raw)? {
                let mut body = record.clone();
                let hash = body.as_object_mut().and_then(|m| m.shift_remove("hash")).unwrap_or(V::Null);
                sequence += 1;
                need(body["sequence"].as_i64() == Some(sequence)
                    && body["previous"].as_str() == Some(previous.as_str())
                    && body["bankHash"].as_str() == Some(digest(&bank).as_str())
                    && hash.as_str() == Some(digest(&body).as_str()), "JOURNAL")?;
                if body["kind"].as_str() == Some("genesis") {
                    need(sequence == 1 && body["stateHash"].as_str() == Some(digest(&host.state).as_str()),
                        "GENESIS")?;
                } else {
                    need(body["kind"].as_str() == Some("batch")
                        && body["sourceSequence"].as_i64().unwrap_or(0)
                            > host.publication["sequence"].as_i64().unwrap_or(0), "CHECKPOINT")?;
                    let mut batch = Vec::new();
                    while host.publication["sequence"].as_i64().unwrap_or(0) + (batch.len() as i64)
                        < body["sourceSequence"].as_i64().unwrap_or(0) {
                        need(index < records.len(), "SOURCE_TRUNCATED")?;
                        batch.push(records[index].clone());
                        index += 1;
                    }
                    let (next_pub, next_state) = host.reduce(&batch,
                        body["availableAt"].as_str().unwrap_or(""),
                        if body["accountEligibility"].is_null() { None } else { Some(&body["accountEligibility"]) })?;
                    host.publication = next_pub;
                    host.state = next_state;
                    for _ in 0..batch.len() {
                        offset = bytes[offset..].iter().position(|x| *x == b'\n').map(|p| p + offset + 1).unwrap_or(offset);
                    }
                    host.offset = offset;
                    need(host.expected().as_object().unwrap().iter()
                        .all(|(k, v)| &body[k] == v), "REPLAY")?;
                }
                previous = hash.as_str().unwrap_or("").to_string();
            }
            need(sequence > 0, "MISSING_GENESIS")?;
            journal_bytes = raw.len() as u64;
            journal_bytes_hash = sha(&raw);
            host.sequence = sequence;
            host.previous = previous;
            host.journal_bytes = journal_bytes;
            host.journal_hash_bytes = journal_bytes_hash;
            host.offset = offset;
        }
        if initialize {
            write(&bank_file, format!("{}\n", serde_json::to_string_pretty(&bank).map_err(|e| e.to_string())?).as_bytes(), false, true)?;
            write(&journal_file, &[], false, true)?;
            host.append(json!({"kind": "genesis", "stateHash": digest(&host.state)}))?;
        }
        host.bank_bytes_hash = sha(&std::fs::read(&bank_file).map_err(|e| e.to_string())?);
        host.prefix_hash = sha(&bytes[..host.offset]);
        Ok(host)
    })();
    if r.is_err() {
        if std::fs::read_to_string(&lock_file).map(|s| s == token).unwrap_or(false) {
            let _ = std::fs::remove_file(&lock_file);
        }
    }
    r
}
