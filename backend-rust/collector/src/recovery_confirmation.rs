// live-paper/recovery-confirmation.mjs — discrete verified bid observations.
use baal_paper::fixed::{big_amount, big_sequence, instant, TICK};
use baal_service_runtime::contract::instant_ns;
use baal_service_runtime::json_text::{js_int, js_truthy};
use num_bigint::BigInt;
use serde_json::{json, Value as V};

/// RECOVERY_CONFIRMATION_POLICY (frozen object).
pub fn recovery_confirmation_policy() -> V {
    json!({"windowMs":180000,"minimumObservations":4,"minimumSpacingMs":1000,
        "maximumGapMs":90000,"minimumDropTicks":2,"minimumReboundTicks":1,
        "minimumAfterTrough":2})
}
fn truthy(v: &V) -> bool { js_truthy(Some(v)) }
// These are point observations across verified recoveries, not a complete tape.
// A temporary input invalidation does not retroactively falsify an earlier point.
/// observeRecovery(points,event,policy)
pub fn observe_recovery(points: &V, event: &V, policy: &V) -> Result<V, String> {
    let book = &event["book"];
    let at = instant(event["quoteReceivedAt"].as_str().unwrap_or(""))?;
    let empty = Vec::new();
    let pts = points.as_array().unwrap_or(&empty);
    let prior = pts.last();
    if !truthy(&event["decisionEligible"])
        || !truthy(&event["continuity"]["ready"]) || !truthy(&event["continuity"]["continuous"]) {
        return Ok(points.clone());
    }
    if let Some(p) = prior {
        if big_sequence(book["sequence"].as_str().unwrap_or(""))?
            <= big_sequence(p["sequence"].as_str().unwrap_or(""))?
            || instant_ns(book["sourceAt"].as_str().unwrap_or("")).map_err(|e| e.to_string())?
                <= instant_ns(p["sourceAt"].as_str().unwrap_or("")).map_err(|e| e.to_string())? {
            return Ok(points.clone());
        }
        if at <= instant(p["receivedAt"].as_str().unwrap_or(""))? {
            return Ok(points.clone());
        }
    }
    let next = json!({"receivedAt":event["quoteReceivedAt"],"confirmedAt":event["confirmedAt"],
        "availableAt":event["availableAt"],"sourceAt":book["sourceAt"],"sequence":book["sequence"],
        "bid":book["bids"][0]["price"],"ask":book["asks"][0]["price"]});
    let window_ms = js_int(&policy["windowMs"]).unwrap_or(0);
    let mut out: Vec<V> = pts.clone();
    out.push(next);
    let mut kept: Vec<V> = out
        .into_iter()
        .filter(|x| instant(x["receivedAt"].as_str().unwrap_or("")).map(|t| t >= at - window_ms).unwrap_or(false))
        .collect();
    if kept.len() > 256 {
        kept = kept[kept.len() - 256..].to_vec();
    }
    Ok(V::Array(kept))
}
/// recoveryConfirmation(points,event,at,policy)
pub fn recovery_confirmation(points: &V, event: &V, at: &str, policy: &V) -> Result<V, String> {
    let now = instant(at)?;
    let current = &event["book"];
    let (window_ms, minimum_observations, minimum_spacing_ms, maximum_gap_ms, minimum_drop_ticks,
        minimum_rebound_ticks, minimum_after_trough) = (
        js_int(&policy["windowMs"]).unwrap_or(0),
        js_int(&policy["minimumObservations"]).unwrap_or(0),
        js_int(&policy["minimumSpacingMs"]).unwrap_or(0),
        js_int(&policy["maximumGapMs"]).unwrap_or(0),
        js_int(&policy["minimumDropTicks"]).unwrap_or(0),
        js_int(&policy["minimumReboundTicks"]).unwrap_or(0),
        js_int(&policy["minimumAfterTrough"]).unwrap_or(0),
    );
    let empty = Vec::new();
    let pts = points.as_array().unwrap_or(&empty);
    let eligible: Vec<&V> = pts
        .iter()
        .filter(|x| {
            let r = instant(x["receivedAt"].as_str().unwrap_or("")).unwrap_or(i64::MIN);
            let c = instant(x["confirmedAt"].as_str().unwrap_or("")).unwrap_or(i64::MIN);
            let a = instant(x["availableAt"].as_str().unwrap_or("")).unwrap_or(i64::MIN);
            r <= now && c <= now && a <= now && r >= now - window_ms
        })
        .collect();
    // Collapse receipts that are too close; use the first observation in each
    // spacing interval so burst intensity cannot manufacture confirmation.
    let mut a: Vec<&V> = Vec::new();
    for x in eligible {
        let last_ok = match a.last() {
            None => true,
            Some(prev) => {
                instant(x["receivedAt"].as_str().unwrap_or(""))?
                    - instant(prev["receivedAt"].as_str().unwrap_or(""))?
                    >= minimum_spacing_ms
            }
        };
        if last_ok {
            a.push(x);
        }
    }
    let reject = |reason: &str| -> V {
        json!({"admitted":false,"reason":reason,"observations":a.len()})
    };
    if (a.len() as i64) < minimum_observations {
        return Ok(reject("recovery_insufficient_observations"));
    }
    let last = a[a.len() - 1];
    if last["sequence"] != current["sequence"] || last["sourceAt"] != current["sourceAt"] {
        return Ok(reject("recovery_current_not_independent"));
    }
    for i in 1..a.len() {
        if instant(a[i]["receivedAt"].as_str().unwrap_or(""))?
            - instant(a[i - 1]["receivedAt"].as_str().unwrap_or(""))?
            > maximum_gap_ms {
            return Ok(reject("recovery_observation_gap"));
        }
    }
    let mut trough = 0usize;
    for i in 1..a.len() {
        if big_amount(a[i]["bid"].as_str().unwrap_or(""))?
            < big_amount(a[trough]["bid"].as_str().unwrap_or(""))? {
            trough = i;
        }
    }
    if trough == 0 {
        return Ok(reject("recovery_no_prior_drop"));
    }
    if ((a.len() - 1 - trough) as i64) < minimum_after_trough {
        return Ok(reject("recovery_insufficient_after_trough"));
    }
    let low = big_amount(a[trough]["bid"].as_str().unwrap_or(""))?;
    let mut peak = BigInt::from(0);
    for x in &a[..trough] {
        let b = big_amount(x["bid"].as_str().unwrap_or(""))?;
        if b > peak {
            peak = b;
        }
    }
    let bid = big_amount(last["bid"].as_str().unwrap_or(""))?;
    let tick = BigInt::from(TICK);
    if &peak - &low < &tick * BigInt::from(minimum_drop_ticks) {
        return Ok(reject("recovery_drop_too_small"));
    }
    if &bid - &low < &tick * BigInt::from(minimum_rebound_ticks) {
        return Ok(reject("recovery_rebound_too_small"));
    }
    if a.len() < 2 {
        // JS: a.at(-2).bid on undefined — TypeError.
        return Err("Cannot read properties of undefined (reading 'bid')".into());
    }
    if big_amount(a[a.len() - 2]["bid"].as_str().unwrap_or(""))? > bid {
        return Ok(reject("recovery_bid_falling"));
    }
    Ok(json!({"admitted":true,"reason":"recovery_confirmed","observations":a.len(),
        "troughReceivedAt":a[trough]["receivedAt"],
        "dropTicks":((&peak - &low) / &tick).to_string().parse::<i64>().unwrap_or(0),
        "reboundTicks":((&bid - &low) / &tick).to_string().parse::<i64>().unwrap_or(0),
        "observedSpanMs":instant(last["receivedAt"].as_str().unwrap_or(""))?
            - instant(a[0]["receivedAt"].as_str().unwrap_or(""))?,
        "evidence":"discrete_verified_bid_observations_not_continuous_tape_or_forecast"}))
}
