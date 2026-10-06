// data/replay-baal-liquidity-sweep-20261006.mjs — sealed-prefix replay of the
// paired liquidity sweep planner over the controller's actual prepared packets.
use crate::baal_paired_liquidity_sweep_v1::plan_baal_paired_liquidity_sweep;
use crate::publication_pack::{now_ms, runtime_dir};
use baal_execution::core::payload_hash;
use baal_paper::fixed::{big_amount, money};
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::{js_int, js_stringify, sha256};
use serde_json::{json, Value as V};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

fn get<'a>(v: &'a V, k: &str) -> &'a V { v.get(k).unwrap_or(&V::Null) }
// Number(v): NaN for missing/non-numeric like JS; strings parse; null -> 0.
fn js_number(v: Option<&V>) -> f64 {
    match v {
        None => f64::NAN,
        Some(V::Null) => 0.0,
        Some(V::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(V::Bool(b)) => {
            if *b { 1.0 } else { 0.0 }
        }
        Some(V::String(s)) => s.trim().parse().unwrap_or(f64::NAN),
        _ => f64::NAN,
    }
}
fn read(p: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

pub fn run() -> Result<V, String> {
    let runtime = PathBuf::from(runtime_dir());
    let metadata = read(&runtime.join("baal-scaled-host-upgrade-v3-prefixes.json"))?;
    let anchor = get(&metadata, "anchors").get("controller").cloned().unwrap_or(V::Null);
    let all = std::fs::read(
        runtime
            .join("baal-scaled-live-runs")
            .join(get(&metadata, "runId").as_str().unwrap_or(""))
            .join("controller")
            .join(get(&anchor, "name").as_str().unwrap_or("")),
    )
    .map_err(|e| e.to_string())?;
    // Buffer.subarray(0, anchor.bytes): NaN -> whole buffer, else trunc/clamp.
    let n = js_number(anchor.get("bytes"));
    let take = if n.is_nan() { all.len() } else { (n.trunc().max(0.0) as usize).min(all.len()) };
    let bytes = &all[..take];
    if sha256(bytes) != get(&anchor, "sha256").as_str().unwrap_or("") || bytes.last() != Some(&10u8) {
        return Err("SEALED_PREFIX_MISMATCH".into());
    }
    let text = String::from_utf8_lossy(bytes);
    let mut prior = json!("");
    let mut samples: Vec<V> = Vec::new();
    for (row_index, line) in text.trim_end().split('\n').enumerate() {
        let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
        let hash_v = row.get("hash").cloned();
        let mut body = row.clone();
        body.as_object_mut().map(|o| o.shift_remove("hash"));
        let chain_ok = js_int(get(&row, "sequence")) == Some(row_index as i64 + 1)
            && get(&row, "previous") == &prior
            && hash_v.as_ref().and_then(V::as_str)
                == Some(sha256(js_stringify(&body).as_bytes()).as_str());
        if !chain_ok {
            return Err("CHAIN".into());
        }
        prior = hash_v.unwrap_or(V::Null);
        // row.event.input on a missing member throws (TypeError) in JS.
        let e = row
            .get("event")
            .and_then(|x| x.get("input"))
            .ok_or_else(|| "TypeError: row.event.input".to_string())?;
        if get(e, "kind") != &json!("prepared") || get(e, "action") != &json!("buy") {
            continue;
        }
        let o = get(&row, "state")
            .get("orders")
            .and_then(|a| a.as_array())
            .and_then(|a| a.last())
            .cloned()
            .unwrap_or(V::Null);
        let g = get(e, "globalRisk");
        let available = big_amount(
            get(get(&row, "state"), "allocation")
                .get("capital")
                .and_then(V::as_str)
                .unwrap_or(""),
        )? - big_amount(get(g, "nominalExposure").as_str().unwrap_or(""))?;
        let before = Instant::now();
        let plan = plan_baal_paired_liquidity_sweep(&json!({
            "binding": {
                "ownerId": "Codex",
                "runId": get(&metadata, "runId").clone(),
                "tournamentId": get(get(&row, "state"), "scope").get("tournamentId").cloned().unwrap_or(V::Null),
                "eventId": get(get(&o, "package"), "eventId").clone(),
                "reviewHash": payload_hash(get(e, "evidence").get("rootReceipts").ok_or("INVALID_JSON_VALUE")?)?,
            },
            "legs": get(get(e, "evidence"), "bookReceipts").clone(),
            "availableNominalCapital": money(&available),
            "now": get(e, "at").clone(),
        }))?;
        let compute_ms = before.elapsed().as_secs_f64() * 1000.0;
        samples.push(json!({
            "actorSequence": get(&row, "sequence").clone(),
            "observedAt": get(e, "at").clone(),
            "marketIds": plan["marketIds"].clone(),
            "exchangeIds": plan["exchangeIds"].clone(),
            "originalPreparedQuantity": get(get(&o, "body"), "quantity").clone(),
            "displayedBookSweep": plan["selected"].clone(),
            "onePercentBestRungControl": plan["onePercentBestRungControl"].clone(),
            "sourceHashes": plan["sourceHashes"].clone(),
            "computeMs": compute_ms,
            "actualAcceptanceOrFillReplayed": false,
            "conditionalMarginNotProfit": true,
        }));
    }
    let mut additional = 0i64;
    let mut distinct: HashSet<String> = HashSet::new();
    for s in &samples {
        let sweep_q = js_int(get(get(s, "displayedBookSweep"), "quantity")).unwrap_or(0);
        let orig_q = js_int(get(s, "originalPreparedQuantity")).unwrap_or(0);
        additional += (sweep_q - orig_q).max(0);
        distinct.insert(
            get(s, "marketIds")
                .as_array()
                .map(|a| a.iter().map(|m| m.as_str().unwrap_or("")).collect::<Vec<_>>().join(":"))
                .unwrap_or_default(),
        );
    }
    let output = json!({
        "schemaVersion": 1,
        "evaluatedAt": ms_to_iso(now_ms() as f64),
        "mode": "sealed_actual_prepared_book_price_depth_comparison",
        "runId": get(&metadata, "runId").clone(),
        "actorPrefix": {
            "bytes": get(&anchor, "bytes").clone(),
            "sha256": get(&anchor, "sha256").clone(),
            "sequence": get(&anchor, "sequence").clone(),
            "hash": get(&anchor, "hash").clone(),
        },
        "samples": samples.clone(),
        "additionalDisplayedQuantityVersusPrepared": additional,
        "samplePackets": samples.len() as i64,
        "distinctEvents": distinct.len() as i64,
        "noNewDataOrApiTraffic": true,
        "networkRequests": 0,
        "competitionOrders": 0,
        "simulatedFills": 0,
        "profitabilityEstablished": false,
        "limitations": [
            "Prepared packets include repeated abandoned intents; quantities and margins must not be summed as independent trades.",
            "Only displayed depth is measured; latency, partial fills, queue priority and collateral effects are not replayed.",
            "Removing the extra buy-price cushion is a separate execution-policy change from depth participation.",
            "Ordinary settlement floors depend on the original mutually exclusive party-winner predicates; administrative outcomes are not guaranteed.",
        ],
    });
    std::fs::write(
        "data/baal-liquidity-sweep-capture-comparison-20261006.json",
        format!("{}\n", serde_json::to_string_pretty(&output).map_err(|e| e.to_string())?),
    )
    .map_err(|e| e.to_string())?;
    Ok(json!({
        "samplePackets": output["samplePackets"].clone(),
        "distinctEvents": output["distinctEvents"].clone(),
        "additionalDisplayedQuantityVersusPrepared": output["additionalDisplayedQuantityVersusPrepared"].clone(),
        "samples": samples.iter().map(|s| {
            let sweep = get(s, "displayedBookSweep").get("quantity").cloned().unwrap_or(V::Null);
            let control = get(s, "onePercentBestRungControl").get("quantity").cloned().unwrap_or(V::Null);
            json!({
                "markets": get(s, "marketIds").clone(),
                "originalQuantity": get(s, "originalPreparedQuantity").clone(),
                "sweepQuantity": if sweep.is_null() { json!(0) } else { sweep },
                "controlQuantity": if control.is_null() { json!(0) } else { control },
                "conditionalMargin": get(s, "displayedBookSweep").get("conditionalOrdinaryMarginLowerBound").cloned().unwrap_or(V::Null),
                "computeMs": get(s, "computeMs").clone(),
            })
        }).collect::<Vec<V>>(),
        "actualFills": 0,
    }))
}
