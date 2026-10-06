// live-paper/fleet-health.mjs — read-only local fleet diagnostics.
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::{digest, js_int, js_truthy};
use serde_json::{json, Map, Value as V};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

// Read-only diagnostics. Never imports account/network clients, reads STOP text,
// repairs a lock, restarts a process, or writes a projection/runtime file.
fn sha(x: Option<&V>) -> V { x.map(|v| json!(digest(v))).unwrap_or(V::Null) }
fn hash_v(x: &V) -> V {
    match x.as_str() {
        Some(s) if s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) => json!(s),
        _ => V::Null,
    }
}
/// Number(v) coercion used by the JS helpers.
fn js_num(v: &V) -> f64 {
    match v {
        V::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        V::String(s) => s.trim().parse::<f64>().unwrap_or(if s.trim().is_empty() { 0.0 } else { f64::NAN }),
        V::Bool(b) => if *b { 1.0 } else { 0.0 },
        V::Null => 0.0,
        _ => f64::NAN,
    }
}
fn pid(v: &V) -> Option<i64> {
    let n = js_num(v);
    if n.is_finite() && n.fract() == 0.0 && n.abs() <= 9_007_199_254_740_991.0 && n > 0.0 { Some(n as i64) } else { None }
}
fn status(x: &V) -> String {
    match x.as_str() {
        Some(s) if !s.is_empty() && s.len() <= 64 && s.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
            && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_') => s.to_string(),
        _ => "unknown".into(),
    }
}
fn safe_time(x: &V) -> V {
    match x.as_str().and_then(parse_iso_ms) {
        Some(ms) if ms.is_finite() => json!(ms_to_iso(ms)),
        _ => V::Null,
    }
}
fn count(x: &V) -> V { js_int(x).filter(|n| *n >= 0).map(|n| json!(n)).unwrap_or(V::Null) }
fn truthy(v: &V) -> bool { js_truthy(Some(v)) }

