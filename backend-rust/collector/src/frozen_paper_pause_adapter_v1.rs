// live-paper/frozen-paper-pause-adapter-v1.mjs — read-only pause import.
// Checks bytes and attribution, never replays economic reducers, creates locks,
// repairs tails, writes files or contacts the account.
use baal_paper::fixed::{big_amount, instant, money};
use baal_service_runtime::journal::{journal_lines, JournalReadOptions};
use baal_service_runtime::json_text::{js_int, js_stringify, js_truthy, sha256};
use num_bigint::BigInt;
use serde_json::{json, Map, Value as V};
use sha2::{Digest, Sha256};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use crate::model::digest;
use crate::paper_consumer_pause_v1::paper_consumer_pause_decision;

// execution/ledger.mjs payloadHash: sorted-key canonical JSON sha256.
fn payload_hash(v: &V) -> Result<String, String> {
    baal_service_runtime::ledger::payload_hash(v).map_err(|e| e.message)
}
fn need(ok: bool, c: &str) -> Result<(), String> {
    if !ok { return Err(format!("FROZEN_PAPER_PAUSE_{}", c)); } Ok(())
}
fn is_hex(v: &V) -> bool {
    v.as_str().is_some_and(|x| is_hex_s(x))
}
fn is_hex_s(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
// JS === on JSON values: numbers by value, else structural.
fn js_eq(a: &V, b: &V) -> bool {
    if let (Some(x), Some(y)) = (a.as_f64(), b.as_f64()) { return x == y; }
    a == b
}
fn js_eq_opt(a: Option<&V>, b: Option<&V>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => js_eq(x, y),
        _ => false,
    }
}
// A hashable value that must exist: canonicalJson(undefined) throws in JS.
fn req(v: Option<&V>) -> Result<V, String> {
    v.cloned().ok_or_else(|| "INVALID_INPUT: Canonical JSON requires finite JSON values".to_string())
}
// JS property read where the receiver may be null (TypeError) vs a primitive
// (undefined result for non-object keys).
fn prop<'a>(v: &'a V, k: &str) -> Result<Option<&'a V>, String> {
    match v {
        V::Object(m) => Ok(m.get(k)),
        V::Null => Err(format!("Cannot read properties of null (reading '{}')", k)),
        _ => Ok(None),
    }
}
// ToPropertyKey/String() for the JS `obj[v]` lookups done by this module.
fn to_key(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Number(_) => js_stringify(v),
        V::Bool(b) => b.to_string(),
        V::Null => "null".into(),
        V::Array(a) => a.iter()
            .map(|x| match x { V::Null => String::new(), _ => to_key(x) })
            .collect::<Vec<_>>().join(","),
        V::Object(_) => "[object Object]".into(),
    }
}
// Object.keys(v) for every JSON shape; the JS TypeError on null/undefined.
fn keys_or_err(v: Option<&V>) -> Result<Vec<String>, String> {
    match v {
        None | Some(V::Null) => Err("Cannot convert undefined or null to object".into()),
        Some(V::Object(m)) => Ok(m.keys().cloned().collect()),
        Some(V::Array(a)) => Ok((0..a.len()).map(|i| i.to_string()).collect()),
        Some(V::String(s)) => Ok((0..s.encode_utf16().count()).map(|i| i.to_string()).collect()),
        Some(_) => Ok(Vec::new()),
    }
}
fn entries_or_err(v: Option<&V>) -> Result<Vec<(String, V)>, String> {
    match v {
        None | Some(V::Null) => Err("Cannot convert undefined or null to object".into()),
        Some(V::Object(m)) => Ok(m.iter().map(|(k, x)| (k.clone(), x.clone())).collect()),
        Some(V::Array(a)) => Ok(a.iter().enumerate().map(|(i, x)| (i.to_string(), x.clone())).collect()),
        Some(V::String(s)) => Ok(s.encode_utf16().enumerate()
            .map(|(i, c)| (i.to_string(), json!(String::from_utf16_lossy(&[c])))).collect()),
        Some(_) => Ok(Vec::new()),
    }
}
fn values_or_err(v: Option<&V>) -> Result<Vec<V>, String> {
    entries_or_err(v).map(|e| e.into_iter().map(|(_, x)| x).collect())
}
// v.length for array/string; undefined for other JSON shapes; TypeError on
// null/undefined.
fn js_len(v: Option<&V>) -> Result<Option<i64>, String> {
    match v {
        None => Err("Cannot read properties of undefined (reading 'length')".into()),
        Some(V::Null) => Err("Cannot read properties of null (reading 'length')".into()),
        Some(V::Array(a)) => Ok(Some(a.len() as i64)),
        Some(V::String(s)) => Ok(Some(s.encode_utf16().count() as i64)),
        Some(_) => Ok(None),
    }
}
// `{...v}` own-enumerable spread into a fresh object.
fn spread_map(v: &V) -> Map<String, V> {
    let mut out = Map::new();
    match v {
        V::Object(m) => out.extend(m.iter().map(|(k, x)| (k.clone(), x.clone()))),
        V::Array(a) => out.extend(a.iter().enumerate().map(|(i, x)| (i.to_string(), x.clone()))),
        V::String(s) => out.extend(s.encode_utf16().enumerate()
            .map(|(i, c)| (i.to_string(), json!(String::from_utf16_lossy(&[c]))))),
        _ => {}
    }
    out
}
// RegExp.test(v) stringifies v first: String(v) for JSON shapes.
fn js_string(v: Option<&V>) -> String {
    match v {
        None => "undefined".into(),
        Some(x) => to_key(x),
    }
}

