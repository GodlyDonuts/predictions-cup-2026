// live-paper/native-dip-host-v1.mjs — frozen native dip host: sealed journal
// prefix → child native evaluate → verified output pointer + status journal.
use baal_execution::core::payload_hash;
use baal_service_runtime::atomic::atomic_json_canonical;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::{js_int, sha256};
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(format!("NATIVE_DIP_{}", code)); }
    Ok(())
}
/// AbortSignal port: a shared flag; `aborted()` mirrors `signal?.aborted`.
pub type AbortSignal = Arc<AtomicBool>;
pub(crate) fn aborted(signal: Option<&AbortSignal>) -> bool {
    signal.is_some_and(|s| s.load(Ordering::SeqCst))
}
pub(crate) fn read_json(p: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
pub(crate) fn sha_file(p: &Path) -> Result<String, String> {
    Ok(sha256(&std::fs::read(p).map_err(|e| e.to_string())?))
}
pub(crate) fn file_dev_ino(m: &std::fs::Metadata) -> (u64, u64) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        (m.dev(), m.ino())
    }
    #[cfg(not(unix))]
    {
        (0, 0)
    }
}
/// fs.writeFileSync(file, body, {flag:'wx',mode:0o600})
pub(crate) fn write_exclusive_600(p: &Path, body: &[u8]) -> Result<(), String> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o.open(p)
        .and_then(|mut f| std::io::Write::write_all(&mut f, body))
        .map_err(|e| e.to_string())
}
/// crypto.randomUUID()
pub(crate) fn uuid_v4() -> String {
    let mut b = [0u8; 16];
    let mut seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64
        ^ std::process::id() as u64;
    for byte in &mut b {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        *byte = seed as u8;
    }
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    format!("{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15])
}
/// /^NATIVE_DIP_[A-Z_]+$/ — which error codes surface verbatim in lastError.
pub(crate) fn is_native_dip_code(m: &str) -> bool {
    m.strip_prefix("NATIVE_DIP_").is_some_and(|r| !r.is_empty() && r.bytes().all(|b| b.is_ascii_uppercase() || b == b'_'))
}
/// JS String(x) for value interpolation (argv, object keys).
pub(crate) fn jstr(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Number(n) => {
            let mut b = ryu_js::Buffer::new();
            b.format_finite(n.as_f64().unwrap_or(f64::NAN)).to_string()
        }
        V::Bool(b) => if *b { "true" } else { "false" }.to_string(),
        V::Null => "null".to_string(),
        _ => serde_json::to_string(v).unwrap_or_default(),
    }
}
/// JS safeName: /^[a-zA-Z0-9_-]{1,100}$/
pub(crate) fn safe_name(s: &V) -> bool {
    s.as_str().is_some_and(|x| !x.is_empty() && x.len() <= 100
        && x.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'))
}
/// JS uuid: /^[a-f0-9-]{36}$/
pub(crate) fn uuid_str(s: &V) -> bool {
    s.as_str().is_some_and(|x| x.len() == 36
        && x.bytes().all(|b| (b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) || b == b'-'))
}
/// JS === on possibly-numeric JSON values.
pub(crate) fn js_eq(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Number(_), V::Number(_)) => a.as_f64().unwrap_or(f64::NAN) == b.as_f64().unwrap_or(f64::NAN),
        _ => a == b,
    }
}
/// Object.entries(v): objects iterate in key order; null/undefined throw
/// TypeError in JS — surfaced as a raw (non-NATIVE_DIP) error here.
pub(crate) fn object_entries(v: &V) -> Result<Vec<(String, V)>, String> {
    match v {
        V::Object(m) => Ok(m.iter().map(|(k, x)| (k.clone(), x.clone())).collect()),
        V::Null => Err("TypeError: Cannot convert undefined or null to object".into()),
        _ => Ok(Vec::new()),
    }
}
/// {file, sha256(fileBytes)} identity map over a module list.
pub(crate) fn file_identity_map(workspace: &Path, files: &[&str]) -> Result<V, String> {
    let mut m = serde_json::Map::new();
    for f in files {
        m.insert(f.to_string(), json!(sha_file(&workspace.join(f))?));
    }
    Ok(V::Object(m))
}
/// sealedNativePrefix(file) — hash a file's sealed newline-bounded prefix length.
pub struct SealedPrefix {
    pub bytes: u64,
    pub dev: u64,
    pub ino: u64,
}
pub fn sealed_native_prefix(file: &Path) -> Result<SealedPrefix, String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut fd = std::fs::File::open(file).map_err(|e| e.to_string())?;
    let stat = fd.metadata().map_err(|e| e.to_string())?;
    let size = stat.len();
    let n = size.min(65536) as usize;
    let mut b = vec![0u8; n];
    fd.seek(SeekFrom::Start(size - n as u64)).map_err(|_| "NATIVE_DIP_SOURCE_READ".to_string())?;
    fd.read_exact(&mut b).map_err(|_| "NATIVE_DIP_SOURCE_READ".to_string())?;
    let Some(at) = b.iter().rposition(|c| *c == b'\n') else {
        return Err("NATIVE_DIP_SOURCE_BOUNDARY".into());
    };
    let (dev, ino) = file_dev_ino(&stat);
    Ok(SealedPrefix { bytes: size - n as u64 + at as u64 + 1, dev, ino })
}
/// hashNativePrefix(file, end, prior, signal) — sha256 of the first `end` bytes
/// plus the first `prior` bytes, chunked like the JS stream reader.
pub fn hash_native_prefix(file: &Path, end: u64, prior: u64, signal: Option<&AbortSignal>) -> Result<(String, String), String> {
    use std::io::Read;
    let mut fd = std::fs::File::open(file).map_err(|e| e.to_string())?;
    let (mut whole, mut old) = (Sha256::new(), Sha256::new());
    let mut n = 0u64;
    let mut buf = vec![0u8; 65536];
    while n < end {
        let want = (end - n).min(65536) as usize;
        let r = fd.read(&mut buf[..want]).map_err(|e| e.to_string())?;
        if r == 0 { break; }
        need(!aborted(signal), "STOPPED")?;
        whole.update(&buf[..r]);
        if n < prior {
            let k = ((prior - n) as usize).min(r);
            old.update(&buf[..k]);
        }
        n += r as u64;
    }
    need(n == end && prior <= end, "SOURCE_TRUNCATED")?;
    Ok((format!("{:x}", whole.finalize()), format!("{:x}", old.finalize())))
}
/// runNativeDipChild(exe, args, signal) — spawn silent, cap stderr at 4096
/// bytes, abort kills the child.
pub fn run_native_dip_child(exe: &Path, args: &[String], signal: Option<&AbortSignal>) -> Result<(), String> {
    need(!aborted(signal), "STOPPED")?;
    use std::process::{Command, Stdio};
    let mut cmd = Command::new(exe);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // windowsHide
    }
    let mut child = cmd.spawn().map_err(|_| "NATIVE_DIP_CHILD_UNAVAILABLE".to_string())?;
    let bytes = Arc::new(AtomicUsize::new(0));
    let counter = bytes.clone();
    let stderr = child.stderr.take();
    let reader = std::thread::spawn(move || {
        use std::io::Read;
        if let Some(mut e) = stderr {
            let mut buf = [0u8; 8192];
            loop {
                match e.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(r) => {
                        counter.fetch_add(r, Ordering::SeqCst);
                    }
                }
            }
        }
    });
    let result = loop {
        match child.try_wait() {
            Ok(Some(st)) => break if st.success() { Ok(()) } else { Err("NATIVE_DIP_CHILD_FAILED".to_string()) },
            Ok(None) => {}
            Err(_) => break Err("NATIVE_DIP_CHILD_UNAVAILABLE".to_string()),
        }
        if aborted(signal) {
            let _ = child.kill();
            break Err("NATIVE_DIP_STOPPED".to_string());
        }
        if bytes.load(Ordering::SeqCst) > 4096 {
            let _ = child.kill();
            break Err("NATIVE_DIP_CHILD_FAILED".to_string());
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    };
    let _ = reader.join();
    result
}

