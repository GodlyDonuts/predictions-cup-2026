// live-paper/executable-edge-watch.mjs — executable-edge cohort host: locked
// single-writer over public-books.jsonl with a per-cohort checkpoint journal.
use serde_json::{json, Value as V};
use std::fs::OpenOptions;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(code.to_string()); }
    Ok(())
}
fn sha(b: &[u8]) -> String { baal_service_runtime::json_text::sha256(b) }
fn complete(file: &Path) -> Result<Vec<u8>, String> {
    let b = std::fs::read(file).map_err(|e| e.to_string())?;
    let end = b.iter().rposition(|x| *x == b'\n').map(|p| p + 1).unwrap_or(0);
    Ok(b[..end].to_vec())
}
fn rows(b: &[u8]) -> Result<Vec<V>, String> {
    need(b.is_empty() || *b.last().unwrap() == b'\n', "EDGE_PARTIAL_JOURNAL")?;
    if b.is_empty() { return Ok(vec![]); }
    let text = String::from_utf8(b.to_vec()).map_err(|e| e.to_string())?;
    let mut parts: Vec<&str> = text.split('\n').collect();
    parts.pop(); // JS .split('\n').slice(0,-1)
    parts.iter().map(|x| serde_json::from_str(x).map_err(|e| e.to_string())).collect()
}
fn durable(file: &Path, b: &[u8], append: bool, create_new: bool) -> Result<(), String> {
    let mut o = OpenOptions::new();
    if create_new { o.write(true).create_new(true); } else if append { o.append(true); } else { o.write(true).truncate(true); }
    #[cfg(unix)]
    o.mode(0o600);
    let mut fd = o.open(file).map_err(|e| e.to_string())?;
    fd.write_all(b).map_err(|_| "EDGE_WRITE_FAILED".to_string())?;
    fd.sync_all().map_err(|e| e.to_string())
}
fn read_json(file: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(file).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
struct SourceStat { ino: u64, birthtime_ms: f64, size: u64 }
fn stat_of(file: &Path) -> Result<SourceStat, String> {
    let m = std::fs::metadata(file).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    return Ok(SourceStat { ino: m.ino(), birthtime_ms: m.created().map_err(|e| e.to_string())?
        .duration_since(std::time::UNIX_EPOCH).map_err(|e| e.to_string())?.as_secs_f64() * 1000.0,
        size: m.len() });
    #[cfg(not(unix))]
    return Ok(SourceStat { ino: 0, birthtime_ms: 0.0, size: m.len() });
}

/// executableEdgeIdentity() — sha256 per frozen-code file, workspace-relative keys.
pub fn executable_edge_identity(root: &Path) -> Result<V, String> {
    let files = ["live-paper/executable-edge.mjs", "live-paper/executable-edge-watch.mjs",
        "live-paper/model.mjs", "live-paper/book-publication.mjs", "paper/fixed.mjs",
        "feed/contract.mjs", "execution/ledger.mjs"];
    let mut m = serde_json::Map::new();
    for p in files {
        m.insert(p.to_string(), json!(sha(&std::fs::read(root.join(p)).map_err(|e| e.to_string())?)));
    }
    Ok(V::Object(m))
}

/// openExecutableEdge host — JS returns a closure object; Rust is a struct.
pub struct ExecutableEdgeHost {
    pub dir: PathBuf,
    pub bank: V,
    state: V,
    cohort: String,
    runtime_dir: PathBuf,
    workspace_dir: PathBuf,
    identity: V,
    publication: V,
    sequence: i64,
    previous: String,
    journal_bytes: u64,
    offset: usize,
    bank_bytes_hash: String,
    prefix_hash: String,
    source_stat: SourceStat,
    lock_file: PathBuf,
    token: String,
    closed: bool,
    failed: bool,
    now: Box<dyn Fn() -> f64>,
}
impl ExecutableEdgeHost {
    fn source_file(&self) -> PathBuf { self.runtime_dir.join("public-books.jsonl") }
    fn bank_file(&self) -> PathBuf { self.dir.join("bank.json") }
    fn journal_file(&self) -> PathBuf { self.dir.join("journal.jsonl") }
    pub fn stopped(&self) -> bool {
        [self.workspace_dir.join("STOP"), self.runtime_dir.join("STOP"),
            self.runtime_dir.join("EXECUTABLE_EDGE_STOP"), self.dir.join("STOP")]
            .iter().any(|p| p.exists())
    }
    fn live(&self) -> Result<(), String> {
        need(!self.closed && !self.failed && !self.stopped()
            && std::fs::read_to_string(&self.lock_file).map(|s| s == self.token).unwrap_or(false),
            "EDGE_NOT_OWNER_OR_STOPPED")
    }
    fn expected(&self) -> V {
        json!({"sourceBytes": self.offset, "sourceSequence": self.publication["sequence"],
            "sourceHash": self.publication["hash"], "stateHash": crate::model::digest(&self.state)})
    }
    fn reduce(&mut self, batch: &[V], at: &str, decisions: bool) -> Result<(), String> {
        for r in batch {
            self.publication = crate::book_publication_v2::apply_book_publication(&self.publication, r)?;
            self.state = crate::executable_edge::advance_executable_edge(&self.state, r, &self.bank,
                at, decisions && self.publication["closed"] != V::Bool(true))?;
        }
        Ok(())
    }
    fn append(&mut self, body: V) -> Result<(), String> {
        self.live()?;
        need(std::fs::metadata(self.journal_file()).map_err(|e| e.to_string())?.len() == self.journal_bytes,
            "EDGE_JOURNAL_CHANGED")?;
        let mut item = json!({"sequence": self.sequence + 1, "previous": self.previous,
            "bankHash": crate::model::digest(&self.bank)});
        for (k, v) in body.as_object().unwrap() { item[k] = v.clone(); }
        let hash = crate::model::digest(&item);
        item["hash"] = json!(hash);
        let b = format!("{}\n", baal_service_runtime::json_text::js_stringify(&item));
        durable(&self.journal_file(), b.as_bytes(), true, false)?;
        self.sequence += 1;
        self.previous = hash.as_str().to_string();
        self.journal_bytes += b.len() as u64;
        Ok(())
    }
    /// consume() — number of source records consumed this pass.
    pub fn consume(&mut self) -> Result<usize, String> {
        self.live()?;
        let r = (|h: &mut Self| -> Result<usize, String> {
            need(crate::model::digest(&executable_edge_identity(&h.workspace_dir)?)
                == crate::model::digest(&h.identity)
                && sha(&std::fs::read(h.bank_file()).map_err(|e| e.to_string())?) == h.bank_bytes_hash,
                "EDGE_FROZEN_CODE_OR_BANK_CHANGED")?;
            let st = stat_of(&h.source_file())?;
            need(st.ino == h.source_stat.ino && st.birthtime_ms == h.source_stat.birthtime_ms
                && st.size >= h.offset as u64, "EDGE_SOURCE_REPLACED")?;
            let bytes = complete(&h.source_file())?;
            need(sha(&bytes[..h.offset]) == h.prefix_hash, "EDGE_SOURCE_REWRITTEN")?;
            if bytes.len() == h.offset { return Ok(0); }
            let batch = rows(&bytes[h.offset..])?;
            let at = baal_service_runtime::contract::ms_to_iso((h.now)());
            h.reduce(&batch, &at, true)?;
            h.offset = bytes.len();
            let mut body = json!({"kind": "batch", "availableAt": at, "allowDecisions": true});
            for (k, v) in h.expected().as_object().unwrap() { body[k] = v.clone(); }
            h.append(body)?;
            h.prefix_hash = sha(&bytes);
            Ok(batch.len())
        })(self);
        if r.is_err() { self.failed = true; }
        r
    }
    /// status()
    pub fn status(&self) -> Result<V, String> {
        let mut out = crate::executable_edge::executable_edge_report(&self.state, &self.bank,
            &baal_service_runtime::contract::ms_to_iso((self.now)()))?;
        out["cohort"] = json!(self.cohort);
        for (k, v) in self.expected().as_object().unwrap() { out[k] = v.clone(); }
        out["journalSequence"] = json!(self.sequence);
        out["journalHash"] = json!(self.previous);
        out["sourceClosed"] = self.publication["closed"].clone();
        out["failed"] = json!(self.failed);
        Ok(out)
    }
    /// state() — structuredClone snapshot.
    pub fn state(&self) -> V { self.state.clone() }
    pub fn close(&mut self) -> Result<(), String> {
        if self.closed { return Ok(()); }
        self.closed = true;
        need(std::fs::read_to_string(&self.lock_file).map(|s| s == self.token).unwrap_or(false),
            "EDGE_LOCK_CHANGED")?;
        std::fs::remove_file(&self.lock_file).map_err(|e| e.to_string())
    }
}
/// openExecutableEdge({runtimeDir, cohort, initialize, now, workspaceDir})
pub fn open_executable_edge(runtime_dir: &Path, cohort: &str, initialize: bool,
    now: impl Fn() -> f64 + 'static, workspace_dir: &Path) -> Result<ExecutableEdgeHost, String> {
    let cohort_ok = !cohort.is_empty() && cohort.len() >= 2 && cohort.len() <= 64
        && cohort.as_bytes()[0].is_ascii_lowercase()
        && cohort.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    need(runtime_dir.is_absolute() && cohort_ok, "EDGE_DIRECTORY")?;
    let dir = runtime_dir.join("executable-edge-cohorts").join(cohort);
    let source_file = runtime_dir.join("public-books.jsonl");
    let bank_file = dir.join("bank.json");
    let journal_file = dir.join("journal.jsonl");
    let lock_file = dir.join("worker.lock");
    let stopped = |workspace_dir: &Path| -> bool {
        [workspace_dir.join("STOP"), runtime_dir.join("STOP"),
            runtime_dir.join("EXECUTABLE_EDGE_STOP"), dir.join("STOP")].iter().any(|p| p.exists())
    };
    need(!stopped(workspace_dir), "EDGE_STOPPED")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let token = format!("{}", json!({"pid": std::process::id(), "id": uuid_like()}));
    let r = (|| -> Result<ExecutableEdgeHost, String> {
        durable(&lock_file, token.as_bytes(), false, true)?;
        let identity = executable_edge_identity(workspace_dir)?;
        let source_stat = stat_of(&source_file)?;
        let initial_bytes = complete(&source_file)?;
        let records = rows(&initial_bytes)?;
        need(!records.is_empty(), "EDGE_SOURCE_EMPTY")?;
        let bank: V;
        if initialize {
            need(!bank_file.exists() && !journal_file.exists(), "EDGE_COHORT_EXISTS")?;
            let config = read_json(&workspace_dir.join("live-paper/config.json"))?;
            let universe = read_json(&workspace_dir.join("live-paper/price-universe.json"))?;
            let last = records.last().unwrap();
            let cutoff = baal_service_runtime::contract::ms_to_iso(now());
            need(baal_service_runtime::contract::parse_iso_ms(last["at"].as_str().unwrap_or(""))
                .is_some_and(|x| x <= baal_service_runtime::contract::parse_iso_ms(&cutoff).unwrap_or(f64::NAN)),
                "EDGE_FUTURE_SOURCE")?;
            bank = crate::executable_edge::create_executable_edge_bank(&config, &universe, &cutoff,
                initial_bytes.len() as i64, &sha(&initial_bytes),
                last["sequence"].as_i64().unwrap_or(0), last["hash"].as_str().unwrap_or(""),
                &identity)?;
        } else {
            bank = read_json(&bank_file)?;
            need(crate::model::digest(&identity) == crate::model::digest(&bank["codeIdentity"]),
                "EDGE_FROZEN_CODE_CHANGED")?;
        }
        let mut publication = crate::book_publication_v2::create_book_publication_state(&bank["universe"])?;
        let mut state = crate::executable_edge::empty_executable_edge(&bank)?;
        let mut offset = 0usize;
        let mut index = 0usize;
        let mut closed = false;
        let mut failed = false;
        for r in &records {
            if publication["sequence"].as_i64().unwrap_or(0) >= bank["sourcePrefixSequence"].as_i64().unwrap_or(0) { break; }
            need(!closed && !failed && !stopped(workspace_dir)
                && std::fs::read_to_string(&lock_file).map(|s| s == token).unwrap_or(false),
                "EDGE_NOT_OWNER_OR_STOPPED")?;
            publication = crate::book_publication_v2::apply_book_publication(&publication, r)?;
            // JS: advanceExecutableEdge(state, r, bank, {availableAt: r.at}) — allowDecisions defaults false.
            state = crate::executable_edge::advance_executable_edge(&state, r, &bank,
                r["at"].as_str().unwrap_or(""), false)?;
            index += 1;
        }
        // Physical source bytes are authoritative; JSON whitespace cannot change prefix identity.
        let mut physical = 0usize;
        for _ in 0..index {
            physical = initial_bytes[physical..].iter().position(|x| *x == b'\n').unwrap_or(0) + physical + 1;
        }
        offset = physical;
        need(offset == bank["sourcePrefixBytes"].as_i64().unwrap_or(-1) as usize
            && sha(&initial_bytes[..offset]) == bank["sourcePrefixHash"].as_str().unwrap_or("")
            && publication["hash"].as_str() == Some(bank["sourcePrefixJournalHash"].as_str().unwrap_or("")),
            "EDGE_PREFIX_CHANGED")?;
        if initialize { need(publication["closed"] != V::Bool(true), "EDGE_SOURCE_CLOSED")?; }
        state["development"] = json!({"sourceSequence": publication["sequence"],
            "counts": state["counts"].clone(), "modelHash": crate::model::digest(&state["cells"])});
        let mut sequence = 0i64;
        let mut previous = crate::model::digest(&bank);
        let mut journal_bytes = 0u64;
        if !initialize {
            let raw = std::fs::read(&journal_file).map_err(|e| e.to_string())?;
            for r in rows(&raw)? {
                let mut body = r.clone();
                let hash = body.as_object_mut().and_then(|m| m.shift_remove("hash")).unwrap_or(V::Null);
                sequence += 1;
                need(body["sequence"].as_i64() == Some(sequence)
                    && body["previous"].as_str() == Some(previous.as_str())
                    && hash.as_str() == Some(crate::model::digest(&body).as_str())
                    && body["bankHash"].as_str() == Some(crate::model::digest(&bank).as_str()),
                    "EDGE_JOURNAL_CHAIN")?;
                if body["kind"].as_str() == Some("genesis") {
                    need(sequence == 1 && body["stateHash"].as_str() == Some(crate::model::digest(&state).as_str()),
                        "EDGE_GENESIS")?;
                } else {
                    need(body["kind"].as_str() == Some("batch")
                        && body["sourceSequence"].as_i64().unwrap_or(0)
                            > publication["sequence"].as_i64().unwrap_or(0), "EDGE_CHECKPOINT")?;
                    let mut batch = Vec::new();
                    while publication["sequence"].as_i64().unwrap_or(0) + (batch.len() as i64)
                        < body["sourceSequence"].as_i64().unwrap_or(0) {
                        need(index < records.len(), "EDGE_SOURCE_TRUNCATED")?;
                        batch.push(records[index].clone());
                        index += 1;
                    }
                    for rec in &batch {
                        publication = crate::book_publication_v2::apply_book_publication(&publication, rec)?;
                        state = crate::executable_edge::advance_executable_edge(&state, rec, &bank,
                            body["availableAt"].as_str().unwrap_or(""),
                            body["allowDecisions"].as_bool() == Some(true)
                                && publication["closed"] != V::Bool(true))?;
                    }
                    physical = offset;
                    for _ in 0..batch.len() {
                        physical = initial_bytes[physical..].iter().position(|x| *x == b'\n').unwrap_or(0) + physical + 1;
                    }
                    offset = physical;
                    let expected = json!({"sourceBytes": offset, "sourceSequence": publication["sequence"],
                        "sourceHash": publication["hash"], "stateHash": crate::model::digest(&state)});
                    need(expected.as_object().unwrap().iter()
                        .all(|(k, v)| &body[k] == v), "EDGE_REPLAY_MISMATCH")?;
                }
                previous = hash.as_str().unwrap_or("").to_string();
            }
            need(sequence > 0, "EDGE_GENESIS_MISSING")?;
            journal_bytes = raw.len() as u64;
        }
        let mut host = ExecutableEdgeHost {
            dir: dir.clone(), bank: bank.clone(), state, cohort: cohort.to_string(),
            runtime_dir: runtime_dir.to_path_buf(), workspace_dir: workspace_dir.to_path_buf(),
            identity, publication, sequence, previous, journal_bytes, offset,
            bank_bytes_hash: String::new(), prefix_hash: String::new(), source_stat,
            lock_file: lock_file.clone(), token: token.clone(), closed, failed,
            now: Box::new(now),
        };
        if initialize {
            host.live()?;
            durable(&bank_file, format!("{}\n", serde_json::to_string_pretty(&bank).map_err(|e| e.to_string())?).as_bytes(), false, true)?;
            durable(&journal_file, &[], false, true)?;
            host.append(json!({"kind": "genesis", "stateHash": crate::model::digest(&host.state)}))?;
        }
        host.bank_bytes_hash = sha(&std::fs::read(&bank_file).map_err(|e| e.to_string())?);
        host.prefix_hash = sha(&initial_bytes[..host.offset]);
        Ok(host)
    })();
    if r.is_err() {
        if std::fs::read_to_string(&lock_file).map(|s| s == token).unwrap_or(false) {
            let _ = std::fs::remove_file(&lock_file);
        }
    }
    r
}
fn uuid_like() -> String {
    // randomUUID() — content-free lock identity; any unique token works.
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let r = t.as_nanos() ^ ((std::process::id() as u128) << 64);
    format!("{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        (r & 0xffffffff) as u32, (r >> 32 & 0xffff) as u16, (r >> 48 & 0xffff) as u16,
        (r >> 64 & 0xffff) as u16, (r >> 80 & 0xffffffffffff) as u64)
}
/// runExecutableEdge(options) — loop consuming + publishing stats until stopped.
pub fn run_executable_edge(runtime_dir: &Path, cohort: &str, initialize: bool, once: bool,
    now: impl Fn() -> f64 + 'static, workspace_dir: &Path) -> Result<V, String> {
    let mut w = open_executable_edge(runtime_dir, cohort, initialize, now, workspace_dir)?;
    let result = (|w: &mut ExecutableEdgeHost| -> Result<(), String> {
        loop {
            w.consume()?;
            let stats = w.status()?;
            crate::paper_runtime::atomic_json(&w.dir.join("stats.json"), &stats)?;
            if once { break; }
            std::thread::sleep(std::time::Duration::from_millis(1000));
        }
        Ok(())
    })(&mut w);
    let close = w.close();
    result.and(close)?;
    w.status()
}
