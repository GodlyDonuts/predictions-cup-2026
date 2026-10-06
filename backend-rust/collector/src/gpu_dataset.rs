// live-paper/gpu-dataset.mjs — exports labels (not journal/account payloads) for
// the GPU MLP baseline. Stays in restricted local runtime.
use serde_json::{json, Map, Value as V};

use baal_service_runtime::contract::parse_iso_ms;
use baal_service_runtime::json_text::js_int;

use crate::history_policy::ISOLATED;
use crate::ml_bank::{capture_rows, training_support};
use crate::model::{digest, validate_config};
use crate::roundtrip_target::{round_trip_label, ROUND_TRIP};
use crate::snapshot_order::LEGACY_SNAPSHOT_ORDER;

fn date_parse(v: &V) -> f64 {
    v.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN)
}

/// gpuDataset(raw, config, {since}).
pub fn gpu_dataset(raw: &str, config: &V, since: Option<&str>) -> Result<V, String> {
    validate_config(config)?;
    let c = config;
    if js_int(&c["feeBps"]) != Some(0) {
        return Err("GPU baseline requires zero additional fees".into());
    }
    let source = capture_rows(Some(raw), None, c, ISOLATED, since, false,
        LEGACY_SNAPSHOT_ORDER, "features_only_v1")?;
    let rows: Vec<V> = source["rows"].as_array().cloned().unwrap_or_default();
    if rows.len() < 80 { return Err("Insufficient accepted snapshots".into()); }
    let split = rows[(rows.len() as f64 * 0.65).floor() as usize]["at"].clone();
    let split_ms = date_parse(&split);
    let market_ids: Vec<String> = c["marketIds"].as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let mut sets: Vec<V> = Vec::new();
    for horizon in [60i64, 180, 300, 600] {
        let mut labels: Vec<V> = Vec::new();
        let mut rejected: Map<String, V> = Map::new();
        for i in 0..rows.len() {
            let out = round_trip_label(&rows, i, c, horizon * 1000, false, None, false)?;
            if !out["label"].is_null() {
                labels.push(out["label"].clone());
            } else {
                let key = out["reason"].as_str().unwrap_or("").to_string();
                let n = rejected.get(&key).and_then(V::as_i64).unwrap_or(0);
                rejected.insert(key, json!(n + 1));
            }
        }
        let train: Vec<V> = labels.iter().filter(|r| date_parse(&r["labelAt"]) < split_ms).cloned().collect();
        let test: Vec<V> = labels.iter().filter(|r| date_parse(&r["at"]) >= split_ms).cloned().collect();
        let support = training_support(train, test, &market_ids, "supported-instruments-v1")?;
        let clean = |r: &V| json!({"x": r["x"], "y": r["y"], "persistence": r["persistence"],
            "at": r["at"], "labelAt": r["labelAt"], "marketId": r["marketId"],
            "instrument": r["instrument"]});
        sets.push(json!({
            "horizon": horizon,
            "coverage": support["coverage"],
            "eligible": support["eligible"],
            "train": support["train"].as_array().map(|a| a.iter().map(clean).collect::<Vec<_>>()),
            "test": support["test"].as_array().map(|a| a.iter().map(clean).collect::<Vec<_>>()),
            "rejected": V::Object(rejected),
            "accepted": labels.len(),
        }));
    }
    let data = json!({
        "schemaVersion": 1, "family": "gpu_mlp", "historyPolicy": ISOLATED,
        "targetPolicy": ROUND_TRIP, "trainingScope": "supported-instruments-v1",
        "requireInstrumentCoverage": true, "breadthContext": false,
        "config": c, "configHash": digest(c),
        "sourceHash": source["sourceHash"], "sourceRecords": source["sourceRecords"],
        "trainingCaptureThrough": source["cutoff"],
        "developmentSplitAt": split, "trainingWindowStart": since,
        "acceptedSnapshots": rows.len(), "sets": sets,
    });
    let mut out = data.clone();
    out["datasetHash"] = json!(digest(&data));
    Ok(out)
}
