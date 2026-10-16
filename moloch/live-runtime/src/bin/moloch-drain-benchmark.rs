//! Offline-only single-event-frame drain benchmark; no transport/client is constructed.
//! Usage: moloch-drain-benchmark ABS_FROZEN_BANK ABS_NEXT_64_ROWS_JSON ABS_NEW_OUTDIR
//! The input bank must be a PRIVATE, immutable offline copy, never a production path.
use moloch_replay::{journal::Journal, session::{BatchEntry, Session}};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs::{self, File, OpenOptions}, io::{Read, Write}, path::{Path, PathBuf}, time::Instant};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn digest_file(path: &Path) -> Result<(u64, String)> {
    let mut f = File::open(path)?;
    let mut h = Sha256::new();
    let mut bytes = 0u64;
    let mut buf = [0u8; 65536];
    loop { let n = f.read(&mut buf)?; if n == 0 { break } h.update(&buf[..n]); bytes += n as u64; }
    Ok((bytes, format!("{:x}", h.finalize())))
}
fn absolute(path: &str) -> Result<PathBuf> {
    let p = PathBuf::from(path);
    if !p.is_absolute() || p.components().any(|c| matches!(c, std::path::Component::ParentDir)) { return Err("ABSOLUTE_NONTRAVERSING_PATH_REQUIRED".into()) }
    // Reject symlinks in every existing component, including ancestors.
    let mut prefix = PathBuf::new();
    for c in p.components() { prefix.push(c); if prefix.exists() && fs::symlink_metadata(&prefix)?.file_type().is_symlink() { return Err("SYMLINK_PATH_DENIED".into()) } }
    Ok(p)
}
fn private_dir(path: &Path) -> Result<()> {
    fs::create_dir(path)?;
    #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; fs::set_permissions(path, fs::Permissions::from_mode(0o700))?; }
    Ok(())
}
fn new_file(path: &Path) -> Result<File> {
    let mut o = OpenOptions::new(); o.write(true).create_new(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; o.mode(0o600); }
    Ok(o.open(path)?)
}
fn inventory(root: &Path) -> Result<BTreeMap<PathBuf, (u64, String)>> {
    fn walk(root: &Path, at: &Path, out: &mut BTreeMap<PathBuf, (u64, String)>) -> Result<()> {
        for e in fs::read_dir(at)? {
            let p = e?.path(); let m = fs::symlink_metadata(&p)?;
            if m.file_type().is_symlink() { return Err("SOURCE_SYMLINK_DENIED".into()) }
            if m.is_dir() { walk(root, &p, out)?; }
            else if m.is_file() { out.insert(p.strip_prefix(root)?.to_owned(), digest_file(&p)?); }
            else { return Err("SOURCE_SPECIAL_FILE_DENIED".into()) }
        } Ok(())
    }
    let mut out = BTreeMap::new(); walk(root, root, &mut out)?; Ok(out)
}
fn copy_bank(source: &Path, target: &Path, files: &BTreeMap<PathBuf, (u64, String)>) -> Result<()> {
    private_dir(target)?;
    for (relative, expected) in files {
        let to = target.join(relative);
        if let Some(parent) = to.parent() {
            let rel = parent.strip_prefix(target)?; let mut at = target.to_owned();
            for c in rel.components() { at.push(c); if !at.exists() { private_dir(&at)?; } }
        }
        let mut from = File::open(source.join(relative))?;
        let mut dest = new_file(&to)?; std::io::copy(&mut from, &mut dest)?; dest.sync_all()?;
        if &digest_file(&to)? != expected { return Err("SOURCE_CHANGED_DURING_COPY".into()) }
    } Ok(())
}
struct Input { source: String, sequence: u64, event: Value }
fn open(root: &Path, manifest: &Value) -> Result<(Session, u128)> {
    let start = Instant::now();
    let bank = Session::open_live(root, &manifest["policy"], &manifest["seed"]["evidence"])?;
    Ok((bank, start.elapsed().as_nanos()))
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 4 { return Err("usage: moloch-drain-benchmark ABS_FROZEN_BANK ABS_NEXT_64_ROWS_JSON ABS_NEW_PRIVATE_OUTDIR".into()) }
    let source = absolute(&args[1])?.canonicalize()?;
    let input_path = absolute(&args[2])?.canonicalize()?;
    let output_arg = absolute(&args[3])?;
    if output_arg.exists() { return Err("OUTPUT_MUST_NOT_EXIST".into()) }
    let output_parent = output_arg.parent().ok_or("OUTPUT_PARENT")?.canonicalize()?;
    let output = output_parent.join(output_arg.file_name().ok_or("OUTPUT_NAME")?);
    if output.starts_with(&source) || source.starts_with(&output) { return Err("OUTPUT_SOURCE_OVERLAP".into()) }
    if !source.is_dir() || fs::metadata(&input_path)?.len() > 32 * 1024 * 1024 { return Err("INPUT_KIND_OR_SIZE".into()) }
    let before = inventory(&source)?;
    for required in ["manifest.json", "head.json", "report.json", "events.jsonl"] { if !before.contains_key(Path::new(required)) { return Err("INCOMPLETE_FROZEN_BANK".into()) } }
    let manifest: Value = serde_json::from_slice(&fs::read(source.join("manifest.json"))?)?;
    if manifest["mode"] != "rust_live_session" { return Err("ORIGINAL_LIVE_MANIFEST_REQUIRED".into()) }
    let head: Value = serde_json::from_slice(&fs::read(source.join("head.json"))?)?;
    let input_bytes = fs::read(&input_path)?;
    let rows: Vec<Value> = serde_json::from_slice(&input_bytes)?;
    if rows.len() != 64 { return Err("EXACTLY_64_ORIGINAL_JOURNAL_ROWS_REQUIRED".into()) }
    let mut chain = Journal { sequence: head["sequence"].as_u64().ok_or("HEAD_SEQUENCE")?, previous: head["previous"].as_str().ok_or("HEAD_PREVIOUS")?.into() };
    let mut inputs = Vec::new(); let mut expected_suffix = Vec::new();
    for row in &rows {
        let mut event = chain.accept(row)?;
        let reference = event.as_object_mut().ok_or("EVENT_OBJECT")?.remove("sourceRef").ok_or("ORIGINAL_SOURCE_REF_REQUIRED")?;
        let source_id = reference["id"].as_str().ok_or("SOURCE_ID")?.to_owned();
        let sequence = reference["sequence"].as_u64().ok_or("SOURCE_SEQUENCE")?;
        if reference["hash"] != format!("{:x}", Sha256::digest(serde_json::to_vec(&event)?)) { return Err("ORIGINAL_SOURCE_HASH_MISMATCH".into()) }
        if event.get("receivedAt").is_none() { return Err("ORIGINAL_EVENT_CLOCK_REQUIRED".into()) }
        serde_json::to_writer(&mut expected_suffix, row)?; expected_suffix.push(b'\n');
        inputs.push(Input { source: source_id, sequence, event });
    }
    // No opens of the original Session, no mutation/recovery/bridge generation of the source.
    private_dir(&output)?;
    let serial_root = output.join("serial-bank"); let batch_root = output.join("batch-bank");
    copy_bank(&source, &serial_root, &before)?; copy_bank(&source, &batch_root, &before)?;
    if inventory(&source)? != before { return Err("ORIGINAL_SOURCE_CHANGED".into()) }
    let (mut serial, serial_restore_ns) = open(&serial_root, &manifest)?;
    if serial.durable_head() != head { return Err("FROZEN_PREFIX_AHEAD_OF_HEAD_OR_CHANGED".into()) }
    let start = Instant::now();
    for i in &inputs { if !serial.accept(&i.source, i.sequence, &i.event)? { return Err("SERIAL_UNEXPECTED_DUPLICATE".into()) } }
    let serial_commit_ns = start.elapsed().as_nanos();
    let serial_state = json!({"head":serial.durable_head(),"report":serial.engine().report()?,"orders":serial.engine().order_records(),"fills":serial.engine().fills()});
    let state = json!({"sequence":serial.durable_head()["sequence"],"orders":serial.engine().order_records().len(),"fills":serial.engine().fills().len(),"contestOrders":serial.engine().live_order_bindings().len()});
    drop(serial);
    let (mut batch, batch_restore_ns) = open(&batch_root, &manifest)?;
    if batch.durable_head() != head { return Err("FROZEN_PREFIX_AHEAD_OF_HEAD_OR_CHANGED".into()) }
    let start = Instant::now();
    for chunk in inputs.chunks(32) {
        let entries: Vec<_> = chunk.iter().map(|i| BatchEntry { source_id: &i.source, sequence: i.sequence, event: &i.event }).collect();
        let result = batch.accept_batch(&entries)?;
        if result.appended_rows != chunk.len() || result.accepted.iter().any(|ok| !ok) { return Err("BATCH_UNEXPECTED_DUPLICATE".into()) }
    }
    let batch_commit_ns = start.elapsed().as_nanos();
    let semantic_equal = serial_state == json!({"head":batch.durable_head(),"report":batch.engine().report()?,"orders":batch.engine().order_records(),"fills":batch.engine().fills()});
    drop(batch);
    let mut checks = serde_json::Map::new(); let mut bytes_equal = true;
    for name in ["manifest.json", "head.json", "report.json", "events.jsonl"] {
        let a = digest_file(&serial_root.join(name))?; let b = digest_file(&batch_root.join(name))?;
        let equal = a == b; bytes_equal &= equal;
        checks.insert(name.into(), json!({"serialBytes":a.0,"serialSha256":a.1,"batchBytes":b.0,"batchSha256":b.1,"equal":equal}));
    }
    let mut expected = Sha256::new();
    let mut original = File::open(source.join("events.jsonl"))?; let mut buf = [0u8;65536];
    loop { let n = original.read(&mut buf)?; if n == 0 { break } expected.update(&buf[..n]); }
    expected.update(&expected_suffix);
    let original_rows_equal = digest_file(&serial_root.join("events.jsonl"))?.1 == format!("{:x}", expected.finalize());
    let (serial, serial_restart_ns) = open(&serial_root, &manifest)?;
    let restored_serial = json!({"head":serial.durable_head(),"report":serial.engine().report()?,"orders":serial.engine().order_records(),"fills":serial.engine().fills()});
    drop(serial);
    let (batch, batch_restart_ns) = open(&batch_root, &manifest)?;
    let restart_equal = restored_serial == serial_state && restored_serial == json!({"head":batch.durable_head(),"report":batch.engine().report()?,"orders":batch.engine().order_records(),"fills":batch.engine().fills()}) && batch.durable_head()["sequence"].as_u64() == Some(chain.sequence);
    drop(batch);
    if inventory(&source)? != before || digest_file(&input_path)?.1 != format!("{:x}", Sha256::digest(&input_bytes)) { return Err("ORIGINAL_INPUT_CHANGED".into()) }
    let result = json!({"schemaVersion":1,"fixture":"64_original_consecutive_journal_rows_single_event_frames","events":64,"batchSize":32,"networkRequests":0,"serialRestoreNs":serial_restore_ns,"batchRestoreNs":batch_restore_ns,"serialCommitNs":serial_commit_ns,"batchCommitNs":batch_commit_ns,"serialRestartNs":serial_restart_ns,"batchRestartNs":batch_restart_ns,"semanticEqual":semantic_equal,"bytesEqual":bytes_equal,"originalRowsEqual":original_rows_equal,"restartEqual":restart_equal,"stateCounts":state,"files":checks,"inputSha256":format!("{:x}",Sha256::digest(input_bytes))});
    let mut file = new_file(&output.join("result.json"))?; serde_json::to_writer_pretty(&mut file, &result)?; file.write_all(b"\n")?; file.sync_all()?;
    #[cfg(unix)] File::open(&output)?.sync_all()?;
    println!("{}", result);
    if !semantic_equal || !bytes_equal || !original_rows_equal || !restart_equal { return Err("BENCHMARK_PARITY_FAILED".into()) }
    Ok(())
}
