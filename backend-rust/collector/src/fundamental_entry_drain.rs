// live-paper/fundamental-entry-drain.mjs — operator marker gate for paper hosts.
use baal_execution::core::payload_hash;
use baal_service_runtime::json_text::js_int;
use serde_json::{json, Value as V};
use std::collections::BTreeSet;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use crate::settlement_paper_host::PaperHost;

fn digest(v: &V) -> String { crate::model::digest(v) }
fn parse_at(v: &V) -> f64 { baal_service_runtime::contract::parse_iso_ms(v.as_str().unwrap_or("")).unwrap_or(f64::NAN) }
fn hex64(v: &V) -> bool { v.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())) }

/// readFundamentalEntryDrain(runtimeDir, bank)
pub fn read_fundamental_entry_drain(runtime_dir: &Path, bank: &V) -> Result<Option<V>, String> {
    let file = runtime_dir.join("FUNDAMENTAL_ENTRY_DRAIN");
    if !file.exists() { return Ok(None); }
    let v: V = serde_json::from_slice(&std::fs::read(&file).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let keys_ok = v.as_object().is_some_and(|m| m.keys().all(|k| {
        ["schemaVersion", "owner", "cohort", "bankHash", "at", "reasonCode", "sourceEvidenceHashes"]
            .contains(&k.as_str())
    }));
    let reason_ok = v["reasonCode"].as_str().is_some_and(|s| {
        let b = s.as_bytes();
        b.len() >= 2 && b.len() <= 80 && b[0].is_ascii_uppercase()
            && b[1..].iter().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == b'_')
    });
    if !(v["schemaVersion"].as_i64() == Some(1)
        && v["owner"].as_str() == Some("Codex")
        && v["cohort"].as_str() == bank["name"].as_str()
        && v["bankHash"].as_str() == Some(digest(bank).as_str())
        && parse_at(&v["at"]).is_finite()
        && reason_ok
        && v["sourceEvidenceHashes"].as_array().is_some_and(|a| !a.is_empty() && a.iter().all(hex64))
        && keys_ok)
    {
        return Err("FUNDAMENTAL_ENTRY_DRAIN_INVALID".into());
    }
    Ok(Some(json!({"markerHash": payload_hash(&v)?, "reasonCode": v["reasonCode"], "at": v["at"]})))
}

// Frozen host has no reducer boundary hook. It can continue a single-book exit
// only when *every* latency wallet already owns its one permitted share. On
// divergence/flat state, preserve reservations and require a named continuation.
/// fundamentalDrainAdmission(stats, bank, batch)
pub fn fundamental_drain_admission(stats: &V, bank: &V, batch: &[V]) -> V {
    let id = bank["research"][0]["marketId"].as_str().unwrap_or("");
    let wallets: Vec<V> = stats["wallets"].as_object().map(|w| w.values().cloned().collect()).unwrap_or_default();
    if stats["failed"] != V::Bool(false) || wallets.is_empty()
        || wallets.iter().any(|w| {
            !w["positions"].is_array() || w["positions"].as_array().unwrap().len() != 1
                || w["positions"][0]["marketId"].as_str() != Some(id)
                || w["positions"][0]["quantity"].as_f64() != Some(1.0)
                || !w["pending"].is_array()
                || w["pending"].as_array().unwrap().iter()
                    .any(|p| p["action"].as_str() != Some("sell") || p["marketId"].as_str() != Some(id))
        })
    {
        return json!({"allowed": false, "reason": "flat_divergent_or_pending_buy_requires_continuation"});
    }
    let scoped: Vec<&V> = batch.iter()
        .filter(|row| row["kind"].as_str() == Some("book")
            && row["payload"]["book"]["marketId"].as_str() == Some(id))
        .collect();
    if scoped.len() > 1 {
        return json!({"allowed": false, "reason": "multi_book_batch_requires_continuation"});
    }
    json!({"allowed": true, "reason": "held_single_book_exit_only"})
}

