// data/prepare-native-cache-publication-20261004.mjs
use crate::publication_pack::{elements, read_json, write_pack};
use serde_json::Value as V;
use std::path::Path;

const FILES: &[&str] = &[
    "backend-rust/src/types.rs", "backend-rust/src/capture.rs", "backend-rust/src/cache.rs",
    "backend-rust/src/main.rs", "backend-rust/build.rs", "backend-rust/README.md",
    "live-paper/rust-cache-v1.test.mjs", "live-paper/native-dip-host-v2.mjs",
    "live-paper/native-dip-host-v2.test.mjs", "live-paper/runner.mjs", "live-paper/runner-lifecycle.test.mjs",
    "data/benchmark-native-cache-20261004.mjs", "data/prepare-native-cache-host-20261004.mjs",
    "data/native-cache-preservation-20261004.mjs", "data/deploy-native-cache-host-20261004.ps1",
    "data/record-native-cache-20261004.mjs", "data/prepare-native-cache-publication-20261004.mjs",
    "data/native-cache-benchmark-20261004.json", "data/native-cache-host-preparation-20261004.json",
    "data/native-cache-preservation-20261004.json", "data/native-cache-preservation-after-20261004.json",
    "data/native-cache-host-deployment-20261004.json", "data/native-cache-milestone-20261004.json",
    "docs/BAAL.md",
];
fn eq(v: &V, x: &V, code: &str) -> Result<(), String> {
    if v == x { Ok(()) } else { Err(code.to_string()) }
}
pub fn run() -> Result<V, String> {
    let e = read_json(Path::new("data/native-cache-milestone-20261004.json"))?;
    eq(&e["checks"]["nodePassed"], &json!(132), "assert")?;
    eq(&e["checks"]["rustPassed"], &json!(2), "assert")?;
    eq(&e["checks"]["nodeFailed"], &json!(0), "assert")?;
    eq(&e["checks"]["rustFailed"], &json!(0), "assert")?;
    eq(&e["checks"]["skipped"], &json!(0), "assert")?;
    eq(&e["services"]["status"], &json!("running"), "assert")?;
    eq(&e["services"]["persistent"], &json!(3), "assert")?;
    eq(&e["competitionOrders"], &json!(0), "assert")?;
    eq(&e["fees"], &json!(0), "assert")?;
    eq(&e["profitabilityEstablished"], &json!(false), "assert")?;
    eq(&e["benchmark"]["oldModelEngineCardsExact"], &json!(100), "assert")?;
    eq(&e["benchmark"]["cachedFullCardsExact"], &json!(100), "assert")?;
    let els = elements(FILES, crate::secret_scan::scan_a, |p| format!("Secret-like content {p}"))?;
    write_pack("data/native-cache-publication-elements.json", &els, false)
}
use serde_json::json;
