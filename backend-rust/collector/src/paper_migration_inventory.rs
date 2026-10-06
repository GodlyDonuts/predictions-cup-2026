// live-paper/paper-migration-inventory.mjs — current-reducer manifest snapshot.
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::{js_int, sha256};
use serde_json::{json, Value as V};
use std::collections::BTreeSet;
fn js_str(v: &V) -> String {
    match v { V::String(s) => s.clone(), V::Number(n) => n.to_string(), V::Bool(b) => b.to_string(), _ => String::new() }
}
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

// These are the current reducers, not every archived experiment. Archives are
// retained separately and must not be discarded by a runtime migration.
pub const CURRENT_PAPER_POINTERS: [(&str, &str); 9] = [
    ("probability-receipt-active.json", "probability-receipt-cohorts"),
    ("calibrated-poll-paper-active.json", "probability-receipt-cohorts"),
    ("primary-breadth-paper-active.json", "probability-receipt-cohorts"),
    ("settlement-paper-active.json", "settlement-paper-cohorts"),
    ("settlement-portfolio-v2-active.json", "settlement-portfolio-v2-cohorts"),
    ("dual-no-pair-active.json", "dual-no-pair-cohorts"),
    ("dual-no-pair-v2-active.json", "dual-no-pair-v2-cohorts"),
    ("coupled-bulk-active.json", "coupled-bulk-cohorts"),
    ("settlement-portfolio-v3-active.json", "settlement-portfolio-v3-cohorts"),
];
fn need(v: bool, c: &str) -> Result<(), String> {
    if !v { return Err(format!("PAPER_INVENTORY_{c}")); }
    Ok(())
}
fn sha(b: &[u8]) -> String { sha256(b) }
fn hash_file(file: &Path, end: u64) -> Result<String, String> {
    let mut h = sha2::Sha256::new();
    use sha2::Digest;
    let mut f = fs::File::open(file).map_err(|e| e.to_string())?;
    let mut remaining = end;
    let mut buf = vec![0u8; 1024 * 1024];
    while remaining > 0 {
        let chunk = remaining.min(buf.len() as u64) as usize;
        let n = f.read(&mut buf[..chunk]).map_err(|e| e.to_string())?;
        need(n > 0, "TRUNCATED_PREFIX")?;
        h.update(&buf[..n]);
        remaining -= n as u64;
    }
    Ok(format!("{:x}", h.finalize()))
}
/// tail(file): last complete row of a JSONL journal + the byte offset just past
/// its newline.
fn tail(file: &Path) -> Result<(V, u64), String> {
    let size = fs::metadata(file).map_err(|e| e.to_string())?.len();
    need(size > 0, "EMPTY_JOURNAL")?;
    let n = size.min(8 * 1024 * 1024);
    let mut buf = vec![0u8; n as usize];
    let mut f = fs::File::open(file).map_err(|e| e.to_string())?;
    use std::io::{Seek, SeekFrom};
    f.seek(SeekFrom::Start(size - n)).map_err(|e| e.to_string())?;
    f.read_exact(&mut buf).map_err(|e| {
        if e.kind() == std::io::ErrorKind::UnexpectedEof { "TRUNCATED_TAIL".to_string() } else { e.to_string() }
    })?;
    let Some(end) = buf.iter().rposition(|&b| b == 10) else { return Err("INCOMPLETE_JOURNAL".into()) };
    let start = buf[..end].iter().rposition(|&b| b == 10).map(|i| i + 1).unwrap_or(0);
    need(start > 0 || n == size, "OVERSIZE_ROW")?;
    let row: V = serde_json::from_slice(&buf[start..end]).map_err(|e| e.to_string())?;
    Ok((row, size - n + end as u64 + 1))
}
fn optional_hash(file: &Path) -> V {
    if file.exists() { fs::read(file).map(|b| json!(sha(&b))).unwrap_or(V::Null) } else { V::Null }
}
fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// capturePaperMigrationInventory({runtimeDir, now=Date.now, maximumAgeMs=30000,
/// pointers=CURRENT_PAPER_POINTERS})
pub fn capture_paper_migration_inventory(options: &V) -> Result<V, String> {
    let runtime_dir = options.get("runtimeDir").and_then(V::as_str).unwrap_or("").to_string();
    // `now` may be a fixed ms value; absent it behaves like Date.now per call.
    let fixed_now = options.get("now").map(|v| js_int(v));
    let clock = move || fixed_now.unwrap_or_else(|| Some(now_ms()));
    let max_age = options.get("maximumAgeMs").map(|v| js_int(v).unwrap_or(i64::MIN)).unwrap_or(30000);
    let pointers: Vec<(String, String)> = match options.get("pointers") {
        Some(V::Array(a)) => a.iter().map(|p| (
            p[0].as_str().unwrap_or("").to_string(), p[1].as_str().unwrap_or("").to_string())).collect(),
        _ => CURRENT_PAPER_POINTERS.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
    };
    need(clock().is_some_and(|x| js_int(&json!(x)).is_some()) && max_age != i64::MIN && max_age > 0, "CLOCK")?;
    let runtime = PathBuf::from(&runtime_dir);
    let mut current = Vec::new();
    for (pointer, family) in &pointers {
        need(CURRENT_PAPER_POINTERS.iter().any(|(p, f)| p == pointer && f == family), "POINTER_SCOPE")?;
        let pointer_file = runtime.join(pointer);
        let pointer_bytes = fs::read(&pointer_file).map_err(|e| e.to_string())?;
        let p: V = serde_json::from_slice(&pointer_bytes).map_err(|e| e.to_string())?;
        let cohort_ok = p["cohort"].as_str().is_some_and(|s| {
            !s.is_empty() && s.len() <= 63
                && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        });
        need(cohort_ok, "COHORT_SCOPE")?;
        let base = runtime.join(family).join(p["cohort"].as_str().unwrap_or(""));
        let bank_bytes = fs::read(base.join("bank.json")).map_err(|e| e.to_string())?;
        let bank: V = serde_json::from_slice(&bank_bytes).map_err(|e| e.to_string())?;
        let bank_hash_method = if ["dual-no-pair-cohorts", "dual-no-pair-v2-cohorts"].contains(&family.as_str()) {
            "canonical_payload"
        } else { "model_digest" };
        let bank_hash = if bank_hash_method == "canonical_payload" {
            baal_execution::core::payload_hash(&bank)?
        } else {
            crate::model::digest(&bank)
        };
        need(bank_hash == p["bankHash"].as_str().unwrap_or(""), "BANK_IDENTITY")?;
        let overlay = base.join("operator-value-revision");
        let has_overlay = overlay.join("opening.json").exists() || overlay.join("journal.jsonl").exists();
        let dir = if has_overlay { overlay.clone() } else { base.clone() };
        if has_overlay {
            need(overlay.join("opening.json").exists() && overlay.join("journal.jsonl").exists(),
                "INCOMPLETE_OVERLAY")?;
        }
        let native_observer = family == "settlement-portfolio-v3-cohorts";
        let journal_file = dir.join(if native_observer { "native-observer.jsonl" } else { "journal.jsonl" });
        let mut stats: Option<V> = None;
        let mut receipt_row = V::Null;
        let mut receipt_bytes = 0u64;
        for _ in 0..8 {
            let stats_bytes = fs::read(dir.join("stats.json")).map_err(|e| e.to_string())?;
            let parsed: V = serde_json::from_slice(&stats_bytes).map_err(|e| e.to_string())?;
            let (row, bytes) = tail(&journal_file)?;
            let owner = if native_observer { &parsed["observer"] } else { &parsed };
            if owner["journalHash"] == row["hash"] && owner["journalSequence"] == row["sequence"]
                && sha(&fs::read(dir.join("stats.json")).map_err(|e| e.to_string())?) == sha(&stats_bytes)
            {
                stats = Some(parsed);
                receipt_row = row;
                receipt_bytes = bytes;
                break;
            }
        }
        let stats = match stats { Some(s) => s, None => return Err("PAPER_INVENTORY_PUBLICATION_RACE".into()) };
        let publication_ok = stats["bankHash"] == p["bankHash"] && stats["cohort"] == p["cohort"]
            && if native_observer {
                stats["mode"] == bank["mode"]
                    && stats["observer"]["status"].as_str() == Some("completed_batch")
                    && stats["observer"]["persistentWorker"] == V::Bool(false)
            } else {
                stats["failed"] == V::Bool(false)
            };
        need(publication_ok, "PUBLICATION_IDENTITY")?;
        let observed = stats["updatedAt"].as_str().and_then(parse_iso_ms);
        let read_at = clock();
        need(observed.is_some_and(|x| x.is_finite())
            && read_at.is_some()
            && observed.unwrap() <= read_at.unwrap() as f64
            && read_at.unwrap() as f64 - observed.unwrap() <= max_age as f64, "STALE_PUBLICATION")?;
        need(sha(&fs::read(&pointer_file).map_err(|e| e.to_string())?) == sha(&pointer_bytes)
            && sha(&fs::read(base.join("bank.json")).map_err(|e| e.to_string())?) == sha(&bank_bytes),
            "IDENTITY_CHANGED")?;
        let single_fields = ["cash", "realizedPnl", "cumulativeBuys", "positions", "pending",
            "reserved", "fills", "recentFills", "package", "closedPackages", "fees"];
        let wallets = if js_truthy(stats.get("wallets")) {
            stats["wallets"].clone()
        } else {
            let mut single = serde_json::Map::new();
            for k in single_fields {
                if let Some(v) = stats.get(k) { single.insert(k.into(), v.clone()); }
            }
            json!({"single": V::Object(single)})
        };
        need(wallets.as_object().is_some_and(|w| !w.is_empty()), "NO_ACCOUNTING")?;
        let drain = if family == "coupled-bulk-cohorts" { Some("COUPLED_BULK_ENTRY_DRAIN") }
            else if family == "settlement-paper-cohorts" { Some("FUNDAMENTAL_ENTRY_DRAIN") }
            else { None };
        let status = if has_overlay || drain.is_some_and(|d| runtime.join(d).exists()) {
            "entry_draining"
        } else { "current_reducer" };
        let owner = if native_observer { &stats["observer"] } else { &stats };
        let rel = dir.strip_prefix(&runtime).unwrap_or(&dir).iter()
            .map(|c| c.to_string_lossy().into_owned()).collect::<Vec<_>>().join("/");
        let anchors = json!({
            "bankHashMethod": bank_hash_method,
            "bankFileHash": sha(&bank_bytes),
            "journalPrefixBytes": receipt_bytes,
            "journalPrefixHash": hash_file(&journal_file, receipt_bytes)?,
            "journalSequence": receipt_row["sequence"],
            "journalHash": receipt_row["hash"],
            "openingHash": optional_hash(&dir.join("opening.json")),
            "sourceHash": owner.get("sourceHash").cloned().unwrap_or(V::Null),
            "sourceSequence": owner.get("sourceSequence").cloned().unwrap_or(V::Null),
            "sourcePrefixHash": if !stats["sourcePrefixHash"].is_null() { stats["sourcePrefixHash"].clone() }
                else { bank.get("sourcePrefixHash").cloned().unwrap_or(V::Null) },
            "sourcePrefixBytes": bank.get("sourcePrefixBytes").cloned().unwrap_or(V::Null),
        });
        let overlay_hash = if has_overlay { stats["revisionHash"].clone() } else { V::Null };
        let overlay_ok = !has_overlay || overlay_hash.as_str().is_some_and(|s| {
            s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        });
        need(overlay_ok, "OVERLAY_BINDING")?;
        let mut entry = serde_json::Map::new();
        entry.insert("cohort".into(), p["cohort"].clone());
        entry.insert("bankHash".into(), p["bankHash"].clone());
        entry.insert("bankMode".into(), bank["mode"].clone());
        entry.insert("authoritativeOwner".into(), json!(if has_overlay { "operator_overlay" }
            else if native_observer { "native_receipt_observer" } else { "original_reducer" }));
        entry.insert("overlayHash".into(), overlay_hash);
        entry.insert("status".into(), json!(status));
        entry.insert("accounting".into(), json!({"wallets": wallets}));
        entry.insert("anchors".into(), anchors);
        entry.insert("scope".into(), json!({
            "pointer": pointer, "family": family, "authoritativeDirectory": rel,
            "publishedAt": stats["updatedAt"], "baseActorMustRemainFenced": has_overlay,
        }));
        current.push(V::Object(entry));
    }
    let finished = clock();
    need(current.iter().all(|r| {
        let pub_ms = r["scope"]["publishedAt"].as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN);
        finished.is_some() && finished.unwrap() as f64 - pub_ms <= max_age as f64
    }), "SNAPSHOT_TOO_SLOW")?;
    need(current.iter().map(|r| js_str(&r["cohort"])).collect::<BTreeSet<_>>().len() == current.len(),
        "DUPLICATE_COHORT")?;
    Ok(json!({
        "schemaVersion": 1, "observedAt": ms_to_iso(finished.unwrap_or(0) as f64), "current": current,
        "independentEconomicReplay": false, "cutoverReady": false, "activationAuthority": false,
        "competitionOrders": 0, "accountRequests": 0,
        "limitations": [
            "A fresh derived publication matched to its sealed journal tail is an inventory checkpoint, not independent economic/source replay.",
            "All archived banks and journals remain preserved outside this current-reducer manifest.",
            "Final cutover requires stopped original writers, fresh reconciled native replay and the account queue migration boundary.",
        ],
    }))
}

use baal_service_runtime::json_text::js_truthy;