fn ns(sec: i64, nsec: i64) -> i128 { sec as i128 * 1_000_000_000 + nsec as i128 }
fn birth_ns(md: &fs::Metadata) -> i128 {
    md.created().ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as i128)
        .unwrap_or(0)
}
struct Stat { stamp: String, identity: String, size: u64 }
fn stat(f: &Path) -> Result<Stat, String> {
    let s = fs::metadata(f).map_err(|e| e.to_string())?;
    let is_sym = fs::symlink_metadata(f).map_err(|e| e.to_string())?.file_type().is_symlink();
    need(s.is_file() && !is_sym, "REGULAR_FILE")?;
    let b = birth_ns(&s);
    Ok(Stat {
        stamp: format!("{}:{}:{}:{}:{}:{}", s.dev(), s.ino(), s.len(),
            ns(s.mtime(), s.mtime_nsec()), ns(s.ctime(), s.ctime_nsec()), b),
        identity: format!("{}:{}:{}", s.dev(), s.ino(), b),
        size: s.len(),
    })
}
fn within(root: &Path, file: &Path) -> Result<PathBuf, String> {
    let actual = fs::canonicalize(file).map_err(|e| e.to_string())?;
    let base = fs::canonicalize(root).map_err(|e| e.to_string())?;
    let prefix = format!("{}{}", base.to_string_lossy(), std::path::MAIN_SEPARATOR);
    need(actual.to_string_lossy().starts_with(&prefix), "PATH_SCOPE")?;
    Ok(actual)
}
struct ReadDoc { value: V, hash: String, stamp: String, file: PathBuf }
fn read_json_doc(file: &Path, max: u64) -> Result<ReadDoc, String> {
    let before = stat(file)?;
    need(before.size <= max, "DOCUMENT_SIZE")?;
    let raw = fs::read(file).map_err(|e| e.to_string())?;
    need(stat(file)?.stamp == before.stamp, "DOCUMENT_CHANGED")?;
    let value: V = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
    Ok(ReadDoc { value, hash: sha256(&raw), stamp: before.stamp, file: file.to_path_buf() })
}

