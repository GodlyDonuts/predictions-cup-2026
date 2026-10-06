use crate::json_text::{js_stringify, sha256};
use serde_json::Value as V;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

// live-paper/journal-lines.mjs: bounded chunk reads, bounded record size, an
// unterminated tail refuses the file rather than silently truncating.
pub struct JournalReadOptions {
    pub chunk_bytes: usize,
    pub max_record_bytes: usize,
    pub start_byte: u64,
    pub end_byte: Option<u64>,
}
impl Default for JournalReadOptions {
    fn default() -> Self {
        JournalReadOptions { chunk_bytes: 1_048_576, max_record_bytes: 16_777_216, start_byte: 0, end_byte: None }
    }
}
pub fn journal_lines(file: &Path, options: &JournalReadOptions) -> Result<Vec<String>, String> {
    if options.chunk_bytes == 0
        || options.chunk_bytes > 1_048_576
        || options.max_record_bytes < options.chunk_bytes
    {
        return Err("INVALID_JOURNAL_READ_LIMIT".into());
    }
    let mut fd = fs::File::open(file).map_err(|e| e.to_string())?;
    let actual = fd.metadata().map_err(|e| e.to_string())?.len();
    let size = options.end_byte.unwrap_or(actual);
    if options.start_byte > size || size > actual {
        return Err("INVALID_JOURNAL_READ_RANGE".into());
    }
    fd.seek(SeekFrom::Start(options.start_byte)).map_err(|e| e.to_string())?;
    let mut pos = options.start_byte;
    let mut tail: Vec<u8> = Vec::new();
    let mut out = Vec::new();
    while pos < size {
        let want = (options.chunk_bytes as u64).min(size - pos) as usize;
        let mut buf = vec![0u8; want];
        let n = fd.read(&mut buf).map_err(|e| e.to_string())?;
        if n < 1 {
            return Err("JOURNAL_READ_TRUNCATED".into());
        }
        pos += n as u64;
        tail.extend_from_slice(&buf[..n]);
        while let Some(end) = tail.iter().position(|b| *b == b'\n') {
            if end > options.max_record_bytes {
                return Err("JOURNAL_RECORD_TOO_LARGE".into());
            }
            let line: Vec<u8> = tail.drain(..=end).collect();
            out.push(String::from_utf8(line[..end].to_vec()).map_err(|_| "JOURNAL_INVALID_ENCODING".to_string())?);
        }
        if tail.len() > options.max_record_bytes {
            return Err("JOURNAL_RECORD_TOO_LARGE".into());
        }
    }
    if !tail.is_empty() {
        return Err("JOURNAL_INCOMPLETE_TAIL: Incomplete paper journal: manual recovery required".into());
    }
    Ok(out)
}

fn parse_line(line: &str) -> Result<V, String> {
    let v: V = serde_json::from_str(line).map_err(|_| "Journal integrity failure".to_string())?;
    if !v.is_object() {
        return Err("Journal integrity failure".into());
    }
    Ok(v)
}

fn body_hash(body: &V) -> String {
    sha256(js_stringify(body).as_bytes())
}

fn verify_record(v: &V, expected_seq: u64, expected_previous: &str) -> Result<(String, V), String> {
    let m = v.as_object().ok_or("Journal integrity failure")?;
    let hash = m.get("hash").and_then(V::as_str).ok_or("Journal integrity failure")?;
    let mut body = v.clone();
    body.as_object_mut().unwrap().shift_remove("hash");
    if crate::json_text::js_int(body.get("sequence").unwrap_or(&V::Null)) != Some(expected_seq as i64)
        || body.get("previous").and_then(V::as_str) != Some(expected_previous)
        || body_hash(&body) != hash
    {
        return Err("Journal integrity failure".into());
    }
    Ok((hash.to_string(), body))
}

// transport-runtime.mjs / live-paper/runtime.mjs Journal: the persisted record
// is {sequence, previous, at, event, state, hash}; `previous` is '' at genesis.
pub struct Journal {
    pub file: PathBuf,
    pub sequence: u64,
    pub previous: String,
    pub last: Option<V>,
}
impl Journal {
    pub fn open(file: &Path) -> Result<Journal, String> {
        let mut j = Journal { file: file.to_path_buf(), sequence: 0, previous: String::new(), last: None };
        if file.exists() {
            for line in journal_lines(file, &JournalReadOptions::default())? {
                if line.is_empty() {
                    continue;
                }
                let v = parse_line(&line)?;
                let (hash, body) = verify_record(&v, j.sequence + 1, &j.previous)?;
                j.sequence = body.get("sequence").and_then(V::as_u64).unwrap();
                j.previous = hash;
                j.last = Some(body);
            }
        }
        Ok(j)
    }
    pub fn commit(&mut self, state: &V, event: &V) -> Result<V, String> {
        let at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let body = serde_json::json!({
            "sequence": self.sequence + 1,
            "previous": self.previous,
            "at": at,
            "event": event,
            "state": state,
        });
        let hash = body_hash(&body);
        let mut record = body.clone();
        record.as_object_mut().unwrap().insert("hash".into(), V::from(hash.clone()));
        let line = format!("{}\n", js_stringify(&record));
        let mut fd = fs::OpenOptions::new().append(true).create(true).open(&self.file).map_err(|e| e.to_string())?;
        fd.write_all(line.as_bytes()).map_err(|_| "Incomplete journal write".to_string())?;
        fd.sync_all().map_err(|e| e.to_string())?;
        self.sequence = body.get("sequence").and_then(V::as_u64).unwrap();
        self.previous = hash;
        self.last = Some(body);
        Ok(record)
    }
}

// runtime.mjs readVerifiedSource: verify complete bytes first, then strip the
// `state` body from each row; prefix hashes stay byte-for-byte identical.
pub struct VerifiedSource {
    pub rows: Vec<V>,
    pub prefix_hashes: Vec<String>,
}
pub fn read_verified_source(file: &Path, options: &JournalReadOptions) -> Result<Option<VerifiedSource>, String> {
    let lines = match journal_lines(file, options) {
        Ok(x) => x,
        Err(e) if e.starts_with("JOURNAL_INCOMPLETE_TAIL") => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut rows = Vec::new();
    let mut prefix_hashes = Vec::new();
    let mut prefix = Sha256::new();
    let mut previous = String::new();
    let mut clock = f64::NEG_INFINITY;
    for (i, line) in lines.iter().enumerate() {
        let v = parse_line(line)?;
        let (hash, mut body) = verify_record(&v, i as u64 + 1, &previous)?;
        let clock_at = body
            .get("event")
            .and_then(|e| e.get("clockAt"))
            .filter(|x| crate::json_text::js_truthy(Some(x)))
            .or_else(|| body.get("at"))
            .and_then(V::as_str);
        let next = clock_at
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|d| d.timestamp_millis() as f64)
            .unwrap_or(f64::NAN);
        if !next.is_finite() || next < clock {
            return Err("Source clock rollback".into());
        }
        clock = next;
        prefix.update(line.as_bytes());
        prefix.update(b"\n");
        prefix_hashes.push(format!("{:x}", prefix.clone().finalize()));
        previous = hash;
        body.as_object_mut().unwrap().shift_remove("state");
        rows.push(body);
    }
    if rows.is_empty() {
        return Ok(None);
    }
    Ok(Some(VerifiedSource { rows, prefix_hashes }))
}
