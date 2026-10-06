// data/archive-native-request-clock-first-20261004.mjs — preserve the original
// request-clock evidence before the lossless source-clock encoding correction.
use serde_json::Value as V;
use std::path::Path;

pub fn run() -> Result<V, String> {
    let file = Path::new("data/native-request-clock-milestone-20261004.json");
    let archive = Path::new("data/native-request-clock-milestone-20261004-v1.json");
    if archive.exists() { return Err("assertion failed".into()); }
    let bytes = std::fs::read(file).map_err(|e| e.to_string())?;
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; o.mode(0o600); }
    let mut f = o.open(archive).map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut f, &bytes).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({"message": "Preserved original request-clock evidence before lossless source-clock encoding correction"}))
}
