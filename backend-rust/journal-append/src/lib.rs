mod canonical;
use serde_json::{Value as V, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};
pub type Result<T> = std::result::Result<T, String>;
const LIMIT: u64 = 16 * 1024 * 1024;
fn need(v: bool, c: &str) -> Result<()> {
    if v { Ok(()) } else { Err(c.into()) }
}
fn hex(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn sha(b: &[u8]) -> String {
    format!("{:x}", Sha256::digest(b))
}
#[cfg(windows)]
fn exclusive(path: &Path, create: bool) -> Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    let mut o = OpenOptions::new();
    o.read(true).append(true).share_mode(1);
    if create {
        o.create_new(true);
    }
    o.open(path)
        .map_err(|_| "APPEND_EXCLUSIVE_OPEN_FAILED".into())
}
#[cfg(not(windows))]
fn exclusive(_path: &Path, _create: bool) -> Result<File> {
    Err("APPEND_WINDOWS_EXCLUSIVE_REQUIRED".into())
}
#[cfg(windows)]
fn identity(f: &File) -> Result<String> {
    use std::os::windows::io::AsRawHandle;
    #[repr(C)]
    struct Info {
        attributes: u32,
        creation_low: u32,
        creation_high: u32,
        access_low: u32,
        access_high: u32,
        write_low: u32,
        write_high: u32,
        volume: u32,
        size_high: u32,
        size_low: u32,
        links: u32,
        index_high: u32,
        index_low: u32,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetFileInformationByHandle(handle: *mut std::ffi::c_void, info: *mut Info) -> i32;
    }
    let mut i: Info = unsafe { std::mem::zeroed() };
    need(
        unsafe { GetFileInformationByHandle(f.as_raw_handle(), &mut i) } != 0,
        "APPEND_FILE_ID_FAILED",
    )?;
    Ok(format!(
        "{:08x}:{:08x}{:08x}",
        i.volume, i.index_high, i.index_low
    ))
}
#[cfg(not(windows))]
fn identity(_f: &File) -> Result<String> {
    Err("APPEND_WINDOWS_EXCLUSIVE_REQUIRED".into())
}
fn clock(raw: &str) -> bool {
    let b = raw.as_bytes();
    if b.len() != 24
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
        || b[19] != b'.'
        || b[23] != b'Z'
    {
        return false;
    }
    if !b
        .iter()
        .enumerate()
        .all(|(i, c)| matches!(i, 4 | 7 | 10 | 13 | 16 | 19 | 23) || c.is_ascii_digit())
    {
        return false;
    }
    let num = |a, z| raw[a..z].parse::<u32>().unwrap();
    let (y, m, d) = (num(0, 4), num(5, 7), num(8, 10));
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let md = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    (1..=12).contains(&m)
        && d > 0
        && d <= md[(m - 1) as usize]
        && num(11, 13) < 24
        && num(14, 16) < 60
        && num(17, 19) < 60
}
// Retain the original compact JSON.stringify bytes, including caller object
// insertion order. Rust never reparses/reserializes event or state for hashing.
fn body_end(raw: &str, sequence: u64, previous: &str) -> Result<usize> {
    let header = format!("{{\"sequence\":{sequence},\"previous\":\"{previous}\",\"at\":");
    need(
        raw.starts_with(&header) && sequence <= 9007199254740991,
        "APPEND_CHAIN_INVALID",
    )?;
    let at_end = canonical::end(raw, header.len())?;
    let at: V = serde_json::from_str(&raw[header.len()..at_end]).map_err(|_| "APPEND_CLOCK")?;
    need(at.as_str().is_some_and(clock), "APPEND_CLOCK")?;
    let event_start = at_end + 9;
    need(
        raw.get(at_end..event_start) == Some(",\"event\":"),
        "APPEND_ENVELOPE",
    )?;
    let event_end = canonical::end(raw, event_start)?;
    let state_start = event_end + 9;
    need(
        raw.get(event_end..state_start) == Some(",\"state\":"),
        "APPEND_ENVELOPE",
    )?;
    canonical::end(raw, state_start)
}
fn record(raw: &str, sequence: u64, previous: &str) -> Result<(String, String)> {
    let end = body_end(raw, sequence, previous)?;
    need(
        raw.get(end..end + 9) == Some(",\"hash\":\"") && raw.get(end + 73..) == Some("\"}"),
        "APPEND_ENVELOPE",
    )?;
    let h = raw.get(end + 9..end + 73).ok_or("APPEND_HASH")?;
    need(hex(h), "APPEND_HASH")?;
    let body = format!("{}}}", &raw[..end]);
    need(sha(body.as_bytes()) == h, "APPEND_HASH")?;
    Ok((h.into(), body))
}
pub struct Journal {
    file: File,
    path: PathBuf,
    file_id: String,
    owner: String,
    generation: u64,
    stop_paths: Vec<PathBuf>,
    sequence: u64,
    previous: String,
    bytes: u64,
    prefix: Sha256,
    last_body: Option<String>,
    failed: bool,
}
impl Journal {
    pub fn open(request: &V) -> Result<Self> {
        let path = PathBuf::from(request["path"].as_str().ok_or("APPEND_PATH")?);
        need(path.is_absolute(), "APPEND_PATH")?;
        let owner = request["ownerToken"]
            .as_str()
            .ok_or("APPEND_OWNER")?
            .to_owned();
        need(!owner.is_empty() && owner.len() <= 160, "APPEND_OWNER")?;
        let generation = request["generation"].as_u64().ok_or("APPEND_GENERATION")?;
        let stop_paths = request["stopPaths"]
            .as_array()
            .ok_or("APPEND_STOP_PATHS")?
            .iter()
            .map(|v| {
                v.as_str()
                    .map(PathBuf::from)
                    .ok_or("APPEND_STOP_PATHS".to_owned())
            })
            .collect::<Result<Vec<_>>>()?;
        need(
            stop_paths.iter().all(|p| p.is_absolute()),
            "APPEND_STOP_PATHS",
        )?;
        need(
            request["canOperate"] == true
                && request["stopped"] == false
                && !stop_paths.iter().any(|p| p.exists()),
            "APPEND_STOPPED",
        )?;
        let expected = request["expectedBytes"]
            .as_u64()
            .ok_or("APPEND_PREFIX_BINDING")?;
        let expected_hash = request["expectedSourceHash"]
            .as_str()
            .ok_or("APPEND_PREFIX_BINDING")?;
        need(hex(expected_hash), "APPEND_PREFIX_BINDING")?;
        let expected_sequence = request["expectedSequence"]
            .as_u64()
            .ok_or("APPEND_PREFIX_BINDING")?;
        let expected_tail = request["expectedTailHash"]
            .as_str()
            .ok_or("APPEND_PREFIX_BINDING")?;
        need(
            (expected_sequence == 0 && expected_tail.is_empty())
                || (expected_sequence > 0
                    && expected_sequence <= 9007199254740991
                    && hex(expected_tail)),
            "APPEND_PREFIX_BINDING",
        )?;
        let create = request["createNew"] == true;
        if create {
            need(
                expected == 0 && expected_sequence == 0 && expected_hash == sha(b""),
                "APPEND_NEW_PREFIX",
            )?;
        }
        let mut file = exclusive(&path, create)?;
        let file_id = identity(&file)?;
        need(
            file.metadata().map_err(|_| "APPEND_METADATA")?.len() == expected,
            "APPEND_PREFIX_SIZE",
        )?;
        file.seek(SeekFrom::Start(0)).map_err(|_| "APPEND_SEEK")?;
        let mut reader = BufReader::with_capacity(1024 * 1024, &mut file);
        let (mut bytes, mut sequence) = (0u64, 0u64);
        let mut previous = String::new();
        let mut prefix = Sha256::new();
        let mut line = Vec::new();
        let mut last_body = None;
        loop {
            line.clear();
            let n = reader
                .by_ref()
                .take(LIMIT + 2)
                .read_until(b'\n', &mut line)
                .map_err(|_| "APPEND_READ")?;
            if n == 0 {
                break;
            }
            need(
                n as u64 <= LIMIT + 1 && line.last() == Some(&b'\n'),
                "APPEND_PARTIAL_OR_OVERSIZE",
            )?;
            prefix.update(&line);
            bytes += n as u64;
            if line.len() == 1 {
                continue;
            }
            let raw = std::str::from_utf8(&line[..line.len() - 1]).map_err(|_| "APPEND_UTF8")?;
            let (h, body) = record(raw, sequence + 1, &previous)?;
            sequence += 1;
            previous = h;
            last_body = Some(body);
        }
        drop(reader);
        need(
            bytes == expected
                && format!("{:x}", prefix.clone().finalize()) == expected_hash
                && sequence == expected_sequence
                && previous == expected_tail,
            "APPEND_PREFIX_MISMATCH",
        )?;
        need(
            identity(&file)? == file_id
                && file.metadata().map_err(|_| "APPEND_METADATA")?.len() == bytes,
            "APPEND_FILE_CHANGED",
        )?;
        file.seek(SeekFrom::End(0)).map_err(|_| "APPEND_SEEK")?;
        Ok(Self {
            file,
            path,
            file_id,
            owner,
            generation,
            stop_paths,
            sequence,
            previous,
            bytes,
            prefix,
            last_body,
            failed: false,
        })
    }
    pub fn status(&self) -> V {
        json!({"schemaVersion":1,"mode":"exclusive_js_journal_append_v1","engineHash":env!("APPEND_ENGINE_HASH"),"fileId":self.file_id,"sequence":self.sequence,"tailHash":self.previous,"sourceBytes":self.bytes,"sourceHash":format!("{:x}",self.prefix.clone().finalize()),"lastBody":self.last_body,"failed":self.failed,"windowsShareMode":"FILE_SHARE_READ_ONLY","networkRequests":0,"contestOrders":0})
    }
    fn guard(&self, r: &V) -> Result<()> {
        need(!self.failed, "APPEND_FAILED_RECONCILE_REQUIRED")?;
        need(
            r["ownerToken"].as_str() == Some(self.owner.as_str())
                && r["generation"].as_u64() == Some(self.generation),
            "APPEND_OWNERSHIP",
        )?;
        need(
            r["canOperate"] == true
                && r["stopped"] == false
                && !self.stop_paths.iter().any(|p| p.exists()),
            "APPEND_STOPPED",
        )?;
        need(
            r["expectedSequence"].as_u64() == Some(self.sequence)
                && r["expectedTailHash"].as_str() == Some(self.previous.as_str())
                && r["expectedBytes"].as_u64() == Some(self.bytes)
                && r["expectedSourceHash"].as_str()
                    == Some(format!("{:x}", self.prefix.clone().finalize()).as_str()),
            "APPEND_UNKNOWN_OR_STALE_HEAD",
        )?;
        need(
            identity(&self.file)? == self.file_id
                && self.file.metadata().map_err(|_| "APPEND_METADATA")?.len() == self.bytes,
            "APPEND_FILE_CHANGED",
        )?;
        let reopened = File::open(&self.path).map_err(|_| "APPEND_PATH_CHANGED")?;
        need(identity(&reopened)? == self.file_id, "APPEND_PATH_CHANGED")?;
        Ok(())
    }
    pub fn commit(&mut self, r: &V) -> Result<V> {
        self.guard(r)?;
        let body = r["bodyJson"].as_str().ok_or("APPEND_BODY")?;
        need(body.len() as u64 <= LIMIT, "APPEND_OVERSIZE")?;
        let end = body_end(body, self.sequence + 1, &self.previous)?;
        need(body.get(end..) == Some("}"), "APPEND_BODY_ENVELOPE")?;
        let hash = sha(body.as_bytes());
        let line = format!("{},\"hash\":\"{hash}\"}}\n", &body[..body.len() - 1]);
        self.guard(r)?;
        if self
            .file
            .write_all(line.as_bytes())
            .and_then(|_| self.file.sync_all())
            .is_err()
        {
            self.failed = true;
            return Err("APPEND_DURABILITY_UNKNOWN_RECONCILE_REQUIRED".into());
        }
        self.prefix.update(line.as_bytes());
        self.bytes += line.len() as u64;
        self.sequence += 1;
        self.previous = hash;
        self.last_body = Some(body.into());
        if !identity(&self.file).is_ok_and(|id| id == self.file_id)
            || !self.file.metadata().is_ok_and(|m| m.len() == self.bytes)
        {
            self.failed = true;
            return Err("APPEND_DURABILITY_UNKNOWN_RECONCILE_REQUIRED".into());
        }
        Ok(json!({"durable":true,"receipt":self.status(),"recordJson":line.trim_end_matches('\n')}))
    }
}
