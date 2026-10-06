// data/prepare-human-assignment-publication-20261004.mjs — publication pack;
// --chunk N prints a 10k slice.
use crate::publication_pack::{chunk, elements, read_json, sha};
use serde_json::{json, Value as V};
use std::path::Path;

const BASE: &str = "research/election-baseline-v1";
const PACK: &str = "data/human-assignment-publication-pack-20261004.json";
pub fn human_assignment_publication_chunk(n: i64) -> Result<String, String> {
    chunk(PACK, n, 10_000)
}
pub fn run() -> Result<V, String> {
    let mut files = vec![
        "execution/production-activation.mjs".to_string(), "execution/production-authority.test.mjs".to_string(),
        "execution/production-reconciliation.mjs".to_string(), "execution/production-reconciliation.test.mjs".to_string(),
        "execution/production-launch.md".to_string(), "execution/README.md".to_string(),
        "live-paper/transport-network-budget.mjs".to_string(), "live-paper/runner.mjs".to_string(),
        "coordination/execution_state.json".to_string(), "docs/COORDINATION.md".to_string(),
        "data/reconcile-production-human-20261004.mjs".to_string(),
        "data/prepare-human-assignment-publication-20261004.mjs".to_string(),
    ];
    for f in [
        "README.md", "definition.json", "definition-before-house-access.json", "download-sources.mjs",
        "model.mjs", "model.test.mjs", "train.mjs", "current-inputs.json", "model-summary.json",
        "source-crosschecks.json", "sources/PEJ5QU-manifest.json", "sources/42MVDX-manifest.json",
    ] {
        files.push(format!("{BASE}/{f}"));
    }
    let report_file = format!("{BASE}/model-report.json");
    let r = read_json(Path::new(&report_file))?;
    let mut summary = serde_json::Map::new();
    for k in [
        "schemaVersion", "id", "at", "definitionHash", "sourceManifests", "sourceIntegrityVerified",
        "rows", "includedCycles", "selectedLambda", "selectedUsingYearsThrough", "residualRms",
        "model", "modelHash", "check", "forecasts", "forecastVintagesAvailable",
        "pollingTransportValidatedByHistoricalCheck", "calibrationEstablishedFor2026",
        "qualifiedProductionForecasts", "ordersSubmitted", "paperFills", "profitabilityEstablished",
        "limitations",
    ] {
        summary.insert(k.to_string(), r.get(k).cloned().unwrap_or(V::Null));
    }
    summary.insert("fullLocalReportSha256".into(), json!(sha(&std::fs::read(&report_file).map_err(|e| e.to_string())?)));
    summary.insert("excludedRaces".into(), json!(r.get("exclusions").and_then(|x| x.as_array()).map(|a| a.len()).unwrap_or(0)));
    summary.insert("development".into(), json!(r.get("development").and_then(|d| d.as_array()).map(|ds| {
        ds.iter().map(|d| json!({
            "lambda": d.get("lambda"), "metrics": d.get("metrics"), "cycleMse": d.get("cycleMse"),
        })).collect::<Vec<V>>()
    }).unwrap_or_default()));
    summary.insert("additionalReviewLimitations".into(), json!([
        "Screened historical two-party probabilities are not unconditional party-win forecasts when a third-party candidate can win.",
        "WV current official ballot has S.Marshall Wilson(CST); original draft candidate alias Rachel Anderson is Rachel Fetty Anderson on the official ballot.",
        "No fresh source eligibility is created by publishing this summary.",
    ]));
    let summary_file = format!("{BASE}/model-summary.json");
    if !Path::new(&summary_file).exists() {
        std::fs::write(&summary_file, serde_json::to_string_pretty(&V::Object(summary)).map_err(|e| e.to_string())? + "\n")
            .map_err(|e| e.to_string())?;
    }
    let refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();
    let els = elements(&refs, crate::secret_scan::pack_check, |p| format!("SECRET_CHECK {p}"))?;
    let bytes = serde_json::to_string(&els).map_err(|e| e.to_string())?;
    std::fs::write(PACK, &bytes).map_err(|e| e.to_string())?;
    Ok(json!({
        "files": files.len(),
        "characters": bytes.len(),
        "chunks": (bytes.len() + 9999) / 10_000,
        "sha256": sha(bytes.as_bytes()),
    }))
}
