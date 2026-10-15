use moloch_replay::session::Session;
use serde_json::json;
use std::{
    fs,
    io::{BufRead, BufReader},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};
#[test]
fn killed_process_releases_lock_without_resetting_wallet() {
    let root = std::env::temp_dir().join(format!(
        "moloch-crash-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    let bank = root.join("bank");
    let policy = root.join("policy.json");
    fs::write(&policy, b"{}").unwrap();
    let mut session = Session::open(&bank, None, &json!({})).unwrap();
    session
        .accept(
            "tick",
            1,
            &json!({"type":"tick","receivedAt":"2026-10-14T00:00:00Z"}),
        )
        .unwrap();
    let expected = session.engine().report().unwrap();
    drop(session);
    let mut child = Command::new(env!("CARGO_BIN_EXE_moloch-session"))
        .arg("hold")
        .arg(&bank)
        .arg("-")
        .arg(&policy)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&line).unwrap()["held"],
        true
    );
    assert!(Session::open(&bank, None, &json!({})).is_err());
    child.kill().unwrap();
    let status = child.wait().unwrap();
    assert!(!status.success());
    let session = Session::open(&bank, None, &json!({})).unwrap();
    assert_eq!(session.engine().report().unwrap(), expected);
    drop(session);
    fs::remove_dir_all(root).unwrap();
}