/// wrapFundamentalEntryDrain(host, {runtimeDir, now})
pub struct FundamentalEntryDrainHost {
    inner: Box<dyn PaperHost>,
    journal: PathBuf,
    source: PathBuf,
    runtime_dir: PathBuf,
    previous: String,
    sequence: i64,
    last_mode: Option<String>,
    last_marker: Option<V>,
    projection: V,
    now: std::sync::Arc<dyn Fn() -> i64 + Send + Sync>,
}
impl FundamentalEntryDrainHost {
    /// wrapFundamentalEntryDrain(host, {runtimeDir, now})
    pub fn wrap(inner: Box<dyn PaperHost>, runtime_dir: &Path,
        now: std::sync::Arc<dyn Fn() -> i64 + Send + Sync>) -> Result<Self, String> {
        let journal = inner.dir().join("operator-entry-drain.jsonl");
        let source = runtime_dir.join("public-books.jsonl");
        let mut previous = payload_hash(&json!({"bankHash": digest(inner.bank()),
            "kind": "fundamental_entry_drain"}))?;
        let mut sequence = 0i64;
        let mut last_mode: Option<String> = None;
        let mut last_marker: Option<V> = None;
        if journal.exists() {
            let raw = std::fs::read_to_string(&journal).map_err(|e| e.to_string())?;
            if !raw.is_empty() && !raw.ends_with('\n') {
                return Err("FUNDAMENTAL_DRAIN_PARTIAL_JOURNAL".into());
            }
            for line in raw.trim_end().split('\n').filter(|l| !l.is_empty()) {
                let record: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
                let hash = record["hash"].as_str().unwrap_or("").to_string();
                let mut body = record.clone();
                body.as_object_mut().and_then(|m| m.shift_remove("hash"));
                sequence += 1;
                if body["sequence"].as_i64() != Some(sequence)
                    || body["previous"].as_str().unwrap_or("") != previous
                    || payload_hash(&body).ok().as_deref() != Some(hash.as_str())
                    || body["bankHash"].as_str() != Some(digest(inner.bank()).as_str())
                {
                    return Err("FUNDAMENTAL_DRAIN_JOURNAL".into());
                }
                previous = hash;
                last_mode = body["mode"].as_str().map(String::from);
                last_marker = Some(json!({"markerHash": body["markerHash"],
                    "reasonCode": body["reasonCode"], "at": body["markerAt"]}));
            }
        }
        let projection = json!({"enabled": last_marker.is_some(), "mode": last_mode,
            "continuationRequired": last_mode.as_deref() == Some("continuation_required"),
            "executionAuthority": false});
        Ok(Self { inner, journal, source, runtime_dir: runtime_dir.to_path_buf(),
            previous, sequence, last_mode, last_marker, projection, now })
    }
    fn marker(&self) -> Result<Option<V>, String> {
        match read_fundamental_entry_drain(&self.runtime_dir, self.inner.bank())? {
            Some(m) => Ok(Some(m)),
            None => Ok(self.last_marker.clone()),
        }
    }
    fn record(&mut self, m: &V, mode: &str, reason: &str, stats: &V) -> Result<(), String> {
        self.projection = json!({"enabled": true, "markerHash": m["markerHash"], "reasonCode": m["reasonCode"],
            "mode": mode, "reason": reason, "continuationRequired": mode == "continuation_required",
            "executionAuthority": false});
        if self.last_mode.as_deref() == Some(mode)
            && self.last_marker.as_ref().map(|l| l["markerHash"].clone()) == Some(m["markerHash"].clone())
        {
            return Ok(());
        }
        let body = json!({"sequence": self.sequence + 1, "previous": self.previous,
            "bankHash": digest(self.inner.bank()),
            "at": baal_service_runtime::contract::ms_to_iso((self.now)() as f64),
            "markerHash": m["markerHash"], "markerAt": m["at"], "reasonCode": m["reasonCode"],
            "mode": mode, "reason": reason,
            "sourceSequence": stats["sourceSequence"], "sourceHash": stats["sourceHash"]});
        let hash = payload_hash(&body)?;
        let mut item = body.clone();
        item["hash"] = json!(hash);
        let mut fd = std::fs::OpenOptions::new().append(true).mode(0o600).open(&self.journal)
            .map_err(|e| e.to_string())?;
        fd.write_all((serde_json::to_string(&item).map_err(|e| e.to_string())? + "\n").as_bytes())
            .map_err(|e| e.to_string())?;
        fd.sync_all().map_err(|e| e.to_string())?;
        self.previous = hash;
        self.sequence += 1;
        self.last_mode = Some(mode.to_string());
        self.last_marker = Some(m.clone());
        Ok(())
    }
}
impl PaperHost for FundamentalEntryDrainHost {
    fn dir(&self) -> &Path { self.inner.dir() }
    fn bank(&self) -> &V { self.inner.bank() }
    /// consume()
    fn consume(&mut self) -> Result<usize, String> {
        let m = self.marker()?;
        let Some(m) = m else { return self.inner.consume(); };
        let stats = self.inner.status()?;
        let size = std::fs::metadata(&self.source).map_err(|e| e.to_string())?.len();
        let source_bytes = js_int(&stats["sourceBytes"]).unwrap_or(-1);
        if source_bytes < 0 || source_bytes as u64 > size {
            return Err("FUNDAMENTAL_DRAIN_SOURCE".into());
        }
        let n = (size - source_bytes as u64) as usize;
        use std::io::{Read, Seek, SeekFrom};
        let mut raw = vec![0u8; n];
        let mut fd = std::fs::File::open(&self.source).map_err(|e| e.to_string())?;
        fd.seek(SeekFrom::Start(source_bytes as u64)).map_err(|e| e.to_string())?;
        fd.read_exact(&mut raw).map_err(|_| "FUNDAMENTAL_DRAIN_SOURCE".to_string())?;
        let tail_end = raw.iter().rposition(|b| *b == b'\n').map(|p| p + 1).unwrap_or(0);
        let tail = String::from_utf8_lossy(&raw[..tail_end]);
        let batch: Vec<V> = if tail.is_empty() {
            Vec::new()
        } else {
            let parts: Vec<&str> = tail.split('\n').collect();
            let mut out = Vec::new();
            for line in parts.iter().take(parts.len() - 1) {
                out.push(serde_json::from_str(line).map_err(|e| e.to_string())?);
            }
            out
        };
        let admission = fundamental_drain_admission(&stats, self.inner.bank(), &batch);
        let allowed = admission["allowed"].as_bool().unwrap_or(false);
        self.record(&m, if allowed { "held_exit_only" } else { "continuation_required" },
            admission["reason"].as_str().unwrap_or(""), &stats)?;
        if allowed { self.inner.consume() } else { Ok(0) }
    }
    /// status()
    fn status(&mut self) -> Result<V, String> {
        let mut out = self.inner.status()?;
        out["operatorEntryDrain"] = self.projection.clone();
        Ok(out)
    }
    /// publish()
    fn publish(&mut self) -> Result<(), String> {
        self.inner.publish()?;
        let m = self.marker()?;
        if let Some(_m) = m {
            let stats = self.status()?;
            let file = self.inner.dir().join("operator-entry-drain-status.json");
            let mut body = json!({"bankHash": digest(self.inner.bank()),
                "updatedAt": baal_service_runtime::contract::ms_to_iso((self.now)() as f64)});
            for (k, v) in stats["operatorEntryDrain"].as_object().cloned().unwrap_or_default() { body[k] = v; }
            let mut fd = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600)
                .open(&file).map_err(|e| e.to_string())?;
            fd.write_all((serde_json::to_string(&body).map_err(|e| e.to_string())? + "\n").as_bytes())
                .map_err(|e| e.to_string())?;
            fd.sync_all().map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    fn close(&mut self) -> Result<(), String> { self.inner.close() }
    fn entry_draining(&self) -> Result<bool, String> { Ok(self.marker()?.is_some()) }
}
