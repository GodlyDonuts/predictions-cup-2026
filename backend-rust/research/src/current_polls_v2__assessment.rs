// research/current-polls-v2/assessment.mjs — recency-weighted primary poll
// margin with scenario valuations under the fitted single-poll error family.
use crate::js::{date_parse, finite, js_max, js_min, js_number, num, require_that};
use crate::poll_calibration_v1__model::probability;
use serde_json::{json, Value as V};

pub fn assess_primary_polls(args: &V) -> Result<V, String> {
    let polls = args["polls"].as_array().cloned().unwrap_or_default();
    let models = args["models"].as_array().cloned().unwrap_or_default();
    let definition = &args["definition"];
    let at_ms = date_parse(&crate::js::js_string(
        args.get("at").unwrap_or(&V::Null),
    ));
    require_that(
        !polls.is_empty() && !models.is_empty() && at_ms.is_finite(),
        "assert(polls.length&&models.length&&Number.isFinite(Date.parse(at)))",
    )?;
    let mut seen = std::collections::HashSet::new();
    for p in &polls {
        let pollster = crate::js::map_key(p.get("pollster"));
        require_that(
            !seen.contains(&pollster),
            "assert(!seen.has(p.pollster))",
        )?;
        seen.insert(pollster);
        require_that(
            ["registered_voters", "likely_voters"]
                .contains(&p.get("population").and_then(V::as_str).unwrap_or(""))
                && [
                    p.get("democratic"),
                    p.get("republican"),
                    p.get("unallocated"),
                ]
                .iter()
                .all(|v| finite(*v) && js_number(*v) >= 0.0 && js_number(*v) <= 100.0)
                && js_number(p.get("democratic")) + js_number(p.get("republican")) <= 100.0
                && date_parse(&field_s(p, "fieldEndAt")) <= at_ms,
            "assert(['registered_voters','likely_voters'].includes(p.population)&&[p.democratic,p.republican,p.unallocated].every(v=>Number.isFinite(v)&&v>=0&&v<=100)&&p.democratic+p.republican<=100&&Date.parse(p.fieldEndAt)<=Date.parse(at))", 
        )?;
    }
    let half_life = js_number(definition.get("recencyHalfLifeDays"));
    let weights: Vec<f64> = polls
        .iter()
        .map(|p| {
            2f64.powf(-((at_ms - date_parse(&field_s(p, "fieldEndAt"))) / 86_400_000.0) / half_life)
        })
        .collect();
    let sum: f64 = weights.iter().sum();
    let w: Vec<f64> = weights.iter().map(|v| v / sum).collect();
    let margin: f64 = polls
        .iter()
        .zip(&w)
        .map(|(p, wi)| {
            (js_number(p.get("democratic")) - js_number(p.get("republican"))) * wi
        })
        .sum();
    let rv_weight: f64 = polls
        .iter()
        .zip(&w)
        .map(|(p, wi)| {
            if p.get("population").and_then(V::as_str) == Some("registered_voters") {
                *wi
            } else {
                0.0
            }
        })
        .sum();
    let unallocated: f64 = polls
        .iter()
        .zip(&w)
        .map(|(p, wi)| js_number(p.get("unallocated")) * wi)
        .sum();
    let haircut = js_number(definition.get("administrativeValuationHaircut"));
    let mut scenarios: Vec<V> = Vec::new();
    for model in &models {
        for rv_shift in list(definition, "rvTurnoutMarginShiftPoints") {
            for rounding_shift in list(definition, "roundingMarginShiftPoints") {
                let m = margin + rv_weight * rv_shift + rounding_shift;
                scenarios.push(json!({
                    "omittedCycle": model.get("omittedCycle").cloned().unwrap_or(V::Null),
                    "rvShift": num(rv_shift),
                    "roundingShift": num(rounding_shift),
                    "marginPoints": num(m),
                    "valuation": num(js_max(0.0, probability(model, m)? - haircut)),
                }));
            }
        }
    }
    let estimates: Vec<f64> = scenarios
        .iter()
        .map(|s| js_number(s.get("valuation")))
        .collect();
    let lower = estimates.iter().cloned().fold(f64::INFINITY, js_min);
    let upper = estimates.iter().cloned().fold(f64::NEG_INFINITY, js_max);
    Ok(json!({
        "marginPoints": num(margin),
        "pollWeights": polls.iter().zip(&w).map(|(p, wi)| json!({
            "pollster": p["pollster"],
            "population": p["population"],
            "fieldEndAt": p["fieldEndAt"],
            "weight": num(*wi),
        })).collect::<Vec<V>>(),
        "rvWeight": num(rv_weight),
        "unallocatedPoints": num(unallocated),
        "ordinaryRange": [num(lower), num(upper)],
        "scenarios": scenarios,
        "centralEstimate": num(js_max(0.0, probability(&models[0], margin)? - haircut)),
        "severeOpponentAllocationStress": num(js_max(
            0.0,
            probability(&models[0], margin - unallocated - 3.0 * rv_weight - 1.0)? - haircut,
        )),
        "estimateMeaning": "Settlement-value research under historical error and declared sensitivity; not calibrated population transport or a short-term return forecast.",
        "executionAuthority": false,
        "profitabilityEstablished": false,
    }))
}

fn list(definition: &V, key: &str) -> Vec<f64> {
    definition[key]
        .as_array()
        .map(|a| a.iter().map(|x| js_number(Some(x))).collect())
        .unwrap_or_default()
}
fn field_s(v: &V, key: &str) -> String {
    crate::js::js_string(v.get(key).unwrap_or(&V::Null))
}
