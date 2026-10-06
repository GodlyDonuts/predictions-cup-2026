// live-paper/native-account-queue.mjs — migration adapter over a pinned local
// admission child (one child belongs to the existing account owner; it is not a
// second HTTP client). Synchronous port: #tail promise serialization becomes a
// Mutex, async sleeps become blocking sleeps, AbortSignal.any becomes a
// combined AtomicBool updated by a watchdog thread.
use serde_json::{json, Value as V};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use baal_execution::core::canonical_payload;
use baal_service_runtime::json_text::{js_int, js_stringify, sha256 as hash};
use crate::native_dip_host_v1::{aborted, uuid_v4, AbortSignal};
use crate::native_journal_append::{PipeChild, PipeEvent};

/// queueError(code, unknown): message 'NATIVE_ACCOUNT_QUEUE_'+code; .code/.status
/// preserved so `error.code ?? 'X'` and `error.status ?? 0` port faithfully.
#[derive(Debug, Clone)]
pub struct QueueError {
    pub message: String,
    pub code: Option<String>,
    pub may_have_executed: bool,
    pub reconciliation_required: bool,
    pub status: Option<i64>,
}
impl QueueError {
    fn new(code: &str, unknown: bool) -> Self {
        QueueError { message: format!("NATIVE_ACCOUNT_QUEUE_{}", code),
            code: Some(code.to_string()), may_have_executed: unknown,
            reconciliation_required: unknown, status: None }
    }
    /// A non-queue error thrown by a caller callback or the runtime.
    pub fn other(message: impl Into<String>) -> Self {
        QueueError { message: message.into(), code: None,
            may_have_executed: false, reconciliation_required: false, status: None }
    }
    pub fn with_status(mut self, status: i64) -> Self { self.status = Some(status); self }
    fn code_or(&self, fallback: &str) -> String {
        self.code.clone().unwrap_or_else(|| fallback.to_string())
    }
}
impl fmt::Display for QueueError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { write!(f, "{}", self.message) }
}
impl std::error::Error for QueueError {}
type Q<T> = Result<T, QueueError>;
fn need(ok: bool, code: &str) -> Q<()> {
    if !ok { return Err(QueueError::new(code, false)); }
    Ok(())
}

/// stable(value) — JSON.stringify with sorted keys, recursively.
fn stable(value: &V) -> String { canonical_payload(value).unwrap_or_else(|_| js_stringify(value)) }

fn identity(v: &V) -> bool {
    v.as_str().map(|s| !s.is_empty() && s.len() <= 160
        && s.bytes().all(|b| b.is_ascii_alphanumeric()
            || matches!(b, b'.' | b'_' | b':' | b'-'))).unwrap_or(false)
}
fn hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

// A trusted local deployment supplies immutable binary hashes. This module never
// reads credentials, opens HTTP sockets or enables an execution activation.
/// verifyNativeAccountQueueBinding(binding) -> executable
pub fn verify_native_account_queue_binding(binding: &V) -> Q<PathBuf> {
    need(binding.is_object()
        && binding.as_object().unwrap().keys()
            .all(|k| ["executable", "binaryHash", "dependencies"].contains(&k.as_str()))
        && binding["executable"].as_str().map(|s| Path::new(s).is_absolute()).unwrap_or(false)
        && hex64(binding["binaryHash"].as_str().unwrap_or("")), "BINARY_BINDING")?;
    let dependencies = binding.get("dependencies").cloned().unwrap_or(V::Null);
    let deps = match &dependencies {
        V::Null => Vec::new(),
        V::Array(a) => a.clone(),
        _ => return Err(QueueError::new("DEPENDENCY_BINDING", false)),
    };
    need(deps.len() <= 4, "DEPENDENCY_BINDING")?;
    let mut items = vec![json!({"path": binding["executable"], "sha256": binding["binaryHash"]})];
    items.extend(deps);
    for item in items {
        need(item.is_object()
            && item.as_object().unwrap().keys().all(|k| ["path", "sha256"].contains(&k.as_str()))
            && item["path"].as_str().map(|s| Path::new(s).is_absolute()).unwrap_or(false)
            && hex64(item["sha256"].as_str().unwrap_or("")), "DEPENDENCY_BINDING")?;
        let p = item["path"].as_str().unwrap();
        let stat = std::fs::symlink_metadata(p);
        let bytes = std::fs::read(p);
        let (stat, bytes) = match (stat, bytes) {
            (Ok(s), Ok(b)) => (s, b),
            _ => return Err(QueueError::new("BINARY_UNAVAILABLE", false)),
        };
        need(stat.is_file() && !stat.is_symlink()
            && hash(&bytes) == item["sha256"].as_str().unwrap_or(""), "BINARY_HASH_MISMATCH")?;
    }
    Ok(PathBuf::from(binding["executable"].as_str().unwrap()))
}

