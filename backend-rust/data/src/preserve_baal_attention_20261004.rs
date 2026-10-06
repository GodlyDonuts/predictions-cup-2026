// data/preserve-baal-attention-20261004.mjs — preservation receipt of the
// v3 maker cohort and sealed source prefixes with tape cadence stats.
use crate::publication_pack::{now_ms, read_json, runtime_dir, sha};
use baal_collector::no_maker_host_v3::no_maker_code_identity_v3;
use baal_execution::core::payload_hash;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use serde_json::{json, Value as V};
use std::io::Write;
use std::path::Path;

pub fn run() -> Result<V, String> {
    let r = runtime_dir();
    let read = |f: &str| read_json(Path::new(&format!("{r}/{f}")));
    if read("COLLECTOR_STOP")?["source"] != json!("baal-attention-deployment-20261004")
        || Path::new(&format!("{r}/../bot.lock")).exists()
    { return Err("STOP or lease provenance".into()); }
    let active = read("no-maker-active.json")?;
    let m = read(&format!("no-maker-cohorts/{}/stats.json", active["cohort"].as_str().unwrap_or("")))?;
    let bank = read(&format!("no-maker-cohorts/{}/bank.json", active["cohort"].as_str().unwrap_or("")))?;
    if active["hostRevision"] != json!(3) || m["failed"] == json!(true)
        || payload_hash(&bank)? != active["bankHash"].as_str().unwrap_or("")
        || payload_hash(&bank["codeIdentity"])? != payload_hash(&no_maker_code_identity_v3(Path::new("live-paper"))?)?
    { return Err("Frozen maker identity".into()); }
    let files = [
        "anonymous-trades.jsonl".to_string(), "public-books.jsonl".to_string(),
        format!("no-maker-cohorts/{}/journal.jsonl", active["cohort"].as_str().unwrap_or("")),
    ];
    let prefixes: Vec<V> = files.iter().map(|file| -> Result<V, String> {
        let bytes = std::fs::read(format!("{r}/{file}")).map_err(|e| e.to_string())?;
        if bytes.is_empty() || *bytes.last().unwrap() != b'\n' {
            return Err("Partial source prefix".into());
        }
        Ok(json!({"file": file, "bytes": bytes.len(), "sha256": sha(&bytes)}))
    }).collect::<Result<_, _>>()?;
    let tape_text = std::fs::read_to_string(format!("{r}/anonymous-trades.jsonl")).map_err(|e| e.to_string())?;
    let tape: Vec<V> = tape_text.trim_end().split('\n')
        .map(|l| serde_json::from_str(l).map_err(|e| e.to_string())).collect::<Result<_, _>>()?;
    let mut gaps = serde_json::Map::<String, V>::new();
    for id in ["986", "1052", "1070", "1071"] {
        let rows: Vec<&V> = tape.iter().filter(|x| x["event"]["exchangeId"] == json!(id)).collect();
        let mut values: Vec<f64> = (1..rows.len()).map(|i|
            parse_iso_ms(rows[i]["event"]["receivedAt"].as_str().unwrap_or("")).unwrap_or(0.0)
                - parse_iso_ms(rows[i - 1]["event"]["receivedAt"].as_str().unwrap_or("")).unwrap_or(0.0)
        ).collect();
        values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        gaps.insert(id.to_string(), json!({
            "pages": rows.len(),
            "lastReceiptAt": rows.last().map(|r| r["event"]["receivedAt"].clone()).unwrap_or(V::Null),
            "medianIntervalMs": values.get((values.len() as f64 * 0.5) as usize).map(|x| json!(x)).unwrap_or(V::Null),
            "p95IntervalMs": values.get((values.len() as f64 * 0.95) as usize).map(|x| json!(x)).unwrap_or(V::Null),
        }));
    }
    let out = json!({
        "at": ms_to_iso(now_ms() as f64), "cohort": active["cohort"],
        "bankHash": active["bankHash"], "codeIdentity": bank["codeIdentity"],
        "prefixes": prefixes, "beforeTape": V::Object(gaps),
        "wallets": m["wallets"].as_object().map(|o| o.iter().map(|(k, w)| (
            k.clone(), json!({
                "cash": w["cash"], "fills": w["fills"], "closedTrades": w["closedTrades"],
                "realizedPnl": w["realizedPnl"], "positions": w["positions"],
                "unresolvedQuotes": w["unresolvedQuotes"].as_array().map(|a| a.len()).unwrap_or(0),
            })
        )).collect::<serde_json::Map<String, V>>()).unwrap_or_default(),
        "competitionOrders": 0,
    });
    std::fs::OpenOptions::new().write(true).create(true).open("data/baal-attention-preservation-20261004.json")
        .and_then(|mut f| f.write_all(serde_json::to_string_pretty(&out).unwrap().as_bytes()))
        .map_err(|e| e.to_string())?;
    Ok(json!({
        "cohort": out["cohort"], "prefixes": out["prefixes"].as_array().map(|a| a.len()).unwrap_or(0),
        "bankUnchanged": true, "modelAccountingPreserved": true,
    }))
}
