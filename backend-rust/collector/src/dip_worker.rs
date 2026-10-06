// live-paper/dip-worker.mjs — dip cohort host: lock ownership, STOP detection,
// verified-source advance, atomic stats publish, and durable fault capture.
// JS async dispatch becomes synchronous calls; the injectable JS collaborators
// (now/writeJson/onFault/saveFault) become boxed closures with the same
// defaults (Date.now / atomicPaperJson / stderr / persistPaperFault).
use crate::paper_persistence::{atomic_paper_json, paper_fault, persist_paper_fault, PaperPersistenceError};
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const DIP_COHORTS: [&str; 3] = ["dip", "dip-scale", "dip-responsive"];

pub type NowFn = dyn Fn() -> f64 + Send + Sync;
pub type WriteJsonFn = dyn Fn(&Path, &V, &dyn Fn() -> bool) -> Result<V, PaperPersistenceError> + Send + Sync;
pub type FaultFn = dyn Fn(&V) + Send + Sync;
pub type SaveFaultFn = dyn Fn(&Path, &V) -> Result<String, PaperPersistenceError> + Send + Sync;

pub struct DipWorkerOptions {
    pub workspace_dir: PathBuf,
    pub now: Arc<NowFn>,
    pub write_json: Arc<WriteJsonFn>,
    pub on_fault: Arc<FaultFn>,
    pub save_fault: Arc<SaveFaultFn>,
}
impl DipWorkerOptions {
    /// Defaults matching the JS destructured parameters.
    pub fn new(workspace_dir: PathBuf) -> Self {
        Self {
            workspace_dir,
            now: Arc::new(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as f64)
                    .unwrap_or(0.0)
            }),
            write_json: Arc::new(|file: &Path, value: &V, can_commit: &dyn Fn() -> bool| {
                atomic_paper_json(file, value, can_commit)
            }),
            on_fault: Arc::new(|fault: &V| {
                use std::io::Write;
                let _ = std::io::stderr()
                    .write_all(format!("{}\n", baal_service_runtime::json_text::js_stringify(fault)).as_bytes());
            }),
            save_fault: Arc::new(|dir: &Path, fault: &V| persist_paper_fault(dir, fault)),
        }
    }
}

/// Internal error type so `poll` can distinguish a PaperPersistenceError
/// (typed `details`) from a plain JS Error(message).
pub(crate) enum DipPollError {
    Paper(PaperPersistenceError),
    Other { message: String, details: Option<V> },
}
impl DipPollError {
    pub(crate) fn message(&self) -> String {
        match self {
            DipPollError::Paper(e) => e.to_string(),
            DipPollError::Other { message, .. } => message.clone(),
        }
    }
    /// The `error.details`/`error.code` view JS paperFault reads.
    pub(crate) fn details(&self) -> Option<V> {
        match self {
            DipPollError::Paper(e) => Some(e.details.clone()),
            DipPollError::Other { details, .. } => details.clone(),
        }
    }
}
impl From<PaperPersistenceError> for DipPollError {
    fn from(e: PaperPersistenceError) -> Self { DipPollError::Paper(e) }
}
impl From<String> for DipPollError {
    fn from(e: String) -> Self { DipPollError::Other { message: e, details: None } }
}
/// Map an io failure onto the JS error.code surface (codes paperFault keeps).
pub(crate) fn io_os_code(e: &std::io::Error) -> &'static str {
    match e.raw_os_error() {
        Some(16) | Some(11) => "EBUSY",
        Some(1) | Some(30) => "EPERM",
        Some(13) => "EACCES",
        Some(28) => "ENOSPC",
        Some(5) => "EIO",
        Some(2) => "ENOENT",
        Some(17) => "EEXIST",
        _ => "UNKNOWN",
    }
}
pub(crate) fn io_err(e: &std::io::Error) -> DipPollError {
    let code = io_os_code(e);
    DipPollError::Other {
        message: e.to_string(),
        details: if code == "UNKNOWN" { None } else { Some(json!({"osCode": code})) },
    }
}

pub struct DipWorker {
    runtime_dir: PathBuf,
    workspace_dir: PathBuf,
    cohort: String,
    lock_file: PathBuf,
    release: Option<crate::paper_runtime::PaperLock>,
    bank: V,
    state: V,
    closed: bool,
    failed: bool,
    partials: u32,
    last_status: String,
    busy: bool,
    phase: String,
    options: DipWorkerOptions,
}

