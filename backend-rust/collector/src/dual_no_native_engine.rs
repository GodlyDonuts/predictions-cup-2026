// live-paper/dual-no-native-engine.mjs — pure offline engine transport over a
// pinned JSONL child. Durability, ownership, publication frontiers and
// continuation identity belong to its integrating host, never this child.
use serde_json::{json, Value as V};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use baal_execution::core::payload_hash;
use baal_paper::fixed::{big_amount as amount, instant};
use baal_service_runtime::json_text::{js_int, js_stringify, js_truthy, sha256 as sha};
use num_bigint::BigInt;

use crate::dual_no_pair_paper_v2::{dual_no_pair_policy_v2, dual_no_pair_report_v2};
use crate::dual_no_review_v1::validate_dual_no_review;
use crate::native_dip_host_v1::jstr;
use crate::native_journal_append::{birthtime_ns, path_resolve, sorted_keys, PipeChild, PipeEvent};

fn need(ok: bool, c: &str) -> Result<(), String> {
    if !ok { return Err(format!("DUAL_NO_NATIVE_{}", c)); }
    Ok(())
}
fn hex64(v: &V) -> bool {
    v.as_str().map(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())).unwrap_or(false)
}
fn object(v: &V) -> bool { v.is_object() }

const SOURCE_FILES: [&str; 4] = ["Cargo.toml", "Cargo.lock", "src/lib.rs", "src/main.rs"];
/// stateKeys sorted.
const STATE_KEYS: &str = "anchor|bankHash|books|cash|counts|cumulativeBuys|fills|lastAt|nextIntentId|nextPackageId|package|pending|positions|realizedPnl|roundtrips";

/// signature(file): ino:birthtimeMs:size:mtimeMs:ctimeMs — JS Number ms values.
fn signature(file: &Path) -> Result<String, String> {
    let s = std::fs::metadata(file).map_err(|e| e.to_string())?;
    need(s.is_file(), "FILE")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let ms = |ns: u64| -> String {
            let mut b = ryu_js::Buffer::new();
            b.format_finite(ns as f64 / 1_000_000.0).to_string()
        };
        let btime = birthtime_ns(&s);
        return Ok(format!("{}:{}:{}:{}:{}", s.ino(), ms(btime), s.size(),
            ms(s.mtime() as u64 * 1_000_000_000 + s.mtime_nsec() as u64),
            ms(s.ctime() as u64 * 1_000_000_000 + s.ctime_nsec() as u64)));
    }
    #[cfg(not(unix))]
    {
        let _ = s;
        Ok(format!("0:{}:{}:{}:{}", ms_of_created(&s), s.len(), "0", "0"))
    }
}
#[cfg(not(unix))]
fn ms_of_created(s: &std::fs::Metadata) -> String {
    let mut b = ryu_js::Buffer::new();
    b.format_finite(s.created().ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as f64).unwrap_or(0.0)).to_string()
}

/// dualNoNativeBinding({binaryPath, sourceRoot, dependencies}).
pub fn dual_no_native_binding(binary_path: &Path, source_root: &Path, dependencies: &[&str])
    -> Result<V, String> {
    need(binary_path.is_absolute() && source_root.is_absolute(), "PATH")?;
    let mut sources = serde_json::Map::new();
    for file in SOURCE_FILES {
        sources.insert(file.to_string(),
            json!(sha(&std::fs::read(source_root.join(file)).map_err(|e| e.to_string())?)));
    }
    Ok(json!({
        "schemaVersion": 1, "mode": "generic_dual_no_v2_jsonl_worker",
        "binaryHash": sha(&std::fs::read(binary_path).map_err(|e| e.to_string())?),
        "engineHash": payload_hash(&V::Object(sources.clone()))?,
        "sources": V::Object(sources),
        "dependencies": dependencies.iter().map(|file| {
            let p = path_resolve(file);
            std::fs::read(&p).map(|b| json!({"path": p.to_string_lossy(), "hash": sha(&b)}))
        }).collect::<Result<Vec<_>, _>>().map_err(|e: std::io::Error| e.to_string())?,
    }))
}