fn specs() -> Vec<V> {
    let mut v = vec![
        json!({"name": "collector", "stats": "stats", "pid": "pid.txt", "script": "live-paper/runner.mjs", "stop": "COLLECTOR_STOP"}),
        json!({"name": "fee-free", "script": "live-paper/fee-free-shadow.mjs", "alternateScripts": ["live-paper/fee-free-shadow-v2.mjs"], "stop": "FEE_FREE_STOP"}),
        json!({"name": "ml", "script": "live-paper/ml-shadow.mjs", "alternateScripts": ["live-paper/ml-shadow-v2.mjs"], "stop": "ML_STOP"}),
    ];
    for name in ["dip", "dip-scale", "dip-responsive"] {
        v.push(json!({"name": name, "script": "live-paper/dip-shadow.mjs",
            "alternateScripts": ["live-paper/dip-worker.mjs", "live-paper/dip-worker-v2.mjs"],
            "stop": "DIP_STOP", "cohortArgument": name}));
    }
    v.push(json!({"name": "shock", "script": "live-paper/shock-shadow.mjs", "alternateScripts": ["live-paper/shock-shadow-v2.mjs"], "stop": "SHOCK_STOP"}));
    v.push(json!({"name": "passive", "script": "live-paper/passive-watch.mjs", "stop": "PASSIVE_STOP", "lock": "passive-watch.lock"}));
    v.push(json!({"name": "basket", "script": "live-paper/basket-watch.mjs", "alternateScripts": ["live-paper/basket-watch-v2.mjs"], "stop": "BASKET_STOP", "lock": "basket-watch.lock"}));
    v.push(json!({"name": "performance", "script": "execution/performance-watch.mjs", "stop": "PERFORMANCE_STOP", "lock": "performance-watch.lock", "derivedProjection": true}));
    v
}
fn active_cohorts() -> Vec<V> {
    vec![
        json!({"name": "entry-comparison", "manifest": "entry-comparison-active.json", "directory": "entry-comparisons", "script": "live-paper/entry-comparison-watch.mjs", "stop": "ENTRY_COMPARISON_STOP"}),
        json!({"name": "limit-entry", "manifest": "limit-entry-active.json", "directory": "limit-entry-cohorts", "script": "live-paper/limit-entry-watch.mjs", "stop": "LIMIT_ENTRY_STOP"}),
        json!({"name": "stream-book", "manifest": "stream-book-active.json", "directory": "stream-book-cohorts", "script": "live-paper/stream-book-watch.mjs", "stop": "STREAM_BOOK_STOP", "globalLock": "stream-book-gateway.lock", "captureInput": true}),
        json!({"name": "stream-dip", "manifest": "stream-dip-active.json", "directory": "stream-dip-cohorts", "script": "live-paper/stream-dip-watch.mjs", "stop": "STREAM_DIP_STOP", "globalLock": "stream-dip.lock", "captureInput": true}),
        json!({"name": "stream-book-v2", "manifest": "stream-book-v2-active.json", "directory": "stream-book-v2-cohorts", "script": "live-paper/stream-book-watch-v2.mjs", "stop": "STREAM_BOOK_V2_STOP", "globalLock": "stream-book-gateway.lock", "captureInput": true}),
        json!({"name": "stream-dip-v2", "manifest": "stream-dip-v2-active.json", "directory": "stream-dip-v2-cohorts", "script": "live-paper/stream-dip-watch-v2.mjs", "stop": "STREAM_DIP_V2_STOP", "globalLock": "stream-dip.lock", "captureInput": true}),
        json!({"name": "stream-recovery", "manifest": "stream-recovery-active.json", "directory": "stream-recovery-cohorts", "script": "live-paper/stream-recovery-watch.mjs", "stop": "STREAM_RECOVERY_STOP", "globalLock": "stream-recovery.lock", "captureInput": true}),
        json!({"name": "public-recovery", "manifest": "public-recovery-active.json", "directory": "public-recovery-cohorts", "script": "live-paper/public-recovery-watch.mjs", "stop": "PUBLIC_RECOVERY_STOP", "globalLock": "public-recovery.lock", "publicInput": true}),
    ]
}
fn cohort_name(value: &V) -> bool {
    let Some(s) = value.as_str() else { return false };
    let b = s.as_bytes();
    if s.is_empty() || s.len() > 64 || !b[0].is_ascii_lowercase()
        || !b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
    {
        return false;
    }
    !matches!(s, "con" | "prn" | "aux" | "nul" | "com1" | "com2" | "com3" | "com4" | "com5"
        | "com6" | "com7" | "com8" | "com9" | "lpt1" | "lpt2" | "lpt3" | "lpt4" | "lpt5"
        | "lpt6" | "lpt7" | "lpt8" | "lpt9")
}
fn file_iso(t: std::time::SystemTime) -> String {
    let ms = t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0);
    ms_to_iso(ms)
}
fn json_file(file: &Path) -> V {
    match fs::metadata(file) {
        Err(e) => json!({"error": if e.kind() == std::io::ErrorKind::NotFound { "missing" } else { "unreadable" }}),
        Ok(stat) => {
            if stat.len() > 4 * 1024 * 1024 { return json!({"error": "oversized"}); }
            match fs::read_to_string(file).ok().and_then(|t| serde_json::from_str::<V>(&t).ok()) {
                Some(v) if v.is_object() => json!({"value": v, "modifiedAt": file_iso(stat.modified().unwrap_or(std::time::UNIX_EPOCH))}),
                Some(_) => json!({"error": "invalid_schema"}),
                None => json!({"error": "unreadable"}),
            }
        }
    }
}
fn pid_file(file: &Path) -> V {
    match fs::metadata(file) {
        Ok(stat) => {
            let value = if stat.len() < 100 {
                fs::read_to_string(file).ok().and_then(|t| pid(&json!(t.trim())))
            } else { None };
            json!({"present": true, "pid": value.map(|x| json!(x)).unwrap_or(V::Null),
                "modifiedAt": file_iso(stat.modified().unwrap_or(std::time::UNIX_EPOCH))})
        }
        Err(_) => json!({"present": false, "pid": V::Null, "modifiedAt": V::Null}),
    }
}
fn stop_file(file: &Path, provenance: &str) -> V {
    match fs::metadata(file) {
        Ok(stat) => json!({"present": true, "provenance": provenance,
            "modifiedAt": file_iso(stat.modified().unwrap_or(std::time::UNIX_EPOCH))}),
        Err(_) => json!({"present": false, "provenance": provenance, "modifiedAt": V::Null}),
    }
}
fn active_cohort_input(runtime_dir: &Path, spec: &V, global_stops: &[V]) -> Option<V> {
    let manifest = json_file(&runtime_dir.join(spec["manifest"].as_str().unwrap_or("")));
    if manifest["error"].as_str() == Some("missing") { return None; }
    let invalid = |spec: &V, reason: &str| -> V {
        json!({"spec": spec, "manifestError": reason, "reportPresent": true,
            "bankPresent": false, "claims": [], "lock": {}, "stops": global_stops})
    };
    let value = &manifest["value"];
    let keys = ["schemaVersion", "cohort", "bankHash"];
    let value_keys_ok = value.as_object().is_some_and(|o| o.len() == keys.len() && o.keys().all(|k| keys.contains(&k.as_str())));
    if value.is_null() || !value_keys_ok || value["schemaVersion"].as_i64() != Some(1)
        || !cohort_name(&value["cohort"]) || hash_v(&value["bankHash"]).is_null()
    {
        return Some(invalid(spec, "invalid_active_manifest"));
    }
    let mut scoped_spec = spec.as_object().cloned().unwrap_or_default();
    scoped_spec.insert("activeCohort".into(), json!(true));
    scoped_spec.insert("cohortArgument".into(), value["cohort"].clone());
    scoped_spec.insert("manifestBankHash".into(), value["bankHash"].clone());
    let scoped_spec = V::Object(scoped_spec);
    let dir = runtime_dir.join(spec["directory"].as_str().unwrap_or(""))
        .join(value["cohort"].as_str().unwrap_or(""));
    // An on-disk junction/symlink must not turn a valid cohort name into an
    // arbitrary file reader. Missing cohort files remain explicitly unavailable.
    let escaped = (|| -> Result<bool, ()> {
        let real_dir = fs::canonicalize(&dir).map_err(|_| ())?;
        let real_runtime = fs::canonicalize(runtime_dir).map_err(|_| ())?;
        if real_dir != real_runtime.join(spec["directory"].as_str().unwrap_or("")).join(value["cohort"].as_str().unwrap_or("")) {
            return Ok(true);
        }
        for name in ["stats.json", "bank.json", "pid.txt", "worker.lock"] {
            match fs::canonicalize(dir.join(name)) {
                Ok(p) if p != real_dir.join(name) => return Ok(true),
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(()),
            }
        }
        Ok(false)
    })();
    match escaped {
        Ok(true) => return Some(invalid(&scoped_spec, "active_cohort_path_escape")),
        Ok(false) => {}
        Err(()) => return Some(invalid(&scoped_spec, "active_cohort_path_unavailable")),
    }
    let r = json_file(&dir.join("stats.json"));
    let b = json_file(&dir.join("bank.json"));
    let p = pid_file(&dir.join("pid.txt"));
    let l = pid_file(&if !spec["globalLock"].is_null() {
        runtime_dir.join(spec["globalLock"].as_str().unwrap_or(""))
    } else { dir.join("worker.lock") });
    let mut claims = vec![
        { let mut c = p.as_object().cloned().unwrap_or_default(); c.insert("source".into(), json!("pid_file")); V::Object(c) },
        { let mut c = l.as_object().cloned().unwrap_or_default(); c.insert("source".into(), json!("lock")); V::Object(c) },
        json!({"source": "report", "pid": pid(&r["value"]["pid"]).map(|x| json!(x)).unwrap_or(V::Null),
            "modifiedAt": r["modifiedAt"]}),
    ];
    claims.retain(|c| !c["pid"].is_null());
    let mut stops = global_stops.to_vec();
    stops.push(stop_file(&runtime_dir.join(spec["stop"].as_str().unwrap_or("")), "worker_stop"));
    stops.push(stop_file(&dir.join("STOP"), "cohort_stop"));
    {
        let mut input = Map::new();
        input.insert("spec".into(), scoped_spec);
        if !r["value"].is_null() { input.insert("report".into(), r["value"].clone()); }
        if !b["value"].is_null() { input.insert("bank".into(), b["value"].clone()); }
        input.insert("reportPresent".into(), json!(true));
        input.insert("bankPresent".into(), json!(b.get("value").is_some()));
        input.insert("claims".into(), json!(claims));
        input.insert("lock".into(), l);
        input.insert("stops".into(), json!(stops));
        Some(V::Object(input))
    }
}
fn capture_metadata(runtime_dir: &Path, bank: Option<&V>) -> V {
    (|| -> V {
        let source_file = bank.and_then(|b| b["sourceFile"].as_str());
        let Some(source_file) = source_file else { return json!({"available": false, "reason": "capture_unavailable"}) };
        let ok = source_file.strip_prefix("realtime-").is_some_and(|rest| {
            rest.strip_suffix(".jsonl").is_some_and(|mid| {
                !mid.is_empty() && mid.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
            })
        });
        if !ok { return json!({"available": false, "reason": "capture_unavailable"}) }
        let session = json_file(&runtime_dir.join("realtime-session.json"))["value"].clone();
        if session["file"] != json!(source_file) {
            return json!({"available": false, "reason": "source_session_changed"});
        }
        match fs::metadata(runtime_dir.join(source_file)) {
            Ok(stat) => json!({"available": stat.is_file() && stat.len() > 0, "kind": "realtime_capture",
                "at": file_iso(stat.modified().unwrap_or(std::time::UNIX_EPOCH)), "bytes": stat.len(), "maxAgeMs": 20000}),
            Err(_) => json!({"available": false, "reason": "capture_unavailable"}),
        }
    })()
}

