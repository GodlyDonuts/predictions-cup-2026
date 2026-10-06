// research/current-polls-v1/assessment.mjs — separate party valuations; a
// discounted Republican range is not complemented into a Democratic forecast.
use crate::election_baseline_v2__model::probability_from_margin;
use crate::js::{date_parse, finite, iso, js_max, js_min, js_number, js_number_str, num, require_that};
use serde_json::{json, Value as V};

const DAY: f64 = 86_400_000.0;

pub fn assess_party_poll(args: &V) -> Result<V, String> {
    let poll = &args["poll"];
    let prior = &args["prior"];
    let definition = &args["definition"];
    let party = field_s(args, "party");
    let assessment_at = field_s(args, "assessmentAt");
    let election_at = field_s(args, "electionAt");
    require_that(
        ["republican", "democratic"].contains(&party.as_str()),
        "unknown_party",
    )?;
    let now = date_parse(&assessment_at);
    let end = date_parse(&field_s(poll, "fieldEndLatestAt"));
    let observed = date_parse(&field_s(poll, "observedAt"));
    let election = date_parse(&election_at);
    require_that(
        [now, end, observed, election].iter().all(|x| x.is_finite())
            && end <= observed
            && observed <= now
            && now < election,
        "noncausal_clock",
    )?;
    require_that(
        now - end <= js_number(definition.get("maximumPollFieldAgeDays")) * DAY,
        "poll_field_too_old",
    )?;
    require_that(
        poll.get("population").and_then(V::as_str) == Some("likely_voters")
            && crate::js::safe_int(poll.get("sampleSize")).is_some_and(|n| n >= 30),
        "population_or_sample_unverified",
    )?;
    let r = js_number(poll.get("republican"));
    let d = js_number(poll.get("democratic"));
    let o = js_number(poll.get("other"));
    let remaining = js_number(poll.get("unallocated"));
    require_that(
        [r, d, o, remaining]
            .iter()
            .all(|x| x.is_finite() && *x >= 0.0 && *x <= 1.0)
            && (r + d + o + remaining - 1.0).abs() < 1e-9,
        "invalid_shares",
    )?;
    let half_width = js_number(definition.get("reportedShareRoundingHalfWidth"));
    require_that(
        poll["ballotVerified"] == V::Bool(true)
            && o + remaining + half_width < js_min(r, d) - half_width,
        "third_party_comparator_unverified",
    )?;
    require_that(
        crate::js::safe_int(prior.get("trainedThroughYear")).is_some()
            && js_number(prior.get("trainedThroughYear")) < js_number_str(&election_at[..4.min(election_at.len())])
            && finite(prior.get("structuralMargin"))
            && finite(prior.get("residualRms"))
            && js_number(prior.get("residualRms")) > 0.0,
        "invalid_prior",
    )?;
    require_that(
        js_number(definition.get("priorWeight")) > 0.0
            && js_number(definition.get("priorWeight")) < 1.0
            && finite(definition.get("nationalTransportSd"))
            && js_number(definition.get("nationalTransportSd")) > 0.0
            && js_number(definition.get("maximumAssessmentAgeMs")) > 0.0
            && js_number(definition.get("maximumAssessmentAgeMs")) <= 21_600_000.0,
        "invalid_definition",
    )?;
    let finite_list = |key: &str| -> bool {
        definition[key].as_array().is_some_and(|a| {
            !a.is_empty() && a.iter().all(|x| finite(Some(x)))
        })
    };
    require_that(
        finite_list("nationalMarginScenarios")
            && finite_list("designEffects")
            && finite_list("undecidedRepublicanFractions")
            && finite_list("pollMarginBiasScenarios")
            && definition["designEffects"]
                .as_array()
                .is_some_and(|a| a.iter().all(|x| js_number(Some(x)) >= 1.0))
            && definition["undecidedRepublicanFractions"]
                .as_array()
                .is_some_and(|a| a.iter().all(|x| js_number(Some(x)) >= 0.0 && js_number(Some(x)) <= 1.0)),
        "invalid_scenarios",
    )?;
    let pos = |v: Option<&V>| finite(v) && js_number(v) > 0.0;
    require_that(
        pos(definition.get("systematicMarginSd"))
            && pos(definition.get("dailyMarginInnovationSd"))
            && pos(definition["stress"].get("systematicMarginSd"))
            && pos(definition["stress"].get("dailyMarginInnovationSd"))
            && js_number(definition.get("reportedShareRoundingHalfWidth")) >= 0.0
            && js_number(definition.get("reportedShareRoundingHalfWidth")) < 0.05
            && js_number(definition.get("administrativeProbabilityHaircut")) >= 0.0
            && js_number(definition.get("administrativeProbabilityHaircut")) < 1.0,
        "invalid_error_assumptions",
    )?;
    let sign = if party == "republican" { 1.0 } else { -1.0 };
    let target = if party == "republican" { r } else { d };
    let opponent = if party == "republican" { d } else { r };
    let sum = target + opponent;
    let gr = 2.0 * opponent / (sum * sum);
    let gd = -2.0 * target / (sum * sum);
    let sampling_variance = (gr * gr * target * (1.0 - target)
        + gd * gd * opponent * (1.0 - opponent)
        - 2.0 * gr * gd * target * opponent)
        / js_number(poll.get("sampleSize"));
    let prior_sd =
        js_number(prior.get("residualRms")).hypot(js_number(definition.get("nationalTransportSd")));
    let days = (election - end) / DAY;
    let prior_weight = js_number(definition.get("priorWeight"));
    let structural = js_number(prior.get("structuralMargin"));
    let haircut = js_number(definition.get("administrativeProbabilityHaircut"));
    let family = |systematic: f64, daily: f64| -> Result<V, String> {
        let mut variants: Vec<V> = Vec::new();
        for national in list(definition, "nationalMarginScenarios") {
            for deff in list(definition, "designEffects") {
                for allocation in list(definition, "undecidedRepublicanFractions") {
                    for bias in list(definition, "pollMarginBiasScenarios") {
                        for rounding in [-1.0_f64, 1.0] {
                            let target_allocation = if party == "republican" {
                                allocation
                            } else {
                                1.0 - allocation
                            };
                            let t = target + rounding * half_width + remaining * target_allocation;
                            let q =
                                opponent - rounding * half_width + remaining * (1.0 - target_allocation);
                            let strongest = js_max(q, o + remaining * (1.0 - target_allocation));
                            let poll_margin = (t - strongest) / (t + strongest) + sign * bias;
                            let mean = prior_weight * sign * (structural + national)
                                + (1.0 - prior_weight) * poll_margin;
                            let poll_sd = (sampling_variance * deff).sqrt().hypot(systematic);
                            let sd = (prior_weight * prior_sd + (1.0 - prior_weight) * poll_sd)
                                .hypot(daily * days.sqrt());
                            let raw = probability_from_margin(mean, sd)?;
                            let valuation = js_max(0.0, raw - haircut);
                            variants.push(json!({
                                "national": num(national),
                                "designEffect": num(deff),
                                "undecidedRepublicanFraction": num(allocation),
                                "bias": num(bias),
                                "rounding": num(rounding),
                                "mean": num(mean),
                                "standardDeviation": num(sd),
                                "modelWinProbability": num(raw),
                                "discountedValuation": num(valuation),
                            }));
                        }
                    }
                }
            }
        }
        let vals = |k: &str| -> Vec<f64> {
            variants.iter().map(|v| js_number(v.get(k))).collect()
        };
        let valuations = vals("discountedValuation");
        let wins = vals("modelWinProbability");
        Ok(json!({
            "lower": num(valuations.iter().cloned().fold(f64::INFINITY, js_min)),
            "upper": num(valuations.iter().cloned().fold(f64::NEG_INFINITY, js_max)),
            "modelWinRange": [
                num(wins.iter().cloned().fold(f64::INFINITY, js_min)),
                num(wins.iter().cloned().fold(f64::NEG_INFINITY, js_max)),
            ],
            "variants": variants,
        }))
    };
    Ok(json!({
        "schemaVersion": 1,
        "party": party,
        "method": "subjective_range",
        "assessedAt": assessment_at,
        "expiresAt": iso(now + js_number(definition.get("maximumAssessmentAgeMs"))),
        "componentClocks": {
            "pollObservedAt": poll["observedAt"],
            "pollFieldEndLatestAt": poll["fieldEndLatestAt"],
            "pollFieldAgeDays": num((now - end) / DAY),
            "daysFromFieldEndToElection": num(days),
            "electionAt": election_at,
            "priorTrainedThroughYear": prior["trainedThroughYear"],
        },
        "sampleSize": poll["sampleSize"],
        "samplingVariance": num(sampling_variance),
        "ordinary": family(
            js_number(definition.get("systematicMarginSd")),
            js_number(definition.get("dailyMarginInnovationSd")),
        )?,
        "stress": family(
            js_number(definition["stress"].get("systematicMarginSd")),
            js_number(definition["stress"].get("dailyMarginInnovationSd")),
        )?,
        "fees": "0.000000",
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