fn verify_binding(binary_path: &Path, source_root: &Path, binding: &V)
    -> Result<Vec<(PathBuf, String)>, String> {
    let mut sorted: Vec<&str> = SOURCE_FILES.to_vec();
    sorted.sort();
    need(binding["schemaVersion"].as_i64() == Some(1)
        && binding["mode"].as_str() == Some("generic_dual_no_v2_jsonl_worker")
        && hex64(&binding["binaryHash"]) && hex64(&binding["engineHash"])
        && object(&binding["sources"])
        && sorted_keys(&binding["sources"]) == sorted.join("|")
        && binding["dependencies"].is_array(), "BINDING")?;
    need(sha(&std::fs::read(binary_path).map_err(|e| e.to_string())?) == binding["binaryHash"].as_str().unwrap_or("")
        && payload_hash(&binding["sources"])? == binding["engineHash"].as_str().unwrap_or(""),
        "BINARY_OR_ENGINE")?;
    let mut files = vec![binary_path.to_path_buf()];
    for file in SOURCE_FILES {
        let target = source_root.join(file);
        need(hex64(&binding["sources"][file])
            && sha(&std::fs::read(&target).map_err(|e| e.to_string())?)
                == binding["sources"][file].as_str().unwrap_or(""), "SOURCE_HASH")?;
        files.push(target);
    }
    for d in binding["dependencies"].as_array().cloned().unwrap_or_default() {
        let dp = PathBuf::from(d["path"].as_str().unwrap_or(""));
        need(dp.is_absolute() && hex64(&d["hash"])
            && sha(&std::fs::read(&dp).map_err(|e| e.to_string())?) == d["hash"].as_str().unwrap_or(""),
            "DEPENDENCY_HASH")?;
        files.push(dp);
    }
    files.iter().map(|f| signature(f).map(|s| (f.clone(), s))).collect()
}

