// live-paper/settlement-portfolio-host-v2.mjs — exclusive-native-journal host.
use baal_execution::core::payload_hash;
use baal_service_runtime::atomic::atomic_json_canonical;
use baal_service_runtime::journal::{journal_lines, JournalReadOptions};
use baal_service_runtime::json_text::{js_stringify, sha256};
use serde_json::{json, Map, Value as V};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::native_journal_append::ExclusiveNativeJournal;
use crate::settlement_paper_host::PaperHost;

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(format!("SETTLEMENT_PORTFOLIO_HOST_{}", code)); }
    Ok(())
}
fn sha(b: &[u8]) -> String { sha256(b) }
fn hex64(v: &V) -> bool { v.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())) }
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
/// settlementPortfolioHostCodeIdentity({engineFile}) — import-graph traversal
/// rooted at this module + the engine file. live_paper_dir stands in for
/// import.meta.url's directory; keys are './'-prefixed relative paths sorted
/// by localeCompare (byte order on ASCII).
pub fn settlement_portfolio_host_code_identity(live_paper_dir: &Path, engine_file: &Path) -> Result<V, String> {
    let root = live_paper_dir.canonicalize().map_err(|e| e.to_string())?;
    let mut seen: std::collections::BTreeMap<String, String> = Default::default();
    fn find_imports(text: &str) -> Vec<String> {
        let mut out = Vec::new();
        let bytes = text.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            let w = if text[i..].starts_with("import") { "import" } else if text[i..].starts_with("export") { "export" } else { "" };
            if !w.is_empty() {
                let start = i + w.len();
                let boundary = i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
                if boundary && start < bytes.len() && bytes[start].is_ascii_whitespace() {
                    let end = bytes.len();
                    let mut j = start;
                    while j < end {
                        let c = bytes[j];
                        if c == b';' { break; }
                        if c == b'\'' || c == b'"' {
                            let mut k = j + 1;
                            while k < end && bytes[k] != c { k += 1; }
                            if k < end {
                                let spec = &text[j + 1..k];
                                if spec.starts_with('.') && spec.ends_with(".mjs") { out.push(spec.to_string()); }
                                j = k;
                            }
                        }
                        j += 1;
                    }
                }
            }
            i += 1;
        }
        out
    }
    fn visit(file: &Path, root: &Path, seen: &mut std::collections::BTreeMap<String, String>) -> Result<(), String> {
        let file = file.canonicalize().map_err(|e| e.to_string())?;
        let relative = file.strip_prefix(root)
            .map(|p| format!("./{}", p.to_string_lossy().replace('\\', "/")))
            .unwrap_or_else(|_| {
                // compute ../-prefixed path relative to root
                let mut ups = 0usize;
                let mut cur = file.clone();
                loop {
                    if cur.starts_with(root) { break; }
                    match cur.parent() { Some(p) => { cur = p.to_path_buf(); ups += 1; } None => break }
                }
                let tail = file.strip_prefix(&cur).map(|p| p.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_else(|_| file.to_string_lossy().replace('\\', "/"));
                "../".repeat(ups) + &tail
            });
        if seen.contains_key(&relative) { return Ok(()); }
        let bytes = std::fs::read(&file).map_err(|e| e.to_string())?;
        seen.insert(relative, sha(&bytes));
        let text = String::from_utf8_lossy(&bytes);
        for spec in find_imports(&text) {
            visit(&file.parent().unwrap_or(root).join(&spec), root, seen)?;
        }
        Ok(())
    }
    visit(&live_paper_dir.join("settlement-portfolio-host-v2.mjs"), &root, &mut seen)?;
    visit(engine_file, &root, &mut seen)?;
    let mut out = Map::new();
    for (k, h) in seen { out.insert(k, json!(h)); }
    Ok(V::Object(out))
}
fn file_identity(s: &std::fs::Metadata) -> String {
    let btime_ms = s.created().ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| format!("{}", (d.as_secs_f64() * 1000.0) as u128)).unwrap_or_else(|| "0".to_string());
    format!("{}:{}:{}", s.dev(), s.ino(), btime_ms)
}
fn sealed_end(file: &Path) -> Result<u64, String> {
    let before = std::fs::metadata(file).map_err(|e| e.to_string())?;
    let take = before.len().min(1_048_576);
    let mut b = vec![0u8; take as usize];
    use std::io::{Read, Seek, SeekFrom};
    let mut fd = std::fs::File::open(file).map_err(|e| e.to_string())?;
    fd.seek(SeekFrom::Start(before.len() - take)).map_err(|e| e.to_string())?;
    fd.read_exact(&mut b).map_err(|e| e.to_string())?;
    let last = b.iter().rposition(|x| *x == b'\n').ok_or("SETTLEMENT_PORTFOLIO_HOST_EMPTY_SOURCE")?;
    Ok(before.len() - take + last as u64 + 1)
}
struct SourceBatch {
    rows: Vec<V>,
    publication: V,
    prefix: Sha256,
    offset: u64,
}
fn source_batch(lines: &[String], publication: V, prefix: Sha256, start: u64) -> Result<SourceBatch, String> {
    let mut rows = Vec::new();
    let mut publication = publication;
    let mut bytes = start;
    let mut h = prefix;
    for line in lines {
        need(!line.is_empty(), "EMPTY_PUBLIC_RECORD")?;
        let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
        need(js_stringify(&row) == *line, "SOURCE_CANONICAL_BYTES")?;
        publication = crate::book_publication_v2::apply_book_publication(&publication, &row)?;
        h.update(format!("{}\n", line).as_bytes());
        bytes += (line.len() + 1) as u64;
        rows.push(row);
    }
    Ok(SourceBatch { rows, publication, prefix: h, offset: bytes })
}
/// The bound native evaluator (default `evaluate`): runs the settlement-value
/// binary with the nativeBackend binding, verifying binary/engine hashes.
fn native_evaluate(runtime_dir: &Path, dir: &Path, bank: &V,
    serial: &Arc<Mutex<u64>>, input: &V) -> Result<V, String> {
    let executable = crate::native_publication::native_publication_binding(runtime_dir, &bank["nativeBackend"])?;
    let n = {
        let mut s = serial.lock().map_err(|_| "SETTLEMENT_PORTFOLIO_HOST_SERIAL".to_string())?;
        *s += 1;
        *s
    };
    let d = dir.join("native-decisions").join(format!("{}-{}", uuid_v4(), n));
    let result = crate::settlement_value_native::run_settlement_value(input, &d, &executable)?;
    need(result["binaryHash"] == bank["nativeBackend"]["binaryHash"]
        && result["result"]["engineHash"] == bank["nativeBackend"]["engineHash"], "NATIVE_VALUE_BINDING")?;
    Ok(result["result"].clone())
}
fn reduce(input: &V, batch: &[V], bank: &V, at: &str, account: &V,
    evaluate: &mut dyn FnMut(&V) -> Result<V, String>) -> Result<V, String> {
    let mut next = input.clone();
    for row in batch {
        need(baal_paper::fixed::instant(row["at"].as_str().unwrap_or(""))?
            <= baal_paper::fixed::instant(at)?, "FUTURE_SOURCE")?;
        next = crate::settlement_portfolio_paper_v2::advance_settlement_portfolio(
            &next, &row.clone(), bank, at, &account.clone(), evaluate)?;
        crate::settlement_portfolio_paper_v2::assert_settlement_portfolio_state(&next, bank)?;
    }
    Ok(next)
}
/// verifySettlementPortfolioSourcePrefix({file, universe, cutoff, now})
pub fn verify_settlement_portfolio_source_prefix(file: &Path, universe: &V,
    cutoff: Option<&str>, now: i64) -> Result<V, String> {
    need(file.is_absolute(), "SOURCE_PATH")?;
    let before = std::fs::metadata(file).map_err(|e| e.to_string())?;
    let end = sealed_end(file)?;
    let mut prefix = Sha256::new();
    let mut publication = crate::book_publication_v2::create_book_publication_state(universe)?;
    let mut bytes = 0u64;
    for line in journal_lines(file, &JournalReadOptions { end_byte: Some(end), ..Default::default() })? {
        need(!line.is_empty(), "EMPTY_PUBLIC_RECORD")?;
        let row: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        need(js_stringify(&row) == line, "SOURCE_CANONICAL_BYTES")?;
        publication = crate::book_publication_v2::apply_book_publication(&publication, &row)?;
        prefix.update(format!("{}\n", line).as_bytes());
        bytes += (line.len() + 1) as u64;
    }
    let prefix_hash = format!("{:x}", prefix.finalize());
    need(bytes == end
        && file_identity(&std::fs::metadata(file).map_err(|e| e.to_string())?) == file_identity(&before)
        && crate::native_journal_restore::source_prefix_hash(file, end, None)? == prefix_hash, "SOURCE_CHANGED_DURING_INITIALIZATION")?;
    let chosen = cutoff.map(String::from)
        .unwrap_or_else(|| baal_service_runtime::contract::ms_to_iso(now as f64));
    need(baal_paper::fixed::instant(&chosen)?
        >= baal_paper::fixed::instant(publication["lastAt"].as_str().unwrap_or(""))?, "FUTURE_SOURCE_CUTOFF")?;
    Ok(json!({"sourcePrefixBytes": end, "sourcePrefixHash": prefix_hash,
        "sourcePrefixSequence": publication["sequence"], "sourcePrefixJournalHash": publication["hash"],
        "cutoff": chosen, "sourceSessionId": publication["sessionId"],
        "sourceFileIdentity": file_identity(&before), "readOnly": true,
        "producerExclusivityEstablished": false}))
}
/// openSettlementPortfolioHostV2({runtimeDir, cohort, append, now, canOperate,
/// workspaceDir, engineFile, maxBatchRows})
pub struct SettlementPortfolioHostV2 {
    pub dir: PathBuf,
    pub bank: V,
    journal: Option<ExclusiveNativeJournal>,
    lock: PathBuf,
    token: String,
    source: PathBuf,
    bank_file: PathBuf,
    bank_bytes: Vec<u8>,
    live_paper_dir: PathBuf,
    engine_file: PathBuf,
    runtime_dir: PathBuf,
    source_id: String,
    publication: V,
    state: V,
    prefix: Sha256,
    offset: u64,
    closed: bool,
    failed: bool,
    code_identity: V,
    shared_head: Arc<Mutex<(V, Sha256, u64)>>,
    source_bytes_verified: u64,
    public_prefix_verify_ms: f64,
    startup: V,
    evaluation_serial: Arc<Mutex<u64>>,
    max_batch_rows: i64,
    now: Arc<dyn Fn() -> i64 + Send + Sync>,
    can_operate: Arc<dyn Fn() -> bool + Send + Sync>,
    stop_paths: Vec<PathBuf>,
    canonical_bank_hash: String,
}
impl SettlementPortfolioHostV2 {
    #[allow(clippy::too_many_arguments)]
    pub fn open(runtime_dir: &Path, cohort: &str, append: &V,
        now: Arc<dyn Fn() -> i64 + Send + Sync>, can_operate: Arc<dyn Fn() -> bool + Send + Sync>,
        workspace_dir: &Path, live_paper_dir: &Path, max_batch_rows: i64) -> Result<Self, String> {
        let cb = cohort.as_bytes();
        need(runtime_dir.is_absolute()
            && cb.len() >= 2 && cb.len() <= 64 && cb[0].is_ascii_lowercase()
            && cb[1..].iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
            && max_batch_rows > 0 && max_batch_rows <= 1000
            && append.is_object(), "OPTIONS")?;
        let dir = runtime_dir.join("settlement-portfolio-v2-cohorts").join(cohort);
        let bank_file = dir.join("bank.json");
        let journal_file = dir.join("journal.jsonl");
        let source = runtime_dir.join("public-books.jsonl");
        let lock = dir.join("worker.lock");
        let stop_paths = vec![workspace_dir.join("STOP"), runtime_dir.join("STOP"),
            runtime_dir.join("COLLECTOR_STOP"), runtime_dir.join("EXECUTION_STOP"),
            dir.join("STOP"), dir.join("OBSERVER_STOP")];
        need(can_operate() && !stop_paths.iter().any(|p| p.exists()), "STOPPED")?;
        need(bank_file.exists(), "SAVED_FUTURE_BANK_REQUIRED")?;
        let bank_bytes = std::fs::read(&bank_file).map_err(|e| e.to_string())?;
        let bank: V = serde_json::from_slice(&bank_bytes).map_err(|e| e.to_string())?;
        let canonical_bank_hash = payload_hash(&bank)?;
        let engine_file = live_paper_dir.join("settlement-portfolio-paper-v2.mjs");
        let code_identity = settlement_portfolio_host_code_identity(live_paper_dir, &engine_file)?;
        need(bank["name"].as_str() == Some(cohort)
            && payload_hash(&bank["codeIdentity"])? == payload_hash(&code_identity)?
            && payload_hash(&bank["appendBackend"])? == payload_hash(&append["binding"])?, "IMMUTABLE_BINDING")?;
        crate::settlement_portfolio_paper_v2::validate_settlement_portfolio_bank(&bank)?;
        need(bank["research"].as_array().is_some_and(|a| !a.is_empty() && a.len() <= 3), "MAX_THREE_RESEARCH_MARKETS")?;
        need(bank["sourcePrefixBytes"].as_i64().is_some_and(|b| b > 0)
            && bank["sourcePrefixSequence"].as_i64().is_some_and(|s| s > 0)
            && hex64(&bank["sourcePrefixHash"]) && hex64(&bank["sourcePrefixJournalHash"]), "SOURCE_BINDING")?;
        if !dir.exists() {
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
        }
        let token = serde_json::to_string(&json!({"pid": std::process::id(), "id": uuid_v4()})).map_err(|e| e.to_string())?;
        let owner_token = uuid_v4();
        let wrote = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600)
            .open(&lock).and_then(|mut f| f.write_all(token.as_bytes()));
        if let Err(e) = wrote { return Err(e.to_string()); }
        let opened = Self::open_inner(runtime_dir, append, now, can_operate, bank, bank_bytes,
            canonical_bank_hash, code_identity, engine_file, dir, bank_file, journal_file, source,
            lock.clone(), token.clone(), owner_token, stop_paths, live_paper_dir, max_batch_rows);
        match opened {
            Ok(h) => Ok(h),
            Err(e) => {
                if lock.exists() && std::fs::read_to_string(&lock).ok().as_deref() == Some(token.as_str()) {
                    let _ = std::fs::remove_file(&lock);
                }
                Err(e)
            }
        }
    }
    fn live_check(closed: bool, failed: bool, can_operate: &Arc<dyn Fn() -> bool + Send + Sync>,
        stop_paths: &[PathBuf], lock: &Path, token: &str, bank_file: &Path, bank_bytes: &[u8],
        live_paper_dir: &Path, engine_file: &Path, code_identity: &V,
        source: &Path, source_id: &str) -> Result<(), String> {
        need(!closed && !failed && can_operate()
            && !stop_paths.iter().any(|p| p.exists())
            && std::fs::read_to_string(lock).map_err(|e| e.to_string())? == token, "OWNERSHIP")?;
        need(sha(&std::fs::read(bank_file).map_err(|e| e.to_string())?) == sha(bank_bytes)
            && payload_hash(&settlement_portfolio_host_code_identity(live_paper_dir, engine_file)?)
                == payload_hash(code_identity), "BANK_OR_CODE_CHANGED")?;
        need(file_identity(&std::fs::metadata(source).map_err(|e| e.to_string())?) == source_id, "SOURCE_REPLACED")?;
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    fn open_inner(runtime_dir: &Path, append: &V,
        now: Arc<dyn Fn() -> i64 + Send + Sync>, can_operate: Arc<dyn Fn() -> bool + Send + Sync>,
        bank: V, bank_bytes: Vec<u8>, canonical_bank_hash: String, code_identity: V,
        engine_file: PathBuf, dir: PathBuf, bank_file: PathBuf, journal_file: PathBuf,
        source: PathBuf, lock: PathBuf, token: String, owner_token: String,
        stop_paths: Vec<PathBuf>, live_paper_dir: &Path, max_batch_rows: i64) -> Result<Self, String> {
        let started = std::time::Instant::now();
        let publication = crate::book_publication_v2::create_book_publication_state(&bank["universe"])?;
        let mut state = crate::settlement_portfolio_paper_v2::create_settlement_portfolio_state(&bank)?;
        let bank_hash = state["bankHash"].as_str().unwrap_or("").to_string();
        need(hex64(&json!(bank_hash)), "ENGINE_BANK_HASH")?;
        let source_id = file_identity(&std::fs::metadata(&source).map_err(|e| e.to_string())?);
        Self::live_check(false, false, &can_operate, &stop_paths, &lock, &token, &bank_file,
            &bank_bytes, live_paper_dir, &engine_file, &code_identity, &source, &source_id)?;
        let evaluation_serial = Arc::new(Mutex::new(0u64));
        // Initial prefix replay from the public source.
        let initial = source_batch(
            &journal_lines(&source, &JournalReadOptions {
                end_byte: Some(bank["sourcePrefixBytes"].as_u64().unwrap_or(0)), ..Default::default() })?,
            publication.clone(), Sha256::new(), 0)?;
        need(initial.offset == bank["sourcePrefixBytes"].as_u64().unwrap_or(0)
            && initial.publication["sequence"] == bank["sourcePrefixSequence"]
            && initial.publication["hash"] == bank["sourcePrefixJournalHash"]
            && format!("{:x}", initial.prefix.clone().finalize()) == bank["sourcePrefixHash"].as_str().unwrap_or(""), "ORIGINAL_PREFIX")?;
        let mut publication = initial.publication;
        let mut prefix = initial.prefix;
        let mut offset = initial.offset;
        need(baal_paper::fixed::instant(publication["lastAt"].as_str().unwrap_or(""))?
            <= baal_paper::fixed::instant(bank["cutoff"].as_str().unwrap_or(""))?, "FUTURE_CUTOFF")?;
        if !journal_file.exists() {
            std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600)
                .open(&journal_file).map_err(|e| e.to_string())?;
        }
        let snapshot = |state: &V, offset: u64, publication: &V, prefix: &Sha256| -> V {
            json!({"paper": state, "sourceBytes": offset, "sourceSequence": publication["sequence"],
                "sourceHash": publication["hash"], "sourcePrefixHash": format!("{:x}", prefix.clone().finalize())})
        };
        let initial_state = snapshot(&state, offset, &publication, &prefix);
        // First read binds the exact journal head and funded invariants.
        let live_args = (can_operate.clone(), stop_paths.clone(), lock.clone(), token.clone(),
            bank_file.clone(), bank_bytes.clone(), live_paper_dir.to_path_buf(),
            engine_file.clone(), code_identity.clone(), source.clone(), source_id.clone());
        let live_args_co = live_args.clone();
        let reconciled = {
            let bank_c = bank.clone();
            let bank_hash = bank_hash.clone();
            let initial_state = initial_state.clone();
            crate::native_journal_append::reconcile_journal_append_source(
                &journal_file,
                &mut |record: &V, _ctx: &V| -> Result<bool, String> {
                    crate::settlement_portfolio_paper_v2::assert_settlement_portfolio_state(
                        &record["state"]["paper"], &bank_c)?;
                    if record["sequence"].as_i64() == Some(1) {
                        return Ok(record["event"]["kind"].as_str() == Some("genesis")
                            && record["event"]["bankHash"] == json!(bank_hash)
                            && payload_hash(&record["state"])? == payload_hash(&initial_state)?);
                    }
                    Ok(record["event"]["kind"].as_str() == Some("source_batch")
                        && record["event"]["rows"].as_array().is_some_and(|r| !r.is_empty() && r.len() <= 1000))
                },
                &|| -> bool {
                    Self::live_check(false, false, &live_args_co.0, &live_args_co.1, &live_args_co.2,
                        &live_args_co.3, &live_args_co.4, &live_args_co.5, &live_args_co.6,
                        &live_args_co.7, &live_args_co.8, &live_args_co.9, &live_args_co.10).is_ok()
                })?
        };
        // Replay the original journal against the actual source tail.
        let actual_lines = journal_lines(&source, &JournalReadOptions {
            start_byte: offset, end_byte: Some(sealed_end(&source)?), ..Default::default() })?;
        let mut actual_iter = actual_lines.into_iter();
        // Replayed (publication, prefix, offset, paper) shared with the validator.
        let replay: Arc<Mutex<(V, Sha256, u64, V)>> =
            Arc::new(Mutex::new((publication.clone(), prefix.clone(), offset, state.clone())));
        let replay_c = replay.clone();
        let mut validate_original = {
            let bank_c = bank.clone();
            let bank_hash = bank_hash.clone();
            let initial_state = initial_state.clone();
            let runtime_dir_c = runtime_dir.to_path_buf();
            let dir_c = dir.clone();
            let serial_c = evaluation_serial.clone();
            move |record: &V, _ctx: &V| -> Result<bool, String> {
                let event = &record["event"];
                if record["sequence"].as_i64() == Some(1) {
                    return Ok(event["kind"].as_str() == Some("genesis")
                        && event["bankHash"] == json!(bank_hash)
                        && event["cutoff"] == bank_c["cutoff"]
                        && payload_hash(&record["state"])? == payload_hash(&initial_state)?);
                }
                need(event["kind"].as_str() == Some("source_batch")
                    && event["rows"].as_array().is_some_and(|r| !r.is_empty() && r.len() <= 1000), "REPLAY_EVENT")?;
                let mut lines = Vec::new();
                for expected in event["rows"].as_array().cloned().unwrap_or_default() {
                    match actual_iter.next() {
                        Some(line) if line == js_stringify(&expected) => lines.push(line),
                        _ => return Err("SETTLEMENT_PORTFOLIO_HOST_ACTUAL_SOURCE_REPLAY".into()),
                    }
                }
                let (pub_t, pre_t, off_t, _p_t) = replay_c.lock()
                    .map_err(|_| "SETTLEMENT_PORTFOLIO_HOST_REPLAY".to_string())?.clone();
                let next_source = source_batch(&lines, pub_t, pre_t, off_t)?;
                let bank_e = bank_c.clone();
                let rt = runtime_dir_c.clone();
                let d = dir_c.clone();
                let ser = serial_c.clone();
                let mut eval = move |input: &V| native_evaluate(&rt, &d, &bank_e, &ser, input);
                let rows = event["rows"].as_array().cloned().unwrap_or_default();
                let paper = reduce(&replay_c.lock()
                        .map_err(|_| "SETTLEMENT_PORTFOLIO_HOST_REPLAY".to_string())?.3.clone(),
                    &rows, &bank_c, event["availableAt"].as_str().unwrap_or(""),
                    &event["accountEligibility"], &mut eval)?;
                *replay_c.lock().map_err(|_| "SETTLEMENT_PORTFOLIO_HOST_REPLAY".to_string())? =
                    (next_source.publication.clone(), next_source.prefix.clone(), next_source.offset, paper.clone());
                Ok(payload_hash(&record["state"])? == payload_hash(&json!({
                    "paper": paper, "sourceBytes": next_source.offset,
                    "sourceSequence": next_source.publication["sequence"],
                    "sourceHash": next_source.publication["hash"],
                    "sourcePrefixHash": format!("{:x}", next_source.prefix.clone().finalize())}))?)
            }
        };
        // Shared head (publication/prefix/offset) consulted by validateTransition.
        let shared_head = Arc::new(Mutex::new((publication.clone(), prefix.clone(), offset)));
        let mut journal = ExclusiveNativeJournal::open(&journal_file,
            Path::new(append["binaryPath"].as_str().unwrap_or("")),
            Path::new(append["sourceRoot"].as_str().unwrap_or("")),
            &append["binding"], &reconciled.head, &owner_token, 1, &stop_paths,
            can_operate.clone(),
            &mut validate_original,
            {
                let bank_c = bank.clone();
                let bank_hash = bank_hash.clone();
                let initial_state = initial_state.clone();
                let shared = shared_head.clone();
                let runtime_dir_c = runtime_dir.to_path_buf();
                let dir_c = dir.clone();
                let serial_c = evaluation_serial.clone();
                Arc::new(move |candidate: &V, event: &V, ctx: &V| -> Result<bool, String> {
                    if ctx["lastBody"].is_null() {
                        return Ok(event["kind"].as_str() == Some("genesis")
                            && event["bankHash"] == json!(bank_hash)
                            && payload_hash(candidate)? == payload_hash(&initial_state)?);
                    }
                    need(event["kind"].as_str() == Some("source_batch"), "TRANSITION_EVENT")?;
                    let lines: Vec<String> = event["rows"].as_array().cloned().unwrap_or_default()
                        .iter().map(|r| js_stringify(r)).collect();
                    let (pub_t, prefix_t, offset_t) = shared.lock()
                        .map_err(|_| "SETTLEMENT_PORTFOLIO_HOST_SHARED".to_string())?.clone();
                    let next_source = source_batch(&lines, pub_t, prefix_t, offset_t)?;
                    let bank_e = bank_c.clone();
                    let rt = runtime_dir_c.clone();
                    let d = dir_c.clone();
                    let ser = serial_c.clone();
                    let mut eval = move |input: &V| native_evaluate(&rt, &d, &bank_e, &ser, input);
                    let rows = event["rows"].as_array().cloned().unwrap_or_default();
                    let paper = reduce(&ctx["lastBody"]["state"]["paper"], &rows, &bank_c,
                        event["availableAt"].as_str().unwrap_or(""), &event["accountEligibility"], &mut eval)?;
                    Ok(payload_hash(candidate)? == payload_hash(&json!({
                        "paper": paper, "sourceBytes": next_source.offset,
                        "sourceSequence": next_source.publication["sequence"],
                        "sourceHash": next_source.publication["hash"],
                        "sourcePrefixHash": format!("{:x}", next_source.prefix.clone().finalize())}))?)
                }) as Arc<dyn Fn(&V, &V, &V) -> Result<bool, String> + Send + Sync>
            },
            30000, 40 * 1024 * 1024)?;
        {
            let done = replay.lock().map_err(|_| "SETTLEMENT_PORTFOLIO_HOST_REPLAY".to_string())?;
            publication = done.0.clone();
            prefix = done.1.clone();
            offset = done.2;
            state = done.3.clone();
        }
        *shared_head.lock().map_err(|_| "SETTLEMENT_PORTFOLIO_HOST_SHARED".to_string())? =
            (publication.clone(), prefix.clone(), offset);
        need(crate::native_journal_restore::source_prefix_hash(&source, offset, None)?
            == format!("{:x}", prefix.clone().finalize()), "SOURCE_CHANGED_DURING_RESTORE")?;
        Self::live_check(false, false, &can_operate, &stop_paths, &lock, &token, &bank_file,
            &bank_bytes, live_paper_dir, &engine_file, &code_identity, &source, &source_id)?;
        if reconciled.head["sequence"].as_i64() == Some(0) {
            journal.commit(&initial_state, &json!({"kind": "genesis", "bankHash": bank_hash,
                "cutoff": bank["cutoff"]}), bank["cutoff"].as_str().unwrap_or(""))?;
        }
        let startup = json!({"coldBootMs": started.elapsed().as_secs_f64() * 1000.0,
            "sourcePrefixBytes": bank["sourcePrefixBytes"], "exclusiveWalletJournal": true});
        let mut host = Self {
            dir, bank, journal: Some(journal), lock, token, source, bank_file, bank_bytes,
            live_paper_dir: live_paper_dir.to_path_buf(), engine_file, runtime_dir: runtime_dir.to_path_buf(),
            source_id, publication, state, prefix, offset, closed: false, failed: false,
            source_bytes_verified: 0, public_prefix_verify_ms: 0.0, startup,
            evaluation_serial, max_batch_rows, now, can_operate, stop_paths, canonical_bank_hash,
            code_identity, shared_head,
        };
        host.publish()?;
        Ok(host)
    }
    fn stopped(&self) -> bool { self.stop_paths.iter().any(|p| p.exists()) }
    fn live(&self) -> Result<(), String> {
        Self::live_check(self.closed, self.failed, &self.can_operate, &self.stop_paths, &self.lock,
            &self.token, &self.bank_file, &self.bank_bytes, &self.live_paper_dir, &self.engine_file,
            &self.current_code_identity(), &self.source, &self.source_id)
    }
    fn current_code_identity(&self) -> V { self.code_identity.clone() }
    /// status()
    pub fn status(&self) -> Result<V, String> {
        let mut out = crate::settlement_portfolio_paper_v2::settlement_portfolio_report(&self.state,
            &self.bank, &baal_service_runtime::contract::ms_to_iso((self.now)() as f64))?;
        out["bankHash"] = self.state["bankHash"].clone();
        out["canonicalBankHash"] = json!(self.canonical_bank_hash);
        out["sourceBytes"] = json!(self.offset);
        out["sourceSequence"] = self.publication["sequence"].clone();
        out["sourceHash"] = self.publication["hash"].clone();
        out["sourcePrefixHash"] = json!(format!("{:x}", self.prefix.clone().finalize()));
        out["sourceClosed"] = self.publication["closed"].clone();
        let (jseq, jhash, jfail, junk) = match &self.journal {
            Some(j) => (j.head()["sequence"].clone(), j.head()["tailHash"].clone(),
                j.status()["failed"].clone(), j.status()["unknownAcceptance"].clone()),
            None => (V::Null, V::Null, V::Null, V::Null),
        };
        out["journalSequence"] = jseq;
        out["journalHash"] = jhash;
        out["appendTransportFailure"] = jfail;
        out["unknownAppendAcceptance"] = junk;
        out["failed"] = json!(self.failed);
        out["workerPid"] = json!(std::process::id());
        out["startup"] = self.startup.clone();
        out["publicSourceVerification"] = json!({"mode": "full_consumed_prefix_each_consume",
            "sourceBytesVerified": self.source_bytes_verified,
            "publicPrefixVerifyMs": self.public_prefix_verify_ms,
            "producerExclusivityEstablished": false});
        out["executionAuthority"] = json!(false);
        out["networkRequests"] = json!(0);
        out["competitionOrders"] = json!(0);
        Ok(out)
    }
    /// publish()
    pub fn publish(&self) -> Result<(), String> {
        atomic_json_canonical(&self.dir.join("stats.json"), &self.status()?)
    }
    /// focus()
    pub fn focus(&self) -> Result<V, String> {
        let mut markets: indexmap::IndexMap<String, V> = Default::default();
        for (_k, w) in self.state["wallets"].as_object().cloned().unwrap_or_default() {
            for id in w["positions"].as_object().map(|p| p.keys().cloned().collect::<Vec<_>>()).unwrap_or_default() {
                markets.insert(id.clone(), json!({"marketId": id, "phase": "held"}));
            }
            for (id, p) in w["pending"].as_object().cloned().unwrap_or_default() {
                if !markets.contains_key(&id) {
                    markets.insert(id.clone(), json!({"marketId": id,
                        "phase": if p["action"].as_str() == Some("sell") { "pending_sell" } else { "pending_buy" }}));
                }
            }
        }
        for r in self.bank["research"].as_array().cloned().unwrap_or_default() {
            let flags_ok = match &r["flags"] {
                V::Null => return Err("SETTLEMENT_PORTFOLIO_HOST_FLAGS".into()),
                V::Array(a) => a.is_empty(),
                V::String(s) => s.is_empty(),
                _ => true,
            };
            let expires = baal_paper::fixed::instant(r["expiresAt"].as_str().unwrap_or(""))?;
            let source_at = baal_paper::fixed::instant(r["sourceAt"].as_str().unwrap_or(""))?;
            if baal_service_runtime::json_text::js_truthy(r.get("settlementReviewed")) && flags_ok
                && (self.now)() < expires
                && (self.now)() - source_at <= self.bank["policy"]["maximumResearchAgeMs"].as_i64().unwrap_or(0)
                && !markets.contains_key(r["marketId"].as_str().unwrap_or(""))
            {
                markets.insert(r["marketId"].as_str().unwrap_or("").to_string(),
                    json!({"marketId": r["marketId"], "phase": "depth_required"}));
            }
        }
        Ok(json!({"bankHash": self.state["bankHash"], "markets": markets.values().cloned().collect::<Vec<_>>(),
            "ownedRiskReads": true, "executionAuthority": false}))
    }
    /// consume({availableAt, accountEligibility})
    pub fn consume(&mut self, available_at: Option<&str>, account_eligibility: &V) -> Result<usize, String> {
        let at = available_at.map(String::from)
            .unwrap_or_else(|| baal_service_runtime::contract::ms_to_iso((self.now)() as f64));
        let r = (|| -> Result<usize, String> {
            self.live()?;
            let began = std::time::Instant::now();
            let expected = format!("{:x}", self.prefix.clone().finalize());
            need(crate::native_journal_restore::source_prefix_hash(&self.source, self.offset, None)? == expected,
                "HISTORICAL_PUBLIC_PREFIX_CHANGED")?;
            self.public_prefix_verify_ms += began.elapsed().as_secs_f64() * 1000.0;
            self.source_bytes_verified += self.offset;
            self.live()?;
            let end = sealed_end(&self.source)?;
            if end == self.offset { return Ok(0); }
            let mut lines = Vec::new();
            for line in journal_lines(&self.source, &JournalReadOptions {
                start_byte: self.offset, end_byte: Some(end), ..Default::default() })? {
                lines.push(line);
                if lines.len() >= self.max_batch_rows as usize { break; }
            }
            let next_source = source_batch(&lines, self.publication.clone(), self.prefix.clone(), self.offset)?;
            baal_paper::fixed::instant(&at)?;
            let mut eval = {
                let rt = self.runtime_dir.clone();
                let d = self.dir.clone();
                let bank_e = self.bank.clone();
                let ser = self.evaluation_serial.clone();
                move |input: &V| native_evaluate(&rt, &d, &bank_e, &ser, input)
            };
            let next_state = reduce(&self.state.clone(), &next_source.rows, &self.bank.clone(),
                &at, account_eligibility, &mut eval)?;
            let event = json!({"kind": "source_batch", "rows": next_source.rows,
                "availableAt": at, "accountEligibility": account_eligibility});
            let candidate = json!({"paper": next_state, "sourceBytes": next_source.offset,
                "sourceSequence": next_source.publication["sequence"],
                "sourceHash": next_source.publication["hash"],
                "sourcePrefixHash": format!("{:x}", next_source.prefix.clone().finalize())});
            if let Some(j) = &mut self.journal {
                j.commit(&candidate, &event, &at)?;
            }
            self.state = next_state;
            self.publication = next_source.publication;
            self.prefix = next_source.prefix;
            self.offset = next_source.offset;
            *self.shared_head.lock().map_err(|_| "SETTLEMENT_PORTFOLIO_HOST_SHARED".to_string())? =
                (self.publication.clone(), self.prefix.clone(), self.offset);
            self.publish()?;
            Ok(next_source.rows.len())
        })();
        if r.is_err() { self.failed = true; }
        r
    }
    /// close()
    pub fn close(&mut self) -> Result<(), String> {
        if self.closed { return Ok(()); }
        self.closed = true;
        if let Some(mut j) = self.journal.take() { let _ = j.close(); }
        need(std::fs::read_to_string(&self.lock).map_err(|e| e.to_string())? == self.token, "LOCK_CHANGED")?;
        std::fs::remove_file(&self.lock).map_err(|e| e.to_string())?;
        Ok(())
    }
}
impl PaperHost for SettlementPortfolioHostV2 {
    fn dir(&self) -> &Path { &self.dir }
    fn bank(&self) -> &V { &self.bank }
    fn consume(&mut self) -> Result<usize, String> { SettlementPortfolioHostV2::consume(self, None, &V::Null) }
    fn status(&mut self) -> Result<V, String> { SettlementPortfolioHostV2::status(self) }
    fn publish(&mut self) -> Result<(), String> { SettlementPortfolioHostV2::publish(self) }
    fn focus(&mut self) -> Result<V, String> { SettlementPortfolioHostV2::focus(self) }
    fn close(&mut self) -> Result<(), String> { SettlementPortfolioHostV2::close(self) }
}
