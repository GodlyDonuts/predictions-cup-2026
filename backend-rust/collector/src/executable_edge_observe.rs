// live-paper/executable-edge-observe.mjs — observed paper worker: executable-edge
// host plus optional bid-anchor / bulk-dip / probability companions publishing
// read-only projections.
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};

fn read_json(file: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(file).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
/// observedPaperErrorCode(error) — error strings carry the JS Error message;
/// error.code only exists for filesystem failures (mapped to raw string).
pub fn observed_paper_error_code(message: &str, code: Option<&str>) -> String {
    let m = message;
    let ok = ["ANCHOR_HOST", "BULK_HOST", "BULK_DIP", "EDGE", "BID_ANCHOR", "INVALID_ANCHOR",
        "INVALID_BULK", "PROBABILITY_HOST", "PROBABILITY_RECEIPT", "PROBABILITY_FOCUS"]
        .iter().any(|p| {
            m.strip_prefix(&format!("{}_", p)).is_some_and(|rest|
                !rest.is_empty() && rest.len() <= 100
                    && rest.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'))
        });
    if ok { return m.to_string(); }
    if let Some(code) = code {
        // /^E[A-Z]{2,20}$/
        if code.len() >= 3 && code.len() <= 21 && code.starts_with('E')
            && code[1..].chars().all(|c| c.is_ascii_uppercase()) {
            return code.to_string();
        }
    }
    "PAPER_WORKER_ERROR".to_string()
}
/// writeObservedProjections(entries) — retry replaceable projections only.
pub fn write_observed_projections(entries: &[(PathBuf, V)]) -> Result<V, String> {
    for attempt in 0..4 {
        let mut failure: Option<String> = None;
        for (file, value) in entries {
            if let Err(e) = crate::paper_runtime::atomic_json(file, value) {
                failure = Some(e);
                break;
            }
        }
        match failure {
            None => return Ok(json!({"attempts": attempt + 1})),
            Some(e) => {
                let retryable = ["EPERM", "EACCES", "EBUSY"].contains(&e.as_str());
                if attempt == 3 || !retryable { return Err(e); }
                std::thread::sleep(std::time::Duration::from_millis(25 * (1 << attempt)));
            }
        }
    }
    unreachable!()
}
/// Companion state shared by the three optional hosts.
enum Companion {
    Anchor(crate::bid_anchor_live_host::LiveBidAnchorHost),
    Bulk(crate::bulk_dip_host::BulkDipHost),
    BulkV2(crate::bulk_dip_host_v2::BulkDipHostV2),
    Probability(crate::probability_receipt_host::ProbabilityReceiptHost),
}
/// runObservedEdge(options) — synchronous version; SIGINT/SIGTERM handled by
/// the caller's stop files instead of process signals.
#[allow(clippy::too_many_arguments)]
pub fn run_observed_edge(runtime_dir: &Path, cohort: &str, initialize: bool,
    bid_anchor: Option<&str>, bulk_dip: Option<&str>, bulk_host_version: Option<i64>,
    probability: Option<&str>, once: bool, now: impl Fn() -> f64 + 'static,
    workspace_dir: &Path) -> Result<(), String> {
    let wrapper_hash = {
        let mut bytes = std::fs::read(workspace_dir.join("live-paper/executable-edge-observe.mjs"))
            .map_err(|e| e.to_string())?;
        bytes.extend(std::fs::read(workspace_dir.join("live-paper/edge-trial-projection.mjs"))
            .map_err(|e| e.to_string())?);
        baal_service_runtime::json_text::sha256(&bytes)
    };
    let now_ref = std::rc::Rc::new(now);
    let mut w = crate::executable_edge_watch::open_executable_edge(runtime_dir, cohort, initialize,
        {
            let n = now_ref.clone();
            move || (*n)()
        }, workspace_dir)?;
    let stop_file = w.dir.join("OBSERVER_STOP");
    let projection = w.dir.join("trial-observation.json");
    let mut companion: Option<Companion> = None;
    let mut bulk: Option<Companion> = None;
    let mut probability_companion: Option<Companion> = None;
    let mut companion_failed = false;
    let mut bulk_failed = false;
    let mut probability_failed = false;
    let result = (|| -> Result<(), String> {
        if stop_file.exists() { return Err("EDGE_OBSERVER_STOP".into()); }
        loop {
            if stop_file.exists() { break; }
            w.consume()?;
            // bid-anchor companion
            if let Some(name) = bid_anchor {
                if !companion_failed {
                    let r = (|companion: &mut Option<Companion>| -> Result<(), String> {
                        if companion.is_none() {
                            *companion = Some(Companion::Anchor(
                                crate::bid_anchor_live_host::open_live_bid_anchor(
                                    runtime_dir, name, false, {
                                        let n = now_ref.clone();
                                        move || (*n)()
                                    }, workspace_dir)?));
                        }
                        let Companion::Anchor(c) = companion.as_mut().unwrap() else { unreachable!() };
                        c.consume()?;
                        let stats = c.status();
                        let lock = read_json(&c.dir.join("worker.lock"))?;
                        write_observed_projections(&[
                            (c.dir.join("stats.json"), stats.clone()),
                            (c.dir.join("read-focus.json"),
                                crate::bid_anchor_read_focus::project_anchor_read_focus(
                                    &stats, &lock, &json!(cohort), workspace_dir)?),
                            (runtime_dir.join("bid-anchor-live-active.json"),
                                json!({"cohort": name, "bankHash": stats["bankHash"]})),
                            (w.dir.join("companion-status.json"),
                                json!({"mode":"bid_anchor_companion_running","at":stats["updatedAt"],
                                    "cohort":name,"sourceClosed":stats["sourceClosed"],
                                    "networkRequests":0,"competitionOrders":0})),
                        ])?;
                        Ok(())
                    })(&mut companion);
                    if let Err(e) = r {
                        companion_failed = true;
                        if let Some(Companion::Anchor(c)) = companion.as_mut() { let _ = c.close(); }
                        companion = None;
                        let _ = crate::paper_runtime::atomic_json(&w.dir.join("companion-status.json"),
                            &json!({"mode":"bid_anchor_companion_failed_closed",
                                "reasonCode":observed_paper_error_code(&e, None),
                                "at":baal_service_runtime::contract::ms_to_iso((*now_ref)()),
                                "cohort":name,"competitionOrders":0,"networkRequests":0}));
                    }
                }
            }
            // bulk-dip companion
            if let Some(name) = bulk_dip {
                if !bulk_failed {
                    let r = (|bulk: &mut Option<Companion>| -> Result<(), String> {
                        if let Some(v) = bulk_host_version {
                            if ![1, 2].contains(&v) { return Err("BULK_HOST_VERSION".into()); }
                        }
                        if bulk.is_none() {
                            *bulk = Some(if bulk_host_version == Some(2) {
                                Companion::BulkV2(crate::bulk_dip_host_v2::open_bulk_dip_v2(
                                    runtime_dir, name, false, {
                                        let n = now_ref.clone();
                                        move || (*n)()
                                    }, workspace_dir)?)
                            } else {
                                Companion::Bulk(crate::bulk_dip_host::open_bulk_dip(
                                    runtime_dir, name, false, {
                                        let n = now_ref.clone();
                                        move || (*n)()
                                    }, workspace_dir)?)
                            });
                        }
                        let (stats, dir) = match bulk.as_mut().unwrap() {
                            Companion::BulkV2(c) => { c.consume()?; (c.status()?, c.dir.clone()) }
                            Companion::Bulk(c) => { c.consume()?; (c.status()?, c.dir.clone()) }
                            _ => unreachable!(),
                        };
                        let lock = read_json(&dir.join("worker.lock"))?;
                        write_observed_projections(&[
                            (dir.join("stats.json"), stats.clone()),
                            (dir.join("read-focus.json"),
                                crate::bulk_dip_read_focus::project_bulk_read_focus(
                                    &stats, &lock, &json!(cohort), workspace_dir)?),
                            (runtime_dir.join("bulk-dip-active.json"),
                                json!({"cohort": name, "bankHash": stats["bankHash"]})),
                            (w.dir.join("bulk-companion-status.json"),
                                json!({"mode":"bulk_dip_companion_running","at":stats["updatedAt"],
                                    "cohort":name,"sourceClosed":stats["sourceClosed"],
                                    "networkRequests":0,"competitionOrders":0})),
                        ])?;
                        Ok(())
                    })(&mut bulk);
                    if let Err(e) = r {
                        bulk_failed = true;
                        match bulk.as_mut() {
                            Some(Companion::BulkV2(c)) => { let _ = c.close(); }
                            Some(Companion::Bulk(c)) => { let _ = c.close(); }
                            _ => {}
                        }
                        bulk = None;
                        let _ = crate::paper_runtime::atomic_json(&w.dir.join("bulk-companion-status.json"),
                            &json!({"mode":"bulk_dip_companion_failed_closed",
                                "reasonCode":observed_paper_error_code(&e, None),
                                "at":baal_service_runtime::contract::ms_to_iso((*now_ref)()),
                                "cohort":name,"competitionOrders":0,"networkRequests":0}));
                    }
                }
            }
            // probability companion
            if let Some(name) = probability {
                if !probability_failed {
                    let r = (|p: &mut Option<Companion>| -> Result<(), String> {
                        if p.is_none() {
                            *p = Some(Companion::Probability(
                                crate::probability_receipt_host::open_probability_receipt(
                                    runtime_dir, name, false, None, {
                                        let n = now_ref.clone();
                                        move || (*n)()
                                    }, workspace_dir)?));
                        }
                        let Companion::Probability(c) = p.as_mut().unwrap() else { unreachable!() };
                        c.consume()?;
                        let stats = c.status();
                        let lock = read_json(&c.dir.join("worker.lock"))?;
                        write_observed_projections(&[
                            (c.dir.join("stats.json"), stats.clone()),
                            (c.dir.join("read-focus.json"),
                                crate::probability_read_focus::project_probability_read_focus(
                                    &stats, &lock, &json!(cohort), workspace_dir)?),
                            (runtime_dir.join("probability-receipt-active.json"),
                                json!({"cohort": name, "bankHash": stats["bankHash"]})),
                            (w.dir.join("probability-companion-status.json"),
                                json!({"mode":"probability_companion_running","at":stats["updatedAt"],
                                    "cohort":name,"sourceClosed":stats["sourceClosed"],
                                    "networkRequests":0,"competitionOrders":0})),
                        ])?;
                        Ok(())
                    })(&mut probability_companion);
                    if let Err(e) = r {
                        probability_failed = true;
                        if let Some(Companion::Probability(c)) = probability_companion.as_mut() { let _ = c.close(); }
                        probability_companion = None;
                        let _ = crate::paper_runtime::atomic_json(&w.dir.join("probability-companion-status.json"),
                            &json!({"mode":"probability_companion_failed_closed",
                                "reasonCode":observed_paper_error_code(&e, None),
                                "at":baal_service_runtime::contract::ms_to_iso((*now_ref)()),
                                "cohort":name,"competitionOrders":0,"networkRequests":0}));
                    }
                }
            }
            // publish
            {
                let stats = w.status()?;
                let lock = read_json(&w.dir.join("worker.lock"))?;
                write_observed_projections(&[
                    (w.dir.join("stats.json"), stats.clone()),
                    (projection.clone(), crate::edge_trial_projection::project_edge_trials(
                        &w.state(), &w.bank, &stats, std::process::id() as i64,
                        lock["id"].as_str().unwrap_or(""), &wrapper_hash, Some("running"))?),
                ])?;
            }
            if once { break; }
            std::thread::sleep(std::time::Duration::from_millis(1000));
        }
        if !once {
            let stats = w.status()?;
            let lock = read_json(&w.dir.join("worker.lock"))?;
            write_observed_projections(&[
                (w.dir.join("stats.json"), stats.clone()),
                (projection.clone(), crate::edge_trial_projection::project_edge_trials(
                    &w.state(), &w.bank, &stats, std::process::id() as i64,
                    lock["id"].as_str().unwrap_or(""), &wrapper_hash, Some("stopped"))?),
            ])?;
        }
        Ok(())
    })();
    // finally: close in order probability -> bulk -> companion -> w.
    if let Some(Companion::Probability(c)) = probability_companion.as_mut() { let _ = c.close(); }
    match bulk.as_mut() {
        Some(Companion::BulkV2(c)) => { let _ = c.close(); }
        Some(Companion::Bulk(c)) => { let _ = c.close(); }
        _ => {}
    }
    if let Some(Companion::Anchor(c)) = companion.as_mut() { let _ = c.close(); }
    let close_result = w.close();
    result.and(close_result)
}
