// research/current-polls-v2/party-assessment.mjs — party-keyed sensitivity
// scenarios; judgments, not calibrated confidence intervals.
use crate::js::{date_parse, finite, js_max, js_min, js_number, js_truthy, num, require_that};
use crate::poll_calibration_v1__model::probability;
use serde_json::{json, Value as V};

// assessPartyPolls({party,polls,models,at,haircut=0.02,halfLifeDays=21,
//                  transportShifts=[-3,0,3],roundingShifts=[-1,0,1],houseShifts=[0,3]})
#[allow(clippy::too_many_arguments)]
pub fn assess_party_polls(
    party: &str,
    polls: &[V],
    models: &[V],
    at: &str,
    haircut: f64,
    half_life_days: f64,
    transport_shifts: &[f64],
    rounding_shifts: &[f64],
    house_shifts: &[f64],
) -> Result<V, String> {
    let at_ms = date_parse(at);
    require_that(
        ["DEM", "REP"].contains(&party) && !polls.is_empty() && !models.is_empty() && at_ms.is_finite(),
        "assert(['DEM','REP'].includes(party)&&polls.length&&models.length&&Number.isFinite(Date.parse(at)))",
    )?;
    let mut seen = std::collections::HashSet::new();
    for p in polls {
        let pollster = crate::js::map_key(p.get("pollster"));
        require_that(!seen.contains(&pollster), "assert(!seen.has(p.pollster))")?;
        seen.insert(pollster);
        require_that(
            [
                p.get("democratic"),
                p.get("republican"),
                p.get("undecided"),
            ]
            .iter()
            .all(|v| finite(*v) && js_number(*v) >= 0.0 && js_number(*v) <= 100.0),
            "assert(shares finite)",
        )?;
        require_that(
            js_number(p.get("democratic")) + js_number(p.get("republican")) + js_number(p.get("undecided"))
                <= 100.00001,
            "assert(shares<=100)",
        )?;
        require_that(
            date_parse(&field_s(p, "fieldEndAt")) <= at_ms,
            "assert(fieldEndAt<=at)",
        )?;
    }
    let raw: Vec<f64> = polls
        .iter()
        .map(|p| {
            2f64.powf(-(at_ms - date_parse(&field_s(p, "fieldEndAt"))) / 86_400_000.0 / half_life_days)
        })
        .collect();
    let total: f64 = raw.iter().sum();
    let weights: Vec<f64> = raw.iter().map(|x| x / total).collect();
    let margin: f64 = polls
        .iter()
        .zip(&weights)
        .map(|(p, wi)| (js_number(p.get("democratic")) - js_number(p.get("republican"))) * wi)
        .sum();
    let undecided: f64 = polls
        .iter()
        .zip(&weights)
        .map(|(p, wi)| js_number(p.get("undecided")) * wi)
        .sum();
    let house_weight: f64 = polls
        .iter()
        .zip(&weights)
        .map(|(p, wi)| {
            if js_truthy(p.get("possibleRepublicanHouseBias")) {
                *wi
            } else {
                0.0
            }
        })
        .sum();
    let value = |model: &V, m: f64| -> Result<f64, String> {
        let p = probability(model, m)?;
        Ok(js_max(
            0.0,
            (if party == "DEM" { p } else { 1.0 - p }) - haircut,
        ))
    };
    let mut scenarios: Vec<V> = Vec::new();
    for model in models {
        for transport in transport_shifts {
            for rounding in rounding_shifts {
                for house in house_shifts {
                    let adjusted = margin + transport + rounding + house * house_weight;
                    scenarios.push(json!({
                        "omittedCycle": model.get("omittedCycle").cloned().unwrap_or(V::Null),
                        "transportShift": num(*transport),
                        "roundingShift": num(*rounding),
                        "possibleRepublicanHouseShift": num(*house),
                        "margin": num(adjusted),
                        "value": num(value(model, adjusted)?),
                    }));
                }
            }
        }
    }
    let stress_margin = if party == "REP" {
        margin + undecided + 3.0 + 1.0 + 3.0 * house_weight
    } else {
        margin - undecided - 3.0 - 1.0
    };
    Ok(json!({
        "party": party,
        "at": at,
        "marginPoints": num(margin),
        "pollWeights": polls.iter().zip(&weights).map(|(p, wi)| json!({
            "pollster": p["pollster"],
            "weight": num(*wi),
            "fieldEndAt": p["fieldEndAt"],
        })).collect::<Vec<V>>(),
        "centralEstimate": num(value(&models[0], margin)?),
        "ordinaryRange": [
            num(scenarios.iter().map(|s| js_number(s.get("value"))).fold(f64::INFINITY, js_min)),
            num(scenarios.iter().map(|s| js_number(s.get("value"))).fold(f64::NEG_INFINITY, js_max)),
        ],
        "severeOpponentUndecidedAllocation": num(value(&models[0], stress_margin)?),
        "scenarios": scenarios,
        "distribution": "fixed_t4",
        "estimateMeaning": "Conditional settlement-value research, not a forecast of near-term bid returns or a confidence interval",
        "executionAuthority": false,
        "profitabilityEstablished": false,
    }))
}

fn field_s(v: &V, key: &str) -> String {
    crate::js::js_string(v.get(key).unwrap_or(&V::Null))
}
