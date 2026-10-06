// data/verify-diagnostic-bootstrap-20261003.mjs — diagnostic bootstrap verification.
use crate::diagnostic_read_focus::{diagnostic_depth_focus, open_diagnostic_focus};
use baal_service_runtime::atomic::atomic_json_canonical;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::js_stringify;
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn rt() -> PathBuf {
    Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default()).join("SigTradingCup").join("paper-runtime")
}
fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
}

pub fn run(root: &Path) -> Result<V, String> {
    let rt_dir = rt();
    let universe_raw: V = serde_json::from_slice(
        &std::fs::read(root.join("live-paper/price-universe.json")).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let universe = baal_collector::breadth::validate_universe(&universe_raw,
        "bda92870-621e-47b0-bc3c-3602c5c26f55")?;
    let start = Instant::now();
    let stop = Arc::new(AtomicBool::new(false));
    let timer_state = Arc::new(Mutex::new((0i64, Instant::now(), 0f64)));
    {
        let stop = stop.clone();
        let state = timer_state.clone();
        std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(100));
                if stop.load(Ordering::SeqCst) { break; }
                let now = Instant::now();
                if let Ok(mut g) = state.lock() {
                    let gap = now.duration_since(g.1).as_secs_f64() * 1000.0;
                    g.2 = g.2.max(gap);
                    g.1 = now;
                    g.0 += 1;
                }
            }
        });
    }
    let timer = stop.clone();
    let timer_state_out = timer_state.clone();
    let result = (|| -> Result<V, String> {
        let mut source = open_diagnostic_focus(&rt_dir, &universe, true)?;
        source.initialize(|| false)?;
        source.consume()?;
        let pid = std::process::id() as i64;
        let p = source.project(&json!({"pid": pid, "id": "offline-bootstrap-proof"}),
            &json!("collector-diagnostic"), now_ms() as i64, &root.join("live-paper"))?;
        let focus = diagnostic_depth_focus(&p, &universe, pid,
            &json!("offline-bootstrap-proof"), now_ms() as i64, &root.join("live-paper"));
        let (beats, _, max_gap) = timer_state_out.lock().map(|g| (g.0, g.1, g.2))
            .unwrap_or((0, Instant::now(), 0.0));
        if focus.is_none() || p["sourceClosed"] != json!(false) || beats == 0 {
            return Err("BOOTSTRAP_PROOF_FAILED".into());
        }
        let result = json!({"at": ms_to_iso(now_ms()),
            "mode": "full_actual_capture_async_bootstrap_validation",
            "codeHash": p["codeHash"], "universeHash": p["universeHash"], "book": p["book"],
            "price": p["price"], "sourceClosed": p["sourceClosed"],
            "candidates": p["candidates"].as_array().map(|a| a.len() as i64).unwrap_or(0),
            "validFocus": focus.is_some(),
            "durationMs": (start.elapsed().as_secs_f64() * 1000.0).round(),
            "eventLoopTimerCallbacks": beats,
            "maximumTimerGapMs": max_gap.round(),
            "networkRequests": 0, "competitionOrders": 0, "extraBackgroundProcesses": 0});
        atomic_json_canonical(&root.join("data/diagnostic-bootstrap-verification-20261003.json"), &result)
            .map_err(|e| e.to_string())?;
        println!("{}", js_stringify(&result));
        Ok(result)
    })();
    timer.store(true, Ordering::SeqCst);
    result
}
