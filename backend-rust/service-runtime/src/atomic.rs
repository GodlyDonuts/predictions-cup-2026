use serde_json::Value as V;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn need(cond: bool, message: impl Into<String>) -> Result<(), String> {
    if cond { Ok(()) } else { Err(message.into()) }
}

// live-paper/runtime.mjs + transport-runtime.mjs atomicJson: pretty JSON to a
// sibling tmp name then rename. No fsync there — parity is deliberate.
pub fn atomic_json(path: &Path, value: &V) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    let body = serde_json::to_string_pretty(value).map_err(|_| "ATOMIC_JSON_SERIALIZE".to_string())?;
    fs::write(&tmp, body.as_bytes()).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| e.to_string())?;
    Ok(())
}

fn fsync_dir(path: &Path) -> Result<(), String> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let file = fs::File::open(dir).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())
}
#[allow(dead_code)]
fn fsync_file(path: &Path) -> Result<(), String> {
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())
}

// execution/ledger.mjs atomicJson: canonical bytes + newline to a unique tmp
// name, fsync the file, rename (Windows retried), fsync the directory.
pub fn atomic_json_canonical(path: &Path, value: &V) -> Result<(), String> {
    let text = crate::json_text::canonical_json(value)?;
    atomic_bytes_canonical(path, format!("{text}\n").as_bytes())
}
pub fn atomic_bytes_canonical(path: &Path, body: &[u8]) -> Result<(), String> {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_nanos();
    let unique = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp: PathBuf = PathBuf::from(format!("{}.{}.{}", path.display(), std::process::id(), nanos ^ unique as u128));
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&tmp).map_err(|e| e.to_string())?;
    file.write_all(body).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    if let Err(e) = replace_file(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    fsync_dir(path)?;
    Ok(())
}

// execution/atomic-rename.mjs replaceFileSync: single rename on POSIX; Windows
// retries EPERM/EACCES/EBUSY after [10,20,40,80,160]ms then throws final error.
pub fn replace_file(tmp: &Path, target: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        let retries = [10u64, 20, 40, 80, 160];
        let mut last = String::new();
        for i in 0..=retries.len() {
            match fs::rename(tmp, target) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    let raw = e.raw_os_error().unwrap_or(0);
                    last = e.to_string();
                    // EPERM=1 EACCES=5(win) EBUSY=32(win)
                    if !(raw == 1 || raw == 5 || raw == 32) {
                        return Err(last);
                    }
                    if i < retries.len() {
                        std::thread::sleep(std::time::Duration::from_millis(retries[i]));
                    }
                }
            }
        }
        return Err(last);
    }
    fs::rename(tmp, target).map_err(|e| e.to_string())
}

pub fn read_json(path: &Path) -> Result<Option<V>, String> {
    let raw = match fs::read_to_string(path) {
        Ok(x) => x,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    Ok(Some(serde_json::from_str(&raw).map_err(|e| e.to_string())?))
}

// transport-runtime.mjs + runtime.mjs lock: create with wx, store pid, return
// close handle. A second acquisition in the same file fails.
pub struct Lock {
    file: PathBuf,
}
impl Lock {
    pub fn acquire(file: &Path) -> Result<Lock, String> {
        let mut f = match fs::OpenOptions::new().write(true).create_new(true).open(file) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(format!("Paper service already locked: {}", file.display()))
            }
            Err(e) => return Err(e.to_string()),
        };
        f.write_all(std::process::id().to_string().as_bytes()).map_err(|e| e.to_string())?;
        Ok(Lock { file: file.to_path_buf() })
    }
    pub fn close(&self) -> Result<(), String> {
        if self.file.exists() {
            fs::remove_file(&self.file).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
