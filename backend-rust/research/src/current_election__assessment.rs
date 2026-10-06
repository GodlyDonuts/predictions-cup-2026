// research/current-election/assessment.mjs — subjective-range current election
// assessment plus a price decision diagnostic (exact micros arithmetic).
use crate::election_baseline_v2__model::probability_from_margin;
use crate::js::{date_parse, finite, iso, js_max, js_min, js_number, js_number_str, num, require_that};
use baal_execution::core::{decimal_micros, format_micros};
use serde_json::{json, Value as V};

const DAY: f64 = 86_400_000.0;

fn share(x: Option<&V>) -> bool {
    let f = finite(x);
    f && js_number(x) >= 0.0 && js_number(x) <= 1.0
}

// clock(x): typeof string && new Date(Date.parse(x)).toISOString() === x.
// (JS would RangeError on an unparseable string; here it simply fails the
// round-trip and lands on the same 'invalid_clock' assert.)
fn clock(x: Option<&V>) -> bool {
    match x {
        Some(V::String(s)) => baal_service_runtime::contract::parse_iso_ms(s)
            .is_some_and(|ms| baal_service_runtime::contract::ms_to_iso(ms) == *s),
        _ => false,
    }
}

fn signed_micros(x: i128) -> Result<String, String> {
    if x < 0 {
        fmt_micros(-x).map(|s| format!("-{s}"))
    } else {
        fmt_micros(x)
    }
}
fn fmt_micros(x: i128) -> Result<String, String> {
    require_that(x >= 0, "INVALID_MICROS")?;
    Ok(format_micros(x))
}

