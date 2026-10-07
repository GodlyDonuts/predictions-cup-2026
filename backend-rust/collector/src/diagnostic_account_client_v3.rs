// live-paper/diagnostic-account-client-v3.mjs — the SAME serialized account
// client (CoordinatedAccountClientV2) plus a diagnostic fetch wrapper that
// records a safe, secret-free fault record for every fetch failure and
// escalates durable recording failures into a fatal error inside perform.
// "Reuses the same queue, credentials, limiter and fetch. This is not a new
// client."
//
// Port notes:
// - The JS extends CoordinatedAccountClientV2 and wraps `this.fetch` AFTER
//   the parent constructor ran, so the diagnostic wrapper sits OUTSIDE the
//   admission wrapper and also records admit() failures. The ported
//   ReadClient freezes `fetch` at construction, so the v2 constructor is
//   inlined here and the chain is installed as Diagnostic(Admitting(
//   supplied)).
// - Rust `Fetch` errors are `Err(String)` while JS throws rich objects. The
//   string is treated as the thrown value's `.code` — the encoding every
//   Rust receipts port uses for `Object.assign(Error(code), {code, ...})` —
//   and the fixed flags the JS receipts producers attach are restored on the
//   error view so the classifier sees identical fields. `.cause.code`
//   chains, `.name`, `.status` and undici codes are unrecoverable from a
//   bare string; such errors classify as UNCLASSIFIED_FETCH_FAILURE.
// - `perform` is private inside the ported ReadClient, so the lastFault
//   reset + fatal conversion wraps request/dispatch/orders under a
//   serializer — the same critical section the JS tail promise provides.
use baal_service_runtime::atomic::atomic_json_canonical;
use baal_service_runtime::client::{
    Authorizer, DispatchSpec, Fetch, FetchOutcome, FetchRequest, ReadClient, ReadClientOptions,
    ReqwestFetch, ServiceError,
};
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::js_int;
use serde_json::{json, Map, Value as V};
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const READ_CAP: usize = 60;
const WRITE_CAP: usize = 24;
const WINDOW_MS: f64 = 60000.0;

fn need(ok: bool, c: &str) -> Result<(), String> {
    if !ok {
        return Err(format!("COORDINATED_CLIENT_{}", c));
    }
    Ok(())
}

// --- safeFetchFailureV1 -----------------------------------------------------

/// Error codes the classifier recognizes as safe to persist.
const KNOWN_CODES: [&str; 20] = [
    "EIO",
    "ENOSPC",
    "EACCES",
    "EPERM",
    "ENETUNREACH",
    "EHOSTUNREACH",
    "UND_ERR_BODY_TIMEOUT",
    "ECONNRESET",
    "ECONNREFUSED",
    "ETIMEDOUT",
    "ENOTFOUND",
    "EAI_AGAIN",
    "UND_ERR_CONNECT_TIMEOUT",
    "UND_ERR_HEADERS_TIMEOUT",
    "UND_ERR_SOCKET",
    "ABORT_ERR",
    "REST_RECEIPT_START_RECORDING_FAILED",
    "REST_RECEIPT_FETCH_UNAVAILABLE",
    "REST_RECEIPT_BEFORE_FETCH_GUARD",
    "REST_RECEIPT_FINAL_FENCE_EXPIRED",
];
/// `['EIO','ENOSPC','EACCES','EPERM']` — errno codes that prove a durable
/// recording could not be written.
const DURABLE_CODES: [&str; 4] = ["EIO", "ENOSPC", "EACCES", "EPERM"];

