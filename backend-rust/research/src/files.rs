// Internal file/network helpers mirroring the node:fs / fetch idioms the
// research scripts use. Error strings on IO failures carry the OS error text,
// matching the JS behavior of propagating an exception (code strings are only
// asserted where the script itself throws them).
use serde_json::Value as V;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn read_bytes(path: &Path) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|e| e.to_string())
}
pub fn read_text(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| e.to_string())
}
pub fn read_json(path: &Path) -> Result<V, String> {
    let text = read_text(path)?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}
pub fn exists(path: &Path) -> bool {
    path.exists()
}
pub fn mkdir(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())
}
// fs.writeFileSync(file, data, {flag:'wx', mode:0o600}) — exclusive create.
pub fn write_wx(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = f.set_permissions(fs::Permissions::from_mode(0o600));
    }
    f.write_all(bytes).map_err(|e| e.to_string())
}
// fs.writeFileSync(file, data) — truncating create (scripts that omit 'wx').
pub fn write_plain(path: &Path, bytes: &[u8]) -> Result<(), String> {
    fs::write(path, bytes).map_err(|e| e.to_string())
}
// JSON.stringify(value, null, 2) + '\n' with 'wx'.
pub fn write_json_pretty_wx(path: &Path, value: &V) -> Result<(), String> {
    write_wx(path, (serde_json::to_string_pretty(value).map_err(|e| e.to_string())? + "\n").as_bytes())
}
pub fn write_json_pretty(path: &Path, value: &V) -> Result<(), String> {
    write_plain(path, (serde_json::to_string_pretty(value).map_err(|e| e.to_string())? + "\n").as_bytes())
}
// path.join(process.env.LOCALAPPDATA, 'SigTradingCup', 'paper-runtime') —
// an unset variable crashes in JS, so this returns Err.
pub fn runtime_dir() -> Result<PathBuf, String> {
    let base = std::env::var("LOCALAPPDATA")
        .map_err(|_| "LOCALAPPDATA is not set".to_string())?;
    Ok(Path::new(&base).join("SigTradingCup").join("paper-runtime"))
}

pub struct Fetch {
    pub status: u16,
    pub bytes: Vec<u8>,
    pub content_type: Option<String>,
}
// fetch(url, {signal:AbortSignal.timeout(ms)}); a timeout of 0 means none.
pub fn fetch(url: &str, timeout_ms: u64) -> Result<Fetch, String> {
    let mut builder = reqwest::blocking::Client::builder();
    if timeout_ms > 0 {
        builder = builder.timeout(std::time::Duration::from_millis(timeout_ms));
    }
    let client = builder.build().map_err(|e| e.to_string())?;
    let response = client.get(url).send().map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let bytes = response.bytes().map_err(|e| e.to_string())?;
    Ok(Fetch { status, bytes: bytes.to_vec(), content_type })
}

pub fn sha256(bytes: &[u8]) -> String {
    baal_service_runtime::json_text::sha256(bytes)
}
pub fn sha1(bytes: &[u8]) -> String {
    format!("{:x}", <sha1::Sha1 as sha1::Digest>::digest(bytes))
}
pub fn md5(bytes: &[u8]) -> String {
    format!("{:x}", md5::compute(bytes))
}
// console.log(JSON.stringify(value)) — JS number/array formatting, not serde's.
pub fn log_json(value: &V) {
    println!("{}", baal_service_runtime::json_text::js_stringify(value));
}
// Number.prototype.toFixed — fraction digits with JS sign handling.
pub fn to_fixed(x: f64, digits: usize) -> String {
    if x.is_nan() {
        return "NaN".to_string();
    }
    if x.is_infinite() {
        return if x < 0.0 { "-Infinity" } else { "Infinity" }.to_string();
    }
    format!("{:.*}", digits, x)
}
// encodeURIComponent: unreserved = A-Z a-z 0-9 - _ . ! ~ * ' ( )
pub fn encode_uri_component(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        let keep = b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b);
        if keep {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
