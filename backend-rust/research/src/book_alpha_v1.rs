// research/book-alpha-v1.mjs — offline book microstructure alpha research.
// Floating regression outputs never serve as a ledger.
use crate::js::{field_str, iso, js_min, js_number, js_template, num, require_that};
use indexmap::IndexMap;
use serde_json::{json, Value as V};

// Fixed before capture: pooled ridge200, one-share/1% depth, protective1tick,
// independent later receipts, three horizons, chronological60% clock split.
pub fn book_alpha_policy() -> V {
    json!({
        "horizonsMs": [30000, 120000, 300000],
        "ridge": 200,
        "trainFraction": 0.6,
        "latencyMs": 1000,
        "maximumLatenessMs": 30000,
        "minimumNet": 0.005,
        "tick": 0.005,
        "depthFractionBps": 100,
        "fees": 0,
    })
}

fn need(v: bool, c: &str) -> Result<(), String> {
    require_that(v, format!("BOOK_ALPHA_{c}"))
}

// `active` is the JS Set of live market ids (order irrelevant).
pub fn reset_book_epochs(active: &mut Vec<V>, row: &V) -> bool {
    if field_str(row, "kind") == "book" {
        return false;
    }
    let market_id = row.get("payload").and_then(|p| p.get("marketId"));
    if field_str(row, "kind") == "invalidation" && !matches!(market_id, Some(V::Null)) {
        if let Some(i) = active.iter().position(|x| Some(x) == market_id) {
            active.remove(i);
        }
    } else {
        active.clear();
    }
    true
}

pub fn book_features(book: &V, prior: Option<&V>) -> Option<V> {
    let empty: Vec<V> = Vec::new();
    let bids = book["bids"].as_array().unwrap_or(&empty);
    let asks = book["asks"].as_array().unwrap_or(&empty);
    let bid = js_number(bids.first().and_then(|l| l.get("price")));
    let ask = js_number(asks.first().and_then(|l| l.get("price")));
    if !bid.is_finite() || !ask.is_finite() || bid < 0.005 || ask > 0.995 || bid > ask {
        return None;
    }
    let bq: f64 = bids
        .iter()
        .take(5)
        .map(|l| js_min(js_number(l.get("quantity")), 1e9))
        .sum();
    let aq: f64 = asks
        .iter()
        .take(5)
        .map(|l| js_min(js_number(l.get("quantity")), 1e9))
        .sum();
    if bq + aq <= 0.0 {
        return None;
    }
    let mid = (bid + ask) / 2.0;
    let imbalance = (bq - aq) / (bq + aq);
    let past_change = match prior {
        Some(p) => mid - js_number(p.get("mid")),
        None => 0.0,
    };
    Some(json!({
        "bid": num(bid),
        "ask": num(ask),
        "mid": num(mid),
        "features": [
            num(ask - bid),
            num(imbalance),
            num(imbalance * (ask - bid) / 2.0),
            num(past_change),
            num(mid * (1.0 - mid)),
        ],
        "buyCapacity": num((js_number(asks.first().and_then(|l| l.get("quantity"))) / 100.0).floor()),
        "sellCapacity": num((js_number(bids.first().and_then(|l| l.get("quantity"))) / 100.0).floor()),
    }))
}

fn first_after<'a>(rows: &'a [V], at: f64) -> Option<&'a V> {
    let (mut lo, mut hi) = (0usize, rows.len());
    while lo < hi {
        let m = (lo + hi) >> 1;
        if js_number(rows[m].get("requestAt")) < at {
            lo = m + 1;
        } else {
            hi = m;
        }
    }
    rows.get(lo)
}

