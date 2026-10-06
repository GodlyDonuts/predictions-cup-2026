// paper/cli.mjs
use crate::engine::replay;
use crate::split::chronological_split;
use baal_service_runtime::json_text::sha256;
use indexmap::IndexMap;
use serde_json::{json, Value as V};

pub fn main(args: &[String]) -> Result<String, String> {
    if args.len() == 1 && args[0] == "--help" {
        return Ok("Offline paper replay: cli --events LOCAL.jsonl --config LOCAL.json\nNo network, account, credential, order, or --live mode is supported.".into());
    }
    let mut options: IndexMap<String, String> = IndexMap::new();
    let mut i = 0;
    while i + 1 <= args.len().saturating_sub(1) {
        let key = &args[i];
        let value = args.get(i + 1).cloned().unwrap_or_default();
        if !["--events", "--config"].contains(&key.as_str())
            || options.contains_key(key)
            || value.is_empty()
            || value.starts_with("--")
            || value
                .split("://")
                .next()
                .map(|scheme| scheme.len() < value.len() && scheme.bytes().enumerate().all(|(j, b)| if j == 0 { b.is_ascii_lowercase() } else { b.is_ascii_lowercase() || b.is_ascii_digit() || b"+.-".contains(&b) }))
                .unwrap_or(false)
        {
            return Err("Only --events LOCAL.jsonl --config LOCAL.json are supported".into());
        }
        options.insert(key.clone(), value);
        i += 2;
    }
    if options.get("--events").is_none() || options.get("--config").is_none() {
        return Err("Both --events and --config are required".into());
    }
    let event_text = std::fs::read_to_string(&options["--events"]).map_err(|e| e.to_string())?;
    let config_text = std::fs::read_to_string(&options["--config"]).map_err(|e| e.to_string())?;
    let config: V = serde_json::from_str(&config_text).map_err(|e| e.to_string())?;
    let events: Vec<V> = event_text
        .split('\n')
        .filter(|l| !l.trim().is_empty())
        .enumerate()
        .map(|(i, l)| serde_json::from_str(l.trim_end_matches('\r')).map_err(|_| format!("Invalid event JSON at nonempty line {}", i + 1)))
        .collect::<Result<_, _>>()?;
    let report = replay(&events, &config)?;
    let mut unique: IndexMap<String, V> = IndexMap::new();
    for e in &events {
        let id = e["eventId"].as_str().unwrap_or("").to_string();
        unique.entry(id).or_insert_with(|| e.clone());
    }
    let split_manifest = chronological_split(&unique.values().cloned().collect::<Vec<_>>(), &config["split"])?;
    let no_trade_events: Vec<V> = events
        .iter()
        .filter(|e| !["submit", "cancel"].contains(&e["type"].as_str().unwrap_or("")))
        .cloned()
        .collect();
    let no_trade = replay(&no_trade_events, &config)?;
    let out = json!({
        "schemaVersion": 1,
        "experiment": "scripted_passive_vs_no_trade",
        "datasetClaim": "Uncalibrated replay; fixture is synthetic, not empirical performance",
        "inputs": {
            "eventFileSha256": sha256(event_text.as_bytes()),
            "configFileSha256": sha256(config_text.as_bytes()),
        },
        "splitManifest": split_manifest,
        "scriptedPassive": report,
        "noTrade": {
            "cash": no_trade["cash"],
            "realizedPnl": no_trade["realizedPnl"],
            "totalFees": no_trade["totalFees"],
            "referenceEquity": no_trade["referenceEquity"],
            "metrics": no_trade["metrics"],
        },
    });
    serde_json::to_string_pretty(&out).map_err(|e| e.to_string())
}
