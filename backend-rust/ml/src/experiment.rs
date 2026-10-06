// ml/experiment.mjs — experiment plumbing: partition counts/minimums,
// develop (test withheld) and finalEvaluation (reproduced selection, held-out
// test). Byte/semantic port.
use crate::core::{
    build_samples, canonical, hash, js_int, js_str, public_sample, split_samples,
    validate_config, Built, Dataset, Error, Partitions, R, Split, WARNING,
};
use crate::model::{metrics, predict, select_model, uncertainty};
use num_bigint::BigInt;
use serde_json::{json, Value as V};
use std::collections::HashSet;

/// prepare(dataset, config) — validate, build, split and gate once; the result
/// is reused by develop/finalEvaluation so validation cannot differ between them.
#[derive(Debug)]
pub struct Prepared {
    pub dataset: Dataset,
    pub config: V,
    pub built: Built,
    pub split: Split,
    pub counts: V,
    pub failures: Vec<String>,
}
fn field_str(v: &V, key: &str) -> String {
    match v.get(key) {
        Some(x) => js_str(x),
        None => "undefined".to_string(),
    }
}
/// prepare(dataset, config)
pub fn prepare(dataset: Dataset, config: &V) -> R<Prepared> {
    let c = validate_config(config)?;
    let built = build_samples(&dataset, &c)?;
    let split = split_samples(&built.samples, &c)?;
    let mut counts = serde_json::Map::new();
    for p in ["train", "validation", "test"] {
        let rows = split.partitions.named(p);
        let sessions: HashSet<String> = rows.iter().map(|r| field_str(&r.v, "sessionId")).collect();
        let instruments: HashSet<String> = rows
            .iter()
            .map(|r| format!("{}/{}/{}", field_str(&r.v, "tournamentId"), field_str(&r.v, "marketId"), field_str(&r.v, "exchangeId")))
            .collect();
        counts.insert(
            p.to_string(),
            json!({"samples": rows.len(), "sessions": sessions.len(), "instruments": instruments.len()}),
        );
    }
    let mut failures: Vec<String> = Vec::new();
    let real = dataset.header["dataKind"].as_str() == Some("real");
    for p in ["train", "validation", "test"] {
        let min_samples = js_int(&c["minSamples"][p])
            .unwrap_or(i64::MIN)
            .max(if real { if p == "train" { 200 } else { 100 } } else { 2 });
        let min_sessions = js_int(&c["minSessions"][p]).unwrap_or(i64::MIN).max(if real { 5 } else { 1 });
        let n_samples = js_int(&counts[p]["samples"]).unwrap();
        let n_sessions = js_int(&counts[p]["sessions"]).unwrap();
        if n_samples < min_samples || n_sessions < min_sessions {
            failures.push(format!(
                "{p}: need >={min_samples} samples and >={min_sessions} sessions; have {n_samples}/{n_sessions}"
            ));
        }
        let rows = split.partitions.named(p);
        if real
            && !rows.is_empty()
            && rows.last().unwrap().start.clone() - &rows[0].start < BigInt::from(3_600_000_000_000i64)
        {
            failures.push(format!("{p}: real-data window shorter than one hour"));
        }
    }
    Ok(Prepared { dataset, config: c, built, split, counts: V::Object(counts), failures })
}

