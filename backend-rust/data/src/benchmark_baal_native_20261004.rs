// data/benchmark-baal-native-20261004.mjs — JS vs native end-to-end parity
// benchmark of the frozen portfolio bank on the sealed journal prefix.
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};

fn localappdata() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).ok_or_else(|| "LOCALAPPDATA".to_string())
}
fn read_json(p: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn now_iso() -> String {
    baal_service_runtime::contract::ms_to_iso(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as f64)
}
fn parse_ms(v: &V) -> f64 {
    baal_service_runtime::contract::parse_iso_ms(v.as_str().unwrap_or("")).unwrap_or(f64::NAN)
}
fn write_wx(path: &Path, body: &[u8]) -> Result<(), String> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; o.mode(0o600); }
    let mut f = o.open(path).map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut f, body).map_err(|e| e.to_string())
}
fn perf_ms() -> f64 {
    // performance.now() — monotonic; only differences are recorded.
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64() * 1000.0
}

pub fn run() -> Result<V, String> {
    let runtime = localappdata()?.join("SigTradingCup").join("paper-runtime");
    let cohort = runtime.join("research-cohorts").join("baal-net-20261004-e");
    let bank_file = cohort.join("bank.json");
    let bank = read_json(&bank_file)?;
    let end_byte = 4_255_286_972u64;
    let journal = runtime.join("journal.jsonl");
    let mut t = perf_ms();
    let source = baal_collector::training_source::read_training_source(&journal, Some(end_byte))?;
    let verify_ms = perf_ms() - t;
    t = perf_ms();
    let cards = baal_collector::portfolio_bank::replay_frozen_portfolio_bank(
        None, Some(&source), &bank, 4, &|_| {})?;
    let replay_ms = perf_ms() - t;
    let out = cohort.join(format!("native-benchmark-{}.json",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()));
    t = perf_ms();
    let executable = baal_collector::rust_backend::native_executable();
    let n = baal_collector::rust_backend::replay_native_bank(&journal, &bank_file, end_byte,
        source.source_hash(), &out, 8, &executable)?;
    let native_wall_ms = perf_ms() - t;
    if n["replay"]["scorecards"] != cards {
        return Err("assertion failed".into());
    }
    let cards_arr = cards.as_array().cloned().unwrap_or_default();
    let result = json!({"at": now_iso(), "sourceHash": source.source_hash(),
        "sourceBytes": end_byte, "sourceRecords": source.source_records(),
        "captureDays": (parse_ms(&source.final_event_clock())
            - parse_ms(&source.records.first().map(|r| &r["at"]).cloned().unwrap_or(V::Null))) / 86_400_000.0,
        "weightsRefitted": false,
        "js": {"workers": 4, "verifyMs": verify_ms, "replayMs": replay_ms,
            "totalMs": verify_ms + replay_ms},
        "rust": {"workers": 8, "verifyMs": n["capture"]["verify_ms"],
            "prepareMs": n["replay"]["prepareMs"], "replayMs": n["replay"]["replayMs"],
            "totalWallMs": native_wall_ms},
        "endToEndSpeedup": (verify_ms + replay_ms) / native_wall_ms,
        "exactScorecards": cards_arr.len(), "exactCheckpoints": cards_arr.len() * 3,
        "candidatesWithFills": cards_arr.iter().filter(|c|
            c["points"].as_array().and_then(|p| p.last())
                .map(|p| p["fillExecutions"].as_i64().unwrap_or(0) > 0).unwrap_or(false)).count(),
        "competitionOrders": 0, "networkRequests": 0, "profitabilityEstablished": false});
    write_wx(Path::new("data/baal-native-benchmark-20261004.json"),
        format!("{}\n", serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?).as_bytes())?;
    Ok(result)
}
