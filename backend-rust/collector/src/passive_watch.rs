// live-paper/passive-watch.mjs — exclusive worker over the realtime capture
// journal; consumes rows into passive-capture state and publishes stats.
use serde_json::{json, Value as V};
use std::io::Write;
use std::path::{Path, PathBuf};

fn hash(value: &V) -> String {
    match value {
        V::String(s) => baal_service_runtime::json_text::sha256(s.as_bytes()),
        other => baal_service_runtime::json_text::sha256(
            baal_service_runtime::json_text::js_stringify(other).as_bytes()),
    }
}
fn hash_bytes(b: &[u8]) -> String { baal_service_runtime::json_text::sha256(b) }
/// passiveCodeIdentity(root) — sha256 of each frozen source file.
pub fn passive_code_identity(root: &Path) -> Result<V, String> {
    let files = ["passive-watch.mjs", "passive-capture.mjs", "passive-observer.mjs",
        "../paper/fixed.mjs", "../feed/contract.mjs"];
    let dirs = ["live-paper", "live-paper", "live-paper", "paper", "feed"];
    let mut m = serde_json::Map::new();
    for (k, d) in files.iter().zip(dirs.iter()) {
        m.insert(k.to_string(), json!(hash_bytes(
            &std::fs::read(root.join(d).join(k)).map_err(|e| e.to_string())?)));
    }
    Ok(V::Object(m))
}
fn source_file(runtime: &Path, name: &V) -> Result<PathBuf, String> {
    let name = name.as_str().unwrap_or("");
    if name.is_empty()
        || !name.strip_prefix("realtime-").is_some_and(|rest| rest.ends_with(".jsonl"))
        || name.contains('/') || name.contains('\\') || name.contains("..")
        || !name.bytes().all(|b| b.is_ascii_alphanumeric()
            || [b'.', b'_', b'-'].contains(&b)) {
        return Err("Invalid market capture filename".into());
    }
    Ok(runtime.join(name))
}
fn complete_lines(bytes: &[u8]) -> Vec<String> {
    let end = bytes.iter().rposition(|x| *x == b'\n').map(|p| p + 1).unwrap_or(0);
    if end == 0 { return vec![]; }
    String::from_utf8_lossy(&bytes[..end])
        .split('\n').filter(|s| !s.is_empty()).map(String::from).collect()
}
fn append(file: &Path, row: &V) -> Result<(), String> {
    let mut fd = std::fs::OpenOptions::new().append(true).open(file).map_err(|e| e.to_string())?;
    let bytes = format!("{}\n", baal_service_runtime::json_text::js_stringify(row));
    let mut written = 0;
    while written < bytes.len() {
        let n = fd.write(&bytes.as_bytes()[written..]).map_err(|e| e.to_string())?;
        if n == 0 { return Err("Passive journal short write".into()); }
        written += n;
    }
    fd.sync_all().map_err(|e| e.to_string())
}
/// openPassiveWatch(runtime, {initialize, now, codeIdentity})
pub struct PassiveWatch {
    runtime: PathBuf,
    bank: V,
    state: V,
    journal_file: PathBuf,
    capture_path: PathBuf,
    offset: usize,
    sequence: i64,
    previous: String,
    now: Box<dyn Fn() -> f64>,
}
impl PassiveWatch {
    /// consume() — rows consumed this pass (8 MiB budget per pass).
    pub fn consume(&mut self) -> Result<usize, String> {
        let stat = std::fs::metadata(&self.capture_path).map_err(|e| e.to_string())?;
        if (stat.len() as usize) < self.offset { return Err("Source capture truncated".into()); }
        let size = (stat.len() as usize - self.offset).min(8 * 1024 * 1024);
        if size == 0 { return Ok(0); }
        use std::io::{Read, Seek, SeekFrom};
        let mut fd = std::fs::File::open(&self.capture_path).map_err(|e| e.to_string())?;
        let mut bytes = vec![0u8; size];
        fd.seek(SeekFrom::Start(self.offset as u64)).map_err(|e| e.to_string())?;
        let mut count = 0usize;
        while count < size {
            let n = fd.read(&mut bytes[count..]).map_err(|e| e.to_string())?;
            if n == 0 { break; }
            count += n;
        }
        let lines = complete_lines(&bytes[..count]);
        if lines.is_empty() && count == 8 * 1024 * 1024 {
            return Err("Oversized passive source row".into());
        }
        let mut consumed = 0usize;
        for line in &lines {
            let raw = format!("{}\n", line);
            let output = crate::passive_capture::consume_passive_capture(&self.state,
                &serde_json::from_str(line).map_err(|e| e.to_string())?)?;
            let mut body = json!({"sequence": self.sequence + 1, "previous": self.previous,
                "sourceLineHash": hash_bytes(raw.as_bytes()),
                "recordsHash": hash(&output["records"]), "eventsHash": hash(&output["events"]),
                "stateHash": hash(&output["state"]),
                "captureIndex": output["state"]["captureIndex"],
                "records": output["records"], "events": output["events"]});
            let record_hash = hash(&body);
            body["hash"] = json!(record_hash);
            append(&self.journal_file, &body)?;
            self.state = output["state"].clone();
            self.offset += raw.len();
            self.sequence += 1;
            self.previous = record_hash;
            consumed += 1;
        }
        Ok(consumed)
    }
    /// status()
    pub fn status(&self) -> V {
        let now = (self.now)();
        let mut report = crate::passive_capture::passive_capture_report(&self.state)
            .unwrap_or_else(|_| json!({"markets": []}));
        let reported_at = baal_service_runtime::contract::ms_to_iso(now);
        let markets: Vec<V> = report["markets"].as_array().cloned().unwrap_or_default().into_iter()
            .map(|mut m| {
                let updated = baal_service_runtime::contract::parse_iso_ms(
                    m["updatedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN);
                m["ready"] = json!(m["ready"] == V::Bool(true) && !m["updatedAt"].is_null()
                    && now - updated >= 0.0 && now - updated < 20000.0);
                m
            }).collect();
        report["markets"] = json!(markets);
        report["sourceFile"] = self.bank["sourceFile"].clone();
        report["sourceBytes"] = json!(self.offset);
        report["journalSequence"] = json!(self.sequence);
        report["journalHash"] = json!(self.previous);
        report["sourceAgeMs"] = json!((now - std::fs::metadata(&self.capture_path)
            .and_then(|m| m.modified()).map(|t| t.duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default().as_secs_f64() * 1000.0).unwrap_or(now)).max(0.0));
        report["codeIdentity"] = self.bank["codeIdentity"].clone();
        report["reportedAt"] = json!(reported_at);
        report
    }
    /// sessionCurrent()
    pub fn session_current(&self) -> bool {
        read_json(&self.runtime.join("realtime-session.json"))
            .map(|s| s["file"] == self.bank["sourceFile"]).unwrap_or(false)
    }
}
fn read_json(file: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(file).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
/// openPassiveWatch(runtime, {initialize, now, codeIdentity})
pub fn open_passive_watch(runtime: &Path, initialize: bool,
    now: impl Fn() -> f64 + 'static, code_identity: &V) -> Result<PassiveWatch, String> {
    let bank_file = runtime.join("passive-bank.json");
    let journal_file = runtime.join("passive-journal.jsonl");
    let bank: V;
    if initialize {
        if bank_file.exists() || journal_file.exists() {
            return Err("Refusing to replace an existing passive cohort".into());
        }
        let session = read_json(&runtime.join("realtime-session.json"))?;
        let capture_path = source_file(runtime, &session["file"])?;
        let source = std::fs::read(&capture_path).map_err(|e| e.to_string())?;
        let lines = complete_lines(&source);
        let prefix = if lines.is_empty() { Vec::new() } else {
            format!("{}\n", lines.join("\n")).into_bytes()
        };
        let cutoff = baal_service_runtime::contract::ms_to_iso(now());
        let source_prefix_hash = hash_bytes(&prefix);
        let session_id = hash(&session["file"])[..32].to_string();
        bank = json!({"schemaVersion":1,"mode":"measurement_only","cutoff":cutoff,
            "sessionId":session_id,"sourceFile":session["file"],"scope":session["scope"],
            "sourcePrefixBytes":prefix.len() as i64,"sourcePrefixHash":source_prefix_hash,
            "codeIdentity":code_identity,
            "sourceHash":hash(&json!({"sourcePrefixHash":source_prefix_hash,"cutoff":cutoff,
                "sessionId":session_id,"scope":session["scope"],"codeIdentity":code_identity}))});
        use std::io::Write;
        let mut fd = std::fs::OpenOptions::new().write(true).create_new(true)
            .open(&bank_file).map_err(|e| e.to_string())?;
        fd.write_all(serde_json::to_string_pretty(&bank).map_err(|e| e.to_string())?.as_bytes())
            .map_err(|e| e.to_string())?;
        drop(fd);
        std::fs::OpenOptions::new().write(true).create_new(true).open(&journal_file)
            .map_err(|e| e.to_string())?;
    } else {
        bank = read_json(&bank_file)?;
    }
    if bank["schemaVersion"].as_i64() != Some(1) || bank["mode"].as_str() != Some("measurement_only")
        || hash(&bank["codeIdentity"]) != hash(code_identity)
        || bank["sourceHash"].as_str() != Some(hash(&json!({
            "sourcePrefixHash": bank["sourcePrefixHash"], "cutoff": bank["cutoff"],
            "sessionId": bank["sessionId"], "scope": bank["scope"],
            "codeIdentity": bank["codeIdentity"]})).as_str()) {
        return Err("Passive source identity changed".into());
    }
    let capture_path = source_file(runtime, &bank["sourceFile"])?;
    let initial_source = std::fs::read(&capture_path).map_err(|e| e.to_string())?;
    if initial_source.len() < bank["sourcePrefixBytes"].as_i64().unwrap_or(0) as usize
        || hash_bytes(&initial_source[..bank["sourcePrefixBytes"].as_i64().unwrap_or(0) as usize])
            != bank["sourcePrefixHash"].as_str().unwrap_or("") {
        return Err("Passive source prefix changed".into());
    }
    let mut capture_args = bank.clone();
    capture_args["sourceHash"] = bank["sourceHash"].clone();
    let mut state = crate::passive_capture::create_passive_capture(&capture_args)?;
    let mut offset = 0usize;
    let mut sequence = 0i64;
    let mut previous = hash(&bank);
    let journal = std::fs::read_to_string(&journal_file).map_err(|e| e.to_string())?;
    if !journal.is_empty() && !journal.ends_with('\n') {
        return Err("Incomplete passive journal".into());
    }
    for line in journal.split('\n').filter(|s| !s.is_empty()) {
        let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
        let mut body = row.clone();
        let record_hash = body.as_object_mut().and_then(|m| m.shift_remove("hash")).unwrap_or(V::Null);
        sequence += 1;
        if record_hash.as_str() != Some(hash(&body).as_str())
            || body["sequence"].as_i64() != Some(sequence)
            || body["previous"].as_str() != Some(previous.as_str()) {
            return Err("Passive journal integrity failure".into());
        }
        let end = initial_source[offset..].iter().position(|x| *x == b'\n').map(|p| p + offset);
        let Some(end) = end else { return Err("Source capture truncated".into()) };
        let raw = &initial_source[offset..=end];
        if hash_bytes(raw) != body["sourceLineHash"].as_str().unwrap_or("") {
            return Err("Processed source capture changed".into());
        }
        let output = crate::passive_capture::consume_passive_capture(&state,
            &serde_json::from_slice(raw).map_err(|e| e.to_string())?)?;
        if hash(&output["records"]) != body["recordsHash"].as_str().unwrap_or("")
            || hash(&output["events"]) != body["eventsHash"].as_str().unwrap_or("")
            || hash(&output["state"]) != body["stateHash"].as_str().unwrap_or("") {
            return Err("Passive replay mismatch".into());
        }
        state = output["state"].clone();
        offset = end + 1;
        previous = record_hash.as_str().unwrap_or("").to_string();
    }
    Ok(PassiveWatch { runtime: runtime.to_path_buf(), bank, state, journal_file,
        capture_path, offset, sequence, previous, now: Box::new(now) })
}
/// runPassiveWatch — the main() entry: consume + publish stats until stopped.
pub fn run_passive_watch(runtime: &Path, workspace: &Path, initialize: bool, once: bool,
    now: impl Fn() -> f64 + 'static) -> Result<(), String> {
    let stopped = |runtime: &Path| -> bool {
        [workspace.join("STOP"), runtime.join("STOP"), runtime.join("PASSIVE_STOP")]
            .iter().any(|p| p.exists())
    };
    if stopped(runtime) { return Err("Passive STOP exists".into()); }
    let _release = crate::paper_runtime::lock(&runtime.join("passive-watch.lock"))?;
    let code_identity = passive_code_identity(workspace)?;
    let mut watch: Option<PassiveWatch> = None;
    let now = std::rc::Rc::new(now);
    let r = (|| -> Result<(), String> {
        let now2 = now.clone();
        watch = Some(open_passive_watch(runtime, initialize, {
            let n = now2.clone();
            move || (*n)()
        }, &code_identity)?);
        let w = watch.as_mut().unwrap();
        loop {
            if stopped(runtime) { break; }
            w.consume()?;
            let report = w.status();
            let current = w.session_current() && report["sourceEnded"] != V::Bool(true);
            let mut out = report.clone();
            out["runtime"] = json!({"status": if current {
                    if report["sourceAgeMs"].as_f64().unwrap_or(f64::MAX) < 20000.0 { "running" }
                    else { "waiting_for_updates" }
                } else { "source_session_ended" }, "pid": std::process::id()});
            crate::paper_runtime::atomic_json(&runtime.join("passive-stats.json"), &out)?;
            if !current || once { break; }
            std::thread::sleep(std::time::Duration::from_millis(2000));
        }
        let report = w.status();
        let current = w.session_current() && report["sourceEnded"] != V::Bool(true);
        let mut out = report.clone();
        out["runtime"] = json!({"status": if once { "initialized" }
            else if current { "stopped" } else { "source_session_ended" },
            "pid": std::process::id()});
        crate::paper_runtime::atomic_json(&runtime.join("passive-stats.json"), &out)?;
        Ok(())
    })();
    if r.is_err() {
        let at = baal_service_runtime::contract::ms_to_iso(now());
        let mut report = watch.as_ref().map(|w| w.status()).unwrap_or_else(||
            json!({"mode":"passive_opportunity_measurement","ordersSubmitted":0,
                "simulatedFills":V::Null,"pnl":V::Null,"markets":[]}));
        report["reportedAt"] = json!(at);
        let markets: Vec<V> = report["markets"].as_array().cloned().unwrap_or_default().into_iter()
            .map(|mut m| { m["ready"] = json!(false); m }).collect();
        report["markets"] = json!(markets);
        report["runtime"] = json!({"status":"failed","pid":std::process::id(),
            "reason":"passive_worker_failed_closed"});
        let _ = crate::paper_runtime::atomic_json(&runtime.join("passive-stats.json"), &report);
        let _ = crate::paper_runtime::atomic_json(&runtime.join("passive-error.json"),
            &json!({"at":at,"reason":"passive_worker_failed_closed","pid":std::process::id()}));
    }
    r
}
