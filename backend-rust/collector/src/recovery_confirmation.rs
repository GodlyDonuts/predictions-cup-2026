// live-paper/recovery-confirmation.mjs — verified recovery point-observation
// accumulator and dip->rebound confirmation gate (windowMs 180000, maxGap 90000).
use baal_paper::fixed::{big_amount, instant, big_sequence, TICK};
use baal_service_runtime::contract::instant_ns;
use baal_service_runtime::json_text::js_truthy;
use num_bigint::BigInt;
use serde_json::{json, Value as V};

/// RECOVERY_CONFIRMATION_POLICY (frozen object).
pub fn recovery_confirmation_policy() -> V {
    json!({"windowMs":180000,"minimumObservations":4,"minimumSpacingMs":1000,
        "maximumGapMs":90000,"minimumDropTicks":2,"minimumReboundTicks":1,
        "minimumAfterTrough":2})
}
fn f64_of_big(v: &BigInt) -> f64 { v.to_string().parse().unwrap_or(f64::NAN) }

// These are point observations across verified recoveries, not a complete tape.
// A temporary input invalidation does not retroactively falsify an earlier point.
/// observeRecovery(points, event, policy)
pub fn observe_recovery(points: &V, event: &V, policy: &V) -> Result<V, String> {
    let book = &event["book"];
    let at = instant(event["quoteReceivedAt"].as_str().unwrap_or(""))?;
    let list: Vec<V> = points.as_array().cloned().unwrap_or_default();
    let prior = list.last().cloned();
    if !js_truthy(event.get("decisionEligible"))
        || !js_truthy(event["continuity"].get("ready"))
        || !js_truthy(event["continuity"].get("continuous"))
    {
        return Ok(points.clone());
    }
    if let Some(p) = &prior {
        if big_sequence(book["sequence"].as_str().unwrap_or(""))?
            <= big_sequence(p["sequence"].as_str().unwrap_or(""))?
            || instant_ns(book["sourceAt"].as_str().unwrap_or("")).map_err(|e| e.0)?
                <= instant_ns(p["sourceAt"].as_str().unwrap_or("")).map_err(|e| e.0)?
        {
            return Ok(points.clone());
        }
        if at <= instant(p["receivedAt"].as_str().unwrap_or(""))? {
            return Ok(points.clone());
        }
    }
    let next = json!({"receivedAt":event["quoteReceivedAt"],"confirmedAt":event["confirmedAt"],
        "availableAt":event["availableAt"],"sourceAt":book["sourceAt"],
        "sequence":book["sequence"],"bid":book["bids"][0]["price"],"ask":book["asks"][0]["price"]});
    let mut out: Vec<V> = list;
    out.push(next);
    let window_ms = policy["windowMs"].as_i64().unwrap_or(0);
    let mut kept: Vec<V> = Vec::new();
    for x in out {
        if instant(x["receivedAt"].as_str().unwrap_or(""))? >= at - window_ms {
            kept.push(x);
        }
    }
    if kept.len() > 256 { kept = kept[kept.len() - 256..].to_vec(); }
    Ok(json!(kept))
}

/// recoveryConfirmation(points, event, at, policy)
pub fn recovery_confirmation(points: &V, event: &V, at: &str, policy: &V) -> Result<V, String> {
    let now = instant(at)?;
    let current = &event["book"];
    let window_ms = policy["windowMs"].as_i64().unwrap_or(0);
    let mut eligible: Vec<V> = Vec::new();
    for x in points.as_array().cloned().unwrap_or_default() {
        if instant(x["receivedAt"].as_str().unwrap_or(""))? <= now
            && instant(x["confirmedAt"].as_str().unwrap_or(""))? <= now
            && instant(x["availableAt"].as_str().unwrap_or(""))? <= now
            && instant(x["receivedAt"].as_str().unwrap_or(""))? >= now - window_ms
        {
            eligible.push(x);
        }
    }
    // Collapse receipts that are too close; use the first observation in each
    // spacing interval so burst intensity cannot manufacture confirmation.
    let minimum_spacing = policy["minimumSpacingMs"].as_i64().unwrap_or(0);
    let mut a: Vec<V> = Vec::new();
    for x in &eligible {
        if a.is_empty()
            || instant(x["receivedAt"].as_str().unwrap_or(""))?
                - instant(a.last().unwrap()["receivedAt"].as_str().unwrap_or(""))?
                >= minimum_spacing
        {
            a.push(x.clone());
        }
    }
    let reject = |reason: &str, n: usize| -> V {
        json!({"admitted":false,"reason":reason,"observations":n})
    };
    if (a.len() as i64) < policy["minimumObservations"].as_i64().unwrap_or(0) {
        return Ok(reject("recovery_insufficient_observations", a.len()));
    }
    let last = a.last().unwrap();
    if last["sequence"] != current["sequence"] || last["sourceAt"] != current["sourceAt"] {
        return Ok(reject("recovery_current_not_independent", a.len()));
    }
    let maximum_gap = policy["maximumGapMs"].as_i64().unwrap_or(0);
    for i in 1..a.len() {
        if instant(a[i]["receivedAt"].as_str().unwrap_or(""))?
            - instant(a[i - 1]["receivedAt"].as_str().unwrap_or(""))?
            > maximum_gap
        {
            return Ok(reject("recovery_observation_gap", a.len()));
        }
    }
    let mut trough = 0usize;
    for i in 1..a.len() {
        if big_amount(a[i]["bid"].as_str().unwrap_or(""))?
            < big_amount(a[trough]["bid"].as_str().unwrap_or(""))?
        {
            trough = i;
        }
    }
    if trough == 0 {
        return Ok(reject("recovery_no_prior_drop", a.len()));
    }
    if ((a.len() - 1 - trough) as i64)
        < policy["minimumAfterTrough"].as_i64().unwrap_or(0)
    {
        return Ok(reject("recovery_insufficient_after_trough", a.len()));
    }
    let low = big_amount(a[trough]["bid"].as_str().unwrap_or(""))?;
    let mut peak = BigInt::from(0);
    for x in &a[..trough] {
        let v = big_amount(x["bid"].as_str().unwrap_or(""))?;
        if v > peak { peak = v; }
    }
    let bid = big_amount(last["bid"].as_str().unwrap_or(""))?;
    let tick = BigInt::from(TICK);
    if &peak - &low < &tick * BigInt::from(policy["minimumDropTicks"].as_i64().unwrap_or(0)) {
        return Ok(reject("recovery_drop_too_small", a.len()));
    }
    if &bid - &low < &tick * BigInt::from(policy["minimumReboundTicks"].as_i64().unwrap_or(0)) {
        return Ok(reject("recovery_rebound_too_small", a.len()));
    }
    if a.len() < 2 {
        return Err("Money must be a canonical decimal string with at most six places".into());
    }
    if big_amount(a[a.len() - 2]["bid"].as_str().unwrap_or(""))? > bid {
        return Ok(reject("recovery_bid_falling", a.len()));
    }
    Ok(json!({"admitted":true,"reason":"recovery_confirmed","observations":a.len(),
        "troughReceivedAt":a[trough]["receivedAt"],
        "dropTicks":f64_of_big(&((&peak - &low) / &tick)),
        "reboundTicks":f64_of_big(&((&bid - &low) / &tick)),
        "observedSpanMs":instant(last["receivedAt"].as_str().unwrap_or(""))?
            - instant(a[0]["receivedAt"].as_str().unwrap_or(""))?,
        "evidence":"discrete_verified_bid_observations_not_continuous_tape_or_forecast"}))
}
