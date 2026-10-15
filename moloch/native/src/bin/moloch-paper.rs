use moloch_replay::engine::Engine;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    env,
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Write},
    time::Instant,
};
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let a = env::args().skip(1).collect::<Vec<_>>();
    if a.len() != 3 {
        return Err("USAGE: moloch-paper EVENTS_JSONL POLICY_JSON NEW_REPORT_JSON".into());
    }
    if std::path::Path::new(&a[2]).exists() {
        return Err("PRESERVE_EXISTING_RUN".into());
    }
    let policy: Value = serde_json::from_reader(File::open(&a[1])?)?;
    let mut engine = Engine::new(&policy)?;
    let mut digest = Sha256::new();
    let start = Instant::now();
    let mut input = BufReader::new(File::open(&a[0])?);
    let mut line = Vec::new();
    while input.read_until(b'\n', &mut line)? > 0 {
        digest.update(&line);
        if !line.iter().all(|c| c.is_ascii_whitespace()) {
            let e: Value = serde_json::from_slice(&line)?;
            engine.advance(&e)?;
        }
        line.clear();
    }
    let report = json!({"report":engine.report()?,"fills":engine.fills(),"sourceSha256":format!("{:x}",digest.finalize()),"wallSeconds":start.elapsed().as_secs_f64(),"networkRequests":0,"contestOrders":0});
    let mut out = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&a[2])?;
    serde_json::to_writer_pretty(&mut out, &report)?;
    out.write_all(b"\n")?;
    out.sync_all()?;
    println!("{}", report["report"]);
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("RUST_PAPER_FAILED: {e}");
        std::process::exit(1)
    }
}