/// validateState(s, b, bankHash) — returns s.
pub fn validate_state(s: &V, b: &V, bank_hash: &str) -> Result<V, String> {
    need(object(s) && sorted_keys(s) == STATE_KEYS
        && s["bankHash"].as_str() == Some(bank_hash), "STATE_BINDING")?;
    instant(&jstr(&s["lastAt"]))?;
    need(instant(&jstr(&s["lastAt"]))? >= instant(&jstr(&b["cutoff"]))?
        && object(&s["positions"]) && object(&s["books"]) && object(&s["counts"])
        && s["fills"].is_array() && s["roundtrips"].is_array(), "STATE_SCHEMA")?;
    // Set of raw exchangeId values — keyed by canonical JSON so a numeric id
    // never aliases its string form (JS Set + === semantics).
    let scope_vals: Vec<V> = {
        let mut seen = BTreeSet::new();
        b["review"]["legs"].as_array().cloned().unwrap_or_default()
            .iter().filter(|l| seen.insert(js_stringify(&l["exchangeId"])))
            .map(|l| l["exchangeId"].clone()).collect()
    };
    let scope: BTreeSet<String> = scope_vals.iter().map(js_stringify).collect();
    let mut quantities: BTreeMap<String, i64> = BTreeMap::new();
    let mut buys = BigInt::from(0);
    let mut sells = BigInt::from(0);
    let mut cost = BigInt::from(0);
    let mut reserved = BigInt::from(0);
    for f in s["fills"].as_array().cloned().unwrap_or_default() {
        need(object(&f) && scope.contains(&js_stringify(&f["exchangeId"]))
            && f["quantity"].as_i64() == Some(1)
            && ["buy", "sell"].contains(&f["action"].as_str().unwrap_or(""))
            && f["fees"].as_str() == Some("0.000000"), "FILL_SCOPE")?;
        let price = amount(&jstr(&f["price"]))?;
        need(price >= BigInt::from(5000) && price <= BigInt::from(995000)
            && &price % BigInt::from(5000) == BigInt::from(0), "FILL_PRICE")?;
        let ex = js_stringify(&f["exchangeId"]);
        let q = quantities.get(&ex).copied().unwrap_or(0)
            + if f["action"].as_str() == Some("buy") { 1 } else { -1 };
        need(q >= 0, "OVERSELL")?;
        quantities.insert(ex, q);
        if f["action"].as_str() == Some("buy") { buys += &price; } else { sells += &price; }
        instant(&jstr(&f["at"]))?;
    }
    for (id, p) in s["positions"].as_object().cloned().unwrap_or_default() {
        let leg_mid = b["review"]["legs"].as_array().cloned().unwrap_or_default()
            .iter().find(|l| l["exchangeId"].as_str() == Some(id.as_str()))
            .map(|l| l["marketId"].clone())
            .ok_or("Cannot read properties of undefined (reading 'marketId')")?;
        need(scope.contains(&js_stringify(&json!(id))) && object(&p)
            && p["exchangeId"].as_str() == Some(id.as_str())
            && p["marketId"] == leg_mid
            && p["quantity"].as_i64() == Some(1)
            && quantities.get(&js_stringify(&json!(id))).copied() == Some(1), "POSITION")?;
        let value = amount(&jstr(&p["cost"]))?;
        need(value >= BigInt::from(5000) && value <= BigInt::from(995000), "POSITION_COST")?;
        cost += &value;
        instant(&jstr(&p["enteredAt"]))?;
    }
    for raw_id in &scope_vals {
        let id = jstr(raw_id);
        let pos_q = s["positions"][id.as_str()].get("quantity").and_then(V::as_i64).unwrap_or(0);
        need(pos_q == quantities.get(&js_stringify(raw_id)).copied().unwrap_or(0), "INVENTORY")?;
    }
    if !s["pending"].is_null() {
        let p = &s["pending"];
        need(object(p) && scope.contains(&js_stringify(&p["exchangeId"]))
            && p["quantity"].as_i64() == Some(1)
            && ["buy", "sell"].contains(&p["action"].as_str().unwrap_or(""))
            && js_int(&p["id"]).map_or(false, |v| v > 0), "PENDING")?;
        instant(&jstr(&p["createdAt"]))?;
        instant(&jstr(&p["expiresAt"]))?;
        let limit = amount(&jstr(&p["limit"]))?;
        need(limit >= BigInt::from(5000) && limit <= BigInt::from(995000)
            && &limit % BigInt::from(5000) == BigInt::from(0), "PENDING_LIMIT")?;
        let has_position = s["positions"].get(&jstr(&p["exchangeId"]))
            .map(|v| js_truthy(Some(v))).unwrap_or(false);
        need(if p["action"].as_str() == Some("buy") { !has_position } else { has_position },
            "COVERED_OR_DUPLICATE")?;
        if p["action"].as_str() == Some("buy") {
            reserved = if p["stage"].as_str() == Some("entry_first") {
                s["package"]["entryLimits"].as_object().cloned().unwrap_or_default()
                    .values().try_fold(BigInt::from(0), |a, v| {
                        amount(&jstr(v)).map(|x| a + x)
                    })?
            } else { limit };
            need(reserved > BigInt::from(0), "RESERVATION")?;
        }
    }
    need(amount(&jstr(&s["cash"]))? == amount(&jstr(&b["policy"]["initialCash"]))? - &buys + &sells
        && amount(&jstr(&s["cash"]))? >= reserved
        && amount(&jstr(&s["realizedPnl"]))? == &sells - &buys + &cost
        && amount(&jstr(&s["cumulativeBuys"]))? == buys, "CASH_FIFO")?;
    need(&cost + &reserved <= amount(&jstr(&b["policy"]["maxOwnedAndReserved"]))?
        && &buys + &reserved <= amount(&jstr(&b["policy"]["cumulativeBuyBudget"]))?, "CAPITAL")?;
    need(js_truthy(s.get("package")) == (cost > BigInt::from(0) || !s["pending"].is_null()), "PACKAGE")?;
    for v in s["counts"].as_object().cloned().unwrap_or_default().values() {
        need(js_int(v).map_or(false, |x| x >= 0), "COUNTS")?;
    }
    need(js_int(&s["nextIntentId"]).map_or(false, |v| v > 0)
        && js_int(&s["nextPackageId"]).map_or(false, |v| v > 0)
        && js_int(&s["cooldownUntil"]).is_some(), "STATE_COUNTERS")?;
    Ok(s.clone())
}

/// openDualNoNativeEngine result — the engine adapter.
pub struct DualNoNativeEngine {
    pub binding: V,
    pub bank_hash: String,
    pub pid: u32,
    files: Vec<(PathBuf, String)>,
    frozen: V,
    current: Option<V>,
    operations: u64,
    closed: bool,
    failed: Option<String>,
    child: PipeChild,
    timeout: Duration,
    tail: Arc<Mutex<()>>,
}

