// research/book-alpha-v1.mjs — offline causal book-alpha ridge regression.
// Offline research only. Floating regression outputs never serve as a ledger.
use serde_json::{json, Value as V};

pub fn book_alpha_policy() -> V {
    json!({"horizonsMs": [30000, 120000, 300000], "ridge": 200, "trainFraction": 0.6,
        "latencyMs": 1000, "maximumLatenessMs": 30000, "minimumNet": 0.005, "tick": 0.005,
        "depthFractionBps": 100, "fees": 0})
}

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(format!("BOOK_ALPHA_{}", code)); }
    Ok(())
}

// resetBookEpochs(active,row) — active is Map<marketId, stream>. JS `marketId!==null`:
// a missing key is !==null (delete, no-op); explicit null falls through to clear().
pub fn reset_book_epochs<T>(active: &mut indexmap::IndexMap<String, T>, row: &V) -> bool {
    if row["kind"].as_str() == Some("book") { return false; }
    let targeted = row["kind"].as_str() == Some("invalidation")
        && row["payload"].as_object().map(|p| p.get("marketId") != Some(&V::Null)).unwrap_or(true);
    if targeted {
        if let Some(k) = row["payload"]["marketId"].as_str().map(String::from) {
            active.shift_remove(&k);
        }
    } else {
        active.clear();
    }
    true
}

pub fn book_features(book: &V, prior: Option<&V>) -> Option<V> {
    let bid = book["bids"][0]["price"].as_f64()?;
    let ask = book["asks"][0]["price"].as_f64()?;
    if !bid.is_finite() || !ask.is_finite() || bid < 0.005 || ask > 0.995 || bid > ask { return None; }
    let bq: f64 = book["bids"].as_array()?.iter().take(5).map(|l| l["quantity"].as_f64().unwrap_or(0.0).min(1e9)).sum();
    let aq: f64 = book["asks"].as_array()?.iter().take(5).map(|l| l["quantity"].as_f64().unwrap_or(0.0).min(1e9)).sum();
    if bq + aq <= 0.0 { return None; }
    let mid = (bid + ask) / 2.0;
    let imbalance = (bq - aq) / (bq + aq);
    let past_change = prior.map(|p| mid - p["mid"].as_f64().unwrap_or(0.0)).unwrap_or(0.0);
    Some(json!({"bid": bid, "ask": ask, "mid": mid,
        "features": [ask - bid, imbalance, imbalance * (ask - bid) / 2.0, past_change, mid * (1.0 - mid)],
        "buyCapacity": (book["asks"][0]["quantity"].as_f64().unwrap_or(0.0) / 100.0).floor(),
        "sellCapacity": (book["bids"][0]["quantity"].as_f64().unwrap_or(0.0) / 100.0).floor()}))
}

fn first_after<'a>(rows: &'a [V], at: f64) -> Option<&'a V> {
    let (mut lo, mut hi) = (0usize, rows.len());
    while lo < hi {
        let m = (lo + hi) >> 1;
        if rows[m]["requestAt"].as_f64().unwrap_or(f64::NAN) < at { lo = m + 1; } else { hi = m; }
    }
    rows.get(lo)
}

