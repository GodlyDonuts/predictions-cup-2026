// live-paper/training-source.mjs — verified immutable in-process journal source.
use baal_service_runtime::journal::journal_lines;
use baal_service_runtime::json_text::digest;
use baal_service_runtime::contract::parse_iso_ms;
use serde_json::Value as V;
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Clone)]
pub struct VerifiedSource {
    pub records: Vec<V>,
    pub prefix_hashes: Vec<String>,
    pub prefix_bytes: Vec<u64>,
    pub format: String,
    pub file_identity: V,
    pub snapshot_bytes: u64,
}
impl VerifiedSource {
    pub fn source_hash(&self) -> &str { self.prefix_hashes.last().map(String::as_str).unwrap_or("") }
    pub fn source_records(&self) -> usize { self.records.len() }
    pub fn source_bytes(&self) -> u64 { *self.prefix_bytes.last().unwrap_or(&0) }
    pub fn final_event_clock(&self) -> V {
        let last = self.records.last().cloned().unwrap_or(V::Null);
        if !last["event"]["clockAt"].is_null() { last["event"]["clockAt"].clone() } else { last["at"].clone() }
    }
}
fn seal(records: Vec<V>, prefix_hashes: Vec<String>, prefix_bytes: Vec<u64>, format: &str, file_identity: V, snapshot_bytes: u64) -> VerifiedSource {
    VerifiedSource { records, prefix_hashes, prefix_bytes, format: format.into(), file_identity, snapshot_bytes }
}
/// readTrainingSource(file, {endByte})
pub fn read_training_source(file: &Path, end_byte: Option<u64>) -> Result<VerifiedSource, String> {
    let initial = std::fs::metadata(file).map_err(|_| "Incomplete capture journal".to_string())?;
    let end = end_byte.unwrap_or(initial.len());
    let mut records = Vec::new();
    let mut prefix_hashes = Vec::new();
    let mut prefix_bytes = Vec::new();
    let mut prefix = Sha256::new();
    let mut previous = String::new();
    let mut clock = f64::NEG_INFINITY;
    let mut bytes = 0u64;
    for line in journal_lines(file, &baal_service_runtime::journal::JournalReadOptions { end_byte: Some(end), ..Default::default() })? {
        let r: V = serde_json::from_str(&line).map_err(|_| "Source integrity failure".to_string())?;
        let record_hash = r["hash"].as_str().unwrap_or("").to_string();
        let mut body = r.clone();
        body.as_object_mut().unwrap().remove("hash");
        if r["sequence"].as_i64() != Some(records.len() as i64 + 1)
            || r["previous"].as_str().unwrap_or("") != previous
            || record_hash != digest(&body) {
            return Err("Source integrity failure".into());
        }
        let next = r["event"]["clockAt"].as_str().or(r["at"].as_str()).and_then(parse_iso_ms)
            .ok_or("Source clock rollback")?;
        if next < clock { return Err("Source clock rollback".into()); }
        clock = next;
        previous = record_hash;
        prefix.update(format!("{}\n", line).as_bytes());
        bytes += line.len() as u64 + 1;
        prefix_hashes.push(format!("{:x}", prefix.clone().finalize()));
        prefix_bytes.push(bytes);
        // replayJournal uses historical skip timestamps when an explicit receipt
        // is absent; preserve that fallback to keep legacy replay exactly equal.
        let mut input = r.clone();
        let skips = input["state"]["skips"].clone();
        input.as_object_mut().unwrap().remove("state");
        if !skips.is_null() { input["state"] = serde_json::json!({"skips": skips}); }
        records.push(input);
    }
    if records.is_empty() || bytes != end { return Err("Incomplete capture journal".into()); }
    let fin = std::fs::metadata(file).map_err(|_| "Training source replaced or truncated".to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if fin.ino() != initial.ino() || fin.len() < end {
            return Err("Training source replaced or truncated".into());
        }
    }
    #[cfg(not(unix))]
    if fin.len() < end { return Err("Training source replaced or truncated".into()); }
    let ino = {
        #[cfg(unix)]
        { use std::os::unix::fs::MetadataExt; serde_json::json!(initial.ino()) }
        #[cfg(not(unix))]
        { serde_json::Value::Null }
    };
    Ok(seal(records, prefix_hashes, prefix_bytes, "verified_original_journal_v1",
        serde_json::json!({"ino": ino}), end))
}
/// trainingSourcePrefix(source, count)
pub fn training_source_prefix(source: &VerifiedSource, count: usize) -> Result<VerifiedSource, String> {
    if count < 1 || count > source.records.len() { return Err("Invalid verified training prefix".into()); }
    Ok(seal(source.records[..count].to_vec(), source.prefix_hashes[..count].to_vec(),
        source.prefix_bytes[..count].to_vec(), &source.format, source.file_identity.clone(),
        source.prefix_bytes[count - 1]))
}
