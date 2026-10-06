// live-paper/probability-receipt-host-v2.mjs — owned-risk continuation receipt worker.
use serde_json::{json, Value as V};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use baal_service_runtime::json_text::digest;
use std::sync::Arc;
use std::io::Write as _;

pub type NowFn = Arc<dyn Fn() -> f64 + Send + Sync>;

// Shared host/journal helpers (previously carried by the v1 host port).
pub(crate) fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok {
        return Err(format!("PROBABILITY_HOST_{code}"));
    }
    Ok(())
}
pub(crate) fn sha(b: &[u8]) -> String {
    baal_service_runtime::json_text::sha256(b)
}
pub(crate) fn cohort_ok(s: &str) -> bool {
    let b = s.as_bytes();
    (2..=64).contains(&b.len())
        && b[0].is_ascii_lowercase()
        && b[1..].iter().all(|x| x.is_ascii_lowercase() || x.is_ascii_digit() || *x == b'-')
}
pub fn uuid_v4() -> String {
    let mut b = [0u8; 16];
    fs::File::open("/dev/urandom")
        .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut b))
        .expect("urandom");
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}
pub(crate) fn to_iso(ms: f64) -> Result<String, String> {
    if !ms.is_finite() {
        return Err("Invalid time value".into());
    }
    Ok(baal_service_runtime::contract::ms_to_iso(ms))
}
pub(crate) fn file_stat(meta: &fs::Metadata) -> (u64, Option<SystemTime>, u64) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        (meta.ino(), meta.created().ok(), meta.len())
    }
    #[cfg(not(unix))]
    {
        (0, meta.created().ok(), meta.len())
    }
}
// bytes.indexOf(10, offset) + 1 — 0 when no newline is found.
pub(crate) fn next_line_end(bytes: &[u8], offset: usize) -> usize {
    bytes[offset.min(bytes.len())..]
        .iter()
        .position(|&b| b == 10)
        .map(|i| offset.min(bytes.len()) + i + 1)
        .unwrap_or(0)
}
pub(crate) fn rows(b: &[u8]) -> Result<Vec<V>, String> {
    need(b.is_empty() || *b.last().unwrap() == 10, "PARTIAL_JOURNAL")?;
    let text = String::from_utf8_lossy(b);
    let parts: Vec<&str> = text.split('\n').collect();
    let mut out = Vec::new();
    for line in &parts[..parts.len() - 1] {
        out.push(serde_json::from_str(line).map_err(|e| e.to_string())?);
    }
    Ok(out)
}
pub(crate) fn complete(file: &Path) -> Result<Vec<u8>, String> {
    let b = fs::read(file).map_err(|e| e.to_string())?;
    let end = b.iter().rposition(|&x| x == 10).map(|i| i + 1).unwrap_or(0);
    Ok(b[..end].to_vec())
}
pub(crate) fn write(file: &Path, bytes: &[u8], flag: &str) -> Result<(), String> {
    let mut opt = fs::OpenOptions::new();
    opt.write(true);
    match flag {
        "wx" => {
            opt.create_new(true);
        }
        "a" => {
            opt.append(true);
        }
        _ => return Err(format!("bad flag {flag}")),
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opt.mode(0o600);
    }
    let mut fd = opt.open(file).map_err(|e| e.to_string())?;
    let mut offset = 0usize;
    while offset < bytes.len() {
        let n = fd.write(&bytes[offset..]).map_err(|e| e.to_string())?;
        need(n > 0, "WRITE")?;
        offset += n;
    }
    fd.sync_all().map_err(|e| e.to_string())
}
pub(crate) fn js_eq(a: &V, b: &V) -> bool {
    if let (Some(x), Some(y)) = (a.as_f64(), b.as_f64()) {
        return x == y;
    }
    a == b
}
/// String(number) — JS number formatting (finite values via ryu-js spelling).
pub(crate) fn fmt_js_num(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x == f64::INFINITY {
        return "Infinity".into();
    }
    if x == f64::NEG_INFINITY {
        return "-Infinity".into();
    }
    baal_service_runtime::json_text::js_stringify(&V::from(x))
}


