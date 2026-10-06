// data/fit-poll-horizon-20261005-v2.mjs — fit the 29-day poll horizon model on
// hash-pinned sources and value the current Kansas known-field margin.
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};

fn sha(b: &[u8]) -> String { baal_service_runtime::json_text::sha256(b) }
fn need(ok: bool, code: &str) -> Result<(), String> { if ok { Ok(()) } else { Err(code.into()) } }
fn read_json(p: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn now_iso() -> String {
    baal_service_runtime::contract::ms_to_iso(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as f64)
}
fn localappdata() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).ok_or_else(|| "LOCALAPPDATA".to_string())
}
fn write_wx(path: &Path, body: &[u8]) -> Result<(), String> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; o.mode(0o600); }
    let mut f = o.open(path).map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut f, body).map_err(|e| e.to_string())
}

/// Top-level script body; returns the console.log summary object.
pub fn run() -> Result<V, String> {
    let sources = read_json(Path::new("research/poll-calibration-v1/sources.json"))?;
    let dir = localappdata()?.join("SigTradingCup/paper-runtime/poll-calibration-sources-20261005");
    for s in sources["captures"].as_array().cloned().unwrap_or_default() {
        need(sha(&std::fs::read(dir.join(s["file"].as_str().unwrap_or(""))).map_err(|e| e.to_string())?)
            == s["sha256"].as_str().unwrap_or(""), "SOURCE_CHANGED")?;
    }
    let senate_bytes = std::fs::read("research/election-baseline-v1/sources/1976-2024-senate-state.csv").map_err(|e| e.to_string())?;
    let override_bytes = std::fs::read("research/election-baseline-v2/final-stage-overrides.json").map_err(|e| e.to_string())?;
    let manifest = read_json(Path::new("research/election-baseline-v2/model-summary-affiliation-invariant.json"))?;
    let expected_senate = manifest["sourceManifests"][0]["files"].as_array().cloned().unwrap_or_default()
        .into_iter().find(|f| f["filename"].as_str() == Some("1976-2024-senate-state.csv"))
        .and_then(|f| f["sha256"].as_str().map(|s| s.to_string())).unwrap_or_default();
    need(sha(&senate_bytes) == expected_senate
        && sha(&override_bytes) == manifest["overridesHash"].as_str().unwrap_or(""), "FINAL_SOURCE_CHANGED")?;
    let raw_csv = String::from_utf8(std::fs::read(dir.join("raw_polls.csv")).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let raw_rows = crate::election_baseline_v1__model::parse_csv(&raw_csv)?;
    let senate_rows = crate::election_baseline_v1__model::parse_csv(
        &String::from_utf8_lossy(&senate_bytes))?;
    let overrides: Vec<V> = serde_json::from_slice(&override_bytes).map_err(|e| e.to_string())?;
    let senate = crate::election_baseline_v2__model::senate_races(&senate_rows, &overrides, true)?;
    let observations = crate::poll_calibration_v1__model::poll_observations(&raw_rows, &senate)?;
    let obs_rows = observations["rows"].as_array().cloned().unwrap_or_default();
    let model = crate::poll_horizon_v2::fit_horizon_error(&obs_rows, 2022)?;
    let diagnostic = crate::poll_horizon_v2::chronological_horizon_diagnostic(&obs_rows, None)?;
    let audit = read_json(Path::new("data/kansas-poll-audit-20261005-v2.json"))?;
    let as_of = audit["at"].as_str().unwrap_or("").to_string();
    let current = crate::poll_horizon_v2::current_known_margin(
        audit["polls"].as_array().map(|a| a.as_slice()).unwrap_or(&[]), "2026-11-03", &as_of)?;
    let central = crate::poll_horizon_v2::value_poll_margin(&model, current["margin"].as_f64().unwrap_or(0.0), None)?;
    let shifts: Vec<V> = [-3.0, 0.0, 3.0].iter().map(|shift| {
        json!({"shiftPoints": shift,
            "value": crate::poll_horizon_v2::value_poll_margin(&model, current["margin"].as_f64().unwrap_or(0.0), Some(*shift)).unwrap_or(f64::NAN)})
    }).collect();
    let siena = crate::poll_horizon_v2::value_poll_margin(&model, audit["meanMarginPoints"].as_f64().unwrap_or(0.0), None)?;
    let out = json!({"schemaVersion": 1, "at": now_iso(), "definition": crate::poll_horizon_v2::definition(),
        "sourceHashes": sources["captures"].as_array().unwrap_or(&vec![]).iter()
            .map(|s| json!({"file": s["file"], "sha256": s["sha256"]})).collect::<Vec<_>>(),
        "finalSourceHash": sha(&senate_bytes), "overridesHash": sha(&override_bytes),
        "model": model, "diagnostic": diagnostic,
        "current": {"asOf": as_of, "marketId": "356", "exchangeId": "1045", "knownFieldMargin": current,
            "centralSettlementValue": central, "marginShiftScenarios": shifts,
            "unknownFieldSienaEqualMarginSensitivity": siena, "newEntryEligible": false},
        "competitionOrders": 0, "profitabilityEstablished": false,
        "limitations": ["Revised outcome/field-date archive lacks original release clocks and population; diagnostics are retrospective, not trading P&L.",
            "Common-cycle correlation is an estimate from few cycles, not an empirically verified2026 joint distribution.",
            "Siena fieldwork is excluded from automatic recency weighting; equal-margin sensitivity remains separately unvalidated.",
            "No source-clock renewal, active bank change, new paper wallet or contest activation follows from these estimates."]});
    write_wx(Path::new("data/poll-horizon-results-20261005-v2.json"),
        format!("{}\n", serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?).as_bytes())?;
    Ok(json!({"races": out["model"]["races"], "cycles": out["model"]["cycles"].as_array().map(|a| a.len()).unwrap_or(0),
        "errorSd": out["model"]["standardDeviationPoints"], "correlationProxy": out["model"]["commonCorrelationProxy"],
        "diagnostic": {"races": out["diagnostic"]["races"], "brier": out["diagnostic"]["brier"]},
        "current": out["current"]}))
}