/// nativeQueueIntentBinding(scope, spec) -> frozen {intent,key,payload_hash}.
pub fn native_queue_intent_binding(scope: &str, spec: &V) -> Q<V> {
    need(identity(&json!(scope)) && identity(&spec["runId"])
        && identity(&spec["intentId"]) && identity(&spec["idempotencyKey"]), "INTENT_IDENTITY")?;
    need(["POST", "DELETE"].contains(&spec["method"].as_str().unwrap_or(""))
        && spec["endpoint"].as_str().map(|e| e.starts_with('/')
            && !e.contains(['\r', '\n'])).unwrap_or(false), "INTENT_OPERATION")?;
    let body_json = if !spec["bodyJson"].is_null() {
        spec["bodyJson"].as_str().map(|s| s.to_string())
            .ok_or_else(|| QueueError::new("INTENT_BODY", false))?
    } else if spec.get("body").is_none() {
        String::new()
    } else {
        js_stringify(&spec["body"])
    };
    need(body_json.len() <= 65536, "INTENT_BODY")?;
    if spec.get("body").is_some() {
        let decoded: V = serde_json::from_str(&body_json)
            .map_err(|_| QueueError::new("INTENT_BODY", false))?;
        need(stable(&decoded) == stable(&spec["body"]), "BODY_ENCODING_MISMATCH")?;
    }
    if spec["method"].as_str() == Some("POST") {
        need(spec["body"]["idempotencyKey"] == spec["idempotencyKey"], "IDEMPOTENCY_KEY_MISMATCH")?;
    }
    Ok(json!({
        "intent": format!("{}:{}", spec["runId"].as_str().unwrap_or(""),
            spec["intentId"].as_str().unwrap_or("")),
        "key": spec["idempotencyKey"],
        "payload_hash": hash(stable(&json!({"scope": scope, "method": spec["method"],
            "endpoint": spec["endpoint"], "bodyJson": body_json})).as_bytes()),
    }))
}

/// Trusted constructor callbacks. Validators may be repeated and must be
/// idempotent. Result/error classifiers are trusted: a write success without
/// one remains unknown.
#[derive(Default)]
pub struct AccountQueueCallbacks {
    pub can_operate: Option<Box<dyn Fn() -> Q<bool> + Send + Sync>>,
    pub can_write: Option<Box<dyn Fn() -> Q<bool> + Send + Sync>>,
    pub validate_lease: Option<Box<dyn Fn(&V, &V) -> Q<bool> + Send + Sync>>,
    pub validate_permit: Option<Box<dyn Fn(&V, &V) -> Q<bool> + Send + Sync>>,
    pub validate_network_permit: Option<Box<dyn Fn(&V, &V) -> Q<bool> + Send + Sync>>,
    pub validate_reconciliation: Option<Box<dyn Fn(&V, &V) -> Q<bool> + Send + Sync>>,
    pub classify_result: Option<Box<dyn Fn(&V, &V) -> Q<V> + Send + Sync>>,
    pub classify_error: Option<Box<dyn Fn(&QueueError, &V) -> Q<V> + Send + Sync>>,
}
pub type QueuePerform = dyn Fn(&V, &AbortSignal) -> Q<V> + Send + Sync;

