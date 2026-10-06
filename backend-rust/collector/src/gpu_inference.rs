// live-paper/gpu-inference.mjs — frozen MLP forward pass.
use serde_json::Value as V;
fn floats(v: &V, n: usize) -> Option<Vec<f64>> {
    let a = v.as_array()?;
    if a.len() != n { return None; }
    a.iter().map(|x| x.as_f64()).collect()
}
pub fn predict_mlp(m: &V, x: &[f64]) -> Result<f64, String> {
    let err = "Invalid frozen GPU model dimensions";
    let width = m.get("width").and_then(V::as_i64).filter(|w| (1..=32).contains(w)).ok_or(err)? as usize;
    let hidden = m.get("hidden").and_then(V::as_i64).filter(|h| (1..=64).contains(h)).ok_or(err)? as usize;
    if x.len() < width { return Err(err.into()); }
    let means = floats(&m["means"], width).ok_or(err)?;
    let scales = floats(&m["scales"], width).ok_or(err)?;
    let hidden_bias = floats(&m["hiddenBias"], hidden).ok_or(err)?;
    let output_weights = floats(&m["outputWeights"], hidden).ok_or(err)?;
    let input_w = m.get("inputWeights").and_then(V::as_array).filter(|a| a.len() == width).ok_or(err)?;
    let input_weights: Vec<Vec<f64>> = input_w.iter().map(|r| floats(r, hidden)).collect::<Option<_>>().ok_or(err)?;
    let scalar = |k: &str| m.get(k).and_then(V::as_f64).ok_or("Invalid frozen GPU model values");
    let output_bias = scalar("outputBias")?;
    let target_mean = scalar("targetMean")?;
    let target_scale = scalar("targetScale")?;
    if scales.iter().any(|v| *v <= 0.0) || target_scale <= 0.0 { return Err("Invalid frozen GPU model values".into()); }
    let mut value = output_bias;
    for h in 0..hidden {
        let mut z = hidden_bias[h];
        for j in 0..width { z += (x[j] - means[j]) / scales[j] * input_weights[j][h]; }
        value += z.max(0.0) * output_weights[h];
    }
    let out = target_mean + target_scale * value;
    if !out.is_finite() { return Err("Nonfinite GPU forecast".into()); }
    Ok(out)
}
