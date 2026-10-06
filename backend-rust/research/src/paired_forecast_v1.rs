// research/paired-forecast-v1.mjs — predict future liquidation changes, with
// persistence as the shrinkage prior. Source quotes do not contain depth; this
// module never produces fills.
use crate::js::{iso, js_max, js_number, num, require_that};
use indexmap::IndexMap;
use serde_json::{json, Value as V};

pub fn definition() -> V {
    json!({
        "horizonsMs": [300000, 1800000, 7200000],
        "maximumLabelDelayMs": 60000,
        "ridgePenalty": 1000,
        "minimumTrainingRows": 1000,
        "minimumPredictedNet": 0.01,
        "trainingFraction": 0.6,
        "featureNames": ["discount_to_one", "quoted_roundtrip_cost", "lag_five_minute_change", "lag_unavailable"],
    })
}

fn need(v: bool, c: &str) -> Result<(), String> {
    require_that(v, format!("PAIR_FORECAST_{c}"))
}

pub fn causal_features(rows: &[V], index: usize) -> Vec<f64> {
    let r = &rows[index];
    let rat = js_number(r.get("at"));
    let mut j = index as i64 - 1;
    while j >= 0 && js_number(rows[j as usize].get("at")) > rat - 300000.0 {
        j -= 1;
    }
    let lag = if j >= 0 && rat - js_number(rows[j as usize].get("at")) <= 600000.0 {
        Some(&rows[j as usize])
    } else {
        None
    };
    let buy = js_number(r.get("buy"));
    let sell = js_number(r.get("sell"));
    vec![
        1.0 - (buy + sell) / 2.0,
        buy - sell,
        match lag {
            Some(l) => sell - js_number(l.get("sell")),
            None => 0.0,
        },
        if lag.is_some() { 0.0 } else { 1.0 },
    ]
}

pub fn matured_labels(rows: &[V], horizon: f64, cutoff: f64) -> Vec<V> {
    let max_delay = js_number(definition().get("maximumLabelDelayMs"));
    let mut out: Vec<V> = Vec::new();
    let mut j = 0usize;
    for i in 0..rows.len() {
        let r = &rows[i];
        let rat = js_number(r.get("at"));
        j = j.max(i + 1);
        while j < rows.len() && js_number(rows[j].get("at")) < rat + horizon {
            j += 1;
        }
        if j == rows.len() {
            break;
        }
        let future = &rows[j];
        if js_number(future.get("at")) > cutoff
            || js_number(future.get("at")) > rat + horizon + max_delay
            || js_number(future.get("minRequestAt")) < rat + horizon
        {
            continue;
        }
        out.push(json!({
            "pair": r["pair"],
            "at": r["at"],
            "labelAt": future["at"],
            "x": causal_features(rows, i).iter().map(|v| num(*v)).collect::<Vec<V>>(),
            "currentBuy": r["buy"],
            "currentSell": r["sell"],
            "y": num(js_number(future.get("sell")) - js_number(r.get("sell"))),
            "net": num(js_number(future.get("sell")) - js_number(r.get("buy"))),
        }));
    }
    out
}

fn solve(a: &[Vec<f64>], b: &[f64]) -> Result<Vec<f64>, String> {
    let n = b.len();
    let mut m: Vec<Vec<f64>> = a
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let mut r = r.clone();
            r.push(b[i]);
            r
        })
        .collect();
    for k in 0..n {
        let mut p = k;
        for i in (k + 1)..n {
            if m[i][k].abs() > m[p][k].abs() {
                p = i;
            }
        }
        m.swap(k, p);
        need(m[k][k].abs() > 1e-12, "SINGULAR")?;
        let v = m[k][k];
        for j in k..=n {
            m[k][j] /= v;
        }
        for i in 0..n {
            if i != k {
                let f = m[i][k];
                for j in k..=n {
                    m[i][j] -= f * m[k][j];
                }
            }
        }
    }
    Ok(m.iter().map(|r| r[n]).collect())
}

fn x_of(r: &V) -> Vec<f64> {
    r["x"]
        .as_array()
        .map(|a| a.iter().map(|v| js_number(Some(v))).collect())
        .unwrap_or_default()
}