struct JournalVerified { last: V, stamp: String, file: PathBuf }
fn verify_journal(file: &Path, anchor: Option<&V>) -> Result<JournalVerified, String> {
    // JS anchor.bytes on undefined/null -> TypeError before the frontier need.
    let anchor = match anchor {
        None => return Err("Cannot read properties of undefined (reading 'bytes')".into()),
        Some(a) if a.is_null() => return Err("Cannot read properties of null (reading 'bytes')".into()),
        Some(a) => a,
    };
    let before = stat(file)?;
    need(js_int(&anchor["bytes"]).is_some_and(|b| b > 0 && b as u64 == before.size)
        && is_hex(&anchor["prefixHash"]) && is_hex(&anchor["hash"])
        && js_int(&anchor["sequence"]).is_some_and(|s| s > 0),
        "JOURNAL_FRONTIER")?;
    let anchor_bytes = js_int(&anchor["bytes"]).unwrap_or(0) as u64;
    let anchor_seq = js_int(&anchor["sequence"]).unwrap_or(-1);
    let anchor_hash = anchor["hash"].as_str().unwrap_or("");
    let anchor_prefix = anchor["prefixHash"].as_str().unwrap_or("");
    let mut bytes: u64 = 0;
    let mut seq: i64 = 0;
    let mut previous = String::new();
    let mut last = V::Null;
    let mut h = Sha256::new();
    for line in journal_lines(file, &JournalReadOptions { end_byte: Some(anchor_bytes), ..Default::default() })? {
        need(!line.is_empty(), "EMPTY_JOURNAL_RECORD")?;
        h.update(line.as_bytes());
        h.update(b"\n");
        bytes += line.len() as u64 + 1;
        let row: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        if row.is_null() {
            return Err("Cannot destructure property 'hash' of 'null' as it is null.".into());
        }
        let mut body = row.clone();
        let hash_v = body.as_object_mut().and_then(|m| m.shift_remove("hash"));
        let hash = match &hash_v { Some(V::String(s)) => s.clone(), _ => String::new() };
        seq += 1;
        need(js_stringify(&row) == line
            && js_int(&body["sequence"]) == Some(seq)
            && body["previous"].as_str() == Some(previous.as_str())
            && is_hex_s(&hash)
            && sha256(js_stringify(&body).as_bytes()) == hash,
            "ORIGINAL_JOURNAL_CHAIN")?;
        instant(body["at"].as_str().unwrap_or(""))?;
        previous = hash;
        last = body;
    }
    need(bytes == anchor_bytes
        && seq == anchor_seq
        && previous == anchor_hash
        && format!("{:x}", h.finalize()) == anchor_prefix
        && stat(file)?.stamp == before.stamp,
        "JOURNAL_CHANGED_OR_HASH")?;
    Ok(JournalVerified { last, stamp: before.stamp, file: file.to_path_buf() })
}

struct SourceVerified { file: PathBuf, identity: String }
fn verify_source(file: &Path, anchor: Option<&V>) -> Result<SourceVerified, String> {
    let anchor = match anchor {
        None => return Err("Cannot read properties of undefined (reading 'bytes')".into()),
        Some(a) if a.is_null() => return Err("Cannot read properties of null (reading 'bytes')".into()),
        Some(a) => a,
    };
    let before = stat(file)?;
    need(js_int(&anchor["bytes"]).is_some_and(|b| b > 0 && before.size >= b as u64)
        && is_hex(&anchor["prefixHash"]) && is_hex(&anchor["hash"])
        && js_int(&anchor["sequence"]).is_some_and(|s| s > 0),
        "SOURCE_FRONTIER")?;
    let anchor_bytes = js_int(&anchor["bytes"]).unwrap_or(0) as u64;
    let anchor_seq = js_int(&anchor["sequence"]).unwrap_or(-1);
    let anchor_hash = anchor["hash"].as_str().unwrap_or("");
    let anchor_prefix = anchor["prefixHash"].as_str().unwrap_or("");
    let mut bytes: u64 = 0;
    let mut seq: i64 = 0;
    let mut previous = String::new();
    let mut h = Sha256::new();
    for line in journal_lines(file, &JournalReadOptions { end_byte: Some(anchor_bytes), ..Default::default() })? {
        need(!line.is_empty(), "EMPTY_SOURCE_RECORD")?;
        h.update(line.as_bytes());
        h.update(b"\n");
        bytes += line.len() as u64 + 1;
        let row: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        if row.is_null() {
            return Err("Cannot destructure property 'hash' of 'null' as it is null.".into());
        }
        let mut body = row.clone();
        let hash_v = body.as_object_mut().and_then(|m| m.shift_remove("hash"));
        let hash = match &hash_v { Some(V::String(s)) => s.clone(), _ => String::new() };
        seq += 1;
        need(js_stringify(&row) == line
            && js_int(&body["sequence"]) == Some(seq)
            && body["previousHash"].as_str() == Some(previous.as_str())
            && is_hex_s(&hash)
            && payload_hash(&body)? == hash,
            "ORIGINAL_PUBLIC_CHAIN")?;
        instant(body["at"].as_str().unwrap_or(""))?;
        previous = hash;
    }
    need(bytes == anchor_bytes
        && seq == anchor_seq
        && previous == anchor_hash
        && format!("{:x}", h.finalize()) == anchor_prefix
        && stat(file)?.identity == before.identity
        && stat(file)?.size >= anchor_bytes,
        "SOURCE_CHANGED_OR_HASH")?;
    Ok(SourceVerified { file: file.to_path_buf(), identity: before.identity })
}