/// The resolved #exit promise value: {code, termination}.
#[derive(Debug, Clone)]
pub struct QueueExit {
    pub code: Option<i32>,
    pub termination: String,
}

/// new NativeAccountQueue({binding, directory, scope, callbacks, now, sleep,
/// signal, rpcTimeoutMs, operationTimeoutMs})
pub struct NativeAccountQueue {
    binding: V,
    scope: String,
    callbacks: AccountQueueCallbacks,
    now: Box<dyn Fn() -> i64 + Send + Sync>,
    sleep: Box<dyn Fn(i64) + Send + Sync>,
    signal: Option<AbortSignal>,
    timeout: Duration,
    operation_timeout: Duration,
    next_slot: f64,
    network_write: bool,
    controller: AbortSignal,
    poison: Option<QueueError>,
    closing: bool,
    closed: bool,
    pending: bool,
    stderr_bytes: usize,
    child: PipeChild,
    exit: Option<QueueExit>,
    tail: Arc<Mutex<()>>,
}

fn clock_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64).unwrap_or(0)
}
fn thread_sleep(ms: i64) {
    if ms > 0 { std::thread::sleep(Duration::from_millis(ms as u64)); }
}

impl NativeAccountQueue {
    pub fn new(binding: &V, directory: &str, scope: &str,
        callbacks: AccountQueueCallbacks, signal: Option<AbortSignal>,
        rpc_timeout_ms: i64, operation_timeout_ms: i64) -> Q<Self> {
        let now: Box<dyn Fn() -> i64 + Send + Sync> = Box::new(clock_now);
        Self::with_clock(binding, directory, scope, callbacks, signal,
            rpc_timeout_ms, operation_timeout_ms, now, Box::new(thread_sleep))
    }
    #[allow(clippy::too_many_arguments)]
    pub fn with_clock(binding: &V, directory: &str, scope: &str,
        callbacks: AccountQueueCallbacks, signal: Option<AbortSignal>,
        rpc_timeout_ms: i64, operation_timeout_ms: i64,
        now: Box<dyn Fn() -> i64 + Send + Sync>,
        sleep: Box<dyn Fn(i64) + Send + Sync>) -> Q<Self> {
        need(!directory.is_empty() && Path::new(directory).is_absolute()
            && identity(&json!(scope))
            && callbacks.can_operate.is_some()
            && (100..=120000).contains(&rpc_timeout_ms)
            && (100..=120000).contains(&operation_timeout_ms), "ADAPTER_CONFIGURATION")?;
        let binding = binding.clone();
        let executable = verify_native_account_queue_binding(&binding)?;
        let child = PipeChild::spawn(&executable,
            &["--offline-jsonl".to_string(), directory.to_string(),
                scope.to_string(), uuid_v4()], None, 16 * 1024 * 1024)
            .map_err(|_| QueueError::new("PROCESS_UNAVAILABLE", false))?;
        let mut q = NativeAccountQueue {
            binding, scope: scope.to_string(), callbacks, signal,
            timeout: Duration::from_millis(rpc_timeout_ms as u64),
            operation_timeout: Duration::from_millis(operation_timeout_ms as u64),
            now, sleep, next_slot: 0.0, network_write: false,
            controller: Arc::new(AtomicBool::new(false)),
            poison: None, closing: false, closed: false, pending: false,
            stderr_bytes: 0, child, exit: None, tail: Arc::new(Mutex::new(())),
        };
        if !q.child.alive() { q.fail("PROCESS_LOST"); }
        Ok(q)
    }

