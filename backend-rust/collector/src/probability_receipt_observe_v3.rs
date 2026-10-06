// live-paper/probability-receipt-observe-v3.mjs — v3 observer loop + v1 companions.
use serde_json::{json, Value as V};
use std::fs;
use std::path::Path;

use crate::probability_receipt_observe::{ObservedWorker, ProbabilityReceiptObserveOptions};
use crate::probability_receipt_host_v2::cohort_ok;

pub const COMPANION_POINTERS: [&str; 2] = [
    "calibrated-poll-paper-active.json",
    "primary-breadth-paper-active.json",
];

/// openProbabilityCompanions(options) — companion v1 workers for live pointers.
pub fn open_probability_companions(
    options: &ProbabilityReceiptObserveOptions,
) -> Result<Vec<Box<dyn ObservedWorker>>, String> {
    let mut workers: Vec<Box<dyn ObservedWorker>> = Vec::new();
    let mut seen = std::collections::BTreeSet::from([options.cohort.to_string()]);
    let run = (|workers: &mut Vec<Box<dyn ObservedWorker>>| -> Result<(), String> {
        for name in COMPANION_POINTERS {
            let file = options.runtime_dir.join(name);
            if !file.exists() {
                continue;
            }
            let pointer: V = serde_json::from_slice(&fs::read(&file).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            let dir = options
                .runtime_dir
                .join("probability-receipt-cohorts")
                .join(pointer["cohort"].as_str().unwrap_or(""));
            if !cohort_ok(pointer["cohort"].as_str().unwrap_or(""))
                || seen.contains(pointer["cohort"].as_str().unwrap_or(""))
            {
                return Err("INVALID_ADDITIONAL_PAPER_COHORT".into());
            }
            seen.insert(pointer["cohort"].as_str().unwrap().to_string());
            if dir.join("OBSERVER_STOP").exists() {
                continue;
            }
            let bank: V = serde_json::from_slice(
                &fs::read(dir.join("bank.json")).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            if bank["schemaVersion"].as_i64() != Some(2) {
                return Err("UNSUPPORTED_ADDITIONAL_PAPER_BANK".into());
            }
            // frozenPaperModule({runtimeDir, bank, modulePath, fallback: additionalHost})
            let _frozen = crate::frozen_paper_runtime::frozen_paper_module(
                options.runtime_dir,
                &bank,
                "live-paper/probability-receipt-host.mjs",
            )?;
            let worker = {
                let n = options.now.clone();
                crate::probability_receipt_host::open_probability_receipt(
                    options.runtime_dir,
                    pointer["cohort"].as_str().unwrap(),
                    options.initialize,
                    options.bundle,
                    move || n(),
                    options.workspace_dir,
                )?
            };
            workers.push(Box::new(worker));
            let w = workers.last().unwrap();
            if w.status()?["bankHash"] != pointer["bankHash"] {
                return Err("ADDITIONAL_PAPER_BANK_MISMATCH".into());
            }
        }
        Ok(())
    })(&mut workers);
    if let Err(e) = run {
        // JS catch: workers closed in reverse; a close error replaces it.
        for worker in workers.iter_mut().rev() {
            if let Err(x) = worker.close() {
                return Err(x);
            }
        }
        return Err(e);
    }
    Ok(workers)
}

/// runProbabilityReceipt(options) — v3 host + companion loop.
pub fn run_probability_receipt(
    options: &ProbabilityReceiptObserveOptions,
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
    // frozenPaperModule({runtimeDir, bank, modulePath: 'live-paper/probability-receipt-host-v3.mjs'})
    let _frozen = crate::frozen_paper_runtime::frozen_paper_module(
        options.runtime_dir,
        &bank,
        "live-paper/probability-receipt-host-v3.mjs",
    )?;
    let mut w = crate::probability_receipt_host_v3::open_probability_receipt(
        options.runtime_dir,
        options.cohort,
        options.initialize,
        options.bundle,
        options.now.clone(),
        options.workspace_dir,
        options.workspace_dir,
    )?;
    let marker = w.dir.join("OBSERVER_STOP");
    let write_all = |entries: &[(std::path::PathBuf, V)]| {
        crate::executable_edge_observe::write_observed_projections(entries)
    };
    let mut additional: Vec<Box<dyn ObservedWorker>> = Vec::new();
    let result = (|| -> Result<(), String> {
        if marker.exists() {
            return Err("PROBABILITY_HOST_OBSERVER_STOP".into());
        }
        additional = open_probability_companions(options)?;
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
            write_all(&[
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
            let mut retired = Vec::new();
            for companion in additional.iter_mut() {
                if companion.dir().join("OBSERVER_STOP").exists() {
                    retired.push(companion.dir().to_path_buf());
                    companion.close()?;
                } else {
                    companion.consume()?;
                    let companion_stats = companion.status()?;
                    let companion_lock: V = serde_json::from_slice(
                        &fs::read(companion.dir().join("worker.lock")).map_err(|e| e.to_string())?,
                    )
                    .map_err(|e| e.to_string())?;
                    write_all(&[
                        (companion.dir().join("stats.json"), companion_stats.clone()),
                        (
                            companion.dir().join("read-focus.json"),
                            crate::probability_read_focus::project_probability_read_focus(
                                &companion_stats,
                                &companion_lock,
                                &companion_stats["cohort"],
                                options.workspace_dir,
                            )?,
                        ),
                    ])?;
                }
            }
            additional.retain(|c| !retired.contains(&c.dir().to_path_buf()));
            if options.once {
                break;
            }
            (options.delay)(1000);
        }
        Ok(())
    })();
    // finally: companions closed forward — a close error aborts the remaining
    // closes AND skips w.close(); otherwise a w.close error replaces result.
    for companion in additional.iter_mut() {
        if let Err(e) = companion.close() {
            return Err(e);
        }
    }
    w.close().and(result)
}
