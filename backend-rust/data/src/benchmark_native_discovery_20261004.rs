// data/benchmark-native-discovery-20261004.mjs — verified minute market screen:
// replays the baseline price capture through the screen and asserts identical
// source/counts/leads.
use baal_collector::collector_market_screen_v1::CollectorMarketScreen;
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Value as V};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn read_json(p: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn localappdata() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).ok_or_else(|| "LOCALAPPDATA".to_string())
}
fn now_iso() -> String {
    baal_service_runtime::contract::ms_to_iso(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as f64)
}
fn perf_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64() * 1000.0
}
fn uuid_v4() -> String { uuid::Uuid::new_v4().to_string() }

pub fn run() -> Result<V, String> {
    let r = localappdata()?.join("SigTradingCup").join("paper-runtime");
    let dir = r.join("screen-audits").join(uuid_v4());
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = dir.join("market-prices.jsonl");
    let baseline = read_json(Path::new("data/native-discovery-baseline-20261004.json"))?;
    let audit_root = r.join("screen-audits");
    let original = std::fs::read_dir(&audit_root).map_err(|e| e.to_string())?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|x| {
            read_json(&x.join("collector-market-screen-v1.json")).ok()
                .is_some_and(|p| p["source"]["hash"] == baseline["source"]["hash"]
                    && p["source"]["bytes"] == baseline["source"]["bytes"])
        })
        .ok_or_else(|| "Original baseline capture required".to_string())?;
    let expected = read_json(&original.join("collector-market-screen-v1.json"))?;
    std::fs::copy(original.join("market-prices.jsonl"), &file).map_err(|e| e.to_string())?;
    let size = std::fs::metadata(&file).map_err(|e| e.to_string())?.len() as usize;
    let mut tail = vec![0u8; size.min(2 * 1024 * 1024)];
    {
        let mut fd = std::fs::File::open(&file).map_err(|e| e.to_string())?;
        fd.seek(SeekFrom::Start((size - tail.len()) as u64)).map_err(|e| e.to_string())?;
        let n = fd.read(&mut tail).map_err(|e| e.to_string())?;
        if n != tail.len() { return Err("assertion failed".into()); }
    }
    if *tail.last().unwrap_or(&0) != 10 { return Err("assertion failed".into()); }
    // tail.lastIndexOf(10, tail.length-2)+1 .. tail.length-1
    let bound = tail.len().saturating_sub(2);
    let pos = tail[..=bound].iter().rposition(|c| *c == 10)
        .map(|p| p + 1).unwrap_or(0);
    let last: V = serde_json::from_slice(&tail[pos..tail.len() - 1]).map_err(|e| e.to_string())?;
    let universe_raw = read_json(Path::new("live-paper/price-universe.json"))?;
    let u = baal_collector::breadth::validate_universe(&universe_raw,
        last["event"]["snapshot"]["tournamentId"].as_str().unwrap_or(""))?;
    let last_seq = last["sequence"].clone();
    let last_hash = last["hash"].clone();
    let u_hash = u["hash"].clone();
    let publisher: Arc<dyn Fn() -> V + Send + Sync> = Arc::new(move || json!({
        "sequence": last_seq, "hash": last_hash, "universeHash": u_hash}));
    let now_ms = baal_service_runtime::contract::parse_iso_ms(
        expected["updatedAt"].as_str().unwrap_or("")).unwrap_or(0.0) as i64;
    let workspace = std::env::current_dir().map_err(|e| e.to_string())?;
    let mut screen = CollectorMarketScreen::open(&dir, &u, &workspace,
        publisher, None, Arc::new(|| true), Arc::new(move || now_ms))?;
    let started = perf_ms();
    let p = screen.initialize();
    let wall_ms = perf_ms() - started;
    if p["available"] != V::Bool(true) {
        return Err(format!("assertion failed: {}", p["reason"].as_str().unwrap_or("")));
    }
    if p["source"]["bytes"].as_u64() != Some(size as u64)
        || p["source"]["sequence"] != last["sequence"] || p["source"]["hash"] != last["hash"]
        || p["counts"] != expected["counts"] || p["leads"] != expected["leads"]
        || p["source"] != expected["source"] {
        return Err("assertion failed".into());
    }
    screen.close();
    let checks = std::fs::read_to_string("data/native-discovery-all-checks-20261004.tap")
        .map_err(|e| e.to_string())?;
    let ok = regex_find(&checks, "# pass 111") && regex_find(&checks, "# fail 0")
        && regex_find(&checks, "# skipped 0");
    if !ok { return Err("assertion failed".into()); }
    let code_hashes: serde_json::Map<String, V> =
        ["live-paper/collector-market-screen-v1.mjs", "live-paper/native-discovery-plan-v1.mjs",
            "live-paper/runner.mjs"].iter().map(|f| -> Result<(String, V), String> {
            Ok((f.to_string(), json!(sha256(&std::fs::read(f).map_err(|e| e.to_string())?))))
        }).collect::<Result<_, String>>()?;
    let e = json!({"schemaVersion": 1, "at": now_iso(), "claim": 5981528650i64,
        "mode": "collector_verified_minute_screen_v1", "wallMs": wall_ms,
        "baselineWallMs": baseline["wallMs"],
        "speedup": baseline["wallMs"].as_f64().unwrap_or(0.0) / wall_ms,
        "samePrefixCountsAndLeadsExact": true, "bootstrapMs": p["bootstrapMs"],
        "source": p["source"], "sourceObservations": p["sourceObservations"],
        "counts": p["counts"],
        "screenedCandidates": p["candidateMarketIds"].as_array().map(|a| a.len()).unwrap_or(0),
        "universeMarkets": u["exchanges"].as_array().map(|a| a.len()).unwrap_or(0),
        "policy": p["policy"], "networkRequests": 0, "executionAuthority": false,
        "competitionOrders": 0, "checks": {"passed": 111, "failed": 0, "skipped": 0},
        "codeHashes": V::Object(code_hashes)});
    baal_service_runtime::atomic::atomic_json_canonical(
        Path::new("data/native-discovery-benchmark-20261004.json"), &e)?;
    Ok(e)
}
// `pattern` matches JS `/# pass 111\b/` — word-bounded line contains.
fn regex_find(text: &str, needle: &str) -> bool {
    text.split('\n').any(|line| {
        line.find(needle).is_some_and(|i| {
            let end = i + needle.len();
            let next = line.as_bytes().get(end).copied();
            next.map(|b| !b.is_ascii_alphanumeric() && b != b'_').unwrap_or(true)
        })
    })
}