impl DipWorker {
    fn owned(&self) -> bool {
        if self.closed {
            return false;
        }
        match std::fs::read_to_string(&self.lock_file) {
            Ok(s) => s == std::process::id().to_string(),
            Err(_) => false,
        }
    }
    pub fn stopped(&self) -> bool {
        ["STOP", "DIP_STOP"]
            .iter()
            .any(|f| self.runtime_dir.join(f).exists())
            || self.workspace_dir.join("STOP").exists()
    }
    pub fn close(&mut self) {
        if self.closed {
            return;
        }
        let release_owned = self.owned();
        self.closed = true;
        if release_owned {
            // PaperLock::drop unlinks the lock file, like JS release().
            drop(self.release.take());
        } else {
            // Not the owner: the file must survive (JS only calls release() when owned).
            if let Some(l) = self.release.take() {
                std::mem::forget(l);
            }
        }
    }
    fn context(&self) -> (String, Option<i64>) {
        // Returns (bankHash, durableSourceSequence) as JS context() computes them:
        // `state?.bankHash ?? digest(bank)` passes a non-string bankHash through
        // to paperFault's hex gate, which rejects it to null.
        let bank_hash = match self.state.get("bankHash") {
            Some(V::Null) | None => crate::model::digest(&self.bank),
            Some(v) => v.as_str().map(String::from).unwrap_or_default(),
        };
        (
            bank_hash,
            self.state.get("sourceSequence").and_then(V::as_i64),
        )
    }
    fn diagnose(&self, error: &DipPollError, attempted_source_sequence: Option<i64>) -> V {
        let (bank_hash, durable) = self.context();
        let fault = paper_fault(
            error.details().as_ref(),
            &self.cohort,
            &self.phase,
            Some((self.options.now)()),
            Some(bank_hash.as_str()),
            durable,
            attempted_source_sequence,
        );
        // A broken stderr must not mask the original fault; stderr remains the
        // fallback when durable fault persistence fails.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            (self.options.on_fault)(&fault);
        }));
        let _ = (self.options.save_fault)(&self.runtime_dir, &fault);
        fault
    }
    fn publish(&mut self, status: &str, fault: Option<&V>) -> Result<(), DipPollError> {
        let coverage_path = self.runtime_dir.join("price-coverage.json");
        let coverage = std::fs::read_to_string(&coverage_path)
            .ok()
            .and_then(|s| serde_json::from_str::<V>(&s).ok())
            .unwrap_or(V::Null);
        self.phase = "report".into();
        let mut report = crate::dip_shadow::dip_report(
            &self.state,
            &self.bank,
            &baal_service_runtime::contract::ms_to_iso((self.options.now)()),
            &coverage,
        )
        .map_err(DipPollError::from)?;
        report["status"] = json!(status);
        if let Some(f) = fault {
            report["fault"] = f.clone();
        }
        report["hostVersion"] = json!("dip-worker-v1");
        self.phase = "stats".into();
        // canCommit: () => owned() && (status !== 'running' || !stopped())
        struct Probe<'a> {
            w: &'a DipWorker,
            status: &'a str,
        }
        let probe = Probe { w: self, status };
        let can_commit = move || {
            probe.w.owned() && (probe.status != "running" || !probe.w.stopped())
        };
        (self.options.write_json)(
            &self.runtime_dir.join(format!("{}-stats.json", self.cohort)),
            &report,
            &can_commit,
        )
        .map_err(DipPollError::from)?;
        self.last_status = status.to_string();
        Ok(())
    }
    fn read_source(&mut self) -> Result<Option<crate::ml_shadow::VerifiedSource>, DipPollError> {
        self.phase = "read_source".into();
        let raw = std::fs::read_to_string(self.runtime_dir.join("journal.jsonl"))
            .map_err(|e| io_err(&e))?;
        self.phase = "verify_source".into();
        crate::ml_shadow::verify_source(&raw).map_err(DipPollError::from)
    }
    /// JS `poll()` — resolves {status, sourceSequence, fault?}; the only
    /// thrown error is the availability guard.
    pub fn poll(&mut self) -> Result<V, String> {
        if self.closed || self.failed || self.busy {
            return Err("Dip host is not available".into());
        }
        self.busy = true;
        let result = self.poll_inner();
        self.busy = false;
        result
    }
    fn poll_inner(&mut self) -> Result<V, String> {
        let mut attempted_source_sequence: Option<i64> = None;
        match self.poll_body(&mut attempted_source_sequence) {
            Ok(v) => Ok(v),
            Err(error) => {
                self.failed = true;
                self.last_status = "failed".into();
                let fault = self.diagnose(&error, attempted_source_sequence);
                if let Err(publish_error) = self.publish("failed", Some(&fault)) {
                    self.diagnose(&publish_error, attempted_source_sequence);
                }
                Ok(json!({
                    "status": "failed",
                    "sourceSequence": self.state["sourceSequence"].clone(),
                    "fault": fault,
                }))
            }
        }
    }
    fn poll_body(&mut self, attempted_source_sequence: &mut Option<i64>) -> Result<V, DipPollError> {
        if self.stopped() {
            self.publish("stopped", None)?;
            return Ok(json!({
                "status": "stopped",
                "sourceSequence": self.state["sourceSequence"].clone(),
            }));
        }
        let records = self.read_source()?;
        match records {
            None => {
                self.partials += 1;
                if self.partials > 3 {
                    return Err(String::from("Incomplete source").into());
                }
                self.publish("awaiting_capture", None)?;
            }
            Some(records) => {
                self.partials = 0;
                self.phase = "advance".into();
                let candidate = crate::dip_shadow::advance_dip(&self.state, &records, &self.bank)
                    .map_err(DipPollError::from)?;
                *attempted_source_sequence = candidate["sourceSequence"].as_i64();
                self.phase = "state".into();
                struct Probe<'a> {
                    w: &'a DipWorker,
                }
                let probe = Probe { w: self };
                let can_commit = move || probe.w.owned() && !probe.w.stopped();
                (self.options.write_json)(
                    &self.runtime_dir.join(format!("{}-state.json", self.cohort)),
                    &candidate,
                    &can_commit,
                )
                .map_err(DipPollError::from)?;
                self.state = candidate;
                self.publish(if self.stopped() { "stopped" } else { "running" }, None)?;
            }
        }
        Ok(json!({
            "status": self.last_status,
            "sourceSequence": self.state["sourceSequence"].clone(),
        }))
    }
}

