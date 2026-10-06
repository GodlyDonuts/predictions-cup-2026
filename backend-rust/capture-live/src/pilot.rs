// capture-live/pilot.mjs — bounded pilot only. Host supplies reviewed
// credentials, SDK, account budget and an exclusive sanitized sink. No order
// methods or fake fills exist.
use baal_feed::collector::MarketCollector;
use baal_service_runtime::contract::{require_that, snapshot_path};
use baal_service_runtime::wait::{check_stop, interruptible_wait, thread_sleep};
use serde_json::{json, Map, Value as V};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::transport::{AbortSignal, BookTransport};

fn iso(ms: f64) -> String {
    baal_service_runtime::contract::ms_to_iso(ms)
}

pub struct PilotParams {
    pub scope: V,
    pub should_stop: Arc<dyn Fn() -> bool + Send + Sync>,
    pub duration_ms: f64,
    pub now: Arc<dyn Fn() -> f64 + Send + Sync>,
    pub sleep: Arc<dyn Fn(f64) + Send + Sync>,
    pub stale_ms: f64,
    pub max_recoveries: i64,
    pub max_connections: i64,
}

/// capturePilot — synchronous port: the JS promise orchestration serializes to
/// sequential calls here; aborts/shouldStop are checked at the same points.
pub fn capture_pilot(
    params: &PilotParams,
    transport: &BookTransport,
    capture: Option<Arc<Mutex<dyn FnMut(&V) + Send>>>,
    signal: Option<AbortSignal>,
) -> Result<V, String> {
    let now = params.now.clone();
    let now_rc: Rc<dyn Fn() -> f64> = Rc::new({
        let n = now.clone();
        move || n()
    });
    let should_stop = params.should_stop.clone();
    let local = AbortSignal::new();
    let combined = match &signal {
        Some(s) => {
            if s.aborted() {
                local.abort();
            }
            local.clone()
        }
        None => local.clone(),
    };
    check_stop(combined.aborted(), &*should_stop).map_err(|e| e.to_string())?;
    require_that(
        params.duration_ms > 0.0
            && params.duration_ms <= 60000.0
            && params.max_recoveries > 0
            && params.max_recoveries <= 10,
        "INVALID_PILOT_BOUND",
    )
    .map_err(|e| e.to_string())?;
    require_that(
        params.max_connections > 0 && params.max_connections <= 2,
        "INVALID_PILOT_BOUND",
    )
    .map_err(|e| e.to_string())?;
    let started = now();
    let cause = Arc::new(Mutex::new(None::<String>));
    let phase = RefCell::new(String::from("construction"));
    let sink_failed = Rc::new(RefCell::new(false));
    let connections = RefCell::new(0i64);
    let recoveries = RefCell::new(0i64);
    let ever_ready = RefCell::new(false);

    let mut collector = MarketCollector::new(
        &params.scope,
        params.stale_ms,
        now_rc.clone(),
        None,
        10000,
    )
    .map_err(|e| e.to_string())?;

    let aborted_fn = {
        let combined = combined.clone();
        move || combined.aborted()
    };
    let sleep_fn = params.sleep.clone();

    // pending pre-subscription buffer, drained after `active` flips.
    let pending: Arc<Mutex<Vec<(String, V, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let active = Arc::new(AtomicBool::new(false));
    let failed = Arc::new(AtomicBool::new(false));

    let mut outcome = "duration_complete";
    let mut run = || -> Result<(), String> {
        let connect = |collector: &mut MarketCollector| -> Result<(), String> {
            if *connections.borrow() >= params.max_connections {
                return Err("CONNECTION_BOUND".into());
            }
            *connections.borrow_mut() += 1;
            *phase.borrow_mut() = "connect".into();
            active.store(false, Ordering::SeqCst);
            failed.store(false, Ordering::SeqCst);
            pending.lock().unwrap().clear();
            let epoch = collector.epoch + 1;
            {
                let failed_c = failed.clone();
                let active_c = active.clone();
                let epoch_cell = Rc::new(RefCell::new(epoch));
                let _ = epoch_cell;
                // transport.onDisconnect equivalent: flag for post-subscribe check.
                *transport.on_disconnect.borrow_mut() = Arc::new(move || {
                    failed_c.store(true, Ordering::SeqCst);
                    let _ = &active_c;
                });
            }
            let pending_c = pending.clone();
            let active_c = active.clone();
            let abort_c = {
                let cause = cause.clone();
                let local = local.clone();
                move |r: &str| {
                    let mut c = cause.lock().unwrap();
                    if c.is_none() {
                        *c = Some(r.to_string());
                    }
                    drop(c);
                    local.abort();
                }
            };
            let combined_c = combined.clone();
            let combined_sig = combined.clone();
            let failed_ev = failed.clone();
            let now_c = now.clone();
            transport.subscribe(
                &collector.scope.topic,
                Some(&combined_sig),
                std::sync::Arc::new(move |event: &str, payload: V| {
                    if combined_c.aborted() || failed_ev.load(Ordering::SeqCst) {
                        return;
                    }
                    let received_at = iso(now_c());
                    let mut p = pending_c.lock().unwrap();
                    if !active_c.load(Ordering::SeqCst) && p.len() >= 1000 {
                        drop(p);
                        abort_c("buffer_failure");
                    } else {
                        p.push((event.to_string(), payload, received_at));
                    }
                }),
            )?;
            check_stop(combined.aborted(), &*should_stop).map_err(|e| e.to_string())?;
            if failed.load(Ordering::SeqCst) {
                return Err("SUBSCRIPTION_FAILED".into());
            }
            collector
                .subscribed(if *connections.borrow() == 1 { "INITIAL" } else { "RECONNECT" })
                .map_err(|e| e.to_string())?;
            active.store(true, Ordering::SeqCst);
            for (event, payload, received_at) in pending.lock().unwrap().drain(..) {
                check_stop(combined.aborted(), &*should_stop).map_err(|e| e.to_string())?;
                collector.ingest(&event, &payload, &received_at, epoch);
            }
            Ok(())
        };

        let recover = |collector: &mut MarketCollector| -> Result<(), String> {
            if *recoveries.borrow() >= params.max_recoveries {
                return Err("RECOVERY_BOUND".into());
            }
            *recoveries.borrow_mut() += 1;
            *phase.borrow_mut() = "recover".into();
            let ticket = collector.begin_recovery()?;
            let raw = transport.read_snapshot(
                "GET",
                &snapshot_path(&collector.scope),
                Some(&combined),
            )?;
            if collector.complete_recovery(&ticket, &raw, &iso(now())) {
                *ever_ready.borrow_mut() = true;
            }
            Ok(())
        };

        connect(&mut collector)?;
        recover(&mut collector)?;
        while now() - started < params.duration_ms {
            check_stop(combined.aborted(), &*should_stop).map_err(|e| e.to_string())?;
            collector.tick();
            if collector.needs_market_state {
                return Err("SETTLEMENT_RECONCILIATION_REQUIRED".into());
            }
            if transport.refresh_due() {
                transport.disconnect()?;
                collector.disconnected("REFRESH").map_err(|e| e.to_string())?;
                connect(&mut collector)?;
                recover(&mut collector)?;
            } else if collector.state == "disconnected" {
                *phase.borrow_mut() = "reconnect_wait".into();
                interruptible_wait(1000.0, &aborted_fn, &*should_stop, &*sleep_fn)
                    .map_err(|e| e.to_string())?;
                connect(&mut collector)?;
                recover(&mut collector)?;
            } else if collector.state == "needs_resync" {
                *phase.borrow_mut() = "recovery_wait".into();
                interruptible_wait(2000.0, &aborted_fn, &*should_stop, &*sleep_fn)
                    .map_err(|e| e.to_string())?;
                recover(&mut collector)?;
            }
            *phase.borrow_mut() = "idle".into();
            interruptible_wait(250.0, &aborted_fn, &*should_stop, &*sleep_fn)
                .map_err(|e| e.to_string())?;
        }
        if !*ever_ready.borrow() || collector.state != "ready" {
            outcome = "blocked";
        }
        Ok(())
    };

    match run() {
        Ok(()) => {}
        Err(e) => {
            // Map JS outcome classification: sink/callback/buffer failures and
            // aborts decide between 'blocked' and 'stopped'.
            let c = cause.lock().unwrap().clone().unwrap_or_default();
            if *sink_failed.borrow() || c == "callback_failure" || c == "buffer_failure" {
                outcome = "blocked";
            } else if signal.as_ref().is_some_and(|s| s.aborted())
                || c == "user_stop"
                || should_stop()
                || e == "Stopped"
                || e == "STOPPED"
            {
                outcome = "stopped";
            } else if c == "deadline"
                && *phase.borrow() == "idle"
                && *ever_ready.borrow()
                && collector.state == "ready"
            {
                outcome = "duration_complete";
            } else {
                outcome = "blocked";
            }
        }
    }
    local.abort();
    // Cleanup never surfaces transport diagnostics as the outcome.
    transport.close();
    let collector_status = collector.status();
    if *sink_failed.borrow() {
        outcome = "blocked";
    }
    let mut report = json!({
        "format": "sig-capture-pilot-summary",
        "schemaVersion": 1,
        "startedAt": iso(started),
        "finishedAt": iso(now()),
        "outcome": outcome,
        "connections": *connections.borrow(),
        "recoveries": *recoveries.borrow(),
        "restCounts": {"reads": transport.reads.get(), "authPosts": transport.auth_posts.get()},
        "collector": collector_status,
        "orderWrites": 0,
        "tradeTapeComplete": false,
        "eligibleForExecution": false,
        "captureCommitted": false,
    });
    if let Some(sink) = &capture {
        if !*sink_failed.borrow() {
            let mut committed = report.clone();
            committed["captureCommitted"] = V::Bool(true);
            let row = json!({"kind":"summary","observedAt":iso(now()),"report":committed});
            let mut sink = sink.lock().unwrap();
            let prev = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sink(&row)));
            match prev {
                Ok(()) => report["captureCommitted"] = V::Bool(true),
                Err(_) => {
                    *sink_failed.borrow_mut() = true;
                    report["outcome"] = V::String("blocked".into());
                }
            }
        }
    }
    let _ = Map::new;
    Ok(report)
}

// Suppress unused-import warning for the default sleep used by callers.
pub fn default_sleep(ms: f64) {
    thread_sleep(ms);
}
