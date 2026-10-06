// live-paper/probability-receipt-observe-v2.mjs — v2 observer loop.
use serde_json::{json, Value as V};
use std::fs;
use std::path::Path;

use crate::probability_receipt_host_v2::{open_probability_receipt, ProbabilityReceiptHostV2};

/// runProbabilityReceipt(options) — resolves the frozen host binding first;
/// in the Rust port the verified binding maps to the crate host function.
pub fn run_probability_receipt(
    options: &crate::probability_receipt_observe::ProbabilityReceiptObserveOptions,
) -> Result<(), String> {
    let bank: V = serde_json::from_slice(
        &fs::read(
            options
                .runtime_dir
                .join("probability-receipt-cohorts")
                .join(options.cohort)
                .join("bank.json"),
        )
        .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    // frozenPaperModule({runtimeDir, bank, modulePath, fallback: currentHost})
    let _frozen = crate::frozen_paper_runtime::frozen_paper_module(
        options.runtime_dir,
        &bank,
        "live-paper/probability-receipt-host-v2.mjs",
    )?;
    let mut w: ProbabilityReceiptHostV2 = open_probability_receipt(
        options.runtime_dir,
        options.cohort,
        options.initialize,
        options.bundle,
        options.now.clone(),
        options.workspace_dir,
        options.source_dir,
    )?;
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
            let stats = w.status()?;
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
    let close_result = w.close();
    close_result.and(result)
}