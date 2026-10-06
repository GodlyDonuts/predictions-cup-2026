// live-paper/expired-flat-paper-retirement.mjs — verified-flat paper retirement.
use baal_execution::core::payload_hash;
use baal_service_runtime::json_text::js_truthy;
use serde_json::{json, Value as V};
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

fn now_ms() -> f64 { SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0) }
fn date_parse(v: &V) -> Option<f64> { v.as_str().and_then(baal_service_runtime::contract::parse_iso_ms) }
// JS String(v) for the scalar shapes .test()/path.join coerce here.
fn js_str(v: Option<&V>) -> String {
    match v {
        None => "undefined".into(),
        Some(V::Null) => "null".into(),
        Some(V::Bool(b)) => b.to_string(),
        Some(V::String(s)) => s.clone(),
        Some(other) => baal_service_runtime::json_text::js_stringify(other),
    }
}
// /^[a-z][a-z0-9-]{1,80}$/ — first letter plus 1-80 tail chars.
fn cohort_ok(s: &str) -> bool {
    let b = s.as_bytes();
    s.len() >= 2 && s.len() <= 81 && b[0].is_ascii_lowercase()
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// flatExpiredPaper(status, expiresAt, now=Date.now())
pub fn flat_expired_paper(status: &V, expires_at: &V, now: f64) -> bool {
    if status["failed"] != V::Bool(false)
        || !status["riskMarketIds"].as_array().is_some_and(|a| a.is_empty())
        || date_parse(expires_at).map(|e| e <= now) != Some(true)
    {
        return false;
    }
    if js_truthy(status.get("wallets")) {
        let wallets: Vec<&V> = match &status["wallets"] {
            V::Object(m) => m.values().collect(),
            V::Array(a) => a.iter().collect(),
            _ => Vec::new(),
        };
        return !wallets.is_empty()
            && wallets.iter().all(|w| {
                w["positions"].as_array().is_some_and(|p| p.is_empty())
                    && w["unresolvedQuotes"].as_array().is_some_and(|q| q.is_empty())
                    && w["reserved"].as_str() == Some("0.000000")
            });
    }
    status["positions"].as_array().is_some_and(|p| p.is_empty())
        && status.get("pending") == Some(&V::Null)
        && status.get("package") == Some(&V::Null)
        && status["reserved"].as_str() == Some("0.000000")
}

/// readExpiredFlatPaperRetirement(runtimeDir, pointerFile, folder)
pub fn read_expired_flat_paper_retirement(runtime_dir: &Path, pointer_file: &str, folder: &str) -> Result<V, String> {
    let file = runtime_dir.join(pointer_file);
    if !file.exists() {
        return Ok(V::Null);
    }
    let active: V = serde_json::from_str(&fs::read_to_string(&file).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let cohort = js_str(active.get("cohort"));
    if !cohort_ok(&cohort) {
        return Err("RETIREMENT_SCOPE".into());
    }
    // path.join requires real strings: a non-string cohort coerced through the
    // regex above (e.g. null -> "null") throws TypeError here in JS.
    if !active["cohort"].is_string() {
        return Err("The \"paths[2]\" argument must be of type string".into());
    }
    let dir = runtime_dir.join(folder).join(&cohort);
    let marker = dir.join("STOP");
    if !marker.exists() {
        return Ok(V::Null);
    }
    let m: V = serde_json::from_str(&fs::read_to_string(&marker).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if m["source"].as_str() != Some("expired_flat_paper_retirement_20261005") {
        return Ok(V::Null);
    }
    let bank: V = serde_json::from_str(&fs::read_to_string(dir.join("bank.json")).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if m["owner"].as_str() != Some("Codex")
        || m["bankHash"] != active["bankHash"]
        || payload_hash(&bank)? != m["bankHash"].as_str().unwrap_or("")
        || !flat_expired_paper(&m["verifiedStatus"], &m["reviewExpiresAt"], now_ms())
        || payload_hash(&m["verifiedStatus"])? != m["statusHash"].as_str().unwrap_or("")
    {
        return Err("RETIREMENT_PROOF".into());
    }
    let mut out = m["verifiedStatus"].clone();
    if !out.is_object() {
        // JS spread of a non-object leaves only the appended keys.
        out = json!({});
    }
    let o = out.as_object_mut().unwrap();
    o.insert("status".into(), json!("retired"));
    o.insert("accountingPreserved".into(), json!(true));
    o.insert("retiredAt".into(), m["at"].clone());
    o.insert("retirementReason".into(), json!("expired_and_verified_flat"));
    o.insert("executionAuthority".into(), json!(false));
    Ok(out)
}