// A current assessment clock never substitutes for the underlying field dates.
pub fn assess_election(args: &V) -> Result<V, String> {
    let poll = &args["poll"];
    let prior = &args["prior"];
    let p = &args["definition"];
    let assessment_at = field_s(args, "assessmentAt");
    let election_at = field_s(args, "electionAt");
    require_that(
        clock(args.get("assessmentAt"))
            && clock(args.get("electionAt"))
            && clock(poll.get("observedAt"))
            && clock(poll.get("fieldEndLatestAt")),
        "invalid_clock",
    )?;
    let now = date_parse(&assessment_at);
    let end = date_parse(field_s(poll, "fieldEndLatestAt").as_str());
    let election = date_parse(&election_at);
    require_that(
        end <= date_parse(field_s(poll, "observedAt").as_str())
            && date_parse(field_s(poll, "observedAt").as_str()) <= now
            && now < election,
        "noncausal_clock",
    )?;
    require_that(
        now - end <= js_number(p.get("maximumPollFieldAgeDays")) * DAY,
        "poll_field_too_old",
    )?;
    require_that(
        poll.get("population").and_then(V::as_str) == Some("likely_voters")
            && crate::js::safe_int(poll.get("sampleSize")).is_some_and(|n| n >= 30),
        "wrong_population_or_sample",
    )?;
    let shares = [
        js_number(poll.get("republican")),
        js_number(poll.get("democratic")),
        js_number(poll.get("independent")),
        js_number(poll.get("undecided")),
    ];
    require_that(
        [
            poll.get("republican"),
            poll.get("democratic"),
            poll.get("independent"),
            poll.get("undecided"),
        ]
        .iter()
        .all(|x| share(*x))
            && (shares.iter().sum::<f64>() - 1.0).abs() <= 0.02,
        "invalid_shares",
    )?;
    require_that(
        poll["ballotVerified"] == V::Bool(true)
            && poll.get("largestNamedOpponent").and_then(V::as_str) == Some("democratic")
            && js_number(poll.get("republican"))
                > js_number(poll.get("independent")) + js_number(poll.get("undecided")),
        "unverified_plurality_scope",
    )?;
    require_that(
        finite(prior.get("structuralMargin"))
            && finite(prior.get("residualRms"))
            && js_number(prior.get("residualRms")) > 0.0
            && js_number(prior.get("trainedThroughYear")) < js_number_str(&election_at[..4.min(election_at.len())]),
        "invalid_prior",
    )?;
    require_that(
        p.get("schemaVersion").and_then(V::as_f64) == Some(1.0)
            && finite(p.get("priorWeight"))
            && js_number(p.get("priorWeight")) > 0.0
            && js_number(p.get("priorWeight")) < 1.0
            && js_number(p.get("nationalTransportSd")) > 0.0
            && js_number(p.get("maximumAssessmentAgeMs")) > 0.0
            && js_number(p.get("maximumAssessmentAgeMs")) <= 21_600_000.0,
        "invalid_definition",
    )?;
    let scenario_list = |key: &str, pred: &dyn Fn(&V) -> bool| -> bool {
        p[key].as_array().is_some_and(|a| a.iter().all(|x| pred(x)))
    };
    require_that(
        scenario_list("nationalMarginScenarios", &|x| finite(Some(x)))
            && scenario_list("designEffects", &|x| finite(Some(x)) && js_number(Some(x)) >= 1.0)
            && scenario_list("undecidedRepublicanFractions", &|x| share(Some(x)))
            && scenario_list("pollMarginBiasScenarios", &|x| finite(Some(x))),
        "invalid_scenarios",
    )?;
    require_that(
        js_number(p.get("systematicMarginSd")) > 0.0
            && js_number(p.get("dailyMarginInnovationSd")) > 0.0
            && share(p.get("administrativeProbabilityHaircut"))
            && share(p.get("reportedShareRoundingHalfWidth")),
        "invalid_error_assumptions",
    )?;
    // For Z=1(R), -1(D), 0(other), Var(mean Z)=(R+D-(R-D)^2)/n.
    // Delta-method variance for (R-D)/(R+D), retaining negative multinomial covariance.
    let r = js_number(poll.get("republican"));
    let d = js_number(poll.get("democratic"));
    let s = r + d;
    let grad_r = 2.0 * d / (s * s);
    let grad_d = -2.0 * r / (s * s);
    let sample = js_number(poll.get("sampleSize"));
    let sampling_variance = (grad_r * grad_r * r * (1.0 - r) + grad_d * grad_d * d * (1.0 - d)
        - 2.0 * grad_r * grad_d * r * d)
        / sample;
    let prior_sd = js_number(prior.get("residualRms")).hypot(js_number(p.get("nationalTransportSd")));
    let days_to_election = (election - end) / DAY;
    let prior_weight = js_number(p.get("priorWeight"));
    let prior_margin = js_number(prior.get("structuralMargin"));
    let undecided = js_number(poll.get("undecided"));
    let independent = js_number(poll.get("independent"));
    let rounding_half = js_number(p.get("reportedShareRoundingHalfWidth"));
    let haircut = js_number(p.get("administrativeProbabilityHaircut"));
    let family = |systematic_sd: f64, daily_sd: f64| -> Result<V, String> {
        require_that(
            systematic_sd.is_finite() && systematic_sd > 0.0 && daily_sd.is_finite() && daily_sd > 0.0,
            "invalid_stress",
        )?;
        let mut variants: Vec<V> = Vec::new();
        for national_margin in p["nationalMarginScenarios"].as_array().cloned().unwrap_or_default() {
            for design_effect in p["designEffects"].as_array().cloned().unwrap_or_default() {
                for undecided_r in p["undecidedRepublicanFractions"].as_array().cloned().unwrap_or_default() {
                    for bias in p["pollMarginBiasScenarios"].as_array().cloned().unwrap_or_default() {
                        let national_margin = js_number(Some(&national_margin));
                        let design_effect = js_number(Some(&design_effect));
                        let undecided_r = js_number(Some(&undecided_r));
                        let bias = js_number(Some(&bias));
                        // Rounded shares may be adverse by half a displayed percentage point each.
                        let final_r = r - rounding_half + undecided * undecided_r;
                        let final_d = d + rounding_half + undecided * (1.0 - undecided_r);
                        let opposing = js_max(final_d, independent + undecided * (1.0 - undecided_r));
                        let poll_margin = (final_r - opposing) / (final_r + opposing) + bias;
                        let mean = prior_weight * (prior_margin + national_margin)
                            + (1.0 - prior_weight) * poll_margin;
                        let poll_sd = (sampling_variance * design_effect).sqrt().hypot(systematic_sd);
                        let correlated_pool_sd = prior_weight * prior_sd + (1.0 - prior_weight) * poll_sd;
                        let standard_deviation = correlated_pool_sd.hypot(daily_sd * days_to_election.sqrt());
                        let ordinary_probability = probability_from_margin(mean, standard_deviation)?;
                        variants.push(json!({
                            "nationalMargin": num(national_margin),
                            "designEffect": num(design_effect),
                            "undecidedR": num(undecided_r),
                            "bias": num(bias),
                            "mean": num(mean),
                            "standardDeviation": num(standard_deviation),
                            "ordinaryProbability": num(ordinary_probability),
                            "contractProbability": num((ordinary_probability - haircut).max(0.0)),
                        }));
                    }
                }
            }
        }
        require_that(!variants.is_empty(), "empty_scenarios")?;
        let lower = variants
            .iter()
            .map(|v| js_number(v.get("contractProbability")))
            .fold(f64::INFINITY, js_min);
        let upper = variants
            .iter()
            .map(|v| js_number(v.get("contractProbability")))
            .fold(f64::NEG_INFINITY, js_max);
        Ok(json!({"lower": num(lower), "upper": num(upper), "variants": variants}))
    };
    let ordinary = family(
        js_number(p.get("systematicMarginSd")),
        js_number(p.get("dailyMarginInnovationSd")),
    )?;
    let stress = family(
        js_number(p["stress"].get("systematicMarginSd")),
        js_number(p["stress"].get("dailyMarginInnovationSd")),
    )?;
    Ok(json!({
        "schemaVersion": 1,
        "modelId": p["id"],
        "method": "subjective_range",
        "assessedAt": assessment_at,
        "expiresAt": iso(now + js_number(p.get("maximumAssessmentAgeMs"))),
        "componentClocks": {
            "pollObservedAt": poll["observedAt"],
            "pollFieldEndLatestAt": poll["fieldEndLatestAt"],
            "pollFieldAgeDays": num((now - end) / DAY),
            "daysFromFieldEndToElection": num(days_to_election),
            "electionAt": election_at,
            "priorTrainedThroughYear": prior["trainedThroughYear"],
        },
        "sampleSize": poll["sampleSize"],
        "samplingVariance": num(sampling_variance),
        "priorSd": num(prior_sd),
        "ordinary": ordinary,
        "stress": stress,
        "profitabilityEstablished": false,
        "limitations": p["limitations"],
    }))
}

