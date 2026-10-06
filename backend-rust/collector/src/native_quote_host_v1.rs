// live-paper/native-quote-host-v1.mjs — frozen causal-quote host replaying the
// books/quotes/eligibility journals through the native quote engine.
use crate::native_dip_host_v1::{aborted, file_dev_ino, file_identity_map, js_eq, jstr,
    object_entries, read_json, safe_name, sealed_native_prefix, sha_file, uuid_v4,
    write_exclusive_600, AbortSignal, run_native_dip_child, SealedPrefix};
use baal_execution::core::payload_hash;
use baal_service_runtime::atomic::atomic_json_canonical;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::{js_int, js_stringify, sha256};
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(format!("NATIVE_QUOTE_{}", code)); }
    Ok(())
}
/// /^NATIVE_QUOTE_[A-Z_]+$/ — which error codes surface verbatim in lastError.
fn is_native_quote_code(m: &str) -> bool {
    m.strip_prefix("NATIVE_QUOTE_").is_some_and(|r| !r.is_empty() && r.bytes().all(|b| b.is_ascii_uppercase() || b == b'_'))
}
fn jnum(v: &V) -> f64 {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse::<f64>().ok())).unwrap_or(f64::NAN)
}
// JS TypeError when reading a property of null/undefined.
fn type_error_prop(prop: &str) -> String {
    format!("TypeError: Cannot read properties of undefined (reading '{}')", prop)
}

pub const NATIVE_QUOTE_HOST_FILES: [&str; 7] = [
    "live-paper/native-quote-host-v1.mjs", "live-paper/native-quote-proof-v1.mjs",
    "live-paper/native-dip-host-v1.mjs", "live-paper/native-dip-proof-v1.mjs",
    "live-paper/account-eligibility-publication-v1.mjs", "live-paper/runtime.mjs",
    "execution/ledger.mjs",
];
/// nativeQuoteHostIdentity(workspace)
pub fn native_quote_host_identity(workspace: &Path) -> Result<V, String> {
    file_identity_map(workspace, &NATIVE_QUOTE_HOST_FILES)
}
/// quoteSourcePrefix(file) — empty file seals at byte 0.
pub fn quote_source_prefix(file: &Path) -> Result<SealedPrefix, String> {
    let stat = std::fs::metadata(file).map_err(|e| e.to_string())?;
    if stat.len() == 0 {
        let (dev, ino) = file_dev_ino(&stat);
        return Ok(SealedPrefix { bytes: 0, dev, ino });
    }
    sealed_native_prefix(file)
}

