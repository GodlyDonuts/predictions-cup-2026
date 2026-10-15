//! Offline session lifecycle; no network client is constructed.
use moloch_replay::session::Session;
use serde_json::{Value, json};
use std::{
    env,
    fs::File,
    io::{BufRead, BufReader, Write},
    path::Path,
};
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let a = env::args().skip(1).collect::<Vec<_>>();
    if !((a.first().is_some_and(|x| x == "ingest") && a.len() == 6)
        || (a.first().is_some_and(|x| x == "hold") && a.len() == 4))
    {
        return Err("USAGE: moloch-session ingest DIR SEED_OR_DASH POLICY SOURCE_ID INPUT_JSONL | hold DIR SEED_OR_DASH POLICY".into());
    }
    let policy: Value = serde_json::from_reader(File::open(&a[3])?)?;
    let seed = if a[2] == "-" {
        None
    } else {
        Some(Path::new(&a[2]))
    };
    let mut session = Session::open(Path::new(&a[1]), seed, &policy)?;
    if a[0] == "hold" {
        println!("{}", json!({"held":true,"pid":std::process::id()}));
        std::io::stdout().flush()?;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    let mut sequence = 0;
    let mut accepted = 0;
    for line in BufReader::new(File::open(&a[5])?).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        sequence += 1;
        let event: Value = serde_json::from_str(&line)?;
        if session.accept(&a[4], sequence, &event)? {
            accepted += 1;
        }
    }
    if sequence < session.last_source_sequence(&a[4]) {
        return Err("PAPER_INPUT_PREFIX_TRUNCATED".into());
    }
    println!(
        "{}",
        json!({"runtime":"rust","mode":"offline_session_ingest","inputEvents":sequence,"newlyCommitted":accepted,"report":session.engine().report()?,"networkRequests":0,"contestOrders":0})
    );
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("MOLOCH_SESSION_FAILED: {e}");
        std::process::exit(1)
    }
}
