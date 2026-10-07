// execution/paired-entry-leg-risk-v1.mjs — scenario comparison under full
// funding. Ported into baal-collector (batch collector-w13): the .mjs lives in
// execution/ but is not yet ported into baal-execution, and another crate's
// lib.rs cannot be committed by this batch. Semantics identical.
// This is a scenario calculation, not an estimated hedge probability or
// execution permission.
use baal_paper::fixed::{big_amount, big_price, money, SCALE, TICK};
use baal_service_runtime::json_text::js_int;
use num_bigint::BigInt;
use serde_json::{json, Map, Value as V};

// JS Number(bigint): correctly rounded nearest f64, ±Infinity on overflow.
fn bigint_f64(x: &BigInt) -> f64 {
    x.to_string().parse::<f64>().unwrap_or(if x.sign() == num_bigint::Sign::Minus {
        f64::NEG_INFINITY
    } else {
        f64::INFINITY
    })
}

/// comparePairedEntryLegRiskV1({legs,quantity,maximumOrderCost='25.000000',exitStressTicks=1})
pub fn compare_paired_entry_leg_risk_v1(input: &V) -> Result<V, String> {
    let legs = input.get("legs").and_then(V::as_array).cloned().unwrap_or_default();
    let quantity = js_int(input.get("quantity").unwrap_or(&V::Null));
    // JS destructuring defaults apply only to undefined: a present non-string
    // is passed to amount(), which throws the canonical money message.
    let maximum_order_cost = match input.get("maximumOrderCost") {
        None => "25.000000".to_string(),
        Some(v) => v.as_str().unwrap_or("").to_string(),
    };
    let exit_stress_ticks = match input.get("exitStressTicks") {
        None => Some(1i64),
        Some(v) => js_int(v),
    };
    if legs.len() != 2
        || legs[0].get("exchangeId") == legs[1].get("exchangeId")
        || !quantity.is_some_and(|q| (1..=100).contains(&q))
        || !exit_stress_ticks.is_some_and(|t| (0..=10).contains(&t))
    {
        return Err("PAIR_LEG_RISK_INPUT".into());
    }
    let quantity = quantity.unwrap();
    let exit_stress_ticks = exit_stress_ticks.unwrap();
    let cap = big_amount(&maximum_order_cost)?;
    if cap <= BigInt::from(0) || cap > BigInt::from(25_000_000) {
        return Err("PAIR_LEG_RISK_CAP".into());
    }
    let qty = BigInt::from(quantity);
    let scale = BigInt::from(SCALE);
    let tick = BigInt::from(TICK);
    let mut costs = Vec::with_capacity(2);
    for l in &legs {
        costs.push(big_price(l.get("noBuyLimit").and_then(V::as_str).unwrap_or(""))? * &qty);
    }
    if costs.iter().any(|c| c > &cap) {
        return Err("PAIR_LEG_RISK_NOT_FUNDED".into());
    }
    let mut scenarios = Vec::with_capacity(2);
    for i in 0..2 {
        let first = &legs[i];
        let other = &legs[1 - i];
        let exit = big_price(first.get("noSellLimit").and_then(V::as_str).unwrap_or(""))?
            - BigInt::from(exit_stress_ticks) * &tick;
        if exit < tick {
            return Err("PAIR_LEG_RISK_EXIT_LIMIT".into());
        }
        let loss = &costs[i] - &exit * &qty;
        let surplus = &scale * &qty - &costs[0] - &costs[1];
        let probability = if surplus > BigInt::from(0) && loss > BigInt::from(0) {
            json!(bigint_f64(&loss) / bigint_f64(&(&loss + &surplus)))
        } else if surplus > BigInt::from(0) {
            json!(0)
        } else {
            V::Null
        };
        let mut s = Map::new();
        s.insert("firstExchangeId".into(), first.get("exchangeId").cloned().unwrap_or(V::Null));
        s.insert("hedgeExchangeId".into(), other.get("exchangeId").cloned().unwrap_or(V::Null));
        s.insert("firstCost".into(), json!(money(&costs[i])));
        s.insert("hedgeCost".into(), json!(money(&costs[1 - i])));
        s.insert("fullFunding".into(), json!(money(&(&costs[0] + &costs[1]))));
        s.insert("conditionalOrdinarySurplusIfBothFill".into(), json!(money(&surplus)));
        s.insert("stressedFirstLegLiquidationPnlIfHedgeFails".into(), json!(money(&(-&loss))));
        s.insert("minimumHedgeCompletionProbabilityForScenarioBreakEven".into(), probability);
        s.insert("probabilityEstimated".into(), json!(false));
        s.insert("unconditionalSettlementGuarantee".into(), json!(false));
        scenarios.push(V::Object(s));
    }
    // JS sort with cmp(a,b)=amount(a)>amount(b)?-1:1: descending by stressed
    // PnL; a tie leaves the earlier scenario first, matching a stable sort.
    let mut ranked: Vec<&V> = scenarios.iter().collect();
    ranked.sort_by(|a, b| {
        let av = big_amount(a["stressedFirstLegLiquidationPnlIfHedgeFails"].as_str().unwrap_or(""))
            .unwrap_or_default();
        let bv = big_amount(b["stressedFirstLegLiquidationPnlIfHedgeFails"].as_str().unwrap_or(""))
            .unwrap_or_default();
        bv.cmp(&av)
    });
    let prefer = ranked[0]["firstExchangeId"].clone();
    let mut out = Map::new();
    out.insert("schemaVersion".into(), json!(1));
    out.insert("kind".into(), json!("paired_entry_sequencing_scenario_v1"));
    out.insert("quantity".into(), json!(quantity));
    out.insert("scenarios".into(), V::Array(scenarios));
    out.insert("preferLowerUnhedgedLossExchangeId".into(), prefer);
    out.insert("executionAuthority".into(), json!(false));
    Ok(V::Object(out))
}