/// develop(prepared, runtime) result: report + selection + split manifest +
/// fitted model; predictions only from finalEvaluation.
#[derive(Debug)]
pub struct DevResult {
    pub report: V,
    pub selection: V,
    pub split_manifest: V,
    pub model: V,
    /// Present only for finalEvaluation.
    pub predictions: Option<V>,
}
fn samples_json(partitions: &Partitions, p: &str) -> Vec<V> {
    partitions.named(p).iter().map(|s| s.v.clone()).collect()
}
/// develop(prepared, runtime)
pub fn develop(prepared: &Prepared, runtime: &V) -> R<DevResult> {
    let dataset = &prepared.dataset;
    let config = &prepared.config;
    let built = &prepared.built;
    let split = &prepared.split;
    let counts = &prepared.counts;
    if !prepared.failures.is_empty() {
        let summary = json!({
            "status": "rejected-insufficient-data",
            "dataKind": dataset.header["dataKind"].clone(),
            "counts": counts.clone(),
            "coverage": built.coverage.clone(),
            "failures": prepared.failures.clone(),
        });
        return Err(Error::with_summary(format!("INSUFFICIENT_DATA: {}", prepared.failures.join("; ")), summary));
    }
    let train = samples_json(&split.partitions, "train");
    let validation = samples_json(&split.partitions, "validation");
    let selected = select_model(&train, &validation, &config["alphas"])?;
    let mut fingerprint = serde_json::Map::new();
    fingerprint.insert("datasetSha256".to_string(), json!(dataset.source_hash));
    fingerprint.insert("configSha256".to_string(), json!(hash(canonical(config))));
    // {...fingerprint} drops the key entirely when the value is undefined.
    if let Some(v) = runtime.get("implementationSha256") {
        fingerprint.insert("implementationSha256".to_string(), v.clone());
    }
    let fingerprint = V::Object(fingerprint);
    let mut selection_map = serde_json::Map::new();
    selection_map.insert("schemaVersion".to_string(), json!(1));
    selection_map.insert("format".to_string(), json!("sig-ridge-selection"));
    if let V::Object(fp) = &fingerprint {
        for (k, v) in fp {
            selection_map.insert(k.clone(), v.clone());
        }
    }
    selection_map.insert("alpha".to_string(), selected["model"]["alpha"].clone());
    selection_map.insert("modelSha256".to_string(), json!(hash(canonical(&selected["model"]))));
    selection_map.insert("selectionBasis".to_string(), json!("minimum validation MAE; ascending-alpha tie break; test withheld"));
    let selection = V::Object(selection_map);
    let synthetic = dataset.header["dataKind"].as_str() == Some("synthetic");
    let mut source = dataset.header.as_object().cloned().unwrap_or_default();
    source.insert("sha256".to_string(), json!(dataset.source_hash));
    let seed = js_int(&config["seed"]).unwrap_or(0) as u64;
    let replicates = js_int(&config["bootstrapReplicates"]).unwrap_or(0) as u64;
    let mut validation_report = metrics(&validation, &selected["model"])?.as_object().unwrap().clone();
    validation_report.insert("uncertainty".to_string(), uncertainty(&validation, &selected["model"], seed, replicates)?);
    let report = json!({
        "schemaVersion": 1,
        "status": "develop-test-withheld",
        "dataKind": dataset.header["dataKind"].clone(),
        "warning": if synthetic { WARNING } else { "Offline forward-price research only. Passing the data-size gate does not establish representativeness, source truth, profitability or live suitability." },
        "source": V::Object(source),
        "runtime": runtime.clone(),
        "config": config.clone(),
        "fingerprint": fingerprint.clone(),
        "counts": counts.clone(),
        "coverage": built.coverage.clone(),
        "splitExcluded": split.excluded.as_array().map(|a| a.len()).unwrap_or(0),
        "splitRules": "Global availability-time cuts. Symmetric embargo. Actual future label end is strictly before next cut minus embargo. Past feature starts at or after prior cut plus embargo. No random split.",
        "model": selected["model"].clone(),
        "candidates": selected["candidates"].clone(),
        "train": metrics(&train, &selected["model"])?,
        "validation": V::Object(validation_report),
        "test": {"status": "withheld", "samples": counts["test"]["samples"].clone()},
        "limitations": [
            "No capture-to-dataset adapter: normalized quality and source provenance must be audited separately.",
            "Source prices include own displayed orders; no aggressor inference, fills, fees, inventory, execution or P&L model.",
            "Event-sampled rows overweight busy markets; labels overlap within a session.",
            "Only within-instrument past/current book features; no causal or generalization claim.",
            "Regression, not a calibrated probability model; Brier score is inapplicable.",
            "Configuration and model choices must be fixed before final test access. Repeated holdout tuning invalidates evaluation; this local CLI cannot police future user research.",
        ],
    });
    let mut partitions_out = serde_json::Map::new();
    for p in ["train", "validation", "test"] {
        partitions_out.insert(
            p.to_string(),
            V::Array(split.partitions.named(p).iter().map(|s| public_sample(s, false)).collect()),
        );
    }
    let split_manifest = json!({
        "schemaVersion": 1,
        "fingerprint": fingerprint,
        "partitions": V::Object(partitions_out),
        "exclusions": {
            "quality": built.excluded.clone(),
            "missingLabelRowIds": built.missing_labels.clone(),
            "split": split.excluded.clone(),
        },
    });
    Ok(DevResult { report, selection, split_manifest, model: selected["model"].clone(), predictions: None })
}

/// finalEvaluation(prepared, runtime, savedSelection) — re-derives the
/// selection from training/validation only; a forged alpha cannot tune to test.
pub fn final_evaluation(prepared: &Prepared, runtime: &V, saved_selection: &V) -> R<DevResult> {
    let dev = develop(prepared, runtime)?;
    if canonical(saved_selection) != canonical(&dev.selection) { return Err(Error::code("SELECTION_MISMATCH")); }
    let rows = samples_json(&prepared.split.partitions, "test");
    let mut test = metrics(&rows, &dev.model)?.as_object().unwrap().clone();
    let seed = js_int(&prepared.config["seed"]).unwrap_or(0) as u64;
    let replicates = js_int(&prepared.config["bootstrapReplicates"]).unwrap_or(0) as u64;
    test.insert("uncertainty".to_string(), uncertainty(&rows, &dev.model, seed, replicates)?);
    // {...dev.report, status, test, selection} — existing keys keep position.
    let mut report = dev.report.as_object().unwrap().clone();
    report["status"] = json!("final-held-out-evaluation");
    report["test"] = V::Object(test);
    report.insert("selection".to_string(), saved_selection.clone());
    let mut predictions = Vec::with_capacity(prepared.split.partitions.test.len());
    for s in &prepared.split.partitions.test {
        let mut p = public_sample(s, true).as_object().unwrap().clone();
        p.insert("predictionMidChange".to_string(), json!(predict(&dev.model, &s.v["x"])?));
        p.insert("zeroChangePrediction".to_string(), json!(0));
        predictions.push(V::Object(p));
    }
    Ok(DevResult { report: V::Object(report), selection: dev.selection, split_manifest: dev.split_manifest, model: dev.model, predictions: Some(V::Array(predictions)) })
}