/// probabilityCodeIdentityV2() — sha256 of each module file.
pub fn probability_code_identity_v2(source_dir: &Path) -> Result<V, String> {
    let mut out = serde_json::Map::new();
    for p in [
        "./probability-receipt-host-v2.mjs",
        "./probability-receipt-v2.mjs",
        "./probability-receipt.mjs",
        "./probability-paper.mjs",
        "./model.mjs",
        "./book-publication.mjs",
        "../paper/fixed.mjs",
        "../feed/contract.mjs",
        "../execution/ledger.mjs",
    ] {
        out.insert(
            p.to_string(),
            json!(sha(&fs::read(source_dir.join(p)).map_err(|e| e.to_string())?)),
        );
    }
    Ok(V::Object(out))
}
fn reduce_batch(
    bank: &V,
    batch: &[V],
    at: &str,
    account: &V,
    source_state: &V,
    wallet_state: &V,
) -> Result<(V, V), String> {
    let mut next_publication = source_state.clone();
    let mut next_state = wallet_state.clone();
    for row in batch {
        next_publication = crate::book_publication_v2::apply_book_publication(&next_publication, row)?;
        crate::probability_receipt_v2::advance_receipt_probability(&mut next_state, row, bank, at, Some(account))?;
    }
    Ok((next_publication, next_state))
}

pub struct ProbabilityReceiptHostV2 {
    pub dir: PathBuf,
    pub bank: V,
    cohort: String,
    closed: bool,
    failed: bool,
    token: String,
    lock_file: PathBuf,
    journal: PathBuf,
    bank_file: PathBuf,
    opening_file: PathBuf,
    source: PathBuf,
    source_stat: (u64, Option<SystemTime>, u64),
    code_identity: V,
    offset: usize,
    publication: V,
    state: V,
    sequence: u64,
    previous: String,
    journal_bytes: u64,
    journal_hash: String,
    bank_bytes_hash: String,
    prefix_hash: String,
    runtime_dir: PathBuf,
    workspace_dir: PathBuf,
    source_dir: PathBuf,
    now: NowFn,
}
impl ProbabilityReceiptHostV2 {
    fn stopped(&self) -> bool {
        [
            self.workspace_dir.join("STOP"),
            self.runtime_dir.join("STOP"),
            self.dir.join("STOP"),
        ]
        .iter()
        .any(|f| f.exists())
    }
    fn live(&self) -> Result<(), String> {
        let held = fs::read_to_string(&self.lock_file).map_err(|e| e.to_string())?;
        need(
            !self.closed && !self.failed && !self.stopped() && held == self.token,
            "OWNERSHIP",
        )
    }
    fn verify_journal(&self) -> Result<(), String> {
        let b = fs::read(&self.journal).map_err(|e| e.to_string())?;
        need(
            b.len() as u64 == self.journal_bytes && sha(&b) == self.journal_hash,
            "JOURNAL_CHANGED",
        )
    }
    fn expected(&self) -> V {
        json!({"sourceBytes": self.offset, "sourceSequence": self.publication["sequence"],
            "sourceHash": self.publication["hash"], "stateHash": digest(&self.state)})
    }
    fn append(&mut self, body: &V) -> Result<(), String> {
        self.live()?;
        self.verify_journal()?;
        let mut item = json!({"sequence": self.sequence + 1, "previous": self.previous, "bankHash": digest(&self.bank)});
        for (k, v) in body.as_object().unwrap() {
            item[k] = v.clone();
        }
        let hash = digest(&item);
        let mut line = item.as_object().unwrap().clone();
        line.insert("hash".to_string(), json!(hash));
        let bytes =
            format!("{}\n", baal_service_runtime::json_text::js_stringify(&V::Object(line))).into_bytes();
        write(&self.journal, &bytes, "a")?;
        self.sequence += 1;
        self.previous = hash;
        self.journal_bytes += bytes.len() as u64;
        self.journal_hash = sha(&fs::read(&self.journal).map_err(|e| e.to_string())?);
        Ok(())
    }
    pub fn consume(&mut self) -> Result<u64, String> {
        self.live()?;
        let run = |w: &mut Self| -> Result<u64, String> {
            need(
                sha(&fs::read(&w.bank_file).map_err(|e| e.to_string())?) == w.bank_bytes_hash
                    && digest(&probability_code_identity_v2(&w.source_dir)?) == digest(&w.code_identity)
                    && sha(&fs::read(&w.opening_file).map_err(|e| e.to_string())?)
                        == w.bank["accountingContinuation"]["openingFileHash"].as_str().unwrap_or(""),
                "IDENTITY_CHANGED",
            )?;
            w.verify_journal()?;
            let stat = fs::metadata(&w.source).map_err(|e| e.to_string())?;
            let cur = file_stat(&stat);
            need(
                cur.0 == w.source_stat.0 && cur.1 == w.source_stat.1 && cur.2 >= w.offset as u64,
                "SOURCE_REPLACED",
            )?;
            let b = complete(&w.source)?;
            let prefix_end = w.offset.min(b.len());
            need(sha(&b[..prefix_end]) == w.prefix_hash, "SOURCE_CHANGED")?;
            if b.len() == w.offset {
                return Ok(0);
            }
            let batch = rows(&b[prefix_end..])?;
            let at = to_iso((w.now)())?;
            let account: V = fs::read(w.runtime_dir.join("official-account.json"))
                .ok()
                .and_then(|x| serde_json::from_slice::<V>(&x).ok())
                .and_then(|c| crate::probability_receipt_v2::probability_account_eligibility(&c))
                .unwrap_or(V::Null);
            let (next_publication, next_state) =
                reduce_batch(&w.bank, &batch, &at, &account, &w.publication, &w.state)?;
            w.append(&json!({"kind":"batch","availableAt":at,"accountEligibility":account,
                "sourceBytes":b.len(),"sourceSequence":next_publication["sequence"],
                "sourceHash":next_publication["hash"],"stateHash":digest(&next_state)}))?;
            w.publication = next_publication;
            w.state = next_state;
            w.offset = b.len();
            w.prefix_hash = sha(&b);
            Ok(batch.len() as u64)
        };
        match run(self) {
            Ok(n) => Ok(n),
            Err(e) => {
                self.failed = true;
                Err(e)
            }
        }
    }
    pub fn status(&self) -> Result<V, String> {
        let at = to_iso((self.now)())?;
        let report = crate::probability_receipt_v2::receipt_probability_report(&self.state, &self.bank, &at);
        let mut out = report.as_object().unwrap().clone();
        out.insert("cohort".to_string(), json!(self.cohort));
        for (k, v) in self.expected().as_object().unwrap() {
            out.insert(k.clone(), v.clone());
        }
        out.insert("journalSequence".to_string(), json!(self.sequence));
        out.insert("journalHash".to_string(), json!(self.previous));
        out.insert("sourceClosed".to_string(), self.publication["closed"].clone());
        out.insert("failed".to_string(), json!(self.failed));
        Ok(V::Object(out))
    }
    pub fn close(&mut self) -> Result<(), String> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        need(
            fs::read_to_string(&self.lock_file).map_err(|e| e.to_string())? == self.token,
            "LOCK_CHANGED",
        )?;
        fs::remove_file(&self.lock_file).map_err(|e| e.to_string())
    }
}

