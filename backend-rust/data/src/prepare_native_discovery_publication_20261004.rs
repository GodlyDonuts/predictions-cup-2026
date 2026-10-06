// data/prepare-native-discovery-publication-20261004.mjs
use crate::publication_pack::{elements, read_json, write_pack};
use serde_json::{json, Value as V};
use std::path::Path;

const FILES: &[&str] = &[
    "live-paper/collector-market-screen-v1.mjs", "live-paper/collector-market-screen-v1.test.mjs",
    "live-paper/native-discovery-plan-v1.mjs", "live-paper/native-discovery-plan-v1.test.mjs",
    "live-paper/runner.mjs", "live-paper/runner-lifecycle.test.mjs",
    "data/benchmark-native-discovery-20261004.mjs", "data/native-discovery-preservation-20261004.mjs",
    "data/deploy-native-discovery-20261004.ps1", "data/record-native-discovery-20261004.mjs",
    "data/prepare-native-discovery-publication-20261004.mjs", "data/native-discovery-baseline-20261004.json",
    "data/native-discovery-benchmark-20261004.json", "data/native-discovery-preservation-20261004.json",
    "data/native-discovery-preservation-after-20261004.json", "data/native-discovery-deployment-20261004.json",
    "data/native-discovery-milestone-20261004.json", "docs/BAAL.md",
];
pub fn run() -> Result<V, String> {
    let e = read_json(Path::new("data/native-discovery-milestone-20261004.json"))?;
    if e["status"] != json!("verified_global_discovery_deployed")
        || e["checks"]["passed"] != json!(111) || e["checks"]["failed"] != json!(0)
        || e["checks"]["skipped"] != json!(0) || e["competitionOrders"] != json!(0)
        || e["services"]["persistent"] != json!(3)
        || e["benchmark"]["samePrefixCountsAndLeadsExact"] != json!(true)
        || e["walletsReset"] != json!(false) || e["profitabilityEstablished"] != json!(false)
    {
        return Err("assert".into());
    }
    let els = elements(FILES, crate::secret_scan::scan_a, |p| format!("Secret-like content {p}"))?;
    write_pack("data/native-discovery-publication-elements.json", &els, false)
}