impl DualNoNativeEngine {
    fn fail(&mut self, code: &str) -> String {
        if self.failed.is_none() {
            self.failed = Some(format!("DUAL_NO_NATIVE_{}", code));
            self.child.kill();
        }
        self.failed.clone().unwrap()
    }
    fn health(&mut self) -> Result<(), String> {
        need(!self.closed && self.failed.is_none() && self.child.alive(), "UNAVAILABLE")?;
        let checks: Result<(), String> = (|| {
            for (f, s) in &self.files {
                need(signature(f)? == *s, "BINDING_FILE_CHANGED")?;
            }
            Ok(())
        })();
        if checks.is_err() {
            let msg = self.fail("BINDING_FILE_CHANGED");
            return Err(msg);
        }
        Ok(())
    }
    /// status()
    pub fn status(&self) -> V {
        json!({"pid": self.pid, "closed": self.closed,
            "failed": self.failed.clone().map_or(V::Null, V::String),
            "operations": self.operations, "bankHash": self.bank_hash,
            "binaryHash": self.binding["binaryHash"], "engineHash": self.binding["engineHash"],
            "networkRequests": 0, "contestOrders": 0})
    }
    fn request<F>(&mut self, command: &V, validate: F) -> Result<V, String>
    where F: FnOnce(&mut Self, &V) -> Result<V, String> {
        let _tail = self.tail.clone();
        let _g = _tail.lock().unwrap_or_else(|e| e.into_inner());
        self.health()?;
        self.operations += 1;
        if let Some(ev) = self.child.drain_event() {
            return Err(match ev {
                PipeEvent::Line(_) => self.fail("UNSOLICITED_RESPONSE"),
                PipeEvent::Stderr(_) => self.fail("UNEXPECTED_STDERR"),
                PipeEvent::Overflow => self.fail("RESPONSE_TOO_LARGE"),
                PipeEvent::Eof => self.fail("PROCESS_DIED"),
            });
        }
        if self.child.write_line(&js_stringify(command)).is_err() {
            return Err(self.fail("STDIN_FAILED"));
        }
        let line = loop {
            match self.child.recv(self.timeout) {
                Ok(PipeEvent::Line(l)) => break l,
                Ok(PipeEvent::Stderr(_)) => return Err(self.fail("UNEXPECTED_STDERR")),
                Ok(PipeEvent::Overflow) => return Err(self.fail("RESPONSE_TOO_LARGE")),
                Ok(PipeEvent::Eof) => return Err(self.fail("PROCESS_DIED")),
                Err(()) => return Err(self.fail("RESPONSE_TIMEOUT")),
            }
        };
        let response: V = match serde_json::from_str(&line) {
            Ok(v) if object(&v) && v.as_object().unwrap().len() == 1 => v,
            _ => return Err(self.fail("MALFORMED_RESPONSE")),
        };
        self.health()?;
        if js_truthy(response.get("error")) {
            let ok = response["error"].as_str()
                .map(|e| !e.is_empty() && e.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_'))
                .unwrap_or(false);
            if !ok {
                let msg = self.fail("MALFORMED_ERROR");
                return Err(msg);
            }
            return Err(format!("DUAL_NO_NATIVE_REJECTED_{}", response["error"].as_str().unwrap_or("")));
        }
        match validate(self, &response) {
            Ok(v) => Ok(v),
            Err(e) => { self.fail("RESULT_VALIDATION"); Err(e) }
        }
    }
    fn state_response(&mut self, r: &V) -> Result<V, String> {
        need(r.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>().join("|"))
            .unwrap_or_default() == "state", "STATE_RESPONSE")?;
        let next = validate_state(&r["state"], &self.frozen, &self.bank_hash)?;
        self.current = Some(next.clone());
        Ok(next)
    }
    /// engine.init()
    pub fn init(&mut self) -> Result<V, String> {
        let frozen = self.frozen.clone();
        self.request(&json!({"op": "init", "bank": frozen}), |s, r| s.state_response(r))
    }
    /// engine.restore(state)
    pub fn restore(&mut self, state: &V) -> Result<V, String> {
        let frozen = self.frozen.clone();
        validate_state(state, &frozen, &self.bank_hash)?;
        self.request(&json!({"op": "restore", "bank": frozen, "state": state}),
            |s, r| s.state_response(r))
    }
    /// engine.advance(event)
    pub fn advance(&mut self, event: &V) -> Result<V, String> {
        instant(&jstr(&event["availableAt"]))?;
        let available_at = event["availableAt"].clone();
        self.request(&json!({"op": "advance", "event": event}), |s, r| {
            let st = s.state_response(r)?;
            need(st["lastAt"] == available_at, "EVENT_CLOCK")?;
            Ok(st)
        })
    }
    /// engine.batch(events)
    pub fn batch(&mut self, events: &[V]) -> Result<V, String> {
        need(!events.is_empty(), "EVENTS")?;
        for e in events { instant(&jstr(&e["availableAt"]))?; }
        let last_at = events.last().map(|e| e["availableAt"].clone()).unwrap_or(V::Null);
        self.request(&json!({"op": "batch", "events": events}), |s, r| {
            let st = s.state_response(r)?;
            need(st["lastAt"] == last_at, "BATCH_CLOCK")?;
            Ok(st)
        })
    }
    /// engine.report(at)
    pub fn report(&mut self, at: &str) -> Result<V, String> {
        instant(at)?;
        let at = at.to_string();
        self.request(&json!({"op": "report", "at": at}), move |s, r| {
            let x = &r["report"];
            let current = s.current.clone().unwrap_or(V::Null);
            need(r.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>().join("|"))
                .unwrap_or_default() == "report"
                && object(x)
                && x["bankHash"].as_str() == Some(s.bank_hash.as_str())
                && x["cohort"] == s.frozen["name"]
                && x["updatedAt"].as_str() == Some(at.as_str())
                && x["executionAuthority"].as_bool() == Some(false)
                && x["networkRequests"].as_i64() == Some(0)
                && x["competitionOrders"].as_i64() == Some(0)
                && x["profitabilityEstablished"].as_bool() == Some(false)
                && x["fees"].as_str() == Some("0.000000")
                && x["cash"] == current["cash"]
                && x["realizedPnl"] == current["realizedPnl"]
                && x["fills"].as_i64() == current["fills"].as_array().map(|a| a.len() as i64)
                && payload_hash(&x["pending"]).unwrap_or_default()
                    == payload_hash(&current["pending"]).unwrap_or_default()
                && payload_hash(&x["package"]).unwrap_or_default()
                    == payload_hash(&current["package"]).unwrap_or_default(), "REPORT_BINDING")?;
            need(payload_hash(x).unwrap_or_default()
                == dual_no_pair_report_v2(&current, &s.frozen, &at)
                    .and_then(|v| payload_hash(&v)).unwrap_or_default(), "REPORT_PARITY")?;
            Ok(x.clone())
        })
    }
    /// engine.close()
    pub fn close(&mut self) {
        if self.closed { return; }
        self.closed = true;
        self.child.shutdown();
    }
}
impl Drop for DualNoNativeEngine {
    fn drop(&mut self) { self.close(); }
}