pub fn causal_book_labels(streams: &[Vec<V>], horizon: f64, policy: &V) -> Vec<V> {
    let mut labels = Vec::new();
    for rows in streams {
        for r in rows.iter() {
            let buy = first_after(rows, r["at"].as_f64().unwrap_or(f64::NAN) + policy["latencyMs"].as_f64().unwrap_or(0.0));
            let exit = first_after(rows, r["at"].as_f64().unwrap_or(f64::NAN) + horizon);
            let (buy, exit) = match (buy, exit) { (Some(b), Some(e)) => (b, e), _ => continue };
            if !(buy["sequence"].as_f64().unwrap_or(0.0) > r["sequence"].as_f64().unwrap_or(0.0)
                && exit["sequence"].as_f64().unwrap_or(0.0) > buy["sequence"].as_f64().unwrap_or(0.0)
                && buy["requestAt"].as_f64().unwrap_or(f64::NAN) > r["at"].as_f64().unwrap_or(f64::NAN)
                && exit["requestAt"].as_f64().unwrap_or(f64::NAN) >= buy["at"].as_f64().unwrap_or(f64::NAN) + policy["latencyMs"].as_f64().unwrap_or(0.0)
                && buy["requestAt"].as_f64().unwrap_or(f64::NAN) - r["at"].as_f64().unwrap_or(f64::NAN) <= policy["maximumLatenessMs"].as_f64().unwrap_or(0.0)
                && exit["requestAt"].as_f64().unwrap_or(f64::NAN) - r["at"].as_f64().unwrap_or(f64::NAN) - horizon <= policy["maximumLatenessMs"].as_f64().unwrap_or(0.0)
                && buy["buyCapacity"].as_f64().unwrap_or(0.0) >= 1.0
                && exit["sellCapacity"].as_f64().unwrap_or(0.0) >= 1.0
                && buy["ask"].as_f64().unwrap_or(f64::NAN) > r["ask"].as_f64().unwrap_or(f64::NAN) + policy["tick"].as_f64().unwrap_or(0.0) + 1e-12) {
                continue;
            }
            let mut l = r.clone();
            l["labelAt"] = exit["at"].clone();
            l["target"] = json!(exit["bid"].as_f64().unwrap_or(0.0) - r["bid"].as_f64().unwrap_or(0.0));
            l["observedAcquisition"] = buy["ask"].clone();
            l["observedLiquidation"] = exit["bid"].clone();
            l["entryAt"] = buy["at"].clone();
            l["exitAt"] = exit["at"].clone();
            labels.push(l);
        }
    }
    labels.sort_by(|a, b| {
        a["at"].as_f64().unwrap_or(f64::NAN).partial_cmp(&b["at"].as_f64().unwrap_or(f64::NAN)).unwrap_or(std::cmp::Ordering::Equal)
            .then(a["marketId"].as_str().unwrap_or("").cmp(b["marketId"].as_str().unwrap_or("")))
    });
    labels
}

fn solve(matrix: &[Vec<f64>], vector: &[f64]) -> Result<Vec<f64>, String> {
    let n = vector.len();
    let mut a: Vec<Vec<f64>> = matrix.iter().enumerate().map(|(i, r)| { let mut r = r.clone(); r.push(vector[i]); r }).collect();
    for p in 0..n {
        let mut max = p;
        for i in (p + 1)..n {
            if a[i][p].abs() > a[max][p].abs() { max = i; }
        }
        a.swap(p, max);
        need(a[p][p].abs() > 1e-12, "SINGULAR")?;
        let d = a[p][p];
        for j in p..=n { a[p][j] /= d; }
        for i in 0..n {
            if i != p {
                let f = a[i][p];
                for j in p..=n { a[i][j] -= f * a[p][j]; }
            }
        }
    }
    Ok(a.iter().map(|r| r[n]).collect())
}

pub fn fit_book_alpha(labels: &[V], cutoff: f64, policy: &V) -> Result<V, String> {
    let train: Vec<V> = labels.iter().filter(|r| {
        r["at"].as_f64().unwrap_or(f64::INFINITY) < cutoff
            && r["labelAt"].as_f64().unwrap_or(f64::INFINITY) <= cutoff
    }).cloned().collect();
    need(train.len() >= 20, "INSUFFICIENT_CHRONOLOGY")?;
    let dimensions = train[0]["features"].as_array().map(|a| a.len()).unwrap_or(0);
    let mean: Vec<f64> = (0..dimensions).map(|j| {
        train.iter().map(|r| r["features"][j].as_f64().unwrap_or(0.0)).sum::<f64>() / train.len() as f64
    }).collect();
    let scale: Vec<f64> = mean.iter().enumerate().map(|(j, m)| {
        let v = (train.iter().map(|r| (r["features"][j].as_f64().unwrap_or(0.0) - m).powi(2)).sum::<f64>() / train.len() as f64).sqrt();
        if v == 0.0 { 1.0 } else { v }
    }).collect();
    let n = dimensions + 1;
    let mut matrix = vec![vec![0.0; n]; n];
    let mut vector = vec![0.0; n];
    for r in &train {
        let x: Vec<f64> = std::iter::once(1.0)
            .chain(r["features"].as_array().unwrap().iter().enumerate().map(|(j, v)| (v.as_f64().unwrap_or(0.0) - mean[j]) / scale[j]))
            .collect();
        for i in 0..n {
            vector[i] += x[i] * r["target"].as_f64().unwrap_or(0.0);
            for j in 0..n { matrix[i][j] += x[i] * x[j]; }
        }
    }
    let ridge = policy["ridge"].as_f64().unwrap_or(0.0);
    for i in 0..n { matrix[i][i] += ridge; }
    let coefficients = solve(&matrix, &vector)?;
    Ok(json!({"cutoff": cutoff, "trainLabels": train.len(), "mean": mean, "scale": scale,
        "coefficients": coefficients, "policy": policy}))
}

