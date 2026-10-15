use serde_json::json;
use std::{env, fs::OpenOptions, io::Write, path::Path};
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let a = env::args().skip(1).collect::<Vec<_>>();
    if a.len() != 2 {
        return Err("USAGE: moloch-seed LEGACY_SNAPSHOT NEW_AUDIT_REPORT".into());
    }
    if Path::new(&a[1]).exists() {
        return Err("PRESERVE_EXISTING_RUN".into());
    }
    let (engine, snapshot, digest) = moloch_replay::legacy::load_snapshot(Path::new(&a[0]))?;
    let report = json!({"mode":"rust_legacy_financial_audit","sourceFileSha256":digest,"sourceSnapshotHash":snapshot["hash"],"priorSequence":snapshot["sequence"],"report":engine.report()?,"networkRequests":0,"contestOrders":0,"activation":false});
    let mut out = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&a[1])?;
    serde_json::to_writer_pretty(&mut out, &report)?;
    out.write_all(b"\n")?;
    out.sync_all()?;
    println!("{}", report);
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("MOLOCH_SEED_FAILED: {e}");
        std::process::exit(1)
    }
}
