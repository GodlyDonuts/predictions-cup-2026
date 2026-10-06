// ml/model.mjs — standardized-feature ridge fit, metrics, alpha selection and
// whole-session cluster bootstrap. Byte/semantic port.
use crate::core::{canonical, hash, js_read_length, js_read_length_opt, js_stringify, to_f64, Error, R, FEATURE_NAMES};
use serde_json::{json, Value as V};

fn finite(xs: &[f64]) -> R<()> {
    if xs.iter().any(|x| !x.is_finite()) { return Err(Error::code("NONFINITE_MODEL")); }
    Ok(())
}
/// Number.isFinite(v): true only for finite V::Number — no coercion.
fn finite_json(v: &V) -> bool { v.as_f64().is_some_and(|x| x.is_finite()) }
/// Neumaier-compensated mean (same ordering as the JS loop).
fn mean(xs: &[f64]) -> f64 {
    let mut sum = 0.0f64;
    let mut correction = 0.0f64;
    for x in xs {
        let adjusted = *x - correction;
        let next = sum + adjusted;
        correction = (next - sum) - adjusted;
        sum = next;
    }
    sum / xs.len() as f64
}
/// JS `[...v]` on a length-bearing value: array elements or string chars.
fn elements(v: &V) -> Vec<V> {
    match v {
        V::Array(a) => a.clone(),
        V::String(s) => s.chars().map(|c| V::String(c.to_string())).collect(),
        _ => Vec::new(),
    }
}

/// fitRidge(samples, alpha) — train-only standardization, centered target,
/// unpenalized intercept. Objective: mean((y - intercept - z*w)^2) +
/// alpha * sum(w^2).
pub fn fit_ridge(samples: &[V], alpha: f64) -> R<V> {
    if samples.len() < 2 || !alpha.is_finite() || alpha <= 0.0 { return Err(Error::code("FIT_INPUT")); }
    let d = FEATURE_NAMES.len();
    for s in samples {
        match js_read_length_opt(s.get("x"))? {
            Some(l) if l == d => {}
            _ => return Err(Error::code("FEATURE_DIMENSION")),
        }
        let mut flat: Vec<V> = elements(&s["x"]);
        flat.push(s["y"].clone());
        if flat.iter().any(|v| !finite_json(v)) { return Err(Error::code("NONFINITE_MODEL")); }
    }
    let n = samples.len();
    let xs: Vec<Vec<f64>> = samples
        .iter()
        .map(|s| elements(&s["x"]).iter().map(|e| e.as_f64().unwrap()).collect())
        .collect();
    let ys: Vec<f64> = samples.iter().map(|s| s["y"].as_f64().unwrap()).collect();
    let means: Vec<f64> = (0..d).map(|j| mean(&xs.iter().map(|x| x[j]).collect::<Vec<_>>())).collect();
    let scales: Vec<f64> = means
        .iter()
        .enumerate()
        .map(|(j, m)| {
            let v = mean(&xs.iter().map(|x| { let d = x[j] - m; d * d }).collect::<Vec<_>>()).sqrt();
            if v == 0.0 { 1.0 } else { v }
        })
        .collect();
    let training_target_mean = mean(&ys);
    let normalized: Vec<Vec<f64>> = xs.iter().map(|x| x.iter().enumerate().map(|(j, x)| (x - means[j]) / scales[j]).collect()).collect();
    // Rounded feature means need not make floating-point standardized columns
    // zero-mean, particularly for adjacent representable near-constant prices.
    // Center the actual transformed data and restore its offset in the intercept.
    let standardized_means: Vec<f64> = (0..d).map(|j| mean(&normalized.iter().map(|z| z[j]).collect::<Vec<_>>())).collect();
    let mut matrix = vec![vec![0.0f64; d + 1]; d];
    for i in 0..n {
        let z: Vec<f64> = normalized[i].iter().enumerate().map(|(j, x)| x - standardized_means[j]).collect();
        for j in 0..d {
            matrix[j][d] += z[j] * (ys[i] - training_target_mean) / n as f64;
            for k in 0..d {
                matrix[j][k] += z[j] * z[k] / n as f64;
            }
        }
    }
    for j in 0..d {
        matrix[j][j] += alpha;
    }
    // Partial-pivot Gaussian elimination of tiny (5x5) positive regularized system.
    for j in 0..d {
        let mut pivot = j;
        for k in j + 1..d {
            if matrix[k][j].abs() > matrix[pivot][j].abs() { pivot = k; }
        }
        matrix.swap(j, pivot);
        if matrix[j][j].abs() < 1e-14 { return Err(Error::code("ILL_CONDITIONED_MODEL")); }
        let v = matrix[j][j];
        for k in j..=d {
            matrix[j][k] /= v;
        }
        for row in 0..d {
            if row != j {
                let f = matrix[row][j];
                for k in j..=d {
                    matrix[row][k] -= f * matrix[j][k];
                }
            }
        }
    }
    let weights: Vec<f64> = matrix.iter().map(|row| row[d]).collect();
    let intercept = training_target_mean - standardized_means.iter().enumerate().map(|(j, m)| m * weights[j]).sum::<f64>();
    let mut all = means.clone();
    all.extend(scales.iter().cloned());
    all.extend(standardized_means.iter().cloned());
    all.push(training_target_mean);
    all.push(intercept);
    all.extend(weights.iter().cloned());
    finite(&all)?;
    // {rowId, x, y} — undefined-valued keys drop out of JSON.stringify.
    let training_rows: Vec<V> = samples
        .iter()
        .map(|s| {
            let mut m = serde_json::Map::new();
            if let Some(v) = s.get("rowId") { m.insert("rowId".to_string(), v.clone()); }
            if let Some(v) = s.get("x") { m.insert("x".to_string(), v.clone()); }
            if let Some(v) = s.get("y") { m.insert("y".to_string(), v.clone()); }
            V::Object(m)
        })
        .collect();
    Ok(json!({
        "family": "ridge-forward-mid-change",
        "featureNames": FEATURE_NAMES,
        "alpha": alpha,
        "means": means,
        "scales": scales,
        "standardizedMeans": standardized_means,
        "trainingTargetMean": training_target_mean,
        "intercept": intercept,
        "weights": weights,
        "trainCount": n,
        "trainingRowsSha256": hash(canonical(&V::Array(training_rows))),
    }))
}

