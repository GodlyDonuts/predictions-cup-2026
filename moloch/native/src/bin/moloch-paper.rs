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
    let mut journal_input = false;
    let mut seed_path = None;
    while a.first().is_some_and(|s| s.starts_with("--")) {
        match a[0].as_str() {
            "--journal" => {
                if journal_input {
                    return Err("DUPLICATE_OPTION".into());
                }
                journal_input = true;
                a.remove(0);
            }
            "--seed" => {
                if seed_path.is_some() || a.len() < 2 {
                    return Err("SEED_OPTION".into());
                }
                seed_path = Some(a.remove(1));
                a.remove(0);
            }
            _ => return Err("UNKNOWN_OPTION".into()),
        }
    }
    if a.len() != 3 {
        return Err("USAGE: moloch-paper EVENTS_JSONL POLICY_JSON NEW_REPORT_JSON".into());
    }
    if std::path::Path::new(&a[2]).exists() {
        return Err("PRESERVE_EXISTING_RUN".into());
    }
    let policy: Value = serde_json::from_reader(File::open(&a[1])?)?;
    let mut seed_source = Value::Null;
    let mut engine = if let Some(path) = seed_path {
        let (mut engine, snapshot, digest) =
            moloch_replay::legacy::load_snapshot(std::path::Path::new(&path))?;
        engine.update_policy(&policy)?;
        seed_source = json!({"sourceFileSha256":digest,"sourceSnapshotHash":snapshot["hash"]});
        engine
    } else {
        Engine::new(&policy)?
    };
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
    let report = json!({"report":engine.report()?,"fills":engine.fills(),"seed":seed_source,"sourceSha256":format!("{:x}",digest.finalize()),"sourceJournal":if journal_input{json!({"sequence":journal.sequence,"previous":journal.previous})}else{Value::Null},"wallSeconds":start.elapsed().as_secs_f64(),"networkRequests":0,"contestOrders":0});
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