/// safeFetchFailureV1(error): the thrown value as a JSON record —
/// `{code, name, cause:{code, errors:[{code}, ...]}, durableRecordingFailed,
/// mayHaveExecuted}` — every property read tolerantly like `error?.x`. Returns
/// `{code, name, durableRecordingFailed, mayHaveExecuted}` in JS literal
/// order; only whitelisted details survive so secrets in messages/causes
/// never reach the diagnostic file.
pub fn safe_fetch_failure_v1(error: &V) -> V {
    let get = |v: &V, k: &str| -> V { v.get(k).cloned().unwrap_or(V::Null) };
    let cause = get(error, "cause");
    let mut candidates: Vec<V> = vec![get(error, "code"), get(&cause, "code")];
    // `Array.isArray(error?.cause?.errors) ? errors.slice(0,8).map(e=>e?.code) : []`
    if let Some(errors) = cause.get("errors").and_then(V::as_array) {
        for e in errors.iter().take(8) {
            candidates.push(get(e, "code"));
        }
    }
    let code = candidates
        .iter()
        .find_map(|c| c.as_str().filter(|s| KNOWN_CODES.contains(s)))
        .unwrap_or("UNCLASSIFIED_FETCH_FAILURE");
    let error_name = get(error, "name");
    let name = match error_name.as_str() {
        Some(s @ ("AbortError" | "TimeoutError" | "TypeError")) => s,
        _ => "Error",
    };
    let durable_recording_failed = get(error, "durableRecordingFailed") == V::Bool(true)
        || get(error, "code")
            .as_str()
            .is_some_and(|c| DURABLE_CODES.contains(&c));
    let may_have_executed = get(error, "mayHaveExecuted") == V::Bool(true);
    let mut out = Map::new();
    out.insert("code".into(), json!(code));
    out.insert("name".into(), json!(name));
    out.insert("durableRecordingFailed".into(), json!(durable_recording_failed));
    out.insert("mayHaveExecuted".into(), json!(may_have_executed));
    V::Object(out)
}

/// The JSON view of a Rust `Err(String)` the classifier reads. Rust fetch
/// producers keep `error.code` in the error string, so the string is exposed
/// as `code`; `cause`/`name` read as undefined like a plain JS `Error`. The
/// JS receipts producers attach fixed flags for two codes — those are
/// restored verbatim (`mayHaveExecuted: method !== 'GET'`), because the Rust
/// ports drop them off the thrown object.
fn fetch_error_view(message: &str, method: &str) -> V {
    let mut v = Map::new();
    v.insert("code".into(), json!(message));
    match message {
        "REST_RECEIPT_START_RECORDING_FAILED" => {
            v.insert("durableRecordingFailed".into(), json!(true));
            v.insert("mayHaveExecuted".into(), json!(method != "GET"));
        }
        "REST_RECEIPT_FETCH_UNAVAILABLE" => {
            v.insert("mayHaveExecuted".into(), json!(method != "GET"));
        }
        _ => {}
    }
    V::Object(v)
}

// --- shared diagnostic state ------------------------------------------------

/// `this.lastFault` / `this.failures` — mutated by the fetch wrapper running
/// on the client's fetch worker thread, read by perform on the caller side.
#[derive(Default)]
struct DiagState {
    /// The most recent safeFetchFailureV1 record, or the
    /// DIAGNOSTIC_PERSISTENCE_FAILED replacement.
    last_fault: Option<V>,
    /// Rolling window of `{at, method, code, name, durableRecordingFailed,
    /// mayHaveExecuted}` records, capped at the last 20.
    failures: Vec<V>,
}

// --- v2 admission state, inlined --------------------------------------------
// Identical logic to coordinated_account_client_v2.rs; it is inlined (rather
// than composing CoordinatedAccountClientV2) because the JS diagnostic
// wrapper must sit OUTSIDE the admission wrapper and the ported ReadClient
// freezes `fetch` at construction.

#[derive(Default)]
struct AdmissionData {
    reads: Vec<f64>,
    writes: Vec<f64>,
}

