// live-paper/quote-outcome-forecast.mjs — two-stage quote outcome forecast:
// arrival model then conditional net value. Research selector only.
use crate::realtime_microprice_dataset::microprice_features_spec;
use baal_service_runtime::json_text::js_stringify;
use serde_json::{json, Value as V};

fn need(v: bool, c: &str) -> Result<(), String> { if !v { return Err(format!("QUOTE_FORECAST_{c}")); } Ok(()) }

fn predict(model: &V, x: &[V], outputs: usize) -> Result<Vec<f64>, String> {
    let n = x.len();
    let finite = |v: &V| v.as_f64().is_some_and(|x| x.is_finite());
    need(model["normalizedClip"] == json!(12)
        && model["means"].as_array().map_or(false, |a| a.len() == n)
        && model["scales"].as_array().map_or(false, |a| a.len() == n)
        && model["coefficients"].as_array().map_or(false, |a| a.len() == n + 1)
        && model["means"].as_array().unwrap().iter().all(finite)
        && model["scales"].as_array().unwrap().iter().all(|v| finite(v) && v.as_f64().unwrap() > 0.0)
        && model["coefficients"].as_array().unwrap().iter()
            .all(|r| r.as_array().map_or(false, |r| r.len() == outputs && r.iter().all(finite))), "MODEL")?;
    let means = model["means"].as_array().unwrap();
    let scales = model["scales"].as_array().unwrap();
    let coefs = model["coefficients"].as_array().unwrap();
    let mut z = vec![1.0f64];
    for (i, v) in x.iter().enumerate() {
        z.push(((v.as_f64().unwrap_or(f64::NAN) - means[i].as_f64().unwrap_or(f64::NAN))
            / scales[i].as_f64().unwrap_or(f64::NAN)).clamp(-12.0, 12.0));
    }
    Ok((0..outputs).map(|side| z.iter().enumerate()
        .map(|(i, a)| a * coefs[i][side].as_f64().unwrap_or(f64::NAN)).sum()).collect())
}

/// quoteOutcomeForecast(report, bank, x)
pub fn quote_outcome_forecast(report: &V, bank: &V, x: &V) -> Result<V, String> {
    need(report["mode"] == "quote_fill_conditional_selector_research"
        && js_stringify(&report["features"]) == js_stringify(&microprice_features_spec())
        && bank["status"] == "research_selector_frozen"
        && x.as_array().map_or(false, |a| a.len() == 20 && a.iter().all(|v| v.as_f64().is_some_and(|n| n.is_finite()))), "BINDING")?;
    need(bank["policy"]["feeTicks"].as_f64() == Some(0.0)
        && bank["policy"]["latencyMs"].as_f64() == Some(3000.0)
        && bank["selector"]["minimumExpectedNetTicks"].as_f64().map_or(false, |v| v.is_finite() && v >= 0.0)
        && bank["selector"]["minimumAnyFillProbability"].as_f64().map_or(false, |v| v.is_finite() && v >= 0.0 && v <= 1.0), "SELECTOR")?;
    let xa = x.as_array().unwrap();
    let arrival = predict(&bank["model"]["arrival"], xa, 2)?;
    let any_fill = arrival[0].clamp(0.0, 1.0);
    let paired = arrival[1].clamp(0.0, any_fill);
    let conditional_net_ticks = predict(&bank["model"]["conditionalValue"], xa, 1)?[0].clamp(-200.0, 199.0);
    let expected_net_ticks = any_fill * conditional_net_ticks;
    Ok(json!({
        "anyFillProbability": any_fill,
        "pairedProbability": paired,
        "conditionalNetTicks": conditional_net_ticks,
        "expectedNetTicks": expected_net_ticks,
        "selected": expected_net_ticks >= bank["selector"]["minimumExpectedNetTicks"].as_f64().unwrap()
            && any_fill >= bank["selector"]["minimumAnyFillProbability"].as_f64().unwrap(),
        "executionAuthority": false,
        "profitabilityEstablished": false,
    }))
}
