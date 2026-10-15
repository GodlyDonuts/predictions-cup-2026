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
    let mut a = env::args().skip(1).collect::<Vec<_>>();
    let journal_input = a.first().is_some_and(|s| s == "--journal");
    if journal_input {
        a.remove(0);
    }
    if a.len() != 3 {
        return Err("USAGE: moloch-paper EVENTS_JSONL POLICY_JSON NEW_REPORT_JSON".into());
    }
    if std::path::Path::new(&a[2]).exists() {
        return Err("PRESERVE_EXISTING_RUN".into());
    }
    let policy: Value = serde_json::from_reader(File::open(&a[1])?)?;
    let mut engine = Engine::new(&policy)?;
    let mut digest = Sha256::new();
    let mut journal = moloch_replay::journal::Journal::default();
    let start = Instant::now();
    let mut input = BufReader::new(File::open(&a[0])?);
    let mut line = Vec::new();
    while input.read_until(b'\n', &mut line)? > 0 {
        digest.update(&line);
        if !line.iter().all(|c| c.is_ascii_whitespace()) {
            let e: Value = serde_json::from_slice(&line)?;
            let e = if journal_input {
                journal.accept(&e)?
            } else {
                e
            };
            engine.advance(&e)?;
        }
        line.clear();
    }
    let report = json!({"report":engine.report()?,"fills":engine.fills(),"sourceSha256":format!("{:x}",digest.finalize()),"sourceJournal":if journal_input{json!({"sequence":journal.sequence,"previous":journal.previous})}else{Value::Null},"wallSeconds":start.elapsed().as_secs_f64(),"networkRequests":0,"contestOrders":0});
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
