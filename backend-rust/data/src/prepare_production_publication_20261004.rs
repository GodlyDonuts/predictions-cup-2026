// data/prepare-production-publication-20261004.mjs — pack + --chunk 12000
use crate::publication_pack::{chunk, elements, sha};
use serde_json::{json, Value as V};

const PACK: &str = "data/production-integration-publication-pack-20261004.json";
const FILES: &[&str] = &[
    "execution/production-activation.mjs", "execution/controller.mjs",
    "execution/settlement-controller.mjs", "execution/settlement-controller.test.mjs",
    "execution/transport.mjs", "execution/production-launch.md", "execution/README.md",
    "live-paper/transport-network-budget.mjs", "live-paper/market-sweep.mjs", "live-paper/runner.mjs",
    "live-paper/runner-lifecycle.test.mjs", "live-paper/production-network-budget.test.mjs",
    "live-paper/production-metadata.mjs", "live-paper/production-metadata.test.mjs",
    "data/deploy-production-collector-20261004.mjs", "data/production-collector-deployment-20261004.json",
    "data/prepare-production-bank-20261004.mjs", "data/production-bank-stage-20261004.json",
    "data/kansas-current-contract-20261004.json", "data/prepare-production-publication-20261004.mjs",
];
pub fn production_publication_chunk(n: i64) -> Result<String, String> {
    chunk(PACK, n, 12_000)
}
pub fn run() -> Result<V, String> {
    let els = elements(FILES, crate::secret_scan::pack_check, |p| format!("SECRET_CHECK {p}"))?;
    let bytes = serde_json::to_string(&els).map_err(|e| e.to_string())?;
    std::fs::write(PACK, &bytes).map_err(|e| e.to_string())?;
    Ok(json!({
        "files": els.len(),
        "characters": bytes.len(),
        "chunks": (bytes.len() + 11_999) / 12_000,
        "sha256": sha(bytes.as_bytes()),
    }))
}
