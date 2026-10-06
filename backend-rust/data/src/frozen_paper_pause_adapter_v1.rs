// live-paper/frozen-paper-pause-adapter-v1.mjs — read-only pause import.
// Checks bytes and attribution; never replays economic reducers, creates
// locks, repairs tails, writes files or contacts the account.
use crate::paper_consumer_pause_v1::paper_consumer_pause_decision;
use baal_collector::model::digest;
use baal_execution::core::payload_hash;
use baal_paper::fixed::{big_amount, instant, money};
use baal_service_runtime::journal::{journal_lines, JournalReadOptions};
use baal_service_runtime::json_text::{js_stringify, sha256};
use num_bigint::BigInt;
use serde_json::{json, Value as V};
use std::fs;
use std::path::{Path, PathBuf};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(format!("FROZEN_PAPER_PAUSE_{code}")); }
    Ok(())
}
fn hex(s: &str) -> bool { s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()) }
fn hexv(v: &V) -> bool { v.as_str().is_some_and(hex) }
fn safe_int(v: &V) -> Option<i64> { baal_service_runtime::json_text::js_int(v) }

struct Stat { dev: u64, ino: u64, size: u64, mtime_ns: i128, ctime_ns: i128, birthtime_ns: i128 }
fn stamp(s: &Stat) -> String {
    format!("{}:{}:{}:{}:{}:{}", s.dev, s.ino, s.size, s.mtime_ns, s.ctime_ns, s.birthtime_ns)
}
fn identity(s: &Stat) -> String { format!("{}:{}:{}", s.dev, s.ino, s.birthtime_ns) }
fn stat(f: &Path) -> Result<Stat, String> {
    let m = fs::metadata(f).map_err(|e| e.to_string())?;
    let l = fs::symlink_metadata(f).map_err(|e| e.to_string())?;
    need(m.is_file() && !l.file_type().is_symlink(), "REGULAR_FILE")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let btime = m.created().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok());
        return Ok(Stat {
            dev: m.dev(), ino: m.ino(), size: m.size(),
            mtime_ns: m.mtime() as i128 * 1_000_000_000 + m.mtime_nsec() as i128,
            ctime_ns: m.ctime() as i128 * 1_000_000_000 + m.ctime_nsec() as i128,
            birthtime_ns: btime.map(|d| d.as_nanos() as i128).unwrap_or(0),
        });
    }
    #[cfg(not(unix))]
    {
        Ok(Stat {
            dev: 0, ino: 0, size: m.len(),
            mtime_ns: m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos() as i128).unwrap_or(0),
            ctime_ns: 0,
            birthtime_ns: m.created().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos() as i128).unwrap_or(0),
        })
    }
}
fn within(root: &Path, file: &Path) -> Result<PathBuf, String> {
    let actual = fs::canonicalize(file).map_err(|e| e.to_string())?;
    let base = fs::canonicalize(root).map_err(|e| e.to_string())?;
    need(actual.starts_with(base.join("x").parent().unwrap_or(&base)) && actual != base, "PATH_SCOPE")?;
    Ok(actual)
}
struct Doc { value: V, hash: String, stamp: String, file: PathBuf }
fn read_json(file: &Path, max: u64) -> Result<Doc, String> {
    let before = stat(file)?;
    need(before.size <= max, "DOCUMENT_SIZE")?;
    let raw = fs::read(file).map_err(|e| e.to_string())?;
    need(stamp(&stat(file)?) == stamp(&before), "DOCUMENT_CHANGED")?;
    Ok(Doc {
        value: serde_json::from_slice(&raw).map_err(|e| e.to_string())?,
        hash: sha256(&raw),
        stamp: stamp(&before),
        file: file.to_path_buf(),
    })
}

