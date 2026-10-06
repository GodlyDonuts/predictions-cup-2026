// live-paper/market-history-watch.mjs — watch loop publishing market-history leads.
use baal_service_runtime::json_text::digest;
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};

fn parse_at(v: &V) -> f64 { baal_service_runtime::contract::parse_iso_ms(v.as_str().unwrap_or("")).unwrap_or(f64::NAN) }
/// marketHistoryCodeIdentity() — sha256 of the four frozen files under `root`.
pub fn market_history_code_identity(root: &Path) -> Result<V, String> {
    let mut map = serde_json::Map::new();
    for p in ["live-paper/market-history.mjs", "live-paper/market-history-audit.mjs",
        "live-paper/market-history-watch.mjs", "live-paper/market-price-publication.mjs"] {
        let bytes = std::fs::read(root.join(p)).map_err(|e| e.to_string())?;
        map.insert(p.to_string(), json!(baal_service_runtime::json_text::sha256(&bytes)));
    }
    Ok(V::Object(map))
}
fn paper_runtime_dir() -> Result<PathBuf, String> {
    Ok(Path::new(&std::env::var("LOCALAPPDATA").map_err(|e| e.to_string())?)
        .join("SigTradingCup/paper-runtime"))
}
/// runMarketHistoryWatch() — synchronous loop. SIGTERM/SIGINT cannot be
/// registered without extra crates; the loop still stops via the stop files.
pub fn run_market_history_watch(root: &Path) -> Result<(), String> {
    let rt = paper_runtime_dir()?;
    let file = rt.join("market-prices.jsonl");
    let _cleanup = crate::paper_runtime::lock(&rt.join("market-history.lock"))?;
    let stopped = || -> bool {
        ["STOP", "COLLECTOR_STOP", "MARKET_HISTORY_STOP"].iter().any(|x| rt.join(x).exists())
            || root.join("STOP").exists()
    };
    let result = (|| -> Result<(), String> {
        if stopped() { return Err("MARKET_HISTORY_STOPPED".into()); }
        let identity = market_history_code_identity(root)?;
        let audit = crate::market_history_audit::audit_market_history(root)?;
        let mut state = audit["state"].clone();
        let report = audit["report"].clone();
        let universe = audit["universe"].clone();
        let source = report["sources"].as_array().cloned().unwrap_or_default()
            .into_iter().find(|x| x["name"].as_str() == Some("market-prices.jsonl"))
            .ok_or("MARKET_HISTORY_NO_SOURCE".to_string())?;
        let mut offset = source["bytes"].as_u64().unwrap_or(0);
        let mut sequence = source["sequence"].as_i64().unwrap_or(0);
        let mut previous = source["hash"].as_str().unwrap_or("").to_string();
        #[cfg(unix)]
        let ino = std::os::unix::fs::MetadataExt::ino(&std::fs::metadata(&file).map_err(|e| e.to_string())?);
        #[cfg(not(unix))]
        let ino = std::fs::metadata(&file).map_err(|e| e.to_string())?.len();
        let publish = |status: &str, state: &V, sequence: i64, previous: &str, offset: u64| -> Result<(), String> {
            let clock_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
            let mut body = json!({"schemaVersion": 1, "ownerId": "Codex", "pid": std::process::id(),
                "status": status, "updatedAt": clock_at, "universeHash": universe["hash"],
                "codeIdentity": identity, "sourceSequence": sequence, "sourceHash": previous,
                "sourceBytes": offset, "bootstrapSources": report["sources"],
                "sourceObservations": state["observations"]});
            let rank = crate::market_history::rank_market_history(state, &universe, &clock_at, &crate::market_history::market_history_policy())?;
            for (k, v) in rank.as_object().unwrap() { body[k] = v.clone(); }
            let mut out = body.clone();
            out["hash"] = json!(digest(&body));
            baal_service_runtime::atomic::atomic_json(&rt.join("market-history-leads.json"), &out)
        };
        loop {
            if stopped() { break; }
            if digest(&identity) != digest(&market_history_code_identity(root)?) {
                return Err("MARKET_HISTORY_CODE_CHANGED".into());
            }
            let stat = std::fs::metadata(&file).map_err(|e| e.to_string())?;
            #[cfg(unix)]
            let stat_ino = std::os::unix::fs::MetadataExt::ino(&stat);
            #[cfg(not(unix))]
            let stat_ino = stat.len();
            if stat_ino != ino || stat.len() < offset {
                return Err("MARKET_HISTORY_SOURCE_CHANGED".into());
            }
            if stat.len() > offset {
                let length = (stat.len() - offset).min(8 * 1024 * 1024) as usize;
                use std::io::{Read, Seek, SeekFrom};
                let mut fd = std::fs::File::open(&file).map_err(|e| e.to_string())?;
                fd.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;
                let mut bytes = vec![0u8; length];
                fd.read_exact(&mut bytes).map_err(|_| "MARKET_HISTORY_SHORT_READ".to_string())?;
                let mut begin = 0usize;
                let mut end;
                while {
                    end = bytes[begin..].iter().position(|b| *b == b'\n').map(|p| p + begin);
                    end.is_some()
                } {
                    let e = end.unwrap();
                    let line = std::str::from_utf8(&bytes[begin..e]).map_err(|_| "MARKET_HISTORY_SOURCE_CHAIN".to_string())?;
                    let row: V = serde_json::from_str(line).map_err(|_| "MARKET_HISTORY_SOURCE_CHAIN".to_string())?;
                    let h = row["hash"].as_str().unwrap_or("").to_string();
                    let mut body = row.clone();
                    body.as_object_mut().and_then(|m| m.shift_remove("hash"));
                    if row["sequence"].as_i64() != Some(sequence + 1)
                        || row["previous"].as_str().unwrap_or("") != previous
                        || digest(&body) != h
                        || row["event"]["type"].as_str() != Some("market_price_observation")
                    {
                        return Err("MARKET_HISTORY_SOURCE_CHAIN".into());
                    }
                    let snapshot = &row["event"]["snapshot"];
                    if parse_at(&snapshot["receivedAt"]) >= parse_at(&state["lastAt"]) {
                        state = crate::market_history::observe_market_history(&state, snapshot, &universe, &crate::market_history::market_history_policy())?;
                    }
                    sequence = row["sequence"].as_i64().unwrap_or(0);
                    previous = h;
                    offset += (e - begin + 1) as u64;
                    begin = e + 1;
                }
                if begin == 0 && length == 8 * 1024 * 1024 {
                    return Err("MARKET_HISTORY_RECORD_TOO_LARGE".into());
                }
            }
            publish("running", &state, sequence, &previous, offset)?;
            std::thread::sleep(std::time::Duration::from_millis(1000));
        }
        publish("stopped", &state, sequence, &previous, offset)
    })();
    drop(_cleanup);
    result
}