    /// get status
    pub fn status(&self) -> V {
        json!({"mode": "pinned_native_account_admission_adapter",
            "closed": self.closed, "failed": self.poison.is_some(),
            "executionAuthority": false, "networkRequests": 0})
    }
    fn fail(&mut self, code: &str) -> QueueError {
        if let Some(e) = &self.poison { return e.clone(); }
        let e = QueueError::new(code, self.network_write);
        self.poison = Some(e.clone());
        self.controller.store(true, Ordering::SeqCst);
        self.child.kill();
        e
    }
    fn handle_pipe_event(&mut self, ev: PipeEvent) -> Option<QueueError> {
        match ev {
            PipeEvent::Stderr(n) => {
                self.stderr_bytes += n;
                if self.stderr_bytes > 2048 { Some(self.fail("PROCESS_DIAGNOSTIC_OVERFLOW")) }
                else { None }
            }
            PipeEvent::Overflow => Some(self.fail("RESPONSE_TOO_LARGE")),
            PipeEvent::Eof => {
                let fail_err = if !self.closing || self.pending {
                    Some(self.fail("PROCESS_LOST"))
                } else { None };
                self.closed = true;
                self.exit = Some(QueueExit { code: None, termination: "close".into() });
                fail_err
            }
            PipeEvent::Line(_) => Some(self.fail("UNEXPECTED_RESPONSE")),
        }
    }
    /// #rpc(command)
    fn rpc(&mut self, command: &V) -> Q<V> {
        if let Some(e) = &self.poison { return Err(e.clone()); }
        need(!self.closed && !self.pending, "PROCESS_NOT_READY")?;
        verify_native_account_queue_binding(&self.binding)?;
        let now = self.clock()?;
        while let Some(ev) = self.child.drain_event() {
            if let Some(e) = self.handle_pipe_event(ev) { return Err(e); }
        }
        let mut payload = serde_json::Map::new();
        payload.insert("scope".to_string(), json!(self.scope));
        payload.insert("now".to_string(), json!(now));
        if let Some(o) = command.as_object() {
            for (k, v) in o { payload.insert(k.clone(), v.clone()); }
        }
        if self.child.write_line(&js_stringify(&V::Object(payload))).is_err() {
            return Err(self.fail("PROCESS_INPUT_LOST"));
        }
        self.pending = true;
        let out = (|| -> Q<V> {
            loop {
                let ev = match self.child.recv(self.timeout) {
                    Ok(e) => e,
                    Err(()) => return Err(self.fail("RESPONSE_TIMEOUT")),
                };
                match ev {
                    PipeEvent::Line(line) => {
                        let response: V = serde_json::from_str(&line)
                            .map_err(|_| self.fail("INVALID_RESPONSE"))?;
                        let ok = response["ok"].as_bool();
                        let valid = ok.is_some()
                            && (!ok.unwrap()
                                || (response.get("result").map(|r| r.is_object()).unwrap_or(false)))
                            && (ok.unwrap()
                                || response["error"].as_str().map(|e| !e.is_empty()
                                    && e.len() <= 90 && e.bytes()
                                        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_'))
                                    .unwrap_or(false));
                        if !valid { return Err(self.fail("INVALID_RESPONSE")); }
                        if ok == Some(true) {
                            return Ok(response["result"].clone());
                        }
                        return Err(QueueError::new(
                            response["error"].as_str().unwrap_or(""), self.network_write));
                    }
                    other => {
                        if let Some(e) = self.handle_pipe_event(other) { return Err(e); }
                    }
                }
            }
        })();
        self.pending = false;
        out
    }
    /// #clock()
    fn clock(&self) -> Q<i64> {
        let now = (self.now)();
        need(now >= 0 && now <= 8640000000000000, "CLOCK")?;
        Ok(now)
    }
    /// #guard(write)
    fn guard(&mut self, write: bool) -> Q<()> {
        if let Some(e) = &self.poison { return Err(e.clone()); }
        need(!aborted(self.signal.as_ref()) && !self.controller.load(Ordering::SeqCst)
            && (self.callbacks.can_operate.as_ref()
                .map(|f| f()).transpose()?.unwrap_or(false)), "STOPPED")?;
        if write {
            need(self.callbacks.can_write.as_ref().map(|f| f()).transpose()?.unwrap_or(false),
                "WRITE_STOPPED_OR_OWNER_LOST")?;
            need(self.callbacks.can_operate.as_ref().map(|f| f()).transpose()?.unwrap_or(false),
                "STOPPED")?;
        }
        need(!aborted(self.signal.as_ref()) && !self.controller.load(Ordering::SeqCst), "STOPPED")?;
        Ok(())
    }
    /// #wait(until, write)
    fn wait(&mut self, until: f64, write: bool) -> Q<()> {
        while (self.clock()? as f64) < until {
            self.guard(write)?;
            (self.sleep)((200.0f64).min(until - self.clock()? as f64) as i64);
        }
        self.guard(write)
    }
    /// #fences(spec, authority, network)
    fn fences(&mut self, spec: &V, authority: &V, network: bool) -> Q<V> {
        self.guard(true)?;
        need(self.callbacks.validate_lease.as_ref()
            .map(|f| f(&authority["lease"], spec)).transpose()?.unwrap_or(false),
            "INVALID_EXECUTION_LEASE")?;
        let validate = if network {
            &self.callbacks.validate_network_permit
        } else {
            &self.callbacks.validate_permit
        };
        need(validate.as_ref()
            .map(|f| f(&authority["permit"], spec)).transpose()?.unwrap_or(false),
            if network { "INVALID_NETWORK_EXECUTION_PERMIT" } else { "INVALID_EXECUTION_PERMIT" })?;
        self.guard(true)?;
        Ok(json!({"authorized": true, "lease_valid": true, "permit_valid": true}))
    }
    /// #perform(perform, spec) — abort composition over a watchdog flag.
    fn perform_op(&mut self, perform: &QueuePerform, spec: &V) -> Q<V> {
        let combined: AbortSignal = Arc::new(AtomicBool::new(false));
        {
            let combined = combined.clone();
            let controller = self.controller.clone();
            let user = self.signal.clone();
            let deadline = Instant::now() + self.operation_timeout;
            std::thread::spawn(move || loop {
                if combined.load(Ordering::SeqCst) || controller.load(Ordering::SeqCst)
                    || Instant::now() >= deadline
                    || aborted(user.as_ref()) {
                    combined.store(true, Ordering::SeqCst);
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            });
        }
        if combined.load(Ordering::SeqCst)
            || self.controller.load(Ordering::SeqCst)
            || aborted(self.signal.as_ref()) {
            return Err(self.poison.clone().unwrap_or_else(||
                QueueError::new("STOPPED", spec["method"].as_str() != Some("GET"))));
        }
        let started = Instant::now();
        let out = perform(spec, &combined);
        if combined.load(Ordering::SeqCst) && out.is_ok() {
            return Err(self.poison.clone().unwrap_or_else(|| {
                if started.elapsed() >= self.operation_timeout {
                    QueueError::new("OPERATION_TIMEOUT", spec["method"].as_str() != Some("GET"))
                } else {
                    QueueError::new("STOPPED", spec["method"].as_str() != Some("GET"))
                }
            }));
        }
        out
    }
    /// #run(spec, authority, perform)
    #[allow(clippy::too_many_arguments)]
    fn run(&mut self, spec: &V, authority: Option<&V>, perform: &QueuePerform,
        before_dispatch: Option<&(dyn Fn() -> Q<()> + Send + Sync)>,
        before_network: Option<&(dyn Fn() -> Q<()> + Send + Sync)>) -> Q<V> {
        let write = spec["method"].as_str() != Some("GET");
        let binding = if write {
            Some(native_queue_intent_binding(&self.scope, spec)?)
        } else { None };
        let request = uuid_v4();
        let mut admitted = false;
        let mut network = false;
        let mut performed = false;
        let mut result = V::Null;
        let mut operation_error: Option<QueueError> = None;
        let attempt = (|| -> Q<()> {
            self.wait(self.next_slot, write)?;
            let mut flags = if write {
                self.fences(spec, authority.unwrap_or(&V::Null), false)?
            } else { V::Null };
            if let Some(f) = before_dispatch { f()?; }
            self.guard(write)?;
            loop {
                let mut cmd = serde_json::Map::new();
                cmd.insert("op".to_string(), json!("admit"));
                cmd.insert("request".to_string(), json!(request));
                cmd.insert("write".to_string(), json!(write));
                if let Some(b) = &binding {
                    cmd.insert("binding".to_string(), b.clone());
                    if let Some(o) = flags.as_object() {
                        for (k, v) in o { cmd.insert(k.clone(), v.clone()); }
                    }
                }
                let response = self.rpc(&V::Object(cmd))?;
                need(response["admitted"].as_bool().is_some(), "ADMISSION_RESPONSE")?;
                if response["admitted"].as_bool() == Some(true) {
                    admitted = true;
                    self.next_slot = response["next"].as_f64().unwrap_or(0.0);
                    break;
                }
                let now = self.clock()?;
                need(js_int(&response["wait_until"]).map(|w| w >= now).unwrap_or(false),
                    "WAIT_RESPONSE")?;
                self.wait(response["wait_until"].as_f64().unwrap_or(0.0), write)?;
                flags = if write {
                    self.fences(spec, authority.unwrap_or(&V::Null), false)?
                } else { V::Null };
            }
            if let Some(f) = before_network { f()?; }
            let network_flags = if write {
                self.fences(spec, authority.unwrap_or(&V::Null), true)?
            } else { V::Null };
            if write { self.network_write = true; }
            let mut cmd = serde_json::Map::new();
            cmd.insert("op".to_string(), json!("network"));
            cmd.insert("request".to_string(), json!(request));
            cmd.insert("write".to_string(), json!(write));
            if let Some(b) = &binding {
                cmd.insert("binding".to_string(), b.clone());
                if let Some(o) = network_flags.as_object() {
                    for (k, v) in o { cmd.insert(k.clone(), v.clone()); }
                }
            }
            let allowed = self.rpc(&V::Object(cmd))?;
            need(allowed["network_permitted"].as_bool() == Some(true), "NETWORK_RESPONSE")?;
            network = true;
            self.guard(write)?;
            performed = true;
            result = self.perform_op(perform, spec)?;
            Ok(())
        })();
        if let Err(e) = attempt { operation_error = Some(e); }
        if admitted && self.poison.is_none() {
            let completion = (|| -> Q<()> {
                let classification: V = if !network && !self.network_write {
                    json!({"outcome": "not_sent"})
                } else if !performed {
                    json!({"outcome": if write { "unknown" } else { "rejected" }})
                } else if let Some(e) = &operation_error {
                    match &self.callbacks.classify_error {
                        Some(f) => f(e, spec)?,
                        None => json!({"outcome": if write { "unknown" } else { "rejected" },
                            "status": e.status.unwrap_or(0)}),
                    }
                } else {
                    match &self.callbacks.classify_result {
                        Some(f) => f(&result, spec)?,
                        None => json!({"outcome": if write { "unknown" } else { "success" }}),
                    }
                };
                let outcome = classification["outcome"].as_str().unwrap_or("");
                need(["success", "rejected", "unknown", "not_sent"].contains(&outcome)
                    && (!network || outcome != "not_sent"), "OUTCOME_CLASSIFICATION")?;
                let status = classification["status"].as_i64().unwrap_or(0);
                need(status >= 0 && status <= 599
                    && js_int(&json!(status)).is_some(), "OUTCOME_STATUS")?;
                let complete = self.rpc(&json!({"op": "complete", "request": request,
                    "outcome": outcome, "status": status,
                    "retry_after": classification["retryAfter"].as_str().unwrap_or(""),
                    "api_code": classification["apiCode"].as_str().unwrap_or("")}))?;
                self.next_slot = complete["next"].as_f64().unwrap_or(0.0);
                if write && outcome == "unknown" {
                    operation_error = Some(QueueError::new("UNKNOWN_ACCEPTANCE_RECONCILE_FIRST", true));
                } else if outcome == "rejected" && operation_error.is_none() {
                    operation_error = Some(QueueError::new("OPERATION_REJECTED", false)
                        .with_status(status));
                }
                Ok(())
            })();
            if let Err(e) = completion {
                self.fail("COMPLETION_DURABILITY_UNCERTAIN");
                operation_error = Some(QueueError::new(
                    &e.code_or("COMPLETION_DURABILITY_UNCERTAIN"), write && network));
            }
        }
        self.network_write = false;
        if let Some(e) = &self.poison { return Err(e.clone()); }
        if let Some(e) = operation_error { return Err(e); }
        Ok(result)
    }
    /// read({endpoint, perform, beforeNetwork})
    pub fn read(&mut self, endpoint: &str, perform: &QueuePerform,
        before_network: Option<&(dyn Fn() -> Q<()> + Send + Sync)>) -> Q<V> {
        need(endpoint.starts_with('/'), "READ_SPEC")?;
        let _tail = self.tail.clone();
        let _g = _tail.lock().unwrap_or_else(|e| e.into_inner());
        self.run(&json!({"method": "GET", "endpoint": endpoint}),
            None, perform, None, before_network)
    }
    /// write(input, authority, {perform, beforeDispatch, beforeNetwork})
    pub fn write(&mut self, input: &V, authority: &V, perform: &QueuePerform,
        before_dispatch: Option<&(dyn Fn() -> Q<()> + Send + Sync)>,
        before_network: Option<&(dyn Fn() -> Q<()> + Send + Sync)>) -> Q<V> {
        // typeof authority.permit==='object' admits null, objects and arrays;
        // authority.lease is a truthiness check.
        need(authority.is_object()
            && !matches!(authority["permit"], V::Bool(_) | V::Number(_) | V::String(_))
            && authority.get("permit").is_some()
            && baal_service_runtime::json_text::js_truthy(authority.get("lease")),
            "WRITE_SPEC")?;
        let spec = input.clone();
        let owned_authority = authority.clone();
        native_queue_intent_binding(&self.scope, &spec)?;
        let _tail = self.tail.clone();
        let _g = _tail.lock().unwrap_or_else(|e| e.into_inner());
        self.run(&spec, Some(&owned_authority), perform, before_dispatch, before_network)
    }
    /// inspect()
    pub fn inspect(&mut self) -> Q<V> {
        let _tail = self.tail.clone();
        let _g = _tail.lock().unwrap_or_else(|e| e.into_inner());
        self.guard(false)?;
        self.rpc(&json!({"op": "status"}))
    }
    /// compact()
    pub fn compact(&mut self) -> Q<V> {
        let _tail = self.tail.clone();
        let _g = _tail.lock().unwrap_or_else(|e| e.into_inner());
        self.guard(false)?;
        self.rpc(&json!({"op": "compact"}))
    }
    /// reconcile(spec, evidence)
    pub fn reconcile(&mut self, spec: &V, evidence: &V) -> Q<V> {
        let binding = native_queue_intent_binding(&self.scope, &spec.clone())?;
        let snapshot = evidence.clone();
        let _tail = self.tail.clone();
        let _g = _tail.lock().unwrap_or_else(|e| e.into_inner());
        self.guard(true)?;
        need(self.callbacks.validate_reconciliation.as_ref()
            .map(|f| f(&binding, &snapshot)).transpose()?.unwrap_or(false),
            "SCOPED_RECONCILIATION_REQUIRED")?;
        self.guard(true)?;
        self.rpc(&json!({"op": "reconcile", "binding": binding, "reconciled": true,
            "lease_valid": true, "permit_valid": true}))
    }
    /// close({force})
    pub fn close(&mut self, force: bool) -> Q<QueueExit> {
        if let Some(e) = &self.exit { return Ok(e.clone()); }
        if self.closed { return Ok(self.exit.clone().unwrap()); }
        self.closing = true;
        if force {
            self.fail("PROCESS_LOST");
        } else {
            let _tail = self.tail.clone();
        let _g = _tail.lock().unwrap_or_else(|e| e.into_inner());
            if self.poison.is_none() { drop(self.child.stdin.take()); }
        }
        let _ = self.child.child.wait();
        self.closed = true;
        let status = QueueExit {
            code: self.child.child.try_wait().ok().flatten().and_then(|s| s.code()),
            termination: String::new(),
        };
        self.exit = Some(status.clone());
        Ok(status)
    }
}
impl Drop for NativeAccountQueue {
    fn drop(&mut self) { let _ = self.close(true); }
}