/// openProbabilityReceipt (v2) — same signature as v1 plus the opening state file.
pub fn open_probability_receipt(
    runtime_dir: &Path,
    cohort: &str,
    initialize: bool,
    bundle: Option<&V>,
    now: NowFn,
    workspace_dir: &Path,
    source_dir: &Path,
) -> Result<ProbabilityReceiptHostV2, String> {
    need(runtime_dir.is_absolute() && cohort_ok(cohort), "DIRECTORY")?;
    let dir = runtime_dir.join("probability-receipt-cohorts").join(cohort);
    let bank_file = dir.join("bank.json");
    let journal = dir.join("journal.jsonl");
    let lock_file = dir.join("worker.lock");
    let source = runtime_dir.join("public-books.jsonl");
    let stopped = || {
        [
            workspace_dir.join("STOP"),
            runtime_dir.join("STOP"),
            dir.join("STOP"),
        ]
        .iter()
        .any(|f| f.exists())
    };
    need(!stopped(), "STOPPED")?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let token = serde_json::to_string(&json!({"pid": std::process::id(), "id": uuid_v4()}))
        .map_err(|e| e.to_string())?;
    write(&lock_file, token.as_bytes(), "wx")?;

    let open_inner = |token: &str| -> Result<ProbabilityReceiptHostV2, String> {
        let bytes = complete(&source)?;
        let records = rows(&bytes)?;
        let source_stat = file_stat(&fs::metadata(&source).map_err(|e| e.to_string())?);
        let code_identity = probability_code_identity_v2(source_dir)?;
        need(!records.is_empty(), "EMPTY")?;
        let bank: V;
        let mut offset = 0usize;
        if initialize {
            need(bundle.is_some() && !bank_file.exists() && !journal.exists(), "BUNDLE_OR_EXISTS")?;
            let bundle = bundle.unwrap();
            let universe: V = serde_json::from_slice(
                &fs::read(source_dir.join("price-universe.json")).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            let _template: V = serde_json::from_slice(
                &fs::read(source_dir.join("config.json")).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            let last = records.last().unwrap();
            let cutoff = to_iso(now())?;
            let last_at = baal_service_runtime::contract::parse_iso_ms(last["at"].as_str().unwrap_or(""));
            let cutoff_ms = baal_service_runtime::contract::parse_iso_ms(&cutoff);
            need(last_at.zip(cutoff_ms).is_some_and(|(a, c)| a <= c), "FUTURE_SOURCE")?;
            // JS would throw TypeError reading bundle.opening.priorBank.config here.
            if !bundle["opening"]["priorBank"]["config"].is_object() {
                return Err("Cannot read properties of undefined (reading 'priorBank')".into());
            }
            bank = json!({
                "schemaVersion": 3,
                "mode": "source_probability_owned_risk_continuation_v2",
                "name": cohort,
                "cutoff": cutoff,
                "universe": universe,
                "identities": bundle["identities"].clone(),
                "research": bundle["research"].clone(),
                "researchEvidence": bundle["researchEvidence"].as_array().cloned().unwrap_or_default(),
                "policy": crate::probability_receipt_v2::probability_receipt_policy_v2(),
                "config": bundle["opening"]["priorBank"]["config"].clone(),
                "accountingContinuation": bundle["accountingContinuation"].clone(),
                "sourcePrefixBytes": bytes.len(),
                "sourcePrefixHash": sha(&bytes),
                "sourcePrefixSequence": last["sequence"],
                "sourcePrefixJournalHash": last["hash"],
                "codeIdentity": code_identity,
            });
        } else {
            bank = serde_json::from_slice(&fs::read(&bank_file).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            need(digest(&bank["codeIdentity"]) == digest(&code_identity), "FROZEN_CODE")?;
        }
        crate::probability_receipt_v2::validate_probability_receipt_bank(&bank)?;
        let opening_file = dir.join("opening-state.json");
        let (opening, opening_bytes): (V, Vec<u8>) = if initialize {
            let o = bundle.unwrap()["opening"].clone();
            let b = format!("{}\n", serde_json::to_string_pretty(&o).map_err(|e| e.to_string())?).into_bytes();
            (o, b)
        } else {
            let b = fs::read(&opening_file).map_err(|e| e.to_string())?;
            (serde_json::from_slice(&b).map_err(|e| e.to_string())?, b)
        };
        need(
            sha(&opening_bytes) == bank["accountingContinuation"]["openingFileHash"].as_str().unwrap_or(""),
            "OPENING_FILE",
        )?;
        let mut publication = crate::book_publication_v2::create_book_publication_state(&bank["universe"])?;
        let mut state = crate::probability_receipt_v2::create_probability_receipt_state(&bank, &opening)?;
        let mut index = 0usize;
        let expected_keys = ["sourceBytes", "sourceSequence", "sourceHash", "stateHash"];
        while publication["sequence"].as_i64().unwrap_or(0)
            < bank["sourcePrefixSequence"].as_i64().unwrap_or(0)
        {
            need(
                fs::read_to_string(&lock_file).map_err(|e| e.to_string())? == token && !stopped(),
                "OWNERSHIP",
            )?;
            let row = records.get(index).cloned();
            index += 1;
            need(row.as_ref().is_some_and(|v| baal_service_runtime::json_text::js_truthy(Some(v))), "TRUNCATED")?;
            publication =
                crate::book_publication_v2::apply_book_publication(&publication, &row.unwrap())?;
            offset = next_line_end(&bytes, offset);
        }
        need(
            offset as u64 == bank["sourcePrefixBytes"].as_u64().unwrap_or(u64::MAX)
                && sha(&bytes[..offset.min(bytes.len())]) == bank["sourcePrefixHash"].as_str().unwrap_or("")
                && publication["hash"].as_str() == bank["sourcePrefixJournalHash"].as_str(),
            "PREFIX",
        )?;
        let mut sequence = 0u64;
        let mut previous = digest(&bank);
        let mut journal_bytes = 0u64;
        let mut journal_hash = sha(&[]);
        if !initialize {
            let raw = fs::read(&journal).map_err(|e| e.to_string())?;
            for record in rows(&raw)? {
                let mut body = record.clone();
                let hash = body
                    .as_object_mut()
                    .and_then(|m| m.shift_remove("hash"))
                    .unwrap_or(V::Null);
                sequence += 1;
                need(
                    body["sequence"].as_u64() == Some(sequence)
                        && body["previous"].as_str() == Some(previous.as_str())
                        && body["bankHash"].as_str() == Some(digest(&bank).as_str())
                        && digest(&body) == hash.as_str().unwrap_or(""),
                    "JOURNAL",
                )?;
                if body["kind"].as_str() == Some("genesis") {
                    need(
                        sequence == 1 && body["stateHash"].as_str() == Some(digest(&state).as_str()),
                        "GENESIS",
                    )?;
                } else {
                    need(
                        body["kind"].as_str() == Some("batch")
                            && body["sourceSequence"].as_i64().unwrap_or(0)
                                > publication["sequence"].as_i64().unwrap_or(0),
                        "CHECKPOINT",
                    )?;
                    let mut batch = Vec::new();
                    while (publication["sequence"].as_i64().unwrap_or(0) + batch.len() as i64)
                        < body["sourceSequence"].as_i64().unwrap_or(0)
                    {
                        need(records.get(index).is_some_and(|v| baal_service_runtime::json_text::js_truthy(Some(v))), "SOURCE_TRUNCATED")?;
                        batch.push(records[index].clone());
                        index += 1;
                    }
                    let (next_pub, next_state) = reduce_batch(
                        &bank,
                        &batch,
                        body["availableAt"].as_str().unwrap_or(""),
                        &body["accountEligibility"],
                        &publication,
                        &state,
                    )?;
                    publication = next_pub;
                    state = next_state;
                    for _ in 0..batch.len() {
                        offset = next_line_end(&bytes, offset);
                    }
                    let expected = json!({"sourceBytes": offset, "sourceSequence": publication["sequence"],
                        "sourceHash": publication["hash"], "stateHash": digest(&state)});
                    need(expected_keys.iter().all(|k| js_eq(&body[k], &expected[k])), "REPLAY")?;
                }
                previous = hash.as_str().unwrap_or("").to_string();
            }
            need(sequence > 0, "MISSING_GENESIS")?;
            journal_bytes = raw.len() as u64;
            journal_hash = sha(&raw);
        }
        let mut host = ProbabilityReceiptHostV2 {
            dir: dir.clone(),
            bank: bank.clone(),
            cohort: cohort.to_string(),
            closed: false,
            failed: false,
            token: token.to_string(),
            lock_file: lock_file.clone(),
            journal: journal.clone(),
            bank_file: bank_file.clone(),
            opening_file: opening_file.clone(),
            source: source.clone(),
            source_stat,
            code_identity,
            offset,
            publication,
            state,
            sequence,
            previous,
            journal_bytes,
            journal_hash,
            bank_bytes_hash: String::new(),
            prefix_hash: String::new(),
            runtime_dir: runtime_dir.to_path_buf(),
            workspace_dir: workspace_dir.to_path_buf(),
            source_dir: source_dir.to_path_buf(),
            now: now.clone(),
        };
        if initialize {
            write(&host.opening_file, &opening_bytes, "wx")?;
            write(
                &bank_file,
                format!("{}\n", serde_json::to_string_pretty(&host.bank).map_err(|e| e.to_string())?)
                    .as_bytes(),
                "wx",
            )?;
            write(&journal, &[], "wx")?;
            host.append(&json!({"kind":"genesis","stateHash":digest(&host.state)}))?;
        }
        host.bank_bytes_hash = sha(&fs::read(&bank_file).map_err(|e| e.to_string())?);
        host.prefix_hash = sha(&bytes[..host.offset.min(bytes.len())]);
        Ok(host)
    };
    match open_inner(&token) {
        Ok(h) => Ok(h),
        Err(e) => {
            if lock_file.exists() {
                match fs::read_to_string(&lock_file) {
                    Ok(content) if content == token => {
                        if let Err(x) = fs::remove_file(&lock_file) {
                            return Err(x.to_string());
                        }
                    }
                    Ok(_) => {}
                    Err(x) => return Err(x.to_string()),
                }
            }
            Err(e)
        }
    }
}
