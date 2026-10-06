// live-paper/native-dip-host-v2.mjs — versioned execution continuation on an
// immutable capture cache prefix plus verified append of the live journal.
use crate::native_dip_host_v1::{aborted, file_dev_ino, file_identity_map, is_native_dip_code, js_eq,
    jstr, object_entries, read_json, safe_name, sealed_native_prefix, sha_file, uuid_str, uuid_v4,
    write_exclusive_600, AbortSignal, run_native_dip_child};
use baal_execution::core::payload_hash;
use baal_service_runtime::atomic::atomic_json_canonical;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::{js_int, sha256};
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(format!("NATIVE_DIP_CACHE_{}", code)); }
    Ok(())
}
fn jnum(v: &V) -> f64 {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse::<f64>().ok())).unwrap_or(f64::NAN)
}
// String(x) for child argv — JS String() coercion.
fn sarg(v: &V) -> String { jstr(v) }

pub const NATIVE_DIP_CACHE_HOST_FILES: [&str; 4] = [
    "live-paper/native-dip-host-v2.mjs", "live-paper/native-dip-host-v1.mjs",
    "live-paper/native-dip-proof-v1.mjs", "execution/ledger.mjs",
];
/// nativeDipCacheHostIdentity(workspace)
pub fn native_dip_cache_host_identity(workspace: &Path) -> Result<V, String> {
    file_identity_map(workspace, &NATIVE_DIP_CACHE_HOST_FILES)
}
/// verifyCacheProof(result, binding, prior) — errors prefixed NATIVE_DIP_CACHE_.
pub fn verify_cache_proof(result: &V, binding: &V, prior: &V) -> Result<(), String> {
    let p = &result["cacheProof"];
    let c = &binding["cache"];
    if !c.is_object() {
        return Err("TypeError: Cannot read properties of undefined (reading 'cacheHash')".into());
    }
    let appended = jnum(&result["capture"]["source_bytes"]) - jnum(&c["sourceBytes"]);
    need(js_int(&p["schemaVersion"]) == Some(1)
        && p["mode"].as_str() == Some("pinned_original_prefix_cache_and_verified_append_v1")
        && js_eq(&p["cacheHash"], &c["cacheHash"])
        && js_eq(&p["sourceHash"], &c["sourceHash"])
        && js_eq(&p["sourceBytes"], &c["sourceBytes"])
        && js_eq(&p["priorSourceHash"], &prior["sourceHash"])
        && js_eq(&p["priorSourceBytes"], &prior["sourceBytes"])
        && p["fullOriginalPrefixRehashed"] == V::Bool(true)
        && js_eq(&p["appendedBytes"], &json!(appended)), "PROOF")?;
    Ok(())
}

