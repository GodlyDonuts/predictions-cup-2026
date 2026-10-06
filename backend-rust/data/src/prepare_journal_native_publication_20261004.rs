// data/prepare-journal-native-publication-20261004.mjs
use crate::publication_pack::{elements, write_pack};
use serde_json::Value as V;

const FILES: &[&str] = &[
    ".gitignore", "backend-rust/journal-restore/Cargo.toml", "backend-rust/journal-restore/Cargo.lock",
    "backend-rust/journal-restore/build.rs", "backend-rust/journal-restore/build.ps1",
    "backend-rust/journal-restore/src/main.rs", "backend-rust/journal-restore/src/canonical.rs",
    "live-paper/native-journal-restore.mjs", "live-paper/native-journal-restore.test.mjs",
    "live-paper/collector-journal-native.mjs", "live-paper/collector-journal-native.test.mjs",
    "live-paper/runner.mjs",
    "data/archive-journal-native-20261004.mjs", "data/benchmark-journal-native-20261004.mjs",
    "data/prepare-journal-native-20261004.mjs", "data/deploy-journal-native-20261004.ps1",
    "data/verify-journal-native-20261004.mjs", "data/record-journal-native-20261004.mjs",
    "data/prepare-journal-native-publication-20261004.mjs",
    "data/journal-native-binding-20261004.json", "data/journal-native-benchmark-20261004.json",
    "data/journal-native-benchmark-20261004-v1.json", "data/journal-native-benchmark-20261004-v2.json",
    "data/journal-native-preservation-20261004.json", "data/journal-native-deployment-20261004.json",
    "data/journal-native-live-20261004.json", "data/journal-native-milestone-20261004.json",
    "backend-rust/README.md", "docs/BAAL.md",
];
pub fn run() -> Result<V, String> {
    let els = elements(FILES, crate::secret_scan::ace_jwt_check, |_p| "Secret-like publication content".to_string())?;
    write_pack("data/journal-native-publication-elements.json", &els, false)
}
