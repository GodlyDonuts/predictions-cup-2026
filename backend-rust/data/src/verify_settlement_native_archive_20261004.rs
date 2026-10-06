// data/verify-settlement-native-archive-20261004.mjs — settlement native archive verification.
use crate::archive_checked_native_graph::archive_checked_native_graph;
use baal_collector::book_publication_v2::{apply_book_publication, create_book_publication_state};
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::{js_stringify, sha256};
use serde_json::{json, Value as V};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Instant;

fn rt() -> PathBuf {
    Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default()).join("SigTradingCup").join("paper-runtime")
}
fn read(f: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(f).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn sha(b: &[u8]) -> String { sha256(b) }
fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
}
fn ae(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(code.to_string()) }
}

pub fn run(root: &Path, argv: &[String]) -> Result<V, String> {
    let runtime = rt();
    let stamp = argv.get(2).map(String::as_str).unwrap_or("");
    if !stamp.chars().all(|c| c.is_ascii_digit()) || stamp.is_empty() {
        return Err("AssertionError: stamp".into());
    }
    let dir = runtime.join("research-cohorts").join("settlement-value-diagnostic-20261004").join(stamp);
    let evidence = read(&root.join(format!("data/settlement-value-snapshot-{}.json", stamp)))?;
    let native = read(&dir.join("publication.json"))?;
    let scope = read(&dir.join("scope.json"))?;
    let n = evidence["source"]["publicBookPrefixBytes"].as_u64().unwrap_or(0) as usize;
    let mut bytes = vec![0u8; n];
    {
        let mut fd = std::fs::File::open(runtime.join("public-books.jsonl"))
            .map_err(|e| e.to_string())?;
        ae(fd.read(&mut bytes).map_err(|e| e.to_string())? == n, "readSync")?;
    }
    ae(sha(&bytes) == evidence["source"]["publicBookPrefixHash"].as_str().unwrap_or(""),
        "publicBookPrefixHash")?;
    ae(native["sourceHash"].as_str().unwrap_or("") == sha(&bytes), "sourceHash")?;
    let start = Instant::now();
    let mut state = create_book_publication_state(&scope)?;
    let text = String::from_utf8_lossy(&bytes);
    for line in text.trim_end_matches('\n').split('\n') {
        if line.is_empty() { continue; }
        let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
        state = apply_book_publication(&state, &row)?;
    }
    if native["state"] != state {
        return Err("deepEqual native.state state".into());
    }
    ae(native["engineHash"] == evidence["native"]["engineHash"], "engineHash")?;
    let exe = baal_collector::rust_backend::native_executable();
    ae(sha(&std::fs::read(&exe).map_err(|e| e.to_string())?)
        == evidence["native"]["binaryHash"].as_str().unwrap_or(""), "binaryHash")?;
    let binding = archive_checked_native_graph(&runtime,
        evidence["native"]["binaryHash"].as_str().unwrap_or(""),
        native["engineHash"].as_str().unwrap_or(""), true)?;
    let result = json!({"schemaVersion": 1, "at": ms_to_iso(now_ms()),
        "claimComment": 5983667980i64,
        "status": "full_actual_publication_state_parity_and_native_archive_verified",
        "sourceBytes": bytes.len() as i64, "sourceHash": sha(&bytes),
        "sourceSequence": state["sequence"], "sourceTailHash": state["hash"],
        "fullStateExact": true,
        "independentJsReplayMs": start.elapsed().as_secs_f64() * 1000.0,
        "nativeBackend": binding, "activeBindingChanged": false,
        "networkRequests": 0, "competitionOrders": 0});
    std::fs::OpenOptions::new().write(true).create_new(true)
        .open(root.join("data/settlement-native-preservation-20261004.json"))
        .and_then(|mut f| std::io::Write::write_all(&mut f,
            format!("{}\n", js_stringify(&result)).as_bytes()))
        .map_err(|e| e.to_string())?;
    println!("{}", js_stringify(&result));
    Ok(result)
}