pub const NATIVE_DIP_HOST_FILES: [&str; 3] = [
    "live-paper/native-dip-host-v1.mjs", "live-paper/native-dip-proof-v1.mjs", "execution/ledger.mjs",
];
/// nativeDipHostIdentity(workspace) — sha256 of each host-source file.
pub fn native_dip_host_identity(workspace: &Path) -> Result<V, String> {
    file_identity_map(workspace, &NATIVE_DIP_HOST_FILES)
}

struct DipCurrent {
    result: V,
    projection: V,
}
struct DipInner {
    current: DipCurrent,
    closed: bool,
    busy: bool,
    settling: bool,
    next_at: i64,
    last_error: Option<String>,
    last_wall_ms: Option<f64>,
    verified_at: Option<String>,
}
/// openNativeDipHost({runtimeDir, universe, signal, canOperate, now}) — `workspace`
/// is the repo root that holds the JS host sources (import.meta.url analog).
pub struct NativeDipHost {
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
    journal: PathBuf,
    status_file: PathBuf,
    pointer_file: PathBuf,
    active_hash: String,
    signal: Option<AbortSignal>,
    control: AbortSignal,
    can_operate: Arc<dyn Fn() -> bool + Send + Sync>,
    now: Arc<dyn Fn() -> i64 + Send + Sync>,
    inner: Mutex<DipInner>,
}
impl NativeDipHost {
    fn interval_ms(&self) -> i64 { js_int(&self.active["intervalMs"]).unwrap_or(0) }
    fn permitted(&self) -> bool {
        !self.inner.lock().unwrap().closed && !aborted(self.signal.as_ref()) && (self.can_operate)()
    }
    fn now(&self) -> i64 { (self.now)() }
    fn output_path(&self, name: &V) -> Result<PathBuf, String> {
        need(name.as_str().is_some_and(|s| s.len() == 41 && s.ends_with(".json")
            && s[..36].bytes().all(|b| (b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) || b == b'-')), "OUTPUT_PATH")?;
        Ok(self.dir.join(name.as_str().unwrap()))
    }
    fn validate_binding(&self) -> Result<(), String> {
        need(sha_file(&self.active_file)? == self.active_hash
            && sha_file(&self.binding_file)? == self.active["bindingHash"].as_str().unwrap_or("")
            && sha_file(&self.declaration_file)? == self.binding["declarationHash"].as_str().unwrap_or("")
            && sha_file(&self.exe)? == self.binding["binaryHash"].as_str().unwrap_or("")
            && sha_file(&self.library)? == self.binding["runtimeLibraryHash"].as_str().unwrap_or(""), "IDENTITY_CHANGED")
    }
    fn restore(&self, pointer: &V) -> Result<DipCurrent, String> {
        let file = self.output_path(&pointer["output"])?;
        need(js_eq(&pointer["activeHash"], &json!(self.active_hash))
            && sha_file(&file)? == pointer["outputHash"].as_str().unwrap_or(""), "OUTPUT_HASH")?;
        let receipt = read_json(&PathBuf::from(format!("{}.receipt.json", file.display())))?;
        need(js_eq(&receipt["binaryHash"], &self.binding["binaryHash"])
            && js_eq(&receipt["engineHash"], &self.binding["engineHash"])
            && js_eq(&receipt["declarationFileHash"], &self.binding["declarationHash"])
            && js_eq(&receipt["outputHash"], &pointer["outputHash"])
            && receipt["competitionOrders"].as_i64() == Some(0), "RECEIPT")?;
        let result = read_json(&file)?;
        need(js_eq(&receipt["sourceHash"], &result["capture"]["source_hash"])
            && js_eq(&receipt["sourceBytes"], &result["capture"]["source_bytes"]), "RECEIPT_SOURCE")?;
        let projection = crate::native_dip_proof_v1::verify_native_dip_result(&result, &self.declaration, &self.binding, &self.universe, None)?;
        Ok(DipCurrent { result, projection })
    }
    /// status() — projection plus live host state.
    pub fn status(&self) -> V {
        let i = self.inner.lock().unwrap();
        let mut s = i.current.projection.as_object().cloned().unwrap_or_default();
        s.insert("activeHash".into(), json!(self.active_hash));
        s.insert("status".into(), json!(if i.closed { "stopped" } else if i.last_error.is_some() { "degraded" } else if i.busy { "replaying" } else { "running" }));
        s.insert("replayInProgress".into(), json!(i.busy));
        s.insert("verifiedAt".into(), i.verified_at.as_ref().map(|x| json!(x)).unwrap_or(V::Null));
        s.insert("lastError".into(), i.last_error.as_ref().map(|x| json!(x)).unwrap_or(V::Null));
        s.insert("lastWallMs".into(), i.last_wall_ms.map(|x| json!(x)).unwrap_or(V::Null));
        let lag = baal_service_runtime::contract::parse_iso_ms(i.current.projection["through"].as_str().unwrap_or(""))
            .map(|t| (self.now() as f64 - t).max(0.0));
        s.insert("sourceLagMs".into(), lag.map(|l| json!(l)).unwrap_or(V::Null));
        s.insert("riskSourceStale".into(), json!(lag.is_some_and(|l| l > self.interval_ms() as f64 * 2.0)));
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
        self.validate_binding()?;
        let prefix = sealed_native_prefix(&self.journal)?;
        let (prior_result, prior_bytes, prior_hash) = {
            let i = self.inner.lock().unwrap();
            (i.current.result.clone(),
                js_int(&i.current.result["capture"]["source_bytes"]).unwrap_or(i64::MAX) as u64,
                i.current.result["capture"]["source_hash"].as_str().unwrap_or("").to_string())
        };
        need(prefix.bytes >= prior_bytes, "SOURCE_REGRESSION")?;
        let hashes = hash_native_prefix(&self.journal, prefix.bytes, prior_bytes, Some(&self.control))?;
        need(hashes.1 == prior_hash, "SOURCE_HISTORY_CHANGED")?;
        if prefix.bytes == prior_bytes {
            let at = ms_to_iso(self.now() as f64);
            let mut i = self.inner.lock().unwrap();
            i.verified_at = Some(at);
            i.last_error = None;
            return Ok(());
        }
        need(self.permitted(), "STOPPED")?;
        let output = format!("{}.json", uuid_v4());
        let file = self.output_path(&json!(output))?;
        run_native_dip_child(&self.exe, &[
            "evaluate".into(), "--journal".into(), self.journal.display().to_string(),
            "--declaration".into(), self.declaration_file.display().to_string(),
            "--end-byte".into(), prefix.bytes.to_string(), "--expected-sha".into(), hashes.0.clone(),
            "--workers".into(), js_int(&self.active["workers"]).unwrap_or(1).to_string(),
            "--out".into(), file.display().to_string()], Some(&self.control))?;
        need(self.permitted(), "STOPPED")?;
        self.validate_binding()?;
        let stat = std::fs::metadata(&self.journal).map_err(|e| e.to_string())?;
        let (dev, ino) = file_dev_ino(&stat);
        need(dev == prefix.dev && ino == prefix.ino && stat.len() >= prefix.bytes, "SOURCE_REPLACED")?;
        let bytes = std::fs::read(&file).map_err(|e| e.to_string())?;
        need(bytes.len() <= 32 * 1024 * 1024, "OUTPUT_SIZE")?;
        let result: V = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        need(result["capture"]["source_hash"].as_str() == Some(hashes.0.as_str())
            && js_int(&result["capture"]["source_bytes"]) == Some(prefix.bytes as i64), "PREFIX_BINDING")?;
        let projection = crate::native_dip_proof_v1::verify_native_dip_result(&result, &self.declaration, &self.binding, &self.universe, Some(&prior_result))?;
        let output_hash = sha256(&bytes);
        let at = ms_to_iso(self.now() as f64);
        let receipt = json!({"at": at, "sourceHash": hashes.0, "sourceBytes": prefix.bytes,
            "engineHash": self.binding["engineHash"], "declarationFileHash": self.binding["declarationHash"],
            "binaryHash": self.binding["binaryHash"], "runtimeLibraryHash": self.binding["runtimeLibraryHash"],
            "outputHash": output_hash, "networkRequests": 0, "competitionOrders": 0});
        write_exclusive_600(&PathBuf::from(format!("{}.receipt.json", file.display())),
            format!("{}\n", serde_json::to_string_pretty(&receipt).map_err(|e| e.to_string())?).as_bytes())?;
        need(self.permitted(), "STOPPED")?;
        atomic_json_canonical(&self.pointer_file, &json!({"schemaVersion": 1, "at": at,
            "activeHash": self.active_hash, "output": output, "outputHash": output_hash}))?;
        let wall = started.elapsed().as_secs_f64() * 1000.0;
        let mut i = self.inner.lock().unwrap();
        i.current = DipCurrent { result, projection };
        i.verified_at = Some(at);
        i.last_error = None;
        i.last_wall_ms = Some(wall);
        Ok(())
    }
    /// kick() — runs one replay inline when due; returns whether a replay ran
    /// (JS returned the in-flight promise).
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
                i.last_error = Some(if is_native_dip_code(&e) { e } else { "NATIVE_DIP_UNAVAILABLE".into() });
            }
        }
        if self.publish().is_err() {
            self.inner.lock().unwrap().last_error = Some("NATIVE_DIP_STATUS_UNAVAILABLE".into());
        }
        self.inner.lock().unwrap().settling = false;
        true
    }
    /// close() — stop, abort the run signal, wait out an in-flight replay and
    /// its trailing publish (JS `await busy` covers the .finally chain).
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
impl Drop for NativeDipHost {
    fn drop(&mut self) { self.control.store(true, Ordering::SeqCst); }
}
/// openNativeDipHost({runtimeDir, universe, signal, canOperate, now})
pub fn open_native_dip_host(runtime_dir: &Path, workspace: &Path, universe: &V,
    signal: Option<AbortSignal>,
    can_operate: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    now: Option<Arc<dyn Fn() -> i64 + Send + Sync>>) -> Result<Option<NativeDipHost>, String> {
    let active_file = runtime_dir.join("native-dip-active.json");
    if !active_file.exists() { return Ok(None); }
    let active_bytes = std::fs::read(&active_file).map_err(|e| e.to_string())?;
    let active: V = serde_json::from_slice(&active_bytes).map_err(|e| e.to_string())?;
    let active_hash = sha256(&active_bytes);
    need(js_int(&active["schemaVersion"]) == Some(1)
        && active["mode"].as_str() == Some("frozen_native_dip_paper_host_v1")
        && safe_name(&active["cohort"])
        && js_int(&active["intervalMs"]).is_some_and(|i| (60000..=900000).contains(&i))
        && js_int(&active["workers"]).is_some_and(|w| (1..=32).contains(&w)), "ACTIVE")?;
    let dir = runtime_dir.join("research-cohorts").join(active["cohort"].as_str().unwrap());
    let binding_file = dir.join("native-clock-binding.json");
    let declaration_file = dir.join("declaration.json");
    let binding = read_json(&binding_file)?;
    let declaration = read_json(&declaration_file)?;
    need(sha_file(&binding_file)? == active["bindingHash"].as_str().unwrap_or("")
        && sha_file(&declaration_file)? == binding["declarationHash"].as_str().unwrap_or("")
        && js_eq(&binding["cohort"], &active["cohort"]), "FROZEN")?;
    need(payload_hash(&native_dip_host_identity(workspace)?).ok() == payload_hash(&active["hostSourceHashes"]).ok(), "HOST_IDENTITY")?;
    for (name, expected) in object_entries(&binding["sourceHashes"])? {
        need(name.starts_with("backend-rust/") && !name.contains(".."), "SOURCE_SCOPE")?;
        need(sha_file(&runtime_dir.join("native-sources")
                .join(binding["engineHash"].as_str().unwrap_or("")).join(&name[13..]))?
            == expected.as_str().unwrap_or(""), "ENGINE_SOURCE")?;
    }
    let build = runtime_dir.join("native-builds").join(binding["binaryHash"].as_str().unwrap_or(""));
    let host = NativeDipHost {
        runtime_dir: runtime_dir.to_path_buf(),
        dir: dir.clone(),
        active: active.clone(),
        binding: binding.clone(),
        declaration,
        universe: universe.clone(),
        active_file: active_file.clone(),
        binding_file,
        declaration_file,
        exe: build.join("baal-backend.exe"),
        library: build.join("libunwind.dll"),
        journal: runtime_dir.join("journal.jsonl"),
        status_file: runtime_dir.join("native-dip-status.json"),
        pointer_file: dir.join("native-host-latest.json"),
        active_hash,
        signal,
        control: Arc::new(AtomicBool::new(false)),
        can_operate: can_operate.unwrap_or_else(|| Arc::new(|| true)),
        now: now.unwrap_or_else(|| Arc::new(|| std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64)),
        inner: Mutex::new(DipInner { current: DipCurrent { result: V::Null, projection: V::Null },
            closed: false, busy: false, settling: false, next_at: 0, last_error: None, last_wall_ms: None, verified_at: None }),
    };
    // Parent abort → run control (JS addEventListener('abort', parentAbort)).
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
    host.validate_binding()?;
    let pointer = if host.pointer_file.exists() {
        read_json(&host.pointer_file)?
    } else {
        let mut p = host.active["seed"].as_object().cloned().unwrap_or_default();
        p.insert("activeHash".into(), json!(host.active_hash));
        V::Object(p)
    };
    let current = host.restore(&pointer)?;
    host.inner.lock().unwrap().current = current;
    host.publish()?;
    Ok(Some(host))
}