pub fn causal_book_labels(streams: &[Vec<V>], horizon: f64, policy: &V) -> Vec<V> {
    let latency = js_number(policy.get("latencyMs"));
    let max_lateness = js_number(policy.get("maximumLatenessMs"));
    let tick = js_number(policy.get("tick"));
    let mut labels: Vec<V> = Vec::new();
    for rows in streams {
        for r in rows {
            let rat = js_number(r.get("at"));
            let buy = first_after(rows, rat + latency);
            let exit = first_after(rows, rat + horizon);
            let (Some(buy), Some(exit)) = (buy, exit) else {
                continue;
            };
            if js_number(buy.get("sequence")) <= js_number(r.get("sequence"))
                || js_number(exit.get("sequence")) <= js_number(buy.get("sequence"))
                || js_number(buy.get("requestAt")) <= rat
                || js_number(exit.get("requestAt")) < js_number(buy.get("at")) + latency
                || js_number(buy.get("requestAt")) - rat > max_lateness
                || js_number(exit.get("requestAt")) - rat - horizon > max_lateness
                || js_number(buy.get("buyCapacity")) < 1.0
                || js_number(exit.get("sellCapacity")) < 1.0
                || js_number(buy.get("ask")) > js_number(r.get("ask")) + tick + 1e-12
            {
                continue;
            }
            let mut label = r.clone();
            if let V::Object(ref mut m) = label {
                m.insert("labelAt".into(), exit["at"].clone());
                m.insert(
                    "target".into(),
                    num(js_number(exit.get("bid")) - js_number(r.get("bid"))),
                );
                m.insert("observedAcquisition".into(), buy["ask"].clone());
                m.insert("observedLiquidation".into(), exit["bid"].clone());
                m.insert("entryAt".into(), buy["at"].clone());
                m.insert("exitAt".into(), exit["at"].clone());
            }
            labels.push(label);
        }
    }
    labels.sort_by(|a, b| {
        js_number(a.get("at"))
            .partial_cmp(&js_number(b.get("at")))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| field_str(a, "marketId").cmp(field_str(b, "marketId")))
    });
    labels
}

fn solve(matrix: &[Vec<f64>], vector: &[f64]) -> Result<Vec<f64>, String> {
    let n = vector.len();
    let mut a: Vec<Vec<f64>> = matrix
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let mut r = r.clone();
            r.push(vector[i]);
            r
        })
        .collect();
    for p in 0..n {
        let mut max = p;
        for i in (p + 1)..n {
            if a[i][p].abs() > a[max][p].abs() {
                max = i;
            }
        }
        a.swap(p, max);
        need(a[p][p].abs() > 1e-12, "SINGULAR")?;
        let d = a[p][p];
        for j in p..=n {
            a[p][j] /= d;
        }
        for i in 0..n {
            if i != p {
                let f = a[i][p];
                for j in p..=n {
                    a[i][j] -= f * a[p][j];
                }
            }
        }
    }
    Ok(a.iter().map(|r| r[n]).collect())
}

pub fn fit_book_alpha(labels: &[V], cutoff: f64, policy: &V) -> Result<V, String> {
    let train: Vec<&V> = labels
        .iter()
        .filter(|r| js_number(r.get("at")) < cutoff && js_number(r.get("labelAt")) <= cutoff)
        .collect();
    need(train.len() >= 20, "INSUFFICIENT_CHRONOLOGY")?;
    let dimensions = train[0]["features"].as_array().map(|a| a.len()).unwrap_or(0);
    let n = dimensions + 1;
    let mut mean = vec![0.0; dimensions];
    for j in 0..dimensions {
        mean[j] = train
            .iter()
            .map(|r| js_number(r["features"].get(j)))
            .sum::<f64>()
            / train.len() as f64;
    }
    let mut scale = vec![0.0; dimensions];
    for (j, m) in mean.iter().enumerate() {
        let sd = (train
            .iter()
            .map(|r| (js_number(r["features"].get(j)) - m).powi(2))
            .sum::<f64>()
            / train.len() as f64)
            .sqrt();
        // JS `Math.sqrt(x) || 1`: 0 and NaN are falsy.
        scale[j] = if sd == 0.0 || sd.is_nan() { 1.0 } else { sd };
    }
    let mut matrix = vec![vec![0.0; n]; n];
    let mut vector = vec![0.0; n];
    for r in &train {
        let mut x = vec![1.0];
        for j in 0..dimensions {
            x.push((js_number(r["features"].get(j)) - mean[j]) / scale[j]);
        }
        let target = js_number(r.get("target"));
        for i in 0..n {
            vector[i] += x[i] * target;
            for j in 0..n {
                matrix[i][j] += x[i] * x[j];
            }
        }
    }
    let ridge = js_number(policy.get("ridge"));
    for i in 0..n {
        matrix[i][i] += ridge;
    }
    let coefficients = solve(&matrix, &vector)?;
    Ok(json!({
        "cutoff": num(cutoff),
        "trainLabels": train.len(),
        "mean": mean.iter().map(|v| num(*v)).collect::<Vec<V>>(),
        "scale": scale.iter().map(|v| num(*v)).collect::<Vec<V>>(),
        "coefficients": coefficients.iter().map(|v| num(*v)).collect::<Vec<V>>(),
        "policy": policy,
    }))
}