/// openDualNoNativeEngine({binaryPath, sourceRoot, binding, bank, timeoutMs,
/// maxResponseBytes}) — verifies the binding, spawns the child, runs init.
pub fn open_dual_no_native_engine(binary_path: &Path, source_root: &Path, binding: &V, bank: &V,
    timeout_ms: i64, max_response_bytes: usize) -> Result<DualNoNativeEngine, String> {
    need(binary_path.is_absolute() && source_root.is_absolute()
        && timeout_ms > 0 && max_response_bytes > 0, "OPTIONS")?;
    validate_dual_no_review(&bank["review"])?;
    need(bank["mode"].as_str() == Some("dual_no_sequential_paper_v2_exact_receipt")
        && payload_hash(&bank["policy"])? == payload_hash(&dual_no_pair_policy_v2())?, "BANK")?;
    instant(&jstr(&bank["cutoff"]))?;
    let frozen = bank.clone();
    let bank_hash = payload_hash(&frozen)?;
    let bound = binding.clone();
    let files = verify_binding(binary_path, source_root, &bound)?;
    let sysroot = std::env::var("SystemRoot").or_else(|_| std::env::var("WINDIR")).unwrap_or_default();
    let dep_dirs: Vec<String> = bound["dependencies"].as_array().cloned().unwrap_or_default()
        .iter().filter_map(|d| Path::new(d["path"].as_str().unwrap_or("")).parent()
            .map(|p| p.to_string_lossy().to_string()))
        .collect();
    let mut path_parts = vec![binary_path.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default()];
    path_parts.extend(dep_dirs);
    path_parts.push(Path::new(if sysroot.is_empty() { "C:/Windows" } else { &sysroot })
        .join("System32").to_string_lossy().to_string());
    let env = vec![
        ("SystemRoot".to_string(), sysroot.clone()),
        ("WINDIR".to_string(), std::env::var("WINDIR").or_else(|_| std::env::var("SystemRoot")).unwrap_or_default()),
        ("PATH".to_string(), path_parts.join(":")),
    ];
    let child = PipeChild::spawn(binary_path, &[], Some(env), max_response_bytes)
        .map_err(|_| "DUAL_NO_NATIVE_PROCESS_UNAVAILABLE".to_string())?;
    let pid = child.child.id();
    let mut engine = DualNoNativeEngine {
        binding: bound, bank_hash, pid, files, frozen, current: None,
        operations: 0, closed: false, failed: None, child,
        timeout: Duration::from_millis(timeout_ms as u64), tail: Arc::new(Mutex::new(())),
    };
    match engine.init() {
        Ok(_) => Ok(engine),
        Err(e) => { engine.close(); Err(e) }
    }
}
