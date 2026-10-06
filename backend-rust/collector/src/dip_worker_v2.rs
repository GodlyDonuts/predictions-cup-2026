// live-paper/dip-worker-v2.mjs — dip cohort host reading the journal through
// the incremental verified-source reader instead of a full re-read per poll.
// Adds sourceReadMetrics to published stats and treats a COMMIT_BLOCKED guard
// rejection while stopped as a clean stop rather than a fault.
use crate::dip_worker::{io_err, DipPollError, DipWorkerOptions, DIP_COHORTS};
use crate::incremental_journal::VerifiedSourceReader;
use crate::paper_persistence::paper_fault;
use serde_json::{json, Value as V};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

// The JS module keeps `const readers = new Map()` keyed by file path so the
// incremental reader's guards/offsets persist across polls and across any
// workers sharing a path within the process.
fn readers() -> &'static Mutex<HashMap<PathBuf, VerifiedSourceReader>> {
    static READERS: OnceLock<Mutex<HashMap<PathBuf, VerifiedSourceReader>>> = OnceLock::new();
    READERS.get_or_init(|| Mutex::new(HashMap::new()))
}
/// readIncrementalSource(file)
fn read_incremental_source(file: &Path) -> Result<Option<Vec<V>>, String> {
    let mut map = readers().lock().unwrap();
    let reader = map
        .entry(file.to_path_buf())
        .or_insert_with(|| VerifiedSourceReader::new(file, 1_048_576, 16_777_216));
    reader.read()
}
/// incrementalSourceMetrics(file) — null when the path has no reader yet.
fn incremental_source_metrics(file: &Path) -> V {
    readers()
        .lock()
        .unwrap()
        .get(file)
        .map(|r| r.metrics())
        .unwrap_or(V::Null)
}
fn source_for(file: &Path) -> Result<Option<crate::ml_shadow::VerifiedSource>, String> {
    let map = readers().lock().unwrap();
    match map.get(file) {
        Some(reader) => Ok(Some(crate::ml_shadow::VerifiedSource {
            records: reader.rows.clone(),
            prefix_hashes: reader.prefix_hashes.clone(),
        })),
        None => Ok(None),
    }
}

pub struct DipWorkerV2 {
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

impl DipWorkerV2 {
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
            drop(self.release.take());
        } else if let Some(l) = self.release.take() {
            std::mem::forget(l);
        }
    }
    fn context(&self) -> (String, Option<i64>) {
        let bank_hash = match self.state.get("bankHash") {
            Some(V::Null) | None => crate::model::digest(&self.bank),
            Some(v) => v.as_str().map(String::from).unwrap_or_default(),
        };
        (bank_hash, self.state.get("sourceSequence").and_then(V::as_i64))
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
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            (self.options.on_fault)(&fault);
        }));
        let _ = (self.options.save_fault)(&self.runtime_dir, &fault);
        fault
    }
    fn publish(&mut self, status: &str, fault: Option<&V>) -> Result<(), DipPollError> {
        let coverage = std::fs::read_to_string(self.runtime_dir.join("price-coverage.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<V>(&s).ok())
            .unwrap_or(V::Null);
        self.phase = "report".into();
        let journal = self.runtime_dir.join("journal.jsonl");
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
        report["hostVersion"] = json!("dip-worker-v2-incremental-source");
        report["sourceReadMetrics"] = incremental_source_metrics(&journal);
        self.phase = "stats".into();
        struct Probe<'a> {
            w: &'a DipWorkerV2,
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
                // A commit blocked solely because the host stopped races a clean
                // stop in v2: publish 'stopped' instead of diagnosing a fault.
                if let DipPollError::Paper(e) = &error {
                    if e.details["phase"].as_str() == Some("guard")
                        && e.details["osCode"].as_str() == Some("COMMIT_BLOCKED")
                        && self.stopped()
                        && self.owned()
                    {
                        // JS: `await publish('stopped')` inside the catch — a
                        // failure propagates out of poll rather than re-entering
                        // the fault path.
                        self.publish("stopped", None).map_err(|e| e.message())?;
                        return Ok(json!({
                            "status": "stopped",
                            "sourceSequence": self.state["sourceSequence"].clone(),
                        }));
                    }
                }
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
        self.phase = "verify_source".into();
        let journal = self.runtime_dir.join("journal.jsonl");
        let records = read_incremental_source(&journal).map_err(DipPollError::from)?;
        match records {
            None => {
                self.partials += 1;
                if self.partials > 3 {
                    return Err(String::from("Incomplete source").into());
                }
                self.publish("awaiting_capture", None)?;
            }
            Some(_) => {
                self.partials = 0;
                self.phase = "advance".into();
                let verified = source_for(&journal)
                    .map_err(DipPollError::from)?
                    .unwrap_or(crate::ml_shadow::VerifiedSource { records: vec![], prefix_hashes: vec![] });
                let candidate = crate::dip_shadow::advance_dip(&self.state, &verified, &self.bank)
                    .map_err(DipPollError::from)?;
                *attempted_source_sequence = candidate["sourceSequence"].as_i64();
                self.phase = "state".into();
                struct Probe<'a> {
                    w: &'a DipWorkerV2,
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

/// openDipWorker v2 — same scope/lock/open-read sequence as v1.
pub fn open_dip_worker_v2(runtime_dir: &Path, cohort: &str, options: DipWorkerOptions) -> Result<DipWorkerV2, String> {
    if !runtime_dir.is_absolute() || !DIP_COHORTS.contains(&cohort) {
        return Err("Invalid dip host scope".into());
    }
    let lock_file = runtime_dir.join(format!("{}-shadow.lock", cohort));
    let release = crate::paper_runtime::lock(&lock_file)?;
    let mut worker = DipWorkerV2 {
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
            Err(match e {
                DipPollError::Paper(e) => e.to_string(),
                DipPollError::Other { message, .. } => message,
            })
        }
    }
}
