// data/prepare-pair-anchor-publication-20261004.mjs
use crate::publication_pack::{elements, write_pack};
use serde_json::Value as V;

const FILES: &[&str] = &[
    "live-paper/baal-pair-paper-v3.mjs", "live-paper/baal-pair-paper-v3.test.mjs",
    "live-paper/baal-pair-host-v4.mjs", "live-paper/baal-pair-host-v4.test.mjs", "live-paper/runner.mjs",
    "data/diagnose-pair-bottleneck-20261004.mjs", "data/compare-pair-anchor-20261004.mjs",
    "data/prepare-pair-anchor-20261004.mjs", "data/recover-pair-anchor-preparation-20261004.mjs",
    "data/check-pair-anchor-prepared-20261004.mjs", "data/deploy-pair-anchor-20261004.ps1",
    "data/verify-pair-anchor-20261004.mjs", "data/record-pair-anchor-20261004.mjs",
    "data/prepare-pair-anchor-publication-20261004.mjs",
    "data/pair-bottleneck-20261004.json", "data/pair-anchor-comparison-20261004.json",
    "data/baal-pair-anchor-bank-20261004-v4.json", "data/pair-anchor-preservation-20261004.json",
    "data/pair-anchor-deployment-20261004.json", "data/pair-anchor-live-20261004.json",
    "data/pair-anchor-milestone-20261004.json", "docs/BAAL.md",
];
pub fn run() -> Result<V, String> {
    let els = elements(FILES, crate::secret_scan::ace_jwt_check, |_f| "Secret-like content in publication".to_string())?;
    write_pack("data/pair-anchor-publication-elements.json", &els, false)
}