/// predict(model, x)
pub fn predict(model: &V, x: &V) -> R<f64> {
    // JS evaluates `x.length !== model.weights.length` left-to-right.
    let xl = js_read_length(x)?;
    let wl = js_read_length_opt(model.get("weights"))?;
    match (xl, wl) {
        (Some(l), Some(r)) if l == r => {}
        _ => return Err(Error::code("FEATURE_DIMENSION")),
    }
    // `model.intercept + reduce` is string concatenation when intercept is a
    // string — the result is never a finite number.
    let intercept = if model["intercept"].is_string() { f64::NAN } else { to_f64(&model["intercept"]) };
    let means = &model["means"];
    let scales = &model["scales"];
    let weights = &model["weights"];
    // x.reduce((s, v, j) => s + term, 0) — intercept is added to the
    // zero-initialized accumulator at the end, not accumulated onto.
    let mut s = 0.0f64;
    for (j, v) in elements(x).iter().enumerate() {
        s += ((to_f64(v) - to_f64(&means[j])) / to_f64(&scales[j])) * to_f64(&weights[j]);
    }
    let y = intercept + s;
    finite(&[y])?;
    Ok(y)
}

/// metrics(samples, model)
pub fn metrics(samples: &[V], model: &V) -> R<V> {
    if samples.is_empty() { return Err(Error::code("NO_EVALUATION_SAMPLES")); }
    if !finite_json(&model["trainingTargetMean"]) { return Err(Error::code("NONFINITE_MODEL")); }
    let training_target_mean = model["trainingTargetMean"].as_f64().unwrap();
    let mut abs_err = Vec::with_capacity(samples.len());
    let mut abs_y = Vec::with_capacity(samples.len());
    let mut abs_mean = Vec::with_capacity(samples.len());
    let mut sq_err = Vec::with_capacity(samples.len());
    let mut ys = Vec::with_capacity(samples.len());
    for s in samples {
        let p = predict(model, &s["x"])?;
        let y = to_f64(&s["y"]);
        abs_err.push((p - y).abs());
        abs_y.push(y.abs());
        abs_mean.push((training_target_mean - y).abs());
        let d = p - y;
        sq_err.push(d * d);
        ys.push(y);
    }
    Ok(json!({
        "count": samples.len(),
        "mae": mean(&abs_err),
        "zeroChangeMae": mean(&abs_y),
        "trainMeanMae": mean(&abs_mean),
        "rmse": mean(&sq_err).sqrt(),
        "meanTarget": mean(&ys),
        "units": "YES price change, range of book midprice [0,1]",
    }))
}