struct CacheCurrent {
    result: V,
    projection: V,
}
struct CacheInner {
    current: CacheCurrent,
    closed: bool,
    busy: bool,
    settling: bool,
    next_at: i64,
    last_error: Option<String>,
    last_wall_ms: Option<f64>,
    verified_at: Option<String>,
}
/// openNativeDipCacheHost({runtimeDir, universe, signal, canOperate, now})
pub struct NativeDipCacheHost {
    runtime_dir: PathBuf,
    dir: PathBuf,
    active: V,
    binding: V,
    declaration: V,
    universe: V,
    active_file: PathBuf,
    binding_file: PathBuf,
    declaration_file: PathBuf,
    cache_file: PathBuf,
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
    inner: Mutex<CacheInner>,
}
impl NativeDipCacheHost {
    fn interval_ms(&self) -> i64 { js_int(&self.active["intervalMs"]).unwrap_or(0) }
    fn permitted(&self) -> bool {
        !self.inner.lock().unwrap().closed && !aborted(self.signal.as_ref()) && (self.can_operate)()
    }
    fn now(&self) -> i64 { (self.now)() }
    fn output_path(&self, name: &V) -> Result<PathBuf, String> {
        need(name.as_str().is_some_and(|s| s == "seed.json"
            || (s.len() == 41 && s.ends_with(".json")
                && s[..36].bytes().all(|b| (b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) || b == b'-'))), "OUTPUT_PATH")?;
        Ok(self.dir.join(name.as_str().unwrap()))
    }
    fn validate_binding(&self) -> Result<(), String> {
        need(sha_file(&self.active_file)? == self.active_hash
            && sha_file(&self.binding_file)? == self.active["bindingHash"].as_str().unwrap_or("")
            && sha_file(&self.declaration_file)? == self.binding["declarationHash"].as_str().unwrap_or("")
            && sha_file(&self.exe)? == self.binding["binaryHash"].as_str().unwrap_or("")
            && sha_file(&self.library)? == self.binding["runtimeLibraryHash"].as_str().unwrap_or("")
            && sha_file(&self.cache_file)? == self.binding["cache"]["cacheHash"].as_str().unwrap_or(""), "IDENTITY_CHANGED")
    }
    fn restore(&self, pointer: &V, is_seed: bool) -> Result<CacheCurrent, String> {
        let file = self.output_path(&pointer["output"])?;
        need(js_eq(&pointer["activeHash"], &json!(self.active_hash))
            && sha_file(&file)? == pointer["outputHash"].as_str().unwrap_or(""), "OUTPUT_HASH")?;
        let result = read_json(&file)?;
        let receipt = read_json(&PathBuf::from(format!("{}.receipt.json", file.display())))?;
        let engine_hash = if is_seed { &self.binding["modelOriginalEngineHash"] } else { &self.binding["engineHash"] };
        let binary_hash = if is_seed { &self.active["seed"]["binaryHash"] } else { &self.binding["binaryHash"] };
        need(js_eq(&receipt["binaryHash"], binary_hash)
            && js_eq(&receipt["engineHash"], engine_hash)
            && js_eq(&receipt["declarationFileHash"], &self.binding["declarationHash"])
            && js_eq(&receipt["outputHash"], &pointer["outputHash"])
            && receipt["competitionOrders"].as_i64() == Some(0)
            && js_eq(&receipt["sourceHash"], &result["capture"]["source_hash"])
            && js_eq(&receipt["sourceBytes"], &result["capture"]["source_bytes"]), "RECEIPT")?;
        if !is_seed {
            need(js_truthy_v(&pointer["prior"]) && js_eq(&receipt["cacheHash"], &self.binding["cache"]["cacheHash"]), "RECEIPT_CACHE")?;
            verify_cache_proof(&result, &self.binding, &pointer["prior"])?;
        }
        let mut b = self.binding.clone();
        b["engineHash"] = engine_hash.clone();
        b["binaryHash"] = binary_hash.clone();
        let projection = crate::native_dip_proof_v1::verify_native_dip_result(&result, &self.declaration, &b, &self.universe, None)?;
        Ok(CacheCurrent { result, projection })
    }
    pub fn status(&self) -> V {
        let i = self.inner.lock().unwrap();
        let mut s = i.current.projection.as_object().cloned().unwrap_or_default();
        s.insert("activeHash".into(), json!(self.active_hash));
        s.insert("hostRevision".into(), json!(2));
        s.insert("status".into(), json!(if i.closed { "stopped" } else if i.last_error.is_some() { "degraded" } else if i.busy { "replaying" } else { "running" }));
        s.insert("replayInProgress".into(), json!(i.busy));
        s.insert("verifiedAt".into(), i.verified_at.as_ref().map(|x| json!(x)).unwrap_or(V::Null));
        s.insert("lastError".into(), i.last_error.as_ref().map(|x| json!(x)).unwrap_or(V::Null));
        s.insert("lastWallMs".into(), i.last_wall_ms.map(|x| json!(x)).unwrap_or(V::Null));
        let t = baal_service_runtime::contract::parse_iso_ms(i.current.projection["through"].as_str().unwrap_or(""));
        let lag = t.map(|x| (self.now() as f64 - x).max(0.0));
        s.insert("sourceLagMs".into(), lag.map(|x| json!(x)).unwrap_or(V::Null));
        s.insert("riskSourceStale".into(), json!(t.is_some_and(|x| self.now() as f64 - x > self.interval_ms() as f64 * 2.0)));
        s.insert("heldRiskRetained".into(), json!(true));
        s.insert("cacheHash".into(), self.binding["cache"]["cacheHash"].clone());
        s.insert("cacheBytes".into(), self.binding["cache"]["cacheBytes"].clone());
        s.insert("originalPrefixAlwaysRehashed".into(), json!(true));
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
                i.current.projection["sourceBytes"].clone(),
                i.current.projection["sourceHash"].clone())
        };
        need(prefix.bytes as f64 >= jnum(&prior_bytes), "SOURCE_REGRESSION")?;
        need(self.permitted(), "STOPPED")?;
        let output = format!("{}.json", uuid_v4());
        let file = self.output_path(&json!(output))?;
        let prior = json!({"sourceBytes": prior_bytes, "sourceHash": prior_hash});
        run_native_dip_child(&self.exe, &[
            "evaluate-cached".into(), "--journal".into(), self.journal.display().to_string(),
            "--declaration".into(), self.declaration_file.display().to_string(),
            "--end-byte".into(), prefix.bytes.to_string(),
            "--workers".into(), sarg(&self.active["workers"]),
            "--out".into(), file.display().to_string(),
            "--cache".into(), self.cache_file.display().to_string(),
            "--cache-sha".into(), jstr(&self.binding["cache"]["cacheHash"]),
            "--cache-source-sha".into(), jstr(&self.binding["cache"]["sourceHash"]),
            "--cache-end-byte".into(), sarg(&self.binding["cache"]["sourceBytes"]),
            "--prior-end-byte".into(), sarg(&prior["sourceBytes"]),
            "--prior-sha".into(), jstr(&prior["sourceHash"])], Some(&self.control))?;
        need(self.permitted(), "STOPPED")?;
        self.validate_binding()?;
        let stat = std::fs::metadata(&self.journal).map_err(|e| e.to_string())?;
        let (dev, ino) = file_dev_ino(&stat);
        need(dev == prefix.dev && ino == prefix.ino && stat.len() >= prefix.bytes, "SOURCE_REPLACED")?;
        let bytes = std::fs::read(&file).map_err(|e| e.to_string())?;
        need(bytes.len() <= 32 * 1024 * 1024, "OUTPUT_SIZE")?;
        let result: V = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        need(js_eq(&result["capture"]["source_bytes"], &json!(prefix.bytes)), "PREFIX_BINDING")?;
        verify_cache_proof(&result, &self.binding, &prior)?;
        let projection = crate::native_dip_proof_v1::verify_native_dip_result(&result, &self.declaration, &self.binding, &self.universe, Some(&prior_result))?;
        let output_hash = sha256(&bytes);
        let at = ms_to_iso(self.now() as f64);
        let receipt = json!({"at": at, "sourceHash": projection["sourceHash"], "sourceBytes": prefix.bytes,
            "engineHash": self.binding["engineHash"], "declarationFileHash": self.binding["declarationHash"],
            "binaryHash": self.binding["binaryHash"], "runtimeLibraryHash": self.binding["runtimeLibraryHash"],
            "cacheHash": self.binding["cache"]["cacheHash"], "outputHash": output_hash,
            "networkRequests": 0, "competitionOrders": 0});
        write_exclusive_600(&PathBuf::from(format!("{}.receipt.json", file.display())),
            format!("{}\n", serde_json::to_string_pretty(&receipt).map_err(|e| e.to_string())?).as_bytes())?;
        need(self.permitted(), "STOPPED")?;
        atomic_json_canonical(&self.pointer_file, &json!({"schemaVersion": 1, "at": at,
            "activeHash": self.active_hash, "output": output, "outputHash": output_hash, "prior": prior}))?;
        let wall = started.elapsed().as_secs_f64() * 1000.0;
        let mut i = self.inner.lock().unwrap();
        i.current = CacheCurrent { result, projection };
        i.verified_at = Some(at);
        i.last_error = None;
        i.last_wall_ms = Some(wall);
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
                i.last_error = Some(if is_native_dip_code(&e) { e } else { "NATIVE_DIP_CACHE_UNAVAILABLE".into() });
            }
        }
        if self.publish().is_err() {
            self.inner.lock().unwrap().last_error = Some("NATIVE_DIP_CACHE_STATUS_UNAVAILABLE".into());
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
impl Drop for NativeDipCacheHost {
    fn drop(&mut self) { self.control.store(true, Ordering::SeqCst); }
}
fn js_truthy_v(v: &V) -> bool { baal_service_runtime::json_text::js_truthy(Some(v)) }
/// openNativeDipCacheHost({runtimeDir, universe, signal, canOperate, now})
pub fn open_native_dip_cache_host(runtime_dir: &Path, workspace: &Path, universe: &V,
    signal: Option<AbortSignal>,
    can_operate: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    now: Option<Arc<dyn Fn() -> i64 + Send + Sync>>) -> Result<Option<NativeDipCacheHost>, String> {
    let active_file = runtime_dir.join("native-dip-cache-active.json");
    if !active_file.exists() { return Ok(None); }
    let active_bytes = std::fs::read(&active_file).map_err(|e| e.to_string())?;
    let active: V = serde_json::from_slice(&active_bytes).map_err(|e| e.to_string())?;
    let active_hash = sha256(&active_bytes);
    need(js_int(&active["schemaVersion"]) == Some(1)
        && active["mode"].as_str() == Some("frozen_native_dip_cache_host_v2")
        && safe_name(&active["cohort"])
        && uuid_str(&active["cacheDirectory"])
        && js_int(&active["intervalMs"]).is_some_and(|i| (30000..=900000).contains(&i))
        && js_int(&active["workers"]).is_some_and(|w| (1..=32).contains(&w)), "ACTIVE")?;
    let dir = runtime_dir.join("native-capture-caches").join(active["cacheDirectory"].as_str().unwrap());
    let binding_file = dir.join("binding.json");
    let binding = read_json(&binding_file)?;
    let declaration_file = runtime_dir.join("research-cohorts").join(active["cohort"].as_str().unwrap()).join("declaration.json");
    let declaration = read_json(&declaration_file)?;
    let cache_file = dir.join("capture.json");
    need(sha_file(&binding_file)? == active["bindingHash"].as_str().unwrap_or("")
        && sha_file(&declaration_file)? == binding["declarationHash"].as_str().unwrap_or("")
        && js_eq(&binding["cohort"], &active["cohort"])
        && binding["modelSourcesUnchanged"] == V::Bool(true)
        && js_eq(&binding["cache"]["directory"], &active["cacheDirectory"])
        && binding["cache"]["file"].as_str() == Some("capture.json"), "FROZEN")?;
    need(payload_hash(&native_dip_cache_host_identity(workspace)?).ok() == payload_hash(&active["hostSourceHashes"]).ok(), "HOST_IDENTITY")?;
    for (name, expected) in object_entries(&binding["sourceHashes"])? {
        need(name.starts_with("backend-rust/") && !name.contains(".."), "SOURCE_SCOPE")?;
        need(sha_file(&runtime_dir.join("native-sources")
                .join(binding["engineHash"].as_str().unwrap_or("")).join(&name[13..]))?
            == expected.as_str().unwrap_or(""), "ENGINE_SOURCE")?;
    }
    let build = runtime_dir.join("native-builds").join(binding["binaryHash"].as_str().unwrap_or(""));
    let host = NativeDipCacheHost {
        runtime_dir: runtime_dir.to_path_buf(),
        dir: dir.clone(),
        active: active.clone(),
        binding: binding.clone(),
        declaration,
        universe: universe.clone(),
        active_file: active_file.clone(),
        binding_file,
        declaration_file,
        cache_file,
        exe: build.join("baal-backend.exe"),
        library: build.join("libunwind.dll"),
        journal: runtime_dir.join("journal.jsonl"),
        status_file: runtime_dir.join("native-dip-cache-status.json"),
        pointer_file: dir.join("host-latest.json"),
        active_hash,
        signal,
        control: Arc::new(AtomicBool::new(false)),
        can_operate: can_operate.unwrap_or_else(|| Arc::new(|| true)),
        now: now.unwrap_or_else(|| Arc::new(|| std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64)),
        inner: Mutex::new(CacheInner { current: CacheCurrent { result: V::Null, projection: V::Null },
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
    host.validate_binding()?;
    let current = if host.pointer_file.exists() {
        host.restore(&read_json(&host.pointer_file)?, false)?
    } else {
        let mut p = host.active["seed"].as_object().cloned().unwrap_or_default();
        p.insert("activeHash".into(), json!(host.active_hash));
        host.restore(&V::Object(p), true)?
    };
    host.inner.lock().unwrap().current = current;
    host.publish()?;
    Ok(Some(host))
}