struct VerifiedJournal { last: V, stamp: String, file: PathBuf }
fn verify_journal(file: &Path, anchor: &V) -> Result<VerifiedJournal, String> {
    let before = stat(file)?;
    let abytes = safe_int(&anchor["bytes"]);
    need(
        abytes.is_some_and(|b| b > 0 && b as u64 == before.size)
            && hexv(&anchor["prefixHash"])
            && hexv(&anchor["hash"])
            && safe_int(&anchor["sequence"]).is_some_and(|s| s > 0),
        "JOURNAL_FRONTIER",
    )?;
    let end = abytes.unwrap() as u64;
    let mut bytes = 0u64;
    let mut seq = 0u64;
    let mut previous = String::new();
    let mut last = V::Null;
    let mut h = sha2::Sha256::new();
    use sha2::Digest;
    for line in journal_lines(file, &JournalReadOptions { end_byte: Some(end), ..Default::default() })? {
        need(!line.is_empty(), "EMPTY_JOURNAL_RECORD")?;
        h.update(format!("{line}\n").as_bytes());
        bytes += (line.len() + 1) as u64;
        let row: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        need(js_stringify(&row) == line, "ORIGINAL_JOURNAL_CHAIN")?;
        let hash = row["hash"].as_str().unwrap_or("").to_string();
        let mut body = row.clone();
        body.as_object_mut().unwrap().shift_remove("hash");
        need(
            body["sequence"].as_u64() == Some(seq + 1)
                && body["previous"] == json!(previous)
                && hex(&hash)
                && sha256(js_stringify(&body).as_bytes()) == hash,
            "ORIGINAL_JOURNAL_CHAIN",
        )?;
        seq += 1;
        instant(body["at"].as_str().unwrap_or("")).map_err(|e| e)?;
        previous = hash;
        last = body;
    }
    need(
        bytes == end && seq == safe_int(&anchor["sequence"]).unwrap() as u64
            && previous == anchor["hash"].as_str().unwrap_or("")
            && sha256(&h.finalize()) == anchor["prefixHash"].as_str().unwrap_or("")
            && stamp(&stat(file)?) == stamp(&before),
        "JOURNAL_CHANGED_OR_HASH",
    )?;
    Ok(VerifiedJournal { last, stamp: stamp(&before), file: file.to_path_buf() })
}

struct VerifiedSource { file: PathBuf, identity: String }
fn verify_source(file: &Path, anchor: &V) -> Result<VerifiedSource, String> {
    let before = stat(file)?;
    let abytes = safe_int(&anchor["bytes"]);
    need(
        abytes.is_some_and(|b| b > 0 && before.size >= b as u64)
            && hexv(&anchor["prefixHash"])
            && hexv(&anchor["hash"])
            && safe_int(&anchor["sequence"]).is_some_and(|s| s > 0),
        "SOURCE_FRONTIER",
    )?;
    let end = abytes.unwrap() as u64;
    let mut bytes = 0u64;
    let mut seq = 0u64;
    let mut previous = String::new();
    let mut h = sha2::Sha256::new();
    use sha2::Digest;
    for line in journal_lines(file, &JournalReadOptions { end_byte: Some(end), ..Default::default() })? {
        need(!line.is_empty(), "EMPTY_SOURCE_RECORD")?;
        h.update(format!("{line}\n").as_bytes());
        bytes += (line.len() + 1) as u64;
        let row: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        need(js_stringify(&row) == line, "ORIGINAL_PUBLIC_CHAIN")?;
        let hash = row["hash"].as_str().unwrap_or("").to_string();
        let mut body = row.clone();
        body.as_object_mut().unwrap().shift_remove("hash");
        need(
            body["sequence"].as_u64() == Some(seq + 1)
                && body["previousHash"] == json!(previous)
                && hex(&hash)
                && payload_hash(&body).ok().as_deref() == Some(hash.as_str()),
            "ORIGINAL_PUBLIC_CHAIN",
        )?;
        seq += 1;
        instant(body["at"].as_str().unwrap_or("")).map_err(|e| e)?;
        previous = hash;
    }
    let after = stat(file)?;
    need(
        bytes == end && seq == safe_int(&anchor["sequence"]).unwrap() as u64
            && previous == anchor["hash"].as_str().unwrap_or("")
            && sha256(&h.finalize()) == anchor["prefixHash"].as_str().unwrap_or("")
            && identity(&after) == identity(&before)
            && after.size >= end,
        "SOURCE_CHANGED_OR_HASH",
    )?;
    Ok(VerifiedSource { file: file.to_path_buf(), identity: identity(&before) })
}

