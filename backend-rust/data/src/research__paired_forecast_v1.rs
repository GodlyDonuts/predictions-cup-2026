// research/paired-forecast-v1.mjs — pooled frozen liquidation-change forecast.
// Predict future liquidation changes, with persistence as the shrinkage prior.
// Source quotes do not contain depth; this module never produces fills.
use serde_json::{json, Value as V};

pub fn definition() -> V {
    json!({"horizonsMs": [300000, 1800000, 7200000], "maximumLabelDelayMs": 60000,
        "ridgePenalty": 1000, "minimumTrainingRows": 1000, "minimumPredictedNet": 0.01,
        "trainingFraction": 0.6,
        "featureNames": ["discount_to_one", "quoted_roundtrip_cost", "lag_five_minute_change", "lag_unavailable"]})
}

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(format!("PAIR_FORECAST_{}", code)); }
    Ok(())
}

pub fn causal_features(rows: &[V], index: usize) -> Vec<f64> {
    let r = &rows[index];
    let mut j = index as i64 - 1;
    while j >= 0 && rows[j as usize]["at"].as_f64().unwrap_or(f64::NAN) > r["at"].as_f64().unwrap_or(f64::NAN) - 300000.0 { j -= 1; }
    let lag = if j >= 0 && r["at"].as_f64().unwrap_or(0.0) - rows[j as usize]["at"].as_f64().unwrap_or(0.0) <= 600000.0 {
        Some(&rows[j as usize])
    } else { None };
    vec![
        1.0 - (r["buy"].as_f64().unwrap_or(0.0) + r["sell"].as_f64().unwrap_or(0.0)) / 2.0,
        r["buy"].as_f64().unwrap_or(0.0) - r["sell"].as_f64().unwrap_or(0.0),
        lag.map(|l| r["sell"].as_f64().unwrap_or(0.0) - l["sell"].as_f64().unwrap_or(0.0)).unwrap_or(0.0),
        if lag.is_none() { 1.0 } else { 0.0 },
    ]
}

pub fn matured_labels(rows: &[V], horizon: f64, cutoff: f64) -> Vec<V> {
    let mut out = Vec::new();
    let mut j = 0usize;
    for i in 0..rows.len() {
        let r = &rows[i];
        j = j.max(i + 1);
        while j < rows.len() && rows[j]["at"].as_f64().unwrap_or(f64::NAN) < r["at"].as_f64().unwrap_or(f64::NAN) + horizon { j += 1; }
        if j == rows.len() { break; }
        let future = &rows[j];
        if future["at"].as_f64().unwrap_or(f64::NAN) > cutoff
            || future["at"].as_f64().unwrap_or(f64::NAN) > r["at"].as_f64().unwrap_or(f64::NAN) + horizon + definition()["maximumLabelDelayMs"].as_f64().unwrap_or(0.0)
            || future["minRequestAt"].as_f64().unwrap_or(f64::NAN) < r["at"].as_f64().unwrap_or(f64::NAN) + horizon {
            continue;
        }
        out.push(json!({"pair": r["pair"], "at": r["at"], "labelAt": future["at"],
            "x": causal_features(rows, i), "currentBuy": r["buy"], "currentSell": r["sell"],
            "y": future["sell"].as_f64().unwrap_or(0.0) - r["sell"].as_f64().unwrap_or(0.0),
            "net": future["sell"].as_f64().unwrap_or(0.0) - r["buy"].as_f64().unwrap_or(0.0)}));
    }
    out
}

fn solve(a: &[Vec<f64>], b: &[f64]) -> Result<Vec<f64>, String> {
    let n = b.len();
    let mut m: Vec<Vec<f64>> = a.iter().enumerate().map(|(i, r)| { let mut r = r.clone(); r.push(b[i]); r }).collect();
    for k in 0..n {
        let mut p = k;
        for i in (k + 1)..n {
            if m[i][k].abs() > m[p][k].abs() { p = i; }
        }
        m.swap(k, p);
        need(m[k][k].abs() > 1e-12, "SINGULAR")?;
        let v = m[k][k];
        for j in k..=n { m[k][j] /= v; }
        for i in 0..n {
            if i != k {
                let f = m[i][k];
                for j in k..=n { m[i][j] -= f * m[k][j]; }
            }
        }
    }
    Ok(m.iter().map(|r| r[n]).collect())
}

