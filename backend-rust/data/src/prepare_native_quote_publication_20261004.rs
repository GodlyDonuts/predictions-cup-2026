// data/prepare-native-quote-publication-20261004.mjs
use crate::publication_pack::{elements, read_json, sha, write_pack};
use serde_json::{json, Value as V};
use std::path::Path;

const FILES: &[&str] = &[
    "backend-rust/src/main.rs", "backend-rust/src/market_observations.rs",
    "backend-rust/src/quote_wallet.rs", "backend-rust/src/quote_replay.rs", "backend-rust/build.rs",
    "backend-rust/README.md", "live-paper/account-eligibility-publication-v1.mjs",
    "live-paper/native-quote-proof-v1.mjs", "live-paper/native-quote-host-v1.mjs",
    "live-paper/native-quote-replay-v1.test.mjs", "live-paper/runner.mjs",
    "live-paper/runner-lifecycle.test.mjs", "data/prepare-native-quote-host-20261004.mjs",
    "data/native-quote-preservation-20261004.mjs", "data/deploy-native-quote-20261004.ps1",
    "data/verify-native-quote-live-20261004.mjs", "data/record-native-quote-milestone-20261004.mjs",
    "data/prepare-native-quote-publication-20261004.mjs", "data/native-quote-preparation-20261004.json",
    "data/native-quote-preservation-20261004.json", "data/native-quote-preservation-after-20261004.json",
    "data/native-quote-deployment-20261004.json", "data/native-quote-live-20261004.json",
    "data/native-quote-milestone-20261004.json", "docs/BAAL.md",
];
pub fn run() -> Result<V, String> {
    let e = read_json(Path::new("data/native-quote-milestone-20261004.json"))?;
    let prep = read_json(Path::new("data/native-quote-preparation-20261004.json"))?;
    if e["tests"]["node"] != json!(104) || e["tests"]["rust"] != json!(3)
        || e["tests"]["failures"] != json!(0) || e["originalScorecardsExact"] != json!(100)
        || e["live"]["persistentProcesses"] != json!(3) || e["competitionOrders"] != json!(0)
        || e["profitabilityEstablished"] != json!(false)
    {
        return Err("assert".into());
    }
    for (name, hash) in prep["sourceHashes"].as_object().into_iter().flatten() {
        if sha(&std::fs::read(name).map_err(|e| e.to_string())?) != *hash {
            return Err(format!("assert {name}"));
        }
    }
    let els = elements(FILES, crate::secret_scan::scan_a, |p| format!("Secret-like content {p}"))?;
    write_pack("data/native-quote-publication-elements.json", &els, false)
}
