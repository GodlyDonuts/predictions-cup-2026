// live-paper/recovery-confirmation.mjs — verified point observations across recoveries.
use baal_paper::fixed::{big_amount, big_sequence, instant, TICK};
use baal_service_runtime::contract::instant_ns;
use num_bigint::BigInt;
use serde_json::{json, Value as V};

pub fn recovery_confirmation_policy() -> V {
    // Object.freeze({...}) — callers pass &V so mutation is impossible.
    json!({"windowMs":180000,"minimumObservations":4,"minimumSpacingMs":1000,"maximumGapMs":90000,
        "minimumDropTicks":2,"minimumReboundTicks":1,"minimumAfterTrough":2})
}

fn amount(v: &V) -> Result<BigInt, String> { big_amount(v.as_str().unwrap_or("")) }
fn seq(v: &V) -> Result<BigInt, String> { big_sequence(v.as_str().unwrap_or("")) }
fn ins(v: &V) -> Result<i64, String> { instant(v.as_str().unwrap_or("")) }

// These are point observations across verified recoveries, not a complete tape.
// A temporary input invalidation does not retroactively falsify an earlier point.
pub fn observe_recovery(points: &[V], event: &V, policy: &V) -> Result<Vec<V>, String> {
    let book = &event["book"];
    let at = ins(&event["quoteReceivedAt"])?;
    let prior = points.last();
    if !event["decisionEligible"].as_bool().unwrap_or(false)
        || !event["continuity"]["ready"].as_bool().unwrap_or(false)
        || !event["continuity"]["continuous"].as_bool().unwrap_or(false)
    {
        return Ok(points.to_vec());
    }
    if let Some(prior) = prior {
        if seq(&book["sequence"])? <= seq(&prior["sequence"])?
            || instant_ns(book["sourceAt"].as_str().unwrap_or("")).map_err(|e| e.0)?
                <= instant_ns(prior["sourceAt"].as_str().unwrap_or("")).map_err(|e| e.0)?
        {
            return Ok(points.to_vec());
        }
        if at <= ins(&prior["receivedAt"])? {
            return Ok(points.to_vec());
        }
    }
    let next = json!({"receivedAt":event["quoteReceivedAt"],"confirmedAt":event["confirmedAt"],
        "availableAt":event["availableAt"],"sourceAt":book["sourceAt"],"sequence":book["sequence"],
        "bid":book["bids"][0]["price"],"ask":book["asks"][0]["price"]});
    let window_ms = policy["windowMs"].as_i64().unwrap_or(0);
    let mut out: Vec<V> = points.iter().cloned().chain(std::iter::once(next)).collect();
    out.retain(|x| ins(&x["receivedAt"]).map(|t| t >= at - window_ms).unwrap_or(false));
    if out.len() > 256 {
        out = out[out.len() - 256..].to_vec();
    }
    Ok(out)
}

pub fn recovery_confirmation(
    points: &[V],
    event: &V,
    at: &str,
    policy: &V,
) -> Result<V, String> {
    let now = instant(at)?;
    let current = &event["book"];
    let window_ms = policy["windowMs"].as_i64().unwrap_or(0);
    let eligible: Vec<&V> = points
        .iter()
        .filter(|x| {
            ins(&x["receivedAt"]).map(|t| t <= now).unwrap_or(false)
                && ins(&x["confirmedAt"]).map(|t| t <= now).unwrap_or(false)
                && ins(&x["availableAt"]).map(|t| t <= now).unwrap_or(false)
                && ins(&x["receivedAt"]).map(|t| t >= now - window_ms).unwrap_or(false)
        })
        .collect();
    // Collapse receipts that are too close; use the first observation in each
    // spacing interval so burst intensity cannot manufacture confirmation.
    let spacing = policy["minimumSpacingMs"].as_i64().unwrap_or(0);
    let mut a: Vec<V> = Vec::new();
    for x in &eligible {
        if a.is_empty()
            || ins(&x["receivedAt"])? - ins(&a.last().unwrap()["receivedAt"])? >= spacing
        {
            a.push((*x).clone());
        }
    }
    let observations = a.len();
    let reject = |reason: &str| -> V {
        json!({"admitted":false,"reason":reason,"observations":observations})
    };
    if a.len() < policy["minimumObservations"].as_u64().unwrap_or(0) as usize {
        return Ok(reject("recovery_insufficient_observations"));
    }
    let last = a.last().unwrap();
    if last["sequence"] != current["sequence"] || last["sourceAt"] != current["sourceAt"] {
        return Ok(reject("recovery_current_not_independent"));
    }
    let max_gap = policy["maximumGapMs"].as_i64().unwrap_or(0);
    for i in 1..a.len() {
        if ins(&a[i]["receivedAt"])? - ins(&a[i - 1]["receivedAt"])? > max_gap {
            return Ok(reject("recovery_observation_gap"));
        }
    }
    let mut trough = 0usize;
    for i in 1..a.len() {
        if amount(&a[i]["bid"])? < amount(&a[trough]["bid"])? {
            trough = i;
        }
    }
    if trough == 0 {
        return Ok(reject("recovery_no_prior_drop"));
    }
    if a.len() - 1 - trough < policy["minimumAfterTrough"].as_u64().unwrap_or(0) as usize {
        return Ok(reject("recovery_insufficient_after_trough"));
    }
    let low = amount(&a[trough]["bid"])?;
    let mut peak = BigInt::from(0);
    for x in &a[..trough] {
        let b = amount(&x["bid"])?;
        if b > peak {
            peak = b;
        }
    }
    let bid = amount(&last["bid"])?;
    let tick = BigInt::from(TICK);
    if &peak - &low < &tick * BigInt::from(policy["minimumDropTicks"].as_i64().unwrap_or(0)) {
        return Ok(reject("recovery_drop_too_small"));
    }
    if &bid - &low < &tick * BigInt::from(policy["minimumReboundTicks"].as_i64().unwrap_or(0)) {
        return Ok(reject("recovery_rebound_too_small"));
    }
    if amount(&a[a.len() - 2]["bid"])? > bid {
        return Ok(reject("recovery_bid_falling"));
    }
    let drop_ticks = (&peak - &low) / &tick;
    let rebound_ticks = (&bid - &low) / &tick;
    Ok(json!({"admitted":true,"reason":"recovery_confirmed","observations":a.len(),
        "troughReceivedAt":a[trough]["receivedAt"],
        "dropTicks":drop_ticks.to_string().parse::<u64>().unwrap_or(0),
        "reboundTicks":rebound_ticks.to_string().parse::<u64>().unwrap_or(0),
        "observedSpanMs":ins(&last["receivedAt"])?-ins(&a[0]["receivedAt"])?,
        "evidence":"discrete_verified_bid_observations_not_continuous_tape_or_forecast"}))
}