/// selectModel(train, validation, alphas) — minimum validation MAE; the first
/// (ascending) alpha wins an exact tie.
pub fn select_model(train: &[V], validation: &[V], alphas: &V) -> R<V> {
    let alphas = match alphas.as_array() {
        Some(a) => a,
        None => return Err(Error::code("alphas.map is not a function")),
    };
    let mut candidates: Vec<V> = Vec::with_capacity(alphas.len());
    for alpha in alphas {
        let model = fit_ridge(train, alpha.as_f64().unwrap_or(f64::NAN))?;
        let validation_metrics = metrics(validation, &model)?;
        candidates.push(json!({"alpha": alpha.clone(), "validation": validation_metrics, "model": model}));
    }
    let mut chosen = match candidates.first() {
        Some(c) => c,
        None => return Err(Error::code("Cannot read properties of undefined (reading 'model')")),
    };
    for candidate in candidates.iter().skip(1) {
        if to_f64(&candidate["validation"]["mae"]) < to_f64(&chosen["validation"]["mae"]) { chosen = candidate; }
    }
    let rest: Vec<V> = candidates
        .iter()
        .map(|c| {
            let mut m = c.as_object().unwrap().clone();
            m.shift_remove("model");
            V::Object(m)
        })
        .collect();
    Ok(json!({"model": chosen["model"].clone(), "candidates": V::Array(rest)}))
}

/// random(seed) — seeded 32-bit LCG (Numerical Recipes), `seed >>> 0`.
pub fn random(seed: u64) -> impl FnMut() -> f64 {
    let mut state = seed as u32;
    move || {
        state = 1664525u32.wrapping_mul(state).wrapping_add(1013904223);
        state as f64 / 4294967296.0
    }
}
fn session_key(v: &V) -> String {
    match v.as_str() {
        Some(s) => s.to_string(),
        None => js_stringify(v),
    }
}
/// uncertainty(samples, model, seed, replicates) — resample whole sessions,
/// preserving within-session serial/cross-market dependence. Descriptive only:
/// different sessions can still be dependent.
pub fn uncertainty(samples: &[V], model: &V, seed: u64, replicates: u64) -> R<V> {
    // JS Map: session clusters keep first-seen order.
    let mut sessions: indexmap::IndexMap<String, Vec<f64>> = indexmap::IndexMap::new();
    for s in samples {
        let y = to_f64(&s["y"]);
        let diff = (predict(model, &s["x"])? - y).abs() - y.abs();
        sessions.entry(session_key(s.get("sessionId").unwrap_or(&V::Null))).or_default().push(diff);
    }
    let clusters: Vec<&Vec<f64>> = sessions.values().collect();
    if clusters.len() < 5 {
        return Ok(json!({
            "method": "whole-session cluster bootstrap",
            "sessionCount": clusters.len(),
            "maeDifference95": V::Null,
            "reason": "Fewer than five sessions; no defensible uncertainty interval supplied",
        }));
    }
    let mut rng = random(seed);
    let mut draws: Vec<f64> = Vec::with_capacity(replicates as usize);
    let n_clusters = clusters.len();
    for _ in 0..replicates {
        let mut sum = 0.0f64;
        let mut n = 0.0f64;
        for _ in 0..n_clusters {
            let xs = clusters[(rng() * n_clusters as f64).floor() as usize];
            sum += xs.iter().sum::<f64>();
            n += xs.len() as f64;
        }
        draws.push(sum / n);
    }
    draws.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let (lo, hi) = if draws.is_empty() {
        (V::Null, V::Null)
    } else {
        let lo = draws[(replicates as f64 * 0.025).floor() as usize];
        let hi = draws[std::cmp::min(replicates as usize - 1, (replicates as f64 * 0.975).floor() as usize)];
        (json!(lo), json!(hi))
    };
    Ok(json!({
        "method": "whole-session cluster bootstrap, percentile, ridge MAE minus zero-change MAE",
        "sessionCount": clusters.len(),
        "seed": seed,
        "replicates": replicates,
        "maeDifference95": [lo, hi],
        "caveat": "Descriptive only; assumes sessions represent independent blocks. Overlapping labels and common regimes reduce effective sample size. Not a significance or profitability claim.",
    }))
}
