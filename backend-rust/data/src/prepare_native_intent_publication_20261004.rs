// data/prepare-native-intent-publication-20261004.mjs — assert the native-intent
// milestone, then pack the publication file set.
use crate::publication_pack::{elements, read_json, write_pack};
use serde_json::{json, Value as V};
use std::path::Path;

fn assert_eq(a: &V, b: &V) -> Result<(), String> {
    if a == b { Ok(()) } else { Err("assert".into()) }
}
const FILES: &[&str] = &[
    "backend-rust/src/main.rs", "backend-rust/src/intent_observation.rs",
    "backend-rust/src/evaluation.rs", "backend-rust/build.rs", "backend-rust/README.md",
    "live-paper/native-intent-proof-v1.mjs", "live-paper/native-intent-proof-v1.test.mjs",
    "live-paper/native-dip-host-v3.mjs", "live-paper/native-dip-host-v3.test.mjs",
    "live-paper/runner.mjs", "live-paper/runner-lifecycle.test.mjs",
    "data/prepare-native-intent-host-20261004.mjs", "data/native-intent-preservation-20261004.mjs",
    "data/deploy-native-intent-20261004.ps1", "data/record-native-intent-20261004.mjs",
    "data/prepare-native-intent-publication-20261004.mjs",
    "data/native-intent-preparation-20261004.json", "data/native-intent-preservation-20261004.json",
    "data/native-intent-preservation-after-20261004.json", "data/native-intent-deployment-20261004.json",
    "data/native-intent-milestone-20261004.json", "docs/BAAL.md",
];
pub fn run() -> Result<V, String> {
    let e = read_json(Path::new("data/native-intent-milestone-20261004.json"))?;
    assert_eq(&e["checks"]["nodePassed"], &json!(171))?;
    assert_eq(&e["checks"]["rustPassed"], &json!(2))?;
    assert_eq(&e["checks"]["failed"], &json!(0))?;
    assert_eq(&e["checks"]["skipped"], &json!(0))?;
    assert_eq(&e["parity"]["originalScorecardsExact"], &json!(100))?;
    assert_eq(&e["parity"]["observedCachedScorecardsExact"], &json!(100))?;
    assert_eq(&e["services"]["persistent"], &json!(3))?;
    assert_eq(&e["competitionOrders"], &json!(0))?;
    assert_eq(&e["walletsReset"], &json!(false))?;
    assert_eq(&e["profitabilityEstablished"], &json!(false))?;
    let els = elements(FILES, crate::secret_scan::scan_a,
        |p| format!("Secret-like content {p}"))?;
    write_pack("data/native-intent-publication-elements.json", &els, false)
}