struct QuoteCurrent {
    result: V,
    projection: V,
}
struct QuoteInner {
    current: QuoteCurrent,
    closed: bool,
    busy: bool,
    settling: bool,
    next_at: i64,
    last_error: Option<String>,
    last_wall_ms: Option<f64>,
    verified_at: Option<String>,
}
/// openNativeQuoteHost({runtimeDir, universe, signal, canOperate, now})
pub struct NativeQuoteHost {
    runtime_dir: PathBuf,
    dir: PathBuf,
    active: V,
    binding: V,
    declaration: V,
    universe: V,
    active_file: PathBuf,
    binding_file: PathBuf,
    declaration_file: PathBuf,
    exe: PathBuf,
    library: PathBuf,
    journals: Vec<(String, PathBuf)>,
    status_file: PathBuf,
    pointer_file: PathBuf,
    active_hash: String,
    signal: Option<AbortSignal>,
    control: AbortSignal,
    can_operate: Arc<dyn Fn() -> bool + Send + Sync>,
    now: Arc<dyn Fn() -> i64 + Send + Sync>,
    inner: Mutex<QuoteInner>,
}
impl NativeQuoteHost {
    fn interval_ms(&self) -> i64 { js_int(&self.active["intervalMs"]).unwrap_or(0) }
    fn permitted(&self) -> bool {
        !self.inner.lock().unwrap().closed && !aborted(self.signal.as_ref()) && (self.can_operate)()
    }
    fn now(&self) -> i64 { (self.now)() }
    fn out_path(&self, name: &V) -> Result<PathBuf, String> {
        need(name.as_str().is_some_and(|s| s == "seed.json"
            || (s.len() == 41 && s.ends_with(".json")
                && s[..36].bytes().all(|b| (b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) || b == b'-'))), "OUTPUT_PATH")?;
        Ok(self.dir.join(name.as_str().unwrap()))
    }
    fn validate(&self) -> Result<(), String> {
        need(sha_file(&self.active_file)? == self.active_hash
            && sha_file(&self.binding_file)? == self.active["bindingHash"].as_str().unwrap_or("")
            && sha_file(&self.declaration_file)? == self.binding["declarationHash"].as_str().unwrap_or("")
            && sha_file(&self.exe)? == self.binding["binaryHash"].as_str().unwrap_or("")
            && sha_file(&self.library)? == self.binding["runtimeLibraryHash"].as_str().unwrap_or(""), "IDENTITY_CHANGED")
    }
    pub fn status(&self) -> V {
        let i = self.inner.lock().unwrap();
        let mut s = i.current.projection.as_object().cloned().unwrap_or_default();
        let focus = crate::native_quote_proof_v1::native_quote_read_focus(&i.current.projection, self.now());
        if let V::Object(f) = focus {
            for (k, v) in f { s.insert(k, v); }
        }
        s.insert("activeHash".into(), json!(self.active_hash));
        s.insert("status".into(), json!(if i.closed { "stopped" } else if i.last_error.is_some() { "degraded" } else if i.busy { "replaying" } else { "running" }));
        s.insert("lastError".into(), i.last_error.as_ref().map(|x| json!(x)).unwrap_or(V::Null));
        s.insert("lastWallMs".into(), i.last_wall_ms.map(|x| json!(x)).unwrap_or(V::Null));
        s.insert("verifiedAt".into(), i.verified_at.as_ref().map(|x| json!(x)).unwrap_or(V::Null));
        s.insert("replayInProgress".into(), json!(i.busy));
        let t = baal_service_runtime::contract::parse_iso_ms(i.current.projection["through"].as_str().unwrap_or(""));
        let lag = t.map(|x| (self.now() as f64 - x).max(0.0));
        s.insert("sourceLagMs".into(), lag.map(|x| json!(x)).unwrap_or(V::Null));
        s.insert("heldRiskRetained".into(), json!(true));
        s.insert("additionalPersistentProcesses".into(), json!(0));
        V::Object(s)
    }
    fn publish(&self) -> Result<(), String> {
        if self.permitted() { atomic_json_canonical(&self.status_file, &self.status())?; }
        Ok(())
    }
    fn replay(&self) -> Result<(), String> {
        let started = std::time::Instant::now();
        self.validate()?;
        let mut prefixes: Vec<(String, SealedPrefix)> = Vec::new();
        for (name, journal) in &self.journals {
            prefixes.push((name.clone(), quote_source_prefix(journal)?));
        }
        let mut sources = serde_json::Map::new();
        let prior_result = self.inner.lock().unwrap().current.result.clone();
        for (name, journal) in &self.journals {
            let prior = &prior_result["capture"]["sources"][name];
            let dev = &self.declaration["development"][name];
            let prefix = &prefixes.iter().find(|(n, _)| n == name).unwrap().1;
            // JS: prior.bytes on undefined throws TypeError; on primitives it is
            // undefined -> NaN comparison -> SOURCE_REGRESSION.
            if prior.is_null() { return Err(type_error_prop("bytes")); }
            need(prefix.bytes as f64 >= jnum(&prior["bytes"]), "SOURCE_REGRESSION")?;
            if dev.is_null() { return Err(type_error_prop("bytes")); }
            let mut checkpoints = vec![dev.clone()];
            if !js_eq(&prior["bytes"], &dev["bytes"]) {
                checkpoints.push(json!({"bytes": prior["bytes"], "hash": prior["hash"]}));
            } else {
                need(js_eq(&prior["hash"], &dev["hash"]), "SOURCE_HISTORY")?;
            }
            sources.insert(name.clone(), json!({"path": journal.display().to_string(),
                "bytes": prefix.bytes, "hash": V::Null, "checkpoints": checkpoints}));
        }
        let output = format!("{}.json", uuid_v4());
        let out = self.out_path(&json!(output))?;
        let sources_file = PathBuf::from(format!("{}.sources.json", out.display()));
        write_exclusive_600(&sources_file, js_stringify(&V::Object(sources)).as_bytes())?;
        need(self.permitted(), "STOPPED")?;
        run_native_dip_child(&self.exe, &[
            "quote-replay".into(), "--declaration".into(), self.declaration_file.display().to_string(),
            "--sources".into(), sources_file.display().to_string(),
            "--workers".into(), jstr(&self.active["workers"]),
            "--out".into(), out.display().to_string()], Some(&self.control))?;
        need(self.permitted(), "STOPPED")?;
        self.validate()?;
        for (name, journal) in &self.journals {
            let stat = std::fs::metadata(journal).map_err(|e| e.to_string())?;
            let prefix = &prefixes.iter().find(|(n, _)| n == name).unwrap().1;
            let (dev, ino) = file_dev_ino(&stat);
            need(dev == prefix.dev && ino == prefix.ino && stat.len() >= prefix.bytes, "SOURCE_REPLACED")?;
        }
        let bytes = std::fs::read(&out).map_err(|e| e.to_string())?;
        need(bytes.len() <= 32 * 1024 * 1024, "OUTPUT_SIZE")?;
        let result: V = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        for (name, _) in &self.journals {
            let prefix = &prefixes.iter().find(|(n, _)| n == name).unwrap().1;
            need(js_eq(&result["capture"]["sources"][name]["bytes"], &json!(prefix.bytes)), "PREFIX_BINDING")?;
        }
        let projection = crate::native_quote_proof_v1::verify_native_quote_result(&result, &self.declaration, &self.binding, &self.universe, Some(&prior_result))?;
        let output_hash = sha256(&bytes);
        let at = ms_to_iso(self.now() as f64);
        let receipt = json!({"at": at, "binaryHash": self.binding["binaryHash"],
            "engineHash": self.binding["engineHash"], "declarationHash": self.binding["declarationHash"],
            "sources": result["capture"]["sources"], "outputHash": output_hash,
            "networkRequests": 0, "competitionOrders": 0});
        write_exclusive_600(&PathBuf::from(format!("{}.receipt.json", out.display())),
            format!("{}\n", serde_json::to_string_pretty(&receipt).map_err(|e| e.to_string())?).as_bytes())?;
        need(self.permitted(), "STOPPED")?;
        atomic_json_canonical(&self.pointer_file, &json!({"schemaVersion": 1,
            "activeHash": self.active_hash, "at": at, "output": output, "outputHash": output_hash}))?;
        let wall = started.elapsed().as_secs_f64() * 1000.0;
        let mut i = self.inner.lock().unwrap();
        i.current = QuoteCurrent { result, projection };
        i.last_wall_ms = Some(wall);
        i.verified_at = Some(at);
        i.last_error = None;
        Ok(())
    }
    pub fn kick(&self) -> bool {
        {
            let mut i = self.inner.lock().unwrap();
            if i.busy { return true; }
            if i.closed || aborted(self.signal.as_ref()) || !(self.can_operate)() || self.now() < i.next_at {
                return false;
            }
            i.next_at = self.now() + self.interval_ms();
            i.busy = true;
        }
        let outcome = self.replay();
        {
            let mut i = self.inner.lock().unwrap();
            i.busy = false;
            i.settling = true;
            if let Err(e) = outcome {
                i.last_error = Some(if is_native_quote_code(&e) { e } else { "NATIVE_QUOTE_REPLAY_UNAVAILABLE".into() });
            }
        }
        if self.publish().is_err() {
            self.inner.lock().unwrap().last_error = Some("NATIVE_QUOTE_STATUS_UNAVAILABLE".into());
        }
        self.inner.lock().unwrap().settling = false;
        true
    }
    pub fn close(&self) {
        self.inner.lock().unwrap().closed = true;
        self.control.store(true, Ordering::SeqCst);
        loop {
            let i = self.inner.lock().unwrap();
            if !i.busy && !i.settling { break; }
            drop(i);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
}
impl Drop for NativeQuoteHost {
    fn drop(&mut self) { self.control.store(true, Ordering::SeqCst); }
}
/// openNativeQuoteHost({runtimeDir, universe, signal, canOperate, now})
pub fn open_native_quote_host(runtime_dir: &Path, workspace: &Path, universe: &V,
    signal: Option<AbortSignal>,
    can_operate: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    now: Option<Arc<dyn Fn() -> i64 + Send + Sync>>) -> Result<Option<NativeQuoteHost>, String> {
    let active_file = runtime_dir.join("native-quote-active.json");
    if !active_file.exists() { return Ok(None); }
    let active_bytes = std::fs::read(&active_file).map_err(|e| e.to_string())?;
    let active: V = serde_json::from_slice(&active_bytes).map_err(|e| e.to_string())?;
    let active_hash = sha256(&active_bytes);
    need(js_int(&active["schemaVersion"]) == Some(1)
        && active["mode"].as_str() == Some("frozen_native_quote_history_host_v1")
        && safe_name(&active["cohort"])
        && js_int(&active["intervalMs"]).is_some_and(|i| (30000..=900000).contains(&i))
        && js_int(&active["workers"]).is_some_and(|w| (1..=32).contains(&w)), "ACTIVE")?;
    let dir = runtime_dir.join("research-cohorts").join(active["cohort"].as_str().unwrap());
    let binding_file = dir.join("binding.json");
    let declaration_file = dir.join("declaration.json");
    let binding = read_json(&binding_file)?;
    let declaration = read_json(&declaration_file)?;
    let pointer_file = dir.join("host-latest.json");
    let status_file = runtime_dir.join("native-quote-status.json");
    need(sha_file(&binding_file)? == active["bindingHash"].as_str().unwrap_or("")
        && sha_file(&declaration_file)? == binding["declarationHash"].as_str().unwrap_or("")
        && js_eq(&binding["cohort"], &active["cohort"])
        && payload_hash(&native_quote_host_identity(workspace)?).ok() == payload_hash(&active["hostSourceHashes"]).ok(), "FROZEN")?;
    for (name, expected) in object_entries(&binding["sourceHashes"])? {
        need(name.starts_with("backend-rust/") && !name.contains(".."), "SOURCE_PATH")?;
        need(sha_file(&runtime_dir.join("native-sources")
                .join(binding["engineHash"].as_str().unwrap_or("")).join(&name[13..]))?
            == expected.as_str().unwrap_or(""), "SOURCE_IDENTITY")?;
    }
    let build = runtime_dir.join("native-builds").join(binding["binaryHash"].as_str().unwrap_or(""));
    let host = NativeQuoteHost {
        runtime_dir: runtime_dir.to_path_buf(),
        dir: dir.clone(),
        active: active.clone(),
        binding: binding.clone(),
        declaration: declaration.clone(),
        universe: universe.clone(),
        active_file: active_file.clone(),
        binding_file,
        declaration_file,
        exe: build.join("baal-backend.exe"),
        library: build.join("libunwind.dll"),
        journals: vec![
            ("books".into(), runtime_dir.join("public-books.jsonl")),
            ("quotes".into(), runtime_dir.join("market-prices.jsonl")),
            ("eligibility".into(), runtime_dir.join("account-eligibility.jsonl")),
        ],
        status_file,
        pointer_file: pointer_file.clone(),
        active_hash,
        signal,
        control: Arc::new(AtomicBool::new(false)),
        can_operate: can_operate.unwrap_or_else(|| Arc::new(|| true)),
        now: now.unwrap_or_else(|| Arc::new(|| std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64)),
        inner: Mutex::new(QuoteInner { current: QuoteCurrent { result: V::Null, projection: V::Null },
            closed: false, busy: false, settling: false, next_at: 0, last_error: None, last_wall_ms: None, verified_at: None }),
    };
    if let Some(sig) = &host.signal {
        let s = sig.clone();
        let c = host.control.clone();
        std::thread::spawn(move || loop {
            if s.load(Ordering::SeqCst) { c.store(true, Ordering::SeqCst); break; }
            if c.load(Ordering::SeqCst) { break; }
            std::thread::sleep(std::time::Duration::from_millis(5));
        });
    }
    if aborted(host.signal.as_ref()) { host.control.store(true, Ordering::SeqCst); }
    host.validate()?;
    // Restore sealed output + receipt, then re-prove it.
    let pointer = if pointer_file.exists() {
        read_json(&host.pointer_file)?
    } else {
        let mut p = host.active["seed"].as_object().cloned().unwrap_or_default();
        p.insert("activeHash".into(), json!(host.active_hash));
        V::Object(p)
    };
    let file = host.out_path(&pointer["output"])?;
    let bytes = std::fs::read(&file).map_err(|e| e.to_string())?;
    let receipt = read_json(&PathBuf::from(format!("{}.receipt.json", file.display())))?;
    need(js_eq(&pointer["activeHash"], &json!(host.active_hash))
        && sha256(&bytes) == pointer["outputHash"].as_str().unwrap_or("")
        && js_eq(&receipt["outputHash"], &pointer["outputHash"])
        && js_eq(&receipt["binaryHash"], &binding["binaryHash"])
        && js_eq(&receipt["engineHash"], &binding["engineHash"])
        && js_eq(&receipt["declarationHash"], &binding["declarationHash"])
        && receipt["competitionOrders"].as_i64() == Some(0), "RESTORE")?;
    let result: V = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    need(payload_hash(&receipt["sources"]).ok() == payload_hash(&result["capture"]["sources"]).ok(), "RESTORE_SOURCE")?;
    let projection = crate::native_quote_proof_v1::verify_native_quote_result(&result, &declaration, &binding, universe, None)?;
    host.inner.lock().unwrap().current = QuoteCurrent { result, projection };
    host.publish()?;
    Ok(Some(host))
}
