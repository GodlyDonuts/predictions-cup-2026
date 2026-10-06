// live-paper/experiment-retirement.mjs
use serde_json::Value as V;
use std::path::Path;
const MARKERS: [&str; 4] = ["DIP_STOP", "SHOCK_STOP", "ENTRY_COMPARISON_STOP", "NATIVE_PRICE_PAPER_STOP"];
/// paperExperimentRetired(runtimeDir, marker)
pub fn paper_experiment_retired(runtime_dir: &Path, marker: &str) -> Result<bool, String> {
    if !MARKERS.contains(&marker) { return Err("INVALID_RETIREMENT_SCOPE".into()); }
    let file = runtime_dir.join(marker);
    let check = || -> Option<bool> {
        let stat = std::fs::metadata(&file).ok()?;
        if !stat.is_file() || stat.len() > 4096 { return Some(false); }
        let record: V = serde_json::from_str(&std::fs::read_to_string(&file).ok()?).ok()?;
        Some(record.get("owner").and_then(V::as_str) == Some("Codex")
            && record.get("reason").and_then(V::as_str) == Some("Retired obsolete experiment at human request")
            && record.get("restartRequires").and_then(V::as_str) == Some("Explicitly selected future experiment")
            && record.get("at").and_then(V::as_str).is_some_and(|s| baal_service_runtime::contract::parse_iso_ms(s).is_some()))
    };
    Ok(check().unwrap_or(false))
}