struct Admissions {
    data: Mutex<AdmissionData>,
    file: PathBuf,
    now: Arc<dyn Fn() -> f64 + Send + Sync>,
    sleep: Arc<dyn Fn(f64) + Send + Sync>,
    signal: Arc<AtomicBool>,
}
impl Admissions {
    fn pause(&self, ms: f64) -> Result<(), String> {
        let until = (self.now)() + ms;
        while (self.now)() < until {
            if self.signal.load(Ordering::SeqCst) {
                return Err("Stopped".to_string());
            }
            (self.sleep)((until - (self.now)()).min(200.0));
        }
        Ok(())
    }
    // async admit(method): rolling window; when full, pause until the oldest
    // admission leaves the window.
    fn admit(&self, method: &str) -> Result<(), String> {
        let mut st = self.data.lock().map_err(|_| "Stopped".to_string())?;
        let maximum = if method == "GET" { READ_CAP } else { WRITE_CAP };
        loop {
            let at = (self.now)();
            {
                let list = if method == "GET" { &mut st.reads } else { &mut st.writes };
                while let Some(&first) = list.first() {
                    if at - first >= WINDOW_MS {
                        list.remove(0);
                    } else {
                        break;
                    }
                }
                if list.len() < maximum {
                    break;
                }
            }
            let wait = if method == "GET" { &st.reads } else { &st.writes }[0] + WINDOW_MS - at;
            self.pause(wait)?;
        }
        let at = (self.now)();
        if method == "GET" {
            &mut st.reads
        } else {
            &mut st.writes
        }
        .push(at);
        st.reads.retain(|x| at - *x < WINDOW_MS);
        st.writes.retain(|x| at - *x < WINDOW_MS);
        // ../execution/ledger.mjs atomicJson — canonical-bytes, fsynced.
        atomic_json_canonical(
            &self.file,
            &json!({"schemaVersion": 1, "reads": st.reads, "writes": st.writes}),
        )
    }
}

/// `this.fetch = async (url, request) => { await this.admit(...); return
/// suppliedFetch(url, request); }` — the v2 wrapper the diagnostic wrapper
/// captures as `original`.
struct AdmittingFetch {
    supplied: Arc<dyn Fetch>,
    admissions: Arc<Admissions>,
}
impl Fetch for AdmittingFetch {
    fn fetch(&self, url: &str, req: &FetchRequest) -> Result<FetchOutcome, String> {
        self.admissions.admit(&req.method.to_uppercase())?;
        self.supplied.fetch(url, req)
    }
}

/// `this.fetch = async (url, request) => { try { return await original(...) }
/// catch (error) { record safe fault; persist; throw error } }` — installed
/// around the admission wrapper exactly like the JS subclassing order.
struct DiagnosticFetch {
    inner: AdmittingFetch,
    file: PathBuf,
    /// `this.now` — the client's clock.
    now: Arc<dyn Fn() -> f64 + Send + Sync>,
    state: Arc<Mutex<DiagState>>,
}
impl Fetch for DiagnosticFetch {
    fn fetch(&self, url: &str, req: &FetchRequest) -> Result<FetchOutcome, String> {
        match self.inner.fetch(url, req) {
            Ok(o) => Ok(o),
            Err(message) => {
                let mut st = self.state.lock().map_err(|_| "Stopped".to_string())?;
                let fault = safe_fetch_failure_v1(&fetch_error_view(&message, &req.method));
                st.last_fault = Some(fault.clone());
                let at = (self.now)();
                // new Date(at).toISOString() throws RangeError on non-finite
                // or out-of-range instants; that throw escapes the wrapper
                // instead of the original error, exactly like the JS.
                if !at.is_finite() || at.abs() > 8.64e15 {
                    return Err("Invalid time value".to_string());
                }
                // {at, method, ...lastFault} — JS property order.
                let mut record = Map::new();
                record.insert("at".into(), json!(ms_to_iso(at)));
                record.insert(
                    "method".into(),
                    json!(if req.method == "GET" { "GET" } else { "WRITE" }),
                );
                if let V::Object(m) = &fault {
                    for (k, v) in m {
                        record.insert(k.clone(), v.clone());
                    }
                }
                st.failures.push(V::Object(record));
                if st.failures.len() > 20 {
                    let drop = st.failures.len() - 20;
                    st.failures.drain(..drop);
                }
                // atomicJson(this.diagnosticFile, {schemaVersion:1, failures})
                if atomic_json_canonical(
                    &self.file,
                    &json!({"schemaVersion": 1, "failures": st.failures}),
                )
                .is_err()
                {
                    let mut lf = Map::new();
                    lf.insert("code".into(), json!("DIAGNOSTIC_PERSISTENCE_FAILED"));
                    lf.insert("name".into(), json!("Error"));
                    lf.insert("durableRecordingFailed".into(), json!(true));
                    lf.insert(
                        "mayHaveExecuted".into(),
                        fault.get("mayHaveExecuted").cloned().unwrap_or(V::Bool(false)),
                    );
                    st.last_fault = Some(V::Object(lf));
                }
                Err(message)
            }
        }
    }
}