// Diagnostics use exact ticks and the same economic hurdle as production.
// They convey no account permission and do not bypass native admission.
pub fn price_decision(args: &V) -> Result<V, String> {
    let assessment = &args["assessment"];
    let book = &args["book"];
    let policy = &args["policy"];
    let now = date_parse(&field_s(args, "asOf"));
    let at = date_parse(&field_s(assessment, "assessedAt"));
    let received = date_parse(&field_s(book, "receivedAt"));
    let started = date_parse(&field_s(book, "requestStartedAt"));
    let max_book_age = js_number(policy.get("maximumBookAgeMs"));
    if ![now, at, received, started].iter().all(|x| x.is_finite())
        || now < received
        || received < started
        || started < at
        || now - received > max_book_age
        || received - started > max_book_age
    {
        return Ok(json!({"eligible": false, "reason": "book_not_causal_or_fresh"}));
    }
    if now >= date_parse(&field_s(assessment, "expiresAt")) {
        return Ok(json!({"eligible": false, "reason": "assessment_expired"}));
    }
    let asks = book["asks"].as_array();
    let bids = book["bids"].as_array();
    if !asks.is_some_and(|a| !a.is_empty()) || !bids.is_some_and(|b| !b.is_empty()) {
        return Ok(json!({"eligible": false, "reason": "missing_depth"}));
    }
    let asks = asks.unwrap();
    let bids = bids.unwrap();
    let ask = decimal_micros(&asks[0]["price"], "exact")?;
    let bid = decimal_micros(&bids[0]["price"], "exact")?;
    let slip = (js_number(policy.get("slippageTicks")) as i128) * 5000;
    require_that(ask > bid && ask < 1_000_000 && bid > 0, "invalid_book")?;
    let lower = (js_number(assessment["ordinary"].get("lower")) * 1_000_000.0).floor() as i128;
    let stress_lower = (js_number(assessment["stress"].get("lower")) * 1_000_000.0).floor() as i128;
    let maximum_ask = lower - decimal_micros(policy.get("minimumEdge").unwrap_or(&V::Null), "exact")? - slip;
    let stressed_edge = stress_lower - ask - slip;
    let edge = lower - ask - slip;
    let available = (js_number(asks[0].get("quantity")) * js_number(policy.get("depthFractionBps")) / 10000.0).floor();
    let quantity = js_min(
        available,
        (decimal_micros(policy.get("maxOrderCost").unwrap_or(&V::Null), "exact")? / (ask + slip)) as f64,
    );
    let spread = ask - bid;
    let reason = if spread > decimal_micros(policy.get("maximumSpread").unwrap_or(&V::Null), "exact")? {
        "spread"
    } else if edge < decimal_micros(policy.get("minimumEdge").unwrap_or(&V::Null), "exact")? {
        "insufficient_edge"
    } else if quantity < 1.0 {
        "insufficient_depth_or_order_budget"
    } else {
        "conditional_candidate"
    };
    Ok(json!({
        "eligible": reason == "conditional_candidate",
        "reason": reason,
        "lower": fmt_micros(lower)?,
        "stressLower": fmt_micros(stress_lower)?,
        "ask": fmt_micros(ask)?,
        "bid": fmt_micros(bid)?,
        "slippage": fmt_micros(slip)?,
        "edge": signed_micros(edge)?,
        "stressedEdge": signed_micros(stressed_edge)?,
        "maximumAsk": signed_micros(maximum_ask)?,
        "quantity": num(quantity),
        "maximumPrincipal": fmt_micros((ask + slip) * quantity as i128)?,
        "fees": "0.000000",
        "executionAuthority": false,
        "profitabilityEstablished": false,
    }))
}

fn field_s(v: &V, key: &str) -> String {
    crate::js::js_string(v.get(key).unwrap_or(&V::Null))
}
