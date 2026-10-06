// live-paper/probability-receipt-observe.mjs — v1 observer loop.
use serde_json::{json, Value as V};
use std::fs;
use std::path::Path;

use crate::probability_receipt_host::{open_probability_receipt, ProbabilityReceiptHost};
use crate::probability_receipt_host_v2::NowFn;

/// Shared worker surface used by the v3/v4 companion loops (JS returns plain
/// objects with dir/bank/consume/status/close members).
pub trait ObservedWorker {
    fn dir(&self) -> &Path;
    fn consume(&mut self) -> Result<usize, String>;
    fn status(&self) -> Result<V, String>;
    fn close(&mut self) -> Result<(), String>;
}
impl ObservedWorker for ProbabilityReceiptHost {
    fn dir(&self) -> &Path {
        self.dir.as_path()
    }
    fn consume(&mut self) -> Result<usize, String> {
        ProbabilityReceiptHost::consume(self)
    }
    fn status(&self) -> Result<V, String> {
        Ok(ProbabilityReceiptHost::status(self))
    }
    fn close(&mut self) -> Result<(), String> {
        ProbabilityReceiptHost::close(self)
    }
}
impl ObservedWorker for crate::probability_receipt_host_v2::ProbabilityReceiptHostV2 {
    fn dir(&self) -> &Path {
        self.dir.as_path()
    }
    fn consume(&mut self) -> Result<usize, String> {
        self.consume().map(|n| n as usize)
    }
    fn status(&self) -> Result<V, String> {
        self.status()
    }
    fn close(&mut self) -> Result<(), String> {
        self.close()
    }
}
impl ObservedWorker for crate::probability_receipt_host_v3::ProbabilityReceiptHostV3 {
    fn dir(&self) -> &Path {
        self.dir.as_path()
    }
    fn consume(&mut self) -> Result<usize, String> {
        self.consume().map(|n| n as usize)
    }
    fn status(&self) -> Result<V, String> {
        self.status()
    }
    fn close(&mut self) -> Result<(), String> {
        self.close()
    }
}

pub struct ProbabilityReceiptObserveOptions<'a> {
    pub runtime_dir: &'a Path,
    pub cohort: &'a str,
    pub once: bool,
    pub bundle: Option<&'a V>,
    pub initialize: bool,
    pub now: NowFn,
    /// JS default = repo root (dirname(dirname(module file))).
    pub workspace_dir: &'a Path,
    /// live-paper module directory for code hashes.
    pub source_dir: &'a Path,
    /// JS signal flag (SIGINT/SIGTERM set stopping=true).
    pub stopping: &'a dyn Fn() -> bool,
    /// Sleep/duration hook — the 1000ms loop delay and write-retry backoff.
    pub delay: &'a dyn Fn(u64),
}

// One active paper engine. Older frozen cohorts remain on disk for replay;
// opening their historical reducers is unnecessary for this strategy's loop.
/// runProbabilityReceipt(options)
pub fn run_probability_receipt(
    options: &ProbabilityReceiptObserveOptions,
) -> Result<(), String> {
    let mut w: ProbabilityReceiptHost = {
        let n = options.now.clone();
        open_probability_receipt(
            options.runtime_dir,
            options.cohort,
            options.initialize,
            options.bundle,
            move || n(),
            options.workspace_dir,
        )?
    };
    let marker = w.dir.join("OBSERVER_STOP");
    let result = (|| -> Result<(), String> {
        if marker.exists() {
            return Err("PROBABILITY_HOST_OBSERVER_STOP".into());
        }
        loop {
            if (options.stopping)() || marker.exists() {
                break;
            }
            w.consume()?;
            let stats = w.status();
            let lock: V = serde_json::from_slice(
                &fs::read(w.dir.join("worker.lock")).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            crate::executable_edge_observe::write_observed_projections(&[
                (w.dir.join("stats.json"), stats.clone()),
                (
                    w.dir.join("read-focus.json"),
                    crate::probability_read_focus::project_probability_read_focus(
                        &stats,
                        &lock,
                        &json!(options.cohort),
                        options.workspace_dir,
                    )?,
                ),
                (
                    options.runtime_dir.join("probability-receipt-active.json"),
                    json!({"cohort": options.cohort, "bankHash": stats["bankHash"]}),
                ),
            ])?;
            if options.once {
                break;
            }
            (options.delay)(1000);
        }
        Ok(())
    })();
    // finally: w.close() — JS lets a close error replace the loop error.
    let close_result = w.close();
    close_result.and(result)
}
