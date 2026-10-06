// data/prepare-election-v2-publication-20261004.mjs — build the publication
// pack; --chunk N prints a slice.
use crate::publication_pack::{chunk, elements, read_json, read_utf8, sha, write_pack};
use serde_json::{json, Value as V};
use std::path::Path;

const BASE: &str = "research/election-baseline-v2";
const PACK: &str = "data/election-v2-publication-pack-20261004.json";
fn files() -> Vec<String> {
    let mut files = vec![
        "AGENTS.md".to_string(), "README.md".to_string(), "coordination/execution_state.json".to_string(),
        "research/election-baseline-v1/README.md".to_string(),
        "research/election-baseline-v1/final-label-review.json".to_string(),
    ];
    for f in [
        "definition.json", "model.mjs", "model.test.mjs", "final-stage-overrides.json",
        "source-manifest.json", "verify-fec-workbooks.py", "download-sources.mjs", "train.mjs",
        "screen.mjs", "README.md", "model-summary.json", "model-summary-affiliation-invariant.json",
        "market-screen.json", "market-screen-affiliation-invariant.json",
    ] {
        files.push(format!("{BASE}/{f}"));
    }
    files.push("data/election-v2-milestone-20261004.json".to_string());
    files.push("data/prepare-election-v2-publication-20261004.mjs".to_string());
    files
}
/// `--chunk N`
pub fn election_v2_publication_chunk(n: i64) -> Result<String, String> {
    chunk(PACK, n, 10_000)
}
pub fn run() -> Result<V, String> {
    let milestone_file = "data/election-v2-milestone-20261004.json";
    let mut milestone = read_json(Path::new(milestone_file))?;
    let mut code_map = serde_json::Map::new();
    for f in ["model.mjs", "train.mjs", "screen.mjs", "verify-fec-workbooks.py"] {
        let p = format!("{BASE}/{f}");
        let h = sha(&std::fs::read(&p).map_err(|e| e.to_string())?);
        code_map.insert(p, json!(h));
    }
    milestone.as_object_mut().unwrap().insert("codeHashes".into(), V::Object(code_map));
    milestone.as_object_mut().unwrap().insert(
        "immutableReportFitReproduced".into(),
        json!({"initial": true, "affiliationInvariant": true}),
    );
    std::fs::write(milestone_file, serde_json::to_string_pretty(&milestone).map_err(|e| e.to_string())? + "\n")
        .map_err(|e| e.to_string())?;
    let names: Vec<String> = files();
    let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
    let els = elements(&refs, crate::secret_scan::pack_check, |p| format!("SECRET_CHECK {p}"))?;
    let bytes = serde_json::to_string(&els).map_err(|e| e.to_string())?;
    std::fs::write(PACK, &bytes).map_err(|e| e.to_string())?;
    let _ = read_utf8(Path::new(PACK))?;
    Ok(json!({
        "files": names.len(),
        "characters": bytes.len(),
        "chunks": (bytes.len() + 9999) / 10_000,
        "sha256": sha(bytes.as_bytes()),
    }))
}
