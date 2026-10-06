// data/run-native-receipt-observer-on-fresh-book-20261006.mjs — receipt observer orchestration.
use baal_service_runtime::json_text::{js_stringify, sha256};
use serde_json::{json, Value as V};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn read(f: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(f).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn rt() -> PathBuf {
    Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default()).join("SigTradingCup").join("paper-runtime")
}
fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
}
fn spawn_with_timeout(exe: &Path, args: &[&Path], timeout: Duration) -> Result<std::process::Output, String> {
    let mut child = Command::new(exe)
        .args(args.iter().map(|p| p.as_os_str()))
        .stdout(Stdio::piped()).stderr(Stdio::piped())
        .spawn().map_err(|e| e.to_string())?;
    let start = Instant::now();
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(_) => return child.wait_with_output().map_err(|e| e.to_string()),
            None => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("ETIMEDOUT".into());
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

pub fn run(root: &Path) -> Result<V, String> {
    let rt_dir = rt();
    let prepared = read(&root.join("data/native-receipt-observer-prepared-20261006.json"))?;
    let dir = rt_dir.join("settlement-portfolio-v3-cohorts").join(prepared["cohort"].as_str().unwrap_or(""));
    let bank = read(&dir.join("bank.json"))?;
    let binary = rt_dir.join(prepared["binaryRelative"].as_str().unwrap_or(""));
    let source = rt_dir.join("public-books.jsonl");
    let markets: std::collections::HashSet<String> = bank["research"].as_array().cloned()
        .unwrap_or_default().iter().filter_map(|r| r["marketId"].as_str().map(String::from)).collect();
    if sha256(&std::fs::read(&binary).map_err(|e| e.to_string())?)
        != prepared["serviceBinaryHash"].as_str().unwrap_or("") {
        return Err("Pinned observer binary changed".into());
    }
    let started = now_ms();
    let mut trigger: Option<V> = None;
    while now_ms() - started < 45000.0 {
        for n in ["STOP", "COLLECTOR_STOP", "EXECUTION_STOP"] {
            if rt_dir.join(n).exists() { return Err("STOP provenance review required".into()); }
        }
        if dir.join("STOP").exists() { return Err("Cohort STOP".into()); }
        let consumed = read(&dir.join("stats.json"))?["observer"]["sourceSequence"].as_u64().unwrap_or(0);
        let size = std::fs::metadata(&source).map_err(|e| e.to_string())?.len();
        let n = size.min(1024 * 1024);
        let mut buffer = vec![0u8; n as usize];
        {
            let mut fd = std::fs::File::open(&source).map_err(|e| e.to_string())?;
            fd.seek(SeekFrom::Start(size - n)).map_err(|e| e.to_string())?;
            let _ = fd.read(&mut buffer).map_err(|e| e.to_string())?;
        }
        let end = buffer.iter().rposition(|b| *b == b'\n').unwrap_or(0);
        let mut rows: Vec<&str> = std::str::from_utf8(&buffer[..end]).unwrap_or("").split('\n').collect();
        if n < size { rows.remove(0); }
        for i in (0..rows.len()).rev() {
            if rows[i].is_empty() { continue; }
            let r: V = serde_json::from_str(rows[i]).map_err(|e| e.to_string())?;
            if r["sequence"].as_u64().unwrap_or(0) <= consumed { break; }
            if r["kind"].as_str() == Some("book")
                && markets.contains(r["payload"]["book"]["marketId"].as_str().unwrap_or("")) {
                let age = now_ms() - r["payload"]["book"]["receivedAt"].as_str()
                    .and_then(baal_service_runtime::contract::parse_iso_ms).unwrap_or(f64::NAN);
                if age >= 0.0 && age <= 5000.0 {
                    trigger = Some(json!({"sourceSequence": r["sequence"], "ageMs": age}));
                    break;
                }
            }
        }
        if trigger.is_some() { break; }
        std::thread::sleep(Duration::from_millis(500));
    }
    match trigger {
        None => {
            let out = json!({"status": "no_fresh_unconsumed_source_within_bound",
                "accountRequests": 0, "competitionOrders": 0});
            println!("{}", js_stringify(&out));
            Ok(out)
        }
        Some(trigger) => {
            let result = spawn_with_timeout(&binary, &[
                &PathBuf::from("--observer-config"),
                &rt_dir.join(prepared["configFileRelative"].as_str().unwrap_or("")),
                &PathBuf::from("--consume-once"), &PathBuf::from("--maximum-rows"),
                &PathBuf::from("4096")], Duration::from_millis(60000))?;
            if result.status.code() != Some(0) {
                return Err(format!("Native observer failed: {}",
                    String::from_utf8_lossy(&result.stderr).trim()));
            }
            let parsed: V = serde_json::from_str(String::from_utf8_lossy(&result.stdout).trim())
                .map_err(|e| e.to_string())?;
            let out = json!({"trigger": trigger, "waitMs": now_ms() - started, "result": parsed,
                "persistentServicesAdded": 0, "accountRequests": 0});
            println!("{}", js_stringify(&out));
            Ok(out)
        }
    }
}