pub fn predict_book_alpha(model: &V, row: &V) -> f64 {
    let empty: Vec<V> = Vec::new();
    let coefs = model["coefficients"].as_array().unwrap_or(&empty);
    let means = model["mean"].as_array().unwrap_or(&empty);
    let scales = model["scale"].as_array().unwrap_or(&empty);
    let features = row["features"].as_array().unwrap_or(&empty);
    js_number(coefs.first())
        + features
            .iter()
            .enumerate()
            .map(|(j, v)| {
                js_number(coefs.get(j + 1))
                    * (js_number(Some(v)) - js_number(means.get(j)))
                    / js_number(scales.get(j))
            })
            .sum::<f64>()
}

pub fn score_book_alpha(model: &V, labels: &[V], decisions: &[V]) -> V {
    let cutoff = js_number(model.get("cutoff"));
    let later: Vec<&V> = labels.iter().filter(|r| js_number(r.get("at")) >= cutoff).collect();
    let mut outcomes: IndexMap<String, &V> = IndexMap::new();
    for r in &later {
        outcomes.insert(
            format!("{}:{}", js_template(r.get("marketId")), js_template(r.get("sequence"))),
            r,
        );
    }
    let mut model_square = 0.0;
    let mut baseline_square = 0.0;
    for r in &later {
        let prediction = predict_book_alpha(model, r);
        model_square += (prediction - js_number(r.get("target"))).powi(2);
        baseline_square += js_number(r.get("target")).powi(2);
    }
    let tick = js_number(model["policy"].get("tick"));
    let minimum_net = js_number(model["policy"].get("minimumNet"));
    let available: Vec<&V> = decisions.iter().filter(|r| js_number(r.get("at")) >= cutoff).collect();
    let mut selected: Vec<V> = Vec::new();
    for r in &available {
        let prediction = predict_book_alpha(model, r);
        let predicted_net = js_number(r.get("bid")) + prediction - js_number(r.get("ask")) - tick;
        if predicted_net >= minimum_net {
            let key = format!("{}:{}", js_template(r.get("marketId")), js_template(r.get("sequence")));
            let outcome = outcomes.get(&key);
            selected.push(json!({
                "marketId": r["marketId"],
                "at": iso(js_number(r.get("at"))),
                "predictedNet": num(predicted_net),
                "laterReceiptOutcomeAvailable": outcome.is_some(),
                "observedRawQuoteMargin": outcome.map(|o| num(js_number(o.get("observedLiquidation")) - js_number(o.get("observedAcquisition")))).unwrap_or(V::Null),
                "oneTickPerSideMargin": outcome.map(|o| num(js_number(o.get("observedLiquidation")) - js_number(o.get("observedAcquisition")) - 2.0 * tick)).unwrap_or(V::Null),
            }));
        }
    }
    let censored = selected
        .iter()
        .filter(|x| !x["laterReceiptOutcomeAvailable"].as_bool().unwrap_or(false))
        .count();
    json!({
        "laterLabels": later.len(),
        "laterDecisionObservations": available.len(),
        "rmse": if later.is_empty() { V::Null } else { num((model_square / later.len() as f64).sqrt()) },
        "unchangedBidRmse": if later.is_empty() { V::Null } else { num((baseline_square / later.len() as f64).sqrt()) },
        "selectedQuoteSignals": selected.len(),
        "censoredSelectedSignals": censored,
        "selected": selected,
        "accountRequests": 0,
        "acceptedOrders": 0,
        "realizedPnl": V::Null,
        "profitabilityEstablished": false,
    })
}
