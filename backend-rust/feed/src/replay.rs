// feed/cli.mjs — offline replay driver.
use crate::capture::Capture;
use crate::collector::MarketCollector;
use crate::ms_to_iso;
use baal_service_runtime::contract::parse_iso_ms;
use baal_service_runtime::json_text::js_stringify;
use serde_json::{json, Value as V};
use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;

fn require_that(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(code.to_string()) }
}

pub fn replay(text: &str, capture: Option<Rc<RefCell<Capture>>>, stop: &dyn Fn() -> bool) -> Result<V, String> {
    let now = Rc::new(Cell::new(0.0f64));
    let mut collector: Option<MarketCollector> = None;
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if stop() {
            if let Some(c) = &mut collector {
                c.stop();
            }
            break;
        }
        if line.trim().is_empty() {
            continue;
        }
        require_that(line.len() <= 8 * 1024 * 1024, "FRAME_TOO_LARGE")?;
        let row: V = serde_json::from_str(line).map_err(|_| "INVALID_SCHEMA".to_string())?;
        require_that(row["format"] == "sig-feed-replay" && row["schemaVersion"] == 1, "UNKNOWN_REPLAY_SCHEMA")?;
        let observed = row["observedAt"].as_str().ok_or("INVALID_SCHEMA")?;
        let at = baal_service_runtime::contract::timestamp_str(observed)
            .ok()
            .and_then(|t| parse_iso_ms(&t))
            .ok_or("INVALID_SCHEMA")?;
        require_that(at >= now.get(), "NONMONOTONIC_RECEIPT")?;
        now.set(at);
        let kind = row["kind"].as_str().unwrap_or("");
        if kind == "config" {
            require_that(collector.is_none(), "DUPLICATE_CONFIG")?;
            let stale = row["staleMs"].as_f64().unwrap_or(10000.0);
            let now_rc = now.clone();
            collector = Some(MarketCollector::new(
                &row["scope"],
                stale,
                Rc::new(move || now_rc.get()),
                capture.clone(),
                100000,
            )?);
            continue;
        }
        require_that(collector.is_some(), "CONFIG_REQUIRED")?;
        let c = collector.as_mut().unwrap();
        match kind {
            "subscribed" => {
                c.subscribed(row["reason"].as_str().unwrap_or("INITIAL"))?;
            }
            "snapshot" => {
                let ticket = c.begin_recovery()?;
                c.complete_recovery(&ticket, &row["payload"], observed);
            }
            "event" => {
                c.ingest(row["event"].as_str().unwrap_or(""), &row["payload"], observed, c.epoch);
            }
            "disconnect" => {
                c.disconnected("DISCONNECTED")?;
            }
            "tick" => {
                c.tick();
            }
            "stop" => {
                c.stop();
            }
            _ => return Err("UNSUPPORTED_REPLAY_RECORD".into()),
        }
    }
    let mut c = collector.ok_or("EMPTY_REPLAY")?;
    let result = c.status();
    if let Some(cap) = &capture {
        cap.borrow_mut()
            .append(&json!({"kind": "summary", "observedAt": ms_to_iso(now.get()), "result": result}))?;
    }
    Ok(result)
}

pub fn main(args: &[String]) -> Result<V, String> {
    if args.is_empty() || (args.len() == 1 && args[0] == "--help") {
        return Ok(json!("Collection is disabled. Offline only: --replay scenario.jsonl [--capture new.jsonl]"));
    }
    if args.first().map(String::as_str) != Some("--replay")
        || args.get(1).is_none()
        || ![2usize, 4].contains(&args.len())
        || (args.len() == 4 && (args[2] != "--capture" || args[3].is_empty()))
    {
        return Err("ONLY_OFFLINE_REPLAY_SUPPORTED".into());
    }
    if Path::new("STOP").exists() {
        return Ok(json!({"state": "stopped", "reason": "STOP_PRESENT", "networkRequests": 0}));
    }
    let meta = std::fs::metadata(&args[1]).map_err(|e| e.to_string())?;
    require_that(meta.len() <= 64 * 1024 * 1024, "REPLAY_TOO_LARGE")?;
    let mut capture = None;
    if args.len() == 4 {
        capture = Some(Rc::new(RefCell::new(Capture::new(Path::new(&args[3]), true)?)));
    }
    let text = std::fs::read_to_string(&args[1]).map_err(|e| e.to_string())?;
    let stop_file = || Path::new("STOP").exists();
    let result = replay(&text, capture.clone(), &stop_file)?;
    if let Some(c) = &capture {
        c.borrow_mut().close();
    }
    let _ = js_stringify(&result);
    Ok(result)
}