// Only the listed runtime PIDs are inspected. Raw process command lines remain
// in memory and never enter the returned health report or error output.
/// probeLocalProcesses(pids)
pub fn probe_local_processes(pids: &[V]) -> V {
    let mut seen = BTreeSet::new();
    let mut out = Map::new();
    for p in pids {
        let Some(id) = pid(p) else { continue };
        if !seen.insert(id) { continue; }
        // kill(pid, 0): ESRCH -> exited; other errors leave liveness unverified.
        let alive = match std::process::Command::new("/bin/kill").arg("-0").arg(id.to_string()).output() {
            Ok(o) if o.status.success() => V::Bool(true),
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr);
                if err.contains("No such process") { V::Bool(false) } else { V::Null }
            }
            Err(_) => if Path::new(&format!("/proc/{id}")).exists() { V::Bool(true) } else { V::Bool(false) },
        };
        out.insert(id.to_string(), json!({"alive": alive, "commandLine": V::Null, "startedAt": V::Null}));
    }
    V::Object(out)
}

// Metadata scan only: do not deserialize market/account payloads. No claim of
// full hash-chain verification is made; the actual workers own that validation.
/// sourceMetadata(file, wanted, publicFormat)
pub fn source_metadata(file: &Path, wanted: &[V], public_format: bool) -> V {
    (|| -> V {
        let stat = match fs::metadata(file) { Ok(s) => s, Err(_) => {
            return json!({"available": false, "reason": "source_unavailable", "clocks": {}, "integrityVerified": false});
        }};
        let target: BTreeSet<u64> = wanted.iter().filter_map(|x| js_int(x).map(|i| i as u64)).collect();
        let mut clocks = Map::new();
        let mut hashes = Map::new();
        if stat.len() == 0 {
            return json!({"available": false, "reason": "empty_source", "clocks": V::Object(clocks)});
        }
        let bytes = match fs::read(file) { Ok(b) => b, Err(_) => {
            return json!({"available": false, "reason": "source_unavailable", "clocks": {}, "integrityVerified": false});
        }};
        let last_is_nl = bytes.last() == Some(&10);
        let text = String::from_utf8_lossy(&bytes);
        let mut lines: Vec<&str> = text.split('\n').collect();
        if last_is_nl { lines.pop(); }
        let mut latest: Option<V> = None;
        let mut malformed = 0i64;
        let n_complete = if last_is_nl { lines.len() } else { lines.len().saturating_sub(1) };
        for line in lines.iter().take(n_complete) {
            let head = parse_journal_line(line, public_format);
            let tail = line_hash(line);
            let Some((seq, at)) = head else {
                malformed += 1;
                continue;
            };
            let at_v = safe_time(&json!(at));
            if tail.is_null() || at_v.is_null() { malformed += 1; continue; }
            let row = json!({"sequence": seq, "at": at_v, "hash": tail});
            latest = Some(row.clone());
            if target.contains(&seq) {
                clocks.insert(seq.to_string(), row["at"].clone());
                hashes.insert(seq.to_string(), row["hash"].clone());
            }
        }
        let mut out = Map::new();
        out.insert("available".into(), json!(latest.is_some()));
        if let Some(l) = latest { if let V::Object(o) = l { for (k, v) in o { out.insert(k, v); } } }
        out.insert("modifiedAt".into(), json!(file_iso(stat.modified().unwrap_or(std::time::UNIX_EPOCH))));
        out.insert("clocks".into(), V::Object(clocks));
        out.insert("hashes".into(), V::Object(hashes));
        out.insert("malformedMetadataRows".into(), json!(malformed));
        out.insert("partialTailExcluded".into(), json!(!last_is_nl));
        out.insert("integrityVerified".into(), json!(false));
        if public_format {
            out.insert("kind".into(), json!("public_book_publication"));
            out.insert("maxAgeMs".into(), json!(20000));
        }
        V::Object(out)
    })()
}
fn line_hash(line: &str) -> V {
    let pat = ",\"hash\":\"";
    if !line.ends_with("\"}") { return V::Null; }
    let Some(i) = line[..line.len() - 2].rfind(pat) else { return V::Null };
    let h = &line[i + pat.len()..line.len() - 2];
    hash_v(&json!(h))
}
/// Returns (sequence, at) for a metadata row, mirroring the JS head regexes.
fn parse_journal_line(line: &str, public: bool) -> Option<(u64, String)> {
    if public {
        let s = line.strip_prefix("{\"schemaVersion\":1,\"kind\":\"")?;
        let mut rest = None;
        for kind in ["session_start", "session_stop", "book", "invalidation"] {
            if let Some(r) = s.strip_prefix(kind) { rest = Some(r); break; }
        }
        let s = rest?.strip_prefix("\",\"sequence\":")?;
        let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() { return None; }
        let s = s[digits.len()..].strip_prefix(",\"previousHash\":\"")?;
        let e = s.find('"')?;
        if !s[..e].bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) { return None; }
        let s = s[e..].strip_prefix("\",\"sessionId\":\"")?;
        let e = s.find('"')?;
        if e == 0 { return None; }
        let s = s[e..].strip_prefix("\",\"tournamentId\":\"")?;
        let e = s.find('"')?;
        if e == 0 { return None; }
        let s = s[e..].strip_prefix("\",\"scopeHash\":\"")?;
        let e = s.find('"')?;
        if e != 64 || !s[..e].bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) { return None; }
        let s = s[e..].strip_prefix("\",\"at\":\"")?;
        let e = s.find('"')?;
        Some((digits.parse().ok()?, s[..e].to_string()))
    } else {
        let s = line.strip_prefix("{\"sequence\":")?;
        let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() { return None; }
        let s = s[digits.len()..].strip_prefix(",\"previous\":\"")?;
        let e = s.find('"')?;
        if !s[..e].bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) { return None; }
        let s = s[e..].strip_prefix("\",\"at\":\"")?;
        let e = s.find('"')?;
        Some((digits.parse().ok()?, s[..e].to_string()))
    }
}

