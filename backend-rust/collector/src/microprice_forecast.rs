// live-paper/microprice-forecast.mjs — frozen feature-set ridge forecast.
// Model output is a research forecast, never an execution permission.
use crate::realtime_microprice_dataset::microprice_features_spec;
use baal_service_runtime::json_text::js_stringify;
use serde_json::{json, Value as V};

/// micropriceForecast(bank, x)
pub fn microprice_forecast(bank: &V, x: &V) -> Result<V, String> {
    let m = &bank["model"];
    let n = MICROPRICE_FEATURE_COUNT;
    let finite = |v: &V| v.as_f64().is_some_and(|x| x.is_finite());
    let valid = bank["mode"] == "realtime_microprice_forecast_research"
        && js_stringify(&bank["features"]) == js_stringify(&microprice_features_spec())
        && x.as_array().is_some_and(|a| a.len() == n && a.iter().all(finite))
        && m["normalizedClip"] == json!(12)
        && m["means"].as_array().is_some_and(|a| a.len() == n && a.iter().all(finite))
        && m["scales"].as_array().is_some_and(|a| a.len() == n && a.iter().all(|v| finite(v) && v.as_f64().unwrap() > 0.0))
        && m["coefficients"].as_array().is_some_and(|a| a.len() == n + 1
            && a.iter().all(|r| r.as_array().is_some_and(|r| r.len() == 2 && r.iter().all(finite))))
        && bank["lowerForecastErrorTicks"].as_array().is_some_and(|a| a.len() == 2
            && a.iter().all(|v| finite(v) && v.as_f64().unwrap() >= 0.0));
    if !valid { return Err("MICROPRICE_FORECAST_BINDING".into()); }
    let xa = x.as_array().unwrap();
    let means = m["means"].as_array().unwrap();
    let scales = m["scales"].as_array().unwrap();
    let coefs = m["coefficients"].as_array().unwrap();
    let mut z = vec![1.0f64];
    for (i, v) in xa.iter().enumerate() {
        z.push(((v.as_f64().unwrap() - means[i].as_f64().unwrap()) / scales[i].as_f64().unwrap())
            .clamp(-12.0, 12.0));
    }
    let predictions: Vec<f64> = (0..2).map(|side| z.iter().enumerate()
        .map(|(i, a)| a * coefs[i][side].as_f64().unwrap()).sum()).collect();
    let lower: Vec<f64> = predictions.iter().enumerate()
        .map(|(i, v)| v - bank["lowerForecastErrorTicks"][i].as_f64().unwrap()).collect();
    Ok(json!({"bidChangeTicks":predictions,"lowerBidChangeTicks":lower,
        "executionAuthority":false,"profitabilityEstablished":false}))
}
const MICROPRICE_FEATURE_COUNT: usize = 20;
