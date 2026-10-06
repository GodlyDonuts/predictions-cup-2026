// data/profile-profit-sprint-postdeployment-20261005.mjs — post-deployment
// read-cadence window profile versus the recorded baseline.
use crate::publication_pack::{now_ms, read_json, runtime_dir, sha};
use baal_collector::read_timing::{timing_distribution, validate_timing};
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::journal::{journal_lines, JournalReadOptions};
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

fn arg(args: &[String], key: &str, otherwise: Option<String>) -> Option<String> {
    args.iter().position(|a| a == key).map(|i| args[i + 1].clone()).or(otherwise)
}
fn seal(file: &str) -> Result<u64, String> {
    let size = std::fs::metadata(file).map_err(|e| e.to_string())?.len();
    let n = size.min(1_048_576);
    let mut f = std::fs::File::open(file).map_err(|e| e.to_string())?;
    f.seek(SeekFrom::Start(size - n)).map_err(|e| e.to_string())?;
    let mut b = vec![0u8; n as usize];
    f.read_exact(&mut b).map_err(|e| e.to_string())?;
    let last = b.iter().rposition(|c| *c == b'\n').ok_or("POSTDEPLOYMENT_EMPTY_SOURCE")?;
    Ok(size - n + last as u64 + 1)
}
fn pid_alive(pid: i64) -> bool {
    if pid <= 0 { return false; }
    std::process::Command::new("kill").arg("-0").arg(pid.to_string())
        .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
        .status().map(|s| s.success()).unwrap_or(false)
}
pub fn run(args: &[String]) -> Result<V, String> {
    let rt = runtime_dir();
    let through = arg(args, "--through", Some(ms_to_iso(now_ms() as f64))).unwrap();
    let from = arg(args, "--from", None).ok_or("POSTDEPLOYMENT_WINDOW_REQUIRED")?;
    let from_ms = parse_iso_ms(&from).unwrap_or(f64::NAN);
    let through_ms = parse_iso_ms(&through).unwrap_or(f64::NAN);
    if !from_ms.is_finite() || !through_ms.is_finite() || through_ms <= from_ms {
        return Err("POSTDEPLOYMENT_WINDOW_REQUIRED".into());
    }
    let in_window = |at: &str| {
        let t = parse_iso_ms(at).unwrap_or(f64::NAN);
        t >= from_ms && t < through_ms
    };
    let raw = std::fs::read_to_string(format!("{rt}/read-timings.jsonl")).map_err(|e| e.to_string())?;
    let complete = &raw[..raw.rfind('\n').map(|i| i + 1).unwrap_or(0)];
    let mut lines: Vec<(String, V)> = Vec::new();
    for line in complete.trim_end().split('\n') {
        if line.is_empty() { continue; }
        let event = validate_timing(&serde_json::from_str::<V>(line).map_err(|e| e.to_string())?)?;
        if in_window(event["queuedAt"].as_str().unwrap_or(""))
            && parse_iso_ms(event["finishedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN) < through_ms
        { lines.push((line.to_string(), event)); }
    }
    let events: Vec<&V> = lines.iter().map(|(_, e)| e).collect();
    let mut groups = serde_json::Map::<String, V>::new();
    for e in &events {
        let kind = e["kind"].as_str().unwrap_or("");
        let g = groups.entry(kind.to_string()).or_insert_with(|| json!({
            "requests": 0, "totalClientMs": 0.0, "serviceMs": 0.0, "admissionMs": 0.0,
        }));
        g["requests"] = json!(g["requests"].as_u64().unwrap_or(0) + 1);
        for (out, key) in [("totalClientMs", "totalMs"), ("serviceMs", "serviceMs"), ("admissionMs", "admissionWaitMs")] {
            g[out] = json!(g[out].as_f64().unwrap_or(0.0) + e[key].as_f64().unwrap_or(0.0));
        }
    }
    let mut recurrence = serde_json::Map::<String, V>::new();
    for id in ["195", "356", "305", "306"] {
        let b: Vec<&&V> = events.iter().filter(|e| e["kind"] == json!("book") && e["marketId"] == json!(id)).collect();
        let vals: Vec<V> = (1..b.len()).map(|i| json!(
            parse_iso_ms(b[i]["finishedAt"].as_str().unwrap_or("")).unwrap_or(0.0)
                - parse_iso_ms(b[i - 1]["finishedAt"].as_str().unwrap_or("")).unwrap_or(0.0)
        )).collect();
        recurrence.insert(id.to_string(), timing_distribution(&vals));
    }
    let outside_client_gap_ms = timing_distribution(&(1..events.len()).map(|i| json!(
        (parse_iso_ms(events[i]["queuedAt"].as_str().unwrap_or("")).unwrap_or(0.0)
            - parse_iso_ms(events[i - 1]["finishedAt"].as_str().unwrap_or("")).unwrap_or(0.0)).max(0.0)
    )).collect::<Vec<V>>());
    let book_service_ms = timing_distribution(&events.iter()
        .filter(|e| e["kind"] == json!("book")).map(|e| e["serviceMs"].clone()).collect::<Vec<V>>());
    let book_admission_ms = timing_distribution(&events.iter()
        .filter(|e| e["kind"] == json!("book")).map(|e| e["admissionWaitMs"].clone()).collect::<Vec<V>>());
    let event_loop_max_ms = timing_distribution(&events.iter().map(|e| e["eventLoopMaxMs"].clone()).collect::<Vec<V>>());
    let account_file = format!("{rt}/account-eligibility.jsonl");
    let book_file = format!("{rt}/public-books.jsonl");
    let mut scopes = Vec::new();
    for line in journal_lines(Path::new(&account_file), &JournalReadOptions {
        end_byte: Some(seal(&account_file)?), ..Default::default() })? {
        let r: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        if r["event"]["type"] == json!("market_account_eligibility") && in_window(r["at"].as_str().unwrap_or("")) {
            scopes.push(json!({"knownAt": r["at"], "ordersAt": r["event"]["snapshot"]["ordersAt"]}));
        }
    }
    let mut books = Vec::new();
    for line in journal_lines(Path::new(&book_file), &JournalReadOptions {
        end_byte: Some(seal(&book_file)?), ..Default::default() })? {
        let r: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        if r["kind"] == json!("book") && in_window(r["at"].as_str().unwrap_or("")) {
            books.push(json!({
                "at": r["at"], "marketId": r["payload"]["book"]["marketId"],
                "levels": [r["payload"]["book"]["bids"].as_array().map(|a| a.len()).unwrap_or(0),
                    r["payload"]["book"]["asks"].as_array().map(|a| a.len()).unwrap_or(0)],
                "sequence": r["sequence"], "hash": r["hash"],
            }));
        }
    }
    let cycles: Vec<V> = (0..scopes.len().saturating_sub(1)).map(|i| {
        let lo = parse_iso_ms(scopes[i]["knownAt"].as_str().unwrap_or("")).unwrap_or(0.0);
        let hi = parse_iso_ms(scopes[i + 1]["knownAt"].as_str().unwrap_or("")).unwrap_or(0.0);
        json!({
            "accountEligibilityAt": scopes[i]["knownAt"], "until": scopes[i + 1]["knownAt"],
            "ordinaryPublishedBooks": books.iter().filter(|b| {
                let at = parse_iso_ms(b["at"].as_str().unwrap_or("")).unwrap_or(0.0);
                at >= lo && at < hi
            }).map(|b| b["marketId"].clone()).collect::<Vec<V>>(),
        })
    }).collect();
    let stats_file = format!("{rt}/stats.json");
    let runtime_stats = read_json(Path::new(&stats_file))?["runtime"].clone();
    let pid = arg(args, "--pid", None).map(|s| s.parse::<i64>().unwrap_or(0))
        .unwrap_or_else(|| runtime_stats["pid"].as_i64().unwrap_or(0));
    let stats_age = now_ms() - std::fs::metadata(&stats_file).map_err(|e| e.to_string())?.modified()
        .map_err(|e| e.to_string())?.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
    if runtime_stats["pid"] != json!(pid) || runtime_stats["status"] != json!("running")
        || runtime_stats["profitSprint"]["enabled"] != json!(true) || stats_age > 10000
    { return Err("POSTDEPLOYMENT_RUNNING_SPRINT_NOT_VERIFIED".into()); }
    let lock = std::fs::read_to_string(format!("{}/SigTradingCup/bot.lock", std::env::var("LOCALAPPDATA").unwrap_or_default())).map_err(|e| e.to_string())?;
    if lock != pid.to_string() { return Err("POSTDEPLOYMENT_COLLECTOR_LOCK_MISMATCH".into()); }
    if !pid_alive(pid) { return Err("kill ESRCH".into()); }
    if runtime_stats["scheduling"]["reads"].as_array().map(|a| a.len()).unwrap_or(0) > 4
        || books.iter().any(|b| b["levels"].as_array().unwrap().iter().any(|n| n.as_u64().unwrap_or(0) > 20))
        || cycles.iter().any(|c| c["ordinaryPublishedBooks"].as_array().map(|a| a.len()).unwrap_or(0) > 4)
    { return Err("POSTDEPLOYMENT_ORDINARY_DEPTH_OR_CAP".into()); }
    let baseline = read_json(Path::new("data/profit-sprint-read-baseline-20261005.json"))?;
    let mut comparisons = serde_json::Map::<String, V>::new();
    for id in ["195", "356"] {
        comparisons.insert(id.to_string(), json!({
            "baselineMedianMs": baseline["bookRecurrence"][id]["median"],
            "postMedianMs": recurrence[id]["median"],
            "postIntervals": recurrence[id]["count"],
            "medianReductionFraction": if recurrence[id]["median"].is_null() { V::Null } else {
                json!(1.0 - recurrence[id]["median"].as_f64().unwrap_or(0.0)
                    / baseline["bookRecurrence"][id]["median"].as_f64().unwrap_or(1.0))
            },
        }));
    }
    let runner = std::fs::read("live-paper/runner.mjs").map_err(|e| e.to_string())?;
    let mut wsha = Sha256::new();
    wsha.update(lines.iter().map(|(l, _)| format!("{l}\n")).collect::<String>().as_bytes());
    let result = json!({
        "schemaVersion": 1, "observedFrom": from, "observedThrough": through,
        "createdAt": ms_to_iso(now_ms() as f64),
        "windowSeconds": (through_ms - from_ms) / 1000.0,
        "readTimingsWindowSha256": format!("{:x}", wsha.finalize()),
        "requests": events.len(), "groups": V::Object(groups.clone()),
        "outsideClientGapMs": outside_client_gap_ms, "bookServiceMs": book_service_ms,
        "bookAdmissionMs": book_admission_ms, "eventLoopMaxMs": event_loop_max_ms,
        "bookRecurrence": V::Object(recurrence.clone()), "baselineComparisons": V::Object(comparisons.clone()),
        "collector": {
            "pid": runtime_stats["pid"], "processAlive": true, "collectorLockMatches": true,
            "status": runtime_stats["status"], "statsFileAgeMs": stats_age,
            "lastCycleAt": runtime_stats["lastCycleAt"], "profitSprint": runtime_stats["profitSprint"],
            "auxiliaryReadAllocation": runtime_stats["auxiliaryReadAllocation"],
            "marketSweepRequestedBatches": runtime_stats["marketSweep"]["requestedBatches"],
            "scheduledMarketIds": runtime_stats["scheduling"]["reads"].as_array().map(|a|
                a.iter().map(|x| x["marketId"].clone()).collect::<Vec<V>>()).unwrap_or_default(),
            "networkBudget": runtime_stats["networkBudget"],
        },
        "ordinaryPublication": {
            "source": "public-books.jsonl plus existing causal account-eligibility boundaries",
            "completedCycleWindows": cycles.len(), "cycles": cycles,
            "books": books.len(),
            "maximumLevelsObserved": books.iter().flat_map(|b|
                b["levels"].as_array().unwrap().iter().filter_map(|n| n.as_u64()).collect::<Vec<_>>()).max().unwrap_or(0),
            "fourBookCapObserved": true,
            "depth20EndpointSourceVerified": String::from_utf8_lossy(&runner).contains("'&depth=20'"),
            "runnerSourceSha256": sha(&runner),
        },
        "platformBudgetChanged": false, "extraAccountClients": 0,
        "contestOrdersSubmittedByProfiler": 0, "profitabilityEstablished": false,
        "limitations": [
            "A short subsequent window measures current cadence, not long-run tail reliability or profit.",
            "Baseline and subsequent windows have different market/load conditions and are not a controlled causal benchmark.",
            "Timing kind book may include separate realtime recovery reads; ordinary published receipts and cycle cap are checked separately.",
            "Stats health is verified by PID/mtime/status; the parent independently verifies CIM command/lock identity.",
        ],
    });
    std::fs::OpenOptions::new().write(true).create(true).open("data/profit-sprint-read-postdeployment-20261005.json")
        .and_then(|mut f| f.write_all(format!("{}\n", serde_json::to_string_pretty(&result).unwrap()).as_bytes()))
        .map_err(|e| e.to_string())?;
    Ok(json!({
        "pid": pid, "requests": result["requests"], "groups": V::Object(groups),
        "recurrence": V::Object(recurrence), "comparisons": V::Object(comparisons),
        "completedCycles": cycles.len(), "sprint": runtime_stats["profitSprint"]["enabled"],
    }))
}
