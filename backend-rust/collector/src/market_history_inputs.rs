// live-paper/market-history-inputs.mjs — validated read of market-history leads.
use baal_service_runtime::json_text::{digest, js_int};
use serde_json::{json, Value as V};
use std::path::Path;

fn parse_at(v: &V) -> f64 { baal_service_runtime::contract::parse_iso_ms(v.as_str().unwrap_or("")).unwrap_or(f64::NAN) }
/// validateMarketHistoryLeads(envelope, universe, now, identity)
pub fn validate_market_history_leads(envelope: &V, universe: &V, now: f64, identity: &V) -> Result<V, String> {
    let h = envelope["hash"].clone();
    let mut body = envelope.clone();
    body.as_object_mut().and_then(|m| m.shift_remove("hash"));
    if h.as_str() != Some(digest(&body).as_str())
        || body["schemaVersion"].as_i64() != Some(1)
        || body["ownerId"].as_str() != Some("Codex")
        || body["status"].as_str() != Some("running")
        || body["executable"] != V::Bool(false)
        || body["networkRequests"].as_i64() != Some(0)
        || body["universeHash"] != universe["hash"]
        || digest(&body["codeIdentity"]) != digest(identity)
        || digest(&body["policy"]) != digest(&crate::market_history::market_history_policy())
        || js_int(&body["sourceSequence"]).map(|x| x < 1).unwrap_or(true)
        || js_int(&body["sourceBytes"]).map(|x| x < 1).unwrap_or(true)
        || !body["sourceHash"].as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
    {
        return Err("MARKET_HISTORY_PROOF".into());
    }
    let at = parse_at(&body["updatedAt"]);
    if !now.is_finite() || !at.is_finite() || at > now || now - at > 45000.0
        || body["clockAt"] != body["updatedAt"]
    {
        return Err("MARKET_HISTORY_STALE".into());
    }
    let members: std::collections::BTreeMap<String, V> = universe["exchanges"].as_array().cloned().unwrap_or_default()
        .iter().map(|x| (x["marketId"].as_str().unwrap_or("").to_string(), x["exchangeId"].clone())).collect();
    let mut seen = std::collections::BTreeSet::new();
    if !body["leads"].is_array()
        || body["leads"].as_array().unwrap().len() > universe["exchanges"].as_array().map(|x| x.len()).unwrap_or(0)
    {
        return Err("MARKET_HISTORY_LEADS".into());
    }
    let policy = crate::market_history::market_history_policy();
    for x in body["leads"].as_array().unwrap() {
        let received = parse_at(&x["receivedAt"]);
        let through = parse_at(&x["historyThrough"]);
        let bad = members.get(x["marketId"].as_str().unwrap_or("")) != Some(&x["exchangeId"])
            || seen.contains(x["marketId"].as_str().unwrap_or(""))
            || x["executable"] != V::Bool(false)
            || x["reason"].as_str() != Some("prior_bid_discount_requires_fresh_depth")
            || js_int(&x["minutes"]).map(|m| m < policy["minMinutes"].as_i64().unwrap_or(0)).unwrap_or(true)
            || !received.is_finite() || received > now || now - received > policy["maxAgeMs"].as_f64().unwrap_or(0.0)
            || !through.is_finite() || through >= (received / 60000.0).floor() * 60000.0 - 60000.0
            || ["referenceBid", "entryAsk", "currentBid", "netRecovery"].iter().any(|k| {
                x[*k].as_f64().map(|v| !v.is_finite() || v < 0.0 || v > 1.0).unwrap_or(true)
            })
            || (x["referenceBid"].as_f64().unwrap_or(0.0) - x["entryAsk"].as_f64().unwrap_or(0.0)
                - x["netRecovery"].as_f64().unwrap_or(0.0)).abs() > 1e-9
            || x["netRecovery"].as_f64().unwrap_or(0.0) < policy["minNetRecovery"].as_f64().unwrap_or(0.0) - 1e-9;
        if bad { return Err("MARKET_HISTORY_LEAD".into()); }
        seen.insert(x["marketId"].as_str().unwrap_or("").to_string());
    }
    Ok(body)
}
/// readMarketHistoryInputs(runtimeDir, universe, now)
/// `root` is the repo root used by marketHistoryCodeIdentity.
pub fn read_market_history_inputs(runtime_dir: &Path, universe: &V, now: f64, root: &Path) -> V {
    let file = runtime_dir.join("market-history-leads.json");
    if !file.exists() {
        return json!({"available": false, "reason": "NOT_CONFIGURED", "candidateMarketIds": []});
    }
    (|| -> Result<V, String> {
        let envelope: V = serde_json::from_slice(&std::fs::read(&file).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let identity = crate::market_history_watch::market_history_code_identity(root)?;
        let body = validate_market_history_leads(&envelope, universe, now, &identity)?;
        let source_file = runtime_dir.join("market-prices.jsonl");
        let size = std::fs::metadata(&source_file).map_err(|e| e.to_string())?.len();
        let source_bytes = body["sourceBytes"].as_u64().unwrap_or(0);
        if size < source_bytes { return Err("MARKET_HISTORY_SOURCE_TRUNCATED".into()); }
        let n = source_bytes.min(65536) as usize;
        use std::io::{Read, Seek, SeekFrom};
        let mut fd = std::fs::File::open(&source_file).map_err(|e| e.to_string())?;
        fd.seek(SeekFrom::Start(source_bytes - n as u64)).map_err(|e| e.to_string())?;
        let mut bytes = vec![0u8; n];
        fd.read_exact(&mut bytes).map_err(|_| "MARKET_HISTORY_SOURCE_READ".to_string())?;
        if *bytes.last().unwrap_or(&0) != b'\n' { return Err("MARKET_HISTORY_SOURCE_BOUNDARY".into()); }
        let start = bytes[..n - 1].iter().rposition(|b| *b == b'\n').map(|p| p + 1).unwrap_or(0);
        let row: V = serde_json::from_slice(&bytes[start..n - 1]).map_err(|e| e.to_string())?;
        let h = row["hash"].clone();
        let mut value = row.clone();
        value.as_object_mut().and_then(|m| m.shift_remove("hash"));
        if row["sequence"].as_i64() != body["sourceSequence"].as_i64()
            || h != body["sourceHash"] || h.as_str() != Some(digest(&value).as_str())
        {
            return Err("MARKET_HISTORY_SOURCE_PROOF".into());
        }
        Ok(json!({"available": true, "reason": "VERIFIED_READ_ONLY_SCREENING",
            "candidateMarketIds": body["leads"].as_array().cloned().unwrap_or_default().iter().map(|x| x["marketId"].clone()).collect::<Vec<_>>(),
            "counts": body["counts"], "sourceSequence": body["sourceSequence"],
            "sourceHash": body["sourceHash"], "networkRequests": 0, "eligibleForExecution": false}))
    })().unwrap_or_else(|_| json!({"available": false, "reason": "UNAVAILABLE_OR_UNVERIFIED", "candidateMarketIds": []}))
}