// payloadHash(Object.keys(a).sort()): JS .sort() orders by UTF-16 code units.
fn sorted_keys_hash(v: Option<&V>) -> Result<String, String> {
    let mut k = keys_or_err(v)?;
    k.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
    payload_hash(&json!(k))
}

fn verify_report(report: Option<&V>, paper: &V, bank: &V) -> Result<(), String> {
    // report?.bankHash===paper.bankHash — only that first access is optional.
    let first = js_eq_opt(report.and_then(|r| r.get("bankHash")), paper.get("bankHash"));
    if first && (report.is_none() || report.is_some_and(|r| r.is_null())) {
        return Err(format!("Cannot read properties of {} (reading 'bankHash')",
            if report.is_none() { "undefined" } else { "null" }));
    }
    let bank_digest = digest(bank);
    let paper_digest = digest(paper);
    if !(first
        && js_eq_opt(report.and_then(|r| r.get("bankHash")), Some(&json!(bank_digest)))
        && js_eq_opt(report.and_then(|r| r.get("stateHash")), Some(&json!(paper_digest)))
        && js_eq_opt(report.and_then(|r| r.get("executionAuthority")), Some(&V::Bool(false)))
        && report.and_then(|r| r.get("unknownAppendAcceptance")) != Some(&V::Bool(true))
        && js_eq_opt(report.and_then(|r| r.get("mode")), bank.get("mode"))
        && js_eq_opt(report.and_then(|r| r.get("cohort")), bank.get("name")))
    {
        need(false, "REPORT_STATE_BINDING")?;
    }
    let rk = sorted_keys_hash(report.and_then(|r| r.get("wallets")))?;
    let pk = sorted_keys_hash(paper.get("wallets"))?;
    need(rk == pk, "REPORT_STATE_BINDING")?;

    let report_v = req(report)?;
    for (k, w) in entries_or_err(paper.get("wallets"))? {
        if w.is_null() {
            return Err("Cannot read properties of null (reading 'cash')".into());
        }
        // r=report.wallets[k] — missing member yields the JS TypeError on .cash.
        let r = match prop(&report_v, "wallets")?.and_then(|x| x.get(&k)) {
            None => return Err("Cannot read properties of undefined (reading 'cash')".into()),
            Some(x) if x.is_null() => {
                return Err("Cannot read properties of null (reading 'cash')".into())
            }
            Some(x) => x,
        };
        let wallet_ok = (|| -> Result<bool, String> {
            if !js_eq_opt(r.get("cash"), w.get("cash")) { return Ok(false); }
            if !js_eq_opt(r.get("realizedPnl"), w.get("realizedPnl")) { return Ok(false); }
            if !js_eq_opt(r.get("cumulativeBuys"), w.get("cumulativeBuys")) { return Ok(false); }
            // r.fills===w.fills.length
            match js_len(w.get("fills"))? {
                Some(l) => if !js_eq_opt(r.get("fills"), Some(&json!(l))) { return Ok(false); },
                None => if r.get("fills").is_some() { return Ok(false); },
            }
            // r.closedTrades===w.fills.filter(f=>f.action==='sell').length
            let fills_arr = match w["fills"].as_array() {
                Some(a) => a,
                None => {
                    return Err(match w.get("fills") {
                        None => "Cannot read properties of undefined (reading 'filter')".into(),
                        Some(V::Null) => "Cannot read properties of null (reading 'filter')".into(),
                        Some(_) => "w.fills.filter is not a function".into(),
                    })
                }
            };
            let sells = fills_arr.iter()
                .filter(|f| f["action"].as_str() == Some("sell")).count() as i64;
            if !js_eq_opt(r.get("closedTrades"), Some(&json!(sells))) { return Ok(false); }
            // payloadHash(r.recentFills)===payloadHash(w.fills.slice(-25))
            let tail: Vec<V> = fills_arr[fills_arr.len().saturating_sub(25)..].to_vec();
            if !(payload_hash(&req(r.get("recentFills"))?)? == payload_hash(&json!(tail))?) {
                return Ok(false);
            }
            // payloadHash(r.pending)===payloadHash(Object.values(w.pending))
            if !(payload_hash(&req(r.get("pending"))?)?
                == payload_hash(&json!(values_or_err(w.get("pending"))?))?)
            {
                return Ok(false);
            }
            Ok(true)
        })();
        need(wallet_ok?, "WALLET_PROJECTION")?;

        // r.reservedCost===money(sum of buy-pending limits)
        let mut sum = BigInt::from(0);
        for p in values_or_err(w.get("pending"))? {
            if p["action"].as_str() == Some("buy") {
                sum += big_amount(p["limit"].as_str().unwrap_or(""))?;
            }
        }
        need(r.get("reservedCost") == Some(&json!(money(&sum))), "RESERVATION_PROJECTION")?;

        let positions_ok = (|| -> Result<bool, String> {
            let rp = match r.get("positions").and_then(|p| p.as_array()) {
                Some(a) => a,
                None => return Ok(false),
            };
            // new Set(r.positions.map(p=>p.marketId)).size===r.positions.length —
            // SameValueZero: primitives by value, objects/arrays by reference
            // (distinct JSON nodes are never equal).
            let mut seen: Vec<&V> = Vec::new();
            for p in rp {
                let m = &p["marketId"];
                match m {
                    V::Object(_) | V::Array(_) => {}
                    _ => {
                        if seen.iter().any(|x| js_eq(x, m)) { return Ok(false); }
                        seen.push(m);
                    }
                }
            }
            if rp.len() != keys_or_err(w.get("positions"))?.len() { return Ok(false); }
            Ok(true)
        })();
        need(positions_ok?, "POSITION_PROJECTION")?;
        let rp = r.get("positions").and_then(|p| p.as_array()).cloned().unwrap_or_default();
        for p in &rp {
            let held = match w.get("positions") {
                None => return Err(format!("Cannot read properties of undefined (reading '{}')",
                    to_key(&p["marketId"]))),
                Some(V::Null) => return Err(format!(
                    "Cannot read properties of null (reading '{}')", to_key(&p["marketId"]))),
                Some(x) => x.get(&to_key(&p["marketId"])),
            };
            let held = match held {
                Some(h) => h,
                None => return need(false, "FIFO_POSITION_PROJECTION"),
            };
            need(js_truthy(Some(held)), "FIFO_POSITION_PROJECTION")?;
            // p.instrument===bank.identities[p.marketId].exchangeId+':YES'
            let ident = match bank.get("identities") {
                None => return Err(format!("Cannot read properties of undefined (reading '{}')",
                    to_key(&p["marketId"]))),
                Some(V::Null) => return Err(format!(
                    "Cannot read properties of null (reading '{}')", to_key(&p["marketId"]))),
                Some(x) => x.get(&to_key(&p["marketId"])),
            };
            let ident = match ident {
                None => return Err("Cannot read properties of undefined (reading 'exchangeId')".into()),
                Some(x) if x.is_null() => {
                    return Err("Cannot read properties of null (reading 'exchangeId')".into())
                }
                Some(x) => x,
            };
            let ex = ident.get("exchangeId").map(to_key).unwrap_or_else(|| "undefined".to_string());
            need(js_eq(&p["instrument"], &json!(format!("{}:YES", ex)))
                && js_eq(&p["quantity"], &held["quantity"])
                && js_eq(&p["cost"], &held["cost"])
                && js_eq(&p["enteredAt"], &held["enteredAt"]),
                "FIFO_POSITION_PROJECTION")?;
        }
    }
    Ok(())
}

