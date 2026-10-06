// live-paper/native-quote-markouts-fixture.mjs — deterministic 90-observation
// markouts fixture: books/quotes/eligibility journals plus the declaration and
// sources manifest, and the runner that shells out to the native executable.
use serde_json::{json, Map, Value as V};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::sha256 as sha;

use crate::book_publication_v2::BookPublicationWriter;
use crate::native_dip_host_v1::uuid_v4;
use crate::rust_backend::native_executable;

/// base = Date.parse('2026-10-04T11:00:00.000Z')
pub fn markouts_base_ms() -> i64 {
    parse_iso_ms("2026-10-04T11:00:00.000Z").unwrap_or(0.0) as i64
}
fn at(n: i64) -> String { ms_to_iso((markouts_base_ms() + n) as f64) }

/// journal(events) — hash-chained journal text: each row
/// {sequence,previous,at,event,state:null} then hash = sha(JSON.stringify(row)).
pub fn markouts_journal(events: &[(i64, V)]) -> String {
    let mut previous = String::new();
    let mut out = String::new();
    for (i, (at_ms, event)) in events.iter().enumerate() {
        let row = json!({"sequence": i + 1, "previous": previous, "at": at(*at_ms),
            "event": event, "state": V::Null});
        previous = sha(baal_service_runtime::json_text::js_stringify(&row).as_bytes());
        let mut full = row.clone();
        full["hash"] = json!(previous);
        out.push_str(&baal_service_runtime::json_text::js_stringify(&full));
        out.push('\n');
    }
    out
}

