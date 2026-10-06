// data/prepare-native-dip-host-publication-20261004.mjs
use crate::publication_pack::{elements, read_json, write_pack};
use serde_json::{json, Value as V};
use std::path::Path;

const FILES: &[&str] = &[
    "live-paper/native-dip-proof-v1.mjs", "live-paper/native-dip-host-v1.mjs",
    "live-paper/native-dip-host-v1.test.mjs", "live-paper/runner.mjs",
    "live-paper/runner-lifecycle.test.mjs", "data/prepare-native-dip-host-20261004.mjs",
    "data/native-dip-host-preservation-20261004.mjs", "data/deploy-native-dip-host-20261004.ps1",
    "data/record-native-dip-host-20261004.mjs", "data/prepare-native-dip-host-publication-20261004.mjs",
    "data/native-dip-host-preparation-20261004.json", "data/native-dip-host-preservation-20261004.json",
    "data/native-dip-host-preservation-after-20261004.json", "data/native-dip-host-deployment-20261004.json",
    "data/native-dip-host-milestone-20261004.json", "docs/BAAL.md",
];
pub fn run() -> Result<V, String> {
    let e = read_json(Path::new("data/native-dip-host-milestone-20261004.json"))?;
    for (k, v) in [("passed", json!(96)), ("failed", json!(0)), ("skipped", json!(0))] {
        if e["checks"][k] != v { return Err("assert".into()); }
    }
    if e["services"]["status"] != json!("running") || e["services"]["persistent"] != json!(3)
        || e["competitionOrders"] != json!(0) || e["fees"] != json!(0)
        || e["profitabilityEstablished"] != json!(false)
    {
        return Err("assert".into());
    }
    let els = elements(FILES, crate::secret_scan::scan_a, |p| format!("Secret-like content {p}"))?;
    write_pack("data/native-dip-host-publication-elements.json", &els, false)
}
