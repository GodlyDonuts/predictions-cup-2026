// execution/atomic-rename.mjs — Windows may briefly deny replacement while a
// reader or scanner holds the destination. Retry the same already-synced
// temporary file. Never unlink the existing projection or repeat the
// authoritative journal operation.
use std::path::Path;
use std::time::Duration;

// Node libuv reports EPERM/EACCES/EBUSY; on Windows those surface as raw OS
// errors 5 (ACCESS_DENIED), 32 (SHARING_VIOLATION) and 33 (LOCK_VIOLATION).
fn retryable(e: &std::io::Error) -> bool {
    if e.kind() == std::io::ErrorKind::PermissionDenied {
        return true;
    }
    matches!(e.raw_os_error(), Some(1 | 13 | 16 | 5 | 32 | 33))
}

fn process_platform() -> &'static str {
    match std::env::consts::OS {
        "windows" => "win32",
        "macos" => "darwin",
        other => other,
    }
}

// replaceFileSync(source, target): returns {attempts} in JS — here the attempt
// count. Retries apply only on win32 while the error is retryable.
pub fn replace_file_sync(source: &Path, target: &Path) -> Result<u64, String> {
    replace_file_sync_on(source, target, process_platform())
}

pub fn replace_file_sync_on(source: &Path, target: &Path, platform: &str) -> Result<u64, String> {
    let delays = [10u64, 20, 40, 80, 160];
    let mut attempt = 0usize;
    loop {
        match std::fs::rename(source, target) {
            Ok(()) => return Ok(attempt as u64 + 1),
            Err(e) => {
                if platform != "win32" || !retryable(&e) || attempt >= delays.len() {
                    return Err(e.to_string());
                }
                std::thread::sleep(Duration::from_millis(delays[attempt]));
            }
        }
        attempt += 1;
    }
}
