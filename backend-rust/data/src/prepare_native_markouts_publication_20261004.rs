// data/prepare-native-markouts-publication-20261004.mjs — assert the native
// markouts milestone (incl. per-source hashes), then pack the publication set.
use crate::publication_pack::{elements, read_json, sha, write_pack};
use serde_json::{json, Value as V};
use std::path::Path;

fn assert_eq(a: &V, b: &V) -> Result<(), String> {
    if a == b { Ok(()) } else { Err("assert".into()) }
}
const FILES: &[&str] = &[
    "backend-rust/src/main.rs", "backend-rust/src/quote_recovery_v2.rs",
    "backend-rust/src/quote_diagnostics.rs", "backend-rust/src/quote_markouts.rs",
    "backend-rust/build.rs", "backend-rust/README.md",
    "live-paper/native-quote-markouts.test.mjs", "data/run-native-quote-markouts-20261004.mjs",
    "data/verify-native-markouts-preservation-20261004.mjs", "data/record-native-markouts-milestone-20261004.mjs",
    "data/prepare-native-markouts-publication-20261004.mjs", "data/native-markouts-milestone-20261004.json",
    "data/native-markouts-preservation-20261004.json", "docs/BAAL.md",
];
pub fn run() -> Result<V, String> {
    let e = read_json(Path::new("data/native-markouts-milestone-20261004.json"))?;
    assert_eq(&e["tests"]["node"], &json!(158))?;
    assert_eq(&e["tests"]["rust"], &json!(7))?;
    assert_eq(&e["tests"]["failures"], &json!(0))?;
    assert_eq(&e["originalWallets"]["cards"], &json!(72))?;
    assert_eq(&e["live"]["additionalPersistentProcesses"], &json!(0))?;
    assert_eq(&e["competitionOrders"], &json!(0))?;
    assert_eq(&e["profitabilityEstablished"], &json!(false))?;
    for (name, hash) in e["sourceHashes"].as_object().cloned().unwrap_or_default() {
        assert_eq(&json!(sha(&std::fs::read(&name).map_err(|e| e.to_string())?)), &hash)
            .map_err(|_| format!("assert {name}"))?;
    }
    let els = elements(FILES, crate::secret_scan::scan_a,
        |p| format!("Secret-like content {p}"))?;
    write_pack("data/native-markouts-publication-elements.json", &els, false)
}