pub fn predict_book_alpha(model: &V, row: &V) -> f64 {
    model["coefficients"][0].as_f64().unwrap_or(0.0)
        + row["features"].as_array().cloned().unwrap_or_default().iter().enumerate()
            .map(|(j, v)| {
                model["coefficients"][j + 1].as_f64().unwrap_or(0.0)
                    * (v.as_f64().unwrap_or(0.0) - model["mean"][j].as_f64().unwrap_or(0.0))
                    / model["scale"][j].as_f64().unwrap_or(1.0)
            }).sum::<f64>()
}

pub fn score_book_alpha(model: &V, labels: &[V], decisions: Option<&[V]>) -> V {
    let later: Vec<V> = labels.iter().filter(|r| r["at"].as_f64().unwrap_or(f64::NEG_INFINITY) >= model["cutoff"].as_f64().unwrap_or(0.0)).cloned().collect();
    let outcomes: std::collections::HashMap<String, V> = later.iter()
        .map(|r| (format!("{}:{}", r["marketId"].as_str().unwrap_or(""), r["sequence"].as_f64().unwrap_or(0.0)), r.clone())).collect();
    let mut selected = Vec::new();
    let (mut model_square, mut baseline_square) = (0.0, 0.0);
    for r in &later {
        let prediction = predict_book_alpha(model, r);
        let target = r["target"].as_f64().unwrap_or(0.0);
        model_square += (prediction - target).powi(2);
        baseline_square += target.powi(2);
    }
    let available: Vec<V> = decisions.unwrap_or(labels).iter()
        .filter(|r| r["at"].as_f64().unwrap_or(f64::NEG_INFINITY) >= model["cutoff"].as_f64().unwrap_or(0.0)).cloned().collect();
    for r in &available {
        let prediction = predict_book_alpha(model, r);
        let predicted_net = r["bid"].as_f64().unwrap_or(0.0) + prediction - r["ask"].as_f64().unwrap_or(0.0) - model["policy"]["tick"].as_f64().unwrap_or(0.0);
        if predicted_net >= model["policy"]["minimumNet"].as_f64().unwrap_or(0.0) {
            let key = format!("{}:{}", r["marketId"].as_str().unwrap_or(""), r["sequence"].as_f64().unwrap_or(0.0));
            let outcome = outcomes.get(&key);
            selected.push(json!({
                "marketId": r["marketId"],
                "at": baal_service_runtime::contract::ms_to_iso(r["at"].as_f64().unwrap_or(0.0)),
                "predictedNet": predicted_net,
                "laterReceiptOutcomeAvailable": outcome.is_some(),
                "observedRawQuoteMargin": outcome.map(|o| o["observedLiquidation"].as_f64().unwrap_or(0.0) - o["observedAcquisition"].as_f64().unwrap_or(0.0)),
                "oneTickPerSideMargin": outcome.map(|o| o["observedLiquidation"].as_f64().unwrap_or(0.0) - o["observedAcquisition"].as_f64().unwrap_or(0.0) - 2.0 * model["policy"]["tick"].as_f64().unwrap_or(0.0)),
            }));
        }
    }
    let censored = selected.iter().filter(|x| x["laterReceiptOutcomeAvailable"].as_bool() != Some(true)).count();
    json!({"laterLabels": later.len(), "laterDecisionObservations": available.len(),
        "rmse": if !later.is_empty() { json!((model_square / later.len() as f64).sqrt()) } else { V::Null },
        "unchangedBidRmse": if !later.is_empty() { json!((baseline_square / later.len() as f64).sqrt()) } else { V::Null },
        "selectedQuoteSignals": selected.len(), "censoredSelectedSignals": censored,
        "selected": selected, "accountRequests": 0, "acceptedOrders": 0,
        "realizedPnl": V::Null, "profitabilityEstablished": false})
}
