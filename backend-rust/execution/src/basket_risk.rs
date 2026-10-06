// execution/basket-risk.mjs — paired-leg basket fill-risk counterfactuals.
// Exact micro-SUSQie integer arithmetic (JS BigInt magnitudes fit i128).
use crate::core::{decimal_micros, format_micros, SCALE, TICK};
use baal_service_runtime::json_text::js_int;
use serde_json::{json, Value as V};

fn check(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(code.to_string()) }
}
// x => Number.isSafeInteger(x) && x >= 0 && x <= 100
fn qty(v: &V) -> bool {
    js_int(v).is_some_and(|x| (0..=100).contains(&x))
}
fn signed(x: i128) -> String {
    if x < 0 { format!("-{}", format_micros(-x)) } else { format_micros(x) }
}
fn limits(values: &V) -> Result<[i128; 2], String> {
    check(values.as_array().is_some_and(|a| a.len() == 2), "INVALID_BASKET_LIMITS")?;
    let mut out = [0i128; 2];
    for (i, v) in values.as_array().unwrap().iter().enumerate() {
        let n = decimal_micros(v, "exact")?;
        check(n >= TICK && n <= SCALE - TICK && n % TICK == 0, "INVALID_BASKET_LIMITS")?;
        out[i] = n;
    }
    Ok(out)
}
/// basketFillRisk(quantities, unitLimits)
pub fn basket_fill_risk(quantities: &V, unit_limits: &V) -> Result<V, String> {
    check(
        quantities.as_array().is_some_and(|a| a.len() == 2 && a.iter().all(qty)),
        "INVALID_BASKET_FILLED_QUANTITIES",
    )?;
    let qs = quantities.as_array().unwrap();
    let q = [js_int(&qs[0]).unwrap() as i128, js_int(&qs[1]).unwrap() as i128];
    let prices = limits(unit_limits)?;
    let costs = [prices[0] * q[0], prices[1] * q[1]];
    let cost = costs[0] + costs[1];
    let statuses: [[&str; 2]; 9] = [
        ["yes", "no"], ["no", "yes"], ["no", "no"],
        ["refund", "yes"], ["yes", "refund"], ["refund", "no"], ["no", "refund"],
        ["refund", "refund"], ["yes", "yes"],
    ];
    let mut scenarios = Vec::new();
    for outcomes in statuses {
        let mut payout = 0i128;
        for (i, s) in outcomes.iter().enumerate() {
            payout += match *s {
                "no" => q[i] * SCALE,
                "refund" => costs[i],
                _ => 0,
            };
        }
        let kind = if outcomes.iter().all(|s| *s != "refund") && outcomes.iter().any(|s| *s == "no") {
            "normal_mutually_exclusive"
        } else if outcomes.contains(&"refund") {
            "refund_exception"
        } else {
            "override_or_predicate_failure"
        };
        scenarios.push(json!({
            "outcomes": outcomes,
            "payoutAtLimits": format_micros(payout),
            "pnlAtLimits": signed(payout - cost),
            "kind": kind,
        }));
    }
    let normal_floor = q[0].min(q[1]) * SCALE;
    Ok(json!({
        "quantities": quantities,
        "unitLimits": unit_limits,
        "maximumCost": format_micros(cost),
        "legCosts": [format_micros(costs[0]), format_micros(costs[1])],
        "conditionalNormalPayoutFloor": format_micros(normal_floor),
        "conditionalNormalPnlFloor": signed(normal_floor - cost),
        "scenarios": scenarios,
        "unconditionalPayoutFloor": "0.000000",
        "unconditionalPnlFloor": signed(-cost),
        "actualFills": false,
        "executionAuthority": false,
    }))
}
/// planBasketRiskPreview(observation, {maxPackageCost, maxUnmatchedLegCost, minimumUnitMargin})
pub fn plan_basket_risk_preview(observation: &V, options: Option<&V>) -> Result<V, String> {
    let get = |k: &str, d: &str| -> V {
        options.and_then(|o| o.get(k)).cloned().unwrap_or_else(|| json!(d))
    };
    let max_package_cost = get("maxPackageCost", "25.000000");
    let max_unmatched_leg_cost = get("maxUnmatchedLegCost", "5.000000");
    let minimum_unit_margin = get("minimumUnitMargin", "0.005000");
    let basket = observation.get("basket").cloned().unwrap_or(V::Null);
    let segments = basket.get("segments").and_then(V::as_array);
    check(
        js_int(basket.get("quantity").unwrap_or(&V::Null)).is_some_and(|x| (1..=100).contains(&x))
            && segments.is_some_and(|s| !s.is_empty() && s.len() <= 200),
        "INVALID_BASKET_OBSERVATION",
    )?;
    let basket_quantity = js_int(basket.get("quantity").unwrap()).unwrap() as i128;
    let segments = segments.unwrap();
    let budget = decimal_micros(&max_package_cost, "exact")?;
    let orphan = decimal_micros(&max_unmatched_leg_cost, "exact")?;
    let margin = decimal_micros(&minimum_unit_margin, "exact")?;
    check(
        budget > 0 && budget <= 25_000_000 && orphan > 0 && orphan <= budget && margin >= TICK && margin < SCALE,
        "INVALID_BASKET_PREVIEW_POLICY",
    )?;
    let mut total = 0i128;
    let mut count = 0i64;
    let mut previous = [0i128, 0i128];
    let mut units: Vec<[i128; 2]> = Vec::new();
    for s in segments {
        check(js_int(s.get("quantity").unwrap_or(&V::Null)).is_some_and(|x| x >= 1 && x <= 100), "INVALID_BASKET_SEGMENT")?;
        let sq = js_int(s.get("quantity").unwrap()).unwrap();
        let ps = limits(s.get("noUnitCostsAfterCushion").unwrap_or(&V::Null))?;
        let unit = ps[0] + ps[1];
        check(
            !(ps[0] < previous[0] || ps[1] < previous[1]) && unit == decimal_micros(s.get("marginalUnitCost").unwrap_or(&V::Null), "exact")?,
            "INVALID_BASKET_SEGMENT_COST",
        )?;
        count += sq;
        total += sq as i128 * unit;
        check(
            count <= 100
                && js_int(s.get("cumulativeQuantity").unwrap_or(&V::Null)) == Some(count)
                && decimal_micros(s.get("cumulativeCost").unwrap_or(&V::Null), "exact")? == total,
            "INVALID_BASKET_SEGMENT_TOTAL",
        )?;
        for _ in 0..sq {
            units.push(ps);
        }
        previous = ps;
    }
    check(
        count as i128 == basket_quantity
            && decimal_micros(basket.get("totalCost").unwrap_or(&V::Null), "exact")? == total
            && decimal_micros(basket.get("conditionalMargin").unwrap_or(&V::Null), "exact")? == count as i128 * SCALE - total,
        "INVALID_BASKET_OBSERVATION_TOTAL",
    )?;
    let mut selected = V::Null;
    for q in 1..=count {
        let ps = units[(q - 1) as usize];
        let worst_unit = ps[0] + ps[1];
        let reservation = q as i128 * worst_unit;
        if reservation > budget || ps.iter().any(|p| q as i128 * p > orphan) || SCALE - worst_unit < margin {
            continue;
        }
        let expected: i128 = units[..q as usize].iter().map(|p| p[0] + p[1]).sum();
        let strings: Vec<String> = ps.iter().map(|p| format_micros(*p)).collect();
        let strings_v = json!(strings);
        selected = json!({
            "quantity": q,
            "hardLimits": strings,
            "observedLadderCost": format_micros(expected),
            "maximumReservationCost": format_micros(reservation),
            "conditionalObservedMargin": format_micros(q as i128 * SCALE - expected),
            "conditionalHardLimitMargin": format_micros(q as i128 * (SCALE - worst_unit)),
            "full": basket_fill_risk(&json!([q, q]), &strings_v)?,
            "onlyFirst": basket_fill_risk(&json!([q, 0]), &strings_v)?,
            "onlySecond": basket_fill_risk(&json!([0, q]), &strings_v)?,
            "unequal": basket_fill_risk(&json!([q, q / 2]), &strings_v)?,
        });
    }
    Ok(json!({
        "schemaVersion": 1,
        "status": if selected.is_null() { "blocked_by_cost_or_depth" } else { "bounded_counterfactual_package" },
        "policy": {"maxPackageCost": max_package_cost, "maxUnmatchedLegCost": max_unmatched_leg_cost, "minimumUnitMargin": minimum_unit_margin},
        "preview": selected,
        "addedFees": "0.000000",
        "executionAuthority": false,
        "ordersSubmitted": 0,
        "actualFills": false,
        "settlementProof": "not_established_by_price_observation",
        "reason": "Hard-limit reservations and unequal-fill/refund scenarios; no executable order or account authority.",
    }))
}