/// openDipWorker({runtimeDir, cohort, ...}). `options.workspace_dir` mirrors
/// the JS `workspaceDir` default (the module's grandparent directory); the
/// caller supplies it explicitly since a Rust crate has no equivalent.
pub fn open_dip_worker(runtime_dir: &Path, cohort: &str, options: DipWorkerOptions) -> Result<DipWorker, String> {
    if !runtime_dir.is_absolute() || !DIP_COHORTS.contains(&cohort) {
        return Err("Invalid dip host scope".into());
    }
    let lock_file = runtime_dir.join(format!("{}-shadow.lock", cohort));
    let release = crate::paper_runtime::lock(&lock_file)?;
    let mut worker = DipWorker {
        runtime_dir: runtime_dir.to_path_buf(),
        workspace_dir: options.workspace_dir.clone(),
        cohort: cohort.to_string(),
        lock_file,
        release: Some(release),
        bank: V::Null,
        state: V::Null,
        closed: false,
        failed: false,
        partials: 0,
        last_status: "starting".into(),
        busy: false,
        phase: "read_bank".into(),
        options,
    };
    let opened = (|| -> Result<(V, V), DipPollError> {
        let bank = serde_json::from_str::<V>(
            &std::fs::read_to_string(worker.runtime_dir.join(format!("{}-bank.json", worker.cohort)))
                .map_err(|e| io_err(&e))?,
        )
        .map_err(|e| e.to_string())?;
        worker.phase = "read_state".into();
        let state = serde_json::from_str::<V>(
            &std::fs::read_to_string(worker.runtime_dir.join(format!("{}-state.json", worker.cohort)))
                .map_err(|e| io_err(&e))?,
        )
        .map_err(|e| e.to_string())?;
        Ok((bank, state))
    })();
    match opened {
        Ok((bank, state)) => {
            worker.bank = bank;
            worker.state = state;
            Ok(worker)
        }
        Err(e) => {
            worker.close();
            Err(e.message())
        }
    }
}