fn get(v: &V, key: &str) -> V { v.get(key).cloned().unwrap_or(V::Null) }
/// `"[^"]*"|'[^']*'|[^\s]+` tokenizer.
fn tokens_of(command: &str) -> Vec<String> {
    let b: Vec<char> = command.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_whitespace() { i += 1; continue; }
        if b[i] == '"' || b[i] == '\'' {
            let q = b[i];
            let mut j = i + 1;
            while j < b.len() && b[j] != q { j += 1; }
            out.push(b[i..=j.min(b.len() - 1)].iter().collect::<String>());
            i = j + 1;
        } else {
            let mut j = i;
            while j < b.len() && !b[j].is_whitespace() { j += 1; }
            out.push(b[i..j].iter().collect::<String>());
            i = j;
        }
    }
    out.into_iter().map(|t| {
        let mut t = t.as_str();
        if t.starts_with('"') || t.starts_with('\'') { t = &t[1..]; }
        if t.ends_with('"') || t.ends_with('\'') { t = &t[..t.len() - 1]; }
        t.to_string()
    }).collect()
}

/// assessWorker(input, now)
pub fn assess_worker(input: &V, now: f64) -> V {
    let spec = &input["spec"];
    let report = &input["report"];
    let bank = &input["bank"];
    let report_present = truthy(&input["reportPresent"]);
    let bank_present = truthy(&input["bankPresent"]);
    let claims = input["claims"].as_array().cloned().unwrap_or_default();
    let lock = &input["lock"];
    let processes = &input["processes"];
    let source = &input["source"];
    let stops = input["stops"].as_array().cloned().unwrap_or_default();
    let manifest_error = input.get("manifestError").cloned().unwrap_or(V::Null);
    let status_src = if !get(report, "status").is_null() { get(report, "status") }
        else { get(report, "runtime")["status"].clone() };
    let reported_status = status(&status_src);
    let timestamp = if !get(report, "updatedAt").is_null() { get(report, "updatedAt") } else { get(report, "reportedAt") };
    let heartbeat = crate::worker_health::worker_health(
        &json!({"updatedAt": timestamp, "status": reported_status}), now, 60000.0);
    let mut seen = BTreeSet::new();
    let mut reported_pids: Vec<i64> = Vec::new();
    for c in &claims {
        if let Some(p) = pid(&c["pid"]) { if seen.insert(p) { reported_pids.push(p); } }
    }
    let process_id = if reported_pids.len() == 1 { Some(reported_pids[0]) } else { None };
    let probe = process_id.map(|id| processes.get(&id.to_string()).cloned().unwrap_or(V::Null)).unwrap_or(V::Null);
    let command = probe["commandLine"].as_str().map(|s| s.replace('\\', "/").to_lowercase());
    let tokens = command.as_deref().map(tokens_of).unwrap_or_default();
    let mut expected: Vec<String> = vec![spec["script"].as_str().unwrap_or("").to_lowercase()];
    for a in spec["alternateScripts"].as_array().cloned().unwrap_or_default() {
        expected.push(a.as_str().unwrap_or("").to_lowercase());
    }
    let script_matches = tokens.iter().any(|t| expected.iter().any(|s| t == s || t.ends_with(&format!("/{s}"))));
    let cohort_flags: Vec<usize> = tokens.iter().enumerate()
        .filter(|(_, t)| t.as_str() == "--name").map(|(i, _)| i).collect();
    let cohort_matches = spec["cohortArgument"].is_null()
        || (cohort_flags.len() == 1
            && tokens.get(cohort_flags[0] + 1).map(|t| json!(t)) == Some(spec["cohortArgument"].clone()));
    let mut identity = if command.is_none() { "unverified".to_string() }
        else if script_matches && cohort_matches { "matches_expected_script".to_string() }
        else { "different_process".to_string() };
    let oldest: Vec<f64> = claims.iter()
        .filter(|c| pid(&c["pid"]) == process_id && truthy(&c["modifiedAt"]))
        .filter_map(|c| parse_iso_ms(c["modifiedAt"].as_str().unwrap_or("")))
        .collect();
    if truthy(&probe["startedAt"]) && !oldest.is_empty()
        && parse_iso_ms(probe["startedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN)
            > oldest.iter().cloned().fold(f64::INFINITY, f64::min) + 5000.0
    {
        identity = "pid_reused_after_claim".into();
    }
    let process_state = if reported_pids.len() > 1 { "conflicting_pid_claims" }
        else if process_id.is_none() { "unknown_pid" }
        else if probe["alive"] == V::Bool(false) { "exited" }
        else if probe["alive"] != V::Bool(true) { "unverified" }
        else if ["different_process", "pid_reused_after_claim"].contains(&identity.as_str()) { "wrong_process" }
        else { "alive" };
    let bank_present_v = if truthy(bank) { Some(bank) } else { None };
    let derived = truthy(&spec["derivedProjection"]);
    let mut lineage = json!({
        "status": if spec["name"].as_str() == Some("collector") || derived { "not_applicable" } else { "unavailable" },
        "bankHash": sha(bank_present_v),
        "reportedBankHash": hash_v(&report["bankHash"]),
    });
    if bank_present_v.is_some() && spec["name"].as_str() != Some("collector") {
        let b = bank_present_v.unwrap();
        if spec["name"].as_str() == Some("passive") {
            let scope_markets = b["scope"]["markets"].as_array().cloned().unwrap_or_default();
            let ok = report["sessionId"] == b["sessionId"] && report["cutoff"] == b["cutoff"]
                && report["sourceFile"] == b["sourceFile"]
                && truthy(&report["codeIdentity"]) && truthy(&b["codeIdentity"])
                && sha(Some(&report["codeIdentity"])) == sha(Some(&b["codeIdentity"]))
                && !scope_markets.is_empty()
                && report["markets"].is_array()
                && report["markets"].as_array().map(|a| a.len()).unwrap_or(0) == scope_markets.len()
                && report["markets"].as_array().cloned().unwrap_or_default().iter()
                    .all(|m| truthy(m) && m["ready"].is_boolean() && m["sourceHash"] == b["sourceHash"]);
            lineage["status"] = json!(if ok { "reported_session_matches" } else { "mismatch" });
        } else {
            let bank_sha = sha(Some(b));
            let ok = hash_v(&report["bankHash"]) == bank_sha
                && (!truthy(&spec["activeCohort"])
                    || (spec["manifestBankHash"] == bank_sha && report["cohort"] == spec["cohortArgument"]));
            lineage["status"] = json!(if ok { "reported_bank_matches" } else { "mismatch" });
        }
        lineage["cutoff"] = safe_time(&b["cutoff"]);
        lineage["frozenSourceHash"] = hash_v(&b["sourceHash"]);
    }
    let sequence = js_int(&report["sourceSequence"]);
    let lag = if sequence.is_some() && truthy(&source["available"]) {
        js_int(&source["sequence"]).map(|s| s - sequence.unwrap())
    } else { None };
    let processed_at = sequence.and_then(|s| source["clocks"].get(&s.to_string()).cloned()).unwrap_or(V::Null);
    let reported_source_hash = {
        let v = if !report["sourceHash"].is_null() { &report["sourceHash"] } else { &report["lastRecordHash"] };
        hash_v(v)
    };
    let captured_source_hash = sequence
        .and_then(|s| source["hashes"].get(&s.to_string()).cloned())
        .map(|h| hash_v(&h)).unwrap_or(V::Null);
    let source_identity = if !reported_source_hash.is_null() && !captured_source_hash.is_null() {
        if reported_source_hash == captured_source_hash { "matches_source_record" } else { "mismatch" }
    } else { "unverified" };
    let source_age_ms = if truthy(&source["at"]) && now >= parse(&source["at"]) {
        json!(now - parse(&source["at"]))
    } else { V::Null };
    let source_delay_ms = if !processed_at.is_null() && truthy(&source["at"]) {
        json!((parse(&source["at"]) - parse(&processed_at)).max(0.0))
    } else { V::Null };
    let active_stops: Vec<V> = stops.iter().filter(|s| truthy(&s["present"])).cloned().collect();
    let configured = report_present || bank_present || !claims.is_empty() || truthy(&lock["present"]);
    let mut reasons: Vec<V> = Vec::new();
    if reported_status == "failed" { reasons.push(json!("worker_reported_failure")); }
    if !manifest_error.is_null() { reasons.push(manifest_error.clone()); }
    if process_state != "alive" { reasons.push(json!(format!("process_{process_state}"))); }
    if identity != "matches_expected_script" && probe["alive"] == V::Bool(true) {
        reasons.push(json!(format!("process_identity_{identity}")));
    }
    if ["stale_worker", "invalid_heartbeat"].contains(&heartbeat["status"].as_str().unwrap_or("")) {
        reasons.push(heartbeat["status"].clone());
    }
    if !truthy(&source["available"]) { reasons.push(json!("source_unavailable")); }
    if source["malformedMetadataRows"].as_i64().unwrap_or(0) > 0 { reasons.push(json!("source_metadata_invalid")); }
    if let Some(l) = lag { if l < 0 { reasons.push(json!("worker_ahead_of_source_snapshot")); } }
    if let Some(l) = lag {
        if l > 3 && !source_delay_ms.is_null() && source_delay_ms.as_f64().unwrap_or(0.0) > 60000.0 {
            reasons.push(json!("source_replay_lag"));
        }
    }
    if !source_age_ms.is_null()
        && source_age_ms.as_f64().unwrap_or(0.0) > source["maxAgeMs"].as_f64().unwrap_or(60000.0)
    {
        reasons.push(json!("source_stale"));
    }
    if source.get("bytes").is_some() && (js_int(&report["sourceBytes"]).is_none()
        || report["sourceBytes"].as_i64().unwrap_or(i64::MAX) > source["bytes"].as_i64().unwrap_or(0))
    {
        reasons.push(json!("source_metadata_invalid"));
    }
    if lineage["status"].as_str() == Some("mismatch") { reasons.push(json!("cohort_lineage_mismatch")); }
    if source_identity == "mismatch" { reasons.push(json!("source_record_hash_mismatch")); }
    if !truthy(&lock["present"]) && process_state == "alive" { reasons.push(json!("worker_lock_missing")); }
    if !active_stops.is_empty() { reasons.push(json!("stop_marker_present")); }
    let reasons_have = |s: &str| reasons.iter().any(|r| r.as_str() == Some(s));
    let overall = if !configured { "not_initialized" }
        else if reported_status == "failed" { "failed" }
        else if !manifest_error.is_null() { "manifest_invalid" }
        else if lineage["status"].as_str() == Some("mismatch") || source_identity == "mismatch" { "lineage_mismatch" }
        else if ["wrong_process", "conflicting_pid_claims"].contains(&process_state) { "pid_conflict" }
        else if process_state == "exited" && !active_stops.is_empty() && !truthy(&lock["present"])
            && ["stopped", "source_session_ended"].contains(&reported_status.as_str()) { reported_status.as_str() }
        else if process_state == "exited" { "exited" }
        else if !active_stops.is_empty() { if process_state == "alive" { "stop_pending" } else { "stopped" } }
        else if heartbeat["status"].as_str() == Some("invalid_heartbeat") { "invalid_clock" }
        else if heartbeat["status"].as_str() == Some("stale_worker") { "stale_heartbeat" }
        else if reasons_have("source_replay_lag") { "source_lag" }
        else if reasons_have("worker_ahead_of_source_snapshot") { "source_snapshot_unverified" }
        else if reasons_have("source_stale") { "source_stale" }
        else if reasons_have("source_unavailable") { "source_unavailable" }
        else if reasons_have("source_metadata_invalid") || lineage["status"].as_str() == Some("unavailable")
            || process_state != "alive" || identity != "matches_expected_script" || !truthy(&lock["present"]) { "unverified" }
        else if reported_status == "running" { "running" }
        else { reported_status.as_str() };
    let mut out = Map::new();
    out.insert("name".into(), spec["name"].clone());
    if truthy(&spec["activeCohort"]) {
        out.insert("cohort".into(), spec["cohortArgument"].clone());
        out.insert("manifestBankHash".into(), spec["manifestBankHash"].clone());
    }
    out.insert("status".into(), json!(overall));
    out.insert("configured".into(), json!(configured));
    out.insert("reportedStatus".into(), json!(reported_status));
    out.insert("pid".into(), process_id.map(|p| json!(p)).unwrap_or(V::Null));
    out.insert("process".into(), json!({
        "status": process_state, "identity": identity,
        "startedAt": safe_time(&probe["startedAt"]),
        "claims": claims.iter().map(|c| json!({"source": c["source"], "pid": c["pid"]})).collect::<Vec<_>>(),
    }));
    out.insert("heartbeat".into(), json!({
        "updatedAt": safe_time(&timestamp), "ageMs": heartbeat["heartbeatAgeMs"],
    }));
    let lag_bytes = if js_int(&source["bytes"]).is_some() && js_int(&report["sourceBytes"]).is_some() {
        json!(source["bytes"].as_i64().unwrap_or(0) - report["sourceBytes"].as_i64().unwrap_or(0))
    } else { V::Null };
    out.insert("source".into(), json!({
        "kind": if source["kind"].is_null() { json!("collector_journal") } else { source["kind"].clone() },
        "available": source["available"] == V::Bool(true),
        "processedSequence": sequence.map(|s| json!(s)).unwrap_or(V::Null),
        "latestSequence": source["sequence"].clone(),
        "lagRecords": lag.map(|l| json!(l)).unwrap_or(V::Null),
        "processedAt": processed_at,
        "lagBytes": lag_bytes,
        "delayBehindSourceMs": source_delay_ms,
        "sourceLatestAt": source["at"].clone(),
        "sourceAgeMs": source_age_ms,
        "identity": source_identity,
    }));
    out.insert("lockPresent".into(), json!(truthy(&lock["present"])));
    out.insert("lineage".into(), lineage);
    out.insert("stops".into(), json!(active_stops.iter()
        .map(|s| json!({"provenance": s["provenance"], "modifiedAt": s["modifiedAt"]})).collect::<Vec<_>>()));
    out.insert("reasons".into(), json!(reasons));
    V::Object(out)
}
fn parse(v: &V) -> f64 { v.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN) }

/// fleetHealth({runtimeDir, workspaceDir, collectorLockPath, now, probeProcesses}).
/// `probe_processes` defaults to `probe_local_processes`.
pub fn fleet_health(options: &V, probe_processes: Option<&dyn Fn(&[V]) -> V>) -> Result<V, String> {
    let runtime_dir = options.get("runtimeDir").and_then(V::as_str).unwrap_or("").to_string();
    // JS: now !== undefined && !Number.isFinite(now) throws; null/non-number too.
    let now_in = match options.get("now") {
        None => None,
        Some(V::Number(n)) => n.as_f64(),
        Some(_) => Some(f64::NAN),
    };
    if runtime_dir.is_empty() || now_in.is_some_and(|n| !n.is_finite()) {
        return Err("Runtime directory and valid clock required".into());
    }
    let runtime_dir = fs::canonicalize(&runtime_dir).map(|p| p.to_string_lossy().into_owned())
        .unwrap_or(runtime_dir);
    let workspace_dir = options.get("workspaceDir").and_then(V::as_str).map(String::from)
        .unwrap_or_else(|| format!("{}/../..", env!("CARGO_MANIFEST_DIR")));
    let workspace_dir = fs::canonicalize(&workspace_dir).map(|p| p.to_string_lossy().into_owned())
        .unwrap_or(workspace_dir);
    let collector_lock = options.get("collectorLockPath").and_then(V::as_str).map(String::from)
        .unwrap_or_else(|| {
            let base = std::env::var("LOCALAPPDATA").ok()
                .or_else(|| Path::new(&runtime_dir).parent().map(|p| p.to_string_lossy().into_owned()))
                .unwrap_or_default();
            format!("{base}/SigTradingCup/bot.lock")
        });
    let runtime_path = Path::new(&runtime_dir);
    let workspace_path = Path::new(&workspace_dir);
    let global_stops = vec![
        stop_file(&workspace_path.join("STOP"), "workspace_stop"),
        stop_file(&runtime_path.join("STOP"), "runtime_stop"),
    ];
    let mut inputs: Vec<V> = Vec::new();
    for spec in specs() {
        let stats_name = if !spec["stats"].is_null() { spec["stats"].as_str().unwrap_or("").to_string() }
            else { format!("{}-stats", spec["name"].as_str().unwrap_or("")) };
        let r = json_file(&runtime_path.join(format!("{stats_name}.json")));
        let b = if spec["name"].as_str() == Some("collector") || truthy(&spec["derivedProjection"]) {
            V::Null
        } else {
            json_file(&runtime_path.join(format!("{}-bank.json", spec["name"].as_str().unwrap_or(""))))
        };
        let p = pid_file(&runtime_path.join(if !spec["pid"].is_null() {
            spec["pid"].as_str().unwrap_or("").to_string()
        } else { format!("{}-pid.txt", spec["name"].as_str().unwrap_or("")) }));
        let l = pid_file(&if spec["name"].as_str() == Some("collector") {
            PathBuf::from(&collector_lock)
        } else {
            runtime_path.join(if !spec["lock"].is_null() { spec["lock"].as_str().unwrap_or("").to_string() }
                else { format!("{}-shadow.lock", spec["name"].as_str().unwrap_or("")) })
        });
        let mut claims = Vec::new();
        {
            let mut c = p.as_object().cloned().unwrap_or_default();
            c.insert("source".into(), json!("pid_file"));
            claims.push(V::Object(c));
        }
        {
            let mut c = l.as_object().cloned().unwrap_or_default();
            c.insert("source".into(), json!("lock"));
            claims.push(V::Object(c));
        }
        {
            let mut c = Map::new();
            c.insert("source".into(), json!("report"));
            let rt_pid = get(&r["value"]["runtime"], "pid");
            let pid_src = if !rt_pid.is_null() { rt_pid } else { get(&r["value"], "pid") };
            c.insert("pid".into(), pid(&pid_src).map(|x| json!(x)).unwrap_or(V::Null));
            if !r["modifiedAt"].is_null() { c.insert("modifiedAt".into(), r["modifiedAt"].clone()); }
            claims.push(V::Object(c));
        }
        claims.retain(|c| !c["pid"].is_null());
        let mut stops = global_stops.clone();
        stops.push(stop_file(&runtime_path.join(spec["stop"].as_str().unwrap_or("")), "worker_stop"));
        let mut input = Map::new();
        input.insert("spec".into(), spec);
        if !r["value"].is_null() { input.insert("report".into(), r["value"].clone()); }
        if !b["value"].is_null() { input.insert("bank".into(), b["value"].clone()); }
        input.insert("reportPresent".into(), json!(r["error"].as_str() != Some("missing")));
        input.insert("bankPresent".into(), json!(!b["value"].is_null()));
        input.insert("claims".into(), json!(claims));
        input.insert("lock".into(), l);
        input.insert("stops".into(), json!(stops));
        inputs.push(V::Object(input));
    }
    for spec in active_cohorts() {
        if let Some(x) = active_cohort_input(runtime_path, &spec, &global_stops) { inputs.push(x); }
    }
    let wanted: Vec<V> = inputs.iter()
        .filter(|x| !truthy(&x["spec"]["publicInput"]))
        .map(|x| x["report"]["sourceSequence"].clone()).collect();
    let public_wanted: Vec<V> = inputs.iter()
        .filter(|x| truthy(&x["spec"]["publicInput"]))
        .map(|x| x["report"]["sourceSequence"].clone()).collect();
    let source = source_metadata(&runtime_path.join("journal.jsonl"), &wanted, false);
    let public_source = source_metadata(&runtime_path.join("public-books.jsonl"), &public_wanted, true);
    let pids: Vec<V> = inputs.iter().flat_map(|x| x["claims"].as_array().cloned().unwrap_or_default())
        .map(|c| c["pid"].clone()).collect();
    let default_probe = |p: &[V]| probe_local_processes(p);
    let processes = probe_processes.unwrap_or(&default_probe)(&pids);
    let official = get(&json_file(&runtime_path.join("competition-stats.json")), "value");
    let realtime = get(&json_file(&runtime_path.join("realtime-stats.json")), "value");
    let passive_bank = inputs.iter().find(|x| x["spec"]["name"].as_str() == Some("passive"))
        .and_then(|x| x.get("bank").cloned());
    let passive_source = capture_metadata(runtime_path, passive_bank.as_ref());
    let execution_stop = stop_file(&runtime_path.join("EXECUTION_STOP"), "execution_stop");
    let now = now_in.unwrap_or_else(|| std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0));
    let mut workers = Vec::new();
    for x in &inputs {
        let spec = &x["spec"];
        let src = if spec["name"].as_str() == Some("passive") { passive_source.clone() }
            else if truthy(&spec["publicInput"]) { public_source.clone() }
            else if truthy(&spec["captureInput"]) { capture_metadata(runtime_path, x.get("bank")) }
            else { source.clone() };
        let mut inp = x.as_object().cloned().unwrap_or_default();
        inp.insert("processes".into(), processes.clone());
        inp.insert("source".into(), src);
        workers.push(assess_worker(&V::Object(inp), now));
    }
    let execution_time = safe_time(&official["updatedAt"]);
    let realtime_time = safe_time(&realtime["at"]);
    let realtime_health = crate::worker_health::worker_health(
        &json!({"updatedAt": realtime_time, "status": "fresh"}), now, 60000.0);
    let realtime_markets_valid = realtime["markets"].as_array().is_some_and(|a| {
        !a.is_empty() && a.iter().all(|m| truthy(m) && m["ready"].is_boolean())
    });
    let status_overall = if !workers.iter().any(|w| truthy(&w["configured"])) { "not_initialized" }
        else if workers.iter().any(|w| truthy(&w["configured"])
            && ["failed", "exited", "pid_conflict", "lineage_mismatch", "manifest_invalid"].contains(&w["status"].as_str().unwrap_or(""))) { "attention_required" }
        else if workers.iter().any(|w| truthy(&w["configured"]) && w["status"].as_str() != Some("running")) { "degraded" }
        else { "running" };
    let mut source_out = source.as_object().cloned().unwrap_or_default();
    source_out.remove("clocks");
    source_out.remove("hashes");
    let mut known = Map::new();
    for name in ["market_batch", "book_dirty", "market_settled"] {
        known.insert(name.into(), count(&realtime["transport"]["diagnostics"]["knownBroadcasts"][name]));
    }
    let collector_status = workers.iter().find(|w| w["name"].as_str() == Some("collector"))
        .map(|w| w["process"]["status"].clone()).unwrap_or(V::Null);
    Ok(json!({
        "schemaVersion": 1, "mode": "read_only_local_fleet_diagnostic",
        "checkedAt": ms_to_iso(now),
        "status": status_overall,
        "workers": workers,
        "source": V::Object(source_out),
        "officialContext": {
            "execution": {
                "status": status(&official["status"]),
                "updatedAt": execution_time,
                "ageMs": if !execution_time.is_null() && now >= parse(&execution_time) {
                    json!(now - parse(&execution_time)) } else { V::Null },
                "stopPresent": execution_stop["present"],
                "collectorProcessStatus": collector_status,
            },
            "realtime": {
                "updatedAt": realtime_time,
                "status": if realtime_markets_valid { realtime_health["status"].clone() } else { json!("invalid_schema") },
                "ageMs": realtime_health["heartbeatAgeMs"],
                "configuredMarkets": if realtime_markets_valid { realtime["markets"].as_array().map(|a| json!(a.len())).unwrap_or(V::Null) } else { V::Null },
                "readyMarkets": if realtime_markets_valid && realtime_health["status"].as_str() == Some("fresh") {
                    realtime["markets"].as_array().map(|a| json!(a.iter().filter(|m| m["ready"] == V::Bool(true)).count())).unwrap_or(V::Null)
                } else { V::Null },
                "marketStreamObserved": if realtime["marketStreamObserved"].is_boolean() { realtime["marketStreamObserved"].clone() } else { V::Null },
                "eventsReceived": count(&realtime["transport"]["eventsReceived"]),
                "knownBroadcasts": V::Object(known),
            },
        },
        "limitations": [
            "PID liveness alone does not prove process identity; missing metadata remains unverified.",
            "Reported bank identity is compared, but this diagnostic does not replay worker journals or establish current market readiness.",
            "Source delay compares journal append clocks, not market latency.",
            "No account bodies, balances, keys, STOP text or raw errors are exported.",
        ],
        "networkRequests": 0, "runtimeWrites": 0, "automaticRestarts": 0,
    }))
}