fn verify_report(report: &V, paper: &V, bank: &V) -> Result<(), String> {
    let mut report_keys: Vec<String> = report["wallets"].as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
    report_keys.sort();
    let mut paper_keys: Vec<String> = paper["wallets"].as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
    paper_keys.sort();
    need(
        report["bankHash"] == paper["bankHash"]
            && report["bankHash"] == json!(digest(bank))
            && report["stateHash"] == json!(digest(paper))
            && report["executionAuthority"] == json!(false)
            && report["unknownAppendAcceptance"] != json!(true)
            && report["mode"] == bank["mode"]
            && report["cohort"] == bank["name"]
            && payload_hash(&json!(report_keys)).ok() == payload_hash(&json!(paper_keys)).ok(),
        "REPORT_STATE_BINDING",
    )?;
    for (k, w) in paper["wallets"].as_object().cloned().unwrap_or_default() {
        let r = &report["wallets"][&k];
        let fills = w["fills"].as_array().cloned().unwrap_or_default();
        let sell_count = fills.iter().filter(|f| f["action"] == json!("sell")).count();
        let pending_values: Vec<V> = w["pending"].as_object().map(|o| o.values().cloned().collect()).unwrap_or_default();
        need(
            r["cash"] == w["cash"]
                && r["realizedPnl"] == w["realizedPnl"]
                && r["cumulativeBuys"] == w["cumulativeBuys"]
                && r["fills"] == json!(fills.len())
                && r["closedTrades"] == json!(sell_count)
                && payload_hash(&r["recentFills"]).ok() == payload_hash(&json!(fills.iter().rev().take(25).rev().cloned().collect::<Vec<_>>())).ok()
                && payload_hash(&r["pending"]).ok() == payload_hash(&json!(pending_values)).ok(),
            "WALLET_PROJECTION",
        )?;
        let reserved: BigInt = pending_values
            .iter()
            .filter(|p| p["action"] == json!("buy"))
            .try_fold(BigInt::from(0), |n, p| Ok(n + big_amount(p["limit"].as_str().unwrap_or(""))?))
            .map_err(|e: String| e)?;
        need(r["reservedCost"] == json!(money(&reserved)), "RESERVATION_PROJECTION")?;
        let positions = r["positions"].as_array().cloned().unwrap_or_default();
        let unique: std::collections::BTreeSet<String> =
            positions.iter().filter_map(|p| p["marketId"].as_str().map(String::from)).collect();
        need(
            r["positions"].is_array()
                && unique.len() == positions.len()
                && positions.len() == w["positions"].as_object().map(|o| o.len()).unwrap_or(0),
            "POSITION_PROJECTION",
        )?;
        for p in &positions {
            let market_id = p["marketId"].as_str().unwrap_or("");
            let held = &w["positions"][market_id];
            need(
                held.is_object()
                    && p["instrument"] == json!(format!("{}:YES", bank["identities"][market_id]["exchangeId"].as_str().unwrap_or("")))
                    && p["quantity"] == held["quantity"]
                    && p["cost"] == held["cost"]
                    && p["enteredAt"] == held["enteredAt"],
                "FIFO_POSITION_PROJECTION",
            )?;
        }
    }
    Ok(())
}

/// readFrozenPaperPause({runtimeDir,kind,pointerFile,cohortDirName,now})
/// Returns Ok(None) when no pause applies; Ok(Some(fields)) mirroring the JS
/// object's data fields (function fields become methods on `FrozenPaperPause`).
pub struct FrozenPaperPause { pub fields: V }
impl FrozenPaperPause {
    pub fn status(&self) -> V { self.fields["report"].clone() }
    pub fn consume(&self) -> i64 { 0 }
    pub fn publish(&self) {}
    pub fn focus(&self) -> V {
        let bank_hash = self.fields["report"]["bankHash"].as_str().unwrap_or("").to_string();
        json!({"bankHash":bank_hash,"markets":[],"ownedRiskReads":false,"paperComparisonPaused":true,
            "retainedRisk":self.fields["retainedRisk"].clone(),"valuationStatus":"frozen_paper_snapshot_not_current","executionAuthority":false})
    }
    pub fn close(&self) {}
}

