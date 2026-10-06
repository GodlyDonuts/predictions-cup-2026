// Shared shape of the prepare-*-publication / prepare-*-pack data scripts:
// verify no secret-like content in each file, emit a JSON elements array.
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Value as V};
use std::path::Path;

pub fn read_utf8(p: &Path) -> Result<String, String> {
    std::fs::read_to_string(p).map_err(|e| format!("{e}"))
}
pub fn sha(b: &[u8]) -> String { sha256(b) }
/// Build the `[{path,mode:'100644',type:'blob',content}]` array after the
/// secret scan, exactly like `files.map(...)` in the JS scripts.
pub fn elements(files: &[&str], scan: impl Fn(&str) -> bool, msg: impl Fn(&str) -> String) -> Result<Vec<V>, String> {
    let mut out = Vec::new();
    for path in files {
        let content = read_utf8(Path::new(path))?;
        if scan(&content) { return Err(msg(path)); }
        out.push(json!({"path": path, "mode": "100644", "type": "blob", "content": content}));
    }
    Ok(out)
}
/// Write `JSON.stringify(elements)` (optionally + "\n") and return the console
/// summary value the JS script prints.
pub fn write_pack(out_file: &str, elements: &[V], trailing_newline: bool) -> Result<V, String> {
    let text = serde_json::to_string(&json!(elements)).map_err(|e| e.to_string())?;
    let mut body = text.clone();
    if trailing_newline { body.push('\n'); }
    std::fs::write(out_file, body).map_err(|e| e.to_string())?;
    Ok(json!({"files": elements.len(), "chars": text.len(), "hash": sha(text.as_bytes())}))
}
/// `--chunk` handler: returns the pack file slice the script prints.
pub fn chunk(pack_file: &str, n: i64, size: usize) -> Result<String, String> {
    if n < 0 { return Err("CHUNK".into()); }
    let text = std::fs::read_to_string(pack_file).map_err(|e| e.to_string())?;
    let start = (n as usize) * size;
    let end = ((n as usize) + 1) * size;
    Ok(text.chars().skip(start).take(end - start).collect())
}
pub fn now_iso() -> String {
    baal_service_runtime::contract::ms_to_iso(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as f64,
    )
}
pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as i64
}
pub fn runtime_dir() -> String {
    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
    format!("{local}/SigTradingCup/paper-runtime")
}
pub fn read_json(p: &Path) -> Result<V, String> {
    serde_json::from_str(&read_utf8(p)?).map_err(|e| e.to_string())
}