pub struct MarkoutsFixture {
    pub dir: PathBuf,
    pub files: Map<String, V>,
    pub rows: Vec<(i64, V)>,
    pub declaration: V,
    pub sources: V,
}
impl Drop for MarkoutsFixture {
    // t.after: assert parent is the OS temp dir before removing recursively.
    fn drop(&mut self) {
        if self.dir.parent().map(|p| p == std::env::temp_dir().as_path()).unwrap_or(false) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

/// fixture() — creates the temp directory and all fixture files.
pub fn markouts_fixture() -> Result<MarkoutsFixture, String> {
    let dir = std::env::temp_dir().join(format!("sig-markouts-{}", uuid_v4()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let universe = json!({"tournamentId": "bda92870-621e-47b0-bc3c-3602c5c26f55",
        "hash": "a".repeat(64),
        "exchanges": [{"marketId": "1", "exchangeId": "101", "option": "YES"}]});
    let mut files = Map::new();
    for k in ["books", "quotes", "eligibility"] {
        files.insert(k.to_string(), json!(dir.join(format!("{}.jsonl", k)).to_string_lossy()));
    }
    let books_file = PathBuf::from(files["books"].as_str().unwrap_or(""));
    let base = markouts_base_ms();
    let writer = BookPublicationWriter::new(&books_file, &universe, "markouts-fixture",
        move || base, || true)?;
    drop(writer);
    let mut rows: Vec<(i64, V)> = Vec::new();
    for i in 0..90i64 {
        let bid = if i < 40 { 0.9 } else if i == 40 { 0.8 } else if i == 41 { 0.84 }
            else if i == 42 { 0.835 } else if i == 43 { 0.84 } else if i < 75 { 0.88 } else { 0.8 };
        rows.push((i * 60000, json!({
            "type": "market_price_observation",
            "snapshot": {
                "tournamentId": universe["tournamentId"],
                "universeHash": universe["hash"],
                "requestStartedAt": at(i * 60000 - 100),
                "receivedAt": at(i * 60000),
                "sourceTimestamp": V::Null,
                "depthAvailable": false,
                "executable": false,
                "quotes": [{
                    "exchangeId": "101", "marketId": "1", "option": "YES",
                    "latestPrice": bid, "bestBid": bid, "bestAsk": bid + 0.005,
                    "spread": 0.005, "crossed": false,
                }],
                "missingIds": [],
            },
        })));
    }
    let quotes_text = markouts_journal(&rows);
    std::fs::write(PathBuf::from(files["quotes"].as_str().unwrap_or("")), &quotes_text)
        .map_err(|e| e.to_string())?;
    std::fs::write(PathBuf::from(files["eligibility"].as_str().unwrap_or("")), "")
        .map_err(|e| e.to_string())?;
    let development: Map<String, V> = files.iter().map(|(k, file)| {
        let bytes = if k == "quotes" {
            markouts_journal(&rows[..40]).into_bytes()
        } else {
            std::fs::read(file.as_str().unwrap_or("")).unwrap_or_default()
        };
        (k.clone(), json!({"bytes": bytes.len(), "hash": sha(&bytes)}))
    }).collect();
    let mut config: V = serde_json::from_str(include_str!("../../../live-paper/config.json"))
        .map_err(|e| e.to_string())?;
    for (k, v) in [
        ("feeBps", json!(0)),
        ("initialCash", json!("100.000000")),
        ("cashReserve", json!("50.000000")),
        ("maxExposure", json!("5.000000")),
        ("orderShares", json!(1)),
        ("maxPositionShares", json!(1)),
        ("latencyMs", json!(3000)),
        ("cycleIntervalMs", json!(30000)),
        ("maxDataAgeMs", json!(20000)),
        ("fillClockPolicy", json!("independent_request_latency_v2")),
        ("stopLoss", json!("0.050000")),
    ] { config[k] = v; }
    config["marketIds"] = json!(["1"]);
    let strategies: Vec<V> = (0..2).map(|recovery_ticks| json!({
        "id": format!("r{}", recovery_ticks),
        "parameters": {
            "lookbackMs": 3600000, "minimumMinutes": 30, "minimumSpanMs": 1800000,
            "minimumEdge": "0.020000", "recoveryTicks": recovery_ticks,
            "horizonMs": 1800000, "takeProfit": "0.010000",
        },
    })).collect();
    let declaration = json!({
        "schemaVersion": 1, "mode": "causal_quote_dip_subsequent_v1",
        "cohort": "markouts-fixture", "declaredAt": at(40 * 60000 + 1),
        "config": config, "universe": universe, "development": development,
        "maxManagedMarkets": 1, "strategies": strategies,
        "scenarios": [{"id": "base", "latencyMs": 3000, "slippageTicks": 1,
            "depthFractionBps": 100}],
    });
    let sources: Map<String, V> = files.iter().map(|(k, file)| {
        let bytes = std::fs::read(file.as_str().unwrap_or("")).unwrap_or_default();
        (k.clone(), json!({
            "path": file, "bytes": bytes.len(), "hash": sha(&bytes),
            "checkpoints": [development[k].clone()],
        }))
    }).collect();
    Ok(MarkoutsFixture { dir, files, rows, declaration, sources: V::Object(sources) })
}

static MARKOUTS_SERIAL: AtomicU64 = AtomicU64::new(0);
pub struct MarkoutsRun {
    pub status: i32,
    pub out: PathBuf,
    /// JSON.parse(out file) when status === 0; null otherwise.
    pub result: V,
}
/// run(f, workers=2, command="quote-markouts", extra=[]) — spawnSync the native
/// executable and read the result file on success.
pub fn markouts_run(f: &MarkoutsFixture, workers: i64, command: &str, extra: &[&str])
    -> Result<MarkoutsRun, String> {
    let n = MARKOUTS_SERIAL.fetch_add(1, Ordering::SeqCst) + 1;
    let df = f.dir.join(format!("d{}.json", n));
    let sf = f.dir.join(format!("s{}.json", n));
    let out = f.dir.join(format!("o{}.json", n));
    std::fs::write(&df, baal_service_runtime::json_text::js_stringify(&f.declaration))
        .map_err(|e| e.to_string())?;
    std::fs::write(&sf, baal_service_runtime::json_text::js_stringify(&f.sources))
        .map_err(|e| e.to_string())?;
    let mut args: Vec<String> = vec![command.to_string(), "--declaration".into(),
        df.to_string_lossy().into(), "--sources".into(), sf.to_string_lossy().into(),
        "--workers".into(), workers.to_string(), "--out".into(), out.to_string_lossy().into()];
    args.extend(extra.iter().map(|s| s.to_string()));
    let output = std::process::Command::new(native_executable()).args(&args).output()
        .map_err(|e| e.to_string())?;
    let status = output.status.code().unwrap_or(-1);
    let result = if status != 0 {
        V::Null
    } else {
        let text = std::fs::read_to_string(&out).map_err(|e| e.to_string())?;
        serde_json::from_str(&text).map_err(|e| e.to_string())?
    };
    Ok(MarkoutsRun { status, out, result })
}