pub fn read_frozen_paper_pause(runtime_dir: &Path, kind: &str, pointer_file: &str, cohort_dir_name: &str, now: &str) -> Result<Option<FrozenPaperPause>, String> {
    need(
        runtime_dir.is_absolute()
            && kind == "settlement_portfolio_v2"
            && pointer_file == "settlement-portfolio-v2-active.json"
            && cohort_dir_name == "settlement-portfolio-v2-cohorts",
        "SUPPORTED_PAPER_SCOPE",
    )?;
    instant(now).map_err(|e| e)?;
    let manifest_file = runtime_dir.join("paper-comparison-pauses.json");
    if !manifest_file.exists() {
        return Ok(None);
    }
    let manifest = read_json(&within(runtime_dir, &manifest_file)?, 1024 * 1024)?;
    need(
        manifest.value["schemaVersion"] == json!(1)
            && manifest.value["owner"] == json!("Codex")
            && manifest.value["entries"].as_array().is_some_and(|e| e.len() <= 16),
        "MANIFEST",
    )?;
    let entries = manifest.value["entries"].as_array().cloned().unwrap_or_default();
    let pointer_path = runtime_dir.join(pointer_file);
    if !pointer_path.exists() {
        need(!entries.iter().any(|e| e["kind"] == json!(kind)), "MISSING_PAUSED_POINTER")?;
        return Ok(None);
    }
    let pointer = read_json(&within(runtime_dir, &pointer_path)?, 65536)?;
    let p = &pointer.value;
    need(
        p["schemaVersion"] == json!(1)
            && p["cohort"].as_str().is_some_and(|c| !c.is_empty() && c.len() <= 63 && c.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'))
            && hexv(&p["bankHash"]),
        "POINTER",
    )?;
    let cohort = p["cohort"].as_str().unwrap().to_string();
    let matches: Vec<V> = entries.iter().filter(|e| e["kind"] == json!(kind) && e["cohort"] == json!(cohort)).cloned().collect();
    need(matches.len() <= 1, "DUPLICATE_PAUSE")?;
    if matches.is_empty() {
        return Ok(None);
    }
    let entry = &matches[0];
    let snapshot_file = entry["snapshotFile"].as_str().unwrap_or("");
    // /^[a-z0-9][a-z0-9_.-]{0,100}\.json$/
    let sn_len = snapshot_file.len();
    need(
        (6..=106).contains(&sn_len)
            && snapshot_file.ends_with(".json")
            && (snapshot_file.as_bytes()[0].is_ascii_lowercase() || snapshot_file.as_bytes()[0].is_ascii_digit())
            && snapshot_file.as_bytes()[1..sn_len - 5].iter().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'.' | b'-'))
            && hexv(&entry["snapshotFileHash"]),
        "SNAPSHOT_NAME",
    )?;
    let dir = runtime_dir.join(cohort_dir_name).join(&cohort);
    let bank_doc = read_json(&within(runtime_dir, &dir.join("bank.json"))?, 4 * 1024 * 1024)?;
    let bank = bank_doc.value.clone();
    need(
        bank["schemaVersion"] == json!(2)
            && bank["mode"] == json!("funded_settlement_portfolio_paper_v2")
            && bank["name"] == json!(cohort)
            && digest(&bank) == p["bankHash"].as_str().unwrap_or(""),
        "IMMUTABLE_BANK",
    )?;
    let snap = read_json(&within(&dir, &dir.join(snapshot_file))?, 16 * 1024 * 1024)?;
    let s = &snap.value;
    need(
        snap.hash == entry["snapshotFileHash"].as_str().unwrap_or("")
            && s["schemaVersion"] == json!(1)
            && s["kind"] == json!(kind)
            && s["cohort"] == json!(cohort)
            && s["bankHash"] == p["bankHash"]
            && s["bankFileHash"] == json!(bank_doc.hash)
            && s["pointerHash"] == json!(pointer.hash)
            && instant(s["savedAt"].as_str().unwrap_or("")).map_err(|e| e)? <= instant(now).map_err(|e| e)?,
        "SNAPSHOT_BINDINGS",
    )?;
    let journal = verify_journal(&within(&dir, &dir.join("journal.jsonl"))?, &s["journalAnchor"])?;
    let state = journal.last["state"].clone();
    need(
        baal_service_runtime::json_text::js_truthy(journal.last.get("event"))
            && state["paper"].is_object()
            && payload_hash(&state["paper"]).ok() == payload_hash(&s["paperState"]).ok()
            && state["paper"]["bankHash"] == p["bankHash"]
            && state["sourceBytes"] == s["sourceAnchor"]["bytes"]
            && state["sourceSequence"] == s["sourceAnchor"]["sequence"]
            && state["sourceHash"] == s["sourceAnchor"]["hash"]
            && state["sourcePrefixHash"] == s["sourceAnchor"]["prefixHash"],
        "EXACT_TAIL_PAPER_AND_SOURCE",
    )?;
    let source = verify_source(&within(runtime_dir, &runtime_dir.join("public-books.jsonl"))?, &s["sourceAnchor"])?;
    verify_report(&s["report"], &state["paper"], &bank)?;
    need(
        s["report"]["journalSequence"] == s["journalAnchor"]["sequence"]
            && s["report"]["journalHash"] == s["journalAnchor"]["hash"]
            && s["report"]["sourceBytes"] == s["sourceAnchor"]["bytes"]
            && s["report"]["sourceSequence"] == s["sourceAnchor"]["sequence"]
            && s["report"]["sourceHash"] == s["sourceAnchor"]["hash"]
            && s["report"]["sourcePrefixHash"] == s["sourceAnchor"]["prefixHash"],
        "REPORT_FRONTIER",
    )?;
    let decision = paper_consumer_pause_decision(&json!({
        "pauseRecord": entry["pauseRecord"], "consumerKind": "counterfactual_paper",
        "cohort": cohort, "bankHash": p["bankHash"], "report": s["report"],
        "journalAnchor": s["journalAnchor"], "sourceAnchor": s["sourceAnchor"],
        "accountingVerified": true, "now": now,
    }));
    need(decision["paused"] == json!(true), "EXPLICIT_OWNED_PAUSE")?;
    for d in [&manifest, &pointer, &bank_doc, &snap] {
        need(stamp(&stat(&d.file)?) == d.stamp, "DOCUMENT_CHANGED_DURING_VERIFY")?;
    }
    need(
        stamp(&stat(&journal.file)?) == journal.stamp && identity(&stat(&source.file)?) == source.identity,
        "SOURCE_REPLACED_DURING_VERIFY",
    )?;
    let mut frozen_report = s["report"].clone();
    let report_hash = payload_hash(&s["report"])?;
    for (k, v) in [
        ("paperComparisonPaused", json!(true)),
        ("paperPause", decision.clone()),
        ("valuationStatus", json!("frozen_paper_snapshot_not_current")),
        ("pausedAt", entry["pauseRecord"]["createdAt"].clone()),
        ("originalReportHash", json!(report_hash)),
        ("persistentWorker", json!(false)),
        ("executionAuthority", json!(false)),
    ] {
        frozen_report[k] = v;
    }
    let mut fields = decision.clone();
    for (k, v) in [
        ("kind", json!(kind)),
        ("cohort", json!(cohort)),
        ("snapshotFileHash", json!(snap.hash)),
        ("verificationMode", json!("full_original_bytes_hashchains_and_tail_accounting_no_economic_replay")),
        ("economicReplayPerformed", json!(false)),
        ("readOnly", json!(true)),
        ("networkRequests", json!(0)),
        ("competitionOrders", json!(0)),
        ("report", frozen_report),
        ("paperState", state["paper"].clone()),
    ] {
        fields[k] = v;
    }
    Ok(Some(FrozenPaperPause { fields }))
}