// --- error type -------------------------------------------------------------

/// Thrown-object stand-in: JS consumers read `e.durableRecordingFailed`,
/// `e.fatal`, `e.code`, `e.mayHaveExecuted` next to the ServiceError fields
/// (`e.status`, `e.apiCode`, `e.recoverable`, `e.stopped`, `e.retryAfterMs`).
#[derive(Debug, Clone)]
pub struct DiagnosticFault {
    pub message: String,
    /// `e.code` — set by the perform() fatal conversion and by guarded
    /// producers; absent on plain service errors.
    pub code: Option<String>,
    pub status: Option<u16>,
    pub api_code: Option<String>,
    pub retry_after_ms: f64,
    pub recoverable: bool,
    pub stopped: bool,
    pub durable_recording_failed: bool,
    pub fatal: bool,
    pub may_have_executed: bool,
}
impl DiagnosticFault {
    fn raw(message: impl Into<String>) -> Self {
        DiagnosticFault {
            message: message.into(),
            code: None,
            status: None,
            api_code: None,
            retry_after_ms: 0.0,
            recoverable: false,
            stopped: false,
            durable_recording_failed: false,
            fatal: false,
            may_have_executed: false,
        }
    }
    /// `Object.assign(Error(this.lastFault.code), {code, fatal:true,
    /// durableRecordingFailed:true, mayHaveExecuted})` — the perform()
    /// conversion.
    fn fatal_fault(fault: &V) -> Self {
        let code = fault.get("code").and_then(V::as_str).unwrap_or("").to_string();
        DiagnosticFault {
            message: code.clone(),
            code: Some(code),
            durable_recording_failed: true,
            fatal: true,
            may_have_executed: fault.get("mayHaveExecuted") == Some(&V::Bool(true)),
            ..DiagnosticFault::raw("")
        }
    }
}
impl From<ServiceError> for DiagnosticFault {
    fn from(e: ServiceError) -> Self {
        DiagnosticFault {
            status: e.status,
            api_code: e.api_code,
            retry_after_ms: e.retry_after_ms,
            recoverable: e.recoverable,
            stopped: e.stopped,
            ..DiagnosticFault::raw(e.message)
        }
    }
}
impl From<String> for DiagnosticFault {
    fn from(m: String) -> Self {
        DiagnosticFault::raw(m)
    }
}
impl From<&str> for DiagnosticFault {
    fn from(m: &str) -> Self {
        DiagnosticFault::raw(m)
    }
}
impl std::fmt::Display for DiagnosticFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for DiagnosticFault {}

// --- client -----------------------------------------------------------------