pub fn fit_frozen_forecast(labels: &[V], cutoff: f64) -> Result<V, String> {
    let def = definition();
    let train: Vec<&V> = labels
        .iter()
        .filter(|r| js_number(r.get("labelAt")) <= cutoff)
        .collect();
    need(
        train.len() as f64 >= js_number(def.get("minimumTrainingRows")),
        "INSUFFICIENT_TRAINING",
    )?;
    let d = def["featureNames"].as_array().map(|a| a.len()).unwrap_or(0);
    let mut mean = vec![0.0; d];
    let mut scale = vec![0.0; d];
    for r in &train {
        let x = x_of(r);
        for j in 0..d {
            mean[j] += x.get(j).copied().unwrap_or(f64::NAN) / train.len() as f64;
        }
    }
    for r in &train {
        let x = x_of(r);
        for j in 0..d {
            scale[j] += (x.get(j).copied().unwrap_or(f64::NAN) - mean[j]).powi(2) / train.len() as f64;
        }
    }
    for j in 0..d {
        scale[j] = js_max(1e-6, scale[j].sqrt());
    }
    let mut a = vec![vec![0.0; d + 1]; d + 1];
    let mut b = vec![0.0; d + 1];
    for r in &train {
        let xr = x_of(r);
        let mut x = vec![1.0];
        for j in 0..d {
            x.push((xr.get(j).copied().unwrap_or(f64::NAN) - mean[j]) / scale[j]);
        }
        let y = js_number(r.get("y"));
        for i in 0..=d {
            b[i] += x[i] * y;
            for j in 0..=d {
                a[i][j] += x[i] * x[j];
            }
        }
    }
    // Intercept is penalized too: an unconditional historical drift is not alpha.
    for i in 0..=d {
        a[i][i] += js_number(def.get("ridgePenalty"));
    }
    let coefficients = solve(&a, &b)?;
    Ok(json!({
        "schemaVersion": 1,
        "cutoff": num(cutoff),
        "definition": def,
        "trainingRows": train.len(),
        "maximumTrainingLabelAt": num(train.iter().fold(f64::NEG_INFINITY, |v, r| js_max(v, js_number(r.get("labelAt"))))),
        "mean": mean.iter().map(|v| num(*v)).collect::<Vec<V>>(),
        "scale": scale.iter().map(|v| num(*v)).collect::<Vec<V>>(),
        "coefficients": coefficients.iter().map(|v| num(*v)).collect::<Vec<V>>(),
        "stateSpecificPredictors": false,
    }))
}

pub fn forecast_change(model: &V, x: &[f64]) -> f64 {
    let empty: Vec<V> = Vec::new();
    let coefs = model["coefficients"].as_array().unwrap_or(&empty);
    let means = model["mean"].as_array().unwrap_or(&empty);
    let scales = model["scale"].as_array().unwrap_or(&empty);
    js_number(coefs.first())
        + x.iter()
            .enumerate()
            .map(|(j, v)| {
                js_number(coefs.get(j + 1))
                    * (*v - js_number(means.get(j)))
                    / js_number(scales.get(j))
            })
            .sum::<f64>()
}

pub fn score_forecast(model: &V, labels: &[V]) -> V {
    let cutoff = js_number(model.get("cutoff"));
    let min_net = js_number(definition().get("minimumPredictedNet"));
    let later: Vec<&V> = labels.iter().filter(|r| js_number(r.get("at")) > cutoff).collect();
    let mut sq = 0.0;
    let mut base_sq = 0.0;
    let mut selected = 0usize;
    let mut net = 0.0;
    let mut by_day: IndexMap<String, V> = IndexMap::new();
    for r in &later {
        let change = forecast_change(model, &x_of(r));
        let predicted_net = js_number(r.get("currentSell")) + change - js_number(r.get("currentBuy"));
        sq += (change - js_number(r.get("y"))).powi(2);
        base_sq += js_number(r.get("y")).powi(2);
        let day = iso(js_number(r.get("at")))[..10].to_string();
        let g = by_day.entry(day).or_insert_with(|| {
            json!({"observations": 0, "selectedQuoteSignals": 0, "selectedQuoteNetSum": 0})
        });
        *g.get_mut("observations").unwrap() = num(js_number(g.get("observations")) + 1.0);
        if predicted_net >= min_net {
            selected += 1;
            net += js_number(r.get("net"));
            *g.get_mut("selectedQuoteSignals").unwrap() =
                num(js_number(g.get("selectedQuoteSignals")) + 1.0);
            *g.get_mut("selectedQuoteNetSum").unwrap() =
                num(js_number(g.get("selectedQuoteNetSum")) + js_number(r.get("net")));
        }
    }
    json!({
        "laterRows": later.len(),
        "rmse": if later.is_empty() { V::Null } else { num((sq / later.len() as f64).sqrt()) },
        "persistenceRmse": if later.is_empty() { V::Null } else { num((base_sq / later.len() as f64).sqrt()) },
        "selectedQuoteSignals": selected,
        "selectedQuoteNetSum": num(net),
        "selectedQuoteNetMean": if selected > 0 { num(net / selected as f64) } else { V::Null },
        "byDay": by_day,
        "simulatedFills": 0,
        "realizedPnl": V::Null,
        "executionEvidence": false,
        "profitabilityEstablished": false,
    })
}
