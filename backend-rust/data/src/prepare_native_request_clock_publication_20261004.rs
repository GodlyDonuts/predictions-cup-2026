// data/prepare-native-request-clock-publication-20261004.mjs
use crate::publication_pack::{elements, read_json, write_pack};
use serde_json::{json, Value as V};
use std::path::Path;

const FILES: &[&str] = &[
    "backend-rust/src/types.rs", "backend-rust/src/engine.rs", "backend-rust/src/evaluation.rs",
    "live-paper/rust-clock-v2.test.mjs", "data/freeze-native-request-clock-20261004.mjs",
    "data/archive-native-request-clock-first-20261004.mjs", "data/verify-native-request-clock-20261004.mjs",
    "data/native-request-clock-evidence-helpers-20261004.mjs", "data/record-native-request-clock-20261004.mjs",
    "data/native-request-clock-declaration-20261004.json", "data/native-request-clock-milestone-20261004.json",
    "data/native-request-clock-milestone-20261004-v1.json",
    "data/prepare-native-request-clock-publication-20261004.mjs", "docs/BAAL.md",
];
pub fn run() -> Result<V, String> {
    let e = read_json(Path::new("data/native-request-clock-milestone-20261004.json"))?;
    if e["checks"]["nodePassed"] != json!(21) || e["checks"]["rustPassed"] != json!(2)
        || e["checks"]["skipped"] != json!(0) || e["checks"]["legacyHistoricalCardsExact"] != json!(180)
        || e["competitionOrders"] != json!(0) || e["profitabilityEstablished"] != json!(false)
        || e["services"]["collectorStatus"] != json!("running")
    {
        return Err("assert".into());
    }
    let els = elements(FILES, crate::secret_scan::scan_a, |p| format!("Secret-like content {p}"))?;
    write_pack("data/native-request-clock-publication-elements.json", &els, false)
}
