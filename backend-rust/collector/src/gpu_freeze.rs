// live-paper/gpu-freeze.mjs — re-verifies a frozen GPU bank against its dataset
// and the immutable journal source: normalization, chronology, development RMSE
// and inference parity all reproduced exactly.
use serde_json::{json, Value as V};

use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::js_int;

use crate::ml_bank::predict;
use crate::ml_shadow::{bind_training_source, verify_source};
use crate::model::digest;

fn date_parse(v: &V) -> f64 {
    v.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN)
}
fn f64v(v: &V) -> f64 { v.as_f64().unwrap_or(f64::NAN) }
fn xs(r: &V) -> Vec<f64> {
    r["x"].as_array().map(|a| a.iter().map(f64v).collect()).unwrap_or_default()
}

/// validateGPU(data, bank, raw).
pub fn validate_gpu(data: &V, bank: &V, raw: &str) -> Result<V, String> {
    let mut body = data.clone();
    let dataset_hash = body.as_object_mut()
        .and_then(|o| o.remove("datasetHash")).unwrap_or(V::Null);
    if dataset_hash != json!(digest(&body))
        || bank["datasetHash"] != dataset_hash
        || bank["configHash"] != json!(digest(&data["config"]))
        || digest(&bank["config"]) != digest(&data["config"]) {
        return Err("GPU dataset/config integrity failed".into());
    }
    let verified = verify_source(raw)?
        .ok_or("Cannot read properties of null (reading 'length')")?;
    bind_training_source(&verified, bank, false)?;
    let models = bank["models"].as_array().cloned().unwrap_or_default();
    let parity = bank["inferenceParity"].as_array().cloned().unwrap_or_default();
    if bank["family"].as_str() != Some("gpu_mlp")
        || js_int(&bank["config"]["feeBps"]) != Some(0)
        || js_int(&bank["modelsTrained"]) != Some(models.len() as i64)
        || models.len() != parity.len()
        || models.is_empty() {
        return Err("Invalid GPU bank".into());
    }
    let mut ids = std::collections::HashSet::new();
    let sets: std::collections::HashMap<i64, V> = data["sets"].as_array()
        .map(|a| a.iter().filter_map(|s| js_int(&s["horizon"]).map(|h| (h, s.clone()))).collect())
        .unwrap_or_default();
    let mut max_error = 0.0f64;
    for m in &models {
        if !ids.insert(crate::native_dip_host_v1::jstr(&m["id"]))
            || m["family"].as_str() != Some("gpu_mlp") {
            return Err("Duplicate/invalid model".into());
        }
        let s = sets.get(&js_int(&m["horizon"]).unwrap_or(i64::MIN)).cloned().unwrap_or(V::Null);
        let train = s["train"].as_array().cloned().unwrap_or_default();
        let test = s["test"].as_array().cloned().unwrap_or_default();
        if !s["eligible"].as_bool().unwrap_or(false) || train.len() < 20 || test.len() < 10
            || js_int(&m["trainRows"]) != Some(train.len() as i64)
            || js_int(&m["testRows"]) != Some(test.len() as i64)
            || digest(&m["coverage"]) != digest(&s["coverage"]) {
            return Err("Coverage mismatch".into());
        }
        let split_ms = date_parse(&bank["developmentSplitAt"]);
        if train.iter().any(|r| date_parse(&r["labelAt"]) >= split_ms)
            || test.iter().any(|r| date_parse(&r["at"]) < split_ms) {
            return Err("Unpurged chronology".into());
        }
        let max_label = train.iter().map(|r| date_parse(&r["labelAt"]))
            .fold(f64::NEG_INFINITY, f64::max);
        let max_label_iso = ms_to_iso(max_label);
        if date_parse(&m["maxTrainingLabelAt"]) != parse_iso_ms(&max_label_iso).unwrap_or(f64::NAN) {
            return Err("Training maturity mismatch".into());
        }
        let width = js_int(&m["width"]).unwrap_or(0) as usize;
        for j in 0..width {
            let x_at = |r: &V| r["x"].as_array().and_then(|a| a.get(j)).map(f64v).unwrap_or(f64::NAN);
            let mean = train.iter().map(x_at).sum::<f64>() / train.len() as f64;
            let var = train.iter().map(|r| (x_at(r) - mean).powi(2)).sum::<f64>() / train.len() as f64;
            let scale = var.sqrt().max(1e-6);
            let m_mean = m["means"].as_array().and_then(|a| a.get(j)).map(f64v).unwrap_or(f64::NAN);
            let m_scale = m["scales"].as_array().and_then(|a| a.get(j)).map(f64v).unwrap_or(f64::NAN);
            if (m_mean - mean).abs() > 1e-8 || (m_scale - scale).abs() > 1e-8 {
                return Err("Training normalization mismatch".into());
            }
        }
        let target_mean = train.iter().map(|r| f64v(&r["y"])).sum::<f64>() / train.len() as f64;
        let target_var = train.iter().map(|r| (f64v(&r["y"]) - target_mean).powi(2)).sum::<f64>() / train.len() as f64;
        let target_scale = target_var.sqrt().max(1e-6);
        if (f64v(&m["targetMean"]) - target_mean).abs() > 1e-10
            || (f64v(&m["targetScale"]) - target_scale).abs() > 1e-10 {
            return Err("Training target scaling mismatch".into());
        }
        let mse = test.iter().try_fold(0.0f64, |acc, r| {
            predict(m, &xs(r)).map(|p| acc + (p - f64v(&r["y"])).powi(2))
        })? / test.len() as f64;
        if (mse.sqrt() - f64v(&m["rmse"])).abs() > 1e-10 {
            return Err("Development score mismatch".into());
        }
        let p = parity.iter().find(|p| p["id"] == m["id"]).cloned()
            .ok_or("Cannot read properties of undefined (reading 'x')")?;
        let err = (predict(m, &xs(&p))? - f64v(&p["expected"])).abs();
        if err > 1e-10 { return Err("GPU inference parity failed".into()); }
        if err > max_error { max_error = err; }
    }
    for m in bank["selected"].as_array().cloned().unwrap_or_default().iter() {
        let found = models.iter().find(|x| x["id"] == m["id"]).cloned().unwrap_or(V::Null);
        if digest(m) != digest(&found) {
            return Err("Selected model changed".into());
        }
    }
    Ok(json!({
        "models": models.len(),
        "selected": bank["selected"].as_array().map(|a| a.len()).unwrap_or(0),
        "parityChecks": parity.len(),
        "maxAbsoluteInferenceError": max_error,
        "sourceVerified": true, "purgedChronologyVerified": true,
        "trainingOnlyNormalizationVerified": true, "developmentScoresReproduced": true,
    }))
}
