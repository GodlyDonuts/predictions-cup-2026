//! Offline receipt-to-event conversion using the same source adapter as capture.
use moloch_replay::source::RestSource;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    env,
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Write},
};
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let a = env::args().skip(1).collect::<Vec<_>>();
    if a.len() != 4 {
        return Err(
            "USAGE: moloch-source RECEIPTS_JSONL CONFIG_JSON NEW_EVENTS_JSONL NEW_STATS_JSON"
                .into(),
        );
    }
    if std::path::Path::new(&a[2]).exists() || std::path::Path::new(&a[3]).exists() {
        return Err("PRESERVE_EXISTING_RUN".into());
    }
    let config: Value = serde_json::from_reader(File::open(&a[1])?)?;
    let mut source = RestSource::new(
        &config["pairs"],
        config["tournamentId"].as_str().ok_or("TOURNAMENT")?,
        config["startedAt"].as_str().ok_or("STARTED_AT")?,
    )?;
    let mut input = BufReader::new(File::open(&a[0])?);
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&a[2])?;
    let mut line = Vec::new();
    let mut digest = Sha256::new();
    let mut receipts = 0;
    let mut events = 0;
    while input.read_until(b'\n', &mut line)? > 0 {
        digest.update(&line);
        if !line.iter().all(|b| b.is_ascii_whitespace()) {
            let r: Value = serde_json::from_slice(&line)?;
            for event in source.accept(&r)? {
                writeln!(output, "{}", event)?;
                events += 1;
            }
            receipts += 1;
        }
        line.clear();
    }
    output.sync_all()?;
    let report = json!({"runtime":"rust","receipts":receipts,"events":events,"sourceSha256":format!("{:x}",digest.finalize()),"scopes":source.scopes(),"networkRequests":0,"contestOrders":0});
    let mut stats = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&a[3])?;
    serde_json::to_writer_pretty(&mut stats, &report)?;
    stats.write_all(b"\n")?;
    stats.sync_all()?;
    println!("{}", report);
    Ok(())
}
fn main() {
    if run().is_err() {
        eprintln!("MOLOCH_SOURCE_FAILED (source contents not logged)");
        std::process::exit(1)
    }
}
