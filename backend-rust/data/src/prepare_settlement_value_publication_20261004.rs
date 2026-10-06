// data/prepare-settlement-value-publication-20261004.mjs — two output files
use crate::publication_pack::{elements, sha};
use serde_json::{json, Value as V};

const FILES: &[&str] = &[
    "backend-rust/src/main.rs", "backend-rust/build.rs", "backend-rust/src/settlement_value.rs",
    "live-paper/native-settlement-value.test.mjs", "live-paper/settlement-value-native.mjs",
    "live-paper/settlement-value-native.test.mjs", "live-paper/probability-native-host.test.mjs",
    "live-paper/settlement-value-policy.md",
    "data/archive-checked-native-graph.mjs", "data/check-settlement-value-20261004.mjs",
    "data/verify-settlement-native-archive-20261004.mjs", "data/capture-kansas-lead-20261004.mjs",
    "data/record-settlement-value-milestone-20261004.mjs", "data/prepare-settlement-value-publication-20261004.mjs",
    "data/settlement-value-milestone-20261004.json", "data/settlement-value-snapshot-1791144422978.json",
    "data/settlement-native-preservation-20261004.json", "data/kansas-source-lead-20261004.json",
    "data/kansas-verified-quotes-20261004.json",
];
pub fn run() -> Result<V, String> {
    let els = elements(FILES, crate::secret_scan::scan_b, |f| format!("Secret scan rejected publication: {f}"))?;
    let packed = serde_json::to_string(&els).map_err(|e| e.to_string())?;
    std::fs::write("data/settlement-value-publication-elements.json", format!("{packed}\n")).map_err(|e| e.to_string())?;
    let mut hashes = serde_json::Map::new();
    for e in &els {
        hashes.insert(e["path"].as_str().unwrap().to_string(), json!(sha(e["content"].as_str().unwrap().as_bytes())));
    }
    std::fs::write(
        "data/settlement-value-publication-hashes.json",
        serde_json::to_string(&V::Object(hashes)).map_err(|e| e.to_string())? + "\n",
    ).map_err(|e| e.to_string())?;
    Ok(json!({
        "files": FILES.len(),
        "characters": packed.len(),
        "paths": FILES,
        "credentialsIncluded": false,
        "rawAccountIncluded": false,
    }))
}
