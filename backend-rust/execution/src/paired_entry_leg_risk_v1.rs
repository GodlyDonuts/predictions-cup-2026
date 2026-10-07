// execution/paired-entry-leg-risk-v1.mjs — compare both sequencing choices
// under full funding. This is a scenario calculation, not an estimated hedge
// probability or execution permission.
//
// Port notes:
// - JS BigInt micro arithmetic -> i128 (caps fit far below i128 bounds).
// - Destructured defaults fire only on absent keys; an explicit null is a
//   value (amount/price/js-int reject it exactly as in JS).
// - Scenarios sort comparator is `a>b ? -1 : 1` (strictly-greater first,
//   no tie branch), matching the JS expression verbatim.
use baal_service_runtime::fixed::{amount, money, price, SCALE, TICK};
use baal_service_runtime::json_text::js_int;
use serde_json::{json, Map, Value as V};

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}

/// Destructured default: absent -> default; present -> the value as &str
/// (non-strings take the \0 sentinel so amount() rejects, as JS does).
fn opt_str<'a>(input: &'a V, k: &str, default: &'a str) -> &'a str {
    match input.get(k) {
        None => default,
        Some(x) => x.as_str().unwrap_or("\u{0}"),
    }
}

fn opt_i64(input: &V, k: &str, default: i64) -> Option<i64> {
    match input.get(k) {
        None => Some(default),
        Some(x) => js_int(x),
    }
}

/// comparePairedEntryLegRiskV1({legs,quantity,maximumOrderCost='25.000000',
///   exitStressTicks=1})
pub fn compare_paired_entry_leg_risk_v1(input: &V) -> Result<V, String> {
    let legs = get(input, "legs").as_array().cloned().unwrap_or_default();
    let quantity = js_int(get(input, "quantity")).unwrap_or(-1);
    let exit_stress_ticks = opt_i64(input, "exitStressTicks", 1).unwrap_or(-1);
    if !(legs.len() == 2
        && get(&legs[0], "exchangeId") != get(&legs[1], "exchangeId")
        && quantity >= 1
        && quantity <= 100
        && exit_stress_ticks >= 0
        && exit_stress_ticks <= 10)
    {
        return Err("PAIR_LEG_RISK_INPUT".to_string());
    }
    let cap = amount(opt_str(input, "maximumOrderCost", "25.000000"))?;
    if !(cap > 0 && cap <= 25_000_000) {
        return Err("PAIR_LEG_RISK_CAP".to_string());
    }
    let costs: Vec<i128> = legs
        .iter()
        .map(|l| {
            price(get(l, "noBuyLimit").as_str().unwrap_or("\u{0}"))
                .map(|p| p * quantity as i128)
        })
        .collect::<Result<_, _>>()?;
    if costs.iter().any(|c| *c > cap) {
        return Err("PAIR_LEG_RISK_NOT_FUNDED".to_string());
    }
    let mut scenarios: Vec<V> = Vec::new();
    for (i, first) in legs.iter().enumerate() {
        let other = &legs[1 - i];
        let exit = price(get(first, "noSellLimit").as_str().unwrap_or("\u{0}"))?
            - exit_stress_ticks as i128 * TICK;
        if exit < TICK {
            return Err("PAIR_LEG_RISK_EXIT_LIMIT".to_string());
        }
        let loss = costs[i] - exit * quantity as i128;
        let surplus = SCALE * quantity as i128 - costs[0] - costs[1];
        let breakeven = if surplus > 0 && loss > 0 {
            json!(loss as f64 / (loss + surplus) as f64)
        } else if surplus > 0 {
            json!(0)
        } else {
            V::Null
        };
        scenarios.push(json!({
            "firstExchangeId": get(first, "exchangeId"),
            "hedgeExchangeId": get(other, "exchangeId"),
            "firstCost": money(costs[i]),
            "hedgeCost": money(costs[1 - i]),
            "fullFunding": money(costs[0] + costs[1]),
            "conditionalOrdinarySurplusIfBothFill": money(surplus),
            "stressedFirstLegLiquidationPnlIfHedgeFails": money(-loss),
            "minimumHedgeCompletionProbabilityForScenarioBreakEven": breakeven,
            "probabilityEstimated": false,
            "unconditionalSettlementGuarantee": false,
        }));
    }
    let mut ranked = scenarios.clone();
    ranked.sort_by(|a, b| {
        let av = amount(get(a, "stressedFirstLegLiquidationPnlIfHedgeFails").as_str().unwrap_or(""))
            .unwrap_or(i128::MIN);
        let bv = amount(get(b, "stressedFirstLegLiquidationPnlIfHedgeFails").as_str().unwrap_or(""))
            .unwrap_or(i128::MIN);
        if av > bv {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Greater
        }
    });
    let mut out = Map::new();
    out.insert("schemaVersion".into(), json!(1));
    out.insert("kind".into(), json!("paired_entry_sequencing_scenario_v1"));
    out.insert("quantity".into(), json!(quantity));
    out.insert("scenarios".into(), V::Array(scenarios));
    out.insert(
        "preferLowerUnhedgedLossExchangeId".into(),
        get(&ranked[0], "firstExchangeId").clone(),
    );
    out.insert("executionAuthority".into(), json!(false));
    Ok(V::Object(out))
}
