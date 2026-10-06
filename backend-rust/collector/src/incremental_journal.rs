// live-paper/incremental-journal.mjs — verified append-only source reader.
use baal_service_runtime::journal::{journal_lines, JournalReadOptions};
use baal_service_runtime::json_text::{digest, sha256};
use baal_service_runtime::contract::parse_iso_ms;
use serde_json::Value as V;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

fn file_identity(file: &Path) -> Result<(u64, u64, u64), String> {
    let m = std::fs::metadata(file).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok((m.dev(), m.ino(), m.len()))
    }
    #[cfg(not(unix))]
    Ok((0, 0, m.len()))
}
pub struct VerifiedSourceReader {
    file: PathBuf,
    chunk_bytes: usize,
    max_record_bytes: usize,
    pub rows: Vec<V>,
    pub prefix_hashes: Vec<String>,
    identity: Option<(u64, u64, u64)>,
    offset: u64,
    previous: String,
    clock: f64,
    prefix: Sha256,
    head: Option<String>,
    boundary: Option<String>,
    metrics: V,
}
impl VerifiedSourceReader {
    pub fn new(file: &Path, chunk_bytes: usize, max_record_bytes: usize) -> Self {
        Self { file: file.to_path_buf(), chunk_bytes, max_record_bytes, rows: vec![], prefix_hashes: vec![],
            identity: None, offset: 0, previous: String::new(), clock: f64::NEG_INFINITY,
            prefix: Sha256::new(), head: None, boundary: None,
            metrics: serde_json::json!({"polls":0,"fullVerifications":0,"appendVerifications":0,"bytesRead":0,
                "guardBytesRead":0,"partialTails":0,"verifiedRecords":0,"verifiedBytes":0}) }
    }
    fn guard(&mut self, start: u64, n: usize) -> Result<String, String> {
        use std::io::{Read, Seek, SeekFrom};
        let mut f = std::fs::File::open(&self.file).map_err(|_| "SOURCE_GUARD_READ".to_string())?;
        f.seek(SeekFrom::Start(start)).map_err(|_| "SOURCE_GUARD_READ".to_string())?;
        let mut b = vec![0u8; n];
        f.read_exact(&mut b).map_err(|_| "SOURCE_GUARD_READ".to_string())?;
        self.metrics["guardBytesRead"] = serde_json::json!(self.metrics["guardBytesRead"].as_i64().unwrap_or(0) + n as i64);
        Ok(sha256(&b))
    }
    /// read() — None for incomplete tail; Err on integrity failures.
    pub fn read(&mut self) -> Result<Option<Vec<V>>, String> {
        self.metrics["polls"] = serde_json::json!(self.metrics["polls"].as_i64().unwrap_or(0) + 1);
        let stat_len = std::fs::metadata(&self.file).map_err(|e| e.to_string())?.len();
        let stat_id = file_identity(&self.file)?;
        if let Some(id) = self.identity {
            if id != stat_id || stat_len < self.offset { return Err("SOURCE_REPLACED_OR_TRUNCATED".into()); }
            if self.offset > 0 {
                if self.guard(0, self.offset.min(4096) as usize)? != self.head.clone().unwrap_or_default()
                    || self.guard(self.offset.saturating_sub(32768), self.offset.min(32768) as usize)? != self.boundary.clone().unwrap_or_default() {
                    return Err("SOURCE_VERIFIED_BOUNDARY_CHANGED".into());
                }
            }
        }
        if stat_len == self.offset { return Ok(if self.rows.is_empty() { None } else { Some(self.rows.clone()) }); }
        let mut appended = Vec::new();
        let mut hashes = Vec::new();
        let mut next_prefix = self.prefix.clone();
        let mut next_previous = self.previous.clone();
        let mut next_clock = self.clock;
        let mut consumed = 0u64;
        let lines = match journal_lines(&self.file, &JournalReadOptions {
            chunk_bytes: self.chunk_bytes, max_record_bytes: self.max_record_bytes,
            start_byte: self.offset, end_byte: Some(stat_len) }) {
            Ok(l) => l,
            Err(e) if e.contains("JOURNAL_INCOMPLETE_TAIL") => {
                self.metrics["partialTails"] = serde_json::json!(self.metrics["partialTails"].as_i64().unwrap_or(0) + 1);
                return Ok(None);
            }
            Err(e) => return Err(e),
        };
        for line in lines {
            let r: V = serde_json::from_str(&line).map_err(|_| "Source integrity failure".to_string())?;
            let hash = r["hash"].as_str().unwrap_or("").to_string();
            let mut body = r.clone();
            body.as_object_mut().unwrap().remove("hash");
            if r["sequence"].as_i64() != Some(self.rows.len() as i64 + appended.len() as i64 + 1)
                || r["previous"].as_str().unwrap_or("") != next_previous || hash != digest(&body) {
                return Err("Source integrity failure".into());
            }
            let at = r["event"]["clockAt"].as_str().or(r["at"].as_str()).and_then(parse_iso_ms)
                .ok_or("Source clock rollback")?;
            if at < next_clock { return Err("Source clock rollback".into()); }
            next_clock = at;
            next_previous = hash;
            next_prefix.update(format!("{}\n", line).as_bytes());
            hashes.push(format!("{:x}", next_prefix.clone().finalize()));
            consumed += line.len() as u64 + 1;
            let mut input = r;
            input.as_object_mut().unwrap().remove("state");
            appended.push(input);
        }
        if self.offset + consumed != stat_len { return Err("SOURCE_BYTE_ACCOUNTING".into()); }
        let after = file_identity(&self.file)?;
        if after != stat_id || std::fs::metadata(&self.file).map_err(|e| e.to_string())?.len() < stat_len {
            return Err("SOURCE_REPLACED_DURING_READ".into());
        }
        let next_head = self.guard(0, stat_len.min(4096) as usize)?;
        let next_boundary = self.guard(stat_len.saturating_sub(32768), stat_len.min(32768) as usize)?;
        self.rows.extend(appended);
        self.prefix_hashes.extend(hashes);
        if self.identity.is_some() {
            self.metrics["appendVerifications"] = serde_json::json!(self.metrics["appendVerifications"].as_i64().unwrap_or(0) + 1);
        } else {
            self.metrics["fullVerifications"] = serde_json::json!(self.metrics["fullVerifications"].as_i64().unwrap_or(0) + 1);
        }
        self.identity = Some(stat_id);
        self.offset = stat_len;
        self.previous = next_previous;
        self.clock = next_clock;
        self.prefix = next_prefix;
        self.head = Some(next_head);
        self.boundary = Some(next_boundary);
        self.metrics["verifiedRecords"] = serde_json::json!(self.rows.len());
        self.metrics["verifiedBytes"] = serde_json::json!(self.offset);
        Ok(if self.rows.is_empty() { None } else { Some(self.rows.clone()) })
    }
    pub fn metrics(&self) -> V {
        let mut m = self.metrics.clone();
        m["sourceSequence"] = serde_json::json!(self.rows.len());
        m["sourceHash"] = serde_json::json!(self.previous);
        m["bytePrefixHash"] = serde_json::json!(format!("{:x}", self.prefix.clone().finalize()));
        m["trustBoundary"] = serde_json::json!("full_chain_on_restart_then_append_identity_head_and_last32KiB_guards; arbitrary_old_middle_rewrites_require_restart_verification");
        m
    }
}
