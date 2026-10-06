// data/prepare-native-mechanisms-publication-20261004.mjs
use crate::publication_pack::{elements, read_json, sha, write_pack};
use serde_json::{json, Value as V};
use std::path::Path;

const FILES: &[&str] = &[
    "backend-rust/src/main.rs", "backend-rust/src/quote_markouts.rs", "backend-rust/src/quote_mechanisms.rs",
    "backend-rust/build.rs", "backend-rust/README.md", "docs/BAAL.md",
    "live-paper/native-quote-markouts-fixture.mjs", "live-paper/native-quote-mechanisms.test.mjs",
    "data/prepare-native-mechanisms-20261004.mjs", "data/run-native-mechanisms-20261004.mjs",
    "data/record-native-mechanisms-milestone-20261004.mjs", "data/prepare-native-mechanisms-publication-20261004.mjs",
    "data/native-mechanisms-protocol-20261004.json", "data/native-mechanisms-evidence-20261004.json",
    "data/native-mechanisms-future-1791141201465.json", "data/native-mechanisms-milestone-20261004.json",
];
pub fn run() -> Result<V, String> {
    let e = read_json(Path::new("data/native-mechanisms-evidence-20261004.json"))?;
    for (file, hash) in e["protocol"]["sourceHashes"].as_object().into_iter().flatten() {
        if sha(&std::fs::read(file).map_err(|e| e.to_string())?) != *hash {
            return Err(format!("assert {file}"));
        }
    }
    let milestone = read_json(Path::new("data/native-mechanisms-milestone-20261004.json"))?;
    if milestone["tests"]["node"] != json!(169) || milestone["tests"]["failures"] != json!(0)
        || milestone["preservation"]["originalLiveWalletCardsExact"] != json!(72)
        || milestone["competitionOrders"] != json!(0)
        || milestone["profitabilityEstablished"] != json!(false)
    {
        return Err("assert".into());
    }
    let els = elements(FILES, crate::secret_scan::scan_a, |p| format!("Secret-like content {p}"))?;
    write_pack("data/native-mechanisms-publication-elements.json", &els, false)
}
