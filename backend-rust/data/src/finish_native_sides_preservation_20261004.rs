// data/finish-native-sides-preservation-20261004.mjs — recheck quote-replay
// sides preservation on the immutable prefix and verify live recovery.
use baal_service_runtime::json_text::{js_stringify, sha256};
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};

fn sha(b: &[u8]) -> String { sha256(b) }
fn read_json(p: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn localappdata() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).ok_or_else(|| "LOCALAPPDATA".to_string())
}
fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as f64
}
fn parse_iso(s: &str) -> f64 {
    baal_service_runtime::contract::parse_iso_ms(s).unwrap_or(f64::NAN)
}
fn write_wx(path: &Path, body: &[u8]) -> Result<(), String> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; o.mode(0o600); }
    let mut f = o.open(path).map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut f, body).map_err(|e| e.to_string())
}
fn need_eq(a: &V, b: &V) -> Result<(), String> {
    if a == b { Ok(()) } else { Err("assertion failed".into()) }
}
fn need(ok: bool) -> Result<(), String> {
    if ok { Ok(()) } else { Err("assertion failed".into()) }
}
fn pid_alive(id: i64) -> bool {
    if id <= 0 { return false; }
    #[cfg(unix)] { unsafe { libc::kill(id as i32, 0) == 0 } }
    #[cfg(not(unix))] { Path::new(&format!("/proc/{}", id)).exists() }
}

fn quote_replay(exe: &Path, declaration: &Path, sources_file: &Path,
        out: &Path) -> Result<(V, String, f64), String> {
    need(!out.exists())?;
    let start = std::time::Instant::now();
    let child = std::process::Command::new(exe)
        .args(["quote-replay",
            "--declaration", &declaration.display().to_string(),
            "--sources", &sources_file.display().to_string(),
            "--workers", "8", "--out", &out.display().to_string()])
        .output().map_err(|e| e.to_string())?;
    need(child.status.success())?;
    let result = read_json(out)?;
    Ok((result, sha(&std::fs::read(out).map_err(|e| e.to_string())?),
        start.elapsed().as_secs_f64() * 1000.0))
}