/// The object JS returns: data fields plus the status/consume/publish/focus/
/// close closures expressed as methods.
pub struct FrozenPaperPause {
    pub record: V,
    bank_hash: String,
    retained_risk: V,
    frozen_report: V,
}
impl FrozenPaperPause {
    /// status:()=>structuredClone(frozenReport)
    pub fn status(&self) -> V { self.frozen_report.clone() }
    /// consume:()=>0
    pub fn consume(&self) -> V { json!(0) }
    /// publish:()=>undefined
    pub fn publish(&self) {}
    /// focus(): retained-risk handle with no owned risk reads.
    pub fn focus(&self) -> V {
        json!({"bankHash":self.bank_hash,"markets":[],"ownedRiskReads":false,
            "paperComparisonPaused":true,"retainedRisk":self.retained_risk,
            "valuationStatus":"frozen_paper_snapshot_not_current","executionAuthority":false})
    }
    /// close:()=>undefined
    pub fn close(&self) {}
}

/// readFrozenPaperPause({runtimeDir,kind,pointerFile,cohortDirName,now})
pub fn read_frozen_paper_pause(input: &V) -> Result<Option<FrozenPaperPause>, String> {
    let runtime_dir = match input.get("runtimeDir") {
        Some(V::String(s)) => s.clone(),
        _ => return Err(format!("The \"path\" argument must be of type string. Received {}",
            if input.get("runtimeDir").is_none() { "undefined" } else { "an instance of Object" })),
    };
    let kind = input["kind"].as_str().unwrap_or("");
    let pointer_file = input["pointerFile"].as_str().unwrap_or("");
    let cohort_dir_name = input["cohortDirName"].as_str().unwrap_or("");
    let now = match input.get("now") {
        None => {
            let ms = std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0);
            baal_service_runtime::contract::ms_to_iso(ms)
        }
        Some(v) => v.as_str().unwrap_or("").to_string(),
    };
    need(Path::new(&runtime_dir).is_absolute()
        && kind == "settlement_portfolio_v2"
        && pointer_file == "settlement-portfolio-v2-active.json"
        && cohort_dir_name == "settlement-portfolio-v2-cohorts",
        "SUPPORTED_PAPER_SCOPE")?;
    let now_ms = instant(&now)?;
    let root = PathBuf::from(&runtime_dir);
    let manifest_file = root.join("paper-comparison-pauses.json");
    if !manifest_file.exists() { return Ok(None); }
    let manifest = read_json_doc(&within(&root, &manifest_file)?, 1024 * 1024)?;
    let mv = &manifest.value;
    need(js_int(&mv["schemaVersion"]) == Some(1)
        && mv["owner"].as_str() == Some("Codex")
        && mv["entries"].as_array().is_some_and(|e| e.len() <= 16),
        "MANIFEST")?;
    let entries = mv["entries"].as_array().cloned().unwrap_or_default();

    let pointer_path = root.join(pointer_file);
    if !pointer_path.exists() {
        let mut any_kind = false;
        for e in &entries {
            if js_eq_opt(prop(e, "kind")?, Some(&json!(kind))) { any_kind = true; }
        }
        need(!any_kind, "MISSING_PAUSED_POINTER")?;
        return Ok(None);
    }
    let pointer = read_json_doc(&within(&root, &pointer_path)?, 65536)?;
    let p = pointer.value.clone();
    // /^[-a-z0-9]{1,63}$/.test(p.cohort)
    let cohort_s = js_string(p.get("cohort"));
    need(js_int(&p["schemaVersion"]) == Some(1)
        && (1..=63).contains(&cohort_s.len())
        && cohort_s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && is_hex(&p["bankHash"]),
        "POINTER")?;
    let mut matches: Vec<V> = Vec::new();
    for e in &entries {
        if js_eq_opt(prop(e, "kind")?, Some(&json!(kind)))
            && js_eq_opt(prop(e, "cohort")?, p.get("cohort"))
        {
            matches.push(e.clone());
        }
    }
    need(matches.len() <= 1, "DUPLICATE_PAUSE")?;
    let entry = match matches.first() {
        None => return Ok(None),
        Some(e) => e.clone(),
    };
    // /^[a-z0-9][a-z0-9_.-]{0,100}\.json$/.test(entry.snapshotFile)
    let snap_name = js_string(prop(&entry, "snapshotFile")?);
    {
        let b = snap_name.as_bytes();
        let head_ok = !b.is_empty() && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit());
        let mid_ok = snap_name.ends_with(".json")
            && (6..=106).contains(&b.len())
            && b[1..b.len() - 5].iter()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()
                    || *c == b'_' || *c == b'.' || *c == b'-');
        need(head_ok && mid_ok && is_hex(prop(&entry, "snapshotFileHash")?.unwrap_or(&V::Null)),
            "SNAPSHOT_NAME")?;
    }
    // path.join(runtimeDir,cohortDirName,p.cohort) — p.cohort must be a string.
    if !p["cohort"].is_string() {
        return Err("The \"paths\" argument must be of type string or an instance of Buffer or URL. Received an instance of Number".into());
    }
    let dir = root.join(cohort_dir_name).join(&cohort_s);
    let bank_doc = read_json_doc(&within(&root, &dir.join("bank.json"))?, 4 * 1024 * 1024)?;
    let bank = bank_doc.value.clone();
    need(js_int(&bank["schemaVersion"]) == Some(2)
        && bank["mode"].as_str() == Some("funded_settlement_portfolio_paper_v2")
        && js_eq(&bank["name"], &p["cohort"])
        && digest(&bank) == p["bankHash"].as_str().unwrap_or(""),
        "IMMUTABLE_BANK")?;

    let snap = read_json_doc(&within(&dir, &dir.join(&snap_name))?, 16 * 1024 * 1024)?;
    let s = snap.value.clone();
    let snap_ok = (|| -> Result<bool, String> {
        if snap.hash != prop(&entry, "snapshotFileHash")?
            .and_then(|v| v.as_str()).unwrap_or("")
        {
            return Ok(false);
        }
        if !(prop(&s, "schemaVersion")?.and_then(js_int) == Some(1)
            && js_eq_opt(prop(&s, "kind")?, Some(&json!(kind)))
            && js_eq_opt(prop(&s, "cohort")?, p.get("cohort"))
            && js_eq_opt(prop(&s, "bankHash")?, p.get("bankHash"))
            && js_eq_opt(prop(&s, "bankFileHash")?, Some(&json!(bank_doc.hash)))
            && js_eq_opt(prop(&s, "pointerHash")?, Some(&json!(pointer.hash))))
        {
            return Ok(false);
        }
        let saved = prop(&s, "savedAt")?.and_then(|v| v.as_str()).unwrap_or("").to_string();
        Ok(instant(&saved)? <= now_ms)
    })();
    need(snap_ok?, "SNAPSHOT_BINDINGS")?;

    let journal = verify_journal(&within(&dir, &dir.join("journal.jsonl"))?,
        prop(&s, "journalAnchor")?)?;
    let state = journal.last.get("state").cloned().unwrap_or(V::Null);
    let state_paper = state.get("paper").cloned().unwrap_or(V::Null);
    let tail_ok = (|| -> Result<bool, String> {
        if !js_truthy(journal.last.get("event")) { return Ok(false); }
        // state?.paper — optional chain: null/undefined state yields undefined.
        if !js_truthy(state.get("paper")) { return Ok(false); }
        if !(payload_hash(&state_paper)? == payload_hash(&req(s.get("paperState"))?)?
            && js_eq_opt(state_paper.get("bankHash"), p.get("bankHash"))) {
            return Ok(false);
        }
        let sa = match prop(&s, "sourceAnchor")? {
            None => return Err("Cannot read properties of undefined (reading 'bytes')".into()),
            Some(x) if x.is_null() => {
                return Err("Cannot read properties of null (reading 'bytes')".into())
            }
            Some(x) => x,
        };
        Ok(js_eq_opt(state.get("sourceBytes"), prop(&sa, "bytes")?)
            && js_eq_opt(state.get("sourceSequence"), prop(&sa, "sequence")?)
            && js_eq_opt(state.get("sourceHash"), prop(&sa, "hash")?)
            && js_eq_opt(state.get("sourcePrefixHash"), prop(&sa, "prefixHash")?))
    })();
    need(tail_ok?, "EXACT_TAIL_PAPER_AND_SOURCE")?;

    let source = verify_source(&within(&root, &root.join("public-books.jsonl"))?,
        prop(&s, "sourceAnchor")?)?;
    verify_report(prop(&s, "report")?, &state_paper, &bank)?;

    let frontier_ok = (|| -> Result<bool, String> {
        let rep = req(prop(&s, "report")?)?;
        let ja = req(prop(&s, "journalAnchor")?)?;
        let sa = req(prop(&s, "sourceAnchor")?)?;
        Ok(js_eq_opt(rep.get("journalSequence"), prop(&ja, "sequence")?)
            && js_eq_opt(rep.get("journalHash"), prop(&ja, "hash")?)
            && js_eq_opt(rep.get("sourceBytes"), prop(&sa, "bytes")?)
            && js_eq_opt(rep.get("sourceSequence"), prop(&sa, "sequence")?)
            && js_eq_opt(rep.get("sourceHash"), prop(&sa, "hash")?)
            && js_eq_opt(rep.get("sourcePrefixHash"), prop(&sa, "prefixHash")?))
    })();
    need(frontier_ok?, "REPORT_FRONTIER")?;

    let decision = paper_consumer_pause_decision(&json!({
        "pauseRecord": prop(&entry, "pauseRecord")?.cloned().unwrap_or(V::Null),
        "consumerKind":"counterfactual_paper","cohort":p["cohort"],
        "bankHash":p["bankHash"],"report":prop(&s, "report")?,
        "journalAnchor":prop(&s, "journalAnchor")?,
        "sourceAnchor":prop(&s, "sourceAnchor")?,
        "accountingVerified":true,"now":now}));
    need(decision["paused"] == V::Bool(true), "EXPLICIT_OWNED_PAUSE")?;

    for d in [&manifest, &pointer, &bank_doc, &snap] {
        need(stat(&d.file)?.stamp == d.stamp, "DOCUMENT_CHANGED_DURING_VERIFY")?;
    }
    need(stat(&journal.file)?.stamp == journal.stamp
        && stat(&source.file)?.identity == source.identity,
        "SOURCE_REPLACED_DURING_VERIFY")?;

    // frozenReport={...structuredClone(s.report),...}
    let mut fr = spread_map(&req(prop(&s, "report")?)?);
    fr.insert("paperComparisonPaused".into(), V::Bool(true));
    fr.insert("paperPause".into(), decision.clone());
    fr.insert("valuationStatus".into(), json!("frozen_paper_snapshot_not_current"));
    let pr = prop(&entry, "pauseRecord")?.cloned().unwrap_or(V::Null);
    fr.insert("pausedAt".into(), pr.get("createdAt").cloned().unwrap_or(V::Null));
    fr.insert("originalReportHash".into(), json!(payload_hash(&req(prop(&s, "report")?)?)?));
    fr.insert("persistentWorker".into(), V::Bool(false));
    fr.insert("executionAuthority".into(), V::Bool(false));
    let frozen_report = V::Object(fr);

    let mut rec = spread_map(&decision);
    rec.insert("kind".into(), json!(kind));
    rec.insert("cohort".into(), p["cohort"].clone());
    rec.insert("snapshotFileHash".into(), json!(snap.hash));
    rec.insert("verificationMode".into(),
        json!("full_original_bytes_hashchains_and_tail_accounting_no_economic_replay"));
    rec.insert("economicReplayPerformed".into(), V::Bool(false));
    rec.insert("readOnly".into(), V::Bool(true));
    rec.insert("networkRequests".into(), json!(0));
    rec.insert("competitionOrders".into(), json!(0));
    rec.insert("report".into(), frozen_report.clone());
    rec.insert("paperState".into(), state_paper.clone());
    Ok(Some(FrozenPaperPause {
        record: V::Object(rec),
        bank_hash: p["bankHash"].as_str().unwrap_or("").to_string(),
        retained_risk: decision["retainedRisk"].clone(),
        frozen_report,
    }))
}
