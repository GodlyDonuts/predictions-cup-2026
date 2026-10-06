// live-paper/execution-metrics.mjs
use serde_json::{json, Value as V};
pub const PAPER_EDGE_MARGINS: [f64; 3] = [0.005, 0.01, 0.015];
/// netEdgeDiagnostics(rows, predictions, margins)
pub fn net_edge_diagnostics(rows: &[V], predictions: &[f64], margins: &[f64]) -> Result<V, String> {
    let ok = predictions.iter().all(|x| x.is_finite())
        && rows.iter().all(|r| r.get("y").and_then(V::as_f64).is_some())
        && !margins.is_empty()
        && { let mut u = margins.to_vec(); u.sort_by(|a, b| a.partial_cmp(b).unwrap()); u.dedup(); u.len() == margins.len() }
        && margins.iter().all(|x| x.is_finite() && *x > 0.0);
    if !ok { return Err("Invalid net-edge diagnostics".into()); }
    let margins_out: Vec<V> = margins.iter().map(|&margin| {
        let indices: Vec<usize> = predictions.iter().enumerate().filter(|(_, p)| **p >= margin).map(|(i, _)| i).collect();
        let count = indices.len();
        let (mut observed, mut predicted, mut wins, mut worst) = (0.0, 0.0, 0usize, f64::INFINITY);
        for i in &indices {
            let y = rows[*i].get("y").and_then(V::as_f64).unwrap();
            observed += y; predicted += predictions[*i];
            if y > 0.0 { wins += 1; }
            worst = worst.min(y);
        }
        let n = count as f64;
        json!({"margin": margin, "decisions": count,
            "meanPredictedNetPerShare": if count > 0 { json!(predicted / n) } else { V::Null },
            "meanObservedNetPerShare": if count > 0 { json!(observed / n) } else { V::Null },
            "positiveOutcomeFraction": if count > 0 { json!(wins as f64 / n) } else { V::Null },
            "worstNetPerShare": if count > 0 { json!(worst) } else { V::Null }})
    }).collect();
    Ok(json!({
        "version": "conditional-development-net-edge-v1",
        "completedLabelCount": rows.len(),
        "margins": margins_out,
        "portfolioPnl": V::Null,
        "profitabilityEstablished": false,
        "limitation": "Overlapping flat-wallet probes conditional on completed labels; excludes censored/no-fill decisions. Development data are reused for model selection, not untouched forward validation. Fees are already included in the target and are not subtracted again.",
    }))
}
