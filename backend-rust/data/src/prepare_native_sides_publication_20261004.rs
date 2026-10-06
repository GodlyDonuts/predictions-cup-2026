// data/prepare-native-sides-publication-20261004.mjs
use crate::publication_pack::{elements, read_json, sha, write_pack};
use serde_json::{json, Value as V};
use std::path::Path;

const FILES: &[&str] = &[
    "backend-rust/src/main.rs", "backend-rust/src/quote_sides.rs", "backend-rust/src/quote_endpoints.rs",
    "backend-rust/build.rs", "backend-rust/README.md", "docs/BAAL.md",
    "live-paper/native-quote-markouts-fixture.mjs", "live-paper/native-quote-markouts.test.mjs",
    "live-paper/native-quote-sides.test.mjs", "data/run-native-quote-sides-20261004.mjs",
    "data/finish-native-sides-preservation-20261004.mjs", "data/record-native-sides-milestone-20261004.mjs",
    "data/prepare-native-sides-publication-20261004.mjs", "data/native-sides-milestone-20261004.json",
    "data/native-sides-preservation-20261004.json",
];
pub fn run() -> Result<V, String> {
    let e = read_json(Path::new("data/native-sides-milestone-20261004.json"))?;
    if e["tests"]["node"] != json!(163) || e["tests"]["rust"] != json!(9)
        || e["tests"]["failures"] != json!(0) || e["originalWallets"]["cards"] != json!(72)
        || e["live"]["additionalPersistentProcesses"] != json!(0) || e["competitionOrders"] != json!(0)
        || e["profitabilityEstablished"] != json!(false)
    {
        return Err("assert".into());
    }
    for (file, hash) in e["sourceHashes"].as_object().into_iter().flatten() {
        if sha(&std::fs::read(file).map_err(|e| e.to_string())?) != *hash {
            return Err(format!("assert {file}"));
        }
    }
    let els = elements(FILES, crate::secret_scan::scan_a, |p| format!("Secret-like content {p}"))?;
    write_pack("data/native-sides-publication-elements.json", &els, false)
}