/// The members destructured ahead of `...options` in the JS constructor:
/// `{diagnosticFile, ...options}` — `options` is the flat
/// CoordinatedAccountClientV2 argument list.
pub struct DiagnosticAccountOptions {
    /// diagnosticFile — must be an absolute path.
    pub diagnostic_file: PathBuf,
    /// admissionFile — must be an absolute path.
    pub admission_file: PathBuf,
    /// coordinatedReadGapMs — the JS default is 100 and every other value
    /// fails COORDINATED_CLIENT_EXPLICIT_COORDINATION.
    pub coordinated_read_gap_ms: i64,
    /// coordination — the baal_single_client_rolling_policy_v1 record.
    pub coordination: Option<V>,
    /// `...options` forwarded to `super(key, options)` (ReadClient).
    pub client: ReadClientOptions,
}
impl Default for DiagnosticAccountOptions {
    fn default() -> Self {
        DiagnosticAccountOptions {
            diagnostic_file: PathBuf::new(),
            admission_file: PathBuf::new(),
            coordinated_read_gap_ms: 100,
            coordination: None,
            client: ReadClientOptions::default(),
        }
    }
}

/// `class DiagnosticAccountClientV3 extends CoordinatedAccountClientV2` —
/// composition plus Deref, so `interval`/`write_interval`/`read_only`/
/// `abort`/`requests`/`close` resolve on the wrapped ReadClient.
pub struct DiagnosticAccountClientV3 {
    client: ReadClient,
    /// this.diagnosticFile
    pub diagnostic_file: PathBuf,
    /// The installed wrapper, exposed like the JS `this.fetch` field.
    fetch: Arc<DiagnosticFetch>,
    admissions: Arc<Admissions>,
    state: Arc<Mutex<DiagState>>,
    /// Serializes a perform's lastFault reset + conversion with the fetch —
    /// the JS tail promise keeps the whole perform body in one critical
    /// section.
    perform_tail: Mutex<()>,
    /// authorizeDispatch, kept for the synchronous dispatch() gate.
    authorize_dispatch: Option<Authorizer>,
}
impl Deref for DiagnosticAccountClientV3 {
    type Target = ReadClient;
    fn deref(&self) -> &ReadClient {
        &self.client
    }
}
impl DiagnosticAccountClientV3 {
    pub fn new(key: &str, options: DiagnosticAccountOptions) -> Result<Self, String> {
        let mut options = options;
        // if (!path.isAbsolute(diagnosticFile)) throw Error('DIAGNOSTIC_PATH')
        if !options.diagnostic_file.is_absolute() {
            return Err("DIAGNOSTIC_PATH".to_string());
        }
        // --- super(key, options): the CoordinatedAccountClientV2
        // constructor, inlined (see module docs for why). ---
        let budget = options.client.network_budget.as_ref();
        let c = options.coordination.as_ref();
        need(
            options.admission_file.is_absolute()
                && options.coordinated_read_gap_ms == 100
                && js_int(budget.and_then(|b| b.get("schemaVersion")).unwrap_or(&V::Null))
                    == Some(4)
                && budget.and_then(|b| b.get("soleAccountClient")) == Some(&V::Bool(true))
                && c.and_then(|v| v.get("kind")).and_then(V::as_str)
                    == Some("baal_single_client_rolling_policy_v1")
                && js_int(c.and_then(|v| v.get("readCompletionGapMs")).unwrap_or(&V::Null))
                    == Some(100)
                && js_int(c.and_then(|v| v.get("writeCompletionGapMs")).unwrap_or(&V::Null))
                    == Some(2500)
                && js_int(c.and_then(|v| v.get("maxReadsPerMinute")).unwrap_or(&V::Null))
                    == Some(60)
                && js_int(c.and_then(|v| v.get("maxWritesPerMinute")).unwrap_or(&V::Null))
                    == Some(24),
            "EXPLICIT_COORDINATION",
        )?;
        // const suppliedFetch = options.fetchFn ?? fetch
        let supplied: Arc<dyn Fetch> = match options.client.fetch.take() {
            Some(f) => f,
            None => Arc::new(ReqwestFetch::new()?),
        };
        let signal = options
            .client
            .signal
            .clone()
            .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
        options.client.signal = Some(signal.clone());
        let admissions = Arc::new(Admissions {
            data: Mutex::new(AdmissionData::default()),
            file: options.admission_file.clone(),
            now: options.client.clocks.now.clone(),
            sleep: options.client.clocks.sleep.clone(),
            signal,
        });
        let state = Arc::new(Mutex::new(DiagState::default()));
        let diagnostic_file = options.diagnostic_file.clone();
        let authorize_dispatch = options.client.authorize_dispatch.clone();
        // `const original = this.fetch; this.fetch = diagnostic(original)`
        // — outside the admission wrapper, like the JS assignment order.
        let fetch = Arc::new(DiagnosticFetch {
            inner: AdmittingFetch {
                supplied,
                admissions: admissions.clone(),
            },
            file: diagnostic_file.clone(),
            now: options.client.clocks.now.clone(),
            state: state.clone(),
        });
        options.client.fetch = Some(fetch.clone());
        let mut client = ReadClient::new(key, options.client)?; // super(key, options)
        client.interval = options.coordinated_read_gap_ms as u64; // this.interval
        if options.admission_file.exists() {
            // JSON.parse(fs.readFileSync(file)) — parse/read errors propagate
            // unwrapped, exactly like the JS.
            let raw = std::fs::read(&options.admission_file).map_err(|e| e.to_string())?;
            let r: V = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
            need(
                js_int(&r["schemaVersion"]) == Some(1)
                    && r["reads"].is_array()
                    && r["writes"].is_array(),
                "PERSISTENT_STATE",
            )?;
            for (key, maximum) in [("reads", READ_CAP), ("writes", WRITE_CAP)] {
                let list = r[key].as_array().unwrap();
                // Array.every short-circuits, so the previous element was
                // already validated when the ordering check runs.
                let mut ok = list.len() <= maximum;
                let mut prev = 0i64;
                for (i, el) in list.iter().enumerate() {
                    match js_int(el) {
                        Some(x)
                            if x >= 0
                                && (x as f64) <= (admissions.now)()
                                && (i == 0 || x >= prev) =>
                        {
                            prev = x;
                        }
                        _ => {
                            ok = false;
                            break;
                        }
                    }
                }
                need(ok, "ADMISSION_CLOCK_OR_CAP")?;
            }
            let mut st = admissions.data.lock().map_err(|_| "Stopped".to_string())?;
            st.reads = r["reads"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap_or(0.0))
                .collect();
            st.writes = r["writes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap_or(0.0))
                .collect();
        }
        Ok(DiagnosticAccountClientV3 {
            client,
            diagnostic_file,
            fetch,
            admissions,
            state,
            perform_tail: Mutex::new(()),
            authorize_dispatch,
        })
    }

    /// `this.lastFault` — the most recent fault record, if any.
    pub fn last_fault(&self) -> Option<V> {
        self.state.lock().ok().and_then(|s| s.last_fault.clone())
    }
    /// `this.failures` — the rolling window of safe failure records.
    pub fn failures(&self) -> Vec<V> {
        self.state
            .lock()
            .map(|s| s.failures.clone())
            .unwrap_or_default()
    }
    /// `this.fetch(url, request)` — the diagnostic wrapper itself; it records
    /// and rethrows, but never performs the fatal conversion (that lives in
    /// perform()).
    pub fn fetch(&self, url: &str, req: &FetchRequest) -> Result<FetchOutcome, String> {
        self.fetch.fetch(url, req)
    }

    /// async admit(method) — also reachable directly on the JS instance.
    pub fn admit(&self, method: &str) -> Result<(), String> {
        self.admissions.admit(method)
    }
    /// this.readAdmissions snapshot.
    pub fn read_admissions(&self) -> Vec<f64> {
        self.admissions
            .data
            .lock()
            .map(|s| s.reads.clone())
            .unwrap_or_default()
    }
    /// this.writeAdmissions snapshot.
    pub fn write_admissions(&self) -> Vec<f64> {
        self.admissions
            .data
            .lock()
            .map(|s| s.writes.clone())
            .unwrap_or_default()
    }

    /// The `catch (error)` tail of v3 perform(): convert to the fatal
    /// lastFault error when the recorded fault proves a durable recording
    /// failure, else rethrow the original error unchanged.
    fn convert(&self, e: ServiceError) -> DiagnosticFault {
        let fault = self.state.lock().ok().and_then(|s| s.last_fault.clone());
        if fault.as_ref().and_then(|f| f.get("durableRecordingFailed")) == Some(&V::Bool(true)) {
            return DiagnosticFault::fatal_fault(fault.as_ref().unwrap());
        }
        DiagnosticFault::from(e)
    }
    /// `perform(...)` prelude shared by request/dispatch/orders: clear
    /// lastFault inside the serialized perform section.
    fn perform_begin(&self) -> Result<(), DiagnosticFault> {
        self.state
            .lock()
            .map_err(|_| DiagnosticFault::raw("Stopped"))?
            .last_fault = None;
        Ok(())
    }

    /// request(endpoint) — JS queues `this.perform(...)` on the tail; the v3
    /// perform override resets lastFault and converts durable failures.
    pub fn request(&self, endpoint: &str) -> Result<V, DiagnosticFault> {
        let _tail = self
            .perform_tail
            .lock()
            .map_err(|_| DiagnosticFault::raw("Stopped"))?;
        self.perform_begin()?;
        self.client.request(endpoint).map_err(|e| self.convert(e))
    }
    /// dispatch(spec) — the JS dispatch() rejects synchronously (readOnly,
    /// authorizeDispatch) BEFORE the queued perform runs, so lastFault is
    /// only cleared once the call is admitted onto the tail; mirror that by
    /// applying the same synchronous gates first.
    pub fn dispatch(&self, spec: DispatchSpec) -> Result<V, DiagnosticFault> {
        if self.client.read_only && spec.method != "GET" {
            return Err(DiagnosticFault::raw("Shared account budget forbids Codex writes"));
        }
        let safe = dispatch_safe_view(&spec);
        let authorized = self
            .authorize_dispatch
            .as_ref()
            .is_some_and(|f| f(&safe));
        if !authorized {
            return Err(DiagnosticFault::raw("Execution dispatch is not authorized"));
        }
        let _tail = self
            .perform_tail
            .lock()
            .map_err(|_| DiagnosticFault::raw("Stopped"))?;
        self.perform_begin()?;
        // ReadClient::dispatch re-applies the readOnly + authorize gates —
        // the JS performs the same checks again inside perform.
        self.client.dispatch(spec).map_err(|e| self.convert(e))
    }
    /// orders() — JS pages through `this.request`, so it rides the v3
    /// perform override too.
    pub fn orders(&self) -> Result<Vec<V>, DiagnosticFault> {
        let _tail = self
            .perform_tail
            .lock()
            .map_err(|_| DiagnosticFault::raw("Stopped"))?;
        self.perform_begin()?;
        self.client.orders().map_err(|e| self.convert(e))
    }
}

/// The `safe` view JS hands to authorizeDispatch — identical shape to
/// DispatchSpec::safe_view in service-runtime.
fn dispatch_safe_view(spec: &DispatchSpec) -> V {
    let mut m = Map::new();
    m.insert("method".into(), json!(spec.method));
    m.insert("endpoint".into(), json!(spec.endpoint));
    if let Some(b) = &spec.body {
        m.insert("body".into(), b.clone());
    }
    if let Some(j) = &spec.body_json {
        m.insert("bodyJson".into(), json!(j));
    }
    V::Object(m)
}
