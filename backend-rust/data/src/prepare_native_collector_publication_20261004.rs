// data/prepare-native-collector-publication-20261004.mjs
use crate::publication_pack::{elements, write_pack};
use serde_json::Value as V;

const FILES: &[&str] = &[
    "live-paper/book-publication-v3.mjs", "live-paper/book-publication-v3.test.mjs",
    "live-paper/collector-publication-native.mjs", "live-paper/runner.mjs",
    "data/prepare-native-collector-20261004.mjs", "data/deploy-native-collector-20261004.ps1",
    "data/verify-native-collector-20261004.mjs", "data/benchmark-native-collector-20261004.mjs",
    "data/record-native-collector-20261004.mjs", "data/prepare-native-collector-publication-20261004.mjs",
    "data/native-collector-preservation-20261004.json", "data/native-collector-deployment-20261004.json",
    "data/native-collector-live-20261004.json", "data/native-collector-benchmark-20261004.json",
    "data/native-collector-milestone-20261004.json", "docs/BAAL.md", "backend-rust/README.md",
];
pub fn run() -> Result<V, String> {
    let els = elements(FILES, crate::secret_scan::ace_jwt_check, |p| format!("Secret-like content: {p}"))?;
    write_pack("data/native-collector-publication-elements.json", &els, false)
}