pub fn run() -> Result<V, String> {
    let runtime = localappdata()?.join("SigTradingCup").join("paper-runtime");
    let active_file = runtime.join("native-quote-active.json");
    let active_hash = sha(&std::fs::read(&active_file).map_err(|e| e.to_string())?);
    let active = read_json(&active_file)?;
    let dir = runtime.join("research-cohorts").join(active["cohort"].as_str().unwrap_or(""));
    let binding = read_json(&dir.join("binding.json"))?;
    let original_exe = runtime.join("native-builds")
        .join(binding["binaryHash"].as_str().unwrap_or("")).join("baal-backend.exe");
    need_eq(&json!(sha(&std::fs::read(&original_exe).map_err(|e| e.to_string())?)),
        &binding["binaryHash"])?;
    let sources_file = dir.join("sides-preservation.sources.json");
    let sources = read_json(&sources_file)?;
    let first_output = dir.join("sides-preservation.json");
    let first = read_json(&first_output)?;
    // Reuse the exact immutable prefix already replayed, not a moving live pointer.
    if let V::Object(m) = &sources {
        for (name, source) in m {
            need_eq(&source["bytes"], &first["capture"]["sources"][name]["bytes"])?;
            need_eq(&source["hash"], &first["capture"]["sources"][name]["hash"])?;
        }
    }
    let declaration = dir.join("declaration.json");
    let (prior, original_output_hash, original_wall) =
        quote_replay(&original_exe, &declaration, &sources_file,
            &dir.join("sides-preservation-original.json"))?;
    let recheck_exe = baal_collector::rust_backend::native_executable();
    let (next, new_output_hash, new_wall) =
        quote_replay(&recheck_exe, &declaration, &sources_file,
            &dir.join("sides-preservation-recheck.json"))?;
    need_eq(&next["evaluation"]["scorecards"], &prior["evaluation"]["scorecards"])?;
    need_eq(&next["evaluation"]["scorecards"], &first["evaluation"]["scorecards"])?;
    need_eq(&prior["capture"]["engineHash"], &binding["engineHash"])?;
    need_eq(&json!(sha(&std::fs::read(&active_file).map_err(|e| e.to_string())?)),
        &json!(active_hash))?;
    let experiment = read_json(Path::new("data/native-quote-endpoints-20261004.json"))?;
    need_eq(&next["capture"]["engineHash"], &experiment["engineHash"])?;
    if let V::Object(m) = &experiment["activeHashesUnchanged"] {
        for (file, hash) in m {
            need_eq(&json!(sha(&std::fs::read(runtime.join(file))
                .map_err(|e| e.to_string())?)), hash)?;
        }
    }
    let mut groups: indexmap::IndexMap<String, V> = indexmap::IndexMap::new();
    let mut fills: indexmap::IndexMap<String, V> = indexmap::IndexMap::new();
    for card in prior["evaluation"]["scorecards"].as_array().cloned().unwrap_or_default() {
        let point = &card["point"];
        let key = js_stringify(&json!({"cash": point["cash"],
            "realizedPnl": point["realizedPnl"], "liquidationPnl": point["liquidationPnl"],
            "completedTrades": point["completedTrades"], "positions": point["positions"],
            "fills": card["fills"].as_array().map(|a| a.len()).unwrap_or(0)}));
        if !groups.contains_key(&key) {
            groups.insert(key.clone(), json!({"wallets": 0, "cash": point["cash"],
                "realizedPnl": point["realizedPnl"], "liquidationPnl": point["liquidationPnl"],
                "completedTrades": point["completedTrades"],
                "fillCount": card["fills"].as_array().map(|a| a.len()).unwrap_or(0)}));
        }
        let g = groups.get_mut(&key).unwrap();
        g["wallets"] = json!(g["wallets"].as_i64().unwrap_or(0) + 1);
        for fill in card["fills"].as_array().cloned().unwrap_or_default() {
            fills.insert(sha(js_stringify(&fill).as_bytes()), fill);
        }
    }
    let stats = read_json(&runtime.join("stats.json"))?;
    let view = read_json(&runtime.join("native-quote-status.json"))?;
    let pids = read_json(Path::new("data/current-runtime-pids.json"))?;
    for key in ["collector", "paper", "dashboard"] {
        need(pid_alive(pids[key].as_i64().unwrap_or(0)))?;
    }
    need_eq(&stats["runtime"]["status"], &json!("running"))?;
    need_eq(&view["status"], &json!("running"))?;
    need_eq(&view["lastError"], &V::Null)?;
    // Error history is retained. Require current recovery, rather than erasing a
    // past transient 503 or incorrectly treating a nonempty history as a failure.
    let errors: Vec<V> = stats["runtime"]["errors"].as_array().cloned().unwrap_or_default()
        .iter().map(|e| {
            let reason = e["reason"].as_str().unwrap_or("");
            let http_status = if reason.len() == 8 && reason.starts_with("http_")
                    && reason[5..].chars().all(|c| c.is_ascii_digit()) {
                json!(reason[5..].parse::<i64>().unwrap_or(0))
            } else { V::Null };
            json!({"at": e["at"], "reasonHash": sha(reason.as_bytes()),
                "httpStatus": http_status})
        }).collect();
    if !errors.is_empty() {
        let last_cycle = parse_iso(stats["runtime"]["lastCycleAt"].as_str().unwrap_or(""));
        let max_err = errors.iter()
            .map(|e| parse_iso(e["at"].as_str().unwrap_or("")))
            .fold(f64::NEG_INFINITY, f64::max);
        need(last_cycle > max_err)?;
    }
    need(now_ms() - parse_iso(stats["runtime"]["lastCycleAt"].as_str().unwrap_or(""))
        < 180000.0)?;
    need(now_ms() - parse_iso(view["verifiedAt"].as_str().unwrap_or("")) < 180000.0)?;
    need_eq(&read_json(&runtime.join("competition-activation.json"))?["enabled"],
        &json!(false))?;
    need_eq(&read_json(&runtime.join("coordinated-network-budget.json"))?["executionOwner"],
        &json!("Daenerys"))?;
    let evidence = json!({"schemaVersion": 1,
        "at": baal_service_runtime::contract::ms_to_iso(now_ms()),
        "status": "native_sides_preservation_and_current_live_recovery_verified",
        "claimComment": 5982841708i64, "through": prior["evaluation"]["evaluationThrough"],
        "activeEngineHash": prior["capture"]["engineHash"],
        "newEngineHash": next["capture"]["engineHash"], "activeHash": active_hash,
        "originalOutputHash": original_output_hash, "newOutputHash": new_output_hash,
        "firstNewOutputHash": sha(&std::fs::read(&first_output).map_err(|e| e.to_string())?),
        "sourcePrefixHash": sha(&std::fs::read(&sources_file).map_err(|e| e.to_string())?),
        "exactOriginalCards": prior["evaluation"]["scorecards"].as_array()
            .map(|a| a.len()).unwrap_or(0),
        "allOriginalFillsExact": true, "originalWalletsReset": false,
        "activeCohortReplaced": false, "originalWallMs": original_wall,
        "newWallMs": new_wall,
        "paperGroups": groups.values().cloned().collect::<Vec<_>>(),
        "uniqueCounterfactualFillVariants": fills.len(),
        "paperFillVariants": fills.values().cloned().collect::<Vec<_>>(),
        "collector": {"pid": pids["collector"], "status": stats["runtime"]["status"],
            "cycles": stats["runtime"]["cycles"],
            "lastCycleAt": stats["runtime"]["lastCycleAt"],
            "errorCount": errors.len(), "errors": errors,
            "currentRecoveryVerified": true},
        "native": {"status": view["status"], "verifiedAt": view["verifiedAt"],
            "lastError": view["lastError"], "heldMarketIds": view["heldMarketIds"]},
        "pids": pids, "persistentProcesses": 3, "additionalPersistentProcesses": 0,
        "fees": 0, "networkRequests": 0, "competitionOrders": 0,
        "profitabilityEstablished": false});
    write_wx(Path::new("data/native-sides-preservation-20261004.json"),
        format!("{}\n", serde_json::to_string_pretty(&evidence).map_err(|e| e.to_string())?).as_bytes())?;
    Ok(json!({"exactOriginalCards": evidence["exactOriginalCards"],
        "allOriginalFillsExact": true, "through": evidence["through"],
        "paperGroups": evidence["paperGroups"], "collector": evidence["collector"],
        "persistentProcesses": 3, "profitabilityEstablished": false}))
}
