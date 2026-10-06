// feed/capture.mjs
use baal_service_runtime::json_text::js_stringify;
use regex::Regex;
use serde_json::{json, Map, Value as V};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::OnceLock;

pub const CAPTURE_SCHEMA_VERSION: i64 = 1;

fn sensitive() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"(?i)(?:authorization|cookie|token|secret|password|api.?key|anon.?key|credential|headers)").unwrap()
    })
}
fn token_like() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"(?i)(?:Bearer\s+\S+|eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+|(?:sk|sig)[_-][A-Za-z0-9_-]{12,})").unwrap()
    })
}

pub fn sanitize(value: &V, depth: u32) -> V {
    if depth > 30 {
        return json!("[REDACTED_DEPTH]");
    }
    match value {
        V::String(s) => json!(token_like().replace_all(s, "[REDACTED]")),
        V::Array(a) => V::Array(a.iter().map(|v| sanitize(v, depth + 1)).collect()),
        V::Object(o) => {
            let mut out = Map::new();
            for (k, v) in o {
                if !sensitive().is_match(k) {
                    out.insert(k.clone(), sanitize(v, depth + 1));
                }
            }
            V::Object(out)
        }
        _ => value.clone(),
    }
}

// A new exclusive file per session; never truncate or reopen an old capture.
// Only normalized contract/state records reach this sink.
pub struct Capture {
    fd: Option<File>,
    sync: bool,
    index: u64,
}

impl Capture {
    pub fn new(path: &Path, sync: bool) -> Result<Capture, String> {
        let fd = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .map_err(|e| e.to_string())?;
        Ok(Capture { fd: Some(fd), sync, index: 0 })
    }
    pub fn append(&mut self, record: &V) -> Result<(), String> {
        let fd = self.fd.as_mut().ok_or("CAPTURE_CLOSED")?;
        let mut row = Map::new();
        row.insert("format".into(), json!("sig-feed-capture"));
        row.insert("schemaVersion".into(), json!(CAPTURE_SCHEMA_VERSION));
        row.insert("captureIndex".into(), json!(self.index));
        self.index += 1;
        if let V::Object(o) = sanitize(record, 0) {
            for (k, v) in o {
                row.insert(k, v);
            }
        }
        fd.write_all(js_stringify(&V::Object(row)).as_bytes()).map_err(|e| e.to_string())?;
        fd.write_all(b"\n").map_err(|e| e.to_string())?;
        if self.sync {
            fd.sync_data().map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    pub fn close(&mut self) {
        self.fd = None;
    }
}