pub fn fit_frozen_forecast(labels: &[V], cutoff: f64) -> Result<V, String> {
    let definition = definition();
    let train: Vec<V> = labels.iter().filter(|r| r["labelAt"].as_f64().unwrap_or(f64::INFINITY) <= cutoff).cloned().collect();
    need(train.len() >= definition["minimumTrainingRows"].as_i64().unwrap_or(0) as usize, "INSUFFICIENT_TRAINING")?;
    let d = definition["featureNames"].as_array().map(|a| a.len()).unwrap_or(0);
    let mut mean = vec![0.0; d];
    let mut scale = vec![0.0; d];
    for r in &train {
        for j in 0..d { mean[j] += r["x"][j].as_f64().unwrap_or(0.0) / train.len() as f64; }
    }
    for r in &train {
        for j in 0..d { scale[j] += (r["x"][j].as_f64().unwrap_or(0.0) - mean[j]).powi(2) / train.len() as f64; }
    }
    for j in 0..d { scale[j] = scale[j].sqrt().max(1e-6); }
    let mut a = vec![vec![0.0; d + 1]; d + 1];
    let mut b = vec![0.0; d + 1];
    for r in &train {
        let x: Vec<f64> = std::iter::once(1.0)
            .chain(r["x"].as_array().unwrap().iter().enumerate().map(|(j, v)| (v.as_f64().unwrap_or(0.0) - mean[j]) / scale[j]))
            .collect();
        for i in 0..=d {
            b[i] += x[i] * r["y"].as_f64().unwrap_or(0.0);
            for j in 0..=d { a[i][j] += x[i] * x[j]; }
        }
    }
    // Intercept is penalized too: an unconditional historical drift is not alpha.
    let ridge = definition["ridgePenalty"].as_f64().unwrap_or(0.0);
    for i in 0..=d { a[i][i] += ridge; }
    Ok(json!({"schemaVersion": 1, "cutoff": cutoff, "definition": definition,
        "trainingRows": train.len(),
        "maximumTrainingLabelAt": train.iter().map(|r| r["labelAt"].as_f64().unwrap_or(f64::NEG_INFINITY)).fold(f64::NEG_INFINITY, f64::max),
        "mean": mean, "scale": scale, "coefficients": solve(&a, &b)?,
        "stateSpecificPredictors": false}))
}

pub fn forecast_change(model: &V, x: &[f64]) -> f64 {
    model["coefficients"][0].as_f64().unwrap_or(0.0)
        + x.iter().enumerate().map(|(j, v)| {
            model["coefficients"][j + 1].as_f64().unwrap_or(0.0)
                * (v - model["mean"][j].as_f64().unwrap_or(0.0)) / model["scale"][j].as_f64().unwrap_or(1.0)
        }).sum::<f64>()
}

pub fn score_forecast(model: &V, labels: &[V]) -> V {
    let later: Vec<V> = labels.iter().filter(|r| r["at"].as_f64().unwrap_or(f64::NEG_INFINITY) > model["cutoff"].as_f64().unwrap_or(0.0)).cloned().collect();
    let (mut sq, mut base_sq, mut selected, mut net) = (0.0, 0.0, 0i64, 0.0);
    let mut by_day: serde_json::Map<String, V> = serde_json::Map::new();
    for r in &later {
        let x: Vec<f64> = r["x"].as_array().cloned().unwrap_or_default().iter().map(|v| v.as_f64().unwrap_or(0.0)).collect();
        let change = forecast_change(model, &x);
        let predicted_net = r["currentSell"].as_f64().unwrap_or(0.0) + change - r["currentBuy"].as_f64().unwrap_or(0.0);
        sq += (change - r["y"].as_f64().unwrap_or(0.0)).powi(2);
        base_sq += r["y"].as_f64().unwrap_or(0.0).powi(2);
        let day = baal_service_runtime::contract::ms_to_iso(r["at"].as_f64().unwrap_or(0.0))[..10].to_string();
        let g = by_day.entry(day).or_insert_with(|| json!({"observations": 0, "selectedQuoteSignals": 0, "selectedQuoteNetSum": 0}));
        let obs = g["observations"].as_i64().unwrap_or(0);
        g["observations"] = json!(obs + 1);
        if predicted_net >= definition()["minimumPredictedNet"].as_f64().unwrap_or(0.0) {
            selected += 1;
            net += r["net"].as_f64().unwrap_or(0.0);
            let s = g["selectedQuoteSignals"].as_i64().unwrap_or(0);
            g["selectedQuoteSignals"] = json!(s + 1);
            let sum = g["selectedQuoteNetSum"].as_f64().unwrap_or(0.0) + r["net"].as_f64().unwrap_or(0.0);
            g["selectedQuoteNetSum"] = json!(sum);
        }
    }
    json!({"laterRows": later.len(),
        "rmse": if !later.is_empty() { json!((sq / later.len() as f64).sqrt()) } else { V::Null },
        "persistenceRmse": if !later.is_empty() { json!((base_sq / later.len() as f64).sqrt()) } else { V::Null },
        "selectedQuoteSignals": selected, "selectedQuoteNetSum": net,
        "selectedQuoteNetMean": if selected > 0 { json!(net / selected as f64) } else { V::Null },
        "byDay": by_day, "simulatedFills": 0, "realizedPnl": V::Null,
        "executionEvidence": false, "profitabilityEstablished": false})
}
